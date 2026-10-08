use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::RwLock;

use libregene_core::enzyme;
use libregene_core::file_io;
use libregene_core::models::{Feature, Primer, ProjectData, Segment};
use libregene_core::primer;
use libregene_core::project::ProjectManager;

use crate::payload::{
    broadcast_project_arcs, feature_mutation_response, filter_project, prune_orphan_bindings,
    sidebar_project_list, with_projects_list, ProjectParams,
};
use crate::state::{validate_user_path, AgentTabs, CODON_OUTPUT_EXTS, SEQ_EXTS};

// ---------------------------------------------------------------------------
// Shared mutation/analysis cores — called by both the Tauri commands and the
// MCP server so every path goes through identical logic (recompute, dirty,
// broadcast). Each mirrors the command it was extracted from.
// ---------------------------------------------------------------------------

pub(crate) async fn do_open_file(
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    path: String,
    record_index: Option<usize>,
) -> Result<serde_json::Value, String> {
    validate_user_path(&path, SEQ_EXTS)?;
    let id = match record_index {
        // A split multi-record FASTA opens each record as its own project; the
        // project id must differ from the file path and from sibling records.
        Some(i) => format!("{}#record-{}", path, i),
        None => path.clone(),
    };
    let path_buf = std::path::PathBuf::from(&path);

    let result =
        tokio::task::spawn_blocking(move || -> Result<ProjectData, String> {
            let mut project = match record_index {
                Some(i) => {
                    let ext = path_buf
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let molecule_type = if ext == "faa" { "protein" } else { "dna" };
                    let records = file_io::fasta::parse_fasta_all_with_molecule_type(
                        &path_buf,
                        molecule_type,
                    )
                    .map_err(|e| e.to_string())?;
                    records
                        .into_iter()
                        .nth(i)
                        .ok_or_else(|| format!("record index {} out of range", i))?
                }
                None => file_io::parse_file(&path_buf).map_err(|e| e.to_string())?,
            };
            enzyme::recompute(&mut project);
            primer::recompute(&mut project);
            Ok(project)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

    match result {
        Ok(project) => {
            let params = ProjectParams {
                enzyme_filter: Some("all".to_string()),
                row_start: None,
                row_end: None,
                cpl: None,
            };
            let return_data = filter_project(&project, &params);

            let (projects, active_id) = {
                let mut pm = pm.write().await;
                // Reopening a file whose in-memory copy has unsaved changes
                // would silently discard them — refuse instead.
                if pm.is_dirty(&id) {
                    return Ok(serde_json::json!({
                        "error": "Project is already open with unsaved changes; save or discard them before reopening the file"
                    }));
                }
                if let Err(e) = pm.load(&id, project) {
                    return Ok(serde_json::json!({"error": e}));
                }
                // A fresh load reflects the file on disk — clear any stale
                // dirty marker from a previous in-memory incarnation.
                pm.mark_clean(&id);
                (pm.list_projects(), pm.active_id().map(|s| s.to_string()))
            };
            // The load may have evicted another project; drop its bindings.
            prune_orphan_bindings(pm, wp, agent_tabs).await;

            Ok(with_projects_list(return_data, &projects, active_id.as_deref()))
        }
        Err(e) => Ok(serde_json::json!({"error": e})),
    }
}

/// A feature to embed in a newly created project (see `create_project`).
/// Coordinates are 0-based inclusive; origin-wrapping features arrive as
/// multiple `segments`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewFeatureInput {
    pub name: String,
    pub ftype: String,
    pub color: String,
    pub strand: String,
    pub segments: Vec<Segment>,
}

/// Convert a [`NewFeatureInput`] into a project [`Feature`], deriving the
/// overall bounds from the segment list. None for empty segments. Bounds come
/// from the first segment's start and the last segment's end (segments are in
/// encoding order), so an origin-wrapping feature keeps its `start > end`
/// semantics instead of being flattened by min/max.
fn feature_from_input(input: NewFeatureInput, id: &str) -> Option<Feature> {
    if input.segments.is_empty() {
        return None;
    }
    let start = input.segments.first().map(|s| s.start).unwrap_or(0);
    let end = input.segments.last().map(|s| s.end).unwrap_or(0);
    Some(Feature {
        id: id.to_string(),
        name: input.name,
        start,
        end,
        color: input.color,
        ftype: input.ftype,
        segments: input.segments,
        strand: input.strand,
        notes: String::new(),
        translation: String::new(),
        qualifiers: Vec::new(),
    })
}

/// Create a new in-memory project from pasted sequence (empty-page "New
/// Sequence"). The virtual id is `untitled-{millis}` (no extension) so the
/// frontend canDirectSave check fails and the first save goes through
/// Save As + rekey_project. Returns the same shape as `do_open_file` plus the
/// generated project id.
pub(crate) async fn do_create_project(
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    name: String,
    sequence: String,
    molecule_type: String,
    topology: String,
    features: Vec<NewFeatureInput>,
) -> Result<serde_json::Value, String> {
    let seq = sequence.to_ascii_uppercase();
    if seq.is_empty() {
        return Ok(serde_json::json!({"error": "Sequence is empty"}));
    }
    let molecule = molecule_type.trim().to_ascii_lowercase();
    let molecule_type = match molecule.as_str() {
        "rna" | "protein" => molecule,
        _ => "dna".to_string(),
    };
    // RNA/Protein are single-strand: always linear. DNA honors the toggle.
    let topo = topology.trim().to_ascii_lowercase();
    let topology = if molecule_type != "dna" {
        "linear".to_string()
    } else if topo == "linear" {
        "linear".to_string()
    } else {
        "circular".to_string()
    };

    let length = seq.len() as i64;
    for f in &features {
        for s in &f.segments {
            if s.start < 0 || s.end < 0 || s.start >= length || s.end >= length {
                return Ok(serde_json::json!({
                    "error": format!(
                        "feature '{}' segment {}..{} is out of bounds for a sequence of length {} (1-based inclusive)",
                        f.name, s.start + 1, s.end + 1, length
                    )
                }));
            }
        }
        // Encoding order (same shape annotate.rs emits): linear segments
        // ascending; an origin-wrapping feature leads with its tail, the one
        // descending transition marking the origin. Out-of-order segments
        // would make the first/last-derived bounds wrong (a phantom wrap).
        let descents = f
            .segments
            .windows(2)
            .filter(|w| w[1].start < w[0].start)
            .count();
        let ordered = f.segments.iter().all(|s| s.start <= s.end)
            && descents <= 1
            && (descents == 0
                || f.segments.first().unwrap().start > f.segments.last().unwrap().end);
        if !ordered {
            return Ok(serde_json::json!({
                "error": format!(
                    "feature '{}' segments are not in encoding order (ascending starts; a wrapping feature leads with its tail)",
                    f.name
                )
            }));
        }
    }

    let features: Vec<Feature> = features
        .into_iter()
        .enumerate()
        .filter_map(|(i, f)| feature_from_input(f, &format!("feature_{}", i)))
        .collect();

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let id = format!("untitled-{}", ts);

    let mut project = ProjectData {
        name,
        sequence: seq,
        length,
        topology,
        molecule_type,
        features,
        ..Default::default()
    };
    let computed = tokio::task::spawn_blocking(move || {
        enzyme::recompute(&mut project);
        primer::recompute(&mut project);
        libregene_core::translate::refresh_feature_translations(&mut project);
        project
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?;

    let params = ProjectParams {
        enzyme_filter: Some("all".to_string()),
        row_start: None,
        row_end: None,
        cpl: None,
    };
    let mut return_data = filter_project(&computed, &params);
    if let Some(ref mut map) = return_data.as_object_mut() {
        map.insert("id".to_string(), serde_json::json!(id));
        map.insert("dirty".to_string(), serde_json::json!(true));
    }

    let (projects, active_id) = {
        let mut pm = pm.write().await;
        if let Err(e) = pm.load(&id, computed) {
            return Ok(serde_json::json!({"error": e}));
        }
        // New in-memory projects start dirty so closing them prompts a save.
        pm.mark_dirty(&id);
        (pm.list_projects(), pm.active_id().map(|s| s.to_string()))
    };
    // The load may have evicted another project; drop its bindings.
    prune_orphan_bindings(pm, wp, agent_tabs).await;

    Ok(with_projects_list(return_data, &projects, active_id.as_deref()))
}

pub(crate) async fn do_save_file(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: String,
    path: String,
) -> Result<serde_json::Value, String> {
    let ext = validate_user_path(&path, CODON_OUTPUT_EXTS)?;
    let save_path = std::path::PathBuf::from(&path);
    // Snapshot the project and clear its dirty flag in ONE write lock: any
    // mutation landing after this point re-marks the project dirty, so a
    // concurrent edit during the blocking write is never mistaken for saved
    // (the written file wouldn't contain it). On write failure the previous
    // dirty state is restored.
    let (project, was_dirty) = {
        let mut pm = pm.write().await;
        let was_dirty = pm.is_dirty(&project_id);
        let project = pm.get_project_by_id(&project_id).cloned();
        if project.is_some() {
            pm.mark_clean(&project_id);
        }
        (project, was_dirty)
    };
    match project {
        Some(ref p) => {
            // Protein projects cannot round-trip through the DNA GenBank
            // writer (amino-acid letters would corrupt the file) — force .gpt.
            if p.molecule_type == "protein" && ext != "gpt" {
                if was_dirty {
                    pm.write().await.mark_dirty(&project_id);
                }
                return Ok(serde_json::json!({
                    "error": "Protein projects must be saved as .gpt (GenBank protein format); .gbk/.gb cannot represent an amino-acid sequence"
                }));
            }
            // Save As semantics: when the target path differs from the
            // project id (first save of an `untitled-*` project, or an
            // explicit new name in the save dialog), the chosen file stem
            // becomes the project name so it lands in the LOCUS field.
            // Direct saves to the same path keep the existing name.
            let new_name = if project_id != path {
                save_path.file_stem().map(|s| s.to_string_lossy().into_owned())
            } else {
                None
            };
            let mut p = p.clone();
            if let Some(ref n) = new_name {
                p.name = n.clone();
            }
            let write_path = save_path.clone();
            let write_ext = ext.clone();
            let result = tokio::task::spawn_blocking(move || {
                if write_ext == "gpt" {
                    file_io::gpt::write_gpt(&p, &write_path)
                } else {
                    file_io::gbk::write_gbk(&p, &write_path)
                }
            })
            .await
            .map_err(|e| format!("task join error: {}", e))?;
            match result {
                Ok(()) => {
                    if let Some(n) = new_name {
                        if let Some(live) = pm.write().await.get_project_mut_by_id(&project_id) {
                            live.name = n;
                        }
                    }
                    let bytes = std::fs::metadata(&save_path).map(|m| m.len()).unwrap_or(0);
                    Ok(serde_json::json!({"status": "ok", "bytesWritten": bytes}))
                }
                Err(e) => {
                    if was_dirty {
                        pm.write().await.mark_dirty(&project_id);
                    }
                    Ok(serde_json::json!({"error": e.to_string()}))
                }
            }
        }
        None => Ok(serde_json::json!({"error": "Project not found"})),
    }
}

/// CAS write-back of the off-lock recomputed fields after a sequence edit:
/// land only when the live sequence still equals the one the recompute ran
/// on — a concurrent edit that landed in between triggered its own recompute,
/// so writing stale results back would clobber the newer state. Primers merge
/// by id: a concurrently added primer survives, a concurrently deleted one
/// stays gone, existing ones get their fresh binding sites.
fn merge_recomputed_after_edit(live: &mut ProjectData, computed: ProjectData) -> bool {
    if live.sequence != computed.sequence {
        return false;
    }
    live.enzymes = computed.enzymes;
    for cp in computed.primers {
        if let Some(pr) = live.primers.iter_mut().find(|pr| pr.id == cp.id) {
            *pr = cp;
        }
    }
    // Refresh translations on features still at their cloned coordinates;
    // features edited concurrently keep their state.
    for cf in &computed.features {
        if let Some(f) = live
            .features
            .iter_mut()
            .find(|f| f.id == cf.id && f.start == cf.start && f.end == cf.end)
        {
            f.translation = cf.translation.clone();
        }
    }
    // Alignments recomputed against the edited template; merge by id so a
    // concurrently added/removed alignment survives.
    for ca in &computed.alignments {
        if let Some(a) = live.alignments.iter_mut().find(|a| a.id == ca.id) {
            *a = ca.clone();
        }
    }
    true
}

/// Clone the project, recompute enzymes/primers/translations/alignments
/// off-lock, CAS write the computed fields back and broadcast. Shared by
/// do_update_sequence and the MCP edit_sequence path (which writes the
/// sequence inside its own critical section, then calls this).
pub(crate) async fn recompute_after_sequence_change<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
) -> Result<(), String> {
    let project_clone = {
        let pm = pm.read().await;
        pm.get_project_by_id(project_id).map(|p| ProjectData {
            // enzyme::recompute below rebuilds the whole enzyme list from the
            // embedded database, so cloning the existing Vec<Enzyme> (the
            // single heaviest field on large plasmids) is pure waste.
            // primers are NOT skipped: primer::recompute reads the existing
            // primer definitions to recompute their binding sites.
            enzymes: Vec::new(),
            name: p.name.clone(),
            definition: p.definition.clone(),
            keywords: p.keywords.clone(),
            lab_host: p.lab_host.clone(),
            sequence: p.sequence.clone(),
            length: p.length,
            topology: p.topology.clone(),
            molecule_type: p.molecule_type.clone(),
            features: p.features.clone(),
            primers: p.primers.clone(),
            alignments: p.alignments.clone(),
            methylation_systems: p.methylation_systems.clone(),
            methylation_overlap: p.methylation_overlap,
            roi: p.roi,
            trace_path: p.trace_path.clone(),
            snapgene_history: p.snapgene_history.clone(),
        })
    };

    if let Some(mut p) = project_clone {
        let computed = tokio::task::spawn_blocking(move || {
            enzyme::recompute(&mut p);
            primer::recompute(&mut p);
            libregene_core::translate::refresh_feature_translations(&mut p);
            libregene_core::align::realign_project(&mut p);
            p
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

        // Write back ONLY the computed fields (CAS on the sequence) so
        // concurrent edits made while spawn_blocking ran (e.g. a feature
        // rename, a newer sequence edit) are not clobbered; this also leaves
        // the global active project untouched (no open_project).
        let mut pm = pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
            merge_recomputed_after_edit(p, computed);
        }
    }

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;
    Ok(())
}

/// Replace a project's sequence (and optionally its whole primer list, e.g.
/// an undo/redo snapshot — `Some` replaces `p.primers` wholesale and binding
/// sites are recomputed; `None` keeps the current primers), recompute
/// enzymes/primers/translations/alignments off-lock, write the computed
/// fields back under a sequence CAS (a stale recompute is dropped), mark
/// dirty and broadcast.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn do_update_sequence<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: String,
    sequence: String,
    primers: Option<Vec<Primer>>,
) -> Result<serde_json::Value, String> {
    {
        let mut pm = pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(&project_id) {
            p.sequence = sequence;
            p.length = p.sequence.len() as i64;
            if let Some(primers) = primers {
                p.primers = primers;
            }
            pm.mark_dirty(&project_id);
        }
    }

    recompute_after_sequence_change(app_handle, pm, wp, agent_tabs, source, &project_id).await?;

    let result = {
        let pm = pm.read().await;
        let params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        pm.get_project_by_id(&project_id)
            .map(|p| filter_project(p, &params))
            .unwrap_or(serde_json::json!({"error": "Project not found after update"}))
    };
    let (projects, active_id) = sidebar_project_list(pm, wp, agent_tabs).await;

    Ok(with_projects_list(result, &projects, active_id.as_deref()))
}

pub(crate) async fn do_activate_project(
    pm: &Arc<RwLock<ProjectManager>>,
    id: String,
) -> Result<serde_json::Value, String> {
    let activated = {
        let mut pm = pm.write().await;
        pm.activate_project(&id)
    };
    if !activated {
        return Ok(serde_json::json!({"error": format!("project not found: {}", id)}));
    }

    let result = {
        let pm = pm.read().await;
        match pm.get_project() {
            Some(p) => {
                let projects = pm.list_projects();
                let active_id = pm.active_id().map(|s| s.to_string());
                let params = ProjectParams {
                    enzyme_filter: Some("all".to_string()),
                    row_start: None,
                    row_end: None,
                    cpl: None,
                };
                let mut filtered = filter_project(p, &params);
                if let Some(ref mut map) = filtered.as_object_mut() {
                    map.insert(
                        "projects".to_string(),
                        serde_json::to_value(&projects).unwrap_or_default(),
                    );
                    map.insert(
                        "activeId".to_string(),
                        serde_json::to_value(&active_id).unwrap_or_default(),
                    );
                }
                filtered
            }
            None => serde_json::json!({"error": "project not found"}),
        }
    };

    Ok(result)
}

/// Close (unload) a project and broadcast. `force` is required to close a
/// project with unsaved changes — the dirty check and the removal happen in
/// the same `pm` write critical section so a concurrent mutation cannot slip
/// in between (TOCTOU).
pub(crate) async fn do_delete_project<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    workspace: &crate::mcp::Workspace,
    source: Option<&str>,
    id: String,
    force: bool,
) -> Result<serde_json::Value, String> {
    enum Outcome {
        Closed,
        Dirty,
        Missing,
    }
    let outcome = {
        let mut pm = pm.write().await;
        if pm.get_project_by_id(&id).is_none() {
            Outcome::Missing
        } else if pm.is_dirty(&id) && !force {
            Outcome::Dirty
        } else {
            pm.close_project(&id);
            Outcome::Closed
        }
    };
    match outcome {
        Outcome::Closed => {
            // Collect the window labels bound to this project, then drop the
            // mappings. Keeping a window open after its project is deleted
            // leaves a ghost webview whose commands would fail — and before
            // the evicted-window fix, silently redirected to the MAIN
            // window's active project, overwriting a different file.
            let orphan_labels: Vec<String> = {
                let mut wp = wp.write().await;
                let labels: Vec<String> = wp
                    .iter()
                    .filter(|(_, v)| *v == &id)
                    .map(|(k, _)| k.clone())
                    .collect();
                wp.retain(|_, v| v != &id);
                labels
            };
            {
                let mut at = agent_tabs.write().await;
                at.remove(&id);
            }
            // Workspace fragments extracted from this project die with it.
            crate::mcp::remove_fragments_of(workspace, &id).await;
            broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;
            for label in orphan_labels {
                if let Some(win) = app_handle.get_webview_window(&label) {
                    let _ = win.close();
                }
            }
            Ok(serde_json::json!({"status": "ok"}))
        }
        Outcome::Dirty => Ok(serde_json::json!({
            "error": "Project has unsaved changes — save_file first, or pass force: true to discard them"
        })),
        Outcome::Missing => Ok(serde_json::json!({"error": "project not found"})),
    }
}

/// Add or replace features (by id) and broadcast. The location string has
/// already been parsed into the Feature by the caller.
pub(crate) async fn do_add_features<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
    features: Vec<Feature>,
) -> Result<serde_json::Value, String> {
    // Merge inside the write lock: cloning the feature list under a read
    // lock and overwriting it in a later write lock would silently drop
    // concurrent feature additions/removals landing between the two locks.
    {
        let mut pm = pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
            for feature in features {
                if let Some(pos) = p.features.iter().position(|f| f.id == feature.id) {
                    p.features[pos] = feature;
                } else {
                    p.features.push(feature);
                }
            }
        }
        pm.mark_dirty(project_id);
    }

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

    Ok(feature_mutation_response(pm, wp, agent_tabs, project_id).await)
}

