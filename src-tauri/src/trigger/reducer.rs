//! Deciding whether a mouse event should start a translation.
//!
//! Extracted from the CGEventTap callback, which is the wrong place to decide
//! anything: it runs on a thread macOS stops delivering events to when it is too
//! slow, and none of it can be exercised without a live session and a real mouse.
//! Here the events are plain values and the clock is a parameter, so the
//! thresholds and the ordering between them can be tested directly — which is
//! what the trigger being flaky needs.
//!
//! The reducer is a small state machine over the three things that have to be
//! remembered between events: where the current press started, where the previous
//! click landed, and when a selection was last translated.

use super::TriggerSource;

/// How far the pointer must travel for a press to be a selection drag rather than
/// a click. Below this it is a click, and a second one close in time and place is
/// a double click.
pub const SELECTION_DRAG_MIN_DISTANCE_PX: f64 = 8.0;

/// How long a press must last to be a drag. A very short one is a stray click or
/// a flick, and translating it is more annoying than missing it.
pub const SELECTION_DRAG_MIN_DURATION_MS: i64 = 80;

/// Suppresses a second selection trigger hot on the heels of the first: a
/// double click and a drag can both end the same selection, and a triple click
/// selects a whole paragraph.
pub const SELECTION_TRIGGER_COOLDOWN_MS: i64 = 700;

/// The window in which a second click counts as a double click. Matches the
/// system's own double-click interval closely enough for selection purposes.
pub const DOUBLE_CLICK_INTERVAL_MS: i64 = 400;

/// How far the second click of a double click may drift. Small enough that it
/// cannot be confused with the drag threshold above.
pub const DOUBLE_CLICK_SLOP_PX: f64 = 6.0;

/// How far the wheel has to turn before it counts as the user moving on.
///
/// Trackpads emit scroll events for the faintest touch, and dismissing the
/// trigger point on those would make it disappear when someone brushed the
/// surface on their way to the keyboard.
pub const SCROLL_DISMISS_PX: f64 = 80.0;

/// The settings the reducer needs, read from the shared state once per event so
/// the decision itself stays a pure function of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerOptions {
    pub trigger_button: i64,
    pub selection_trigger_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEvent {
    Down,
    Up,
}

/// One event from the tap, as much of it as the decision needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEvent {
    Button {
        kind: ButtonEvent,
        /// 0 for the left button, otherwise the number macOS reports.
        button: i64,
        x: f64,
        y: f64,
        timestamp_ms: i64,
    },
    /// The wheel or the trackpad moved by `delta` points.
    Scroll { delta: f64, timestamp_ms: i64 },
}

impl PointerEvent {
    /// Where the pointer was, for the events that happen at a place.
    ///
    /// A scroll has a location too, but it is not what a scroll means: the point
    /// goes away whoever the pointer is over.
    pub fn position(&self) -> Option<(f64, f64)> {
        match self {
            Self::Button { x, y, .. } => Some((*x, *y)),
            Self::Scroll { .. } => None,
        }
    }

    pub fn timestamp_ms(&self) -> i64 {
        match self {
            Self::Button { timestamp_ms, .. } | Self::Scroll { timestamp_ms, .. } => *timestamp_ms,
        }
    }
}

/// An event that could have triggered a translation but did not. Recorded so a
/// trigger that fails can be traced to the layer that dropped it, rather than
/// looking like the app ignoring the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ignored {
    /// A button other than the configured trigger button. Only interesting while
    /// the user is testing which button their mouse sends.
    OtherButton,
    /// A release with no press before it. The tap was rebuilt mid-drag, most
    /// likely, and the gesture cannot be measured.
    PressNotSeen,
    /// A selection gesture arrived while the selection trigger is switched off.
    SelectionTriggerDisabled,
    /// A selection was translated a moment ago.
    Cooldown,
}

impl Ignored {
    pub fn describe(self) -> &'static str {
        match self {
            Self::OtherButton => "按下的不是设定的触发键",
            Self::PressNotSeen => "只收到松开，没收到对应的按下",
            Self::SelectionTriggerDisabled => "划词触发是关闭的",
            Self::Cooldown => "刚刚才触发过一次",
        }
    }
}

