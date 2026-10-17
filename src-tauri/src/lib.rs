#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

// The app is macOS-only by decision, not by omission: the trigger layer is built
// on CGEventTap and the Accessibility API, and the popup relies on AppKit window
// behaviour. A build for another platform would be a different program, so say so
// here rather than compiling a half-working one out of `#[cfg]` forks nobody ever
// runs.
#[cfg(not(target_os = "macos"))]
compile_error!("fk-trans targets macOS only; see README.md");

mod capture;
mod clipboard;
mod commands;
mod compat;
mod config;
mod history;
mod macos;
mod mouse;
mod pipeline;
mod translate;
mod tray;
mod trigger;

use config::AppConfig;
use history::HistoryStore;
use macos::ax::ScreenRect;
use mouse::listener::MouseTriggerEvent;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, PhysicalPosition};
use tokio::sync::RwLock;
use translate::TranslationEngine;
use trigger::placement::{self, Point, Size};
use trigger::TriggerSource;

/// Distance between the popup and whatever it is anchored to.
const POPUP_GAP: f64 = 10.0;
/// The closest the popup may come to the edge of the usable screen area.
const POPUP_SCREEN_MARGIN: f64 = 8.0;
const LOG_MAX_FILE_SIZE_BYTES: u64 = 512 * 1024;
const LOG_ROTATION_KEEP_FILES: usize = 4;

/// Payload of the `translation-ready` event.
///
/// A real struct rather than a `serde_json::json!` literal, so the generated
/// TypeScript and the Rust side cannot drift. The cursor position used to be
/// part of this and was never read — Rust positions the window itself.
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types.ts")]
pub struct TranslationReadyPayload {
    pub original: String,
    pub result: translate::provider::TranslateResult,
}

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub translation_engine: RwLock<TranslationEngine>,
    pub history: HistoryStore,
    pub mouse_listener: Mutex<Option<mouse::listener::MouseListener>>,
    pub mouse_trigger_state: mouse::listener::SharedMouseTriggerState,
    /// Lives in shared state so diagnostics can report a wedged run instead of
    /// leaving the user with an app that silently ignores every trigger.
    pub pipeline_gate: Arc<pipeline::PipelineGate>,
    /// Set when the config file could not be read at startup. Without this the
    /// app looks like it simply forgot the user's settings.
    pub config_load_problem: Option<config::ConfigLoadProblem>,
    /// Changes made while loading the config, e.g. a provider entry that no
    /// longer exists. Reported in Diagnostics so a migration is not mistaken for
    /// the app rewriting settings on its own.
    pub config_notices: Vec<String>,
}

/// Releases the pipeline gate when the run ends, however it ends.
struct GateGuard {
    gate: Arc<pipeline::PipelineGate>,
    token: pipeline::GateToken,
}

impl Drop for GateGuard {
    fn drop(&mut self) {
        self.gate.release(self.token);
    }
}

/// Claim the pipeline gate, reporting the outcome to diagnostics.
///
/// Returns `None` when a live run already holds the gate. A *reclaimed* gate is
/// logged as a warning and surfaced in diagnostics, because it means a previous
/// run wedged — the app recovered, but something upstream is broken and the user
/// needs to be able to see that rather than just noticing odd behaviour.
fn acquire_pipeline_gate(
    app: &tauri::AppHandle,
    gate: &Arc<pipeline::PipelineGate>,
) -> Option<GateGuard> {
    let now_ms = mouse::listener::now_millis();

    match gate.try_acquire(now_ms) {
        pipeline::GateOutcome::Acquired(token) => Some(GateGuard {
            gate: gate.clone(),
            token,
        }),
        pipeline::GateOutcome::Reclaimed { token, held_for_ms } => {
            let note = format!(
                "Recovered: previous pipeline run was wedged for {}s and was abandoned",
                held_for_ms / 1000
            );
            log::warn!("[pipeline] {}", note);
            if let Some(state) = app.try_state::<AppState>() {
                mouse::listener::mark_pipeline_result(&state.mouse_trigger_state, &note, None);
            }
            Some(GateGuard {
                gate: gate.clone(),
                token,
            })
        }
        pipeline::GateOutcome::Busy { held_for_ms } => {
            log::info!(
                "[pipeline] Previous pipeline still running ({}s), skipping trigger",
                held_for_ms / 1000
            );
            if let Some(state) = app.try_state::<AppState>() {
                mouse::listener::mark_pipeline_result(
                    &state.mouse_trigger_state,
                    "Skipped: previous pipeline still running",
                    None,
                );
            }
            None
        }
    }
}

fn current_cursor_position(app: &tauri::AppHandle) -> PhysicalPosition<f64> {
    match app.cursor_position() {
        Ok(pos) => pos,
        Err(e) => {
            log::warn!("[pipeline] Failed to read cursor via Tauri: {}", e);
            mouse::cursor::get_cursor_position()
                .map(|pos| PhysicalPosition::new(pos.x, pos.y))
                .unwrap_or_else(|| {
                    log::warn!("[pipeline] Failed to read cursor via CoreGraphics fallback");
                    PhysicalPosition::new(0.0, 0.0)
                })
        }
    }
}

