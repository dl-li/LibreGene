//! Session-scoped sequence workspace: an in-memory, agent-only staging area
//! for sequence fragments (feature/region/enzyme-digest extracts) that other
//! tools can consume by hash. Nothing here is persisted — the workspace dies
//! with the app process.
//!
//! Open projects are implicit workspace members: they are not copied into the
//! Vec, `list_workspace` and `resolve_workspace_hash` merge them in live
//! (hashes computed from the current in-memory sequence, so project edits
//! never leave a stale hash behind).

use std::sync::Arc;

use rmcp::{ErrorData, handler::server::wrapper::Json};
use tauri::Runtime;
use tokio::sync::RwLock;

use libregene_core::models::{Feature, Primer, ProjectData};
use libregene_core::project::ProjectManager;
use libregene_core::utils::{orientation_hashes, to_rna};

use crate::mcp::LibreGeneMcp;
use crate::mcp::next_id;
use crate::mcp::support::{fail_envelope, from1, insert_seq_hashes, ok_envelope, unit_for};
use crate::mcp::tools::{build_export_data, fragment_pieces, resolve_export_region};
use crate::mcp::types::{AddToWorkspaceRequest, RegionSpec};

/// One staged fragment. `sequence` is uppercase and linear; `features` /
/// `primers` are already rebased to [0, len-1].
pub(crate) struct WorkspaceItem {
    pub id: String,
    pub name: String,
    pub sequence: String,
    pub molecule_type: String,
    pub features: Vec<Feature>,
    pub primers: Vec<Primer>,
    /// Human-readable provenance, e.g. "proj.gbk (feature 'ampR')".
    pub source: String,
    /// true for add_to_workspace extracts (agent-only, temporary); false for
    /// the implicit membership of an open project.
    pub temporary: bool,
}

pub(crate) type Workspace = Arc<RwLock<Vec<WorkspaceItem>>>;

/// `{id, name, length, moleculeType, unit, featureCount, source, temporary,
/// sequenceHash, revCompHash}` — one workspace array entry of list_workspace.
pub(crate) fn workspace_item_json(item: &WorkspaceItem) -> serde_json::Value {
    let mut v = serde_json::json!({
        "id": item.id,
        "name": item.name,
        "length": item.sequence.len(),
        "moleculeType": item.molecule_type,
        "unit": unit_for(&item.molecule_type),
        "featureCount": item.features.len(),
        "source": item.source,
        "temporary": item.temporary,
    });
    insert_seq_hashes(&mut v, &orientation_hashes(&item.sequence, &item.molecule_type));
    v
}

/// The content a `hash` input resolved to, in the requested orientation
/// (a swapped hash pair arrives already reverse-complemented, annotations
/// flipped).
pub(crate) struct ResolvedHash {
    pub sequence: String,
    pub molecule_type: String,
    pub features: Vec<Feature>,
    pub primers: Vec<Primer>,
    /// true when the requested hash pair matched in swapped (fwd↔rev) order,
    /// i.e. the caller asked for the entry's reverse complement.
    pub flipped: bool,
    /// Entry name or project label, for notes/messages.
    pub label: String,
    /// Total number of matching entries (the first, newest, was used).
    pub match_count: usize,
}

/// Reverse-complement a sequence and flip its annotations (feature spans and
/// primer sites) via the save_file export engine on a throwaway project.
fn flipped_content(
    sequence: &str,
    molecule_type: &str,
    features: Vec<Feature>,
    primers: Vec<Primer>,
) -> (String, Vec<Feature>, Vec<Primer>) {
    let len = sequence.len() as i64;
    let project = ProjectData {
        sequence: sequence.to_string(),
        length: len,
        topology: "linear".to_string(),
        molecule_type: molecule_type.to_string(),
        features,
        primers,
        ..Default::default()
    };
    let (seq, feats, primers) = build_export_data(&project, &[(0, len - 1)], true);
    let seq = if molecule_type == "rna" { to_rna(&seq) } else { seq };
    (seq, feats, primers)
}

