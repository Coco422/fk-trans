#![allow(unexpected_cfgs)]

use core_foundation::{
    base::TCFType,
    boolean::CFBoolean,
    dictionary::{CFDictionary, CFDictionaryRef},
    string::{CFString, CFStringRef},
};
use objc::runtime::Object;
use objc::{class, msg_send, sel, sel_impl};

pub fn set_accessory_activation_policy(app: &tauri::AppHandle) {
    if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Accessory) {
        log::warn!("Failed to set macOS accessory activation policy: {}", e);
    }
}

fn activate_app_now() {
    unsafe {
        let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
    }
}

pub fn focus_window(window: &tauri::WebviewWindow) {
    let target = window.clone();
    if let Err(e) = window.run_on_main_thread(move || {
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
        log::warn!("Failed to focus macOS window on main thread: {}", e);
    }
}

/// Configure a Tauri window as non-activating (won't steal focus) on macOS.
/// This is essential for the popup window so it appears without interrupting the user.
pub fn configure_popup_window(window: &tauri::WebviewWindow) {
    let target = window.clone();
    if let Err(e) = window.run_on_main_thread(move || {
        if let Ok(ns_window_ptr) = target.ns_window() {
            let ns_window = ns_window_ptr as *mut Object;
            unsafe {
                // NSWindowStyleMaskBorderless = 0
                // NSNonactivatingPanelMask = 1 << 7 (0x80)
                let style_mask: u64 = 0 | (1 << 7);
                let _: () = msg_send![ns_window, setStyleMask: style_mask];

                // Set the window level to floating (NSStatusWindowLevel = 25)
                let _: () = msg_send![ns_window, setLevel: 25i64];

                let _: () = msg_send![ns_window, setHidesOnDeactivate: false];

                let bg: *mut Object = msg_send![class!(NSColor), clearColor];
                let _: () = msg_send![ns_window, setBackgroundColor: bg];

                let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: true];
            }
        }
    }) {
        log::warn!("Failed to configure popup window on main thread: {}", e);
    }
}

extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

pub fn check_accessibility_permissions() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn request_accessibility_permissions() -> bool {
    unsafe {
        let prompt_key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let prompt_value = CFBoolean::true_value();
        let options =
            CFDictionary::from_CFType_pairs(&[(prompt_key.as_CFType(), prompt_value.as_CFType())]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
    }
}
