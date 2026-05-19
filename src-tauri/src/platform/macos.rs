#![allow(unexpected_cfgs)]

use objc::runtime::Object;
use objc::{msg_send, sel, sel_impl, class};

pub fn hide_dock_icon() {
    unsafe {
        let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        // NSApplicationActivationPolicyAccessory = 2
        let _: () = msg_send![ns_app, setActivationPolicy: 2i64];
    }
}

/// Configure a Tauri window as non-activating (won't steal focus) on macOS.
/// This is essential for the popup window so it appears without interrupting the user.
pub fn configure_popup_window(window: &tauri::WebviewWindow) {
    if let Ok(ns_window_ptr) = window.ns_window() {
        let ns_window = ns_window_ptr as *mut Object;
        unsafe {
            // NSWindowStyleMaskBorderless = 0
            // NSNonactivatingPanelMask = 1 << 7 (0x80)
            // This makes the window not activate the app when shown
            let style_mask: u64 = 0 | (1 << 7);
            let _: () = msg_send![ns_window, setStyleMask: style_mask];

            // Set the window level to floating (NSStatusWindowLevel = 25)
            let _: () = msg_send![ns_window, setLevel: 25i64];

            // Don't become key window on click
            let _: () = msg_send![ns_window, setHidesOnDeactivate: false];

            // Set background to clear for transparency
            let bg: *mut Object = msg_send![class!(NSColor), clearColor];
            let _: () = msg_send![ns_window, setBackgroundColor: bg];

            // Make the titlebar transparent
            let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: true];
        }
    }
}

extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

pub fn check_accessibility_permissions() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn open_accessibility_settings() {
    use std::process::Command;
    let _ = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}
