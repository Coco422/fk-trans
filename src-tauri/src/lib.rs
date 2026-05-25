mod clipboard;
mod commands;
mod config;
mod history;
mod mouse;
mod ocr;
mod platform;
mod translate;
mod tray;

use config::AppConfig;
use history::HistoryStore;
use mouse::listener::{MouseTriggerEvent, TriggerSource};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize};
use tokio::sync::RwLock;
use tokio::time::{sleep, timeout, Duration};
use translate::provider::TranslateError;
use translate::TranslationEngine;

const POPUP_CURSOR_OFFSET_X: f64 = 14.0;
const POPUP_CURSOR_OFFSET_Y: f64 = 16.0;
const POPUP_SCREEN_MARGIN: f64 = 8.0;
const LOG_MAX_FILE_SIZE_BYTES: u64 = 512 * 1024;
const LOG_ROTATION_KEEP_FILES: usize = 4;
const CLIPBOARD_CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);
const KEYBOARD_SHORTCUT_CAPTURE_DELAY: Duration = Duration::from_millis(160);
const MACOS_ACCESSIBILITY_PERMISSION_MESSAGE: &str = "Accessibility permission is required to copy selected text. Add the current dev executable in System Settings > Privacy & Security > Accessibility, then restart npm run tauri dev.";
const MACOS_SCREEN_RECORDING_PERMISSION_MESSAGE: &str = "Screen Recording permission is required for OCR. Add the current dev executable in System Settings > Privacy & Security > Screen Recording, then restart npm run tauri dev.";

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub translation_engine: RwLock<TranslationEngine>,
    pub history: HistoryStore,
    pub mouse_listener: Mutex<Option<mouse::listener::MouseListener>>,
    pub mouse_trigger_state: mouse::listener::SharedMouseTriggerState,
    pub ocr_runtime: ocr::OcrRuntime,
    pub pipeline_running: Arc<AtomicBool>,
}

pub(crate) struct PipelineGuard {
    pub(crate) running: Arc<AtomicBool>,
}

impl Drop for PipelineGuard {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CaptureMetadata {
    Clipboard,
    Ocr {
        backend: &'static str,
        elapsed_ms: u64,
    },
}

fn current_cursor_position(app: &tauri::AppHandle) -> PhysicalPosition<f64> {
    match app.cursor_position() {
        Ok(pos) => pos,
        Err(e) => {
            log::warn!("[pipeline] Failed to read cursor via Tauri: {}", e);
            let pos = mouse::cursor::get_cursor_position();
            PhysicalPosition::new(pos.x, pos.y)
        }
    }
}

fn popup_position_for_cursor(
    window: &tauri::WebviewWindow,
    cursor: PhysicalPosition<f64>,
) -> PhysicalPosition<i32> {
    let mut x = cursor.x + POPUP_CURSOR_OFFSET_X;
    let mut y = cursor.y + POPUP_CURSOR_OFFSET_Y;

    if let (Ok(size), Ok(Some(monitor))) = (
        window.outer_size(),
        window.monitor_from_point(cursor.x, cursor.y),
    ) {
        let work_area = monitor.work_area();
        let left = work_area.position.x as f64 + POPUP_SCREEN_MARGIN;
        let top = work_area.position.y as f64 + POPUP_SCREEN_MARGIN;
        let right = work_area.position.x as f64 + work_area.size.width as f64 - POPUP_SCREEN_MARGIN;
        let bottom =
            work_area.position.y as f64 + work_area.size.height as f64 - POPUP_SCREEN_MARGIN;
        let width = size.width as f64;
        let height = size.height as f64;

        if x + width > right {
            x = cursor.x - width - POPUP_CURSOR_OFFSET_X;
        }
        if y + height > bottom {
            y = cursor.y - height - POPUP_CURSOR_OFFSET_Y;
        }

        x = x.max(left).min((right - width).max(left));
        y = y.max(top).min((bottom - height).max(top));
    }

    PhysicalPosition::new(x.round() as i32, y.round() as i32)
}

pub(crate) fn show_popup_at_cursor(app: &tauri::AppHandle, cursor: PhysicalPosition<f64>) {
    if let Some(window) = app.get_webview_window("popup") {
        let popup_pos = popup_position_for_cursor(&window, cursor);
        log::info!(
            "[pipeline] Showing popup at ({}, {})",
            popup_pos.x,
            popup_pos.y
        );
        let _ = window.set_position(tauri::Position::Physical(popup_pos));
        let _ = window.show();
    }
}

fn show_popup_error(app: &tauri::AppHandle, cursor: PhysicalPosition<f64>, message: String) {
    show_popup_at_cursor(app, cursor);
    let _ = app.emit("translation-error", message);
}

pub(crate) fn emit_mouse_trigger_state(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let snapshot = mouse::listener::snapshot(&state.mouse_trigger_state);
        let _ = app.emit("mouse-trigger-state", snapshot);
    }
}

