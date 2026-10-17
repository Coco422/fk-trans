//! The trigger point and the card it opens into.
//!
//! A selection does not translate immediately: a small point appears beside it,
//! and the translation happens when the point is clicked. That is the whole
//! reason a trackpad user can use this app — they can select with a double click,
//! but they have no middle button to press, and translating on every selection
//! would be both noisy and expensive.
//!
//! This is the state machine, as pure data: given a selection's rectangle, a
//! work area and a clock, it says what the window should be. The window itself
//! is in `macos::panel`, and `lib.rs` does nothing but apply what comes back.

use super::placement::{self, Point, Size};
use crate::macos::ax::ScreenRect;

/// The trigger point. Small enough to be unobtrusive, large enough to hit.
pub const POINT_SIZE: Size = Size {
    width: 22.0,
    height: 22.0,
};

/// The card is one fixed width so the text wraps predictably, and its height is
/// whatever the webview measures — up to a point, past which the content scrolls.
pub const CARD_WIDTH: f64 = 380.0;
pub const CARD_MIN_HEIGHT: f64 = 132.0;
pub const CARD_MAX_HEIGHT: f64 = 460.0;

const POINT_GAP: f64 = 6.0;
const CARD_GAP: f64 = 8.0;
const SCREEN_MARGIN: f64 = 8.0;

/// How long the point waits to be clicked.
///
/// Short, because an unclicked point is stale: the user has read the selection,
/// moved on, or the window underneath has changed. The card has no equivalent
/// deadline here — it hides itself from the frontend, which is where its own
/// content state lives.
pub const POINT_TTL_MS: i64 = 4_000;

/// [`POINT_TTL_MS`] as the duration the expiry timer is armed with.
pub const POINT_TTL: std::time::Duration = std::time::Duration::from_millis(POINT_TTL_MS as u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Hidden,
    /// The point, waiting to be clicked.
    Point,
    /// The card, showing a translation.
    Expanded,
}

/// Where the window goes, in logical points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub origin: Point,
    pub size: Size,
}

impl Placement {
    /// Whether a point falls inside the frame, in the same screen coordinates.
    ///
    /// The event tap reports positions in exactly these points, so this is what
    /// tells a click on the trigger point apart from a click on the app behind
    /// it.
    pub fn contains(&self, at: Point) -> bool {
        at.x >= self.origin.x
            && at.x <= self.origin.x + self.size.width
            && at.y >= self.origin.y
            && at.y <= self.origin.y + self.size.height
    }
}

/// What the caller should do to the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Transition {
    /// Nothing about the window changes.
    None,
    /// Put the window here and show it.
    Show(Placement),
    /// Hide the window.
    Hide,
}

#[derive(Debug, Clone)]
pub struct Panel {
    phase: Phase,
    /// The trigger point's top-left. The card is placed from it, so the two are
    /// always in the same part of the screen.
    anchor: Point,
    /// The display the selection was on, as it was when the point appeared.
    work_area: ScreenRect,
    /// The height the webview last reported for the card.
    card_height: f64,
    /// The text the point stands for, read when the point appeared. Kept so a
    /// click has something to translate even if the selection has since changed.
    selection: String,
    /// Bumped on every transition. A translation that comes back carrying an
    /// older generation belongs to a panel the user has already dismissed.
    generation: u64,
    shown_at_ms: i64,
}

impl Default for Panel {
    fn default() -> Self {
        Self::new()
    }
}

