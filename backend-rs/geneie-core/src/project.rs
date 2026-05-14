use std::collections::HashMap;

use crate::models::ProjectData;

/// Maximum number of projects that can be open simultaneously.
const MAX_PROJECTS: usize = 24;

/// In-memory project manager — supports multiple open projects.
#[derive(Default)]
pub struct ProjectManager {
    projects: HashMap<String, ProjectData>,
    active: Option<String>,
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
        // Evict the first non-active project
        let keys: Vec<String> = self.projects.keys().cloned().collect();
        for k in &keys {
            if Some(k.as_str()) != self.active.as_deref() {
                self.projects.remove(k);
                return;
            }
        }
    }

    pub fn load(&mut self, id: &str, project: ProjectData) {
        // Only evict when adding a genuinely new entry
        if !self.projects.contains_key(id) {
            self.evict_one();
        }
        self.projects.insert(id.to_string(), project.clone());
        self.active = Some(id.to_string());
    }

    pub fn open_project(&mut self, id: String, project: ProjectData) {
        // Only evict when adding a genuinely new entry
        if !self.projects.contains_key(&id) {
            self.evict_one();
        }
        self.projects.insert(id.clone(), project);
        self.active = Some(id);
    }

    pub fn get_project(&self) -> Option<&ProjectData> {
        self.active
            .as_ref()
            .and_then(|id| self.projects.get(id))
    }

    pub fn get_project_mut(&mut self) -> Option<&mut ProjectData> {
        self.active
            .as_ref()
            .and_then(|id| self.projects.get_mut(id))
    }

    pub fn active_id(&self) -> Option<&str> {
        self.active.as_deref()
    }

    pub fn list_projects(&self) -> Vec<serde_json::Value> {
        self.projects
            .iter()
            .map(|(id, p)| {
                serde_json::json!({
                    "id": id,
                    "name": id,
                    "length": p.length,
                    "topology": p.topology,
                })
            })
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

    pub fn close_project(&mut self, id: &str) -> bool {
        let existed = self.projects.remove(id).is_some();
        if self.active.as_deref() == Some(id) {
            self.active = self.projects.keys().next().cloned();
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
        }
    }

    pub fn update_features(&mut self, features: Vec<crate::models::Feature>) {
        if let Some(p) = self.get_project_mut() {
            p.features = features;
        }
    }

    pub fn update_primers(&mut self, primers: Vec<crate::models::Primer>) {
        if let Some(p) = self.get_project_mut() {
            p.primers = primers;
        }
    }

    pub fn set_methylation_systems(&mut self, systems: Vec<String>) {
        if let Some(p) = self.get_project_mut() {
            p.methylation_systems = systems;
        }
    }
}
