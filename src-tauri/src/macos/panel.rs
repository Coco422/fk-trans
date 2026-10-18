//! The popup window, as a real `NSPanel`.
//!
//! Tauri's windows are plain `NSWindow`s, and two of the things this app needs
//! are only available on `NSPanel`:
//!
//! * **Not activating the app.** A borderless `NSWindow` cannot become key, but
//!   clicking one still activates the owning app — which would pull focus out of
//!   whatever the user is reading, and very often destroy the selection we are in
//!   the middle of translating. `NSNonactivatingPanelMask` is what prevents it,
//!   and on a plain `NSWindow` the bit is ignored.
//! * **Not swallowing the first click.** AppKit discards the first click in an
//!   inactive window unless that window is a panel that becomes key only when it
//!   needs to. For a trigger point that has to be clicked, that is the difference
//!   between working and appearing to do nothing.
//!
//! So the popup window is swizzled into a panel at startup. If that fails the app
//! still runs — the popup then behaves the way it did before, activating the app
//! on the first click — because a degraded popup is better than no app.

use crate::trigger::panel::{Placement, Transition};
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, PanelHandle, PanelLevel, StyleMask, WebviewWindowExt,
};

tauri_panel! {
    panel!(TriggerPanel {})
}

const WINDOW_LABEL: &str = "popup";

pub struct TriggerPanelWindow {
    app: AppHandle,
    panel: PanelHandle<tauri::Wry>,
    window: WebviewWindow,
}

impl TriggerPanelWindow {
    /// Turn the `popup` window into a panel, and configure it.
    pub fn adopt(app: &AppHandle) -> Option<Self> {
        let window = app.get_webview_window(WINDOW_LABEL)?;

        // `to_panel` reaches for state that `tauri_nspanel::init()` manages, and
        // panics through `AppHandle::state` when the plugin was not registered.
        // That panic cannot unwind out of Tauri's setup hook, so it aborts the
        // process — which is exactly what happened the first time this ran.
        if app
            .try_state::<tauri_nspanel::WebviewPanelManager<tauri::Wry>>()
            .is_none()
        {
            log::error!(
                "[panel] tauri_nspanel::init() was not registered; running with a plain window"
            );
            return None;
        }

        let panel = match window.to_panel::<TriggerPanel>() {
            Ok(panel) => panel,
            Err(error) => {
                log::error!("[panel] Could not convert the popup window to an NSPanel: {error}");
                return None;
            }
        };

        configure(&panel);
        log::info!("[panel] The popup window is an NSPanel");

        Some(Self {
            app: app.clone(),
            panel,
            window,
        })
    }

    /// Put the window where the panel state machine says, or hide it.
    ///
    /// The placement is in logical points — the same space Accessibility reports
    /// selections in — and Tauri converts it to backing pixels once, here.
    pub fn apply(&self, transition: Transition) {
        match transition {
            Transition::None => {}
            Transition::Hide => {
                let panel = self.panel.clone();
                self.on_main(move || panel.hide());
            }
            Transition::Show(placement) => self.show(&placement),
        }
    }

    fn show(&self, placement: &Placement) {
        use tauri::{LogicalPosition, LogicalSize, Position, Size};

        // Sized before it is moved, so a growing window is never briefly placed
        // at its old position with its new size.
        let _ = self.window.set_size(Size::Logical(LogicalSize::new(
            placement.size.width,
            placement.size.height,
        )));
        let _ = self.window.set_position(Position::Logical(LogicalPosition::new(
            placement.origin.x,
            placement.origin.y,
        )));

        log::info!(
            "[panel] Showing at ({:.0}, {:.0}) size {:.0}x{:.0}",
            placement.origin.x,
            placement.origin.y,
            placement.size.width,
            placement.size.height
        );

        let panel = self.panel.clone();
        self.on_main(move || panel.show());
    }

    /// AppKit window operations belong on the main thread; the panel handle is
    /// only `Send` so that it can be handed there.
    fn on_main(&self, work: impl FnOnce() + Send + 'static) {
        if let Err(error) = self.app.run_on_main_thread(work) {
            log::error!("[panel] Could not reach the main thread: {error}");
        }
    }
}

fn configure(panel: &PanelHandle<tauri::Wry>) {
    panel.set_level(PanelLevel::Status.value());
    panel.set_floating_panel(true);
    panel.set_becomes_key_only_if_needed(true);
    panel.set_hides_on_deactivate(false);
    panel.set_has_shadow(false);
    panel.set_transparent(true);
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .value(),
    );

    // Borderless because the panel draws itself, and nonactivating because of
    // everything described at the top of this file.
    let mask = StyleMask::empty().borderless().nonactivating_panel();
    if let Err(error) = panel.set_style_mask(mask.value()) {
        log::error!("[panel] Could not set the panel style mask: {error}");
    }
}
