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
    MouseSelection,
    KeyboardShortcut,
    OcrShortcut,
    Test,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseTriggerState {
    pub status: MouseTriggerStatus,
    pub accessibility_trusted: bool,
    pub trigger_button: i64,
    pub selection_trigger_enabled: bool,
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
    pub source: TriggerSource,
    pub timestamp_ms: i64,
}

pub struct MouseListener {
    running: Arc<AtomicBool>,
}

const SELECTION_DRAG_MIN_DISTANCE_PX: f64 = 8.0;
const SELECTION_DRAG_MIN_DURATION_MS: i64 = 80;
const SELECTION_TRIGGER_COOLDOWN_MS: i64 = 700;

#[derive(Debug, Clone, Copy)]
struct DragPoint {
    x: f64,
    y: f64,
    timestamp_ms: i64,
}

#[derive(Debug, Default)]
struct SelectionDragTracker {
    start: Option<DragPoint>,
    last_trigger_at: Option<i64>,
}

fn drag_distance(a: DragPoint, b: DragPoint) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
}

fn is_selection_drag(
    start: DragPoint,
    end: DragPoint,
    last_trigger_at: Option<i64>,
    enabled: bool,
) -> bool {
    if !enabled {
        return false;
    }

    if drag_distance(start, end) < SELECTION_DRAG_MIN_DISTANCE_PX {
        return false;
    }

    if end.timestamp_ms - start.timestamp_ms < SELECTION_DRAG_MIN_DURATION_MS {
        return false;
    }

    if let Some(last) = last_trigger_at {
        if end.timestamp_ms - last < SELECTION_TRIGGER_COOLDOWN_MS {
            return false;
        }
    }

    true
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

pub fn set_selection_trigger_enabled(state: &SharedMouseTriggerState, enabled: bool) {
    let mut guard = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.selection_trigger_enabled = enabled;
    guard.last_pipeline_result = Some(format!(
        "Selection trigger {}",
        if enabled { "enabled" } else { "disabled" }
    ));
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MacosTapEventKind {
    TapDisabled,
    OtherMouseDown,
    LeftMouseDown,
    LeftMouseUp,
    Ignored,
    Invalid,
}

#[cfg(target_os = "macos")]
fn classify_macos_tap_event(
    event_type: core_graphics::event::CGEventType,
    event_is_null: bool,
) -> MacosTapEventKind {
    use core_graphics::event::CGEventType;

    match event_type {
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            MacosTapEventKind::TapDisabled
        }
        CGEventType::OtherMouseDown if event_is_null => MacosTapEventKind::Invalid,
        CGEventType::LeftMouseDown if event_is_null => MacosTapEventKind::Invalid,
        CGEventType::LeftMouseUp if event_is_null => MacosTapEventKind::Invalid,
        CGEventType::OtherMouseDown => MacosTapEventKind::OtherMouseDown,
        CGEventType::LeftMouseDown => MacosTapEventKind::LeftMouseDown,
        CGEventType::LeftMouseUp => MacosTapEventKind::LeftMouseUp,
        _ => MacosTapEventKind::Ignored,
    }
}

#[cfg(target_os = "macos")]
struct RawMouseTapContext {
    running: Arc<AtomicBool>,
    restart_requested: Arc<AtomicBool>,
    tx: Sender<MouseTriggerEvent>,
    state: SharedMouseTriggerState,
    drag_tracker: Arc<Mutex<SelectionDragTracker>>,
}

#[cfg(target_os = "macos")]
struct RawMacosEventTap {
    mach_port: core_foundation::mach_port::CFMachPort,
    _context: Box<RawMouseTapContext>,
}

#[cfg(target_os = "macos")]
impl RawMacosEventTap {
    fn new(
        running: Arc<AtomicBool>,
        tx: Sender<MouseTriggerEvent>,
        restart_requested: Arc<AtomicBool>,
        state: SharedMouseTriggerState,
        drag_tracker: Arc<Mutex<SelectionDragTracker>>,
    ) -> Result<Self, ()> {
        use core_foundation::base::TCFType;
        use core_foundation::mach_port::CFMachPort;
        use core_graphics::event::{
            CGEventMask, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
        };
        use std::ffi::c_void;

        let context = Box::new(RawMouseTapContext {
            running,
            restart_requested,
            tx,
            state,
            drag_tracker,
        });
        let context_ptr = Box::into_raw(context);
        let event_mask = (1u64 << CGEventType::OtherMouseDown as CGEventMask)
            | (1u64 << CGEventType::LeftMouseDown as CGEventMask)
            | (1u64 << CGEventType::LeftMouseUp as CGEventMask);

        let tap_ref = unsafe {
            CGEventTapCreate(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                event_mask,
                raw_macos_event_tap_callback,
                context_ptr.cast::<c_void>(),
            )
        };

        let context = unsafe { Box::from_raw(context_ptr) };
        if tap_ref.is_null() {
            return Err(());
        }

        let mach_port = unsafe { CFMachPort::wrap_under_create_rule(tap_ref) };
        Ok(Self {
            mach_port,
            _context: context,
        })
    }

    fn enable(&self) {
        use core_foundation::base::TCFType;

        unsafe {
            CGEventTapEnable(self.mach_port.as_concrete_TypeRef(), true);
        }
    }

    fn disable(&self) {
        use core_foundation::base::TCFType;

        unsafe {
            CGEventTapEnable(self.mach_port.as_concrete_TypeRef(), false);
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for RawMacosEventTap {
    fn drop(&mut self) {
        self.disable();
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn raw_macos_event_tap_callback(
    _proxy: *const std::ffi::c_void,
    event_type: core_graphics::event::CGEventType,
    event: core_graphics::sys::CGEventRef,
    user_info: *mut std::ffi::c_void,
) -> core_graphics::sys::CGEventRef {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    let result = catch_unwind(AssertUnwindSafe(|| {
        if user_info.is_null() {
            log::error!("[mouse] macOS event tap callback missing context");
            return;
        }

        let context = unsafe { &*(user_info as *const RawMouseTapContext) };
        handle_raw_macos_tap_event(context, event_type, event);
    }));

    if result.is_err() {
        log::error!("[mouse] macOS event tap callback panicked");
    }

    event
}

#[cfg(target_os = "macos")]
fn handle_raw_macos_tap_event(
    context: &RawMouseTapContext,
    event_type: core_graphics::event::CGEventType,
    event: core_graphics::sys::CGEventRef,
) {
    use core_foundation::runloop::CFRunLoop;

    match classify_macos_tap_event(event_type, event.is_null()) {
        MacosTapEventKind::TapDisabled => {
            let reason = format!("Event tap disabled by macOS: {:?}", event_type);
            log::warn!("[mouse] {}", reason);
            {
                let mut guard = context
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.status = MouseTriggerStatus::Failed;
                guard.last_error = Some(reason);
            }
            context.restart_requested.store(true, Ordering::SeqCst);
            CFRunLoop::get_current().stop();
        }
        MacosTapEventKind::Invalid => {
            log::warn!("[mouse] Ignoring macOS {:?} with null event", event_type);
        }
        MacosTapEventKind::Ignored => {}
        MacosTapEventKind::OtherMouseDown => handle_macos_other_mouse_down(context, event),
        MacosTapEventKind::LeftMouseDown => handle_macos_left_mouse_down(context, event),
        MacosTapEventKind::LeftMouseUp => handle_macos_left_mouse_up(context, event),
    }
}

#[cfg(target_os = "macos")]
fn handle_macos_other_mouse_down(
    context: &RawMouseTapContext,
    event: core_graphics::sys::CGEventRef,
) {
    use core_graphics::event::EventField;

    if !context.running.load(Ordering::SeqCst) {
        return;
    }

    let button =
        unsafe { CGEventGetIntegerValueField(event, EventField::MOUSE_EVENT_BUTTON_NUMBER) };
    let timestamp_ms = now_millis();
    let is_trigger = {
        let mut guard = context
            .state
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
            guard.last_pipeline_result = Some(format!("Trigger button {} received", button));
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
    let _ = context.tx.send(MouseTriggerEvent {
        button,
        is_trigger,
        source: TriggerSource::MouseMiddle,
        timestamp_ms,
    });
}

#[cfg(target_os = "macos")]
fn handle_macos_left_mouse_down(
    context: &RawMouseTapContext,
    event: core_graphics::sys::CGEventRef,
) {
    if !context.running.load(Ordering::SeqCst) {
        return;
    }

    let location = unsafe { CGEventGetLocation(event) };
    let timestamp_ms = now_millis();
    {
        let mut tracker = context
            .drag_tracker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        tracker.start = Some(DragPoint {
            x: location.x,
            y: location.y,
            timestamp_ms,
        });
    }
    let mut guard = context
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.status = MouseTriggerStatus::EventReceived;
    guard.accessibility_trusted = true;
    guard.last_button = Some(0);
    guard.last_event_at = Some(timestamp_ms);
    guard.last_error = None;
}

#[cfg(target_os = "macos")]
fn handle_macos_left_mouse_up(context: &RawMouseTapContext, event: core_graphics::sys::CGEventRef) {
    if !context.running.load(Ordering::SeqCst) {
        return;
    }

    let location = unsafe { CGEventGetLocation(event) };
    let timestamp_ms = now_millis();
    let end = DragPoint {
        x: location.x,
        y: location.y,
        timestamp_ms,
    };

    let (start, last_trigger_at) = {
        let mut tracker = context
            .drag_tracker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let start = tracker.start.take();
        (start, tracker.last_trigger_at)
    };

    let Some(start) = start else {
        return;
    };

    let should_trigger = {
        let mut guard = context
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.status = MouseTriggerStatus::EventReceived;
        guard.accessibility_trusted = true;
        guard.last_button = Some(0);
        guard.last_event_at = Some(timestamp_ms);
        guard.last_error = None;

        let should_trigger =
            is_selection_drag(start, end, last_trigger_at, guard.selection_trigger_enabled);
        if should_trigger {
            guard.last_trigger_at = Some(timestamp_ms);
            guard.last_pipeline_result = Some("Selection drag detected".to_string());
        }
        should_trigger
    };

    if should_trigger {
        {
            let mut tracker = context
                .drag_tracker
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            tracker.last_trigger_at = Some(timestamp_ms);
        }
        log::info!(
            "[mouse] Selection drag received: distance={:.1}",
            drag_distance(start, end)
        );
        let _ = context.tx.send(MouseTriggerEvent {
            button: 0,
            is_trigger: true,
            source: TriggerSource::MouseSelection,
            timestamp_ms,
        });
    }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: core_graphics::event::CGEventTapLocation,
        place: core_graphics::event::CGEventTapPlacement,
        options: core_graphics::event::CGEventTapOptions,
        events_of_interest: core_graphics::event::CGEventMask,
        callback: unsafe extern "C" fn(
            proxy: *const std::ffi::c_void,
            event_type: core_graphics::event::CGEventType,
            event: core_graphics::sys::CGEventRef,
            user_info: *mut std::ffi::c_void,
        ) -> core_graphics::sys::CGEventRef,
        user_info: *mut std::ffi::c_void,
    ) -> core_foundation::mach_port::CFMachPortRef;

    fn CGEventTapEnable(tap: core_foundation::mach_port::CFMachPortRef, enable: bool);

    fn CGEventGetIntegerValueField(
        event: core_graphics::sys::CGEventRef,
        field: core_graphics::event::CGEventField,
    ) -> i64;

    fn CGEventGetLocation(
        event: core_graphics::sys::CGEventRef,
    ) -> core_graphics::geometry::CGPoint;
}

#[cfg(target_os = "macos")]
fn start_platform_listener(
    running: Arc<AtomicBool>,
    tx: Sender<MouseTriggerEvent>,
    state: SharedMouseTriggerState,
) {
    use crate::platform;
    use core_foundation::runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop};
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
            let drag_tracker = Arc::new(Mutex::new(SelectionDragTracker::default()));
            let tap = RawMacosEventTap::new(
                running.clone(),
                tx.clone(),
                restart_requested.clone(),
                state.clone(),
                drag_tracker,
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

            tap.disable();
            current_loop.remove_source(&loop_source, unsafe { kCFRunLoopCommonModes });

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
        let drag_tracker = Arc::new(Mutex::new(SelectionDragTracker::default()));
        let last_mouse_pos = Arc::new(Mutex::new(None::<(f64, f64)>));
        let callback_drag_tracker = drag_tracker.clone();
        let callback_mouse_pos = last_mouse_pos.clone();
        let callback = move |event: Event| {
            if !running.load(Ordering::SeqCst) {
                return;
            }

            match event.event_type {
                EventType::MouseMove { x, y } => {
                    let mut pos = callback_mouse_pos
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    *pos = Some((x, y));
                }
                EventType::ButtonPress(Button::Left) => {
                    let timestamp_ms = now_millis();
                    if let Some((x, y)) = *callback_mouse_pos
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                    {
                        let mut tracker = callback_drag_tracker
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        tracker.start = Some(DragPoint { x, y, timestamp_ms });
                    }

                    let mut guard = callback_state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.status = MouseTriggerStatus::EventReceived;
                    guard.last_button = Some(0);
                    guard.last_event_at = Some(timestamp_ms);
                }
                EventType::ButtonRelease(Button::Left) => {
                    let timestamp_ms = now_millis();
                    let Some((x, y)) = *callback_mouse_pos
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                    else {
                        return;
                    };
                    let end = DragPoint { x, y, timestamp_ms };
                    let (start, last_trigger_at) = {
                        let mut tracker = callback_drag_tracker
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        let start = tracker.start.take();
                        (start, tracker.last_trigger_at)
                    };
                    let Some(start) = start else {
                        return;
                    };

                    let should_trigger = {
                        let mut guard = callback_state
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        guard.status = MouseTriggerStatus::EventReceived;
                        guard.last_button = Some(0);
                        guard.last_event_at = Some(timestamp_ms);

                        let should_trigger = is_selection_drag(
                            start,
                            end,
                            last_trigger_at,
                            guard.selection_trigger_enabled,
                        );
                        if should_trigger {
                            guard.last_trigger_at = Some(timestamp_ms);
                            guard.last_pipeline_result =
                                Some("Selection drag detected".to_string());
                        }
                        should_trigger
                    };

                    if should_trigger {
                        let mut tracker = callback_drag_tracker
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        tracker.last_trigger_at = Some(timestamp_ms);
                        let _ = tx.send(MouseTriggerEvent {
                            button: 0,
                            is_trigger: true,
                            source: TriggerSource::MouseSelection,
                            timestamp_ms,
                        });
                    }
                }
                EventType::ButtonPress(Button::Middle) => {
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
                        source: TriggerSource::MouseMiddle,
                        timestamp_ms,
                    });
                }
                _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f64, y: f64, timestamp_ms: i64) -> DragPoint {
        DragPoint { x, y, timestamp_ms }
    }

    #[test]
    fn selection_drag_requires_enabled_distance_and_duration() {
        let start = point(0.0, 0.0, 1_000);

        assert!(!is_selection_drag(
            start,
            point(100.0, 0.0, 1_200),
            None,
            false
        ));
        assert!(!is_selection_drag(
            start,
            point(4.0, 0.0, 1_200),
            None,
            true
        ));
        assert!(!is_selection_drag(
            start,
            point(100.0, 0.0, 1_020),
            None,
            true
        ));
        assert!(is_selection_drag(
            start,
            point(100.0, 0.0, 1_200),
            None,
            true
        ));
    }

    #[test]
    fn selection_drag_cooldown_blocks_duplicate_triggers() {
        let start = point(0.0, 0.0, 1_000);

        assert!(!is_selection_drag(
            start,
            point(100.0, 0.0, 1_200),
            Some(800),
            true
        ));
        assert!(is_selection_drag(
            start,
            point(100.0, 0.0, 1_600),
            Some(800),
            true
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_tap_disabled_is_handled_before_event_pointer_is_used() {
        use core_graphics::event::CGEventType;

        assert_eq!(
            classify_macos_tap_event(CGEventType::TapDisabledByUserInput, true),
            MacosTapEventKind::TapDisabled
        );
        assert_eq!(
            classify_macos_tap_event(CGEventType::TapDisabledByTimeout, true),
            MacosTapEventKind::TapDisabled
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_mouse_events_require_non_null_event() {
        use core_graphics::event::CGEventType;

        assert_eq!(
            classify_macos_tap_event(CGEventType::OtherMouseDown, true),
            MacosTapEventKind::Invalid
        );
        assert_eq!(
            classify_macos_tap_event(CGEventType::LeftMouseDown, true),
            MacosTapEventKind::Invalid
        );
        assert_eq!(
            classify_macos_tap_event(CGEventType::LeftMouseUp, true),
            MacosTapEventKind::Invalid
        );
        assert_eq!(
            classify_macos_tap_event(CGEventType::OtherMouseDown, false),
            MacosTapEventKind::OtherMouseDown
        );
    }
}