/// The scale factor of the display the pointer is on.
///
/// Accessibility and Tauri both express logical positions in points, but Tauri
/// reports the pointer in physical pixels, so the pointer has to be converted
/// back before it can be used as a logical anchor.
fn scale_factor_at(window: &tauri::WebviewWindow, cursor: PhysicalPosition<f64>) -> f64 {
    window
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or(1.0)
}

/// The usable area of the display the pointer is on, in logical points.
///
/// Taken from the pointer rather than from the selection because a selection is
/// always on the display the pointer that made it is on, and the pointer is the
/// one Tauri can answer questions about in physical coordinates.
fn work_area_at(window: &tauri::WebviewWindow, cursor: PhysicalPosition<f64>) -> Option<ScreenRect> {
    let monitor = window
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()?;
    // Each half converts on its own: `PhysicalRect` has no conversion, because a
    // position and a size do not scale the same way (`ceil` vs `floor`).
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let position = area.position.to_logical::<f64>(scale);
    let size = area.size.to_logical::<f64>(scale);

    Some(ScreenRect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

/// Where to put the popup, in logical points.
///
/// Anchored to the selection when the app reported its geometry, and to the
/// pointer otherwise. Anchoring to the selection is the point of asking
/// Accessibility for its rectangle at all: the popup opens beside the words being
/// translated rather than beside wherever the pointer ended up.
///
/// A selection gets the same treatment a small box next to any selection gets —
/// just outside its bottom-right corner, flipping to the other side when there is
/// no room. The trigger point will land in exactly the same place, smaller.
fn popup_origin(
    window: &tauri::WebviewWindow,
    cursor: PhysicalPosition<f64>,
    bounds: Option<ScreenRect>,
    popup: Size,
) -> Point {
    let cursor_point = || {
        let logical = cursor.to_logical::<f64>(scale_factor_at(window, cursor));
        Point {
            x: logical.x,
            y: logical.y,
        }
    };

    let Some(work_area) = work_area_at(window, cursor) else {
        // Nothing to clamp against. Roughly the right place beats not showing it.
        let anchor = bounds.map_or_else(cursor_point, |rect| Point {
            x: rect.right(),
            y: rect.bottom(),
        });
        return Point {
            x: anchor.x + POPUP_GAP,
            y: anchor.y + POPUP_GAP,
        };
    };

    match bounds {
        Some(rect) => {
            placement::trigger_point(rect, popup, work_area, POPUP_GAP, POPUP_SCREEN_MARGIN)
        }
        None => placement::panel_origin(
            cursor_point(),
            popup,
            work_area,
            POPUP_GAP,
            POPUP_SCREEN_MARGIN,
        ),
    }
}

fn popup_size(window: &tauri::WebviewWindow, scale: f64) -> Option<Size> {
    let size = window.outer_size().ok()?.to_logical::<f64>(scale);

    Some(Size {
        width: size.width,
        height: size.height,
    })
}

fn show_popup(app: &tauri::AppHandle, cursor: PhysicalPosition<f64>, bounds: Option<ScreenRect>) {
    let Some(window) = app.get_webview_window("popup") else {
        log::warn!("[pipeline] No popup window to show");
        return;
    };
    let scale = scale_factor_at(&window, cursor);
    let Some(size) = popup_size(&window, scale) else {
        log::warn!("[pipeline] Could not measure the popup window");
        return;
    };

    let origin = popup_origin(&window, cursor, bounds, size);
    log::info!(
        "[pipeline] Showing popup at ({:.0}, {:.0}), anchored to {}",
        origin.x,
        origin.y,
        if bounds.is_some() {
            "the selection"
        } else {
            "the cursor"
        }
    );
    let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
        origin.x, origin.y,
    )));
    let _ = window.show();
}

