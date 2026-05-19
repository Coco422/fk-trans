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
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio::sync::RwLock;
use translate::TranslationEngine;

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub translation_engine: RwLock<TranslationEngine>,
    pub history: HistoryStore,
}

/// Shared translation pipeline: capture clipboard, translate, show popup.
async fn run_translation_pipeline(app: tauri::AppHandle, cm: Arc<clipboard::manager::ClipboardManager>) {
    // Check if enabled
    {
        let state = app.state::<AppState>();
        let config = state.config.lock().unwrap();
        if !config.enabled {
            return;
        }
    }

    // Emit loading event
    let _ = app.emit("translation-started", ());

    // Capture selected text
    let text = match cm.capture_selected_text().await {
        Some(t) => t,
        None => return,
    };

    // Get cursor position
    let pos = mouse::cursor::get_cursor_position();

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
            // Save to history
            state.history.add(history::HistoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: chrono::Utc::now().timestamp(),
                original: text.clone(),
                translated: result.translated.clone(),
                source_lang: result.source_lang.clone(),
                target_lang: result.target_lang.clone(),
                provider: result.provider.clone(),
            });

            // Position and show popup
            if let Some(window) = app.get_webview_window("popup") {
                let _ = window.set_position(tauri::Position::Logical(
                    tauri::LogicalPosition::new(pos.x, pos.y),
                ));
                let _ = window.show();
                let _ = window.set_focus();
            }

            // Emit translation result
            let _ = app.emit(
                "translation-ready",
                serde_json::json!({
                    "original": text,
                    "result": result,
                    "cursor_x": pos.x,
                    "cursor_y": pos.y,
                }),
            );
        }
        Err(e) => {
            log::error!("Translation error: {}", e);

            // Show popup with error
            if let Some(window) = app.get_webview_window("popup") {
                let _ = window.set_position(tauri::Position::Logical(
                    tauri::LogicalPosition::new(pos.x, pos.y),
                ));
                let _ = window.show();
                let _ = window.set_focus();
            }

            let _ = app.emit("translation-error", e.to_string());
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

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
                    log::warn!("Accessibility permissions not granted. Mouse listener may not work.");
                    platform::macos::open_accessibility_settings();
                }
            }

            // Initialize state
            let config = config::load_config();
            let active_provider = config.active_provider.clone();
            let provider_configs = config.providers.clone();

            app.manage(AppState {
                config: Mutex::new(config),
                translation_engine: RwLock::new(TranslationEngine::new(active_provider, &provider_configs)),
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

            // --- Mouse listener pipeline ---
            let app_handle = app.handle().clone();
            let (tx, rx) = std::sync::mpsc::channel::<()>();

            // Start mouse listener in dedicated thread
            let listener = mouse::listener::MouseListener::new();
            listener.start(tx);

            let app_handle_clone = app_handle.clone();
            let clipboard_clone = clipboard_manager.clone();

            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv() {
                        Ok(()) => {
                            let app = app_handle_clone.clone();
                            let cm = clipboard_clone.clone();
                            tokio::spawn(run_translation_pipeline(app, cm));
                        }
                        Err(e) => {
                            log::error!("Channel error: {}", e);
                            break;
                        }
                    }
                }
            });

            // --- Global shortcut: Cmd+Shift+T ---
            use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

            let shortcut_cm = clipboard_manager.clone();
            let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyT);
            let gs = app.global_shortcut();
            let _ = gs.on_shortcut(shortcut, move |app_handle, _shortcut, _event| {
                let app = app_handle.clone();
                let cm = shortcut_cm.clone();
                tokio::spawn(run_translation_pipeline(app, cm));
            });

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
