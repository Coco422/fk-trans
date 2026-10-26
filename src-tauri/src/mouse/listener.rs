//! The CGEventTap that notices the trigger gestures.
//!
//! This file is plumbing and nothing else: it turns tap events into plain values,
//! asks [`crate::trigger::reducer`] what they mean, records the answer for the
//! diagnostics panel, and forwards triggers to the pipeline. Every rule about
//! *when* a translation starts lives in the reducer, where it can be tested
//! without a mouse.
//!
//! The tap can be switched off by macOS at any time — the callback is killed if
//! it is too slow, and the tap dies with it. So the loop below rebuilds the tap
//! whenever it notices, and reports why in the shared state rather than going
//! quiet.

use crate::trigger::reducer::{
    ButtonEvent, Decision, Ignored, PointerEvent, TriggerOptions, TriggerReducer,
};
use core_graphics::event::{CGEvent, CGEventType, EventField};
use serde::{Deserialize, Serialize};
use std::sync::mpsc::Sender;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use ts_rs::TS;

/// Re-exported so callers do not have to know that the vocabulary moved into
/// `trigger`; the trigger source is not only a mouse concept.
pub use crate::trigger::TriggerSource;

pub type SharedMouseTriggerState = Arc<Mutex<MouseTriggerState>>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types.ts")]
pub enum MouseTriggerStatus {
    Starting,
    PermissionMissing,
    TapCreated,
    Listening,
    EventReceived,
    PipelineTriggered,
    Failed,
    Stopped,
}

/// Timestamps are milliseconds since the epoch. They cross IPC as plain JSON
/// numbers (`#[ts(type = "number")]`) rather than ts-rs's default `bigint`, which
/// the frontend would never see and could not compare against.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types.ts")]
pub struct MouseTriggerState {
    pub status: MouseTriggerStatus,
    pub accessibility_trusted: bool,
    #[ts(type = "number")]
    pub trigger_button: i64,
    pub selection_trigger_enabled: bool,
    #[ts(type = "number | null")]
    pub last_button: Option<i64>,
    #[ts(type = "number | null")]
    pub last_event_at: Option<i64>,
    #[ts(type = "number | null")]
    pub last_trigger_at: Option<i64>,
    #[ts(type = "number | null")]
    pub last_pipeline_at: Option<i64>,
    pub last_pipeline_source: Option<TriggerSource>,
    pub last_pipeline_result: Option<String>,
    pub last_error: Option<String>,
    #[ts(type = "number | null")]
    pub test_active_until: Option<i64>,
}

/// A mouse event the tap saw, and what the reducer made of it.
///
/// Sent for everything the tap concludes something about, not only the events
/// that start a translation, so the diagnostics panel keeps up with what the
/// listener is actually seeing. Plain scrolls are the exception: there are
/// hundreds of them a second and almost none of them mean anything.
#[derive(Debug, Clone)]
pub struct MouseTriggerEvent {
    pub timestamp_ms: i64,
    /// Where the pointer was, for the events that happen at a place.
    pub position: Option<(f64, f64)>,
    pub decision: Decision,
}

pub struct MouseListener {
    running: Arc<AtomicBool>,
}

impl MouseTriggerState {
    pub fn new(trigger_button: i64, selection_trigger_enabled: bool) -> Self {
        Self {
            status: MouseTriggerStatus::Starting,
            accessibility_trusted: false,
            trigger_button,
            selection_trigger_enabled,
            last_button: None,
            last_event_at: None,
            last_trigger_at: None,
            last_pipeline_at: None,
            last_pipeline_source: None,
            last_pipeline_result: None,
            last_error: None,
            test_active_until: None,
        }
    }
}

pub fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn new_shared_state_with_options(
    trigger_button: i64,
    selection_trigger_enabled: bool,
) -> SharedMouseTriggerState {
    Arc::new(Mutex::new(MouseTriggerState::new(
        trigger_button,
        selection_trigger_enabled,
    )))
}

