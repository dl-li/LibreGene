use tauri::{AppHandle, Emitter, State};

use libregene_core::models::Feature;

use crate::kernels::{
    codon_optimize, do_annotate_features, do_find_orfs, do_search_sequence, do_update_sequence,
};
use crate::mcp;
use crate::payload::resolve_project_id;
use crate::state::AppState;
use crate::tray::mcp_status_text;

// ---------------------------------------------------------------------------
// Tauri commands — enzymes
// ---------------------------------------------------------------------------

/// Return the full static enzyme database (all records, regardless of whether
/// they cut the current sequence).
#[tauri::command]
pub(crate) async fn get_enzyme_database() -> Result<serde_json::Value, String> {
    let db = libregene_core::enzyme::search::get_db();
    serde_json::to_value(&db.enzymes).map_err(|e| e.to_string())
}

/// Return the provider database (per-provider buffers, temps, catalog numbers).
#[tauri::command]
pub(crate) async fn get_enzyme_providers() -> Result<serde_json::Value, String> {
    let data = libregene_core::enzyme::search::get_provider_data();
    serde_json::to_value(data).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tauri commands — ORF search / sequence search / primer design
// ---------------------------------------------------------------------------

/// Find open reading frames (ATG→stop, both strands, all three frames) on the
/// active project's sequence. Returns virtual CDS features (display only),
/// shaped exactly like the ORF plugin's old JS output: id `orf-<strand><start>:<end>`,
/// name `ORF <start+1>..<end+1>`, per-strand colors, and a `qualifiers`
/// `[("orf", "true")]` entry the renderer maps back to `orf: true`.
#[tauri::command]
pub(crate) async fn find_orfs(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    min_aa: Option<usize>,
) -> Result<Vec<Feature>, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await?;

    do_find_orfs(&state.pm, &project_id, min_aa).await
}

/// Search the active project's sequence for an IUPAC-aware query on both
/// strands, mirroring `findSeqMatches` in `src/searchUtils.js`.
#[tauri::command]
pub(crate) async fn search_sequence(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<libregene_core::search::SeqMatch>, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await?;

    do_search_sequence(&state.pm, &project_id, query).await
}