pub(crate) async fn do_delete_feature<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
    id: String,
) -> Result<serde_json::Value, String> {
    let feats = {
        let mut pm = pm.write().await;
        let exists = pm
            .get_project_by_id(project_id)
            .map(|p| p.features.iter().any(|f| f.id == id))
            .unwrap_or(false);
        if !exists {
            return Err(format!("Feature not found: {}", id));
        }
        let feats: Vec<Feature> = pm
            .get_project_by_id(project_id)
            .map(|p| p.features.iter().filter(|f| f.id != id).cloned().collect())
            .unwrap_or_default();
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
            p.features = feats.clone();
        }
        pm.mark_dirty(project_id);
        feats
    };

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

    let (projects, active_id) = sidebar_project_list(pm, wp, agent_tabs).await;
    Ok(with_projects_list(
        serde_json::json!({ "features": feats }),
        &projects,
        active_id.as_deref(),
    ))
}

/// Apply `apply` to the feature `feature_id`, mark dirty and broadcast.
pub(crate) async fn do_update_feature<R: Runtime, F>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
    feature_id: &str,
    apply: F,
) -> Result<serde_json::Value, String>
where
    F: FnOnce(&mut Feature) -> Result<(), String>,
{
    {
        let mut pm = pm.write().await;
        match pm.get_project_mut_by_id(project_id) {
            Some(p) => match p.features.iter_mut().find(|f| f.id == feature_id) {
                Some(f) => apply(f)?,
                None => return Err(format!("Feature not found: {}", feature_id)),
            },
            None => return Err(format!("Project not found: {}", project_id)),
        }
        pm.mark_dirty(project_id);
    }

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

    Ok(feature_mutation_response(pm, wp, agent_tabs, project_id).await)
}

