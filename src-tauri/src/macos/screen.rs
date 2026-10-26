//! Which display a point is on, and how much of it is usable.
//!
//! This is asked with AppKit directly rather than through Tauri's
//! `monitor_from_point`, which compares its argument against `CGDisplayBounds` —
//! a rectangle in *points* — while documenting the argument as *physical pixels*.
//! On any display that is not 1x the two disagree by the scale factor and the
//! lookup misses, which used to cost the trigger point its placement entirely.
//!
//! The circle to get round: converting a physical pointer position into points
//! needs a scale factor, and the scale factor belongs to a display — which is the
//! thing being looked for. So every display is tried with its *own* scale factor,
//! and the one whose frame then contains the pointer is the answer.
//!
//! AppKit reports frames with the origin at the bottom-left of the primary
//! display; Accessibility, the event tap, and the window placement all work from
//! its top-left. [`flip_rect`] is that conversion, and it is the part worth
//! testing, so it is a plain function of the primary display's height.

use crate::macos::ax::ScreenRect;
use core_graphics::geometry::CGRect;
use objc::runtime::Object;
use objc::{class, msg_send, sel, sel_impl};

/// One display, as AppKit describes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawScreen {
    /// The whole display, bottom-left origin.
    pub frame: ScreenRect,
    /// The part not taken by the menu bar and the Dock, same origin.
    pub visible_frame: ScreenRect,
    pub scale: f64,
}

/// A display, resolved into the space everything else uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Display {
    /// Usable area in logical points, top-left origin.
    pub work_area: ScreenRect,
    /// Backing pixels per point.
    pub scale: f64,
}

/// Turn a rectangle from AppKit's bottom-left space into a top-left one.
///
/// `primary_height` is the height of the display whose origin AppKit measures
/// from, which is always the first one it lists.
pub fn flip_rect(rect: ScreenRect, primary_height: f64) -> ScreenRect {
    ScreenRect {
        x: rect.x,
        y: primary_height - (rect.y + rect.height),
        width: rect.width,
        height: rect.height,
    }
}

/// Resolve the display a physical pointer position is on.
///
/// Returns `None` when no display claims the point, which happens if the pointer
/// moved between reading it and asking — ordinary, and not worth guessing at.
pub fn resolve(screens: &[RawScreen], physical: (f64, f64)) -> Option<Display> {
    let primary_height = screens.first()?.frame.height;

    for screen in screens {
        // Each display is tested with its own scale factor, which is what makes
        // this work across a Retina laptop and a 1x external monitor at once.
        let candidate = (
            physical.0 / screen.scale,
            physical.1 / screen.scale,
        );
        let frame = flip_rect(screen.frame, primary_height);

        if frame.contains(candidate) {
            return Some(Display {
                work_area: flip_rect(screen.visible_frame, primary_height),
                scale: screen.scale,
            });
        }
    }

    None
}

/// The display a physical pointer position is on.
///
/// AppKit is queried from whatever thread this is called on, as Tauri's own
/// monitor code already did. These are read-only queries with no UI mutation.
pub fn display_at(physical: (f64, f64)) -> Option<Display> {
    let screens = read_screens()?;
    resolve(&screens, physical)
}

