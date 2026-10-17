//! Reading the current selection by asking the frontmost app to copy it.
//!
//! This is the fallback strategy: it synthesises Cmd+C and reads the result from
//! the pasteboard. It is the most compatible way to get a selection, and also the
//! most invasive, so the invariants here matter:
//!
//! * The user's pasteboard is restored **whole**, on every exit path.
//! * The pasteboard is never cleared just to detect whether a copy happened —
//!   the system's `changeCount` already says that.

use super::pasteboard::{system_pasteboard, Pasteboard, RestoreOnDrop};
use std::sync::Arc;
use tokio::time::{sleep, timeout, Duration};

/// The shortest selection worth translating. A single character is almost always
/// an accidental click-drag rather than something the user meant to select.
const MIN_SELECTION_CHARS: usize = 2;

const MACOS_ANSI_C_KEYCODE: u16 = 0x08;

#[derive(Debug, Clone, Copy)]
pub struct CaptureTiming {
    pub copy_shortcut_timeout: Duration,
    pub poll_interval: Duration,
    pub poll_attempts: usize,
}

pub const DEFAULT_CAPTURE_TIMING: CaptureTiming = CaptureTiming {
    copy_shortcut_timeout: Duration::from_millis(1_500),
    poll_interval: Duration::from_millis(50),
    poll_attempts: 24,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CopyKeyEvent {
    keycode: u16,
    keydown: bool,
    command_flag: bool,
}

pub struct ClipboardManager {
    pasteboard: Arc<dyn Pasteboard>,
    timing: CaptureTiming,
    capture_lock: tokio::sync::Mutex<()>,
}

impl Default for ClipboardManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardManager {
    pub fn new() -> Self {
        Self::with_pasteboard(system_pasteboard(), DEFAULT_CAPTURE_TIMING)
    }

    pub fn with_pasteboard(pasteboard: Arc<dyn Pasteboard>, timing: CaptureTiming) -> Self {
        Self {
            pasteboard,
            timing,
            capture_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn capture_selected_text(&self) -> Option<String> {
        self.capture_with(async { Self::send_copy_shortcut() }).await
    }

    /// The capture flow with the synthesised keystroke injected, so the
    /// restore-on-every-path behaviour can be exercised without posting real
    /// events to whatever app happens to be frontmost.
    async fn capture_with(
        &self,
        send_copy: impl std::future::Future<Output = Result<(), String>>,
    ) -> Option<String> {
        log::debug!("[clipboard] Waiting for capture lock");
        let _guard = self.capture_lock.lock().await;
        log::info!("[clipboard] Starting capture");

        let pasteboard = self.pasteboard.as_ref();

        let before = pasteboard.change_count();
        let Some(snapshot) = pasteboard.snapshot() else {
            log::warn!("[clipboard] Could not read the pasteboard, skipping");
            return None;
        };
        log::info!(
            "[clipboard] Backed up {} pasteboard item(s)",
            snapshot.items.len()
        );

        // From here on the clipboard is the app's to overwrite, so it goes back
        // exactly as found no matter how this function exits.
        let _restore = RestoreOnDrop::new(pasteboard, snapshot);

        match timeout(self.timing.copy_shortcut_timeout, send_copy).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                log::warn!("[clipboard] Cmd+C simulation failed: {}", e);
                return None;
            }
            Err(_) => {
                log::error!(
                    "[clipboard] Cmd+C simulation timed out after {} ms",
                    self.timing.copy_shortcut_timeout.as_millis()
                );
                return None;
            }
        }
        log::info!("[clipboard] Cmd+C sent, waiting for pasteboard update");

        // The system bumps `changeCount` on every write, so an unchanged count
        // means the app had nothing to copy — no need to have pre-cleared the
        // clipboard with a sentinel to find that out.
        let mut captured = None;
        for _ in 0..self.timing.poll_attempts {
            sleep(self.timing.poll_interval).await;
            if pasteboard.change_count() != before {
                captured = pasteboard.read_text();
                break;
            }
        }

        let Some(text) = captured else {
            log::warn!("[clipboard] Pasteboard did not change after Cmd+C, skipping");
            return None;
        };
        log::info!("[clipboard] Read pasteboard: {} chars", text.chars().count());

        if text.trim().chars().count() < MIN_SELECTION_CHARS {
            log::warn!(
                "[clipboard] Selection too short ({} chars), skipping",
                text.trim().chars().count()
            );
            return None;
        }

        log::info!(
            "[clipboard] Captured selected text length: {} chars",
            text.chars().count()
        );
        Some(text)
    }

    /// Synthesise Cmd+C so the frontmost app puts its selection on the
    /// pasteboard for us to read.
    fn send_copy_shortcut() -> Result<(), String> {
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
}

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

#[cfg(test)]
mod tests {
    use super::*;

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

    use crate::clipboard::pasteboard::{
        Pasteboard, PasteboardItem, PasteboardSnapshot, RestoreOnDrop, UTF8_TEXT_TYPE,
    };
    use std::sync::{Arc, Mutex};

    const IMAGE_TYPE: &str = "public.png";

    fn text_item(value: &str) -> PasteboardItem {
        PasteboardItem {
            entries: vec![(UTF8_TEXT_TYPE.to_string(), value.as_bytes().to_vec())],
        }
    }

    fn image_item() -> PasteboardItem {
        PasteboardItem {
            entries: vec![(IMAGE_TYPE.to_string(), vec![0x89, b'P', b'N', b'G'])],
        }
    }

    #[derive(Default)]
    struct FakeState {
        change_count: i64,
        items: Vec<PasteboardItem>,
        /// What the frontmost app copies when Cmd+C arrives. `None` models an app
        /// with nothing selected, which is the case the old code misreported.
        selection: Option<String>,
    }

    /// An in-memory pasteboard that can hold content which is *not* text — the
    /// exact situation the old text-only backup destroyed.
    struct FakePasteboard {
        state: Mutex<FakeState>,
    }

    impl FakePasteboard {
        fn holding(items: Vec<PasteboardItem>) -> Arc<Self> {
            Arc::new(Self {
                state: Mutex::new(FakeState {
                    change_count: 1,
                    items,
                    selection: None,
                }),
            })
        }

        fn with_selection(self: Arc<Self>, selection: &str) -> Arc<Self> {
            self.lock().selection = Some(selection.to_string());
            self
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
            self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
        }

        fn item_count(&self) -> usize {
            self.lock().items.len()
        }

        fn has_image(&self) -> bool {
            self.lock()
                .items
                .iter()
                .any(|item| item.entries.iter().any(|(t, _)| t == IMAGE_TYPE))
        }

        /// Stand in for the frontmost app handling Cmd+C.
        fn perform_copy(&self) {
            let mut state = self.lock();
            if let Some(selection) = state.selection.clone() {
                state.items = vec![text_item(&selection)];
                state.change_count += 1;
            }
        }
    }

    impl Pasteboard for FakePasteboard {
        fn change_count(&self) -> i64 {
            self.lock().change_count
        }

        fn snapshot(&self) -> Option<PasteboardSnapshot> {
            Some(PasteboardSnapshot {
                items: self.lock().items.clone(),
            })
        }

        fn restore(&self, snapshot: &PasteboardSnapshot) {
            let mut state = self.lock();
            state.items = snapshot.items.clone();
            state.change_count += 1;
        }

        fn read_text(&self) -> Option<String> {
            let state = self.lock();
            state.items.iter().find_map(|item| {
                item.entries
                    .iter()
                    .find(|(type_id, _)| type_id == UTF8_TEXT_TYPE)
                    .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
            })
        }
    }

    fn fast_timing() -> CaptureTiming {
        CaptureTiming {
            copy_shortcut_timeout: Duration::from_millis(200),
            poll_interval: Duration::from_millis(1),
            poll_attempts: 8,
        }
    }

    fn manager_for(pasteboard: Arc<FakePasteboard>) -> ClipboardManager {
        ClipboardManager::with_pasteboard(pasteboard, fast_timing())
    }

    /// The core regression: the user's clipboard held an image, and using
    /// middle-click translation must not destroy it. The old code backed up only
    /// `get_text()`, which is empty for an image, and then restored that empty
    /// string over the top.
    #[tokio::test]
    async fn capture_leaves_a_non_text_clipboard_intact() {
        let pasteboard = FakePasteboard::holding(vec![image_item(), text_item("copied before")])
            .with_selection("hello world");
        let manager = manager_for(pasteboard.clone());

        let copy_target = pasteboard.clone();
        let captured = manager
            .capture_with(async move {
                copy_target.perform_copy();
                Ok(())
            })
            .await;

        assert_eq!(captured.as_deref(), Some("hello world"));
        assert!(
            pasteboard.has_image(),
            "the image the user had copied must survive a translation"
        );
        assert_eq!(pasteboard.item_count(), 2, "both original items restored");
        assert_eq!(pasteboard.read_text().as_deref(), Some("copied before"));
    }

    #[tokio::test]
    async fn capture_restores_the_clipboard_when_nothing_is_selected() {
        let pasteboard = FakePasteboard::holding(vec![image_item(), text_item("keep me")]);
        let manager = manager_for(pasteboard.clone());

        // No selection: the app does not touch the pasteboard, so the change
        // count stays put and the capture reports nothing.
        let captured = manager.capture_with(async { Ok(()) }).await;

        assert_eq!(captured, None);
        assert!(pasteboard.has_image());
        assert_eq!(pasteboard.read_text().as_deref(), Some("keep me"));
    }

    #[tokio::test]
    async fn capture_restores_the_clipboard_when_the_copy_shortcut_fails() {
        let pasteboard = FakePasteboard::holding(vec![image_item()]);
        let manager = manager_for(pasteboard.clone());

        let captured = manager
            .capture_with(async { Err("no accessibility permission".to_string()) })
            .await;

        assert_eq!(captured, None);
        assert!(
            pasteboard.has_image(),
            "a failed capture must not cost the user their clipboard"
        );
    }

    /// A slow or hung copy must not leave the clipboard overwritten either.
    #[tokio::test]
    async fn capture_restores_the_clipboard_when_the_copy_shortcut_times_out() {
        let pasteboard = FakePasteboard::holding(vec![image_item()]);
        let manager = manager_for(pasteboard.clone());

        let captured = manager
            .capture_with(async {
                sleep(Duration::from_secs(5)).await;
                Ok(())
            })
            .await;

        assert_eq!(captured, None);
        assert!(pasteboard.has_image());
    }

    #[tokio::test]
    async fn capture_rejects_a_one_character_selection() {
        let pasteboard = FakePasteboard::holding(vec![text_item("before")]).with_selection("x");
        let manager = manager_for(pasteboard.clone());

        let copy_target = pasteboard.clone();
        let captured = manager
            .capture_with(async move {
                copy_target.perform_copy();
                Ok(())
            })
            .await;

        assert_eq!(captured, None);
        assert_eq!(pasteboard.read_text().as_deref(), Some("before"));
    }

    /// The restore is a `Drop` impl rather than hand-written calls at each exit,
    /// so it holds for paths nobody wrote yet — including panics.
    #[test]
    fn restore_on_drop_restores_even_while_unwinding() {
        let pasteboard = FakePasteboard::holding(vec![image_item()]);
        assert_eq!(pasteboard.item_count(), 1);

        let concrete = pasteboard.clone();
        let erased: Arc<dyn Pasteboard> = pasteboard;
        let snapshot = erased.snapshot().expect("snapshot");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _restore = RestoreOnDrop::new(erased.as_ref(), snapshot);
            concrete.lock().items.clear();
            panic!("something went wrong mid-capture");
        }));

        assert!(result.is_err(), "the panic should propagate");
        assert!(
            concrete.has_image(),
            "the clipboard must be restored even when the capture panics"
        );
    }
}
