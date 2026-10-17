//! Deciding when a translation starts, and where what follows appears.
//!
//! The CGEventTap callback is a bad place to make decisions: it runs on a thread
//! macOS stops delivering events to if it is slow, and nothing about it can be
//! exercised without a live session. So the decisions live here, as pure
//! functions over plain data, and `mouse::listener` only converts events into
//! that data and applies what comes back.
//!
//! * [`reducer`] — whether a mouse event should start a translation.
//! * [`placement`] — where a small window goes near a selection.

pub mod panel;
pub mod placement;
pub mod reducer;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What started a translation.
///
/// Lives here rather than next to the mouse listener because it is not only
/// mouse events that produce one — a keyboard shortcut and the diagnostics test
/// do too.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types.ts")]
pub enum TriggerSource {
    MouseMiddle,
    MouseSelection,
    KeyboardShortcut,
    Test,
}