/// Cheap structural equality for the primers list (Primer has no PartialEq).
/// The snapshot/merge write-back guard only needs to detect whether the list
/// membership changed (add/remove/replace), not binding-site internals.
pub(crate) fn primers_equal(a: &[Primer], b: &[Primer]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.id == y.id && x.name == y.name && x.primer_seq == y.primer_seq)
}

pub(crate) async fn do_add_primer<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
    primer: Primer,
) -> Result<serde_json::Value, String> {
    // Snapshot → merge → recompute (off-lock) → write-back. The write-back
    // only lands when the primers list is unchanged since the snapshot;
    // otherwise a concurrent add slipped in and we retry onto the fresh list
    // (binding sites depend only on the template, which does not change).
    loop {
        let name_conflict = {
            let pm = pm.read().await;
            pm.get_project_by_id(project_id).map(|p| {
                if p.primers.iter().any(|p| p.id != primer.id && p.name == primer.name) {
                    Some("primer")
                } else if p.features.iter().any(|f| f.name == primer.name) {
                    Some("feature")
                } else {
                    None
                }
            }).unwrap_or(None)
        };
        if let Some(kind) = name_conflict {
            return Ok(serde_json::json!({"error": format!("Primer name '{}' already exists as a {}", primer.name, kind)}));
        }

        let snapshot = {
            let pm = pm.read().await;
            pm.get_project_by_id(project_id).map(|p| {
                (p.sequence.clone(), p.topology.clone(), p.primers.clone())
            })
        };
        let Some((template, topology, existing)) = snapshot else {
            return Ok(serde_json::json!({"error": "Project not found"}));
        };

        let mut merged = existing.clone();
        if let Some(pos) = merged.iter().position(|p| p.id == primer.id) {
            merged[pos] = primer.clone();
        } else {
            merged.push(primer.clone());
        }

        let updated = tokio::task::spawn_blocking(move || {
            libregene_core::primer::align::recompute_all_primers(&template, &topology, &merged)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

        let wrote = {
            let mut pm = pm.write().await;
            match pm.get_project_mut_by_id(project_id) {
                Some(p) if primers_equal(&p.primers, &existing) => {
                    p.primers = updated;
                    pm.mark_dirty(project_id);
                    true
                }
                // The list changed under us (another window added a primer):
                // re-merge onto the fresh state instead of overwriting.
                Some(_) => false,
                None => return Ok(serde_json::json!({"error": "Project not found"})),
            }
        };
        if wrote {
            break;
        }
    }

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

    let pm = pm.read().await;
    match pm.get_project_by_id(project_id) {
        Some(p) => {
            let params = ProjectParams {
                enzyme_filter: Some("all".to_string()),
                row_start: None,
                row_end: None,
                cpl: None,
            };
            Ok(filter_project(p, &params))
        }
        None => Ok(serde_json::json!({"error": "Project not found"})),
    }
}

pub(crate) async fn do_delete_primer<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
    id: String,
) -> Result<serde_json::Value, String> {
    let primers = {
        let mut pm = pm.write().await;
        let exists = pm
            .get_project_by_id(project_id)
            .map(|p| p.primers.iter().any(|pr| pr.id == id))
            .unwrap_or(false);
        if !exists {
            return Err(format!("Primer not found: {}", id));
        }
        let primers: Vec<Primer> = pm
            .get_project_by_id(project_id)
            .map(|p| p.primers.iter().filter(|pr| pr.id != id).cloned().collect())
            .unwrap_or_default();
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
            p.primers = primers.clone();
        }
        pm.mark_dirty(project_id);
        primers
    };

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

    let (projects, active_id) = sidebar_project_list(pm, wp, agent_tabs).await;
    Ok(with_projects_list(
        serde_json::json!({ "primers": primers }),
        &projects,
        active_id.as_deref(),
    ))
}

