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

/// Whether this process is running from inside an `.app` bundle.
///
/// macOS grants Accessibility to a specific executable, not to a product name.
/// `npm run tauri dev` runs `src-tauri/target/debug/fk-trans` directly, which is
/// a different identity from the installed `fk-trans.app` — so a permission
/// granted to one does nothing for the other, and the switch in System Settings
/// looks on while the app still reports it as missing.
pub fn is_bundled(executable: &std::path::Path) -> bool {
    executable
        .components()
        .any(|part| part.as_os_str().to_string_lossy().ends_with(".app"))
}

/// The executable macOS is judging, for the user to add by hand.
pub fn current_executable() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()
}

/// Show the executable in Finder, so it can be dragged into the Accessibility
/// list when System Settings does not offer it.
pub fn reveal_executable() -> Result<(), String> {
    let path = current_executable().ok_or("无法确定当前可执行文件的位置")?;
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(|error| format!("Failed to reveal {}: {error}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_installed_app_is_bundled() {
        assert!(is_bundled(Path::new(
            "/Applications/fk-trans.app/Contents/MacOS/fk-trans"
        )));
    }

    /// The case that made a granted permission look ignored.
    #[test]
    fn the_dev_binary_is_not_bundled() {
        assert!(!is_bundled(Path::new(
            "/Users/someone/proj/fk-trans/src-tauri/target/debug/fk-trans"
        )));
    }
}
