//! Window behaviour that only AppKit can express.

use objc::runtime::Object;
use objc::{class, msg_send, sel, sel_impl};

/// Run as an accessory: no Dock icon, no app menu. The app lives in the menu bar.
pub fn set_accessory_activation_policy(app: &tauri::AppHandle) {
    if let Err(error) = app.set_activation_policy(tauri::ActivationPolicy::Accessory) {
        log::warn!("Failed to set accessibility activation policy: {error}");
    }
}

fn activate_app_now() {
    unsafe {
        let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
    }
}

/// Bring a window to the front and give it focus.
///
/// Only for windows the user explicitly asked for, such as Settings — never for
/// the popup, which must not take focus away from the app being read.
pub fn focus_window(window: &tauri::WebviewWindow) {
    let target = window.clone();
    if let Err(error) = window.run_on_main_thread(move || {
        activate_app_now();

        if let Ok(ns_window_ptr) = target.ns_window() {
            let ns_window = ns_window_ptr as *mut Object;
            unsafe {
                let nil: *mut Object = std::ptr::null_mut();
                let _: () = msg_send![ns_window, makeKeyAndOrderFront: nil];
                let _: () = msg_send![ns_window, orderFrontRegardless];
            }
        }
    }) {
        log::warn!("Failed to focus a window on the main thread: {error}");
    }
}

/// Make a window a non-activating, floating, transparent panel.
///
/// Note what this does *not* achieve: `NSNonactivatingPanelMask` is only
/// meaningful on an `NSPanel`, and Tauri's windows are plain `NSWindow`s, so
/// setting that bit is ignored. This currently works because the app runs as an
/// accessory and nothing ever calls `makeKeyAndOrderFront` on the popup — which
/// is why the popup is described as non-activating by accident rather than by
/// construction. Replacing this with a real `NSPanel` is the fix.
pub fn configure_popup_window(window: &tauri::WebviewWindow) {
    let target = window.clone();
    if let Err(error) = window.run_on_main_thread(move || {
        if let Ok(ns_window_ptr) = target.ns_window() {
            let ns_window = ns_window_ptr as *mut Object;
            unsafe {
                // NSWindowStyleMaskBorderless | NSNonactivatingPanelMask
                let style_mask: u64 = 0 | (1 << 7);
                let _: () = msg_send![ns_window, setStyleMask: style_mask];

                // NSStatusWindowLevel
                let _: () = msg_send![ns_window, setLevel: 25i64];
                let _: () = msg_send![ns_window, setHidesOnDeactivate: false];

                let background: *mut Object = msg_send![class!(NSColor), clearColor];
                let _: () = msg_send![ns_window, setBackgroundColor: background];
                let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: true];
            }
        }
    }) {
        log::warn!("Failed to configure the popup window on the main thread: {error}");
    }
}
