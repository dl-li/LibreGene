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

    /// Evict the oldest non-active project to stay under the limit.
    /// No-op if already within capacity, or the only project is the active one.
    fn evict_one(&mut self) {
        if self.projects.len() < MAX_PROJECTS || self.projects.is_empty() {
            return;
        }
        // Evict the oldest non-active project (first in insertion order that isn't active)
        let idx = self.ordered_ids.iter().position(|k| {
            Some(k.as_str()) != self.active.as_deref()
        });
        if let Some(i) = idx {
            let id = self.ordered_ids.remove(i);
            self.projects.remove(&id);
            self.dirty_projects.remove(&id);
        }
    }

    pub fn load(&mut self, id: &str, project: ProjectData) {
        // Only evict when adding a genuinely new entry
        if !self.projects.contains_key(id) {
            self.evict_one();
            self.ordered_ids.push(id.to_string());
        }
        self.projects.insert(id.to_string(), project.clone());
        self.active = Some(id.to_string());
    }

    pub fn open_project(&mut self, id: String, project: ProjectData) {
        // Only evict when adding a genuinely new entry
        if !self.projects.contains_key(&id) {
            self.evict_one();
            self.ordered_ids.push(id.clone());
        }
        self.projects.insert(id.clone(), project);
        self.active = Some(id);
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

    pub fn set_roi(&mut self, start: i64, end: i64) {
        if let Some(p) = self.get_project_mut() {
            p.roi = Some((start, end));
        }
    }

    pub fn clear_roi(&mut self) {
        if let Some(p) = self.get_project_mut() {
            p.roi = None;
        }
    }

    pub fn update_sequence(&mut self, seq: String) {
        if let Some(p) = self.get_project_mut() {
            p.sequence = seq;
            p.length = p.sequence.len() as i64;
            if let Some(ref active) = self.active {
                self.dirty_projects.insert(active.clone());
            }
        }
    }

    pub fn update_features(&mut self, features: Vec<crate::models::Feature>) {
        if let Some(p) = self.get_project_mut() {
            p.features = features;
            if let Some(ref active) = self.active {
                self.dirty_projects.insert(active.clone());
            }
        }
    }

    /// Update the `ftype` field of a single feature identified by `feature_id`.
    pub fn update_feature_ftype(&mut self, feature_id: &str, new_ftype: &str) {
        if let Some(p) = self.get_project_mut() {
            if let Some(f) = p.features.iter_mut().find(|f| f.id == feature_id) {
                f.ftype = new_ftype.to_string();
            }
            if let Some(ref active) = self.active {
                self.dirty_projects.insert(active.clone());
            }
        }
    }

    /// Update the `color` field of a single feature identified by `feature_id`.
    pub fn update_feature_color(&mut self, feature_id: &str, new_color: &str) {
        if let Some(p) = self.get_project_mut() {
            if let Some(f) = p.features.iter_mut().find(|f| f.id == feature_id) {
                f.color = new_color.to_string();
            }
            if let Some(ref active) = self.active {
                self.dirty_projects.insert(active.clone());
            }
        }
    }

    /// Update a feature's location by parsing a GenBank location string.
    /// Returns None if the location string is invalid.
    pub fn update_feature_location(
        &mut self,
        feature_id: &str,
        location_str: &str,
    ) -> Result<(), String> {
        let parsed = crate::file_io::gbk::parse_location_string(location_str)
            .ok_or_else(|| format!("Invalid location: {}", location_str))?;
        let (segments, start, end, strand) = parsed;
        if let Some(p) = self.get_project_mut() {
            if let Some(f) = p.features.iter_mut().find(|f| f.id == feature_id) {
                f.segments = segments;
                f.start = start;
                f.end = end;
                f.strand = strand;
            }
            if let Some(ref active) = self.active {
                self.dirty_projects.insert(active.clone());
            }
        }
        Ok(())
    }

    pub fn update_primers(&mut self, primers: Vec<crate::models::Primer>) {
        if let Some(p) = self.get_project_mut() {
            p.primers = primers;
            if let Some(ref active) = self.active {
                self.dirty_projects.insert(active.clone());
            }
        }
    }

    pub fn set_methylation_systems(&mut self, systems: Vec<String>) {
        if let Some(p) = self.get_project_mut() {
            p.methylation_systems = systems;
        }
    }
}
