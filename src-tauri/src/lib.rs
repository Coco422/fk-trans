mod clipboard;
mod commands;
mod config;
mod history;
mod mouse;
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
use tauri::{Emitter, Manager, PhysicalPosition};
use tokio::sync::RwLock;
use tokio::time::{timeout, Duration};
use translate::provider::TranslateError;
use translate::TranslationEngine;

const POPUP_CURSOR_OFFSET_X: f64 = 14.0;
const POPUP_CURSOR_OFFSET_Y: f64 = 16.0;
const POPUP_SCREEN_MARGIN: f64 = 8.0;
const LOG_MAX_FILE_SIZE_BYTES: u64 = 512 * 1024;
const LOG_ROTATION_KEEP_FILES: usize = 4;
const CLIPBOARD_CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub translation_engine: RwLock<TranslationEngine>,
    pub history: HistoryStore,
    pub mouse_listener: Mutex<Option<mouse::listener::MouseListener>>,
    pub mouse_trigger_state: mouse::listener::SharedMouseTriggerState,
}

struct PipelineGuard {
    running: Arc<AtomicBool>,
}

impl Drop for PipelineGuard {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }
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

fn show_popup_at_cursor(app: &tauri::AppHandle, cursor: PhysicalPosition<f64>) {
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

fn emit_mouse_trigger_state(app: &tauri::AppHandle) {
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

    // Show a visible loading state before clipboard capture so trigger failures are observable.
    show_popup_at_cursor(&app, cursor_pos);
    let _ = app.emit("translation-started", ());

    log::info!("[pipeline] Capturing selected text...");
    let text = match timeout(CLIPBOARD_CAPTURE_TIMEOUT, cm.capture_selected_text()).await {
        Ok(Some(t)) => {
            log::info!(
                "[pipeline] Captured text: chars={} hash={:016x}",
                t.chars().count(),
                text_hash(&t)
            );
            t
        }
        Ok(None) => {
            log::warn!("[pipeline] No text captured, skipping");
            let _ = app.emit(
                "translation-error",
                "No selected text captured. Select text in another app and try again.".to_string(),
            );
            let state = app.state::<AppState>();
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Failed: no selected text captured",
                Some("Clipboard capture did not return selected text".to_string()),
            );
            emit_mouse_trigger_state(&app);
            return;
        }
        Err(_) => {
            log::error!(
                "[pipeline] Clipboard capture timed out after {} ms",
                CLIPBOARD_CAPTURE_TIMEOUT.as_millis()
            );
            let _ = app.emit(
                "translation-error",
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

    // Get config values
    let state = app.state::<AppState>();
    let (source_lang, target_lang) = {
        let config = state.config.lock().unwrap();
        (config.source_lang.clone(), config.target_lang.clone())
    };

    // Translate
    let engine = state.translation_engine.read().await;
    match engine.translate(&text, &source_lang, &target_lang).await {
        Ok(result) => {
            let success_message = format!(
                "Success: translated {} chars with {}",
                text.chars().count(),
                result.provider
            );
            state.history.add(history::HistoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: chrono::Utc::now().timestamp(),
                original: text.clone(),
                translated: result.translated.clone(),
                source_lang: result.source_lang.clone(),
                target_lang: result.target_lang.clone(),
                provider: result.provider.clone(),
            });

            let _ = app.emit(
                "translation-ready",
                serde_json::json!({
                    "original": text,
                    "result": result,
                    "cursor_x": cursor_pos.x,
                    "cursor_y": cursor_pos.y,
                }),
            );
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
            let mouse_trigger_state =
                mouse::listener::new_shared_state(config.mouse_trigger_button);

            app.manage(AppState {
                config: Mutex::new(config),
                translation_engine: RwLock::new(TranslationEngine::new(
                    active_provider,
                    &provider_configs,
                )),
                history: HistoryStore::new(),
                mouse_listener: Mutex::new(None),
                mouse_trigger_state: mouse_trigger_state.clone(),
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
            let pipeline_running = Arc::new(AtomicBool::new(false));

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
            commands::diagnostics::log_frontend_event,
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
}