/// Resolve a workspace hash input — `"<sequenceHash>"` or
/// `"<sequenceHash>/<revCompHash>"` — against the workspace items (newest
/// first) and then the open projects (whose hashes are computed live).
/// A pair matching in swapped order resolves to the entry's reverse
/// complement (annotations flipped). Lock order: the workspace guard is
/// dropped before the pm guard is taken; the two are never held together.
pub(crate) async fn resolve_workspace_hash(
    pm: &Arc<RwLock<ProjectManager>>,
    workspace: &Workspace,
    hash_str: &str,
) -> Result<ResolvedHash, String> {
    let parts: Vec<&str> = hash_str.split('/').collect();
    let (g1, g2) = match parts.as_slice() {
        [a] if !a.is_empty() => ((*a).to_string(), None),
        [a, b] if !a.is_empty() && !b.is_empty() => {
            ((*a).to_string(), Some((*b).to_string()))
        }
        _ => {
            return Err(format!(
                "invalid hash '{}': expected \"<sequenceHash>\" or \"<sequenceHash>/<revCompHash>\" (see list_workspace)",
                hash_str
            ));
        }
    };
    // Some(false) = direct orientation, Some(true) = reverse complement.
    let matches = |fwd: &str, rev: &Option<String>| -> Option<bool> {
        match &g2 {
            Some(g2) => {
                if fwd == g1.as_str() && rev.as_deref() == Some(g2.as_str()) {
                    Some(false)
                } else if fwd == g2.as_str() && rev.as_deref() == Some(g1.as_str()) {
                    Some(true)
                } else {
                    None
                }
            }
            None => {
                if fwd == g1.as_str() {
                    Some(false)
                } else if rev.as_deref() == Some(g1.as_str()) {
                    Some(true)
                } else {
                    None
                }
            }
        }
    };

    let mut first: Option<ResolvedHash> = None;
    let mut match_count = 0usize;
    {
        let ws = workspace.read().await;
        for item in ws.iter().rev() {
            let (fwd, rev) = orientation_hashes(&item.sequence, &item.molecule_type);
            if let Some(flipped) = matches(&fwd, &rev) {
                match_count += 1;
                if first.is_none() {
                    let (sequence, features, primers) = if flipped {
                        flipped_content(
                            &item.sequence,
                            &item.molecule_type,
                            item.features.clone(),
                            item.primers.clone(),
                        )
                    } else {
                        (
                            item.sequence.clone(),
                            item.features.clone(),
                            item.primers.clone(),
                        )
                    };
                    first = Some(ResolvedHash {
                        sequence,
                        molecule_type: item.molecule_type.clone(),
                        features,
                        primers,
                        flipped,
                        label: item.name.clone(),
                        match_count: 0,
                    });
                }
            }
        }
    }
    {
        let pm = pm.read().await;
        let ids: Vec<String> = pm
            .list_projects()
            .iter()
            .filter_map(|e| e["id"].as_str().map(String::from))
            .collect();
        for pid in ids {
            let Some(p) = pm.get_project_by_id(&pid) else {
                continue;
            };
            if p.length <= 0 {
                continue;
            }
            let (fwd, rev) = orientation_hashes(&p.sequence, &p.molecule_type);
            if let Some(flipped) = matches(&fwd, &rev) {
                match_count += 1;
                if first.is_none() {
                    let (sequence, features, primers) = if flipped {
                        flipped_content(
                            &p.sequence,
                            &p.molecule_type,
                            p.features.clone(),
                            p.primers.clone(),
                        )
                    } else {
                        (
                            p.sequence.clone(),
                            p.features.clone(),
                            p.primers.clone(),
                        )
                    };
                    let label = if p.name.is_empty() { pid.clone() } else { p.name.clone() };
                    first = Some(ResolvedHash {
                        sequence,
                        molecule_type: p.molecule_type.clone(),
                        features,
                        primers,
                        flipped,
                        label,
                        match_count: 0,
                    });
                }
            }
        }
    }
    match first {
        Some(mut r) => {
            r.match_count = match_count;
            Ok(r)
        }
        None => Err(format!(
            "no workspace item or open project matches hash '{}' — call list_workspace to see the current entries and their hashes",
            hash_str
        )),
    }
}

/// A fragment selected by add_to_workspace: template pieces in export order,
/// the flip flag and a human-readable description for `name`/`source`.
struct FragmentSpec {
    pieces: Vec<(i64, i64)>,
    flip: bool,
    desc: String,
}