fn text_hash(text: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn translate_error_kind(error: &TranslateError) -> &'static str {
    match error {
        TranslateError::Network(_) => "network",
        TranslateError::Api(_) => "api",
        TranslateError::RateLimited => "rate_limited",
        TranslateError::Config(_) => "config",
    }
}

fn apply_log_level(debug_logging: bool) {
    log::set_max_level(if debug_logging {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    });
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        log::error!("[panic] {}", info);
    }));
}

fn capture_delay_for_source(source: TriggerSource) -> Duration {
    match source {
        TriggerSource::KeyboardShortcut => KEYBOARD_SHORTCUT_CAPTURE_DELAY,
        TriggerSource::MouseMiddle | TriggerSource::OcrShortcut | TriggerSource::Test => {
            Duration::ZERO
        }
    }
}

fn macos_accessibility_error(trusted: bool) -> Option<&'static str> {
    if trusted {
        None
    } else {
        Some(MACOS_ACCESSIBILITY_PERMISSION_MESSAGE)
    }
}

fn translation_payload(
    text: String,
    result: translate::provider::TranslateResult,
    popup_anchor: PhysicalPosition<f64>,
    capture: CaptureMetadata,
) -> serde_json::Value {
    let capture_source = match capture {
        CaptureMetadata::Clipboard => "clipboard",
        CaptureMetadata::Ocr { .. } => "ocr",
    };
    let mut payload = serde_json::json!({
        "original": text,
        "result": result,
        "cursor_x": popup_anchor.x,
        "cursor_y": popup_anchor.y,
        "capture_source": capture_source,
    });
    if let CaptureMetadata::Ocr {
        backend,
        elapsed_ms,
    } = capture
    {
        payload["ocr_backend"] = serde_json::json!(backend);
        payload["ocr_elapsed_ms"] = serde_json::json!(elapsed_ms);
    }
    payload
}

pub async fn translate_and_emit(
    app: tauri::AppHandle,
    text: String,
    popup_anchor: PhysicalPosition<f64>,
    capture: CaptureMetadata,
) {
    let state = app.state::<AppState>();
    let (source_lang, target_lang) = {
        let config = state.config.lock().unwrap();
        (config.source_lang.clone(), config.target_lang.clone())
    };

    let engine = state.translation_engine.read().await;
    match engine.translate(&text, &source_lang, &target_lang).await {
        Ok(result) => {
            let capture_source = match capture {
                CaptureMetadata::Clipboard => "clipboard",
                CaptureMetadata::Ocr { .. } => "ocr",
            };
            let success_message = format!(
                "Success: translated {} chars from {} with {}",
                text.chars().count(),
                capture_source,
                result.provider
            );
            if matches!(capture, CaptureMetadata::Clipboard) {
                state.history.add(history::HistoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: chrono::Utc::now().timestamp(),
                    original: text.clone(),
                    translated: result.translated.clone(),
                    source_lang: result.source_lang.clone(),
                    target_lang: result.target_lang.clone(),
                    provider: result.provider.clone(),
                });
            }

            let payload = translation_payload(text, result, popup_anchor, capture);
            let _ = app.emit("translation-ready", payload);
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                success_message,
                None,
            );
            emit_mouse_trigger_state(&app);
        }
        Err(e) => {
            let error_kind = translate_error_kind(&e);
            log::error!("[pipeline] Translation error kind={}", error_kind);
            let _ = app.emit("translation-error", e.to_string());
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Failed: translation error",
                Some(format!("Translation error kind: {}", error_kind)),
            );
            emit_mouse_trigger_state(&app);
        }
    }
}

