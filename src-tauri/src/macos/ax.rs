//! Reading the selection through the Accessibility API.
//!
//! This is the preferred way to get a selection, because it reads the text
//! *directly* — nothing is copied, so the pasteboard is never touched, no
//! keystroke is synthesised into whichever app happens to be frontmost, and the
//! app cannot race us by writing the clipboard late. It also gives us the
//! selection's position on screen, which the pasteboard route cannot.
//!
//! What it cannot do is work everywhere. Apps differ in which attributes they
//! implement, so this returns a classified [`Failure`] rather than an
//! `Option`-shaped shrug: the caller can then fall back deliberately, and
//! diagnostics can record which app needed which strategy.

use accessibility_sys::{
    error_string, kAXBoundsForRangeParameterizedAttribute, kAXErrorAPIDisabled,
    kAXErrorAttributeUnsupported, kAXErrorCannotComplete, kAXErrorInvalidUIElement, kAXErrorNoValue,
    kAXErrorParameterizedAttributeUnsupported, kAXFocusedUIElementAttribute,
    kAXSelectedTextAttribute, kAXSelectedTextRangeAttribute, kAXValueTypeCGRect,
    kAXValueTypeCFRange, AXUIElementCopyAttributeValue, AXUIElementCopyParameterizedAttributeValue,
    AXUIElementCreateSystemWide, AXUIElementRef, AXUIElementSetMessagingTimeout, AXValueGetType,
    AXValueGetValue, AXValueRef,
};
use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation_sys::base::{CFIndex, CFRelease, CFTypeRef, CFRange};
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use std::ffi::c_void;

/// How long a single AX call may block before macOS gives up on it.
///
/// An app that is busy can leave an AX call hanging indefinitely, and this runs
/// on a worker thread the translation pipeline is waiting on. Without a bound, a
/// wedged app would wedge the translation too — which is the failure mode
/// `pipeline::PipelineGate` exists to recover from, but better not to create.
const MESSAGING_TIMEOUT_SECONDS: f32 = 2.0;

/// A rectangle in the global display space.
///
/// This is the space Accessibility reports in: origin at the top-left of the
/// primary display, `y` increasing downwards, measured in **points** rather than
/// backing pixels. Points, not pixels, is why the trigger point is positioned
/// with a logical rather than a physical coordinate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ScreenRect {
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// Whether a point falls inside, in the same coordinate space.
    pub fn contains(&self, point: (f64, f64)) -> bool {
        point.0 >= self.x
            && point.0 <= self.right()
            && point.1 >= self.y
            && point.1 <= self.bottom()
    }

    pub(crate) fn from_cg(rect: CGRect) -> Self {
        Self {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Selection {
    pub text: String,
    /// Where the selection is on screen, when the app reports it. Absent is
    /// ordinary — plenty of apps expose the text but not its geometry — and the
    /// caller decides what to do instead rather than guessing.
    pub bounds: Option<ScreenRect>,
}

/// Why an AX read did not produce a selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Accessibility permission has not been granted.
    NotTrusted,
    /// Nothing is focused, so there is nothing to ask.
    NoFocusedElement,
    /// Focused, but nothing is selected (or the selection is blank).
    NoSelection,
    /// The app does not implement the attribute. Ordinary for terminals, some
    /// Electron apps, and anything drawing its own text.
    Unsupported,
    /// The app was busy, or the element went away mid-read. Worth retrying later;
    /// the others are not.
    Blocked { code: &'static str },
    Other { code: &'static str },
}

impl Failure {
    /// A stable label, for grouping in diagnostics.
    pub fn label(&self) -> &'static str {
        match self {
            Self::NotTrusted => "not_trusted",
            Self::NoFocusedElement => "no_focused_element",
            Self::NoSelection => "no_selection",
            Self::Unsupported => "unsupported",
            Self::Blocked { .. } => "blocked",
            Self::Other { .. } => "other",
        }
    }

    /// The AX error name, when there was one.
    pub fn detail(&self) -> Option<&'static str> {
        match self {
            Self::Blocked { code } | Self::Other { code } => Some(code),
            _ => None,
        }
    }
}

/// Map an `AXError` to a [`Failure`]. Pure, so the classification is testable
/// without an app that misbehaves in exactly the right way.
///
/// The pattern names come from the C headers and are spelled the way those
/// headers spell them.
#[allow(non_upper_case_globals)]
fn classify(error: i32) -> Failure {
    // kAXErrorSuccess must not reach here; anything unrecognised is reported with
    // its own name rather than flattened into a generic failure.
    match error {
        kAXErrorAPIDisabled => Failure::NotTrusted,
        kAXErrorNoValue => Failure::NoSelection,
        kAXErrorAttributeUnsupported | kAXErrorParameterizedAttributeUnsupported => {
            Failure::Unsupported
        }
        kAXErrorCannotComplete | kAXErrorInvalidUIElement => {
            Failure::Blocked { code: error_string(error) }
        }
        other => Failure::Other { code: error_string(other) },
    }
}

/// Releases a `+1` CoreFoundation object when it goes out of scope.
///
/// Every `AX...Copy...` call returns one of these, and the AX API has no RAII of
/// its own — the reference implementations in Objective-C rely on ARC, which Rust
/// does not have.
struct OwnedCf(CFTypeRef);

impl Drop for OwnedCf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

unsafe fn cfstring_to_string(value: CFStringRef) -> Option<String> {
    if value.is_null() {
        return None;
    }
    // `wrap_under_get_rule` borrows rather than taking ownership; the caller's
    // `OwnedCf` is what releases it.
    Some(CFString::wrap_under_get_rule(value).to_string())
}

/// Copy one attribute, mapping failure to a [`Failure`].
unsafe fn copy_attribute(element: AXUIElementRef, attribute: &str) -> Result<OwnedCf, Failure> {
    let key = CFString::new(attribute);
    let mut value: CFTypeRef = std::ptr::null();
    let error = AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut value);
    if error != 0 {
        return Err(classify(error));
    }
    if value.is_null() {
        // A success with no value is an empty selection, not an error.
        return Err(Failure::NoSelection);
    }
    Ok(OwnedCf(value))
}

