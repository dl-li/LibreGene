use std::collections::HashMap;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::RwLock;

use libregene_core::project::ProjectManager;

// ---------------------------------------------------------------------------
// Path validation for filesystem-touching commands
// ---------------------------------------------------------------------------

/// Reject path-traversal and require a sequence-file extension.
/// Returns Ok(normalized lowercase extension without dot) if acceptable.
///
/// We don't lock paths to a fixed directory (users open/save anywhere), but we
/// do refuse parent-directory traversal components and require a known
/// sequence/export extension so a raw path string from an untrusted caller
/// (e.g. the MCP bridge) can't read/write arbitrary files.
pub(crate) fn validate_user_path(path: &str, allowed_exts: &[&str]) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("path is empty".to_string());
    }
    // Reject any `..` component (works for both separators and mixed forms
    // after we normalize). canonicalize() would also resolve symlinks, but
    // the file may not exist yet (save target), so we inspect components.
    let p = std::path::Path::new(trimmed);
    for comp in p.components() {
        use std::path::Component;
        match comp {
            Component::ParentDir => {
                return Err("parent-directory traversal (..) is not allowed".to_string())
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) | Component::CurDir => {}
        }
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !allowed_exts.iter().any(|a| *a == ext) {
        return Err(format!(
            "file extension '.{}' is not allowed (allowed: {})",
            ext,
            allowed_exts.join(", ")
        ));
    }
    Ok(ext)
}

pub(crate) const SEQ_EXTS: &[&str] = &[
    "gbk", "gb", "genbank", "gbf", "gbff",
    "dna", "rna", "prot",
    "gpt", "gp", "gpe", "gpff",
    "fasta", "fa", "fna", "fas", "ffn", "fsa", "faa", "frn",
    "ab1", "seq",
];
pub(crate) const TEXT_EXPORT_EXTS: &[&str] = &["txt", "csv", "json"];
/// Output extensions accepted by save paths that write a whole project
/// (.gbk/.gb/.genbank → DNA/RNA GenBank, .gpt → protein GenBank).
pub(crate) const CODON_OUTPUT_EXTS: &[&str] = &["gbk", "gb", "genbank", "gpt"];
/// Output extensions accepted by MCP `convert_sequence`'s `output_path`:
/// the GenBank set above plus bare-sequence text (.fa/.fasta/.txt).
pub(crate) const CONVERT_OUTPUT_EXTS: &[&str] = &["gbk", "gb", "genbank", "gpt", "fa", "fasta", "txt"];

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

/// Per-agent-tab metadata. Agent tabs (projects bound by the MCP
/// `open_project` tool) stay in the main window's sidebar; this map adds
/// the lock state.
#[derive(Clone)]
pub struct AgentTabMeta {
    pub locked: bool,
}

/// Agent-bound projects, keyed by project id.
pub type AgentTabs = Arc<RwLock<HashMap<String, AgentTabMeta>>>;

pub struct AppState {
    pub pm: Arc<RwLock<ProjectManager>>,
    /// Maps window labels to project IDs for multi-window support.
    /// Main window ("main") is NOT in this map — it uses the active project.
    /// Project windows ("project-{sanitized_id}") are mapped to their project.
    pub window_projects: Arc<RwLock<HashMap<String, String>>>,
    /// MCP-agent-bound projects, keyed by project id. Lock order: never take
    /// this lock while holding `pm` or `window_projects`.
    pub agent_tabs: AgentTabs,
    /// Paths handed to us by the OS (Open With / double-click / second
    /// instance) that the frontend hasn't consumed yet. The frontend drains
    /// this via `take_pending_opens` on mount so cold-start events that
    /// arrive before the webview is ready are not lost.
    pub pending_opens: Arc<std::sync::Mutex<Vec<String>>>,
    /// The disabled status line at the top of the tray menu, kept so
    /// `set_mcp_config` can refresh its text when the MCP config changes.
    pub tray_status: Arc<std::sync::Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>>,
}

/// Filter OS-supplied open targets (argv / Opened events) to existing files
/// with a supported sequence extension.
pub(crate) fn collect_open_targets<I: IntoIterator<Item = String>>(args: I) -> Vec<String> {
    args.into_iter()
        .filter(|a| !a.starts_with('-'))
        .filter(|a| {
            let p = std::path::Path::new(a);
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| SEQ_EXTS.contains(&e.to_ascii_lowercase().as_str()))
                    .unwrap_or(false)
        })
        .collect()
}

/// Queue OS-opened paths for the frontend and notify it, then focus the main
/// window. The frontend opens each path through the normal `open_file` path.
pub(crate) fn queue_open_targets(app: &AppHandle, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let state = app.state::<AppState>();
    if let Ok(mut pending) = state.pending_opens.lock() {
        for p in &paths {
            if !pending.contains(p) {
                pending.push(p.clone());
            }
        }
    }
    for p in paths {
        let _ = app.emit("file-opened", p);
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
