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
use trigger::placement::Point;
use trigger::TriggerSource;

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
    /// The trigger point and the card it opens into.
    pub trigger_panel: Mutex<trigger::panel::Panel>,
    /// The popup window, once macOS has been asked to make it a panel. `None`
    /// when that failed, in which case the popup still works but behaves like an
    /// ordinary window: it activates the app when clicked.
    pub panel_window: Mutex<Option<macos::panel::TriggerPanelWindow>>,
    /// Shared so the trigger point can fall back to a clipboard capture when it
    /// is clicked, which is the only way to read a selection in an app that will
    /// not describe one.
    pub clipboard: Arc<clipboard::manager::ClipboardManager>,
    /// Whether the popup became a panel at startup. Reported in Diagnostics
    /// because the fallback is silent and its symptom — the trigger point
    /// taking focus away from the app being read — looks like a different bug.
    pub trigger_panel_active: bool,
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
pub(crate) fn acquire_pipeline_gate(
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
                "已恢复：上一次翻译卡住了 {} 秒，已放弃",
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
                    "已跳过：上一次翻译还在进行",
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

/// Where the pointer is, in the logical points the panel is placed in, and the
/// usable area of the display it is on.
///
/// Resolved together because each needs the other: the pointer arrives in
/// physical pixels, and turning those into points needs the scale factor of the
/// display the pointer is on.
///
/// This goes through AppKit rather than Tauri's `monitor_from_point`, which
/// compares against a rectangle in points while documenting its argument as
/// physical pixels — so on a Retina display it never finds anything, and the
/// trigger point was never placed at all. See `macos::screen`.
fn pointer_and_display(cursor: PhysicalPosition<f64>) -> Option<(Point, ScreenRect)> {
    let display = macos::screen::display_at((cursor.x, cursor.y))?;
    let logical = cursor.to_logical::<f64>(display.scale);

    Some((
        Point {
            x: logical.x,
            y: logical.y,
        },
        display.work_area,
    ))
}

/// Run a transition from the panel state machine against the real window.
pub(crate) fn apply_panel(app: &tauri::AppHandle, transition: trigger::panel::Transition) {
    let state = app.state::<AppState>();
    let guard = state
        .panel_window
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    match guard.as_ref() {
        Some(window) => window.apply(transition),
        None => log::warn!("[panel] No panel window, dropping {transition:?}"),
    }
}

/// Show the result card beside the pointer, with no point to click first.
///
/// What a middle click and the keyboard shortcut do: the user has already asked
/// for a translation, so the card goes up immediately and the loading state is
/// visible while it is fetched.
fn show_translation_card(app: &tauri::AppHandle, cursor: PhysicalPosition<f64>) {
    let Some((pointer, work_area)) = pointer_and_display(cursor) else {
        log::warn!("[panel] Could not work out which display the pointer is on");
        return;
    };

    let now = mouse::listener::now_millis();

    let transition = {
        let state = app.state::<AppState>();
        let mut panel = state
            .trigger_panel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        panel.show_card(pointer, work_area, now)
    };

    apply_panel(app, transition);
    let _ = app.emit("translation-started", ());
}

/// Show the trigger point for a selection, instead of translating it.
///
/// The point is the whole point: nothing is translated until it is clicked, so
/// selecting text never costs a request, and a trackpad user — who has no middle
/// button — has something to click.
fn show_trigger_point(
    app: &tauri::AppHandle,
    cursor: PhysicalPosition<f64>,
    bounds: Option<ScreenRect>,
    text: String,
) {
    let Some((pointer, work_area)) = pointer_and_display(cursor) else {
        log::warn!("[panel] Could not work out which display the pointer is on");
        return;
    };

    let now = mouse::listener::now_millis();

    // Which rectangle the point is placed against is worth saying out loud: a
    // point in an unexpected place is the first symptom of an app answering with
    // the bounds of the whole page rather than of the selection.
    let rect = trigger::panel::anchor_rect(bounds, pointer, work_area);
    log::info!(
        "[panel] Anchoring the trigger point to the {}",
        if rect.width > 0.0 {
            "selection"
        } else {
            "pointer (no usable selection rectangle)"
        }
    );

    let transition = {
        let state = app.state::<AppState>();
        let mut panel = state
            .trigger_panel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        panel.show_point(rect, work_area, text, now)
    };

    apply_panel(app, transition);
    let _ = app.emit("trigger-point", ());

    // An unclicked point goes away on its own. The deadline is checked against
    // the point's own age, so this timer cannot take down a card that has since
    // replaced it.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(trigger::panel::POINT_TTL).await;
        let transition = {
            let state = handle.state::<AppState>();
            let mut panel = state
                .trigger_panel
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            panel.expire_point(mouse::listener::now_millis())
        };
        apply_panel(&handle, transition);
    });
}