/// Read an `AXValue` holding a `CFRange`.
///
/// The range counts **UTF-16 code units**, not characters: an emoji or a
/// composed character occupies more than one unit, so this value must never be
/// used to slice a Rust string. It is only ever handed back to AX.
unsafe fn axvalue_range(value: CFTypeRef) -> Option<CFRange> {
    let ax_value = value as AXValueRef;
    if ax_value.is_null() || AXValueGetType(ax_value) != kAXValueTypeCFRange {
        return None;
    }
    let mut range = CFRange {
        location: 0 as CFIndex,
        length: 0 as CFIndex,
    };
    let ok = AXValueGetValue(
        ax_value,
        kAXValueTypeCFRange,
        &mut range as *mut CFRange as *mut c_void,
    );
    ok.then_some(range)
}

unsafe fn axvalue_rect(value: CFTypeRef) -> Option<ScreenRect> {
    let ax_value = value as AXValueRef;
    if ax_value.is_null() || AXValueGetType(ax_value) != kAXValueTypeCGRect {
        return None;
    }
    let mut rect = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width: 0.0,
            height: 0.0,
        },
    };
    let ok = AXValueGetValue(
        ax_value,
        kAXValueTypeCGRect,
        &mut rect as *mut CGRect as *mut c_void,
    );
    ok.then(|| ScreenRect::from_cg(rect))
}

/// The selection of whatever is focused, right now.
pub fn read_selection() -> Result<Selection, Failure> {
    if !super::permission::is_granted() {
        return Err(Failure::NotTrusted);
    }

    // SAFETY: every pointer below is checked for null before use, and every
    // borrowed object is kept alive by an `OwnedCf` for as long as it is read.
    unsafe {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return Err(Failure::Other {
                code: "AXUIElementCreateSystemWide",
            });
        }
        let _system = OwnedCf(system as CFTypeRef);
        AXUIElementSetMessagingTimeout(system, MESSAGING_TIMEOUT_SECONDS);

        let focused = copy_attribute(system, kAXFocusedUIElementAttribute)?;
        let element = focused.0 as AXUIElementRef;
        if element.is_null() {
            return Err(Failure::NoFocusedElement);
        }
        AXUIElementSetMessagingTimeout(element, MESSAGING_TIMEOUT_SECONDS);

        // Text first. An app can support this and not the range, in which case we
        // still have something worth translating.
        let text = match copy_attribute(element, kAXSelectedTextAttribute) {
            Ok(value) => cfstring_to_string(value.0 as CFStringRef).unwrap_or_default(),
            // An empty or missing selection is not fatal on its own: the range may
            // still tell us something, and the caller may fall back.
            Err(Failure::NoSelection) => String::new(),
            Err(Failure::Unsupported) => String::new(),
            Err(other) => return Err(other),
        };

        // The range is only wanted for its geometry, which is what the trigger
        // point is anchored to.
        let bounds = read_selection_bounds(element);

        if text.trim().is_empty() {
            if bounds.is_none() {
                return Err(Failure::NoSelection);
            }
            // Geometry without text: the app supports ranges but not the text
            // attribute. The pasteboard fallback can still get the words.
            return Err(Failure::Unsupported);
        }

        Ok(Selection { text, bounds })
    }
}

