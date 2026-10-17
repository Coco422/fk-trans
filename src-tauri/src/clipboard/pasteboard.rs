//! The system pasteboard, backed up as a whole.
//!
//! Reading selected text by synthesising Cmd+C means temporarily taking over the
//! user's clipboard, so it has to be put back exactly as it was found. Saving
//! only the text is not enough: if the clipboard held an image, a file list, or
//! rich text, the "text" is an empty string, and restoring it wipes the user's
//! clipboard. That is silent data loss for a feature the user never asked to
//! involve their clipboard at all.
//!
//! So the unit of backup is the whole pasteboard: every `NSPasteboardItem`, with
//! every type it advertises. The model type is plain data, which keeps the
//! capture logic testable through the [`Pasteboard`] seam without touching the
//! real system pasteboard.

use std::sync::Arc;

/// One pasteboard item and all of its typed payloads. Items are restored
/// together, so a copied file that exposes both a URL and a string keeps both.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PasteboardItem {
    /// `(type identifier, bytes)`, e.g. `("public.utf8-plain-text", b"...")`.
    pub entries: Vec<(String, Vec<u8>)>,
}

/// Everything that was on the pasteboard, ready to be put back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PasteboardSnapshot {
    pub items: Vec<PasteboardItem>,
}

impl PasteboardSnapshot {
    /// A snapshot with nothing on it — the pasteboard was empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// The type identifier for plain UTF-8 text, as AppKit spells it.
pub const UTF8_TEXT_TYPE: &str = "public.utf8-plain-text";

pub trait Pasteboard: Send + Sync {
    /// Bumped by the system on every pasteboard write. Used to tell whether a
    /// synthesised copy actually reached the pasteboard, which is how a capture
    /// reports "nothing was selected" without having to clear the clipboard first.
    fn change_count(&self) -> i64;

    fn snapshot(&self) -> Option<PasteboardSnapshot>;

    /// Puts `snapshot` back, replacing whatever is on the pasteboard now.
    fn restore(&self, snapshot: &PasteboardSnapshot);

    fn read_text(&self) -> Option<String>;
}

/// Restores the pasteboard when it goes out of scope, on every exit path
/// including panics. The previous hand-written restores ran on two of the three
/// early returns, which is the kind of thing that silently regresses the moment
/// someone adds a fourth.
pub struct RestoreOnDrop<'a> {
    pasteboard: &'a dyn Pasteboard,
    snapshot: PasteboardSnapshot,
}

impl<'a> RestoreOnDrop<'a> {
    pub fn new(pasteboard: &'a dyn Pasteboard, snapshot: PasteboardSnapshot) -> Self {
        Self {
            pasteboard,
            snapshot,
        }
    }
}

impl Drop for RestoreOnDrop<'_> {
    fn drop(&mut self) {
        self.pasteboard.restore(&self.snapshot);
    }
}