/// Enzyme mode: resolve 1 double-cutting or 2 single-cutting enzymes to their
/// fragment(s). Cut positions are the internal 0-based
/// `cut_pairs[0].top_cut_index` of each site (a cut at C severs between bases
/// C-1 and C), wrapped into 0..len on circular templates.
fn enzyme_fragments(project: &ProjectData, names: &[String]) -> Result<Vec<FragmentSpec>, serde_json::Value> {
    let fail = |msg: String| -> serde_json::Value { fail_envelope(&project.name, msg) };
    let len = project.length;
    let circular = project.topology == "circular";
    let db = libregene_core::enzyme::search::get_db();
    let project_names: Vec<&str> = project.enzymes.iter().map(|e| e.name.as_str()).collect();

    let mut per_enzyme: Vec<(String, Vec<i64>)> = Vec::new();
    let mut unknown: Vec<serde_json::Value> = Vec::new();
    for name in names {
        let sites: Vec<&libregene_core::models::Enzyme> = project
            .enzymes
            .iter()
            .filter(|e| e.name.eq_ignore_ascii_case(name))
            .collect();
        if sites.is_empty() {
            if db.enzymes.iter().any(|e| e.name.eq_ignore_ascii_case(name)) {
                return Err(fail(format!(
                    "enzyme '{}' has no recognition site on this sequence (siteCount 0)",
                    name
                )));
            }
            let q = name.to_lowercase();
            let similar: Vec<&str> = project_names
                .iter()
                .copied()
                .filter(|a| a.to_lowercase().contains(&q))
                .take(5)
                .collect();
            unknown.push(serde_json::json!({
                "name": name,
                "message": format!("Unknown enzyme '{}': not in the enzyme database", name),
                "similar": similar,
            }));
            continue;
        }
        let cuts: Vec<i64> = sites
            .iter()
            .map(|e| {
                let c = e
                    .cut_pairs
                    .first()
                    .map(|p| p.top_cut_index)
                    .unwrap_or(e.cut_index);
                if circular { c.rem_euclid(len) } else { c }
            })
            .collect();
        per_enzyme.push((sites[0].name.clone(), cuts));
    }
    if !unknown.is_empty() {
        let mut v = fail(format!(
            "{} unknown enzyme name(s) (see unknownEnzymes for near-match suggestions)",
            unknown.len()
        ));
        v["unknownEnzymes"] = serde_json::json!(unknown);
        return Err(v);
    }

    let label = if !project.name.is_empty() { project.name.clone() } else { "project".to_string() };
    match per_enzyme.as_slice() {
        [(name, cuts)] => {
            if cuts.len() != 2 {
                return Err(fail(format!(
                    "enzyme '{}' must cut exactly twice for single-enzyme extraction, but it has {} site(s) on this sequence",
                    name,
                    cuts.len()
                )));
            }
            let (c1, c2) = (cuts[0].min(cuts[1]), cuts[0].max(cuts[1]));
            if circular {
                let fwd = fragment_pieces(project, c1, c2).map_err(|e| fail(e))?;
                let rev = fragment_pieces(project, c2, c1).map_err(|e| fail(e))?;
                Ok(vec![
                    FragmentSpec { pieces: fwd, flip: false, desc: format!("{} fragment 1 of {}", name, label) },
                    FragmentSpec { pieces: rev, flip: false, desc: format!("{} fragment 2 of {}", name, label) },
                ])
            } else {
                if c1 == c2 {
                    return Err(fail(format!(
                        "both '{}' sites cut at the same position — the fragment between them is empty",
                        name
                    )));
                }
                Ok(vec![FragmentSpec {
                    pieces: vec![(c1, c2 - 1)],
                    flip: false,
                    desc: format!("{} double-cut fragment of {}", name, label),
                }])
            }
        }
        [(name1, cuts1), (name2, cuts2)] => {
            if cuts1.len() != 1 || cuts2.len() != 1 {
                return Err(fail(format!(
                    "two-enzyme extraction needs exactly 1 cut site per enzyme; '{}' has {}, '{}' has {}",
                    name1,
                    cuts1.len(),
                    name2,
                    cuts2.len()
                )));
            }
            let (c1, c2) = (cuts1[0], cuts2[0]);
            if circular {
                let pieces = fragment_pieces(project, c1, c2).map_err(|e| fail(e))?;
                Ok(vec![FragmentSpec {
                    pieces,
                    flip: false,
                    desc: format!("{}→{} fragment of {}", name1, name2, label),
                }])
            } else {
                if c1 == c2 {
                    return Err(fail(format!(
                        "'{}' and '{}' cut at the same position — the fragment between them is empty",
                        name1, name2
                    )));
                }
                // Respect the requested direction: enzyme1 → enzyme2; on a
                // linear template c1 > c2 is the reverse-complement arc.
                let (pieces, flip) = if c1 < c2 {
                    (vec![(c1, c2 - 1)], false)
                } else {
                    (vec![(c2, c1 - 1)], true)
                };
                Ok(vec![FragmentSpec {
                    pieces,
                    flip,
                    desc: format!("{}→{} fragment of {}", name1, name2, label),
                }])
            }
        }
        _ => Err(fail(
            "enzymes must hold 1 name (an enzyme cutting exactly twice) or 2 names (each cutting exactly once)"
                .to_string(),
        )),
    }
}