/// Shared translation pipeline: capture clipboard, translate, show popup.
async fn run_translation_pipeline(
    app: tauri::AppHandle,
    cm: Arc<clipboard::manager::ClipboardManager>,
    running: Arc<AtomicBool>,
    source: TriggerSource,
) {
    if running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::info!("[pipeline] Previous pipeline still running, skipping trigger");
        if let Some(state) = app.try_state::<AppState>() {
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Skipped: previous pipeline still running",
                None,
            );
            emit_mouse_trigger_state(&app);
        }
        return;
    }
    let _guard = PipelineGuard { running };

    log::info!(
        "[pipeline] Translation pipeline triggered from {:?}",
        source
    );
    if let Some(state) = app.try_state::<AppState>() {
        mouse::listener::mark_pipeline_triggered(&state.mouse_trigger_state, source);
        emit_mouse_trigger_state(&app);
    }

    // Check if enabled and locally usable before touching the clipboard.
    let readiness = {
        let state = app.state::<AppState>();
        let config = state.config.lock().unwrap();
        if !config.enabled {
            log::info!("[pipeline] Disabled, skipping");
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Skipped: fk-trans disabled",
                None,
            );
            emit_mouse_trigger_state(&app);
            return;
        }
        config::validate_active_provider(&config)
    };

    let cursor_pos = current_cursor_position(&app);
    log::info!(
        "[pipeline] Cursor captured at ({}, {})",
        cursor_pos.x,
        cursor_pos.y
    );

    if let Err(reason) = readiness {
        log::warn!("[pipeline] No available provider: {}", reason);
        show_popup_error(
            &app,
            cursor_pos,
            "No available translation provider. Configure one in Settings.".to_string(),
        );
        let state = app.state::<AppState>();
        mouse::listener::mark_pipeline_result(
            &state.mouse_trigger_state,
            "Failed: no available translation provider",
            Some(reason),
        );
        emit_mouse_trigger_state(&app);
        return;
    }

    #[cfg(target_os = "macos")]
    if let Some(message) =
        macos_accessibility_error(platform::macos::check_accessibility_permissions())
    {
        log::warn!("[pipeline] Accessibility permission missing before clipboard capture");
        let _ = platform::macos::request_accessibility_permissions();
        show_popup_error(&app, cursor_pos, message.to_string());
        let state = app.state::<AppState>();
        mouse::listener::mark_pipeline_result(
            &state.mouse_trigger_state,
            "Failed: accessibility permission missing",
            Some(message.to_string()),
        );
        emit_mouse_trigger_state(&app);
        return;
    }

    let capture_delay = capture_delay_for_source(source);
    if !capture_delay.is_zero() {
        log::debug!(
            "[pipeline] Waiting {} ms for shortcut modifiers to release",
            capture_delay.as_millis()
        );
        sleep(capture_delay).await;
    }

    log::info!("[pipeline] Capturing selected text...");
    let text = match timeout(CLIPBOARD_CAPTURE_TIMEOUT, cm.capture_selected_text()).await {
        Ok(Ok(t)) => {
            log::info!(
                "[pipeline] Captured text: chars={} hash={:016x}",
                t.chars().count(),
                text_hash(&t)
            );
            t
        }
        Ok(Err(error)) => {
            let user_message = error.user_message();
            let diagnostic_reason = error.diagnostic_reason();
            log::warn!("[pipeline] Clipboard capture failed: {}", diagnostic_reason);
            show_popup_error(&app, cursor_pos, user_message);
            let state = app.state::<AppState>();
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Failed: no selected text captured",
                Some(diagnostic_reason),
            );
            emit_mouse_trigger_state(&app);
            return;
        }
        Err(_) => {
            log::error!(
                "[pipeline] Clipboard capture timed out after {} ms",
                CLIPBOARD_CAPTURE_TIMEOUT.as_millis()
            );
            show_popup_error(
                &app,
                cursor_pos,
                "Clipboard capture timed out. Try again after selecting text in the target app."
                    .to_string(),
            );
            let state = app.state::<AppState>();
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Failed: clipboard capture timed out",
                Some("Clipboard capture timeout".to_string()),
            );
            emit_mouse_trigger_state(&app);
            return;
        }
    };

    show_popup_at_cursor(&app, cursor_pos);
    let _ = app.emit("translation-started", ());

    translate_and_emit(app, text, cursor_pos, CaptureMetadata::Clipboard).await;
}

