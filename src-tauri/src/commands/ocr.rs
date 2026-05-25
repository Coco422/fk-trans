use crate::ocr::{self, OcrSelectionPayload, OcrSelectionRect};
use crate::AppState;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OcrReadyPayload {
    text: String,
    image_data_url: String,
    regions: Vec<ocr::OcrTextRegion>,
    cursor_x: f64,
    cursor_y: f64,
    capture_source: &'static str,
    ocr_backend: &'static str,
    ocr_elapsed_ms: u64,
    source_lang: String,
    target_lang: String,
}

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

    crate::show_ocr_popup_at_cursor(&app, crop.popup_anchor);
    let _ = app.emit("ocr-started", ());

    let started = std::time::Instant::now();
    let ocr::OcrCrop {
        png_bytes,
        image_data_url,
        popup_anchor,
    } = crop;
    let recognition = match tauri::async_runtime::spawn_blocking(move || {
        ocr::recognize_text_from_png(&png_bytes)
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

    let recognition = match recognition {
        Ok(recognition) if !recognition.text.trim().is_empty() => {
            state.ocr_runtime.mark_result(
                format!("OCR recognized {} chars", recognition.text.chars().count()),
                Some(elapsed_ms),
            );
            recognition
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

    let (source_lang, target_lang) = {
        let config = state
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (config.source_lang.clone(), config.target_lang.clone())
    };
    let payload = OcrReadyPayload {
        text: recognition.text,
        image_data_url,
        regions: recognition.regions,
        cursor_x: popup_anchor.x,
        cursor_y: popup_anchor.y,
        capture_source: "ocr",
        ocr_backend: "apple_vision",
        ocr_elapsed_ms: elapsed_ms,
        source_lang,
        target_lang,
    };
    let _ = app.emit("ocr-ready", payload);
    crate::mouse::listener::mark_pipeline_result(
        &state.mouse_trigger_state,
        "Success: OCR recognized text",
        None,
    );
    crate::emit_mouse_trigger_state(&app);

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
