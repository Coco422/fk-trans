use crate::ocr::{self, OcrSelectionPayload, OcrSelectionRect};
use crate::AppState;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
pub fn get_ocr_selection_payload(
    state: State<'_, AppState>,
) -> Result<Option<OcrSelectionPayload>, String> {
    Ok(state.ocr_runtime.latest_payload())
}

#[tauri::command]
pub async fn complete_ocr_selection(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    selection: OcrSelectionRect,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("ocr-select") {
        let _ = window.hide();
    }

    let _guard = crate::PipelineGuard {
        running: state.pipeline_running.clone(),
    };
    let Some(crop) = state.ocr_runtime.crop_selection(&session_id, selection)? else {
        crate::mouse::listener::mark_pipeline_result(
            &state.mouse_trigger_state,
            "Canceled: OCR selection too small",
            None,
        );
        crate::emit_mouse_trigger_state(&app);
        return Ok(());
    };

    crate::show_popup_at_cursor(&app, crop.popup_anchor);
    let _ = app.emit("translation-started", ());

    let started = std::time::Instant::now();
    let ocr_text = match tauri::async_runtime::spawn_blocking(move || {
        ocr::recognize_text_from_png(&crop.png_bytes)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => {
            let message = format!("OCR worker failed: {}", error);
            state.ocr_runtime.mark_error(message.clone());
            let _ = app.emit("translation-error", message.clone());
            crate::mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Failed: OCR worker failed",
                Some(message),
            );
            crate::emit_mouse_trigger_state(&app);
            return Ok(());
        }
    };
    let elapsed_ms = started.elapsed().as_millis() as u64;

    let text = match ocr_text {
        Ok(text) if !text.trim().is_empty() => {
            state.ocr_runtime.mark_result(
                format!("OCR recognized {} chars", text.chars().count()),
                Some(elapsed_ms),
            );
            text
        }
        Ok(_) => {
            let message = "OCR found no readable text".to_string();
            state.ocr_runtime.mark_error(message.clone());
            let _ = app.emit("translation-error", message.clone());
            crate::mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Failed: OCR found no readable text",
                Some(message),
            );
            crate::emit_mouse_trigger_state(&app);
            return Ok(());
        }
        Err(error) => {
            state.ocr_runtime.mark_error(error.clone());
            let _ = app.emit("translation-error", error.clone());
            crate::mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Failed: OCR recognition error",
                Some(error),
            );
            crate::emit_mouse_trigger_state(&app);
            return Ok(());
        }
    };

    crate::translate_and_emit(
        app,
        text,
        crop.popup_anchor,
        crate::CaptureMetadata::Ocr {
            backend: "apple_vision",
            elapsed_ms,
        },
    )
    .await;

    Ok(())
}

#[tauri::command]
pub fn cancel_ocr_selection(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("ocr-select") {
        let _ = window.hide();
    }
    state.ocr_runtime.cancel_selection(&session_id);
    state
        .pipeline_running
        .store(false, std::sync::atomic::Ordering::SeqCst);
    crate::mouse::listener::mark_pipeline_result(
        &state.mouse_trigger_state,
        "Canceled: OCR selection canceled",
        None,
    );
    crate::emit_mouse_trigger_state(&app);
    Ok(())
}

#[tauri::command]
pub fn open_screen_recording_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::open_screen_recording_settings()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Screen Recording settings shortcut is only available on macOS".to_string())
    }
}