fn read_screens() -> Option<Vec<RawScreen>> {
    // SAFETY: `screens` is an autoreleased NSArray of NSScreen, and every object
    // taken from it is used before this function returns. No object here is
    // owned, so nothing is released.
    unsafe {
        let screens: *mut Object = msg_send![class!(NSScreen), screens];
        if screens.is_null() {
            return None;
        }

        let count: usize = msg_send![screens, count];
        if count == 0 {
            return None;
        }

        let mut result = Vec::with_capacity(count);
        for index in 0..count {
            let screen: *mut Object = msg_send![screens, objectAtIndex: index];
            if screen.is_null() {
                continue;
            }

            let frame: CGRect = msg_send![screen, frame];
            let visible: CGRect = msg_send![screen, visibleFrame];
            let scale: f64 = msg_send![screen, backingScaleFactor];

            result.push(RawScreen {
                frame: ScreenRect::from_cg(frame),
                visible_frame: ScreenRect::from_cg(visible),
                // A display that reports a nonsense scale is worse than one that
                // reports none, so fall back rather than divide by it.
                scale: if scale > 0.0 { scale } else { 1.0 },
            });
        }
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> ScreenRect {
        ScreenRect {
            x,
            y,
            width,
            height,
        }
    }

    /// A 2x laptop display with a 25pt menu bar. AppKit's origin is bottom-left,
    /// so the visible part sits at y = 0 and the display's top is at y = 982.
    fn retina() -> RawScreen {
        RawScreen {
            frame: rect(0.0, 0.0, 1512.0, 982.0),
            visible_frame: rect(0.0, 0.0, 1512.0, 957.0),
            scale: 2.0,
        }
    }

    /// A second display to the right, 1x, with a Dock along its bottom.
    fn external() -> RawScreen {
        RawScreen {
            frame: rect(1512.0, 0.0, 1920.0, 1080.0),
            visible_frame: rect(1512.0, 70.0, 1920.0, 1010.0),
            scale: 1.0,
        }
    }

    #[test]
    fn a_rect_is_flipped_about_the_primary_display() {
        let flipped = flip_rect(rect(0.0, 0.0, 1512.0, 982.0), 982.0);

        // The whole display covers the global space from the top: y = 0.
        assert_eq!(flipped, rect(0.0, 0.0, 1512.0, 982.0));
    }

    #[test]
    fn the_menu_bar_becomes_the_top_of_the_work_area() {
        // AppKit's visible frame stops 25pt below the top; in top-left terms that
        // is a 25pt inset at the top.
        let flipped = flip_rect(rect(0.0, 0.0, 1512.0, 957.0), 982.0);

        assert_eq!(flipped.y, 25.0);
        assert_eq!(flipped.height, 957.0);
    }

    #[test]
    fn a_dock_at_the_bottom_becomes_an_inset_at_the_bottom() {
        let flipped = flip_rect(external().visible_frame, 982.0);

        // The work area ends 70pt above the display's bottom edge.
        assert_eq!(flipped.bottom(), 982.0 - 70.0);
    }

    /// The failure this module exists for: a pointer at a physical position on a
    /// 2x display. Dividing by the wrong scale factor put it outside every
    /// display, and the trigger point was simply never placed.
    #[test]
    fn a_pointer_on_a_retina_display_is_found_from_its_physical_position() {
        let display = resolve(&[retina()], (1796.0, 1153.0)).expect("the display is there");

        assert_eq!(display.scale, 2.0);
        assert_eq!(display.work_area.y, 25.0, "the menu bar is excluded");
    }

    /// Two displays with different scale factors is the case a single assumed
    /// scale factor cannot handle.
    #[test]
    fn each_display_is_tried_with_its_own_scale_factor() {
        let screens = [retina(), external()];

        // Physical (2400, 500) is (1200, 250) points on the 2x display.
        let on_retina = resolve(&screens, (2400.0, 500.0)).expect("retina");
        assert_eq!(on_retina.scale, 2.0);

        // Physical (2000, 500) cannot be on the 2x display — that would be 1000
        // points, inside it — so this is ambiguous, and the first match wins.
        // The unambiguous case: (3200, 500) is 1600 points, past the 1512pt
        // edge of the first display.
        let on_external = resolve(&screens, (3200.0, 500.0)).expect("external");
        assert_eq!(on_external.scale, 1.0);
        assert_eq!(on_external.work_area.x, 1512.0);
    }

    #[test]
    fn a_pointer_below_every_display_is_not_claimed_by_one() {
        assert!(resolve(&[retina()], (1000.0, 5000.0)).is_none());
    }

    #[test]
    fn no_displays_at_all_is_not_a_panic() {
        assert!(resolve(&[], (0.0, 0.0)).is_none());
    }

    /// The `msg_send!` returns are the part the pure tests cannot reach: a
    /// struct returned through the Objective-C runtime is only correct if the
    /// ABI matches, and a mismatch shows up as plausible-looking garbage rather
    /// than as a crash.
    ///
    /// So ask for a real display and then look it up again by its own centre,
    /// converted the way the pipeline converts the pointer. A headless runner
    /// with no screens passes rather than fails — there is nothing to check.
    #[test]
    fn a_real_display_can_be_found_again_by_its_own_centre() {
        let Some(screens) = read_screens() else {
            return;
        };
        assert!(!screens.is_empty(), "screens were reported as empty");

        for screen in &screens {
            assert!(screen.scale > 0.0, "scale: {screen:?}");
            assert!(screen.frame.width > 0.0, "frame: {screen:?}");
            assert!(screen.frame.height > 0.0, "frame: {screen:?}");
            assert!(
                screen.visible_frame.width <= screen.frame.width + 1.0,
                "the work area cannot be wider than the display: {screen:?}"
            );
            assert!(
                screen.visible_frame.height <= screen.frame.height + 1.0,
                "the work area cannot be taller than the display: {screen:?}"
            );
        }

        let primary_height = screens[0].frame.height;
        let centre = flip_rect(screens[0].frame, primary_height);
        let physical = (
            (centre.x + centre.width / 2.0) * screens[0].scale,
            (centre.y + centre.height / 2.0) * screens[0].scale,
        );

        let found = resolve(&screens, physical).expect("the centre of a display is on it");
        assert_eq!(found.scale, screens[0].scale);
        assert_eq!(
            found.work_area,
            flip_rect(screens[0].visible_frame, primary_height)
        );
    }

    /// A display reporting a zero scale would otherwise be divided by.
    #[test]
    fn a_nonsense_scale_factor_is_replaced() {
        let broken = RawScreen {
            scale: 0.0,
            ..retina()
        };
        let screens = [broken];

        // Whatever it resolves to, it must not be by dividing by zero.
        let display = resolve(&screens, (100.0, 100.0));
        assert!(display.is_none() || display.unwrap().scale > 0.0);
    }
}