/// Run automatic annotation of a project's sequence against the embedded
/// SnapGene feature database. Read-only: returns detected features (camelCase,
/// 0-based inclusive coordinates), does not modify the project. `project_id`
/// defaults to the calling window's project.
#[tauri::command]
pub(crate) async fn annotate_features(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> Result<Vec<libregene_core::annotate::AnnotatedFeature>, String> {
    let project_id = match project_id {
        Some(id) => id,
        None => resolve_project_id(&state, webview_window.label()).await?,
    };

    do_annotate_features(&state.pm, &project_id).await
}

/// Run automatic annotation on a bare sequence (no project required), for the
/// Empty-page "New Sequence" dialog's live feature preview. `circular` doubles
/// the query so origin-wrapping features are found. Read-only; returns
/// camelCase AnnotatedFeature with 0-based inclusive coordinates. Protein
/// sequences match against the translated CDS features of the database.
#[tauri::command]
pub(crate) async fn annotate_sequence(
    sequence: String,
    circular: bool,
    molecule_type: Option<String>,
) -> Result<Vec<libregene_core::annotate::AnnotatedFeature>, String> {
    tokio::task::spawn_blocking(move || {
        if molecule_type.as_deref() == Some("protein") {
            libregene_core::annotate::annotate_protein(&sequence, circular)
        } else {
            libregene_core::annotate::annotate_sequence(&sequence, circular)
        }
    })
    .await
    .map_err(|e| format!("task join error: {}", e))
}

// ---------------------------------------------------------------------------
// Tauri commands — codon optimization
// ---------------------------------------------------------------------------

/// List the built-in codon-usage species keys (e.g. "e_coli", "h_sapiens").
#[tauri::command]
pub(crate) async fn list_codon_species() -> Vec<String> {
    libregene_core::codon::list_species()
        .into_iter()
        .map(|s| s.to_string())
        .collect()
}

/// Preview a CDS/mRNA feature's synonymous codon optimization without
/// modifying the project. Returns the translated aa, the replacement codons,
/// CAI/GC before and after, per-codon repairs, and unresolved violations.
/// `method` is use_best_codon | match_codon_usage | harmonize_rca;
/// harmonize_rca additionally uses `original_species` as the source table.
#[tauri::command]
pub(crate) async fn preview_codon_optimization(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    feature_id: String,
    species: String,
    method: String,
    custom_table: Option<Vec<(char, String, f64)>>,
    original_species: Option<String>,
    avoid_enzyme_sites: Option<Vec<String>>,
    gc_window: Option<(usize, f64, f64)>,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await?;
    let project = {
        let pm = state.pm.read().await;
        pm.get_project_by_id(&project_id)
            .ok_or_else(|| "Project not found".to_string())?
            .clone()
    };
    let f_id = feature_id.clone();
    let sp = species.clone();
    let m = method.clone();
    let (_, result, coding) = tokio::task::spawn_blocking(move || {
        codon_optimize(
            &project,
            &f_id,
            &sp,
            &m,
            custom_table,
            original_species.as_deref(),
            avoid_enzyme_sites,
            gc_window,
        )
    })
    .await
    .map_err(|e| format!("task join error: {}", e))??;
    Ok(codon_optimization_json(&result, &coding, &method, &species))
}

/// Apply a synonymous codon optimization: replace the feature's coding bases
/// in the template sequence (equal-length, so feature coordinates are
/// unchanged), then recompute enzymes/primers/translations and mark the
/// project dirty. Returns the same summary as preview plus ok/message.
#[tauri::command]
pub(crate) async fn apply_codon_optimization(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    app_handle: AppHandle,
    feature_id: String,
    species: String,
    method: String,
    custom_table: Option<Vec<(char, String, f64)>>,
    original_species: Option<String>,
    avoid_enzyme_sites: Option<Vec<String>>,
    gc_window: Option<(usize, f64, f64)>,
) -> Result<serde_json::Value, String> {
    let project_id = resolve_project_id(&state, webview_window.label()).await?;
    let project = {
        let pm = state.pm.read().await;
        pm.get_project_by_id(&project_id)
            .ok_or_else(|| "Project not found".to_string())?
            .clone()
    };
    let f_id = feature_id.clone();
    let sp = species.clone();
    let m = method.clone();
    let (new_sequence, result, coding) = tokio::task::spawn_blocking(move || {
        codon_optimize(
            &project,
            &f_id,
            &sp,
            &m,
            custom_table,
            original_species.as_deref(),
            avoid_enzyme_sites,
            gc_window,
        )
    })
    .await
    .map_err(|e| format!("task join error: {}", e))??;
    do_update_sequence(
        &app_handle,
        &state.pm,
        &state.window_projects,
        &state.agent_tabs,
        Some(webview_window.label()),
        project_id.clone(),
        new_sequence,
        None,
    )
    .await?;
    let mut v = codon_optimization_json(&result, &coding, &method, &species);
    v["ok"] = serde_json::json!(true);
    v["message"] = serde_json::json!(format!(
        "Optimized CDS {} ({}): CAI {:.3} → {:.3}, {} repairs, {} unresolved",
        feature_id,
        species,
        result.cai_before,
        result.cai_after,
        result.repairs.len(),
        result.unresolved.len()
    ));
    Ok(v)
}

fn codon_optimization_json(
    result: &libregene_core::codon::OptimizeResult,
    coding: &libregene_core::codon::CodingDna,
    method: &str,
    species: &str,
) -> serde_json::Value {
    serde_json::json!({
        "aa": coding.aa,
        "codonCount": coding.codons.len(),
        "newCodons": result.new_codons,
        "caiBefore": result.cai_before,
        "caiAfter": result.cai_after,
        "gcBefore": result.gc_before,
        "gcAfter": result.gc_after,
        "repairs": result.repairs,
        "unresolved": result.unresolved,
        "method": method,
        "species": species,
    })
}

/// Submit a sequence to NCBI BLAST (fixed preset per molecule type) and open
/// the official results page in the system browser. Returns the results URL.
#[tauri::command]
pub(crate) async fn blast_submit(
    app: tauri::AppHandle,
    sequence: String,
    molecule_type: String,
) -> Result<String, String> {
    let submission = tauri::async_runtime::spawn_blocking(move || {
        libregene_core::blast::submit(&sequence, &molecule_type)
    })
    .await
    .map_err(|e| e.to_string())??;
    let url = libregene_core::blast::results_url(&submission.rid);
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(url)
}

/// Activate the decoration plugin's overlay titlebar, then show the window.
/// Windows start hidden (visible: false) so native decorations never flash.
#[cfg(target_os = "macos")]
const TL_INSET: (f32, f32) = (14.0, 22.0);

#[tauri::command]
pub(crate) fn activate_custom_titlebar(window: tauri::WebviewWindow) -> Result<(), String> {
    use tauri_plugin_decoration::WebviewWindowExt;
    window
        .create_overlay_titlebar()
        .map_err(|e| e.to_string())?;
    // Store the tuned inset in the plugin registry (the value it replays on
    // resize/focus; same semantics as tao's trafficLightPosition: x = left
    // edge, y = extra container height).
    #[cfg(target_os = "macos")]
    window
        .set_traffic_lights_inset(TL_INSET.0, TL_INSET.1)
        .map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    Ok(())
}

/// Re-assert the tuned traffic-light inset after native open/save panels.
#[tauri::command]
pub(crate) fn reassert_traffic_lights(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use tauri_plugin_decoration::WebviewWindowExt;
        let _ = window.set_traffic_lights_inset(TL_INSET.0, TL_INSET.1);
    }
    let _ = window;
    Ok(())
}