fn validate_ocr_start_with_platform(
    config: &AppConfig,
    platform_ready: bool,
    screen_recording_ready: bool,
) -> Result<(), String> {
    if !config.enabled {
        return Err("fk-trans is disabled".to_string());
    }
    if !config.ocr_enabled {
        return Err("OCR shortcut is disabled".to_string());
    }
    if !platform_ready {
        return Err("OCR is only implemented on macOS in this version".to_string());
    }
    if !screen_recording_ready {
        return Err(MACOS_SCREEN_RECORDING_PERMISSION_MESSAGE.to_string());
    }
    config::validate_active_provider(config)
}

async fn start_ocr_selection_pipeline(app: tauri::AppHandle, running: Arc<AtomicBool>) {
    if running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::info!("[ocr] Previous pipeline still running, skipping OCR trigger");
        if let Some(state) = app.try_state::<AppState>() {
            state
                .ocr_runtime
                .mark_result("OCR skipped: previous pipeline still running", None);
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Skipped: previous pipeline still running",
                None,
            );
            emit_mouse_trigger_state(&app);
        }
        return;
    }

    log::info!("[ocr] Cmd+Shift+O triggered");
    let cursor_pos = current_cursor_position(&app);
    let state = app.state::<AppState>();
    mouse::listener::mark_pipeline_triggered(
        &state.mouse_trigger_state,
        TriggerSource::OcrShortcut,
    );
    emit_mouse_trigger_state(&app);

    #[cfg(target_os = "macos")]
    let screen_recording_ready = platform::macos::check_screen_recording_permissions();
    #[cfg(not(target_os = "macos"))]
    let screen_recording_ready = false;

    let readiness = {
        let config = state.config.lock().unwrap();
        validate_ocr_start_with_platform(&config, cfg!(target_os = "macos"), screen_recording_ready)
    };
    if let Err(reason) = readiness {
        log::warn!("[ocr] OCR start skipped: {}", reason);
        if !screen_recording_ready {
            #[cfg(target_os = "macos")]
            {
                let _ = platform::macos::request_screen_recording_permissions();
            }
            state.ocr_runtime.mark_screen_capture_error(reason.clone());
        } else {
            state.ocr_runtime.mark_error(reason.clone());
        }
        show_popup_error(&app, cursor_pos, reason.clone());
        mouse::listener::mark_pipeline_result(
            &state.mouse_trigger_state,
            "Failed: OCR unavailable",
            Some(reason),
        );
        emit_mouse_trigger_state(&app);
        running.store(false, Ordering::SeqCst);
        return;
    }

    let payload = match state.ocr_runtime.start_selection_session(cursor_pos) {
        Ok(payload) => payload,
        Err(error) => {
            log::warn!("[ocr] Screen capture failed: {}", error);
            show_popup_error(
                &app,
                cursor_pos,
                "Screen capture failed. Check macOS Screen Recording permission.".to_string(),
            );
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Failed: OCR screen capture failed",
                Some(error),
            );
            emit_mouse_trigger_state(&app);
            running.store(false, Ordering::SeqCst);
            return;
        }
    };

    let Some(window) = app.get_webview_window("ocr-select") else {
        let error = "OCR selection window is unavailable".to_string();
        state.ocr_runtime.mark_error(error.clone());
        show_popup_error(&app, cursor_pos, error.clone());
        mouse::listener::mark_pipeline_result(
            &state.mouse_trigger_state,
            "Failed: OCR selection window unavailable",
            Some(error),
        );
        emit_mouse_trigger_state(&app);
        running.store(false, Ordering::SeqCst);
        return;
    };

    let _ = window.set_position(tauri::Position::Physical(PhysicalPosition::new(
        payload.monitor_x,
        payload.monitor_y,
    )));
    let _ = window.set_size(tauri::Size::Physical(PhysicalSize::new(
        payload.monitor_width,
        payload.monitor_height,
    )));
    let _ = window.show();
    let _ = window.set_focus();
    #[cfg(target_os = "macos")]
    platform::macos::focus_window(&window);
    let _ = window.emit("ocr-selection-started", payload);
}

