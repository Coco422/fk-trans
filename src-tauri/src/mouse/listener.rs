use serde::{Deserialize, Serialize};
use std::sync::mpsc::Sender;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

pub type SharedMouseTriggerState = Arc<Mutex<MouseTriggerState>>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSource {
    MouseMiddle,
    KeyboardShortcut,
    Test,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseTriggerState {
    pub status: MouseTriggerStatus,
    pub accessibility_trusted: bool,
    pub trigger_button: i64,
    pub last_button: Option<i64>,
    pub last_event_at: Option<i64>,
    pub last_trigger_at: Option<i64>,
    pub last_pipeline_at: Option<i64>,
    pub last_pipeline_source: Option<TriggerSource>,
    pub last_pipeline_result: Option<String>,
    pub last_error: Option<String>,
    pub test_active_until: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct MouseTriggerEvent {
    pub button: i64,
    pub is_trigger: bool,
    pub timestamp_ms: i64,
}

pub struct MouseListener {
    running: Arc<AtomicBool>,
}

impl MouseTriggerState {
    pub fn new(trigger_button: i64) -> Self {
        Self {
            status: MouseTriggerStatus::Starting,
            accessibility_trusted: false,
            trigger_button,
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

pub fn new_shared_state(trigger_button: i64) -> SharedMouseTriggerState {
    Arc::new(Mutex::new(MouseTriggerState::new(trigger_button)))
}

pub fn snapshot(state: &SharedMouseTriggerState) -> MouseTriggerState {
    match state.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

pub fn set_trigger_button(state: &SharedMouseTriggerState, button: i64) {
    let mut guard = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.trigger_button = button;
    guard.last_pipeline_result = Some(format!("Trigger button set to {}", button));
}

pub fn start_test_window(state: &SharedMouseTriggerState, duration_ms: i64) -> MouseTriggerState {
    let mut guard = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.test_active_until = Some(now_millis() + duration_ms);
    guard.last_pipeline_result = Some("Middle click test started".to_string());
    guard.clone()
}

pub fn mark_pipeline_triggered(
    state: &SharedMouseTriggerState,
    source: TriggerSource,
) -> MouseTriggerState {
    let mut guard = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    let mut guard = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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

impl Drop for MouseListener {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "macos")]
fn start_platform_listener(
    running: Arc<AtomicBool>,
    tx: Sender<MouseTriggerEvent>,
    state: SharedMouseTriggerState,
) {
    use crate::platform;
    use core_foundation::runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop};
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
        EventField,
    };
    use std::time::Duration;

    std::thread::spawn(move || {
        log::info!("[mouse] CoreGraphics listener starting");

        let mut prompted_for_accessibility = false;

        while running.load(Ordering::SeqCst) {
            {
                let mut guard = state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.status = MouseTriggerStatus::Starting;
                guard.accessibility_trusted = platform::macos::check_accessibility_permissions();
            }

            if !platform::macos::check_accessibility_permissions() {
                {
                    let mut guard = state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.status = MouseTriggerStatus::PermissionMissing;
                    guard.accessibility_trusted = false;
                    guard.last_error =
                        Some("macOS Accessibility permission is not granted".to_string());
                }

                if !prompted_for_accessibility {
                    prompted_for_accessibility = true;
                    log::warn!("[mouse] Accessibility permission missing; requesting macOS prompt");
                    let _ = platform::macos::request_accessibility_permissions();
                }
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }

            prompted_for_accessibility = false;
            {
                let mut guard = state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.accessibility_trusted = true;
                guard.last_error = None;
            }

            let restart_requested = Arc::new(AtomicBool::new(false));
            let callback_running = running.clone();
            let callback_tx = tx.clone();
            let callback_restart = restart_requested.clone();
            let callback_state = state.clone();

            let tap = CGEventTap::new(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                vec![CGEventType::OtherMouseDown],
                move |_proxy, event_type, event| {
                    if matches!(
                        event_type,
                        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
                    ) {
                        let reason = format!("Event tap disabled by macOS: {:?}", event_type);
                        log::warn!("[mouse] {}", reason);
                        {
                            let mut guard = callback_state
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            guard.status = MouseTriggerStatus::Failed;
                            guard.last_error = Some(reason);
                        }
                        callback_restart.store(true, Ordering::SeqCst);
                        CFRunLoop::get_current().stop();
                        return None;
                    }

                    if !callback_running.load(Ordering::SeqCst)
                        || !matches!(event_type, CGEventType::OtherMouseDown)
                    {
                        return None;
                    }

                    let button =
                        event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER);
                    let timestamp_ms = now_millis();
                    let is_trigger = {
                        let mut guard = callback_state
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        let is_trigger = button == guard.trigger_button;
                        guard.status = MouseTriggerStatus::EventReceived;
                        guard.accessibility_trusted = true;
                        guard.last_button = Some(button);
                        guard.last_event_at = Some(timestamp_ms);
                        guard.last_error = None;
                        if is_trigger {
                            guard.last_trigger_at = Some(timestamp_ms);
                            guard.last_pipeline_result =
                                Some(format!("Trigger button {} received", button));
                        } else if guard
                            .test_active_until
                            .is_some_and(|until| until > timestamp_ms)
                        {
                            guard.last_pipeline_result = Some(format!(
                                "Received button {}, configured trigger is {}",
                                button, guard.trigger_button
                            ));
                        }
                        is_trigger
                    };

                    log::info!(
                        "[mouse] OtherMouseDown received: button={}, trigger={}",
                        button,
                        is_trigger
                    );
                    let _ = callback_tx.send(MouseTriggerEvent {
                        button,
                        is_trigger,
                        timestamp_ms,
                    });
                    None
                },
            );

            let Ok(tap) = tap else {
                let reason = "Failed to create macOS CoreGraphics event tap".to_string();
                log::error!("[mouse] {}", reason);
                {
                    let mut guard = state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.status = MouseTriggerStatus::Failed;
                    guard.accessibility_trusted =
                        platform::macos::check_accessibility_permissions();
                    guard.last_error = Some(reason);
                }
                std::thread::sleep(Duration::from_secs(1));
                continue;
            };

            {
                let mut guard = state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.status = MouseTriggerStatus::TapCreated;
                guard.accessibility_trusted = true;
                guard.last_error = None;
            }

            let current_loop = CFRunLoop::get_current();
            let Ok(loop_source) = tap.mach_port.create_runloop_source(0) else {
                let reason = "Failed to create macOS event tap run loop source".to_string();
                log::error!("[mouse] {}", reason);
                {
                    let mut guard = state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.status = MouseTriggerStatus::Failed;
                    guard.last_error = Some(reason);
                }
                std::thread::sleep(Duration::from_secs(1));
                continue;
            };

            current_loop.add_source(&loop_source, unsafe { kCFRunLoopCommonModes });
            tap.enable();
            {
                let mut guard = state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
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

        {
            let mut guard = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.status = MouseTriggerStatus::Stopped;
        }
        log::info!("[mouse] CoreGraphics listener stopped");
    });
}

#[cfg(not(target_os = "macos"))]
fn start_platform_listener(
    running: Arc<AtomicBool>,
    tx: Sender<MouseTriggerEvent>,
    state: SharedMouseTriggerState,
) {
    use rdev::{listen, Button, Event, EventType};

    std::thread::spawn(move || {
        {
            let mut guard = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.status = MouseTriggerStatus::Listening;
            guard.accessibility_trusted = true;
        }

        let callback_state = state.clone();
        let callback = move |event: Event| {
            if !running.load(Ordering::SeqCst) {
                return;
            }

            if let EventType::ButtonPress(Button::Middle) = event.event_type {
                let timestamp_ms = now_millis();
                {
                    let mut guard = callback_state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.status = MouseTriggerStatus::EventReceived;
                    guard.last_button = Some(2);
                    guard.last_event_at = Some(timestamp_ms);
                    guard.last_trigger_at = Some(timestamp_ms);
                }
                let _ = tx.send(MouseTriggerEvent {
                    button: 2,
                    is_trigger: true,
                    timestamp_ms,
                });
            }
        };

        if let Err(e) = listen(callback) {
            let reason = format!("Mouse listener error: {:?}", e);
            log::error!("[mouse] {}", reason);
            let mut guard = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.status = MouseTriggerStatus::Failed;
            guard.last_error = Some(reason);
        }
    });
}
