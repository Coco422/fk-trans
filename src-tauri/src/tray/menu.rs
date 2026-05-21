use tauri::{
    AppHandle, Emitter, Manager,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    menu::{MenuBuilder, MenuItemBuilder, CheckMenuItemBuilder},
};
use crate::AppState;
use crate::config;
#[cfg(target_os = "macos")]
use crate::platform;

pub fn create_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let state = app.state::<AppState>();
    let is_enabled = state.config.lock().unwrap().enabled;

    let enable_item = CheckMenuItemBuilder::with_id("enable", "Enable fk-trans")
        .checked(is_enabled)
        .build(app)?;
    let settings_item = MenuItemBuilder::with_id("settings", "Settings...")
        .build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit")
        .build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&enable_item)
        .item(&settings_item)
        .separator()
        .item(&quit_item)
        .build()?;

    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "enable" => {
                let state = app.state::<AppState>();
                let mut config = state.config.lock().unwrap();
                config.enabled = !config.enabled;
                let new_state = config.enabled;
                config::save_config(&config);
                drop(config);
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.emit("config-changed", ());
                }
                let _ = enable_item.set_checked(new_state);
                log::info!("fk-trans {}", if new_state { "enabled" } else { "disabled" });
            }
            "settings" => {
                if let Some(window) = app.get_webview_window("main") {
                    reveal_window(&window);
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    reveal_window(&window);
                }
            }
        })
        .build(app)?;

    Ok(())
}

fn reveal_window(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.set_always_on_top(true);
    let _ = window.show();

    #[cfg(target_os = "macos")]
    platform::macos::focus_window(window);

    let _ = window.set_focus();

    let window = window.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let _ = window.set_always_on_top(false);
    });
}
