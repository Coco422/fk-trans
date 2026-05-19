use core_graphics::display::CGDisplay;
use core_graphics::event::CGEvent;
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

#[derive(Debug, Clone, Copy)]
pub struct CursorPosition {
    pub x: f64,
    pub y: f64,
}

pub fn get_cursor_position() -> CursorPosition {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).unwrap();
    let event = CGEvent::new(source).unwrap();
    let point = event.location();

    // CGEvent Y is bottom-up, convert to top-down
    let screen_height = CGDisplay::main().pixels_high() as f64;
    let scale = CGDisplay::main().pixels_wide() as f64
        / CGDisplay::main().bounds().size.width;

    CursorPosition {
        x: point.x / scale,
        y: (screen_height - point.y) / scale,
    }
}