fn ocr_shortcut() -> tauri_plugin_global_shortcut::Shortcut {
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

    Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyO)
}

pub(crate) fn register_ocr_shortcut(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    let shortcut = ocr_shortcut();
    let gs = app.global_shortcut();
    if gs.is_registered(shortcut) {
        return Ok(());
    }
    let running = app.state::<AppState>().pipeline_running.clone();
    gs.on_shortcut(shortcut, move |app_handle, _shortcut, event| {
        if event.state != ShortcutState::Released {
            return;
        }

        let app = app_handle.clone();
        let running = running.clone();
        tauri::async_runtime::spawn(start_ocr_selection_pipeline(app, running));
    })
    .map_err(|e| e.to_string())
}

pub(crate) fn unregister_ocr_shortcut(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    let shortcut = ocr_shortcut();
    let gs = app.global_shortcut();
    if gs.is_registered(shortcut) {
        gs.unregister(shortcut).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Debug)
                .max_file_size(LOG_MAX_FILE_SIZE_BYTES as u128)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(
                    LOG_ROTATION_KEEP_FILES,
                ))
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: None,
                    }),
                ])
                .build(),
        )
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            install_panic_hook();

            // Hide dock icon on macOS
            #[cfg(target_os = "macos")]
            {
                platform::macos::set_accessory_activation_policy(app.handle());

                if !platform::macos::check_accessibility_permissions() {
                    log::warn!(
                        "Accessibility permissions not granted. Mouse listener may not work."
                    );
                    let _ = platform::macos::request_accessibility_permissions();
                }
            }

            // Initialize state
            let config = config::load_config();
            apply_log_level(config.debug_logging);
            log::info!(
                "[logging] Debug logging {}, max_file_size={} bytes, rotated_files_kept={}",
                if config.debug_logging {
                    "enabled"
                } else {
                    "disabled"
                },
                LOG_MAX_FILE_SIZE_BYTES,
                LOG_ROTATION_KEEP_FILES
            );
            let active_provider = config.active_provider.clone();
            let provider_configs = config.providers.clone();
            let ocr_enabled = config.ocr_enabled;
            let mouse_trigger_state =
                mouse::listener::new_shared_state(config.mouse_trigger_button);
            let pipeline_running = Arc::new(AtomicBool::new(false));

            app.manage(AppState {
                config: Mutex::new(config),
                translation_engine: RwLock::new(TranslationEngine::new(
                    active_provider,
                    &provider_configs,
                )),
                history: HistoryStore::new(),
                mouse_listener: Mutex::new(None),
                mouse_trigger_state: mouse_trigger_state.clone(),
                ocr_runtime: ocr::OcrRuntime::new(),
                pipeline_running: pipeline_running.clone(),
            });

            // Create system tray
            tray::menu::create_tray(app.handle()).expect("Failed to create tray");

            // Configure popup window as non-activating on macOS
            #[cfg(target_os = "macos")]
            {
                if let Some(popup) = app.get_webview_window("popup") {
                    platform::macos::configure_popup_window(&popup);
                }
            }

            // Shared clipboard manager
            let clipboard_manager = Arc::new(clipboard::manager::ClipboardManager::new());

            // --- Mouse listener pipeline ---
            let app_handle = app.handle().clone();
            let (tx, rx) = std::sync::mpsc::channel::<MouseTriggerEvent>();

            // Start mouse listener in dedicated thread
            let listener = mouse::listener::MouseListener::new();
            listener.start(tx, mouse_trigger_state.clone());
            app.state::<AppState>()
                .mouse_listener
                .lock()
                .unwrap()
                .replace(listener);

            let app_handle_clone = app_handle.clone();
            let clipboard_clone = clipboard_manager.clone();
            let mouse_pipeline_running = pipeline_running.clone();

            // Use a dedicated thread for receiving to avoid blocking tokio runtime
            std::thread::spawn(move || loop {
                match rx.recv() {
                    Ok(event) => {
                        log::info!(
                            "[mouse] Event delivered to pipeline receiver: button={} trigger={} ts={}",
                            event.button,
                            event.is_trigger,
                            event.timestamp_ms
                        );
                        let app = app_handle_clone.clone();
                        emit_mouse_trigger_state(&app);
                        if !event.is_trigger {
                            continue;
                        }
                        let cm = clipboard_clone.clone();
                        let running = mouse_pipeline_running.clone();
                        tauri::async_runtime::spawn(run_translation_pipeline(
                            app,
                            cm,
                            running,
                            TriggerSource::MouseMiddle,
                        ));
                    }
                    Err(e) => {
                        log::error!("Channel error: {}", e);
                        break;
                    }
                }
            });

            // --- Global shortcut: Cmd+Shift+T ---
            use tauri_plugin_global_shortcut::{
                Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
            };

            let shortcut_cm = clipboard_manager.clone();
            let shortcut_pipeline_running = pipeline_running.clone();
            let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyT);
            let gs = app.global_shortcut();
            match gs.on_shortcut(shortcut, move |app_handle, _shortcut, event| {
                if event.state != ShortcutState::Released {
                    return;
                }

                log::info!("[shortcut] Cmd+Shift+T triggered");
                let app = app_handle.clone();
                let cm = shortcut_cm.clone();
                let running = shortcut_pipeline_running.clone();
                tauri::async_runtime::spawn(run_translation_pipeline(
                    app,
                    cm,
                    running,
                    TriggerSource::KeyboardShortcut,
                ));
            }) {
                Ok(_) => log::info!("[shortcut] Cmd+Shift+T registered successfully"),
                Err(e) => log::error!("[shortcut] Failed to register Cmd+Shift+T: {}", e),
            }

            if ocr_enabled {
                match register_ocr_shortcut(app.handle()) {
                    Ok(_) => log::info!("[shortcut] Cmd+Shift+O registered successfully"),
                    Err(e) => log::error!("[shortcut] Failed to register Cmd+Shift+O: {}", e),
                }
            } else {
                log::info!("[shortcut] Cmd+Shift+O not registered because OCR is disabled");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::translation::translate_text,
            commands::translation::ai_action,
            commands::translation::get_history,
            commands::translation::clear_history,
            commands::settings::get_config,
            commands::settings::update_config,
            commands::settings::update_provider,
            commands::settings::test_provider,
            commands::diagnostics::get_diagnostics_snapshot,
            commands::diagnostics::start_middle_click_test,
            commands::diagnostics::export_diagnostics_report,
            commands::diagnostics::reveal_diagnostics_folder,
            commands::diagnostics::open_accessibility_settings,
            commands::diagnostics::get_macos_dev_permission_target,
            commands::diagnostics::reveal_current_executable,
            commands::diagnostics::log_frontend_event,
            commands::ocr::get_ocr_selection_payload,
            commands::ocr::complete_ocr_selection,
            commands::ocr::cancel_ocr_selection,
            commands::ocr::open_screen_recording_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_rotation_policy_is_bounded() {
        assert!(LOG_MAX_FILE_SIZE_BYTES <= 1024 * 1024);
        assert!(LOG_ROTATION_KEEP_FILES <= 5);
    }

    #[test]
    fn clipboard_capture_timeout_is_short() {
        assert!(CLIPBOARD_CAPTURE_TIMEOUT <= Duration::from_secs(5));
    }

    #[test]
    fn ocr_disabled_rejects_start_before_provider_validation() {
        let config = AppConfig {
            ocr_enabled: false,
            ..AppConfig::default()
        };

        assert_eq!(
            validate_ocr_start_with_platform(&config, true, true),
            Err("OCR shortcut is disabled".to_string())
        );
    }

    #[test]
    fn ocr_requires_screen_recording_before_provider_validation() {
        let config = AppConfig {
            providers: vec![config::ProviderConfig {
                name: "deeplx".to_string(),
                base_url: "http://127.0.0.1:1188".to_string(),
                api_key: String::new(),
                model: String::new(),
                system_prompt: String::new(),
                user_prompt: String::new(),
                extra_params: serde_json::json!({}),
            }],
            active_provider: "deeplx".to_string(),
            ..AppConfig::default()
        };

        assert_eq!(
            validate_ocr_start_with_platform(&config, true, false),
            Err(MACOS_SCREEN_RECORDING_PERMISSION_MESSAGE.to_string())
        );
    }

    #[test]
    fn keyboard_shortcut_waits_before_clipboard_capture() {
        assert_eq!(
            capture_delay_for_source(TriggerSource::KeyboardShortcut),
            KEYBOARD_SHORTCUT_CAPTURE_DELAY
        );
        assert_eq!(
            capture_delay_for_source(TriggerSource::MouseMiddle),
            Duration::ZERO
        );
    }

    #[test]
    fn accessibility_error_blocks_clipboard_capture_when_missing() {
        assert_eq!(
            macos_accessibility_error(false),
            Some(MACOS_ACCESSIBILITY_PERMISSION_MESSAGE)
        );
        assert_eq!(macos_accessibility_error(true), None);
    }

    #[test]
    fn translation_payload_marks_clipboard_and_ocr_sources() {
        let result = translate::provider::TranslateResult {
            original: "hello".to_string(),
            translated: "你好".to_string(),
            source_lang: "en".to_string(),
            target_lang: "zh".to_string(),
            provider: "test".to_string(),
            alternatives: vec![],
        };
        let anchor = PhysicalPosition::new(1.0, 2.0);

        let clipboard = translation_payload(
            "hello".to_string(),
            result.clone(),
            anchor,
            CaptureMetadata::Clipboard,
        );
        let ocr = translation_payload(
            "hello".to_string(),
            result,
            anchor,
            CaptureMetadata::Ocr {
                backend: "apple_vision",
                elapsed_ms: 42,
            },
        );

        assert_eq!(clipboard["capture_source"], "clipboard");
        assert!(clipboard.get("ocr_backend").is_none());
        assert_eq!(ocr["capture_source"], "ocr");
        assert_eq!(ocr["ocr_backend"], "apple_vision");
        assert_eq!(ocr["ocr_elapsed_ms"], 42);
    }
}
