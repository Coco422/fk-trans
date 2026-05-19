use rdev::{Button, Event, EventType, listen};
use std::sync::mpsc::Sender;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

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

        std::thread::spawn(move || {
            let callback = move |event: Event| {
                if !running.load(Ordering::SeqCst) {
                    return;
                }

                if let EventType::ButtonPress(Button::Middle) = event.event_type {
                    let _ = tx.send(());
                }
            };

            if let Err(e) = listen(callback) {
                log::error!("Mouse listener error: {:?}", e);
            }
        });
    }

    #[allow(dead_code)]
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
