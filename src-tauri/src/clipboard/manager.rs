use arboard::Clipboard;
use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use std::sync::Mutex;
use tokio::time::{Duration, sleep};

pub struct ClipboardManager {
    previous_text: Mutex<String>,
}

impl ClipboardManager {
    pub fn new() -> Self {
        Self {
            previous_text: Mutex::new(String::new()),
        }
    }

    pub async fn capture_selected_text(&self) -> Option<String> {
        // Save current clipboard content
        let original_clipboard = {
            let mut cb = Clipboard::new().ok()?;
            cb.get_text().unwrap_or_default()
        };

        // Simulate Cmd+C — create Enigo on demand (not Send-safe to store)
        {
            let mut enigo = Enigo::new(&Settings::default()).ok()?;
            let _ = enigo.key(Key::Meta, Press);
            let _ = enigo.key(Key::Unicode('c'), Click);
            let _ = enigo.key(Key::Meta, Release);
        }

        // Wait for clipboard to update
        sleep(Duration::from_millis(100)).await;

        // Read new clipboard
        let text = {
            let mut cb = Clipboard::new().ok()?;
            cb.get_text().unwrap_or_default()
        };

        // Validate
        if text.len() < 2 {
            return None;
        }

        // Debounce: same text as last capture
        {
            let previous = self.previous_text.lock().unwrap();
            if *previous == text {
                return None;
            }
        }

        // Update previous text
        {
            let mut previous = self.previous_text.lock().unwrap();
            *previous = text.clone();
        }

        // Restore original clipboard
        let original = original_clipboard.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(200)).await;
            if let Ok(mut cb) = Clipboard::new() {
                let _ = cb.set_text(&original);
            }
        });

        Some(text)
    }
}