/// Fallback: restore native decorations if plugin activation fails.
#[tauri::command]
pub(crate) fn restore_native_titlebar(window: tauri::WebviewWindow) -> Result<(), String> {
    use tauri_plugin_decoration::WebviewWindowExt;
    window
        .restore_native_titlebar()
        .map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    Ok(())
}

/// Return the project_id bound to the calling window.
/// Returns null for the main window (use active project instead).
#[tauri::command]
pub(crate) async fn get_window_project_id(
    webview_window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let wp = state.window_projects.read().await;
    Ok(wp.get(webview_window.label()).cloned())
}

/// Return the lock state of the agent tab bound to `project_id`:
/// `{projectId, locked}` for agent-bound projects, null otherwise.
#[tauri::command]
pub(crate) async fn get_agent_tab_state(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let at = state.agent_tabs.read().await;
    Ok(match at.get(&project_id) {
        Some(meta) => serde_json::json!({
            "projectId": project_id,
            "locked": meta.locked,
        }),
        None => serde_json::Value::Null,
    })
}

/// Set an agent tab's lock state (the unlock/lock button in the UI). The next
/// MCP tool call on the bound project re-locks it via
/// `lock_agent_tab_for_project`.
#[tauri::command]
pub(crate) async fn set_agent_tab_locked(
    project_id: String,
    app_handle: AppHandle,
    state: State<'_, AppState>,
    locked: bool,
) -> Result<serde_json::Value, String> {
    let mut at = state.agent_tabs.write().await;
    match at.get_mut(&project_id) {
        Some(meta) => {
            meta.locked = locked;
            let _ = app_handle.emit(
                "agent-tab-lock",
                serde_json::json!({
                    "projectId": project_id,
                    "locked": locked,
                }),
            );
            Ok(serde_json::json!({"status": "ok", "locked": locked}))
        }
        None => Ok(serde_json::json!({"error": "not an agent tab"})),
    }
}

// ---------------------------------------------------------------------------
// Tauri commands — MCP server settings
// ---------------------------------------------------------------------------

/// Current MCP server runtime config (enabled + loopback port).
#[tauri::command]
pub(crate) async fn get_mcp_config(
    mcp: State<'_, mcp::McpServer<tauri::Wry>>,
) -> Result<serde_json::Value, String> {
    let cfg = mcp.config();
    Ok(serde_json::json!({
        "enabled": cfg.enabled,
        "port": cfg.port,
        "requireAuth": cfg.require_auth,
    }))
}

/// Enable/disable the MCP server, move it to a new loopback port, or toggle
/// bearer-token verification. The server is stopped/restarted in place for
/// enabled/port changes — no app restart needed. `require_auth` is applied
/// live (the middleware reads it per request) and never rotates the token.
#[tauri::command]
pub(crate) async fn set_mcp_config(
    mcp: State<'_, mcp::McpServer<tauri::Wry>>,
    state: State<'_, AppState>,
    enabled: bool,
    port: u16,
    require_auth: bool,
) -> Result<serde_json::Value, String> {
    let cfg = mcp.set_config(enabled, port, require_auth).await?;
    if let Ok(tray_status) = state.tray_status.lock() {
        if let Some(item) = tray_status.as_ref() {
            let _ = item.set_text(mcp_status_text(cfg.enabled, cfg.port));
        }
    }
    Ok(serde_json::json!({
        "enabled": cfg.enabled,
        "port": cfg.port,
        "requireAuth": cfg.require_auth,
        "status": "ok",
    }))
}

/// Return the bearer token the trusted frontend must send to talk to the
/// loopback MCP server. Only the in-app webview can reach this command;
/// combined with the Host check on the server it keeps other local processes
/// (and browser pages via DNS rebinding) from driving MCP tools.
#[tauri::command]
pub(crate) async fn get_mcp_token(
    mcp: State<'_, mcp::McpServer<tauri::Wry>>,
) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "token": mcp.auth_token() }))
}

/// Rotate the MCP bearer token on user request. The new token is persisted
/// and takes effect immediately for the running server.
#[tauri::command]
pub(crate) async fn regenerate_mcp_token(
    mcp: State<'_, mcp::McpServer<tauri::Wry>>,
) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "token": mcp.regenerate_auth_token() }))
}

/// Quit immediately, discarding unsaved changes. The frontend calls this only
/// after the user confirmed the dialog triggered by the tray's
/// `quit-requested` event.
#[tauri::command]
pub(crate) async fn force_quit(app: AppHandle) -> Result<(), String> {
    app.exit(0);
    Ok(())
}