/// The screen rectangle of the current selection, if the app will say.
///
/// Separate from the text because it is a *parameterised* attribute: the range is
/// passed in as an `AXValue`, and the answer comes back as another one. Anything
/// short of a full answer is `None` — geometry is a nicety, never a reason to
/// fail the read.
unsafe fn read_selection_bounds(element: AXUIElementRef) -> Option<ScreenRect> {
    let range_value = copy_attribute(element, kAXSelectedTextRangeAttribute).ok()?;
    let range = axvalue_range(range_value.0)?;
    if range.length <= 0 {
        return None;
    }

    let attribute = CFString::new(kAXBoundsForRangeParameterizedAttribute);
    let mut bounds: CFTypeRef = std::ptr::null();
    let error = AXUIElementCopyParameterizedAttributeValue(
        element,
        attribute.as_concrete_TypeRef(),
        range_value.0,
        &mut bounds,
    );
    if error != 0 || bounds.is_null() {
        return None;
    }
    let _bounds = OwnedCf(bounds);

    axvalue_rect(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_api_disabled_error_means_permission_is_missing() {
        assert_eq!(classify(kAXErrorAPIDisabled), Failure::NotTrusted);
        assert_eq!(classify(kAXErrorAPIDisabled).label(), "not_trusted");
    }

    /// The distinction the fallback chain depends on: "this app will never give
    /// me a selection" is a different decision from "ask again in a moment".
    #[test]
    fn unsupported_is_not_confused_with_busy() {
        let unsupported = classify(kAXErrorAttributeUnsupported);
        let parameterised = classify(kAXErrorParameterizedAttributeUnsupported);
        let busy = classify(kAXErrorCannotComplete);

        assert_eq!(unsupported, Failure::Unsupported);
        assert_eq!(parameterised, Failure::Unsupported);
        assert_eq!(busy.label(), "blocked");
        assert_ne!(unsupported, busy);
    }

    #[test]
    fn a_disappearing_element_is_transient_and_carries_its_ax_name() {
        let failure = classify(kAXErrorInvalidUIElement);
        assert_eq!(failure.label(), "blocked");
        assert_eq!(failure.detail(), Some("kAXErrorInvalidUIElement"));
    }

    #[test]
    fn no_value_means_nothing_is_selected() {
        assert_eq!(classify(kAXErrorNoValue), Failure::NoSelection);
        assert_eq!(classify(kAXErrorNoValue).label(), "no_selection");
    }

    /// An error code we have never seen must still be reportable by name, not
    /// flattened into the same bucket as everything else.
    #[test]
    fn an_unknown_error_keeps_its_code() {
        let failure = classify(-99999);
        assert_eq!(failure.label(), "other");
        assert_eq!(failure.detail(), Some("unknown error"));
    }

    #[test]
    fn a_rect_knows_what_is_inside_it() {
        let rect = ScreenRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        };

        assert!(rect.contains((0.0, 0.0)), "the top-left corner is inside");
        assert!(rect.contains((100.0, 50.0)), "and so is the bottom-right");
        assert!(rect.contains((50.0, 25.0)));
        assert!(!rect.contains((-1.0, 25.0)));
        assert!(!rect.contains((50.0, 51.0)));
    }

    #[test]
    fn a_rect_reports_its_edges() {
        let rect = ScreenRect {
            x: 100.0,
            y: 200.0,
            width: 30.0,
            height: 12.0,
        };
        assert_eq!(rect.right(), 130.0);
        assert_eq!(rect.bottom(), 212.0);
    }
}