pub(crate) async fn do_set_methylation<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
    systems: Vec<String>,
    overlap: Option<i64>,
) -> Result<serde_json::Value, String> {
    let systems: Vec<String> = systems
        .into_iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let overlap = overlap.unwrap_or(2);

    let project_data = {
        let mut pm = pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
            p.methylation_systems = systems;
            p.methylation_overlap = overlap;
            Some(p.clone())
        } else {
            None
        }
    };

    if let Some(mut p) = project_data {
        let computed = tokio::task::spawn_blocking(move || {
            enzyme::recompute_methylation_only(&mut p);
            p
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

        // recompute_methylation_only only touches the enzyme list — write
        // back just that field so concurrent edits survive, and leave the
        // global active project untouched (no open_project).
        let mut pm = pm.write().await;
        if let Some(p) = pm.get_project_mut_by_id(project_id) {
            p.enzymes = computed.enzymes;
        }
        pm.mark_dirty(project_id);
    }

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

    let result = {
        let pm = pm.read().await;
        let params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        pm.get_project_by_id(project_id)
            .map(|p| filter_project(p, &params))
            .unwrap_or(serde_json::json!({"error": "Project not found after methylation"}))
    };
    let (projects, active_id) = sidebar_project_list(pm, wp, agent_tabs).await;

    Ok(with_projects_list(result, &projects, active_id.as_deref()))
}

/// Toggle a DNA project's topology (circular <-> linear) and recompute
/// topology-dependent data (enzymes, primer binding sites, translations).
pub(crate) async fn do_set_topology<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
    topology: &str,
) -> Result<serde_json::Value, String> {
    let topology = topology.trim().to_ascii_lowercase();
    if topology != "circular" && topology != "linear" {
        return Err(format!("invalid topology: {}", topology));
    }

    {
        let mut pm = pm.write().await;
        match pm.get_project_mut_by_id(project_id) {
            Some(p) if p.molecule_type != "dna" => {
                return Ok(serde_json::json!({"error": "topology toggle is DNA-only"}));
            }
            Some(p) => {
                p.topology = topology;
                pm.mark_dirty(project_id);
            }
            None => return Ok(serde_json::json!({"error": "project not found"})),
        }
    }

    recompute_after_sequence_change(app_handle, pm, wp, agent_tabs, source, project_id).await?;

    let result = {
        let pm = pm.read().await;
        let params = ProjectParams {
            enzyme_filter: Some("all".to_string()),
            row_start: None,
            row_end: None,
            cpl: None,
        };
        pm.get_project_by_id(project_id)
            .map(|p| filter_project(p, &params))
            .unwrap_or(serde_json::json!({"error": "Project not found after topology change"}))
    };
    let (projects, active_id) = sidebar_project_list(pm, wp, agent_tabs).await;

    Ok(with_projects_list(result, &projects, active_id.as_deref()))
}

/// Human-readable rejection reason for a failed read alignment. The prefix is
/// kept stable so callers can detect the family (`starts_with`).
pub(crate) fn alignment_reject_message(r: libregene_core::align::AlignReject) -> String {
    use libregene_core::align::{AlignReject, MIN_ALIGNED_LEN, MIN_IDENTITY};
    match r {
        AlignReject::LowIdentity { identity, .. } => format!(
            "No significant alignment found: identity {:.3} is below the {:.2} minimum",
            identity, MIN_IDENTITY
        ),
        AlignReject::TooShort { span } => format!(
            "No significant alignment found: aligned span {} bp is below the {} bp minimum",
            span, MIN_ALIGNED_LEN
        ),
        AlignReject::NoSignificantAlignment => "No significant alignment found".to_string(),
    }
}

/// Resolve the user-selectable alignment algorithm ("smith-waterman" |
/// "blast"); unknown or missing values fall back to the default.
pub(crate) fn parse_align_algorithm(s: Option<&str>) -> libregene_core::align::AlignAlgorithm {
    s.and_then(libregene_core::align::AlignAlgorithm::parse)
        .unwrap_or_default()
}