/// Why whatever the trigger was showing should go away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DismissReason {
    /// The user clicked somewhere else, which is as clear a way as any of saying
    /// they are done with what was on screen.
    Clicked,
    /// The wheel turned far enough. Scrolling inside the app being read almost
    /// always means the selection has moved or gone.
    Scrolled,
}

impl DismissReason {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Clicked => "点到别处去了",
            Self::Scrolled => "后面的页面滚动了",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Nothing to do. Most events, including ordinary clicks.
    Quiet,
    /// A translation should start, from the given source.
    Trigger(TriggerSource),
    /// A gesture that would have translated something, declined for a reason
    /// worth reporting.
    Ignored(Ignored),
    /// Whatever the panel is showing should go.
    ///
    /// Deliberately unconditional: the reducer does not know what is on screen,
    /// so it reports the event and lets the panel decide whether it applies. One
    /// place knows whether the point is showing, and it is not here.
    Dismiss(DismissReason),
}

#[derive(Debug, Clone, Copy)]
struct Press {
    x: f64,
    y: f64,
    timestamp_ms: i64,
}

#[derive(Debug, Clone, Copy)]
struct Click {
    x: f64,
    y: f64,
    timestamp_ms: i64,
}

#[derive(Debug, Default)]
pub struct TriggerReducer {
    press: Option<Press>,
    previous_click: Option<Click>,
    last_selection_trigger_at: Option<i64>,
}

fn distance(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    (dx * dx + dy * dy).sqrt()
}

impl TriggerReducer {
    pub fn reduce(&mut self, event: PointerEvent, options: TriggerOptions) -> Decision {
        match event {
            PointerEvent::Button {
                kind: ButtonEvent::Down,
                button,
                x,
                y,
                timestamp_ms,
            } => self.press(button, x, y, timestamp_ms, options),
            PointerEvent::Button {
                kind: ButtonEvent::Up,
                button,
                x,
                y,
                timestamp_ms,
            } => self.release(button, x, y, timestamp_ms, options),
            PointerEvent::Scroll { delta, .. } => {
                if delta.abs() < SCROLL_DISMISS_PX {
                    Decision::Quiet
                } else {
                    Decision::Dismiss(DismissReason::Scrolled)
                }
            }
        }
    }

    /// The trigger button fires on the press, so a translation starts on the
    /// middle-click itself rather than when the button comes back up.
    fn press(
        &mut self,
        button: i64,
        x: f64,
        y: f64,
        timestamp_ms: i64,
        options: TriggerOptions,
    ) -> Decision {
        if button == options.trigger_button {
            return Decision::Trigger(TriggerSource::MouseMiddle);
        }
        if button != 0 {
            return Decision::Ignored(Ignored::OtherButton);
        }

        // Recorded before the decision below, because a click that dismisses
        // something can still be the start of a selection drag.
        self.press = Some(Press { x, y, timestamp_ms });

        Decision::Dismiss(DismissReason::Clicked)
    }

    fn release(
        &mut self,
        button: i64,
        x: f64,
        y: f64,
        timestamp_ms: i64,
        options: TriggerOptions,
    ) -> Decision {
        if button != 0 {
            return Decision::Quiet;
        }

        // The press has to be consumed either way, so a release that will not
        // trigger does not leave its press behind to pair with the next one.
        let Some(press) = self.press.take() else {
            return Decision::Ignored(Ignored::PressNotSeen);
        };

        let click = Click {
            x,
            y,
            timestamp_ms,
        };
        // Whether the pointer moved at all decides whether this was a click.
        // Duration only decides whether a *drag* was deliberate, so a flick is
        // not a drag but is still not a click either.
        let travelled = distance(press.x, press.y, x, y);
        let moved = travelled >= SELECTION_DRAG_MIN_DISTANCE_PX;
        let is_drag = moved && timestamp_ms - press.timestamp_ms >= SELECTION_DRAG_MIN_DURATION_MS;

        let is_double_click = !moved && self.is_double_click(&click);
        // Recorded after the check, or the click would be compared with itself.
        // Anything that moved clears the sequence: a drag is not a click, so it
        // can neither pair with one nor start a double click of its own.
        self.previous_click = if moved { None } else { Some(click) };

        if !is_drag && !is_double_click {
            // An ordinary click: selecting nothing, so there is nothing to report.
            return Decision::Quiet;
        }

        if !options.selection_trigger_enabled {
            return Decision::Ignored(Ignored::SelectionTriggerDisabled);
        }

        if let Some(last) = self.last_selection_trigger_at {
            if timestamp_ms - last < SELECTION_TRIGGER_COOLDOWN_MS {
                return Decision::Ignored(Ignored::Cooldown);
            }
        }

        self.last_selection_trigger_at = Some(timestamp_ms);
        Decision::Trigger(TriggerSource::MouseSelection)
    }