/// A poisoned lock means some other thread panicked while holding it. The state
/// is a handful of plain fields, so carrying on with it beats taking the whole
/// listener down.
fn lock(state: &SharedMouseTriggerState) -> std::sync::MutexGuard<'_, MouseTriggerState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn snapshot(state: &SharedMouseTriggerState) -> MouseTriggerState {
    lock(state).clone()
}

pub fn set_trigger_button(state: &SharedMouseTriggerState, button: i64) {
    let mut guard = lock(state);
    guard.trigger_button = button;
    guard.last_pipeline_result = Some(format!("触发键已设为 {}", button));
}

pub fn set_selection_trigger_enabled(state: &SharedMouseTriggerState, enabled: bool) {
    let mut guard = lock(state);
    guard.selection_trigger_enabled = enabled;
    guard.last_pipeline_result = Some(format!(
        "划词触发{}",
        if enabled { "已开启" } else { "已关闭" }
    ));
}

pub fn start_test_window(state: &SharedMouseTriggerState, duration_ms: i64) -> MouseTriggerState {
    let mut guard = lock(state);
    guard.test_active_until = Some(now_millis() + duration_ms);
    guard.last_pipeline_result = Some("已开始检测鼠标按键".to_string());
    guard.clone()
}

pub fn mark_pipeline_triggered(
    state: &SharedMouseTriggerState,
    source: TriggerSource,
) -> MouseTriggerState {
    let mut guard = lock(state);
    let now = now_millis();
    guard.status = MouseTriggerStatus::PipelineTriggered;
    guard.last_trigger_at = Some(now);
    guard.last_pipeline_at = Some(now);
    guard.last_pipeline_source = Some(source);
    guard.last_pipeline_result = Some("Pipeline triggered".to_string());
    guard.last_error = None;
    guard.clone()
}

pub fn mark_pipeline_result(
    state: &SharedMouseTriggerState,
    result: impl Into<String>,
    error: Option<String>,
) -> MouseTriggerState {
    let mut guard = lock(state);
    guard.last_pipeline_at = Some(now_millis());
    guard.last_pipeline_result = Some(result.into());
    guard.last_error = error;
    guard.clone()
}

impl MouseListener {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&self, tx: Sender<MouseTriggerEvent>, state: SharedMouseTriggerState) {
        let running = self.running.clone();
        running.store(true, Ordering::SeqCst);

        start_platform_listener(running, tx, state);
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

impl Default for MouseListener {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MouseListener {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Turn a tap event into the plain description the reducer works on.
///
/// The tap subscribes to exactly the event types handled below, so anything else
/// cannot arrive.
fn pointer_event(event_type: CGEventType, event: &CGEvent) -> Option<PointerEvent> {
    let timestamp_ms = now_millis();
    let location = event.location();
    let button = |kind, button| PointerEvent::Button {
        kind,
        button,
        x: location.x,
        y: location.y,
        timestamp_ms,
    };

    match event_type {
        CGEventType::OtherMouseDown => Some(button(
            ButtonEvent::Down,
            event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER),
        )),
        CGEventType::LeftMouseDown => Some(button(ButtonEvent::Down, 0)),
        CGEventType::LeftMouseUp => Some(button(ButtonEvent::Up, 0)),
        CGEventType::ScrollWheel => {
            let vertical =
                event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1);
            let horizontal =
                event.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2);

            // Whichever axis moved more. A sideways scroll means the same thing
            // as a vertical one here.
            let delta = if vertical.abs() >= horizontal.abs() {
                vertical
            } else {
                horizontal
            };

            Some(PointerEvent::Scroll {
                delta: delta as f64,
                timestamp_ms,
            })
        }
        _ => None,
    }
}

/// Whether a declined gesture belongs in the diagnostics panel.
///
/// A click on some other button is noise — it happens constantly — unless the
/// user has asked to see which button their mouse sends. The rest are gestures
/// the user made on purpose, and staying silent about them is what makes a
/// misconfigured trigger look like a broken app.
fn is_worth_reporting(reason: Ignored) -> bool {
    !matches!(reason, Ignored::OtherButton)
}

