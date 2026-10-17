//! Where to put the trigger point and the result panel.
//!
//! Pure geometry, in the global display space Accessibility reports in: origin at
//! the top-left of the primary display, `y` increasing downwards, measured in
//! points. Tauri's `LogicalPosition` uses the same space, so nothing here
//! converts units — the conversion to backing pixels happens once, in the one
//! place that calls `set_position`.
//!
//! Kept separate from the window code because it is the part worth testing: a
//! point placed off-screen is invisible, and the flip-to-the-other-side rules are
//! easy to get backwards.

use crate::macos::ax::ScreenRect;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// Keep a point far enough inside `area` that a `size`-sized box at that point is
/// fully visible.
fn clamp_into(point: Point, size: Size, area: ScreenRect, margin: f64) -> Point {
    let min_x = area.x + margin;
    let min_y = area.y + margin;
    // `max(min_x)` guards a work area smaller than the box, where otherwise the
    // clamp would invert and push the box back off-screen.
    let max_x = (area.right() - margin - size.width).max(min_x);
    let max_y = (area.bottom() - margin - size.height).max(min_y);

    Point {
        x: point.x.clamp(min_x, max_x),
        y: point.y.clamp(min_y, max_y),
    }
}

/// The top-left of a small box to be shown near a selection.
///
/// Prefers outside the bottom-right corner. Each axis flips independently when
/// the preferred side would leave the work area — a point above the selection is
/// worse than one below it only if the one below is off-screen. Finally the
/// result is clamped, so a selection taller than its screen still gets a visible
/// point.
pub fn trigger_point(
    selection: ScreenRect,
    dot: Size,
    work_area: ScreenRect,
    gap: f64,
    margin: f64,
) -> Point {
    let mut x = selection.right() + gap;
    if x + dot.width > work_area.right() - margin {
        x = selection.x - gap - dot.width;
    }

    let mut y = selection.bottom() + gap;
    if y + dot.height > work_area.bottom() - margin {
        y = selection.y - gap - dot.height;
    }

    clamp_into(Point { x, y }, dot, work_area, margin)
}

