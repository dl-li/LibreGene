use std::collections::{HashMap, HashSet};

use crate::models::ProjectData;

/// Maximum number of projects that can be open simultaneously.
const MAX_PROJECTS: usize = 24;

/// In-memory project manager — supports multiple open projects.
#[derive(Default)]
pub struct ProjectManager {
    projects: HashMap<String, ProjectData>,
    /// Insertion order tracking for deterministic project listing (newest at end).
    ordered_ids: Vec<String>,
    active: Option<String>,
    /// Set of project IDs that have unsaved modifications.
    dirty_projects: HashSet<String>,
}

impl ProjectManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evict the oldest non-active, non-dirty project to stay under the limit.
    /// Returns false when at capacity with no evictable candidate — dirty
    /// projects are never evicted, so their unsaved changes can't be lost.
    fn evict_one(&mut self) -> bool {
        if self.projects.len() < MAX_PROJECTS {
            return true;
        }
        let idx = self.ordered_ids.iter().position(|k| {
            Some(k.as_str()) != self.active.as_deref() && !self.dirty_projects.contains(k)
        });
        if let Some(i) = idx {
            let id = self.ordered_ids.remove(i);
            self.projects.remove(&id);
            self.dirty_projects.remove(&id);
        }
        self.projects.len() < MAX_PROJECTS
    }

    pub fn load(&mut self, id: &str, project: ProjectData) -> Result<(), String> {
        // Only evict when adding a genuinely new entry
        if !self.projects.contains_key(id) {
            if !self.evict_one() {
                return Err(format!(
                    "project limit ({MAX_PROJECTS}) reached and every other open project has unsaved changes; save or close one before loading {id}"
                ));
            }
            self.ordered_ids.push(id.to_string());
        }
        self.projects.insert(id.to_string(), project);
        self.active = Some(id.to_string());
        Ok(())
    }

    pub fn open_project(&mut self, id: String, project: ProjectData) -> Result<(), String> {
        // Only evict when adding a genuinely new entry
        if !self.projects.contains_key(&id) {
            if !self.evict_one() {
                return Err(format!(
                    "project limit ({MAX_PROJECTS}) reached and every other open project has unsaved changes; save or close one before loading {id}"
                ));
            }
            self.ordered_ids.push(id.clone());
        }
        self.projects.insert(id.clone(), project);
        self.active = Some(id);
        Ok(())
    }

    pub fn get_project(&self) -> Option<&ProjectData> {
        self.active
            .as_ref()
            .and_then(|id| self.projects.get(id))
    }

    pub fn get_project_by_id(&self, id: &str) -> Option<&ProjectData> {
        self.projects.get(id)
    }

    pub fn get_project_mut(&mut self) -> Option<&mut ProjectData> {
        self.active
            .as_ref()
            .and_then(|id| self.projects.get_mut(id))
    }

    /// Get a mutable reference to any project by its ID (for multi-window support).
    pub fn get_project_mut_by_id(&mut self, id: &str) -> Option<&mut ProjectData> {
        self.projects.get_mut(id)
    }

    pub fn active_id(&self) -> Option<&str> {
        self.active.as_deref()
    }

    pub fn list_projects(&self) -> Vec<serde_json::Value> {
        self.ordered_ids
            .iter()
            .filter_map(|id| self.projects.get(id).map(|p| {
                serde_json::json!({
                    "id": id,
                    "name": id,
                    "length": p.length,
                    "topology": p.topology,
                    "moleculeType": p.molecule_type,
                    "dirty": self.dirty_projects.contains(id.as_str()),
                })
            }))
            .collect()
    }

    pub fn activate_project(&mut self, id: &str) -> bool {
        if self.projects.contains_key(id) {
            self.active = Some(id.to_string());
            true
        } else {
            false
        }
    }

    /// Mark a project as having unsaved changes.
    pub fn mark_dirty(&mut self, id: &str) {
        self.dirty_projects.insert(id.to_string());
    }

    /// Mark a project as clean (saved to disk).
    pub fn mark_clean(&mut self, id: &str) {
        self.dirty_projects.remove(id);
    }

    /// Check whether a project has unsaved changes.
    pub fn is_dirty(&self, id: &str) -> bool {
        self.dirty_projects.contains(id)
    }

    /// Rename a project's ID (e.g. after Save As to a new file path).
    /// Returns false if old_id doesn't exist or new_id already exists.
    pub fn rename_id(&mut self, old_id: &str, new_id: &str) -> bool {
        if !self.projects.contains_key(old_id) || self.projects.contains_key(new_id) {
            return false;
        }
        let project = self.projects.remove(old_id).unwrap();
        self.projects.insert(new_id.to_string(), project);
        if let Some(pos) = self.ordered_ids.iter().position(|i| i == old_id) {
            self.ordered_ids[pos] = new_id.to_string();
        }
        if self.active.as_deref() == Some(old_id) {
            self.active = Some(new_id.to_string());
        }
        if self.dirty_projects.remove(old_id) {
            self.dirty_projects.insert(new_id.to_string());
        }
        true
    }

    /// Return the ordered list of all project IDs.
    pub fn all_project_ids(&self) -> Vec<String> {
        self.ordered_ids.clone()
    }

    pub fn close_project(&mut self, id: &str) -> bool {
        let existed = self.projects.remove(id).is_some();
        self.ordered_ids.retain(|i| i != id);
        self.dirty_projects.remove(id);
        if self.active.as_deref() == Some(id) {
            self.active = self.ordered_ids.first().cloned();
        }
        existed
    }

    /// Align `seq` against project `id`'s sequence and store the result.
    /// Returns the stored alignment, or None if there is no significant match.
    pub fn add_alignment(&mut self, id: &str, name: &str, seq: &str) -> Option<crate::models::Alignment> {
        let p = self.projects.get(id)?;
        let circular = p.topology == "circular";
        let mut aln = crate::align::align_read(&p.sequence, seq, circular)?;
        aln.name = name.to_string();
        aln.id = crate::align::next_alignment_id(&p.alignments);
        let p = self.projects.get_mut(id)?;
        p.alignments.push(aln.clone());
        self.dirty_projects.insert(id.to_string());
        Some(aln)
    }

    /// Remove an alignment by its id. Returns true if one was removed.
    pub fn remove_alignment(&mut self, id: &str, alignment_id: &str) -> bool {
        if let Some(p) = self.projects.get_mut(id) {
            let before = p.alignments.len();
            p.alignments.retain(|a| a.id != alignment_id);
            if p.alignments.len() != before {
                self.dirty_projects.insert(id.to_string());
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proj(name: &str) -> ProjectData {
        ProjectData {
            name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn eviction_skips_dirty_projects() {
        let mut pm = ProjectManager::new();
        for i in 0..MAX_PROJECTS {
            let id = format!("p{i}");
            pm.load(&id, proj(&id)).unwrap();
        }
        // Active is p23; mark p0 dirty → p1 becomes the oldest evictable.
        pm.mark_dirty("p0");
        pm.load("p24", proj("p24")).unwrap();
        assert!(pm.get_project_by_id("p0").is_some(), "dirty project must survive eviction");
        assert!(pm.get_project_by_id("p1").is_none(), "oldest clean project evicted");
    }

    #[test]
    fn load_errors_when_no_evictable_project() {
        let mut pm = ProjectManager::new();
        for i in 0..MAX_PROJECTS {
            let id = format!("p{i}");
            pm.load(&id, proj(&id)).unwrap();
        }
        // Every project except the active one (p23) is dirty.
        for i in 0..MAX_PROJECTS - 1 {
            pm.mark_dirty(&format!("p{i}"));
        }
        let err = pm.load("p25", proj("p25")).unwrap_err();
        assert!(err.contains("unsaved changes"), "{err}");
        assert!(pm.get_project_by_id("p25").is_none());
    }
}