impl<R: Runtime> LibreGeneMcp<R> {
    pub(crate) async fn add_to_workspace_impl(
        &self,
        request: AddToWorkspaceRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let seq_hashes = orientation_hashes(&project.sequence, &project.molecule_type);
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            insert_seq_hashes(&mut v, &seq_hashes);
            Json(v)
        };

        let feature_active = request.feature_id.is_some();
        let region_active = request.start.is_some() || request.end.is_some();
        let enzymes_active = request.enzymes.is_some();
        if [feature_active, region_active, enzymes_active]
            .into_iter()
            .filter(|a| *a)
            .count()
            != 1
        {
            return Ok(fail(
                "exactly one selector required: featureId, start+end, or enzymes".to_string(),
            ));
        }

        let mut specs: Vec<FragmentSpec> = Vec::new();
        if enzymes_active {
            if !project.is_dna() {
                return Err(ErrorData::invalid_params(
                    format!(
                        "enzyme-based extraction is only supported on DNA projects; project '{}' is a {} project",
                        id, project.molecule_type
                    ),
                    None,
                ));
            }
            let names: Vec<String> = request
                .enzymes
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            match enzyme_fragments(&project, &names) {
                Ok(f) => specs = f,
                Err(mut v) => {
                    v["projectId"] = serde_json::json!(id);
                    insert_seq_hashes(&mut v, &seq_hashes);
                    return Ok(Json(v));
                }
            }
        } else {
            let spec = RegionSpec {
                start: request.start.map(from1),
                end: request.end.map(from1),
                feature_id: request.feature_id.clone(),
                cut1: None,
                cut2: None,
            };
            match resolve_export_region(&project, &spec) {
                Ok((pieces, flip, desc)) => specs.push(FragmentSpec { pieces, flip, desc }),
                Err(e) => return Ok(fail(e)),
            }
        }

        if request.name.is_some() && specs.len() != 1 {
            return Ok(fail(
                "name can only be set when exactly one fragment is produced".to_string(),
            ));
        }

        let mut added: Vec<serde_json::Value> = Vec::new();
        {
            let mut ws = self.workspace.write().await;
            for spec in &specs {
                let (sequence, features, primers) = build_export_data(&project, &spec.pieces, spec.flip);
                let name = request.name.clone().unwrap_or_else(|| spec.desc.clone());
                let item = WorkspaceItem {
                    id: next_id("ws"),
                    name,
                    sequence,
                    molecule_type: project.molecule_type.clone(),
                    features,
                    primers,
                    source: format!("{} ({})", id, spec.desc),
                    temporary: true,
                };
                added.push(workspace_item_json(&item));
                ws.push(item);
            }
        }
        let count = added.len();
        let workspace_count = self.workspace.read().await.len();
        let desc = if specs.len() == 1 {
            specs[0].desc.clone()
        } else {
            specs.iter().map(|s| s.desc.clone()).collect::<Vec<_>>().join("; ")
        };
        let mut v = ok_envelope(
            &id,
            format!("Added {} fragment(s) to the workspace ({})", count, desc),
            None,
        );
        v["unit"] = serde_json::json!(unit_for(&project.molecule_type));
        v["added"] = serde_json::json!(added);
        v["workspaceCount"] = serde_json::json!(workspace_count);
        insert_seq_hashes(&mut v, &seq_hashes);
        Ok(Json(v))
    }
}
