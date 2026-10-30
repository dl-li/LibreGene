use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::RwLock;

use libregene_core::models::{Feature, Primer, ProjectData};
use libregene_core::project::ProjectManager;

pub(crate) const SESSION_ROLLBACK_SOURCE: &str = "session-edit-log";
pub(crate) type SessionEditLog = Arc<RwLock<SessionEditLogStore>>;

#[derive(Clone, Copy, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum EditOrigin {
    User,
    Agent,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionEditEntry {
    pub id: u64,
    pub timestamp: u64,
    pub origin: EditOrigin,
    pub saved: bool,
    pub summary: String,
    pub details: Vec<String>,
}

struct LoggedEdit {
    entry: SessionEditEntry,
    before: ProjectData,
}

struct ProjectEditLog {
    current: ProjectData,
    baseline_dirty: bool,
    entries: Vec<LoggedEdit>,
}

#[derive(Default)]
pub(crate) struct SessionEditLogStore {
    projects: HashMap<String, ProjectEditLog>,
    next_id: u64,
}

fn feature_content(feature: &Feature) -> serde_json::Value {
    let mut value = serde_json::to_value(feature).unwrap_or_default();
    if let Some(object) = value.as_object_mut() {
        object.remove("translation");
    }
    value
}

fn primer_content(primer: &Primer) -> serde_json::Value {
    let mut value = serde_json::to_value(primer).unwrap_or_default();
    if let Some(object) = value.as_object_mut() {
        object.remove("bindingSites");
    }
    value
}

fn project_content(project: &ProjectData) -> serde_json::Value {
    serde_json::json!({
        "name": project.name,
        "definition": project.definition,
        "keywords": project.keywords,
        "labHost": project.lab_host,
        "sequence": project.sequence,
        "length": project.length,
        "topology": project.topology,
        "moleculeType": project.molecule_type,
        "features": project.features.iter().map(feature_content).collect::<Vec<_>>(),
        "primers": project.primers.iter().map(primer_content).collect::<Vec<_>>(),
        "alignments": project.alignments,
        "roi": project.roi,
        "tracePath": project.trace_path,
        "textColorsTop": project.text_colors_top,
        "textColorsBottom": project.text_colors_bottom,
    })
}

fn sequence_change(before: &str, after: &str, unit: &str) -> String {
    let old = before.as_bytes();
    let new = after.as_bytes();
    let limit = old.len().min(new.len());
    let mut start = 0;
    while start < limit && old[start] == new[start] {
        start += 1;
    }
    let mut old_end = old.len();
    let mut new_end = new.len();
    while old_end > start && new_end > start && old[old_end - 1] == new[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }
    let removed = &before[start..old_end];
    let inserted = &after[start..new_end];
    let snippet = |sequence: &str| {
        const MAX_BASES: usize = 40;
        if sequence.len() > MAX_BASES {
            format!("{}…", &sequence[..MAX_BASES])
        } else {
            sequence.to_string()
        }
    };
    let position = start + 1;
    if removed.is_empty() {
        format!(
            "Inserted {} {} before position {} ({})",
            inserted.len(),
            unit,
            position,
            snippet(inserted)
        )
    } else if inserted.is_empty() {
        let end = old_end.max(start + 1);
        format!(
            "Deleted {} {} at {}–{} ({})",
            removed.len(),
            unit,
            position,
            end,
            snippet(removed)
        )
    } else {
        let end = old_end.max(start + 1);
        format!(
            "Replaced {} {} at {}–{} with {} {} ({} → {})",
            removed.len(),
            unit,
            position,
            end,
            inserted.len(),
            unit,
            snippet(removed),
            snippet(inserted)
        )
    }
}