    /// Whether `click` is the second of a double click, given the previous one.
    ///
    /// A double click selects a word without moving the pointer, which is how
    /// users without a middle mouse button select a single word — the drag rule
    /// never fires for them.
    fn is_double_click(&self, click: &Click) -> bool {
        let Some(previous) = self.previous_click else {
            return false;
        };

        click.timestamp_ms - previous.timestamp_ms <= DOUBLE_CLICK_INTERVAL_MS
            && distance(previous.x, previous.y, click.x, click.y) <= DOUBLE_CLICK_SLOP_PX
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRIGGER_BUTTON: i64 = 2;

    fn options() -> TriggerOptions {
        TriggerOptions {
            trigger_button: TRIGGER_BUTTON,
            selection_trigger_enabled: true,
        }
    }

    fn down(button: i64, x: f64, y: f64, timestamp_ms: i64) -> PointerEvent {
        PointerEvent::Button {
            kind: ButtonEvent::Down,
            button,
            x,
            y,
            timestamp_ms,
        }
    }

    fn up(x: f64, y: f64, timestamp_ms: i64) -> PointerEvent {
        PointerEvent::Button {
            kind: ButtonEvent::Up,
            button: 0,
            x,
            y,
            timestamp_ms,
        }
    }

    fn scroll(delta: f64) -> PointerEvent {
        PointerEvent::Scroll {
            delta,
            timestamp_ms: 0,
        }
    }

    /// A press and release in one call, which is what most of these tests are
    /// actually about.
    fn click(reducer: &mut TriggerReducer, x: f64, y: f64, timestamp_ms: i64) -> Decision {
        click_with(reducer, x, y, timestamp_ms, options())
    }

    fn click_with(
        reducer: &mut TriggerReducer,
        x: f64,
        y: f64,
        timestamp_ms: i64,
        options: TriggerOptions,
    ) -> Decision {
        // A press on the left button always reports a click now, so it is the
        // release that these tests are asking about.
        reducer.reduce(down(0, x, y, timestamp_ms), options);
        reducer.reduce(up(x, y, timestamp_ms + 30), options)
    }

    fn drag(
        reducer: &mut TriggerReducer,
        from: (f64, f64),
        to: (f64, f64),
        start_ms: i64,
        duration_ms: i64,
    ) -> Decision {
        reducer.reduce(down(0, from.0, from.1, start_ms), options());
        reducer.reduce(up(to.0, to.1, start_ms + duration_ms), options())
    }

    /// A press on the left button is also when the panel is told the user has
    /// gone elsewhere, so tests about clicks have to check both events.
    #[test]
    fn a_left_press_reports_a_click() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(
            reducer.reduce(down(0, 10.0, 10.0, 1_000), options()),
            Decision::Dismiss(DismissReason::Clicked)
        );
    }

