//! Commands the trigger panel issues about itself.
//!
//! The panel's webview is the only thing that knows when the point was clicked,
//! how tall the rendered translation turned out, and whether the user pressed
//! Escape. So those arrive here as commands, rather than being inferred from
//! mouse events that cannot see any of them.

use crate::{acquire_pipeline_gate, apply_panel, dismiss_panel, translate_and_show, AppState};
use tauri::{AppHandle, Emitter, Manager};

/// The trigger point was clicked: open the card and translate what it stands for.
///
/// This is where a selection trigger finally costs a request. Until the click,
/// nothing has been sent anywhere — that is the difference between this and
/// translating on every selection.
#[tauri::command]
pub async fn expand_trigger_panel(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    let (transition, text) = {
        let mut panel = state
            .trigger_panel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // No point to expand means the click arrived too late — the point had
        // already expired — or a second click landed on a card that is already
        // open.
        let Some(transition) = panel.expand() else {
            return Err("the trigger point is no longer on screen".to_string());
        };

        (transition, panel.selection().unwrap_or_default().to_string())
    };

    apply_panel(&app, transition);
    let _ = app.emit("translation-started", ());

    // Nothing could be read when the point appeared, which is what every app
    // that will not describe its selection looks like — terminals, mostly. Only
    // now that the user has asked is a clipboard capture fair: it takes the
    // clipboard over for a moment, and doing that before they asked would be
    // taking something that was never ours to take.
    let text = if text.trim().is_empty() {
        let clipboard = state.clipboard.clone();
        match crate::capture::capture_selection(&clipboard, true)
            .await
            .captured
        {
            Some(captured) => captured.text,
            None => {
                // The card is already open and showing its loading state, so a
                // refusal has to say so; the error alone would leave it loading
                // until the idle timer closed it.
                let _ = app.emit("translation-error", "没有取到选中的文字".to_string());
                return Err("the selection could not be read".to_string());
            }
        }
    } else {
        text
    };

    let Some(gate) = acquire_pipeline_gate(&app, &state.pipeline_gate) else {
        let _ = app.emit(
            "translation-error",
            "上一个翻译还没有结束，请稍后再试".to_string(),
        );
        return Err("another translation is still running".to_string());
    };

    translate_and_show(&app, text, gate).await;
    Ok(())
}

/// The webview measured its content and needs the window to match.
///
/// The card's height is the one thing Rust cannot work out: only the rendered,
/// wrapped text knows how tall it is.
#[tauri::command]
pub fn resize_trigger_panel(app: AppHandle, height: f64) {
    let transition = {
        let state = app.state::<AppState>();
        let mut panel = state
            .trigger_panel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        panel.resize_card(height)
    };

    apply_panel(&app, transition);
}

/// The panel asked to be dismissed: Escape, its close button, or its idle timer.
#[tauri::command]
pub fn dismiss_trigger_panel(app: AppHandle) {
    dismiss_panel(&app);
}
