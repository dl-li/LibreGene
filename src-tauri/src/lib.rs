use std::collections::HashMap;
use std::sync::Arc;

use tauri::Manager;
use tokio::sync::RwLock;

use libregene_core::project::ProjectManager;

mod commands;
mod kernels;
mod mcp;
mod payload;
mod state;
mod tray;

pub(crate) use kernels::*;
pub(crate) use payload::*;
pub(crate) use state::*;
pub(crate) use tray::*;

// ---------------------------------------------------------------------------
// App entry point
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_decoration::init())
        // Keep one process on Windows/Linux: a second launch (e.g. opening
        // another file from Explorer) forwards its argv to the running
        // instance instead of spawning a new one. No-op on macOS, where the
        // OS routes open requests to the running app via RunEvent::Opened.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            queue_open_targets(app, collect_open_targets(argv.into_iter().skip(1)));
        }))
        // Close-to-tray: closing the main window only hides it, keeping the
        // process (and the MCP server) alive. Project windows close normally.
        // Without a tray icon there is no way to bring a hidden window back,
        // so in that case let the window close normally.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main"
                    && window.app_handle().tray_by_id("main-tray").is_some()
                {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .manage(AppState {
            pm: Arc::new(RwLock::new(ProjectManager::new())),
            window_projects: Arc::new(RwLock::new(HashMap::new())),
            agent_tabs: Arc::new(RwLock::new(HashMap::new())),
            workspace: Arc::new(RwLock::new(Vec::new())),
            pending_opens: Arc::new(std::sync::Mutex::new(Vec::new())),
            tray_status: Arc::new(std::sync::Mutex::new(None)),
        })
        .setup(|app| {
            let state = app.state::<AppState>();
            let mcp = mcp::McpServer::new(
                app.handle().clone(),
                state.pm.clone(),
                state.window_projects.clone(),
                state.agent_tabs.clone(),
                state.workspace.clone(),
                {
                    // Server state can change without a set_config call (bind
                    // failure): refresh the tray status line so it matches
                    // what get_mcp_config reports.
                    let app = app.handle().clone();
                    move |enabled, port| {
                        let tray_status = app.state::<AppState>().tray_status.clone();
                        let guard = tray_status.lock();
                        if let Ok(guard) = guard {
                            if let Some(item) = guard.as_ref() {
                                let _ = item.set_text(mcp_status_text(enabled, port));
                            }
                        }
                    }
                },
            );
            app.manage(mcp.clone());
            // Start with the default config (enabled on MCP_PORT); the frontend
            // reconciles with the persisted localStorage config on mount.
            tauri::async_runtime::block_on(async move { mcp.apply().await });
            // Best-effort: Linux sessions without an appindicator service
            // (e.g. Flatpak sandboxes lacking the library) get no tray icon
            // instead of a failed startup. The tray-icon crate panics when
            // the shared library is missing, so probe before calling in.
            #[cfg(target_os = "linux")]
            let indicator_ok = appindicator_available();
            #[cfg(not(target_os = "linux"))]
            let indicator_ok = true;
            if indicator_ok {
                if let Err(e) = setup_tray(app) {
                    eprintln!("system tray unavailable: {e}");
                }
            }
            // Cold-start file open (Windows/Linux: path passed in argv).
            queue_open_targets(
                app.handle(),
                collect_open_targets(std::env::args().skip(1)),
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::projects::get_project,
            commands::projects::get_project_by_id,
            commands::projects::open_file,
            commands::projects::peek_fasta_records,
            commands::projects::take_pending_opens,
            commands::projects::create_project,
            commands::projects::save_file,
            commands::projects::write_text_file,
            commands::seqedit::update_sequence,
            commands::seqedit::set_roi,
            commands::seqedit::clear_roi,
            commands::seqedit::set_topology,
            commands::features::get_features,
            commands::features::add_feature,
            commands::features::delete_feature,
            commands::features::update_feature_ftype,
            commands::features::update_feature_color,
            commands::features::update_feature_name,
            commands::features::update_feature_strand,
            commands::features::update_feature_location,
            commands::primers::get_primers,
            commands::primers::add_primer,
            commands::primers::add_primers,
            commands::primers::delete_primer,
            commands::primers::check_primers_binding,
            commands::primers::compute_primer_alignment,
            commands::primers::design_primer_candidates,
            commands::misc::find_orfs,
            commands::misc::search_sequence,
            commands::misc::annotate_features,
            commands::misc::annotate_sequence,
            commands::misc::list_codon_species,
            commands::misc::preview_codon_optimization,
            commands::misc::apply_codon_optimization,
            commands::misc::get_enzyme_database,
            commands::misc::get_enzyme_providers,
            commands::alignments::add_alignment,
            commands::alignments::add_alignment_seq,
            commands::alignments::remove_alignment,
            commands::alignments::get_chromatogram,
            commands::alignments::get_snapgene_history,
            commands::alignments::open_snapgene_snapshot,
            commands::seqedit::set_methylation,
            commands::projects::get_projects,
            commands::projects::activate_project,
            commands::projects::delete_project,
            commands::projects::open_in_new_window,
            commands::misc::get_window_project_id,
            commands::misc::get_agent_tab_state,
            commands::misc::set_agent_tab_locked,
            commands::projects::rekey_project,
            commands::primers::compute_tm,
            commands::misc::blast_submit,
            commands::misc::get_mcp_config,
            commands::misc::set_mcp_config,
            commands::misc::get_mcp_token,
            commands::misc::regenerate_mcp_token,
            commands::misc::activate_custom_titlebar,
            commands::misc::reassert_traffic_lights,
            commands::misc::restore_native_titlebar,
            commands::misc::force_quit,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    app.run(|app_handle, event| {
        // macOS routes Open With / double-click / dock drops here (including
        // cold start). Windows/Linux open targets arrive via argv instead.
        #[cfg(target_os = "macos")]
        match event {
            tauri::RunEvent::Opened { urls } => {
                let paths = urls
                    .into_iter()
                    .filter_map(|u| u.to_file_path().ok())
                    .filter_map(|p| p.to_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>();
                queue_open_targets(app_handle, collect_open_targets(paths));
            }
            // Clicking the Dock icon with no visible windows reopens the
            // (hidden) main window.
            tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => show_main_window(app_handle),
            _ => {}
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app_handle, event);
    });
}