/// The top-left of the expanded panel, given where the trigger point is.
///
/// Opens below-right of the point, flipping each axis when that would leave the
/// work area, then clamped. The panel is placed from the point rather than from
/// the selection so the two always appear together.
pub fn panel_origin(
    anchor: Point,
    panel: Size,
    work_area: ScreenRect,
    gap: f64,
    margin: f64,
) -> Point {
    let mut x = anchor.x;
    if x + panel.width > work_area.right() - margin {
        x = anchor.x - panel.width + 0.0;
    }

    let mut y = anchor.y + gap;
    if y + panel.height > work_area.bottom() - margin {
        y = anchor.y - gap - panel.height;
    }

    clamp_into(Point { x, y }, panel, work_area, margin)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1512x982 display with a 25pt menu bar, in points.
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

    const DOT: Size = Size {
        width: 22.0,
        height: 22.0,
    };
    const GAP: f64 = 6.0;
    const MARGIN: f64 = 8.0;

    #[test]
    fn a_comfortable_selection_gets_the_point_at_its_bottom_right() {
        let point = trigger_point(selection(100.0, 200.0, 120.0, 18.0), DOT, work_area(), GAP, MARGIN);

        // 100 + 120 + 6, and 200 + 18 + 6.
        assert_eq!(point, Point { x: 226.0, y: 224.0 });
    }

    /// Near the right edge the point goes to the left of the selection instead of
    /// off the screen.
    #[test]
    fn the_point_flips_left_near_the_right_edge() {
        let selection = selection(1400.0, 200.0, 100.0, 18.0);
        let point = trigger_point(selection, DOT, work_area(), GAP, MARGIN);

        assert!(
            point.x + DOT.width <= work_area().right() - MARGIN,
            "the point must fit: {point:?}"
        );
        assert_eq!(point.x, 1400.0 - GAP - DOT.width);
    }

    /// And above rather than below near the bottom edge.
    #[test]
    fn the_point_flips_up_near_the_bottom_edge() {
        let selection = selection(100.0, 950.0, 120.0, 18.0);
        let point = trigger_point(selection, DOT, work_area(), GAP, MARGIN);

        assert!(
            point.y + DOT.height <= work_area().bottom() - MARGIN,
            "the point must fit: {point:?}"
        );
        assert_eq!(point.y, 950.0 - GAP - DOT.height);
    }

    #[test]
    fn both_axes_can_flip_at_once() {
        let selection = selection(1450.0, 960.0, 55.0, 16.0);
        let point = trigger_point(selection, DOT, work_area(), GAP, MARGIN);

        assert!(point.x + DOT.width <= work_area().right() - MARGIN, "{point:?}");
        assert!(point.y + DOT.height <= work_area().bottom() - MARGIN, "{point:?}");
    }

    /// Flipping can still land outside when the selection is pinned to the far
    /// edge; the clamp is what guarantees visibility.
    #[test]
    fn a_selection_at_the_very_corner_still_yields_a_visible_point() {
        let point = trigger_point(
            selection(1500.0, 975.0, 12.0, 7.0),
            DOT,
            work_area(),
            GAP,
            MARGIN,
        );

        assert!(point.x >= work_area().x + MARGIN, "{point:?}");
        assert!(point.y >= work_area().y + MARGIN, "{point:?}");
        assert!(point.x + DOT.width <= work_area().right() - MARGIN, "{point:?}");
        assert!(point.y + DOT.height <= work_area().bottom() - MARGIN, "{point:?}");
    }

    /// A display to the left of the primary one has negative coordinates. Clamping
    /// must respect the work area's own origin rather than assuming zero.
    #[test]
    fn a_negative_origin_display_is_clamped_to_its_own_work_area() {
        let left_display = ScreenRect {
            x: -1512.0,
            y: 25.0,
            width: 1512.0,
            height: 957.0,
        };
        let point = trigger_point(
            selection(-120.0, 200.0, 100.0, 18.0),
            DOT,
            left_display,
            GAP,
            MARGIN,
        );

        assert!(point.x >= left_display.x + MARGIN, "{point:?}");
        assert!(point.x + DOT.width <= left_display.right() - MARGIN, "{point:?}");
    }

    #[test]
    fn the_panel_opens_below_the_point() {
        let panel = Size {
            width: 376.0,
            height: 200.0,
        };
        let origin = panel_origin(Point { x: 226.0, y: 224.0 }, panel, work_area(), GAP, MARGIN);

        assert_eq!(origin.x, 226.0);
        assert_eq!(origin.y, 224.0 + GAP);
    }

    #[test]
    fn the_panel_moves_above_the_point_when_there_is_no_room_below() {
        let panel = Size {
            width: 376.0,
            height: 300.0,
        };
        let anchor = Point { x: 226.0, y: 900.0 };
        let origin = panel_origin(anchor, panel, work_area(), GAP, MARGIN);

        assert!(origin.y + panel.height <= work_area().bottom() - MARGIN, "{origin:?}");
        assert_eq!(origin.y, 900.0 - GAP - panel.height);
    }

    #[test]
    fn the_panel_is_kept_inside_the_work_area_horizontally() {
        let panel = Size {
            width: 376.0,
            height: 200.0,
        };
        let origin = panel_origin(Point { x: 1400.0, y: 200.0 }, panel, work_area(), GAP, MARGIN);

        assert!(origin.x >= work_area().x + MARGIN, "{origin:?}");
        assert!(origin.x + panel.width <= work_area().right() - MARGIN, "{origin:?}");
    }

    /// A work area smaller than the box must not produce an inverted clamp range.
    #[test]
    fn a_tiny_work_area_does_not_invert_the_clamp() {
        let tiny = ScreenRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let point = trigger_point(selection(0.0, 0.0, 5.0, 5.0), DOT, tiny, GAP, MARGIN);

        assert!(point.x >= 0.0);
        assert!(point.y >= 0.0);
    }
}
