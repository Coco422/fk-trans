use std::sync::mpsc::Sender;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub struct MouseListener {
    running: Arc<AtomicBool>,
}

impl MouseListener {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&self, tx: Sender<()>) {
        let running = self.running.clone();
        running.store(true, Ordering::SeqCst);

        start_platform_listener(running, tx);
    }

    #[allow(dead_code)]
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

#[cfg(target_os = "macos")]
fn start_platform_listener(running: Arc<AtomicBool>, tx: Sender<()>) {
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
        EventField,
    };

    std::thread::spawn(move || {
        eprintln!("[mouse] CoreGraphics listener started, waiting for middle-click...");

        let tap = CGEventTap::new(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            vec![CGEventType::OtherMouseDown],
            move |_proxy, event_type, event| {
                if running.load(Ordering::SeqCst)
                    && matches!(event_type, CGEventType::OtherMouseDown)
                    && event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER) == 2
                {
                    eprintln!("[mouse] *** Middle-click detected! ***");
                    log::info!("[mouse] Middle-click detected, triggering translation");
                    let _ = tx.send(());
                }
                None
            },
        );

        let Ok(tap) = tap else {
            eprintln!("[mouse] Listener FATAL error: failed to create event tap — check Accessibility permissions!");
            log::error!("Mouse listener error: failed to create event tap (check Accessibility permissions)");
            return;
        };

        let current_loop = CFRunLoop::get_current();
        let Ok(loop_source) = tap.mach_port.create_runloop_source(0) else {
            eprintln!("[mouse] Listener FATAL error: failed to create run loop source");
            log::error!("Mouse listener error: failed to create run loop source");
            return;
        };

        current_loop.add_source(&loop_source, unsafe { kCFRunLoopCommonModes });
        tap.enable();
        CFRunLoop::run_current();
    });
}

#[cfg(not(target_os = "macos"))]
fn start_platform_listener(running: Arc<AtomicBool>, tx: Sender<()>) {
    use rdev::{listen, Button, Event, EventType};

    std::thread::spawn(move || {
        eprintln!("[mouse] Listener thread started, waiting for middle-click...");
        let callback = move |event: Event| {
            if !running.load(Ordering::SeqCst) {
                return;
            }

            if let EventType::ButtonPress(Button::Middle) = event.event_type {
                eprintln!("[mouse] *** Middle-click detected! ***");
                log::info!("[mouse] Middle-click detected, triggering translation");
                let _ = tx.send(());
            }
        };

        if let Err(e) = listen(callback) {
            eprintln!(
                "[mouse] Listener FATAL error: {:?} — check Accessibility permissions!",
                e
            );
            log::error!(
                "Mouse listener error (check Accessibility permissions): {:?}",
                e
            );
        }
    });
}