fn show_popup_error(app: &tauri::AppHandle, cursor: PhysicalPosition<f64>, message: String) {
    show_popup(app, cursor, None);
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
    gate: Arc<pipeline::PipelineGate>,
    source: TriggerSource,
) {
    let is_selection_source = source == TriggerSource::MouseSelection;

    let _guard = match acquire_pipeline_gate(&app, &gate) {
        Some(guard) => guard,
        None => {
            emit_mouse_trigger_state(&app);
            return;
        }
    };

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
        let config = state
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        if is_selection_source {
            let state = app.state::<AppState>();
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Skipped: selection trigger ignored because provider is not ready",
                Some(reason),
            );
            emit_mouse_trigger_state(&app);
            return;
        }

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

    if !is_selection_source {
        // An explicit trigger gets a visible loading state before the capture,
        // which can take a moment: without it a slow or failing capture is
        // indistinguishable from the app ignoring the click.
        show_popup(&app, cursor_pos, None);
        let _ = app.emit("translation-started", ());
    }

    // The strategy chain does its own waiting: it lets the app settle after the
    // mouse-up, bounds each attempt, and decides whether the pasteboard fallback
    // is worth trying. Reading a selection before the app has finished making it
    // is the main reason the old capture was unreliable.
    log::info!("[pipeline] Capturing selected text...");
    let state = app.state::<AppState>();
    let capture = capture::capture_selection(&cm, true).await;
    let capture_note = capture.summary();
    log::info!("[pipeline] Capture: {capture_note}");

    let Some(captured) = capture.captured else {
        log::warn!("[pipeline] No text captured, skipping: {capture_note}");
        if is_selection_source {
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Ignored: selection drag produced no captured text",
                Some(capture_note),
            );
        } else {
            let _ = app.emit(
                "translation-error",
                "No selected text captured. Select text in another app and try again.".to_string(),
            );
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Failed: no selected text captured",
                Some(capture_note),
            );
        }
        emit_mouse_trigger_state(&app);
        return;
    };

    let text = captured.text;
    log::info!(
        "[pipeline] Captured text via {}: chars={} hash={:016x}",
        captured.strategy.label(),
        text.chars().count(),
        text_hash(&text)
    );

    if is_selection_source {
        // Deliberately after the capture, not before: only now do we know where
        // the selection is, and a popup that appears at the pointer and then jumps
        // is worse than one that appears in the right place.
        show_popup(&app, cursor_pos, captured.bounds);
        let _ = app.emit("translation-started", ());
    }

    let (source_lang, target_lang) = {
        let config = state
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
                TranslationReadyPayload {
                    original: text,
                    result,
                },
            );
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                success_message,
                None,
            );
            emit_mouse_trigger_state(&app);
        }
        Err(error) => {
            log::error!("[pipeline] Translation error kind={}", error.kind());
            let _ = app.emit("translation-error", error.to_string());
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "Failed: translation error",
                Some(format!("Translation error kind: {}", error.kind())),
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

            {
                macos::window::set_accessory_activation_policy(app.handle());

                if !macos::permission::is_granted() {
                    log::warn!(
                        "Accessibility permissions not granted. Mouse listener may not work."
                    );
                    let _ = macos::permission::request();
                }
            }

            // Initialize state
            let config::ConfigLoad {
                config,
                problem: config_load_problem,
                notices: config_notices,
            } = config::load_config();
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
            let mouse_trigger_state = mouse::listener::new_shared_state_with_options(
                config.mouse_trigger_button,
                config.selection_trigger_enabled,
            );

            let pipeline_gate = Arc::new(pipeline::PipelineGate::default());

            app.manage(AppState {
                config: Mutex::new(config),
                translation_engine: RwLock::new(TranslationEngine::new(
                    active_provider,
                    &provider_configs,
                )),
                history: HistoryStore::new(),
                mouse_listener: Mutex::new(None),
                mouse_trigger_state: mouse_trigger_state.clone(),
                pipeline_gate: pipeline_gate.clone(),
                config_load_problem,
                config_notices,
            });

            // Create system tray
            if let Err(e) = tray::menu::create_tray(app.handle()) {
                log::error!("[tray] Failed to create tray: {}", e);
            }

            if let Some(popup) = app.get_webview_window("popup") {
                macos::window::configure_popup_window(&popup);
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
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .replace(listener);

            let app_handle_clone = app_handle.clone();
            let clipboard_clone = clipboard_manager.clone();
            let mouse_pipeline_gate = pipeline_gate.clone();

            // Use a dedicated thread for receiving to avoid blocking tokio runtime
            std::thread::spawn(move || loop {
                match rx.recv() {
                    Ok(event) => {
                        log::info!(
                            "[mouse] Event delivered to pipeline receiver: button={} trigger={:?} ts={}",
                            event.button,
                            event.trigger,
                            event.timestamp_ms
                        );
                        let app = app_handle_clone.clone();
                        emit_mouse_trigger_state(&app);

                        let Some(source) = event.trigger else {
                            continue;
                        };
                        let cm = clipboard_clone.clone();
                        let gate = mouse_pipeline_gate.clone();
                        tauri::async_runtime::spawn(run_translation_pipeline(app, cm, gate, source));
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
            let shortcut_pipeline_gate = pipeline_gate.clone();
            let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyT);
            let gs = app.global_shortcut();
            match gs.on_shortcut(shortcut, move |app_handle, _shortcut, event| {
                if event.state != ShortcutState::Released {
                    return;
                }

                log::info!("[shortcut] Cmd+Shift+T triggered");
                let app = app_handle.clone();
                let cm = shortcut_cm.clone();
                let gate = shortcut_pipeline_gate.clone();
                tauri::async_runtime::spawn(run_translation_pipeline(
                    app,
                    cm,
                    gate,
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

    /// The gap and the margin both take space away from the popup. On the
    /// smallest display worth supporting they must still leave room for one.
    #[test]
    fn the_popup_offsets_are_small_enough_for_a_small_display() {
        let smallest_display_height = 600.0_f64;
        assert!(POPUP_GAP + POPUP_SCREEN_MARGIN < smallest_display_height / 4.0);
    }
}
