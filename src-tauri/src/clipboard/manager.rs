use arboard::Clipboard;
#[cfg(not(target_os = "macos"))]
use std::panic::{catch_unwind, AssertUnwindSafe};
use tokio::time::{sleep, timeout, Duration};
use uuid::Uuid;

const COPY_SHORTCUT_TIMEOUT: Duration = Duration::from_millis(1_500);
const CLIPBOARD_UPDATE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const CLIPBOARD_UPDATE_POLL_ATTEMPTS: usize = 24;
#[cfg(target_os = "macos")]
const MACOS_ANSI_C_KEYCODE: u16 = 0x08;

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CopyKeyEvent {
    keycode: u16,
    keydown: bool,
    command_flag: bool,
}

pub struct ClipboardManager {
    capture_lock: tokio::sync::Mutex<()>,
}

#[cfg(target_os = "macos")]
fn macos_copy_key_sequence() -> [CopyKeyEvent; 4] {
    use core_graphics::event::KeyCode;

    [
        CopyKeyEvent {
            keycode: KeyCode::COMMAND,
            keydown: true,
            command_flag: true,
        },
        CopyKeyEvent {
            keycode: MACOS_ANSI_C_KEYCODE,
            keydown: true,
            command_flag: true,
        },
        CopyKeyEvent {
            keycode: MACOS_ANSI_C_KEYCODE,
            keydown: false,
            command_flag: true,
        },
        CopyKeyEvent {
            keycode: KeyCode::COMMAND,
            keydown: false,
            command_flag: false,
        },
    ]
}

impl ClipboardManager {
    pub fn new() -> Self {
        Self {
            capture_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn capture_selected_text(&self) -> Option<String> {
        log::debug!("[clipboard] Waiting for capture lock");
        let _guard = self.capture_lock.lock().await;
        log::info!("[clipboard] Starting capture");

        // Save current clipboard content
        let original_clipboard = {
            let mut cb = match Clipboard::new() {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("[clipboard] Failed to access clipboard: {}", e);
                    return None;
                }
            };
            cb.get_text().unwrap_or_default()
        };
        log::info!(
            "[clipboard] Original clipboard saved ({} chars)",
            original_clipboard.len()
        );

        let sentinel = format!("__fk_trans_capture_{}__", Uuid::new_v4());
        log::debug!("[clipboard] Writing clipboard sentinel");
        if !Self::set_clipboard_text(&sentinel) {
            log::warn!("[clipboard] Failed to write sentinel");
            return None;
        }

        log::info!("[clipboard] Simulating Cmd+C");
        match timeout(COPY_SHORTCUT_TIMEOUT, Self::send_copy_shortcut()).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                log::warn!("[clipboard] Cmd+C simulation failed: {}", e);
                Self::restore_clipboard(&original_clipboard);
                return None;
            }
            Err(_) => {
                log::error!(
                    "[clipboard] Cmd+C simulation timed out after {} ms",
                    COPY_SHORTCUT_TIMEOUT.as_millis()
                );
                Self::restore_clipboard(&original_clipboard);
                return None;
            }
        }
        log::info!("[clipboard] Cmd+C sent, waiting for clipboard update");

        // Wait until the target app overwrites the sentinel, then restore the user's clipboard.
        let mut text = sentinel.clone();
        for _ in 0..CLIPBOARD_UPDATE_POLL_ATTEMPTS {
            sleep(CLIPBOARD_UPDATE_POLL_INTERVAL).await;
            text = Self::get_clipboard_text().unwrap_or_default();
            if text != sentinel {
                break;
            }
        }
        log::info!("[clipboard] Read clipboard: {} chars", text.len());

        Self::restore_clipboard(&original_clipboard);
        log::debug!("[clipboard] Original clipboard restored");

        // Validate
        if text == sentinel {
            log::warn!("[clipboard] Clipboard did not change after Cmd+C, skipping");
            return None;
        }

        if text.trim().len() < 2 {
            log::warn!(
                "[clipboard] Text too short ({}), skipping",
                text.trim().len()
            );
            return None;
        }

        log::info!(
            "[clipboard] Captured selected text length: {} chars",
            text.chars().count()
        );
        Some(text)
    }

    async fn send_copy_shortcut() -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            Self::send_copy_shortcut_macos()
        }

        #[cfg(not(target_os = "macos"))]
        {
            Self::send_copy_shortcut_enigo().await
        }
    }

    #[cfg(target_os = "macos")]
    fn send_copy_shortcut_macos() -> Result<(), String> {
        use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

        log::debug!("[clipboard] Sending Cmd+C via CoreGraphics keycodes");

        for event in macos_copy_key_sequence() {
            let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
                .map_err(|_| "Failed to create CoreGraphics event source".to_string())?;
            let key_event = CGEvent::new_keyboard_event(source, event.keycode, event.keydown)
                .map_err(|_| "Failed to create CoreGraphics keyboard event".to_string())?;
            key_event.set_flags(if event.command_flag {
                CGEventFlags::CGEventFlagCommand
            } else {
                CGEventFlags::CGEventFlagNull
            });
            key_event.post(CGEventTapLocation::HID);
        }

        log::debug!("[clipboard] CoreGraphics Cmd+C completed");
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    async fn send_copy_shortcut_enigo() -> Result<(), String> {
        use enigo::{
            Direction::{Click, Press, Release},
            Enigo, Key, Keyboard, Settings,
        };

        tokio::task::spawn_blocking(|| {
            catch_unwind(AssertUnwindSafe(|| {
                log::debug!("[clipboard] Creating Enigo");
                let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;

                log::debug!("[clipboard] Pressing Meta");
                enigo.key(Key::Meta, Press).map_err(|e| e.to_string())?;
                log::debug!("[clipboard] Clicking C");
                let click_result = enigo.key(Key::Unicode('c'), Click);
                log::debug!("[clipboard] Releasing Meta");
                let release_result = enigo.key(Key::Meta, Release);

                click_result.map_err(|e| e.to_string())?;
                release_result.map_err(|e| e.to_string())?;
                log::debug!("[clipboard] Enigo Cmd+C completed");
                Ok(())
            }))
            .map_err(|_| "Enigo panicked while simulating Cmd+C".to_string())?
        })
        .await
        .map_err(|e| format!("Cmd+C worker failed: {}", e))?
    }

    fn get_clipboard_text() -> Option<String> {
        let mut cb = Clipboard::new().ok()?;
        cb.get_text().ok()
    }

    fn set_clipboard_text(text: &str) -> bool {
        match Clipboard::new() {
            Ok(mut cb) => cb.set_text(text).is_ok(),
            Err(e) => {
                log::warn!("[clipboard] Failed to access clipboard: {}", e);
                false
            }
        }
    }

    fn restore_clipboard(original: &str) {
        if !Self::set_clipboard_text(original) {
            log::warn!("[clipboard] Failed to restore original clipboard");
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_copy_sequence_uses_raw_command_c_keycodes() {
        use core_graphics::event::KeyCode;

        let events = macos_copy_key_sequence();

        assert_eq!(
            events,
            [
                CopyKeyEvent {
                    keycode: KeyCode::COMMAND,
                    keydown: true,
                    command_flag: true,
                },
                CopyKeyEvent {
                    keycode: MACOS_ANSI_C_KEYCODE,
                    keydown: true,
                    command_flag: true,
                },
                CopyKeyEvent {
                    keycode: MACOS_ANSI_C_KEYCODE,
                    keydown: false,
                    command_flag: true,
                },
                CopyKeyEvent {
                    keycode: KeyCode::COMMAND,
                    keydown: false,
                    command_flag: false,
                },
            ]
        );
    }
}