impl Panel {
    pub fn new() -> Self {
        Self {
            phase: Phase::Hidden,
            anchor: Point { x: 0.0, y: 0.0 },
            work_area: ScreenRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            card_height: CARD_MIN_HEIGHT,
            selection: String::new(),
            generation: 0,
            shown_at_ms: 0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn selection(&self) -> Option<&str> {
        match self.phase {
            Phase::Expanded => Some(&self.selection),
            _ => None,
        }
    }

    /// Whether the panel is on screen in any form.
    pub fn is_visible(&self) -> bool {
        self.phase != Phase::Hidden
    }

    /// Show the trigger point for a selection.
    ///
    /// `bounds` is where the app says the selection is, which it often will not
    /// say; `pointer` is then used instead, which is where the user finished
    /// selecting. Both go through the same placement rules, so the point flips to
    /// the other side of the screen edge either way.
    pub fn show_point(
        &mut self,
        bounds: Option<ScreenRect>,
        pointer: Point,
        work_area: ScreenRect,
        selection: String,
        now_ms: i64,
    ) -> Transition {
        // A selection with no rectangle is a selection with no width, at the
        // pointer — which the placement rules already handle.
        let rect = bounds.unwrap_or(ScreenRect {
            x: pointer.x,
            y: pointer.y,
            width: 0.0,
            height: 0.0,
        });

        self.anchor = placement::trigger_point(
            rect,
            POINT_SIZE,
            work_area,
            POINT_GAP,
            SCREEN_MARGIN,
        );
        self.work_area = work_area;
        self.selection = selection;
        self.card_height = CARD_MIN_HEIGHT;
        self.phase = Phase::Point;
        self.shown_at_ms = now_ms;
        self.generation += 1;

        Transition::Show(self.placement())
    }

    /// Show the card straight away, with no point to click first.
    ///
    /// What a middle click and the keyboard shortcut do: the user has already
    /// asked for a translation, so there is nothing to wait for. `work_area` is
    /// used only to decide which way the card opens.
    pub fn show_card(&mut self, pointer: Point, work_area: ScreenRect, now_ms: i64) -> Transition {
        self.anchor = pointer;
        self.work_area = work_area;
        self.selection.clear();
        self.card_height = CARD_MIN_HEIGHT;
        self.phase = Phase::Expanded;
        self.shown_at_ms = now_ms;
        self.generation += 1;

        Transition::Show(self.placement())
    }

    /// The point was clicked: open the card where the point is.
    ///
    /// Returns `None` when there is no point to expand — a double click on the
    /// point, or a click that arrived after the point expired.
    pub fn expand(&mut self) -> Option<Transition> {
        if self.phase != Phase::Point {
            return None;
        }

        self.phase = Phase::Expanded;
        self.card_height = CARD_MIN_HEIGHT;
        self.generation += 1;
        Some(Transition::Show(self.placement()))
    }

    /// The webview measured its content and reports the height it needs.
    ///
    /// Clamped rather than trusted: the webview is the only thing that knows how
    /// tall the translation is, and the only thing that could ask for a card
    /// taller than the screen.
    pub fn resize_card(&mut self, height: f64) -> Transition {
        if self.phase != Phase::Expanded {
            return Transition::None;
        }

        let height = if height.is_finite() {
            height.clamp(CARD_MIN_HEIGHT, CARD_MAX_HEIGHT)
        } else {
            CARD_MIN_HEIGHT
        };

        if (height - self.card_height).abs() < 1.0 {
            return Transition::None;
        }
        self.card_height = height;

        Transition::Show(self.placement())
    }

    /// Hide, whatever was showing.
    pub fn hide(&mut self) -> Transition {
        if self.phase == Phase::Hidden {
            return Transition::None;
        }

        self.phase = Phase::Hidden;
        self.selection.clear();
        self.generation += 1;
        Transition::Hide
    }

    /// Hide the point because the user did something else.
    ///
    /// `at` is where the pointer was, for a click. A click *on* the point is the
    /// click that opens it, so it must not close it in the same breath — and the
    /// tap sees that click too, just before the panel does. A scroll has no such
    /// case and passes `None`.
    ///
    /// Deliberately does *not* apply to the card: that is a translation the user
    /// asked for and is reading, and a stray scroll in the app behind it must not
    /// take it away.
    pub fn dismiss_point(&mut self, at: Option<Point>) -> Transition {
        if self.phase != Phase::Point {
            return Transition::None;
        }
        if at.is_some_and(|point| self.placement().contains(point)) {
            return Transition::None;
        }
        self.hide()
    }

    /// Hide the point if it has been waiting too long to be clicked.
    ///
    /// Checked on a timer that was started when the point appeared rather than
    /// against the current generation, because a deadline that has already passed
    /// cannot belong to a point that has just been replaced.
    pub fn expire_point(&mut self, now_ms: i64) -> Transition {
        if self.phase != Phase::Point || now_ms - self.shown_at_ms < POINT_TTL_MS {
            return Transition::None;
        }
        self.hide()
    }

    fn placement(&self) -> Placement {
        match self.phase {
            Phase::Hidden => Placement {
                origin: self.anchor,
                size: POINT_SIZE,
            },
            Phase::Point => Placement {
                origin: self.anchor,
                size: POINT_SIZE,
            },
            Phase::Expanded => Placement {
                origin: placement::panel_origin(
                    self.anchor,
                    Size {
                        width: CARD_WIDTH,
                        height: self.card_height,
                    },
                    self.work_area,
                    CARD_GAP,
                    SCREEN_MARGIN,
                ),
                size: Size {
                    width: CARD_WIDTH,
                    height: self.card_height,
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work_area() -> ScreenRect {
        ScreenRect {
            x: 0.0,
            y: 25.0,
            width: 1512.0,
            height: 957.0,
        }
    }

    fn selection(x: f64, y: f64, width: f64, height: f64) -> ScreenRect {
        ScreenRect {
            x,
            y,
            width,
            height,
        }
    }

    fn pointer_at(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    fn pointed() -> Panel {
        let mut panel = Panel::new();
        panel.show_point(
            Some(selection(100.0, 200.0, 120.0, 18.0)),
            pointer_at(220.0, 218.0),
            work_area(),
            "hello world".to_string(),
            1_000,
        );
        panel
    }

    #[test]
    fn a_new_panel_is_hidden_and_stays_out_of_the_way() {
        let panel = Panel::new();
        assert_eq!(panel.phase(), Phase::Hidden);
        assert!(!panel.is_visible());
        assert_eq!(panel.selection(), None);
    }

    #[test]
    fn a_point_appears_at_the_bottom_right_of_the_selection() {
        let panel = pointed();

        assert_eq!(panel.phase(), Phase::Point);
        // 100 + 120 + 6, and 200 + 18 + 6.
        assert_eq!(
            panel.placement(),
            Placement {
                origin: Point { x: 226.0, y: 224.0 },
                size: POINT_SIZE,
            }
        );
    }

    /// Plenty of apps will not say where the selection is. The point still has to
    /// appear, at the pointer that made the selection.
    #[test]
    fn without_a_rectangle_the_point_uses_the_pointer() {
        let mut panel = Panel::new();
        panel.show_point(
            None,
            pointer_at(300.0, 400.0),
            work_area(),
            "text".to_string(),
            1_000,
        );

        assert_eq!(
            panel.placement().origin,
            Point {
                x: 300.0 + POINT_GAP,
                y: 400.0 + POINT_GAP
            }
        );
    }

    /// Near the right edge the point goes to the left of the selection rather
    /// than off the screen.
    #[test]
    fn the_point_flips_left_near_the_right_edge() {
        let mut panel = Panel::new();
        panel.show_point(
            Some(selection(1400.0, 200.0, 100.0, 18.0)),
            pointer_at(1500.0, 218.0),
            work_area(),
            "text".to_string(),
            1_000,
        );

        let origin = panel.placement().origin;
        assert_eq!(origin.x, 1400.0 - POINT_GAP - POINT_SIZE.width);
    }

    /// A click expands into the card, placed from where the point was.
    #[test]
    fn clicking_the_point_opens_the_card_below_it() {
        let mut panel = pointed();
        let point_origin = panel.placement().origin;

        let transition = panel.expand().expect("a point is showing");

        assert_eq!(panel.phase(), Phase::Expanded);
        let Transition::Show(placement) = transition else {
            panic!("expanding must show the card, got {transition:?}");
        };
        assert_eq!(placement.size.width, CARD_WIDTH);
        assert_eq!(placement.origin.x, point_origin.x);
        assert_eq!(placement.origin.y, point_origin.y + CARD_GAP);
    }

    /// Only a point expands. A second click on a card that is already open must
    /// not restart the translation.
    #[test]
    fn only_a_point_can_be_expanded() {
        let mut panel = pointed();
        assert!(panel.expand().is_some());

        assert!(panel.expand().is_none(), "the card is already open");
    }

    #[test]
    fn a_hidden_panel_cannot_be_expanded() {
        let mut panel = Panel::new();
        assert!(panel.expand().is_none());
    }

    /// The card's height comes from the webview, clamped so it can never be
    /// taller than the screen it has to fit on.
    #[test]
    fn the_card_height_is_clamped() {
        let mut panel = pointed();
        panel.expand();

        panel.resize_card(10.0);
        assert_eq!(panel.placement().size.height, CARD_MIN_HEIGHT);

        panel.resize_card(10_000.0);
        assert_eq!(panel.placement().size.height, CARD_MAX_HEIGHT);

        panel.resize_card(f64::NAN);
        assert_eq!(panel.placement().size.height, CARD_MIN_HEIGHT);
    }

    /// A resize must keep the card anchored where it was, not move it back to
    /// some default position.
    #[test]
    fn resizing_keeps_the_card_at_its_anchor() {
        let mut panel = pointed();
        panel.expand();
        let before = panel.placement().origin;

        panel.resize_card(300.0);

        assert_eq!(panel.placement().origin.x, before.x);
        assert!(
            panel.placement().origin.y <= before.y,
            "a taller card grows upwards when there is no room below"
        );
    }

    /// A card near the bottom of the screen opens upwards instead of running off
    /// the edge.
    #[test]
    fn a_card_near_the_bottom_edge_opens_upwards() {
        let mut panel = Panel::new();
        panel.show_point(
            Some(selection(100.0, 900.0, 120.0, 18.0)),
            pointer_at(220.0, 918.0),
            work_area(),
            "text".to_string(),
            1_000,
        );
        panel.expand();

        let placement = panel.placement();
        assert!(
            placement.origin.y + placement.size.height <= work_area().bottom() - SCREEN_MARGIN,
            "{placement:?}"
        );
    }

    /// A scroll or a click elsewhere closes the point, which is what the user
    /// meant by moving on.
    #[test]
    fn a_point_is_dismissed_by_activity_elsewhere() {
        let mut panel = pointed();

        assert_eq!(panel.dismiss_point(Some(pointer_at(900.0, 900.0))), Transition::Hide);
        assert_eq!(panel.phase(), Phase::Hidden);
        assert_eq!(
            panel.dismiss_point(Some(pointer_at(900.0, 900.0))),
            Transition::None,
            "already hidden"
        );
    }

    /// But not the card: that is a translation the user asked for and is reading.
    #[test]
    fn the_card_survives_activity_elsewhere() {
        let mut panel = pointed();
        panel.expand();

        assert_eq!(panel.dismiss_point(Some(pointer_at(0.0, 0.0))), Transition::None);
        assert_eq!(panel.phase(), Phase::Expanded);
    }

    #[test]
    fn an_unclicked_point_expires() {
        let mut panel = pointed();

        assert_eq!(panel.expire_point(1_000 + POINT_TTL_MS - 1), Transition::None);
        assert_eq!(panel.expire_point(1_000 + POINT_TTL_MS), Transition::Hide);
        assert_eq!(panel.phase(), Phase::Hidden);
    }

    /// The card has no such deadline from here; the frontend owns its own.
    #[test]
    fn the_card_does_not_expire() {
        let mut panel = pointed();
        panel.expand();

        assert_eq!(panel.expire_point(1_000 + POINT_TTL_MS * 100), Transition::None);
        assert_eq!(panel.phase(), Phase::Expanded);
    }

    /// The generation is what stops a slow translation from resurrecting a panel
    /// the user has since dismissed.
    #[test]
    fn every_transition_changes_the_generation() {
        let mut panel = Panel::new();
        let start = panel.generation();

        panel.show_point(None, pointer_at(0.0, 0.0), work_area(), "a".to_string(), 0);
        let after_point = panel.generation();
        assert_ne!(after_point, start);

        panel.expand();
        let after_expand = panel.generation();
        assert_ne!(after_expand, after_point);

        panel.hide();
        assert_ne!(panel.generation(), after_expand);
    }

    /// A middle click has no point to click first, so the card goes straight up.
    #[test]
    fn a_card_can_be_shown_without_a_point() {
        let mut panel = Panel::new();

        let transition = panel.show_card(pointer_at(400.0, 300.0), work_area(), 1_000);

        assert_eq!(panel.phase(), Phase::Expanded);
        let Transition::Show(placement) = transition else {
            panic!("expected the card to be shown, got {transition:?}");
        };
        assert_eq!(placement.size.width, CARD_WIDTH);
    }

    /// A new point is a new answer: whatever the previous one was waiting for is
    /// no longer wanted.
    #[test]
    fn a_second_point_supersedes_the_first() {
        let mut panel = pointed();
        let first = panel.generation();

        panel.show_point(
            Some(selection(10.0, 20.0, 30.0, 12.0)),
            pointer_at(40.0, 32.0),
            work_area(),
            "second".to_string(),
            2_000,
        );

        assert_ne!(panel.generation(), first);
        assert_eq!(panel.phase(), Phase::Point);
    }

    /// The text is only handed out once the user has asked for a translation.
    #[test]
    fn the_selection_is_only_available_once_expanded() {
        let mut panel = pointed();
        assert_eq!(panel.selection(), None);

        panel.expand();
        assert_eq!(panel.selection(), Some("hello world"));

        panel.hide();
        assert_eq!(panel.selection(), None);
    }

    /// A click on the point is the click that opens it. The tap sees that click
    /// first, so reading it as a click away from the point would take the point
    /// down a moment before anything could open it.
    #[test]
    fn a_click_on_the_point_does_not_dismiss_it() {
        let mut panel = pointed();
        let placement = panel.placement();
        let centre = Point {
            x: placement.origin.x + placement.size.width / 2.0,
            y: placement.origin.y + placement.size.height / 2.0,
        };

        assert_eq!(panel.dismiss_point(Some(centre)), Transition::None);
        assert_eq!(panel.phase(), Phase::Point);
        assert!(panel.expand().is_some(), "the point is still there to click");
    }

    #[test]
    fn a_click_just_outside_the_point_dismisses_it() {
        let mut panel = pointed();
        let origin = panel.placement().origin;

        assert_eq!(
            panel.dismiss_point(Some(Point {
                x: origin.x - 1.0,
                y: origin.y,
            })),
            Transition::Hide
        );
    }

    /// A scroll has no position to be inside of: it dismisses the point wherever
    /// the pointer happens to be.
    #[test]
    fn a_scroll_dismisses_the_point_from_anywhere() {
        let mut panel = pointed();
        assert_eq!(panel.dismiss_point(None), Transition::Hide);
    }

    /// The gaps and margins all take space away from the card, which is the
    /// largest thing the panel ever shows. On the smallest display worth
    /// supporting they must still leave room for one.
    #[test]
    fn the_card_fits_on_a_small_display() {
        let smallest_display_height = 600.0_f64;
        assert!(CARD_MAX_HEIGHT + CARD_GAP + 2.0 * SCREEN_MARGIN < smallest_display_height);
    }
}
