//! Everything that talks to macOS.
//!
//! The crate targets macOS only, so nothing here is behind a `cfg` gate; the
//! guard is a single `compile_error!` in `lib.rs`. That keeps this module free of
//! the `#[cfg(target_os = ...)]` forks that used to be spread across the mouse
//! listener, the clipboard, and diagnostics — forks that were never compiled, let
//! alone tested, on the platform the app actually ships for.

pub mod ax;
pub mod panel;
pub mod permission;
pub mod screen;
pub mod window;
