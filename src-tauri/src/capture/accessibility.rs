//! The Accessibility strategy: ask the focused app for its selection.
//!
//! Wrapped rather than called directly for two reasons. The FFI is bounded by
//! `AXUIElementSetMessagingTimeout`, but boundedly-slow is still slow, and this
//! runs on the async runtime — a blocked app must not stall a runtime worker. And
//! a wall-clock ceiling on top means a bug in the AX layer cannot hang the
//! translation pipeline.

use crate::macos::ax::{self, Failure, ScreenRect};
use tokio::time::{timeout, Duration};

/// The ceiling on a whole read, on top of per-call AX timeouts. Three seconds is
/// generous for reading two attributes; anything slower is a symptom, not
/// latency worth waiting for.
const READ_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct Captured {
    pub text: String,
    pub bounds: Option<ScreenRect>,
}

pub async fn read() -> Result<Captured, Failure> {
    // The handle is a future; the timeout has to wrap awaiting it, not the other
    // way round. A blocking task cannot be cancelled once it starts, which is
    // fine: `AXUIElementSetMessagingTimeout` is what actually bounds the FFI, and
    // this is the backstop for a bug in that layer.
    let read = tokio::task::spawn_blocking(ax::read_selection);

    match timeout(READ_TIMEOUT, read).await {
        Ok(Ok(Ok(selection))) => Ok(Captured {
            text: selection.text,
            bounds: selection.bounds,
        }),
        Ok(Ok(Err(failure))) => Err(failure),
        Ok(Err(join_error)) => {
            log::error!("[capture] Accessibility read task failed: {join_error}");
            Err(Failure::Other {
                code: "AX read task failed",
            })
        }
        Err(_) => {
            log::warn!(
                "[capture] Accessibility read did not finish within {}s",
                READ_TIMEOUT.as_secs()
            );
            Err(Failure::Blocked {
                code: "read timed out",
            })
        }
    }
}
