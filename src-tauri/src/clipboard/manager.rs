use arboard::Clipboard;
use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

pub struct ClipboardManager {
    capture_lock: tokio::sync::Mutex<()>,
}

impl ClipboardManager {
    pub fn new() -> Self {
        Self {
            capture_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn capture_selected_text(&self) -> Option<String> {
        let _guard = self.capture_lock.lock().await;
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
        eprintln!(
            "[clipboard] Original clipboard saved ({} chars)",
            original_clipboard.len()
        );

        let sentinel = format!("__fk_trans_capture_{}__", Uuid::new_v4());
        if !Self::set_clipboard_text(&sentinel) {
            eprintln!("[clipboard] Failed to write sentinel");
            return None;
        }

        // Simulate Cmd+C — create Enigo on demand (not Send-safe to store)
        {
            eprintln!("[clipboard] Simulating Cmd+C...");
            let mut enigo = match Enigo::new(&Settings::default()) {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("[clipboard] Failed to create Enigo: {}", e);
                    Self::restore_clipboard(&original_clipboard);
                    return None;
                }
            };
            let _ = enigo.key(Key::Meta, Press);
            let _ = enigo.key(Key::Unicode('c'), Click);
            let _ = enigo.key(Key::Meta, Release);
        }
        eprintln!("[clipboard] Cmd+C sent, waiting for clipboard update...");

        // Wait until the target app overwrites the sentinel, then restore the user's clipboard.
        let mut text = sentinel.clone();
        for _ in 0..12 {
            sleep(Duration::from_millis(50)).await;
            text = Self::get_clipboard_text().unwrap_or_default();
            if text != sentinel {
                break;
            }
        }
        eprintln!("[clipboard] Read clipboard: {} chars", text.len());

        Self::restore_clipboard(&original_clipboard);

        // Validate
        if text == sentinel {
            eprintln!("[clipboard] Clipboard did not change after Cmd+C, skipping");
            return None;
        }

        if text.trim().len() < 2 {
            eprintln!(
                "[clipboard] Text too short ({}), skipping",
                text.trim().len()
            );
            return None;
        }

        let preview: String = text.chars().take(80).collect();
        eprintln!("[clipboard] Captured: {:?}", preview);
        Some(text)
    }

    fn get_clipboard_text() -> Option<String> {
        let mut cb = Clipboard::new().ok()?;
        cb.get_text().ok()
    }

    fn set_clipboard_text(text: &str) -> bool {
        match Clipboard::new() {
            Ok(mut cb) => cb.set_text(text).is_ok(),
            Err(e) => {
                eprintln!("[clipboard] Failed to access clipboard: {}", e);
                false
            }
        }
    }

    fn restore_clipboard(original: &str) {
        if !Self::set_clipboard_text(original) {
            eprintln!("[clipboard] Failed to restore original clipboard");
        }
    }
}