/// The real system pasteboard.
pub fn system_pasteboard() -> Arc<dyn Pasteboard> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(macos::MacPasteboard)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Arc::new(arboard::ArboardPasteboard)
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{Pasteboard, PasteboardItem, PasteboardSnapshot, UTF8_TEXT_TYPE};
    use objc::rc::autoreleasepool;
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use std::sync::Mutex;

    const NS_UTF8_ENCODING: u64 = 4;

    /// AppKit's pasteboard is not safe to touch from two threads at once — two
    /// concurrent readers segfault the process rather than failing. Captures are
    /// already serialised by `ClipboardManager`'s own lock, but holding this for
    /// every access makes the guarantee independent of the caller.
    static ACCESS: Mutex<()> = Mutex::new(());

    /// Run `body` holding the pasteboard lock, inside an autorelease pool.
    ///
    /// The pool matters on its own: these calls run on Tauri worker threads,
    /// which have no pool of their own, so every `NSString`/`NSData`/
    /// `NSMutableArray` created here would otherwise live until the process
    /// exits. This runs on every middle-click, so it would be a real leak.
    fn with_pasteboard<T>(body: impl FnOnce() -> T) -> T {
        let _access = ACCESS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        autoreleasepool(body)
    }

    pub struct MacPasteboard;

    fn general() -> *mut Object {
        unsafe { msg_send![class!(NSPasteboard), generalPasteboard] }
    }

    /// `NSString` -> `String`. Returns `None` for nil or non-UTF8 values rather
    /// than panicking: a type declaration is written by whichever app owns the
    /// pasteboard, so it is not ours to trust.
    unsafe fn nsstring_to_string(value: *mut Object) -> Option<String> {
        if value.is_null() {
            return None;
        }
        let ptr: *const c_char = msg_send![value, UTF8String];
        if ptr.is_null() {
            return None;
        }
        Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }

    unsafe fn string_to_nsstring(value: &str) -> *mut Object {
        let allocated: *mut Object = msg_send![class!(NSString), alloc];
        let string: *mut Object = msg_send![allocated,
            initWithBytes: value.as_ptr()
            length: value.len()
            encoding: NS_UTF8_ENCODING
        ];
        string
    }

    unsafe fn bytes_to_nsdata(bytes: &[u8]) -> *mut Object {
        if bytes.is_empty() {
            let empty: *mut Object = msg_send![class!(NSData), data];
            return empty;
        }
        let data: *mut Object = msg_send![class!(NSData),
            dataWithBytes: bytes.as_ptr()
            length: bytes.len()
        ];
        data
    }

    unsafe fn read_item(item: *mut Object) -> Option<PasteboardItem> {
        if item.is_null() {
            return None;
        }
        let types: *mut Object = msg_send![item, types];
        if types.is_null() {
            return None;
        }
        let count: usize = msg_send![types, count];

        let mut entries = Vec::new();
        for index in 0..count {
            let type_object: *mut Object = msg_send![types, objectAtIndex: index];
            let Some(type_id) = nsstring_to_string(type_object) else {
                continue;
            };
            let query = string_to_nsstring(&type_id);
            let data: *mut Object = msg_send![item, dataForType: query];
            if data.is_null() {
                continue;
            }
            let length: usize = msg_send![data, length];
            let pointer: *const u8 = msg_send![data, bytes];
            if pointer.is_null() {
                continue;
            }
            entries.push((type_id, std::slice::from_raw_parts(pointer, length).to_vec()));
        }

        Some(PasteboardItem { entries })
    }

    impl Pasteboard for MacPasteboard {
        fn change_count(&self) -> i64 {
            with_pasteboard(|| unsafe {
                let pasteboard = general();
                if pasteboard.is_null() {
                    return 0;
                }
                msg_send![pasteboard, changeCount]
            })
        }

        fn snapshot(&self) -> Option<PasteboardSnapshot> {
            with_pasteboard(|| unsafe {
                let pasteboard = general();
                if pasteboard.is_null() {
                    return None;
                }
                let items: *mut Object = msg_send![pasteboard, pasteboardItems];
                if items.is_null() {
                    // An empty pasteboard reports nil; that is a valid snapshot.
                    return Some(PasteboardSnapshot::default());
                }
                let count: usize = msg_send![items, count];
                let mut collected = Vec::with_capacity(count);
                for index in 0..count {
                    let item: *mut Object = msg_send![items, objectAtIndex: index];
                    if let Some(item) = read_item(item) {
                        collected.push(item);
                    }
                }
                Some(PasteboardSnapshot { items: collected })
            })
        }

        fn restore(&self, snapshot: &PasteboardSnapshot) {
            with_pasteboard(|| unsafe {
                let pasteboard = general();
                if pasteboard.is_null() {
                    log::warn!("[clipboard] No general pasteboard to restore into");
                    return;
                }
                let _: i64 = msg_send![pasteboard, clearContents];
                if snapshot.is_empty() {
                    return;
                }

                let array: *mut Object = msg_send![class!(NSMutableArray), array];
                for item in &snapshot.items {
                    let ns_item: *mut Object = msg_send![class!(NSPasteboardItem), new];
                    for (type_id, bytes) in &item.entries {
                        let type_object = string_to_nsstring(type_id);
                        let data = bytes_to_nsdata(bytes);
                        let _: bool = msg_send![ns_item, setData: data forType: type_object];
                    }
                    let _: () = msg_send![array, addObject: ns_item];
                }
                let _: bool = msg_send![pasteboard, writeObjects: array];
            })
        }

        fn read_text(&self) -> Option<String> {
            with_pasteboard(|| unsafe {
                let pasteboard = general();
                if pasteboard.is_null() {
                    return None;
                }
                let query = string_to_nsstring(UTF8_TEXT_TYPE);
                let value: *mut Object = msg_send![pasteboard, stringForType: query];
                nsstring_to_string(value)
            })
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod arboard_backend {
    use super::{Pasteboard, PasteboardItem, PasteboardSnapshot, UTF8_TEXT_TYPE};
    use arboard::Clipboard;
    use std::sync::Mutex;

    /// Non-macOS fallback kept only so the crate still builds there. It cannot
    /// preserve non-text clipboard content; macOS is the supported target.
    pub struct ArboardPasteboard {
        change_count: Mutex<i64>,
    }

    impl Pasteboard for ArboardPasteboard {
        fn change_count(&self) -> i64 {
            *self.change_count.lock().unwrap_or_else(|p| p.into_inner())
        }

        fn snapshot(&self) -> Option<PasteboardSnapshot> {
            let text = Clipboard::new().ok()?.get_text().ok()?;
            Some(PasteboardSnapshot {
                items: vec![PasteboardItem {
                    entries: vec![(UTF8_TEXT_TYPE.to_string(), text.into_bytes())],
                }],
            })
        }

        fn restore(&self, snapshot: &PasteboardSnapshot) {
            let text = snapshot
                .items
                .first()
                .and_then(|item| {
                    item.entries
                        .iter()
                        .find(|(type_id, _)| type_id == UTF8_TEXT_TYPE)
                })
                .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned());
            if let Some(text) = text {
                if let Ok(mut clipboard) = Clipboard::new() {
                    let _ = clipboard.set_text(text);
                }
            }
        }

        fn read_text(&self) -> Option<String> {
            Clipboard::new().ok()?.get_text().ok()
        }

    }
}

#[cfg(not(target_os = "macos"))]
pub use arboard_backend::ArboardPasteboard;

#[cfg(test)]
mod tests {
    use super::*;

    /// The system pasteboard is one global resource. The backend's access lock
    /// serialises individual calls but not a whole snapshot → assert → restore
    /// sequence, so these tests must not interleave with each other.
    #[cfg(target_os = "macos")]
    fn pasteboard_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn text_of(item: &PasteboardItem) -> Option<String> {
        item.entries
            .iter()
            .find(|(type_id, _)| type_id == UTF8_TEXT_TYPE)
            .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
    }

    fn has_type(item: &PasteboardItem, type_id: &str) -> bool {
        item.entries.iter().any(|(t, _)| t == type_id)
    }

    /// The in-memory model is what the capture logic is tested against, so its
    /// accessors need to be right too.
    #[test]
    fn item_lookup_matches_on_the_type_identifier() {
        let item = PasteboardItem {
            entries: vec![
                (UTF8_TEXT_TYPE.to_string(), b"hello".to_vec()),
                ("public.html".to_string(), b"<b>hello</b>".to_vec()),
            ],
        };

        assert_eq!(text_of(&item).as_deref(), Some("hello"));
        assert!(has_type(&item, "public.html"));
        assert!(!has_type(&item, "public.png"));
        assert!(PasteboardSnapshot::default().is_empty());
    }

    /// Verifies the real AppKit bridge, which the in-memory fake cannot: a wrong
    /// `msg_send` signature crashes or corrupts rather than failing an assertion.
    ///
    /// Only re-writes the pasteboard with the content it already holds, so it
    /// cannot cost the user anything they had copied.
    #[cfg(target_os = "macos")]
    #[test]
    fn real_pasteboard_survives_a_snapshot_and_restore() {
        let _serial = pasteboard_test_lock();
        let pasteboard = system_pasteboard();

        let count_before = pasteboard.change_count();
        assert!(
            count_before > 0,
            "the system pasteboard should report a change count"
        );

        let snapshot = pasteboard.snapshot().expect("read the system pasteboard");
        let text_before = pasteboard.read_text();

        pasteboard.restore(&snapshot);

        assert_eq!(
            pasteboard.read_text(),
            text_before,
            "restoring a snapshot must preserve the text it captured"
        );
        assert!(
            pasteboard.change_count() > count_before,
            "restore must actually write to the pasteboard"
        );
    }

    /// AppKit's pasteboard is not safe to touch from several threads at once —
    /// writing this test without the access lock segfaulted the test binary.
    /// Captures run on Tauri worker threads, so the serialisation has to hold
    /// even when several callers arrive together.
    #[cfg(target_os = "macos")]
    #[test]
    fn real_pasteboard_tolerates_concurrent_access() {
        let _serial = pasteboard_test_lock();
        let pasteboard = system_pasteboard();
        let users_clipboard = pasteboard.snapshot().expect("snapshot");
        let _put_it_back = RestoreOnDrop::new(pasteboard.as_ref(), users_clipboard);

        let threads: Vec<_> = (0..8)
            .map(|worker| {
                std::thread::spawn(move || {
                    let pasteboard = system_pasteboard();
                    for round in 0..25 {
                        let value = format!("fk-trans concurrent {worker}-{round}");
                        pasteboard.restore(&PasteboardSnapshot {
                            items: vec![PasteboardItem {
                                entries: vec![(UTF8_TEXT_TYPE.to_string(), value.into_bytes())],
                            }],
                        });
                        let _ = pasteboard.change_count();
                        let _ = pasteboard.read_text();
                        let _ = pasteboard.snapshot();
                    }
                })
            })
            .collect();

        for thread in threads {
            thread.join().expect("worker thread must survive the pasteboard race");
        }
    }

    /// The regression this module exists for, verified against the real
    /// pasteboard: a copied item exposing several types — one of them not text —
    /// must come back whole. The old text-only backup kept just the string and
    /// dropped everything else.
    #[cfg(target_os = "macos")]
    #[test]
    fn real_pasteboard_round_trips_non_text_types() {
        let _serial = pasteboard_test_lock();
        let pasteboard = system_pasteboard();

        let users_clipboard = pasteboard.snapshot().expect("read the system pasteboard");
        let _put_it_back = RestoreOnDrop::new(pasteboard.as_ref(), users_clipboard);

        let synthetic = PasteboardSnapshot {
            items: vec![PasteboardItem {
                entries: vec![
                    (UTF8_TEXT_TYPE.to_string(), b"fk-trans probe".to_vec()),
                    (
                        "public.html".to_string(),
                        b"<b>fk-trans probe</b>".to_vec(),
                    ),
                ],
            }],
        };

        pasteboard.restore(&synthetic);

        let round_tripped = pasteboard.snapshot().expect("re-read the pasteboard");
        let item = round_tripped.items.first().expect("one item survived");
        assert_eq!(text_of(item).as_deref(), Some("fk-trans probe"));
        assert!(
            has_type(item, "public.html"),
            "a non-text type must survive the round trip; dropping it is the data loss this module prevents"
        );
    }
}