fn describe_trigger(source: TriggerSource, button: i64) -> String {
    match source {
        TriggerSource::MouseMiddle => format!("收到触发键 {}", button),
        TriggerSource::MouseSelection => "检测到划词动作".to_string(),
        TriggerSource::KeyboardShortcut | TriggerSource::Test => "已触发".to_string(),
    }
}

fn start_platform_listener(
    running: Arc<AtomicBool>,
    tx: Sender<MouseTriggerEvent>,
    state: SharedMouseTriggerState,
) {
    use crate::macos::permission;
    use core_foundation::runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop};
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    };
    use std::time::Duration;

    std::thread::spawn(move || {
        log::info!("[mouse] CoreGraphics listener starting");

        let mut prompted_for_accessibility = false;

        while running.load(Ordering::SeqCst) {
            {
                let mut guard = lock(&state);
                guard.status = MouseTriggerStatus::Starting;
                guard.accessibility_trusted = permission::is_granted();
            }

            if !permission::is_granted() {
                {
                    let mut guard = lock(&state);
                    guard.status = MouseTriggerStatus::PermissionMissing;
                    guard.accessibility_trusted = false;
                    guard.last_error =
                        Some("macOS Accessibility permission is not granted".to_string());
                }

                if !prompted_for_accessibility {
                    prompted_for_accessibility = true;
                    log::warn!("[mouse] Accessibility permission missing; requesting macOS prompt");
                    let _ = permission::request();
                }
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }

            prompted_for_accessibility = false;
            {
                let mut guard = lock(&state);
                guard.accessibility_trusted = true;
                guard.last_error = None;
            }

            let restart_requested = Arc::new(AtomicBool::new(false));
            // Rebuilt with the tap, so a gesture interrupted by a restart cannot
            // pair its press with a release from the next generation.
            let reducer = Arc::new(Mutex::new(TriggerReducer::default()));
            let callback_running = running.clone();
            let callback_tx = tx.clone();
            let callback_restart = restart_requested.clone();
            let callback_state = state.clone();
            let callback_reducer = reducer.clone();

            let tap = CGEventTap::new(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                vec![
                    CGEventType::OtherMouseDown,
                    CGEventType::LeftMouseDown,
                    CGEventType::LeftMouseUp,
                    CGEventType::ScrollWheel,
                ],
                move |_proxy, event_type, event| {
                    if matches!(
                        event_type,
                        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
                    ) {
                        let reason = format!("Event tap disabled by macOS: {:?}", event_type);
                        log::warn!("[mouse] {}", reason);
                        {
                            let mut guard = lock(&callback_state);
                            guard.status = MouseTriggerStatus::Failed;
                            guard.last_error = Some(reason);
                        }
                        callback_restart.store(true, Ordering::SeqCst);
                        CFRunLoop::get_current().stop();
                        return None;
                    }

                    if !callback_running.load(Ordering::SeqCst) {
                        return None;
                    }

                    let Some(pointer) = pointer_event(event_type, event) else {
                        return None;
                    };

                    let options = {
                        let guard = lock(&callback_state);
                        TriggerOptions {
                            trigger_button: guard.trigger_button,
                            selection_trigger_enabled: guard.selection_trigger_enabled,
                        }
                    };

                    let decision = {
                        let mut reducer = callback_reducer
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        reducer.reduce(pointer, options)
                    };

                    // Scrolling is continuous and almost never has anything to do
                    // with us. Taking the state lock and sending a message per
                    // wheel notch would be a lot of work to decide nothing.
                    //
                    // Only scrolls: a click that decides nothing still says the
                    // tap is alive and what it saw.
                    if matches!(pointer, PointerEvent::Scroll { .. })
                        && decision == Decision::Quiet
                    {
                        return None;
                    }

                    let button = match pointer {
                        PointerEvent::Button { button, .. } => Some(button),
                        PointerEvent::Scroll { .. } => None,
                    };

                    {
                        let mut guard = lock(&callback_state);
                        guard.status = MouseTriggerStatus::EventReceived;
                        guard.accessibility_trusted = true;
                        guard.last_event_at = Some(pointer.timestamp_ms());
                        guard.last_error = None;
                        if button.is_some() {
                            guard.last_button = button;
                        }

                        let testing = guard
                            .test_active_until
                            .is_some_and(|until| until > pointer.timestamp_ms());

                        match decision {
                            Decision::Trigger(source) => {
                                guard.last_trigger_at = Some(pointer.timestamp_ms());
                                guard.last_pipeline_result = Some(describe_trigger(
                                    source,
                                    button.unwrap_or_default(),
                                ));
                            }
                            Decision::Ignored(Ignored::OtherButton) if testing => {
                                guard.last_pipeline_result = Some(format!(
                                    "收到按键 {}，当前触发键是 {}",
                                    button.unwrap_or_default(),
                                    guard.trigger_button
                                ));
                            }
                            Decision::Ignored(reason) if is_worth_reporting(reason) => {
                                guard.last_pipeline_result =
                                    Some(format!("已忽略：{}", reason.describe()));
                            }
                            // Whether a dismissal applies is the panel's to say,
                            // so nothing is written here; the receiver writes the
                            // outcome once it knows.
                            Decision::Dismiss(_) => {}
                            Decision::Quiet | Decision::Ignored(_) => {}
                        }
                    }

                    if let Decision::Trigger(source) = decision {
                        log::info!("[mouse] Trigger: source={source:?} button={button:?}");
                    }

                    let _ = callback_tx.send(MouseTriggerEvent {
                        timestamp_ms: pointer.timestamp_ms(),
                        position: pointer.position(),
                        decision,
                    });
                    None
                },
            );

            let Ok(tap) = tap else {
                let reason = "Failed to create macOS CoreGraphics event tap".to_string();
                log::error!("[mouse] {}", reason);
                {
                    let mut guard = lock(&state);
                    guard.status = MouseTriggerStatus::Failed;
                    guard.accessibility_trusted = permission::is_granted();
                    guard.last_error = Some(reason);
                }
                std::thread::sleep(Duration::from_secs(1));
                continue;
            };

            {
                let mut guard = lock(&state);
                guard.status = MouseTriggerStatus::TapCreated;
                guard.accessibility_trusted = true;
                guard.last_error = None;
            }

            let current_loop = CFRunLoop::get_current();
            let Ok(loop_source) = tap.mach_port.create_runloop_source(0) else {
                let reason = "Failed to create macOS event tap run loop source".to_string();
                log::error!("[mouse] {}", reason);
                {
                    let mut guard = lock(&state);
                    guard.status = MouseTriggerStatus::Failed;
                    guard.last_error = Some(reason);
                }
                std::thread::sleep(Duration::from_secs(1));
                continue;
            };

            current_loop.add_source(&loop_source, unsafe { kCFRunLoopCommonModes });
            tap.enable();
            {
                let mut guard = lock(&state);
                guard.status = MouseTriggerStatus::Listening;
                guard.accessibility_trusted = true;
                guard.last_error = None;
            }
            log::info!("[mouse] CoreGraphics event tap listening");

            while running.load(Ordering::SeqCst) && !restart_requested.load(Ordering::SeqCst) {
                let _ = CFRunLoop::run_in_mode(
                    unsafe { kCFRunLoopDefaultMode },
                    Duration::from_millis(500),
                    false,
                );
            }

            if running.load(Ordering::SeqCst) {
                log::info!("[mouse] Rebuilding CoreGraphics event tap");
                std::thread::sleep(Duration::from_millis(200));
            }
        }

        lock(&state).status = MouseTriggerStatus::Stopped;
        log::info!("[mouse] CoreGraphics listener stopped");
    });
}
