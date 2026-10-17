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