fn feature_location(feature: &Feature) -> String {
    let location = if feature.segments.is_empty() {
        format!("{}–{}", feature.start + 1, feature.end + 1)
    } else {
        feature
            .segments
            .iter()
            .map(|segment| format!("{}–{}", segment.start + 1, segment.end + 1))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if feature.strand == "-" {
        format!("complement({location})")
    } else {
        location
    }
}

fn feature_changes(before: &[Feature], after: &[Feature]) -> Vec<String> {
    let before_by_id: HashMap<&str, &Feature> = before.iter().map(|f| (f.id.as_str(), f)).collect();
    let after_by_id: HashMap<&str, &Feature> = after.iter().map(|f| (f.id.as_str(), f)).collect();
    let added: Vec<&&Feature> = after_by_id
        .iter()
        .filter(|(id, _)| !before_by_id.contains_key(**id))
        .map(|(_, feature)| feature)
        .collect();
    let removed: Vec<&&Feature> = before_by_id
        .iter()
        .filter(|(id, _)| !after_by_id.contains_key(**id))
        .map(|(_, feature)| feature)
        .collect();
    let mut changed: Vec<&&Feature> = after_by_id
        .iter()
        .filter(|(id, feature)| {
            before_by_id
                .get(**id)
                .is_some_and(|old| feature_content(old) != feature_content(feature))
        })
        .map(|(_, feature)| feature)
        .collect();
    changed.sort_by_key(|feature| feature.name.as_str());

    let mut changes = Vec::new();
    if added.len() == 1 {
        let feature = added[0];
        changes.push(format!(
            "Added feature ‘{}’ ({}, {})",
            feature.name,
            feature.ftype,
            feature_location(feature)
        ));
    } else if !added.is_empty() {
        changes.push(format!("Added {} features", added.len()));
    }
    if removed.len() == 1 {
        changes.push(format!("Deleted feature ‘{}’", removed[0].name));
    } else if !removed.is_empty() {
        changes.push(format!("Deleted {} features", removed.len()));
    }
    if changed.len() == 1 {
        changes.push(format!("Updated feature ‘{}’", changed[0].name));
    } else if !changed.is_empty() {
        changes.push(format!("Updated {} features", changed.len()));
    }
    changes
}

fn primer_changes(before: &[Primer], after: &[Primer]) -> Vec<String> {
    let before_by_id: HashMap<&str, &Primer> = before.iter().map(|p| (p.id.as_str(), p)).collect();
    let after_by_id: HashMap<&str, &Primer> = after.iter().map(|p| (p.id.as_str(), p)).collect();
    let added: Vec<&&Primer> = after_by_id
        .iter()
        .filter(|(id, _)| !before_by_id.contains_key(**id))
        .map(|(_, primer)| primer)
        .collect();
    let removed: Vec<&&Primer> = before_by_id
        .iter()
        .filter(|(id, _)| !after_by_id.contains_key(**id))
        .map(|(_, primer)| primer)
        .collect();
    let changed = after_by_id
        .iter()
        .filter(|(id, primer)| {
            before_by_id
                .get(**id)
                .is_some_and(|old| primer_content(old) != primer_content(primer))
        })
        .count();

    let mut changes = Vec::new();
    if added.len() == 1 {
        changes.push(format!("Added primer ‘{}’", added[0].name));
    } else if !added.is_empty() {
        changes.push(format!("Added {} primers", added.len()));
    }
    if removed.len() == 1 {
        changes.push(format!("Deleted primer ‘{}’", removed[0].name));
    } else if !removed.is_empty() {
        changes.push(format!("Deleted {} primers", removed.len()));
    }
    if changed == 1 {
        changes.push("Updated 1 primer".to_string());
    } else if changed > 1 {
        changes.push(format!("Updated {changed} primers"));
    }
    changes
}

fn alignment_changes(before: &ProjectData, after: &ProjectData) -> Vec<String> {
    let before_ids: HashSet<&str> = before.alignments.iter().map(|a| a.id.as_str()).collect();
    let after_ids: HashSet<&str> = after.alignments.iter().map(|a| a.id.as_str()).collect();
    let added: Vec<_> = after
        .alignments
        .iter()
        .filter(|alignment| !before_ids.contains(alignment.id.as_str()))
        .collect();
    let removed: Vec<_> = before
        .alignments
        .iter()
        .filter(|alignment| !after_ids.contains(alignment.id.as_str()))
        .collect();
    let mut changes = Vec::new();
    if added.len() == 1 {
        changes.push(format!("Added alignment ‘{}’", added[0].name));
    } else if !added.is_empty() {
        changes.push(format!("Added {} alignments", added.len()));
    }
    if removed.len() == 1 {
        changes.push(format!("Removed alignment ‘{}’", removed[0].name));
    } else if !removed.is_empty() {
        changes.push(format!("Removed {} alignments", removed.len()));
    }
    changes
}

fn describe_change(before: &ProjectData, after: &ProjectData) -> Vec<String> {
    let mut details = Vec::new();
    if before.sequence != after.sequence {
        let unit = match after.molecule_type.as_str() {
            "rna" => "nt",
            "protein" => "aa",
            _ => "bp",
        };
        details.push(sequence_change(&before.sequence, &after.sequence, unit));
    }
    details.extend(feature_changes(&before.features, &after.features));
    details.extend(primer_changes(&before.primers, &after.primers));
    details.extend(alignment_changes(before, after));
    if before.topology != after.topology {
        details.push(format!("Changed topology to {}", after.topology));
    }
    if before.text_colors_top != after.text_colors_top
        || before.text_colors_bottom != after.text_colors_bottom
    {
        details.push("Changed sequence text colors".to_string());
    }
    if before.roi != after.roi {
        details.push(match after.roi {
            Some((start, end)) => format!("Set region of interest to {}–{}", start + 1, end + 1),
            None => "Cleared region of interest".to_string(),
        });
    }
    if before.name != after.name
        || before.definition != after.definition
        || before.keywords != after.keywords
        || before.lab_host != after.lab_host
    {
        details.push("Updated project metadata".to_string());
    }
    if details.is_empty() {
        details.push("Edited project".to_string());
    }
    details
}

impl SessionEditLogStore {
    pub fn register(&mut self, project_id: &str, project: &ProjectData, dirty: bool) {
        self.projects
            .entry(project_id.to_string())
            .or_insert_with(|| ProjectEditLog {
                current: project.clone(),
                baseline_dirty: dirty,
                entries: Vec::new(),
            });
    }

    pub fn retain_projects(&mut self, project_ids: &HashSet<String>) {
        self.projects.retain(|id, _| project_ids.contains(id));
    }

    pub fn record(
        &mut self,
        project_id: &str,
        after: &ProjectData,
        origin: EditOrigin,
    ) -> Option<SessionEditEntry> {
        let log = self.projects.get_mut(project_id)?;
        if project_content(&log.current) == project_content(after) {
            log.current = after.clone();
            return None;
        }
        let details = describe_change(&log.current, after);
        self.next_id += 1;
        let entry = SessionEditEntry {
            id: self.next_id,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            origin,
            saved: false,
            summary: details[0].clone(),
            details,
        };
        log.entries.push(LoggedEdit {
            entry: entry.clone(),
            before: std::mem::replace(&mut log.current, after.clone()),
        });
        Some(entry)
    }

    pub fn entries(&self, project_id: &str) -> Vec<SessionEditEntry> {
        self.projects
            .get(project_id)
            .map(|log| {
                log.entries
                    .iter()
                    .map(|entry| entry.entry.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn mark_saved(&mut self, project_id: &str, current: &ProjectData) {
        if let Some(log) = self.projects.get_mut(project_id) {
            for entry in &mut log.entries {
                entry.entry.saved = true;
            }
            log.current = current.clone();
            log.baseline_dirty = false;
        }
    }

    pub fn rekey(&mut self, old_id: &str, new_id: &str) {
        if old_id == new_id {
            return;
        }
        if let Some(log) = self.projects.remove(old_id) {
            self.projects.insert(new_id.to_string(), log);
        }
    }

    pub fn rollback(
        &mut self,
        project_id: &str,
        entry_id: u64,
    ) -> Option<(ProjectData, bool, bool)> {
        let log = self.projects.get_mut(project_id)?;
        let index = log
            .entries
            .iter()
            .position(|entry| entry.entry.id == entry_id && !entry.entry.saved)?;
        let before = log.entries[index].before.clone();
        log.entries.truncate(index);
        log.current = before.clone();
        let has_unsaved = log.entries.iter().any(|entry| !entry.entry.saved);
        Some((before, log.baseline_dirty, has_unsaved))
    }
}

fn emit_entries<R: Runtime>(
    app_handle: &AppHandle<R>,
    project_id: &str,
    entries: Vec<SessionEditEntry>,
) {
    let _ = app_handle.emit(
        "session-edit-log-update",
        serde_json::json!({ "projectId": project_id, "entries": entries }),
    );
}

pub(crate) async fn register_project<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
) {
    let Some(state) = app_handle.try_state::<crate::AppState>() else {
        return;
    };
    let (project, dirty) = {
        let pm = pm.read().await;
        let Some(project) = pm.get_project_by_id(project_id).cloned() else {
            return;
        };
        (project, pm.is_dirty(project_id))
    };
    let mut log = state.session_edit_log.write().await;
    log.register(project_id, &project, dirty);
    emit_entries(app_handle, project_id, log.entries(project_id));
}

pub(crate) async fn mark_project_saved<R: Runtime>(
    app_handle: &AppHandle<R>,
    pm: &Arc<RwLock<ProjectManager>>,
    project_id: &str,
) {
    let Some(state) = app_handle.try_state::<crate::AppState>() else {
        return;
    };
    let pm = pm.read().await;
    if pm.is_dirty(project_id) {
        return;
    }
    let Some(project) = pm.get_project_by_id(project_id).cloned() else {
        return;
    };
    let mut log = state.session_edit_log.write().await;
    log.mark_saved(project_id, &project);
    emit_entries(app_handle, project_id, log.entries(project_id));
}

pub(crate) async fn rekey_project<R: Runtime>(
    app_handle: &AppHandle<R>,
    old_id: &str,
    new_id: &str,
) {
    let Some(state) = app_handle.try_state::<crate::AppState>() else {
        return;
    };
    let entries = {
        let mut log = state.session_edit_log.write().await;
        log.rekey(old_id, new_id);
        log.entries(new_id)
    };
    emit_entries(app_handle, old_id, Vec::new());
    emit_entries(app_handle, new_id, entries);
}

pub(crate) async fn record_project_change<R: Runtime>(
    app_handle: &AppHandle<R>,
    project_id: &str,
    after: &ProjectData,
    source: Option<&str>,
) {
    if source == Some(SESSION_ROLLBACK_SOURCE) {
        return;
    }
    let Some(state) = app_handle.try_state::<crate::AppState>() else {
        return;
    };
    let origin = if source.is_some() {
        EditOrigin::User
    } else {
        EditOrigin::Agent
    };
    let mut log = state.session_edit_log.write().await;
    if log.record(project_id, after, origin).is_some() {
        emit_entries(app_handle, project_id, log.entries(project_id));
    }
}

pub(crate) async fn sync_projects<R: Runtime>(
    app_handle: &AppHandle<R>,
    projects: &HashMap<String, (ProjectData, bool)>,
) {
    let Some(state) = app_handle.try_state::<crate::AppState>() else {
        return;
    };
    let mut log = state.session_edit_log.write().await;
    let ids: HashSet<String> = projects.keys().cloned().collect();
    log.retain_projects(&ids);
    for (id, (project, dirty)) in projects {
        log.register(id, project, *dirty);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libregene_core::models::{Alignment, Segment};

    fn project(sequence: &str) -> ProjectData {
        ProjectData {
            sequence: sequence.to_string(),
            length: sequence.len() as i64,
            ..Default::default()
        }
    }

    #[test]
    fn records_readable_sequence_changes_and_origins() {
        let mut store = SessionEditLogStore::default();
        let initial = project("AACCGGTT");
        store.register("p", &initial, false);
        let edited = project("AACCAGGTT");
        let entry = store.record("p", &edited, EditOrigin::Agent).unwrap();
        assert_eq!(entry.origin, EditOrigin::Agent);
        assert_eq!(entry.summary, "Inserted 1 bp before position 5 (A)");
        let undone = store.record("p", &initial, EditOrigin::User).unwrap();
        assert_eq!(undone.origin, EditOrigin::User);
        assert_eq!(undone.summary, "Deleted 1 bp at 5–5 (A)");
    }

    #[test]
    fn describes_annotation_primer_alignment_and_topology_changes() {
        let mut store = SessionEditLogStore::default();
        let initial = project("AACCGGTT");
        store.register("p", &initial, false);
        let mut edited = initial.clone();
        edited.features.push(Feature {
            id: "f1".to_string(),
            name: "GFP".to_string(),
            ftype: "CDS".to_string(),
            start: 1,
            end: 5,
            color: "#60A5FA".to_string(),
            segments: vec![Segment {
                start: 1,
                end: 5,
                color: None,
            }],
            strand: "+".to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        });
        edited.primers.push(Primer {
            id: "p1".to_string(),
            name: "forward".to_string(),
            r#type: "fwd".to_string(),
            primer_seq: "AACCGGTT".to_string(),
            binding_sites: Vec::new(),
        });
        edited.alignments.push(Alignment {
            id: "a1".to_string(),
            name: "read 1".to_string(),
            ..Default::default()
        });
        edited.topology = "linear".to_string();
        let entry = store.record("p", &edited, EditOrigin::User).unwrap();
        assert!(
            entry
                .details
                .contains(&"Added feature ‘GFP’ (CDS, 2–6)".to_string())
        );
        assert!(
            entry
                .details
                .contains(&"Added primer ‘forward’".to_string())
        );
        assert!(
            entry
                .details
                .contains(&"Added alignment ‘read 1’".to_string())
        );
        assert!(
            entry
                .details
                .contains(&"Changed topology to linear".to_string())
        );
    }

    #[test]
    fn rollback_restores_whole_project_and_truncates_later_entries() {
        let mut store = SessionEditLogStore::default();
        let initial = project("AACCGGTT");
        store.register("p", &initial, false);
        let mut first = initial.clone();
        first.topology = "linear".to_string();
        let first_entry = store.record("p", &first, EditOrigin::User).unwrap();
        let mut second = first.clone();
        second.sequence = "AACCAGTT".to_string();
        second.length = 8;
        second.alignments.push(Alignment {
            id: "a1".to_string(),
            ..Default::default()
        });
        store.record("p", &second, EditOrigin::Agent).unwrap();

        let (restored, baseline_dirty, has_remaining) =
            store.rollback("p", first_entry.id).unwrap();
        assert_eq!(restored.sequence, initial.sequence);
        assert_eq!(restored.topology, initial.topology);
        assert!(restored.alignments.is_empty());
        assert!(!baseline_dirty);
        assert!(!has_remaining);
        assert!(store.entries("p").is_empty());
        assert!(store.rollback("p", first_entry.id).is_none());
    }

    #[test]
    fn save_keeps_entries_as_a_rollback_boundary_and_survives_rekey() {
        let mut store = SessionEditLogStore::default();
        let initial = project("AAAA");
        store.register("p", &initial, false);
        let saved = project("AAAAT");
        let saved_entry = store.record("p", &saved, EditOrigin::User).unwrap();
        store.mark_saved("p", &saved);
        assert_eq!(store.entries("p").len(), 1);
        assert!(store.entries("p")[0].saved);
        assert!(store.rollback("p", saved_entry.id).is_none());

        let edited = project("AAAATG");
        let edited_entry = store.record("p", &edited, EditOrigin::Agent).unwrap();
        let (restored, baseline_dirty, has_unsaved) = store.rollback("p", edited_entry.id).unwrap();
        assert_eq!(restored.sequence, saved.sequence);
        assert!(!baseline_dirty);
        assert!(!has_unsaved);
        assert_eq!(store.entries("p").len(), 1);
        assert!(store.entries("p")[0].saved);

        store.rekey("p", "/tmp/saved.gbk");
        assert!(store.entries("p").is_empty());
        assert_eq!(store.entries("/tmp/saved.gbk").len(), 1);
        assert!(store.rollback("/tmp/saved.gbk", saved_entry.id).is_none());
    }

    #[test]
    fn derived_recomputation_and_user_settings_do_not_create_an_entry() {
        let mut store = SessionEditLogStore::default();
        let initial = project("AAAA");
        store.register("p", &initial, false);
        let mut recomputed = initial.clone();
        recomputed.enzymes.push(Default::default());
        recomputed.methylation_systems = vec!["dam".to_string()];
        recomputed.methylation_overlap = 4;
        assert!(store.record("p", &recomputed, EditOrigin::Agent).is_none());
        assert!(store.entries("p").is_empty());
    }
}