/// Hide the trigger point or the card, whichever is showing.
pub(crate) fn dismiss_panel(app: &tauri::AppHandle) {
    let transition = {
        let state = app.state::<AppState>();
        let mut panel = state
            .trigger_panel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        panel.hide()
    };
    apply_panel(app, transition);
}

/// Hide the trigger point because of something the user did in another app.
///
/// The position is what keeps a click *on* the point from being read as a click
/// away from it: the tap sees that click first, and would otherwise take the
/// point down a moment before the panel's own click handler could open it.
fn dismiss_panel_at(
    app: &tauri::AppHandle,
    reason: trigger::reducer::DismissReason,
    position: Option<(f64, f64)>,
) {
    let at = position.map(|(x, y)| Point { x, y });

    let transition = {
        let state = app.state::<AppState>();
        let mut panel = state
            .trigger_panel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        panel.dismiss_point(at)
    };

    // Either nothing was showing, or the click was meant for the panel itself.
    // The snapshot goes out either way: the diagnostics panel is showing what the
    // tap saw, and this event is part of that whether or not it changed anything.
    if transition == trigger::panel::Transition::None {
        emit_mouse_trigger_state(app);
        return;
    }

    log::info!("[panel] Dismissed the trigger point: {}", reason.describe());
    apply_panel(app, transition);

    let state = app.state::<AppState>();
    mouse::listener::mark_pipeline_result(
        &state.mouse_trigger_state,
        format!("已收起：{}", reason.describe()),
        None,
    );
    emit_mouse_trigger_state(app);
}

fn show_popup_error(app: &tauri::AppHandle, cursor: PhysicalPosition<f64>, message: String) {
    show_translation_card(app, cursor);
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

    // Held until the answer is on screen, or handed on to `translate_and_show`.
    let guard = match acquire_pipeline_gate(&app, &gate) {
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
                "已跳过：fk-trans 已关闭",
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
                "已跳过：翻译服务未就绪，忽略划词触发",
                Some(reason),
            );
            emit_mouse_trigger_state(&app);
            return;
        }

        show_popup_error(
            &app,
            cursor_pos,
            "没有可用的翻译服务，请到设置里配置一个。".to_string(),
        );
        let state = app.state::<AppState>();
        mouse::listener::mark_pipeline_result(
            &state.mouse_trigger_state,
            "失败：没有可用的翻译服务",
            Some(reason),
        );
        emit_mouse_trigger_state(&app);
        return;
    }

    if !is_selection_source {
        // An explicit trigger gets a visible loading state before the capture,
        // which can take a moment: without it a slow or failing capture is
        // indistinguishable from the app ignoring the click.
        show_translation_card(&app, cursor_pos);
    }

    // The strategy chain does its own waiting: it lets the app settle after the
    // mouse-up, bounds each attempt, and decides whether the pasteboard fallback
    // is worth trying. Reading a selection before the app has finished making it
    // is the main reason the old capture was unreliable.
    //
    // A selection trigger allows no pasteboard fallback: this run only decides
    // whether to offer a trigger point, and synthesising Cmd+C to find that out
    // would take over the user's clipboard for a translation they have not asked
    // for yet. The fallback happens on the click, when they have.
    log::info!("[pipeline] Capturing selected text...");
    let state = app.state::<AppState>();
    let capture = capture::capture_selection(&cm, !is_selection_source).await;
    let capture_note = capture.summary();
    log::info!("[pipeline] Capture: {capture_note}");

    if is_selection_source {
        // The point appears whether or not the text could be read, and whatever
        // was read is carried to the click. An app that will not describe its
        // selection — every terminal — is exactly the case the pasteboard
        // fallback exists for, and that fallback only becomes fair game once the
        // user has asked for a translation. So it happens then, not now.
        let (text, bounds) = match capture.captured {
            Some(captured) => (captured.text, captured.bounds),
            None => (String::new(), None),
        };
        let char_count = text.chars().count();

        show_trigger_point(&app, cursor_pos, bounds, text);

        mouse::listener::mark_pipeline_result(
            &state.mouse_trigger_state,
            if char_count > 0 {
                format!("就绪：已选中 {char_count} 个字符，等待点击触发点")
            } else {
                "就绪：等点击触发点（此刻还读不到选中的文字）".to_string()
            },
            Some(capture_note),
        );
        emit_mouse_trigger_state(&app);
        return;
    }

    let Some(captured) = capture.captured else {
        log::warn!("[pipeline] No text captured, skipping: {capture_note}");
        let _ = app.emit(
            "translation-error",
            "没有取到选中的文字，请先在别的应用里选中文字再试。".to_string(),
        );
        mouse::listener::mark_pipeline_result(
            &state.mouse_trigger_state,
            "失败：没有取到选中的文字",
            Some(capture_note),
        );
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

    translate_and_show(&app, text, guard).await;
}

