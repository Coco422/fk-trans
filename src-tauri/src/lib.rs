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
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{Emitter, Manager, PhysicalPosition};
use tokio::sync::RwLock;
use translate::TranslationEngine;

const POPUP_CURSOR_OFFSET_X: f64 = 14.0;
const POPUP_CURSOR_OFFSET_Y: f64 = 16.0;
const POPUP_SCREEN_MARGIN: f64 = 8.0;

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub translation_engine: RwLock<TranslationEngine>,
    pub history: HistoryStore,
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

/// Shared translation pipeline: capture clipboard, translate, show popup.
async fn run_translation_pipeline(
    app: tauri::AppHandle,
    cm: Arc<clipboard::manager::ClipboardManager>,
    running: Arc<AtomicBool>,
) {
    if running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::info!("[pipeline] Previous pipeline still running, skipping trigger");
        return;
    }
    let _guard = PipelineGuard { running };

    log::info!("[pipeline] Translation pipeline triggered");

    // Check if enabled
    {
        let state = app.state::<AppState>();
        let config = state.config.lock().unwrap();
        if !config.enabled {
            log::info!("[pipeline] Disabled, skipping");
            return;
        }
    }

    let cursor_pos = current_cursor_position(&app);
    log::info!(
        "[pipeline] Cursor captured at ({}, {})",
        cursor_pos.x,
        cursor_pos.y
    );

    // Capture selected text before showing the popup so failed captures do not flash.
    log::info!("[pipeline] Capturing selected text...");
    let text = match cm.capture_selected_text().await {
        Some(t) => {
            log::info!("[pipeline] Captured text: {} chars", t.len());
            t
        }
        None => {
            log::warn!("[pipeline] No text captured, skipping");
            return;
        }
    };

    // Show popup at cursor position with loading state.
    if let Some(window) = app.get_webview_window("popup") {
        let popup_pos = popup_position_for_cursor(&window, cursor_pos);
        log::info!(
            "[pipeline] Showing popup at ({}, {})",
            popup_pos.x,
            popup_pos.y
        );
        let _ = window.set_position(tauri::Position::Physical(popup_pos));
        let _ = window.show();
    }
    let _ = app.emit("translation-started", ());

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
        }
        Err(e) => {
            log::error!("Translation error: {}", e);
            let _ = app.emit("translation-error", e.to_string());
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Hide dock icon on macOS
            #[cfg(target_os = "macos")]
            {
                platform::macos::hide_dock_icon();

                if !platform::macos::check_accessibility_permissions() {
                    log::warn!(
                        "Accessibility permissions not granted. Mouse listener may not work."
                    );
                    platform::macos::open_accessibility_settings();
                }
            }

            // Initialize state
            let config = config::load_config();
            let active_provider = config.active_provider.clone();
            let provider_configs = config.providers.clone();

            app.manage(AppState {
                config: Mutex::new(config),
                translation_engine: RwLock::new(TranslationEngine::new(
                    active_provider,
                    &provider_configs,
                )),
                history: HistoryStore::new(),
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
            let (tx, rx) = std::sync::mpsc::channel::<()>();

            // Start mouse listener in dedicated thread
            let listener = mouse::listener::MouseListener::new();
            listener.start(tx);

            let app_handle_clone = app_handle.clone();
            let clipboard_clone = clipboard_manager.clone();
            let mouse_pipeline_running = pipeline_running.clone();

            // Use a dedicated thread for receiving to avoid blocking tokio runtime
            std::thread::spawn(move || loop {
                match rx.recv() {
                    Ok(()) => {
                        let app = app_handle_clone.clone();
                        let cm = clipboard_clone.clone();
                        let running = mouse_pipeline_running.clone();
                        tauri::async_runtime::spawn(run_translation_pipeline(app, cm, running));
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

                eprintln!("[shortcut] *** Cmd+Shift+T released! ***");
                log::info!("[shortcut] Cmd+Shift+T triggered");
                let app = app_handle.clone();
                let cm = shortcut_cm.clone();
                let running = shortcut_pipeline_running.clone();
                tauri::async_runtime::spawn(run_translation_pipeline(app, cm, running));
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
