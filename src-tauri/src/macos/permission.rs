//! Accessibility permission.
//!
//! Reading the selection and watching the mouse both require it, so this is the
//! first thing that can go wrong and the one the user has to be told about.

use accessibility_sys::{
    kAXTrustedCheckOptionPrompt, AXIsProcessTrusted, AXIsProcessTrustedWithOptions,
};
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;

pub fn is_granted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Ask macOS to show the "grant Accessibility access" prompt.
///
/// Only ever called once per run: macOS ignores repeat calls, and the prompt is
/// modal enough that showing it on a loop would be hostile. Returns whether
/// access is granted as of the call.
pub fn request() -> bool {
    unsafe {
        let prompt_key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let prompt_value = CFBoolean::true_value();
        let options =
            CFDictionary::from_CFType_pairs(&[(prompt_key.as_CFType(), prompt_value.as_CFType())]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
    }
}

pub fn open_settings() -> Result<(), String> {
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn()
        .map_err(|error| format!("Failed to open Accessibility settings: {error}"))?;
    Ok(())
}
