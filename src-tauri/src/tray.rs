use tauri::{AppHandle, Emitter, Manager};

use crate::mcp;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// System tray — closing the main window hides it (close-to-tray) so the
// process and the embedded MCP server stay alive for agents.
// ---------------------------------------------------------------------------

pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub(crate) fn mcp_status_text(enabled: bool, port: u16) -> String {
    if enabled {
        format!("MCP: running · port {port}")
    } else {
        "MCP: disabled".to_string()
    }
}

/// The tray-icon crate panics (not Err) when neither appindicator shared
/// library is present — common in Flatpak sandboxes and minimal sessions.
/// Probe with dlopen first so those sessions skip the tray instead of
/// crashing at startup.
#[cfg(target_os = "linux")]
pub(crate) fn appindicator_available() -> bool {
    use std::ffi::CString;
    for name in [
        "libayatana-appindicator3.so.1",
        "libappindicator3.so.1",
        "libayatana-appindicator3.so",
        "libappindicator3.so",
    ] {
        let Ok(c) = CString::new(name) else { continue };
        let handle = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if !handle.is_null() {
            unsafe { libc::dlclose(handle) };
            return true;
        }
    }
    false
}

pub(crate) fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let mcp_cfg = app.state::<mcp::McpServer<tauri::Wry>>().config();
    let status =
        MenuItemBuilder::with_id("mcp-status", mcp_status_text(mcp_cfg.enabled, mcp_cfg.port))
            .enabled(false)
            .build(app)?;
    *app.state::<AppState>().tray_status.lock().unwrap() = Some(status.clone());

    let menu = MenuBuilder::new(app)
        .item(&status)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&MenuItemBuilder::with_id("show", "Show LibreGene").build(app)?)
        .item(&MenuItemBuilder::with_id("quit", "Quit LibreGene").build(app)?)
        .build()?;

    TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .icon(
            app.default_window_icon()
                .expect("app icon missing")
                .clone(),
        )
        .tooltip("LibreGene")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => {
                // With unsaved projects, defer to the frontend's unsaved-
                // changes flow instead of exiting abruptly: show the main
                // window and let it confirm/discard first.
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let dirty_ids: Vec<String> = {
                        let state = app.state::<AppState>();
                        let pm = state.pm.read().await;
                        pm.all_project_ids()
                            .into_iter()
                            .filter(|id| pm.is_dirty(id))
                            .collect()
                    };
                    if dirty_ids.is_empty() {
                        app.exit(0);
                    } else {
                        show_main_window(&app);
                        let _ = app.emit("quit-requested", dirty_ids);
                    }
                });
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
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}
