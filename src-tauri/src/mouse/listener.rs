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

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

impl Drop for MouseListener {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "macos")]
fn start_platform_listener(running: Arc<AtomicBool>, tx: Sender<()>) {
    use crate::platform;
    use core_foundation::runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop};
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
        EventField,
    };
    use std::time::Duration;

    std::thread::spawn(move || {
        eprintln!("[mouse] CoreGraphics listener started, waiting for middle-click...");

        let mut prompted_for_accessibility = false;

        while running.load(Ordering::SeqCst) {
            if !platform::macos::check_accessibility_permissions() {
                if !prompted_for_accessibility {
                    prompted_for_accessibility = true;
                    log::warn!(
                        "[mouse] Accessibility permissions not granted. Requesting permission."
                    );
                    let _ = platform::macos::request_accessibility_permissions();
                }
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }

            prompted_for_accessibility = false;
            let restart_requested = Arc::new(AtomicBool::new(false));
            let callback_running = running.clone();
            let callback_tx = tx.clone();
            let callback_restart = restart_requested.clone();

            let tap = CGEventTap::new(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                vec![CGEventType::OtherMouseDown],
                move |_proxy, event_type, event| {
                    if matches!(
                        event_type,
                        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
                    ) {
                        log::warn!("[mouse] Event tap disabled by macOS; recreating listener");
                        callback_restart.store(true, Ordering::SeqCst);
                        CFRunLoop::get_current().stop();
                        return None;
                    }

                    if callback_running.load(Ordering::SeqCst)
                        && matches!(event_type, CGEventType::OtherMouseDown)
                        && event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER) == 2
                    {
                        eprintln!("[mouse] *** Middle-click detected! ***");
                        log::info!("[mouse] Middle-click detected, triggering translation");
                        let _ = callback_tx.send(());
                    }
                    None
                },
            );

            let Ok(tap) = tap else {
                log::error!("[mouse] Failed to create event tap. Will retry while app is running.");
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };

            let current_loop = CFRunLoop::get_current();
            let Ok(loop_source) = tap.mach_port.create_runloop_source(0) else {
                log::error!("[mouse] Failed to create event tap run loop source. Retrying.");
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };

            current_loop.add_source(&loop_source, unsafe { kCFRunLoopCommonModes });
            tap.enable();
            log::info!("[mouse] CoreGraphics event tap enabled");

            while running.load(Ordering::SeqCst) && !restart_requested.load(Ordering::SeqCst) {
                let _ = CFRunLoop::run_in_mode(
                    unsafe { kCFRunLoopDefaultMode },
                    Duration::from_millis(500),
                    false,
                );
            }

            if running.load(Ordering::SeqCst) {
                log::info!("[mouse] Restarting CoreGraphics event tap");
                std::thread::sleep(Duration::from_millis(250));
            }
        }

        log::info!("[mouse] CoreGraphics listener stopped");
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