/// CAS/merge write-back for a freshly computed alignment (same discipline as
/// do_add_primer): land the computed list wholesale only when the live
/// alignment ids still match the snapshot the new alignment's id was
/// allocated from; otherwise a concurrent add/remove slipped in — append the
/// new alignment onto the live list with an id re-allocated from the CURRENT
/// list, so the other writer's entry survives.
pub(crate) async fn commit_computed_alignment(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
    snapshot_ids: &[String],
    computed: ProjectData,
) {
    let mut pm = pm.write().await;
    if let Some(p) = pm.get_project_mut_by_id(project_id) {
        if p.alignments.len() == snapshot_ids.len()
            && p
                .alignments
                .iter()
                .zip(snapshot_ids)
                .all(|(a, id)| &a.id == id)
        {
            p.alignments = computed.alignments;
        } else if let Some(mut aln) = computed.alignments.into_iter().last() {
            aln.id = libregene_core::align::next_alignment_id(&p.alignments);
            p.alignments.push(aln);
        }
    }
    pm.mark_dirty(project_id);
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn do_add_alignment_seq<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
    name: String,
    seq: String,
    trace_path: Option<String>,
    algorithm: libregene_core::align::AlignAlgorithm,
) -> Result<serde_json::Value, String> {
    let project_clone = {
        let pm = pm.read().await;
        pm.get_project_by_id(project_id).cloned()
    };
    let project_clone = match project_clone {
        Some(p) => p,
        None => return Ok(serde_json::json!({"error": "Project not found"})),
    };
    if !project_clone.is_dna() {
        return Ok(serde_json::json!({"error": format!(
            "Alignments are only supported for DNA projects; project '{}' is a {} project",
            project_id, project_clone.molecule_type
        )}));
    }
    let snapshot_ids: Vec<String> = project_clone.alignments.iter().map(|a| a.id.clone()).collect();

    let clean_seq: String = seq
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_uppercase();
    if clean_seq.is_empty() {
        return Ok(serde_json::json!({"error": "Sequence is empty"}));
    }

    let computed = tokio::task::spawn_blocking(move || -> Result<ProjectData, String> {
        let mut p = project_clone;
        let circular = p.topology == "circular";
        let mut aln =
            libregene_core::align::align_read_checked_with(&p.sequence, &clean_seq, circular, algorithm)
                .map_err(alignment_reject_message)?;
        aln.name = if name.trim().is_empty() {
            "alignment".to_string()
        } else {
            name.trim().to_string()
        };
        aln.id = libregene_core::align::next_alignment_id(&p.alignments);
        aln.trace_path = trace_path;
        p.alignments.push(aln);
        Ok(p)
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?;

    let computed = match computed {
        Ok(p) => p,
        Err(e) if e.starts_with("No significant alignment found") => return Err(e),
        Err(e) => return Ok(serde_json::json!({"error": e})),
    };

    commit_computed_alignment(pm, project_id, &snapshot_ids, computed).await;

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

    let pm = pm.read().await;
    match pm.get_project_by_id(project_id) {
        Some(p) => {
            let params = ProjectParams {
                enzyme_filter: Some("all".to_string()),
                row_start: None,
                row_end: None,
                cpl: None,
            };
            Ok(filter_project(p, &params))
        }
        None => Ok(serde_json::json!({"error": "Project not found"})),
    }
}

pub(crate) async fn do_remove_alignment<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    wp: &Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: &AgentTabs,
    source: Option<&str>,
    project_id: &str,
    alignment_id: String,
) -> Result<serde_json::Value, String> {
    {
        let mut pm = pm.write().await;
        pm.remove_alignment(project_id, &alignment_id);
    }

    broadcast_project_arcs(app_handle, pm, wp, agent_tabs, source).await;

    let pm = pm.read().await;
    match pm.get_project_by_id(project_id) {
        Some(p) => {
            let params = ProjectParams {
                enzyme_filter: Some("all".to_string()),
                row_start: None,
                row_end: None,
                cpl: None,
            };
            Ok(filter_project(p, &params))
        }
        None => Ok(serde_json::json!({"error": "Project not found"})),
    }
}

pub(crate) async fn do_find_orfs(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
    min_aa: Option<usize>,
) -> Result<Vec<Feature>, String> {
    let (sequence, topology) = {
        let pm = pm.read().await;
        let project = pm
            .get_project_by_id(project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        (project.sequence.clone(), project.topology.clone())
    };

    let min_aa = min_aa.unwrap_or(75);
    tokio::task::spawn_blocking(move || {
        libregene_core::orf::find_orfs(&sequence, &topology, min_aa)
    })
    .await
    .map_err(|e| format!("task join error: {}", e))
}

pub(crate) async fn do_search_sequence(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
    query: String,
) -> Result<Vec<libregene_core::search::SeqMatch>, String> {
    let sequence = {
        let pm = pm.read().await;
        let project = pm
            .get_project_by_id(project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        project.sequence.clone()
    };

    tokio::task::spawn_blocking(move || {
        libregene_core::search::find_seq_matches(&sequence, &query)
    })
    .await
    .map_err(|e| format!("task join error: {}", e))
}

/// Run automatic annotation of a project's sequence against the embedded
/// SnapGene feature database. Read-only: returns detected features, never
/// modifies the project (no dirty/broadcast). Coordinates are 0-based
/// inclusive; circular sequences may report origin-wrapping features with
/// `start > end` and split `segments`. DNA projects match at the nucleotide
/// level plus the protein level for CDS (codon-optimization-proof); protein
/// projects match the amino-acid sequence against the CDS translations; RNA
/// projects are unsupported and return an empty list.
pub(crate) async fn do_annotate_features(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
) -> Result<Vec<libregene_core::annotate::AnnotatedFeature>, String> {
    let (sequence, topology, molecule_type) = {
        let pm = pm.read().await;
        let project = pm
            .get_project_by_id(project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        (
            project.sequence.clone(),
            project.topology.clone(),
            project.molecule_type.clone(),
        )
    };

    tokio::task::spawn_blocking(move || {
        if molecule_type == "protein" {
            libregene_core::annotate::annotate_protein(&sequence, topology == "circular")
        } else if molecule_type == "rna" {
            Vec::new()
        } else {
            libregene_core::annotate::annotate_sequence(&sequence, topology == "circular")
        }
    })
    .await
    .map_err(|e| format!("task join error: {}", e))
}

fn parse_optimize_method(s: &str) -> Result<libregene_core::codon::OptimizeMethod, String> {
    let normalized = s.trim().to_ascii_lowercase().replace('_', "");
    match normalized.as_str() {
        "usebestcodon" => Ok(libregene_core::codon::OptimizeMethod::UseBestCodon),
        "matchcodonusage" => Ok(libregene_core::codon::OptimizeMethod::MatchCodonUsage),
        "harmonizerca" => Ok(libregene_core::codon::OptimizeMethod::HarmonizeRca),
        _ => Err(format!(
            "unknown method '{}' (expected use_best_codon, match_codon_usage, or harmonize_rca)",
            s
        )),
    }
}

/// Resolve the codon-usage table: a caller-supplied custom table wins,
/// otherwise the built-in table for `species`.
pub(crate) fn codon_usage_table(
    species: &str,
    custom_table: Option<Vec<(char, String, f64)>>,
) -> Result<libregene_core::codon::CodonUsageTable, String> {
    match custom_table {
        Some(rows) => Ok(libregene_core::codon::table_from_custom(&rows)),
        None => match libregene_core::codon::get_table(species) {
            Some(t) => Ok(t.clone()),
            None => Err(format!(
                "unknown species '{}' (available: {})",
                species,
                libregene_core::codon::list_species().join(", ")
            )),
        },
    }
}

/// Build [`OptimizeOptions`] from the string `method` and the optional
/// source-table / avoidance / GC-window parameters shared by all callers.
pub(crate) fn codon_optimize_options(
    method: &str,
    original_species: Option<&str>,
    avoid_enzyme_sites: Option<Vec<String>>,
    gc_window: Option<(usize, f64, f64)>,
) -> Result<libregene_core::codon::OptimizeOptions, String> {
    let original_table = match original_species {
        Some(s) => Some(
            libregene_core::codon::get_table(s)
                .ok_or_else(|| format!("unknown species '{}'", s))?
                .clone(),
        ),
        None => None,
    };
    Ok(libregene_core::codon::OptimizeOptions {
        method: parse_optimize_method(method)?,
        original_table,
        avoid_enzyme_sites: avoid_enzyme_sites.unwrap_or_default(),
        gc_window,
        ..libregene_core::codon::OptimizeOptions::default()
    })
}

/// Shared codon-optimization core (Tauri commands + MCP `convert_sequence`): find
/// the CDS/mRNA feature, extract its coding sequence, run the optimizer, and
/// build the equal-length replacement sequence via `segments_on_template`
/// write-back (minus-strand pieces reverse-complemented). Read-only — callers
/// decide whether to apply via `do_update_sequence`.
pub(crate) fn codon_optimize(
    project: &ProjectData,
    feature_id: &str,
    species: &str,
    method: &str,
    custom_table: Option<Vec<(char, String, f64)>>,
    original_species: Option<&str>,
    avoid_enzyme_sites: Option<Vec<String>>,
    gc_window: Option<(usize, f64, f64)>,
) -> Result<
    (
        String,
        libregene_core::codon::OptimizeResult,
        libregene_core::codon::CodingDna,
    ),
    String,
> {
    let feature = project
        .features
        .iter()
        .find(|f| f.id == feature_id)
        .ok_or_else(|| format!("Feature not found: {}", feature_id))?;
    if feature.ftype != "CDS" && feature.ftype != "mRNA" {
        return Err(format!(
            "feature '{}' is {} (only CDS/mRNA can be codon-optimized)",
            feature_id, feature.ftype
        ));
    }
    let coding =
        libregene_core::codon::extract_codons(&project.sequence, feature, &project.topology)?;

    let table = codon_usage_table(species, custom_table)?;
    let opts = codon_optimize_options(method, original_species, avoid_enzyme_sites, gc_window)?;
    let result = libregene_core::codon::optimize_codons(&coding.codons, &table, &opts);

    let new_coding: String = result.new_codons.concat();
    let mut bytes = project.sequence.as_bytes().to_vec();
    let minus = feature.strand == "-";
    let mut off = 0usize;
    for &(s, e) in &coding.segments_on_template {
        let len = e - s + 1;
        let piece = &new_coding[off..off + len];
        if minus {
            let rc = libregene_core::utils::reverse_complement(piece);
            bytes[s..=e].copy_from_slice(rc.as_bytes());
        } else {
            bytes[s..=e].copy_from_slice(piece.as_bytes());
        }
        off += len;
    }
    let new_sequence = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    Ok((new_sequence, result, coding))
}

pub(crate) async fn do_check_primers_binding(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
    primers: Vec<Primer>,
) -> Result<serde_json::Value, String> {
    let (template, topology) = {
        let pm = pm.read().await;
        let project = pm
            .get_project_by_id(project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        (project.sequence.clone(), project.topology.clone())
    };

    let results = tokio::task::spawn_blocking(move || {
        let updated =
            libregene_core::primer::align::recompute_all_primers(&template, &topology, &primers);
        updated
            .into_iter()
            .map(|p| {
                let sites: Vec<serde_json::Value> = p
                    .binding_sites
                    .iter()
                    .map(|s| {
                        let (aligned_template, match_mask) =
                            libregene_core::primer::align::template_coverage(
                                &template, &topology, &p.primer_seq, s,
                            );
                        serde_json::json!({
                            "strand": s.strand,
                            "templateStart": s.template_start,
                            "templateEnd": s.template_end,
                            "tm": s.tm,
                            "annealLen": libregene_core::primer::align::anneal_len(
                                &template, &topology, &p.primer_seq, s,
                            ),
                            "mismatchedTail": s.five_prime_tail.len(),
                            "alignedTemplate": aligned_template,
                            "matchMask": match_mask,
                        })
                    })
                    .collect();
                serde_json::json!({
                    "id": p.id,
                    "binds": !p.binding_sites.is_empty(),
                    "site": sites.first().cloned(),
                    "bindingSiteCount": p.binding_sites.len(),
                    "sites": sites,
                })
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?;

    Ok(serde_json::json!({ "results": results }))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn do_design_primer_candidates(
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
    mode: String,
    seg: Option<Segment>,
    seg2: Option<Segment>,
    name: Option<String>,
    name1: Option<String>,
    name2: Option<String>,
    site_name: Option<String>,
    target_tm: f64,
    overlap_len: Option<usize>,
    arm_len: Option<usize>,
    mut_seq: Option<String>,
    fwd_tail: Option<String>,
    rev_tail: Option<String>,
    na_conc: Option<f64>,
    mg_conc: Option<f64>,
    dntp_conc: Option<f64>,
    tris_conc: Option<f64>,
    primer_conc: Option<f64>,
) -> Result<Vec<libregene_core::primer::design::PrimerGroup>, String> {
    let (sequence, topology) = {
        let pm = pm.read().await;
        let project = pm
            .get_project_by_id(project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        (project.sequence.clone(), project.topology.clone())
    };

    let tm_params = libregene_core::primer::thermodynamics::TmParams {
        na_conc: na_conc.unwrap_or(0.050),
        mg_conc: mg_conc.unwrap_or(0.0),
        dntp_conc: dntp_conc.unwrap_or(0.0),
        tris_conc: tris_conc.unwrap_or(0.0),
        primer_conc: primer_conc.unwrap_or(2.5e-7),
    };

    let seg1 = seg.ok_or_else(|| "Segment required for primer design".to_string())?;
    let overlap_len = overlap_len.unwrap_or(20).max(8);
    let arm_len = arm_len.unwrap_or(20).max(8);
    let mut_seq = mut_seq.unwrap_or_default();

    tokio::task::spawn_blocking(move || -> Result<Vec<libregene_core::primer::design::PrimerGroup>, String> {
        let mut groups = match mode.as_str() {
            "amplify" => {
                let name = name.unwrap_or_else(|| "Amplicon".to_string());
                match (fwd_tail, rev_tail) {
                    (None, None) => libregene_core::primer::design::build_amplify_groups(
                        &sequence, &seg1, &name, target_tm, &topology, &tm_params,
                    ),
                    (f, r) => libregene_core::primer::design::build_amplify_groups_tailed(
                        &sequence, &seg1, &name, target_tm, &topology,
                        f.as_deref().unwrap_or(""), r.as_deref().unwrap_or(""), &tm_params,
                    ),
                }
            }
            "oepcr" => {
                let seg2 = seg2.as_ref().ok_or_else(|| "Second segment required for OE-PCR".to_string())?;
                let name1 = name1.unwrap_or_else(|| "Fragment 1".to_string());
                let name2 = name2.unwrap_or_else(|| "Fragment 2".to_string());
                libregene_core::primer::design::build_oepcr_groups(
                    &sequence, &seg1, seg2, &name1, &name2, target_tm, overlap_len, &topology, &tm_params,
                )
            }
            "mutagenesis" => {
                let site_name = site_name.unwrap_or_else(|| "Mutation".to_string());
                libregene_core::primer::design::build_mutagenesis_groups(
                    &sequence, &seg1, &site_name, &mut_seq, target_tm, arm_len, &topology, &tm_params,
                )
            }
            other => return Err(format!("Unknown primer design mode: {other}")),
        };
        let segs: Vec<libregene_core::models::Segment> = match mode.as_str() {
            "amplify" | "mutagenesis" => vec![seg1],
            "oepcr" => {
                let seg2 = seg2.ok_or_else(|| "Second segment required for OE-PCR".to_string())?;
                vec![seg1, seg2]
            }
            _ => Vec::new(),
        };
        libregene_core::primer::design::unify_candidate_tm(
            &sequence, &topology, &segs, &mut groups, &tm_params,
        );
        Ok(groups)
    })
    .await
    .map_err(|e| format!("task join error: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::primers::compute_primer_alignment_sync;
    use crate::payload::resolve_project_id;
    use crate::state::{AgentTabs, AppState, TEXT_EXPORT_EXTS};

    #[test]
    fn primer_alignment_rev_tail_at_circular_end() {
        // Regression: a reverse primer whose binding site ends at the very end
        // of a circular template and carries a non-matching 5' tail must still
        // produce an alignment (the tail overhang stays unaligned).
        let template: &str = "GCCACCATGGATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCATGCGGAGCAATCACAGGTGAGCAAAAAA";
        let primer = "ccgctcgagTTTTTTGCTCACCTGTGATTGCTCC";
        let tm = libregene_core::primer::thermodynamics::TmParams {
            na_conc: 0.050, mg_conc: 0.0, dntp_conc: 0.0, tris_conc: 0.0, primer_conc: 2.5e-7,
        };
        let res = compute_primer_alignment_sync(template, true, "TIGR3-XhoI-R", primer, None, &tm)
            .expect("alignment computation failed");
        let cur = &res["current"];
        assert_eq!(cur["strand"], serde_json::json!(-1));
        let aln = cur["alignment"].as_str().expect("alignment text missing");
        assert!(aln.contains("3' <"), "expected reverse-primer arrows:\n{}", aln);
        assert!(aln.contains("GGAGCAATCACAGGTGAGCAAAAAA"), "template line:\n{}", aln);
        assert!(
            aln.contains("gagctcgcc"),
            "non-matching 5' tail must be visible as an overhang:\n{}",
            aln
        );
    }

    #[test]
    fn primer_alignment_rev_window_covers_footprint_end() {
        // Regression (MX5-R): a reverse primer annealing at the 3' end of a
        // linear template — footprint extends right of tp_3prime up to the
        // template end; the window must include those bases.
        let template: &str = "AATTTCTACTAAGTGTAGATACCGCAGCAGCGCAGTTGCGCTCAATTTCTACTAAGTGTAGATATGCGAATGCTCTGGTCAAAGCAGCTT";
        let primer = "AAGCTGCTTTGACCAGAGCATTCGCATATCTACACTTAGTAGAAATTGAGCGCAACTGCGCTGCTGCGGT";
        let tm = libregene_core::primer::thermodynamics::TmParams {
            na_conc: 0.050, mg_conc: 0.0, dntp_conc: 0.0, tris_conc: 0.0, primer_conc: 2.5e-7,
        };
        let res = compute_primer_alignment_sync(template, false, "MX5-R", primer, None, &tm)
            .expect("alignment computation failed");
        let cur = &res["current"];
        assert_eq!(cur["strand"], serde_json::json!(-1));
        assert_eq!(cur["end"], serde_json::json!(90));
        let aln = cur["alignment"].as_str().expect("alignment text missing");
        assert!(aln.contains("CAGCTT"), "template line must reach position 90:\n{}", aln);
    }



    #[test]
    fn validate_path_accepts_normal_sequence_file() {
        assert_eq!(validate_user_path("C:/some/dir/plasmid.gbk", SEQ_EXTS).unwrap(), "gbk");
        assert_eq!(validate_user_path("plasmid.fa", SEQ_EXTS).unwrap(), "fa");
        assert_eq!(validate_user_path("reads.faa", SEQ_EXTS).unwrap(), "faa");
        assert_eq!(validate_user_path("genome.gbff", SEQ_EXTS).unwrap(), "gbff");
        assert_eq!(validate_user_path("entry.gp", SEQ_EXTS).unwrap(), "gp");
        assert_eq!(validate_user_path("mystery.seq", SEQ_EXTS).unwrap(), "seq");
    }

    #[test]
    fn validate_path_rejects_empty() {
        assert!(validate_user_path("   ", SEQ_EXTS).is_err());
    }

    #[test]
    fn validate_path_rejects_parent_traversal() {
        // The headline case: a raw path string from an untrusted caller must
        // not be able to escape a directory or point at arbitrary files via ..
        assert!(validate_user_path("../etc/passwd", SEQ_EXTS).is_err());
        assert!(validate_user_path("dir/../../secret.gbk", SEQ_EXTS).is_err());
        assert!(validate_user_path("../../../../Windows/System32/x.gbk", SEQ_EXTS).is_err());
    }

    #[test]
    fn validate_path_rejects_wrong_extension() {
        // Even a benign-looking path must have a sequence/export extension,
        // so an untrusted caller can't read/write e.g. .bashrc by extension swap.
        assert!(validate_user_path("notes.txt", SEQ_EXTS).is_err());
        assert!(validate_user_path("noext", SEQ_EXTS).is_err());
    }

    #[test]
    fn validate_path_accepts_text_exports_for_write() {
        assert_eq!(validate_user_path("enzymes.csv", TEXT_EXPORT_EXTS).unwrap(), "csv");
        assert_eq!(validate_user_path("data.json", TEXT_EXPORT_EXTS).unwrap(), "json");
        assert!(validate_user_path("evil.exe", TEXT_EXPORT_EXTS).is_err());
    }

    #[test]
    fn validate_path_accepts_codon_output_exts() {
        assert_eq!(validate_user_path("out.gbk", CODON_OUTPUT_EXTS).unwrap(), "gbk");
        assert_eq!(validate_user_path("out.GB", CODON_OUTPUT_EXTS).unwrap(), "gb");
        assert_eq!(validate_user_path("out.gpt", CODON_OUTPUT_EXTS).unwrap(), "gpt");
        assert!(validate_user_path("out.fasta", CODON_OUTPUT_EXTS).is_err());
        assert!(validate_user_path("out.ab1", CODON_OUTPUT_EXTS).is_err());
        assert!(validate_user_path("out.txt", CODON_OUTPUT_EXTS).is_err());
    }

    #[tokio::test]
    async fn save_as_adopts_chosen_file_stem_as_locus() {
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write().await.open_project(
            "untitled-1".to_string(),
            ProjectData {
                name: "Untitled".to_string(),
                sequence: "GATTACAGTCGATTACAGTC".to_string(),
                length: 20,
                topology: "circular".to_string(),
                ..Default::default()
            },
        ).unwrap();
        let dir = std::env::temp_dir().join(format!("libregene-saveas-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("My Plasmid.gbk");
        let out = do_save_file(&pm, "untitled-1".to_string(), path.to_str().unwrap().to_string())
            .await
            .unwrap();
        assert!(out.get("error").is_none(), "{out}");
        // In-memory name adopted the chosen stem.
        assert_eq!(
            pm.read().await.get_project_by_id("untitled-1").unwrap().name,
            "My Plasmid"
        );
        // LOCUS carries it (spaces → underscores).
        let text = std::fs::read_to_string(&path).unwrap();
        let first_line = text.lines().next().unwrap_or("");
        assert!(first_line.starts_with("LOCUS"), "{first_line}");
        assert!(first_line.contains("My_Plasmid"), "{first_line}");
        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn direct_save_keeps_existing_locus_name() {
        let path = std::env::temp_dir()
            .join(format!("libregene-directsave-{}.gbk", std::process::id()));
        let id = path.to_str().unwrap().to_string();
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write().await.open_project(
            id.clone(),
            ProjectData {
                name: "OriginalLocus".to_string(),
                sequence: "GATTACAGTCGATTACAGTC".to_string(),
                length: 20,
                topology: "linear".to_string(),
                ..Default::default()
            },
        ).unwrap();
        let out = do_save_file(&pm, id.clone(), id.clone()).await.unwrap();
        assert!(out.get("error").is_none(), "{out}");
        let text = std::fs::read_to_string(&path).unwrap();
        let first_line = text.lines().next().unwrap_or("");
        assert!(first_line.contains("OriginalLocus"), "{first_line}");
        std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn check_primer_binding_returns_all_sites_best_first() {
        // "GATTACAGTC" occurs twice in the template (linear): 0..10 and 10..20.
        // (10-mer — a 7-mer has negative Tm under the SnapGene-aligned defaults
        // and would be filtered out by the tm threshold.)
        let tpl = "GATTACAGTCGATTACAGTC";
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write().await.open_project(
            "p1".to_string(),
            ProjectData {
                sequence: tpl.to_string(),
                length: tpl.len() as i64,
                topology: "linear".to_string(),
                ..Default::default()
            },
        ).unwrap();
        let primers = vec![Primer {
            id: "f1".to_string(),
            name: "f1".to_string(),
            r#type: "fwd".to_string(),
            primer_seq: "GATTACAGTC".to_string(),
            binding_sites: Vec::new(),
        }];
        let out = do_check_primers_binding(&pm, "p1", primers).await.unwrap();
        let results = out["results"].as_array().expect("results array");
        assert_eq!(results.len(), 1);
        let r0 = &results[0];
        assert_eq!(r0["id"], "f1");
        assert_eq!(r0["binds"], true);
        assert_eq!(r0["bindingSiteCount"], 2);
        let sites = r0["sites"].as_array().expect("sites array");
        assert_eq!(sites.len(), 2);
        // Every site carries the documented fields.
        for s in sites {
            for key in [
                "strand",
                "templateStart",
                "templateEnd",
                "tm",
                "annealLen",
                "mismatchedTail",
                "alignedTemplate",
                "matchMask",
            ] {
                assert!(s.get(key).is_some(), "site missing field {}", key);
            }
        }
        // `site` is the best (first) site; `sites` is best-first (Tm desc).
        let starts: Vec<i64> = sites
            .iter()
            .map(|s| s["templateStart"].as_i64().unwrap())
            .collect();
        assert_eq!(r0["site"]["templateStart"], sites[0]["templateStart"]);
        assert!(starts.contains(&0) && starts.contains(&10));
        let tms: Vec<f64> = sites
            .iter()
            .map(|s| s["tm"].as_f64().unwrap())
            .collect();
        assert!(tms.windows(2).all(|w| w[0] >= w[1]));
    }

    #[tokio::test]
    async fn check_primer_binding_reports_tail_coverage() {
        // Enzyme-tail primer: "GCG" protect+site-like tail whose 3'-most base
        // happens to match the template next to the anneal core. The footprint
        // stops at the first 5'-ward mismatch, but alignedTemplate/matchMask
        // must expose the tail's per-base pairing against the template.
        let tpl = "TGCGTACGCTAGCTA";
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write().await.open_project(
            "p1".to_string(),
            ProjectData {
                sequence: tpl.to_string(),
                length: tpl.len() as i64,
                topology: "linear".to_string(),
                ..Default::default()
            },
        ).unwrap();
        let primers = vec![Primer {
            id: "t1".to_string(),
            name: "t1".to_string(),
            r#type: "fwd".to_string(),
            primer_seq: "AGCGTACGCTAGCTA".to_string(),
            binding_sites: Vec::new(),
        }];
        let out = do_check_primers_binding(&pm, "p1", primers).await.unwrap();
        let r0 = &out["results"][0];
        let site = &r0["sites"][0];
        // Footprint: primer[1..] "GCGTACGCTAGCTA" matches template[1..15];
        // primer[0] 'A' faces template[0] 'T' — a mismatch the mask must show.
        assert_eq!(site["mismatchedTail"], 1);
        assert_eq!(site["alignedTemplate"], "TGCGTACGCTAGCTA");
        assert_eq!(site["matchMask"], ".||||||||||||||");
        // annealLen covers the tail bases that pair (14), beyond a nominal
        // 13-bp design core — the documented design/check discrepancy.
        assert_eq!(site["annealLen"], 14);
    }

    #[tokio::test]
    async fn resolve_project_id_never_falls_back_for_evicted_project_windows() {
        use tauri::test::{mock_builder, mock_context, noop_assets};
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write()
            .await
            .open_project(
                "p1".to_string(),
                ProjectData {
                    sequence: "GATTACA".to_string(),
                    length: 7,
                    topology: "linear".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
        app.manage(AppState {
            pm: pm.clone(),
            window_projects: Arc::new(RwLock::new(HashMap::new())),
            agent_tabs: Arc::new(RwLock::new(HashMap::new())),
            workspace: Arc::new(RwLock::new(Vec::new())),
            pending_opens: Arc::new(std::sync::Mutex::new(Vec::new())),
            tray_status: Arc::new(std::sync::Mutex::new(None)),
        });
        let state = app.state::<AppState>();

        // Main window falls back to the active project.
        assert_eq!(resolve_project_id(&state, "main").await.unwrap(), "p1");

        // Project windows resolve through their mapping.
        state
            .window_projects
            .write()
            .await
            .insert("project-x-1".to_string(), "p1".to_string());
        assert_eq!(resolve_project_id(&state, "project-x-1").await.unwrap(), "p1");

        // Evicted (mapping pruned): the window errors instead of silently
        // falling back to the main window's active project — which would
        // route its mutations to the wrong file.
        state.window_projects.write().await.remove("project-x-1");
        let err = resolve_project_id(&state, "project-x-1").await.unwrap_err();
        assert!(err.contains("evicted"), "{err}");
    }

    #[tokio::test]
    async fn concurrent_primer_adds_do_not_clobber_each_other() {
        use tauri::test::{mock_builder, mock_context, noop_assets};
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write()
            .await
            .open_project(
                "p1".to_string(),
                ProjectData {
                    sequence: "GATTACAGATTACAGATTACA".to_string(),
                    length: 21,
                    topology: "linear".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
        let wp: Arc<RwLock<HashMap<String, String>>> = Arc::new(RwLock::new(HashMap::new()));
        let agent_tabs: AgentTabs = Arc::new(RwLock::new(HashMap::new()));
        let mk = |id: &str| Primer {
            id: id.to_string(),
            name: id.to_string(),
            r#type: "fwd".to_string(),
            primer_seq: "GATTACA".to_string(),
            binding_sites: Vec::new(),
        };

        // Two adds racing on the same empty list: each snapshots the same
        // state, so a naive last-writer-wins write-back would drop one.
        let (a, b) = tokio::join!(
            do_add_primer(
                app.handle(),
                &pm,
                &wp,
                &agent_tabs,
                None,
                "p1",
                mk("f1"),
            ),
            do_add_primer(
                app.handle(),
                &pm,
                &wp,
                &agent_tabs,
                None,
                "p1",
                mk("f2"),
            ),
        );
        a.unwrap();
        b.unwrap();

        let pm = pm.read().await;
        let p = pm.get_project_by_id("p1").unwrap();
        let names: Vec<&str> = p.primers.iter().map(|pr| pr.name.as_str()).collect();
        assert!(
            names.contains(&"f1") && names.contains(&"f2"),
            "both primers must survive a concurrent add: {:?}",
            names
        );
    }

    #[tokio::test]
    async fn create_project_validates_feature_segment_encoding_order() {
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        let wp: Arc<RwLock<HashMap<String, String>>> = Arc::new(RwLock::new(HashMap::new()));
        let agent_tabs: AgentTabs = Arc::new(RwLock::new(HashMap::new()));
        let seq = "A".repeat(5000);
        let nf = |name: &str, segments: Vec<Segment>| NewFeatureInput {
            name: name.to_string(),
            ftype: "gene".to_string(),
            color: "#fff".to_string(),
            strand: "+".to_string(),
            segments,
        };

        // Origin-wrapping feature in encoding order (tail first) keeps
        // start > end semantics instead of being flattened.
        let ok = do_create_project(
            &pm,
            &wp,
            &agent_tabs,
            "circ".to_string(),
            seq.clone(),
            "dna".to_string(),
            "circular".to_string(),
            vec![nf(
                "wrap",
                vec![
                    Segment { start: 4900, end: 4999, color: None },
                    Segment { start: 0, end: 99, color: None },
                ],
            )],
        )
        .await
        .unwrap();
        let feats = ok["features"].as_array().unwrap();
        assert_eq!(feats[0]["start"], 4900);
        assert_eq!(feats[0]["end"], 99);

        // Head-before-tail disorder → explicit error instead of a phantom wrap.
        let bad = do_create_project(
            &pm,
            &wp,
            &agent_tabs,
            "circ2".to_string(),
            seq.clone(),
            "dna".to_string(),
            "circular".to_string(),
            vec![nf(
                "bad",
                vec![
                    Segment { start: 0, end: 99, color: None },
                    Segment { start: 4900, end: 4999, color: None },
                    Segment { start: 200, end: 299, color: None },
                ],
            )],
        )
        .await
        .unwrap();
        assert!(
            bad["error"].as_str().unwrap().contains("encoding order"),
            "{}",
            bad
        );

        // A segment with start > end is not a linear span → error.
        let bad2 = do_create_project(
            &pm,
            &wp,
            &agent_tabs,
            "circ3".to_string(),
            seq.clone(),
            "dna".to_string(),
            "circular".to_string(),
            vec![nf("bad2", vec![Segment { start: 100, end: 50, color: None }])],
        )
        .await
        .unwrap();
        assert!(bad2["error"].as_str().is_some(), "{}", bad2);

        // Multi-segment linear feature (ascending, no wrap) stays valid.
        let ok2 = do_create_project(
            &pm,
            &wp,
            &agent_tabs,
            "linear".to_string(),
            seq.clone(),
            "dna".to_string(),
            "linear".to_string(),
            vec![nf(
                "multi",
                vec![
                    Segment { start: 10, end: 20, color: None },
                    Segment { start: 30, end: 40, color: None },
                ],
            )],
        )
        .await
        .unwrap();
        let feats2 = ok2["features"].as_array().unwrap();
        assert_eq!(feats2[0]["start"], 10);
        assert_eq!(feats2[0]["end"], 40);
    }

    #[tokio::test]
    async fn concurrent_alignment_adds_do_not_clobber_each_other() {
        use tauri::test::{mock_builder, mock_context, noop_assets};
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        // Pseudo-random 200 bp template (repeats would confuse the aligner).
        let mut seq = String::new();
        let mut x = 7u64;
        for _ in 0..200 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            seq.push(b"ACGT"[(x >> 33) as usize & 3] as char);
        }
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write()
            .await
            .open_project(
                "p1".to_string(),
                ProjectData {
                    sequence: seq.clone(),
                    length: 200,
                    topology: "linear".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
        let wp: Arc<RwLock<HashMap<String, String>>> = Arc::new(RwLock::new(HashMap::new()));
        let agent_tabs: AgentTabs = Arc::new(RwLock::new(HashMap::new()));
        let read = seq[20..170].to_string();

        // Two adds racing on the same empty alignment list: each snapshots
        // the same state, so a naive last-writer-wins write-back would drop
        // one alignment (and both would allocate the same id).
        let (a, b) = tokio::join!(
            do_add_alignment_seq(
                app.handle(),
                &pm,
                &wp,
                &agent_tabs,
                None,
                "p1",
                "r1".to_string(),
                read.clone(),
                None,
                libregene_core::align::AlignAlgorithm::SmithWaterman,
            ),
            do_add_alignment_seq(
                app.handle(),
                &pm,
                &wp,
                &agent_tabs,
                None,
                "p1",
                "r2".to_string(),
                read.clone(),
                None,
                libregene_core::align::AlignAlgorithm::SmithWaterman,
            ),
        );
        a.unwrap();
        b.unwrap();

        let pm = pm.read().await;
        let p = pm.get_project_by_id("p1").unwrap();
        let names: Vec<&str> = p.alignments.iter().map(|al| al.name.as_str()).collect();
        assert!(
            names.contains(&"r1") && names.contains(&"r2"),
            "both alignments must survive a concurrent add: {:?}",
            names
        );
        let ids: std::collections::HashSet<&str> =
            p.alignments.iter().map(|al| al.id.as_str()).collect();
        assert_eq!(ids.len(), p.alignments.len(), "ids must be unique");
    }

    #[tokio::test]
    async fn commit_computed_alignment_merges_onto_changed_list() {
        let pm = Arc::new(RwLock::new(ProjectManager::new()));
        pm.write()
            .await
            .open_project(
                "p1".to_string(),
                ProjectData {
                    sequence: "ACGT".repeat(50),
                    length: 200,
                    topology: "linear".to_string(),
                    alignments: vec![
                        libregene_core::models::Alignment {
                            id: "aln-1".to_string(),
                            name: "old".to_string(),
                            ..Default::default()
                        },
                        // A concurrent add that slipped in after the snapshot.
                        libregene_core::models::Alignment {
                            id: "aln-9".to_string(),
                            name: "concurrent".to_string(),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
            )
            .unwrap();
        // The computed state derives from a snapshot of [aln-1] plus the new
        // alignment (id allocated from the snapshot).
        let computed = ProjectData {
            alignments: vec![
                libregene_core::models::Alignment {
                    id: "aln-1".to_string(),
                    name: "old".to_string(),
                    ..Default::default()
                },
                libregene_core::models::Alignment {
                    id: "aln-2".to_string(),
                    name: "new".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        commit_computed_alignment(&pm, "p1", &["aln-1".to_string()], computed).await;
        let pmr = pm.read().await;
        let p = pmr.get_project_by_id("p1").unwrap();
        let ids: Vec<&str> = p.alignments.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids.len(), 3, "{ids:?}");
        assert!(ids.contains(&"aln-1") && ids.contains(&"aln-9"), "{ids:?}");
        let new = p.alignments.iter().find(|a| a.name == "new").unwrap();
        assert!(
            new.id != "aln-2" || !ids[..2].contains(&"aln-2"),
            "id re-allocated from the live list: {ids:?}"
        );
    }

    #[test]
    fn merge_recomputed_after_edit_cas_and_primer_merge() {
        let mk_primer = |id: &str, sites: usize| Primer {
            id: id.to_string(),
            name: id.to_string(),
            r#type: "fwd".to_string(),
            primer_seq: "ACGT".to_string(),
            binding_sites: vec![
                libregene_core::models::PrimerBindingSite {
                    primer_id: id.to_string(),
                    strand: 1,
                    template_start: 0,
                    template_end: 4,
                    tm: 60.0,
                    gc_content: 0.5,
                    match_score: 4,
                    has_3_prime_mismatch: false,
                    five_prime_tail: String::new(),
                    three_prime_tail: String::new(),
                    alignment: Default::default(),
                };
                sites
            ],
        };
        // Same sequence: enzymes replaced, existing primer's sites updated,
        // concurrently added primer kept, concurrently deleted primer gone.
        let mut live = ProjectData {
            sequence: "ACGTACGT".to_string(),
            length: 8,
            primers: vec![mk_primer("keep", 0), mk_primer("added", 1)],
            ..Default::default()
        };
        let computed = ProjectData {
            sequence: "ACGTACGT".to_string(),
            length: 8,
            enzymes: vec![libregene_core::models::Enzyme {
                name: "EcoRI".to_string(),
                ..Default::default()
            }],
            primers: vec![mk_primer("keep", 2), mk_primer("deleted", 3)],
            ..Default::default()
        };
        assert!(merge_recomputed_after_edit(&mut live, computed));
        assert_eq!(live.enzymes.len(), 1);
        assert_eq!(live.enzymes[0].name, "EcoRI");
        let ids: Vec<&str> = live.primers.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["keep", "added"], "{ids:?}");
        assert_eq!(live.primers[0].binding_sites.len(), 2);
        assert_eq!(live.primers[1].binding_sites.len(), 1);

        // Sequence changed under the recompute: nothing is written back.
        let mut live = ProjectData {
            sequence: "TTTT".to_string(),
            length: 4,
            ..Default::default()
        };
        let computed = ProjectData {
            sequence: "ACGTACGT".to_string(),
            length: 8,
            enzymes: vec![libregene_core::models::Enzyme {
                name: "EcoRI".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(!merge_recomputed_after_edit(&mut live, computed));
        assert!(live.enzymes.is_empty());
    }
}