/// Translate `text` and put the answer on screen.
///
/// Every trigger ends up here with the text already in hand, whether it came from
/// a capture or from a trigger point the user clicked.
pub(crate) async fn translate_and_show(
    app: &tauri::AppHandle,
    text: String,
    // Held for the whole translation and not read: the gate has to stay claimed
    // until the answer is on screen, or a second trigger could start a
    // translation on top of this one.
    _gate: GateGuard,
) {
    let state = app.state::<AppState>();

    let (source_lang, target_lang) = {
        let config = state
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (config.source_lang.clone(), config.target_lang.clone())
    };

    let engine = state.translation_engine.read().await;
    match engine.translate(&text, &source_lang, &target_lang).await {
        Ok(result) => {
            let success_message = format!(
                "成功：用 {} 翻译了 {} 个字符",
                result.provider,
                text.chars().count()
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
            emit_mouse_trigger_state(app);
        }
        Err(error) => {
            log::error!("[pipeline] Translation error kind={}", error.kind());
            let _ = app.emit("translation-error", error.to_string());
            mouse::listener::mark_pipeline_result(
                &state.mouse_trigger_state,
                "失败：翻译出错",
                Some(format!("Translation error kind: {}", error.kind())),
            );
            emit_mouse_trigger_state(app);
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
        // Must be registered before `macos::panel` converts anything: `to_panel`
        // reaches for state this plugin's own setup manages.
        .plugin(tauri_nspanel::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // LaunchAgent rather than an AppleScript login item: it survives a
        // reboot without the user's session, and it is one plist the app can
        // remove cleanly when the toggle is turned off.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
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

            // Shared clipboard manager. It lives in the app state rather than in
            // a closure because the click on the trigger point needs it too.
            let clipboard_manager = Arc::new(clipboard::manager::ClipboardManager::new());

            // The popup becomes a real panel before anything can show it, so no
            // window is ever on screen in the state that activates the app.
            let panel_window = macos::panel::TriggerPanelWindow::adopt(app.handle());
            if panel_window.is_none() {
                log::warn!("[panel] Running with a plain popup window; it will steal focus");
            }

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
                clipboard: clipboard_manager.clone(),
                trigger_panel: Mutex::new(trigger::panel::Panel::new()),
                trigger_panel_active: panel_window.is_some(),
                panel_window: Mutex::new(panel_window),
            });

            // Create system tray
            if let Err(e) = tray::menu::create_tray(app.handle()) {
                log::error!("[tray] Failed to create tray: {}", e);
            }

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
                            "[mouse] Event delivered: {:?} position={:?} ts={}",
                            event.decision,
                            event.position,
                            event.timestamp_ms
                        );
                        let app = app_handle_clone.clone();

                        match event.decision {
                            trigger::reducer::Decision::Trigger(source) => {
                                let cm = clipboard_clone.clone();
                                let gate = mouse_pipeline_gate.clone();
                                tauri::async_runtime::spawn(run_translation_pipeline(
                                    app, cm, gate, source,
                                ));
                            }
                            trigger::reducer::Decision::Dismiss(reason) => {
                                dismiss_panel_at(&app, reason, event.position);
                            }
                            trigger::reducer::Decision::Quiet
                            | trigger::reducer::Decision::Ignored(_) => {
                                emit_mouse_trigger_state(&app);
                            }
                        }
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
            commands::translation::get_languages,
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
            commands::diagnostics::reveal_executable,
            commands::diagnostics::log_frontend_event,
            commands::trigger::expand_trigger_panel,
            commands::trigger::resize_trigger_panel,
            commands::trigger::dismiss_trigger_panel,
        ])
        .on_window_event(|window, event| {
            // The settings window is the only one the user can close, and closing
            // it must not take the app with it: fk-trans lives in the menu bar,
            // and the tray is how it comes back. Hiding it also keeps the webview
            // alive, so reopening is instant and the drafts survive.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Closing the last window is not quitting, for the same reason: a
            // menu-bar app with no menu bar icon would be unreachable. An
            // explicit exit — the tray's Quit — carries a code and still runs.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_rotation_policy_is_bounded() {
        assert!(LOG_MAX_FILE_SIZE_BYTES <= 1024 * 1024);
        assert!(LOG_ROTATION_KEEP_FILES <= 5);
    }

}