    /// Turning the wheel is the other way the user says they have moved on.
    #[test]
    fn a_scroll_past_the_threshold_reports_a_scroll() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(
            reducer.reduce(scroll(-200.0), options()),
            Decision::Dismiss(DismissReason::Scrolled)
        );
        assert_eq!(
            reducer.reduce(scroll(200.0), options()),
            Decision::Dismiss(DismissReason::Scrolled)
        );
    }

    /// A trackpad emits scroll events for the faintest touch, and dismissing on
    /// those would take the point away when someone brushed the surface on their
    /// way to the keyboard.
    #[test]
    fn a_small_scroll_is_not_the_user_moving_on() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(reducer.reduce(scroll(-4.0), options()), Decision::Quiet);
        assert_eq!(
            reducer.reduce(scroll(SCROLL_DISMISS_PX - 1.0), options()),
            Decision::Quiet
        );
    }

    /// A scroll says nothing about the middle button, and must not be mistaken
    /// for one.
    #[test]
    fn a_scroll_does_not_disturb_the_trigger_button() {
        let mut reducer = TriggerReducer::default();

        reducer.reduce(scroll(-500.0), options());

        assert_eq!(
            reducer.reduce(down(TRIGGER_BUTTON, 100.0, 100.0, 1_000), options()),
            Decision::Trigger(TriggerSource::MouseMiddle)
        );
    }

    #[test]
    fn the_configured_button_triggers_on_the_press() {
        let mut reducer = TriggerReducer::default();

        // On the press, not the release: nothing else has happened yet.
        assert_eq!(
            reducer.reduce(down(TRIGGER_BUTTON, 100.0, 100.0, 1_000), options()),
            Decision::Trigger(TriggerSource::MouseMiddle)
        );
    }

    #[test]
    fn another_mouse_button_is_reported_but_does_not_trigger() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(
            reducer.reduce(down(3, 100.0, 100.0, 1_000), options()),
            Decision::Ignored(Ignored::OtherButton)
        );
    }

    /// The whole reason the selection trigger exists: a drag long enough and slow
    /// enough to be a selection.
    #[test]
    fn a_selection_drag_triggers() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(
            drag(&mut reducer, (0.0, 0.0), (100.0, 0.0), 1_000, 200),
            Decision::Trigger(TriggerSource::MouseSelection)
        );
    }

    /// A drag that barely moved is a click, not a selection.
    #[test]
    fn a_drag_that_barely_moved_does_not_trigger() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(
            drag(&mut reducer, (0.0, 0.0), (4.0, 0.0), 1_000, 200),
            Decision::Quiet
        );
    }

    /// A flick: far enough, but far too quick to have been deliberate.
    #[test]
    fn a_drag_that_was_too_quick_does_not_trigger() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(
            drag(&mut reducer, (0.0, 0.0), (100.0, 0.0), 1_000, 20),
            Decision::Quiet
        );
    }

    /// The reason a trackpad user can use this app at all: two quick clicks in
    /// the same place select a word.
    #[test]
    fn a_double_click_triggers() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(click(&mut reducer, 100.0, 100.0, 1_000), Decision::Quiet);
        assert_eq!(
            click(&mut reducer, 100.0, 100.0, 1_250),
            Decision::Trigger(TriggerSource::MouseSelection)
        );
    }

    #[test]
    fn two_slow_clicks_are_not_a_double_click() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(click(&mut reducer, 100.0, 100.0, 1_000), Decision::Quiet);
        assert_eq!(click(&mut reducer, 100.0, 100.0, 2_000), Decision::Quiet);
    }

    #[test]
    fn two_clicks_in_different_places_are_not_a_double_click() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(click(&mut reducer, 100.0, 100.0, 1_000), Decision::Quiet);
        assert_eq!(click(&mut reducer, 300.0, 300.0, 1_050), Decision::Quiet);
    }

    /// A triple click selects a paragraph; without the cooldown it would be a
    /// third translation of nearly the same words.
    #[test]
    fn the_cooldown_suppresses_a_third_click() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(click(&mut reducer, 100.0, 100.0, 1_000), Decision::Quiet);
        assert_eq!(
            click(&mut reducer, 100.0, 100.0, 1_250),
            Decision::Trigger(TriggerSource::MouseSelection)
        );
        assert_eq!(
            click(&mut reducer, 100.0, 100.0, 1_400),
            Decision::Ignored(Ignored::Cooldown)
        );
    }

    #[test]
    fn the_cooldown_expires() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(
            drag(&mut reducer, (0.0, 0.0), (100.0, 0.0), 1_000, 200),
            Decision::Trigger(TriggerSource::MouseSelection)
        );
        assert_eq!(
            drag(&mut reducer, (0.0, 0.0), (100.0, 0.0), 2_000, 200),
            Decision::Trigger(TriggerSource::MouseSelection)
        );
    }

    /// With the toggle off a gesture is still recognised, and the reason it did
    /// nothing is reportable — otherwise the setting looks like a broken app.
    #[test]
    fn a_gesture_with_the_selection_trigger_off_says_so() {
        let mut reducer = TriggerReducer::default();
        let off = TriggerOptions {
            selection_trigger_enabled: false,
            ..options()
        };

        reducer.reduce(down(0, 0.0, 0.0, 1_000), off);
        assert_eq!(
            reducer.reduce(up(100.0, 0.0, 1_200), off),
            Decision::Ignored(Ignored::SelectionTriggerDisabled)
        );
    }

    /// Switching the trigger off must not stop the middle button working.
    #[test]
    fn the_middle_button_works_with_the_selection_trigger_off() {
        let mut reducer = TriggerReducer::default();
        let off = TriggerOptions {
            selection_trigger_enabled: false,
            ..options()
        };

        assert_eq!(
            reducer.reduce(down(TRIGGER_BUTTON, 100.0, 100.0, 1_000), off),
            Decision::Trigger(TriggerSource::MouseMiddle)
        );
    }

    /// A release with no press in front of it cannot be measured. It is reported
    /// rather than guessed at, because it means the tap was rebuilt mid-gesture.
    #[test]
    fn a_release_without_a_press_is_reported() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(
            reducer.reduce(up(100.0, 100.0, 1_000), options()),
            Decision::Ignored(Ignored::PressNotSeen)
        );
    }

    /// A press must not pair with a release from a different gesture. Otherwise a
    /// click after an interrupted drag would be measured against the stale press
    /// and could look like a drag.
    #[test]
    fn a_second_press_replaces_the_first() {
        let mut reducer = TriggerReducer::default();

        reducer.reduce(down(0, 0.0, 0.0, 1_000), options());
        reducer.reduce(down(0, 500.0, 500.0, 1_100), options());


        assert_eq!(
            reducer.reduce(up(500.0, 500.0, 1_200), options()),
            Decision::Quiet,
            "the release must be measured from the press it belongs to"
        );
    }

    /// A drag is not a click, so a click landing where the drag ended is not the
    /// second half of a double click — it is the first click of a new one.
    #[test]
    fn a_click_after_a_drag_is_not_a_double_click() {
        let mut reducer = TriggerReducer::default();

        drag(&mut reducer, (0.0, 0.0), (100.0, 0.0), 1_000, 200);
        assert_eq!(click(&mut reducer, 100.0, 0.0, 1_300), Decision::Quiet);
    }

    /// And a drag in between breaks a sequence that would otherwise pair up.
    #[test]
    fn a_drag_breaks_a_double_click_sequence() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(click(&mut reducer, 100.0, 100.0, 1_000), Decision::Quiet);
        drag(&mut reducer, (100.0, 100.0), (400.0, 100.0), 1_100, 300);
        assert_eq!(click(&mut reducer, 400.0, 100.0, 1_500), Decision::Quiet);
    }

    /// A flick is neither a drag nor a click: too fast to be deliberate, but it
    /// still moved, so it must not set up a double click either.
    #[test]
    fn a_flick_is_not_a_click() {
        let mut reducer = TriggerReducer::default();

        assert_eq!(
            drag(&mut reducer, (0.0, 0.0), (100.0, 0.0), 1_000, 20),
            Decision::Quiet
        );
        assert_eq!(click(&mut reducer, 100.0, 0.0, 1_100), Decision::Quiet);
    }

    /// Every ignored reason has to read as a sentence, because it is shown as one.
    #[test]
    fn every_ignored_reason_describes_itself() {
        for reason in [
            Ignored::OtherButton,
            Ignored::PressNotSeen,
            Ignored::SelectionTriggerDisabled,
            Ignored::Cooldown,
        ] {
            let description = reason.describe();
            assert!(!description.is_empty());
            assert!(
                !description.ends_with('。') && !description.ends_with('.'),
                "these are used mid-sentence: {description}"
            );
        }
    }

    /// The two thresholds have to stay ordered: a click must not be able to be
    /// mistaken for a drag, or double clicks would never be detected.
    #[test]
    fn the_double_click_slop_is_below_the_drag_threshold() {
        assert!(DOUBLE_CLICK_SLOP_PX < SELECTION_DRAG_MIN_DISTANCE_PX);
    }

    /// And the cooldown has to outlast a double click, or the second half of one
    /// would be suppressed.
    #[test]
    fn the_cooldown_outlasts_a_double_click() {
        assert!(SELECTION_TRIGGER_COOLDOWN_MS > DOUBLE_CLICK_INTERVAL_MS);
    }
}
