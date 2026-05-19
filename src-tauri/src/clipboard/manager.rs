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
        eprintln!("[clipboard] Starting capture...");

        // Save current clipboard content
        let original_clipboard = {
            let mut cb = match Clipboard::new() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[clipboard] Failed to access clipboard: {}", e);
                    return None;
                }
            };
            cb.get_text().unwrap_or_default()
        };
        eprintln!("[clipboard] Original clipboard saved ({} chars)", original_clipboard.len());

        // Simulate Cmd+C — create Enigo on demand (not Send-safe to store)
        {
            eprintln!("[clipboard] Simulating Cmd+C...");
            let mut enigo = match Enigo::new(&Settings::default()) {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("[clipboard] Failed to create Enigo: {}", e);
                    return None;
                }
            };
            let _ = enigo.key(Key::Meta, Press);
            let _ = enigo.key(Key::Unicode('c'), Click);
            let _ = enigo.key(Key::Meta, Release);
        }
        eprintln!("[clipboard] Cmd+C sent, waiting 150ms...");

        // Wait for clipboard to update
        sleep(Duration::from_millis(150)).await;

        // Read new clipboard
        let text = {
            let mut cb = match Clipboard::new() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[clipboard] Failed to read clipboard: {}", e);
                    return None;
                }
            };
            cb.get_text().unwrap_or_default()
        };
        eprintln!("[clipboard] Read clipboard: {} chars", text.len());

        // Validate
        if text.len() < 2 {
            eprintln!("[clipboard] Text too short ({}), skipping", text.len());
            return None;
        }

        // Debounce: same text as last capture
        {
            let previous = self.previous_text.lock().unwrap();
            if *previous == text {
                eprintln!("[clipboard] Same as previous capture, skipping");
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

        eprintln!("[clipboard] Captured: {:?}", &text[..text.len().min(80)]);
        Some(text)
    }
}
