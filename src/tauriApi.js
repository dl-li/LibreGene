/**
 * Geneie Tauri IPC API layer.
 *
 * Tauri-desktop-only. Uses invoke() for IPC commands and listen() for events.
 */

// Detect if running inside Tauri
export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

let invoke;
let listen;
let dialog;

async function tauriInvoke(cmd, args) {
  if (!invoke) {
    const mod = await import('@tauri-apps/api/core');
    invoke = mod.invoke;
  }
  return invoke(cmd, args);
}

async function tauriListen(event, callback) {
  if (!listen) {
    const mod = await import('@tauri-apps/api/event');
    listen = mod.listen;
  }
  return listen(event, callback);
}

async function tauriOpen(options) {
  if (!dialog) {
    const mod = await import('@tauri-apps/plugin-dialog');
    dialog = mod;
  }
  return dialog.open(options);
}

async function tauriSave(options) {
  if (!dialog) {
    const mod = await import('@tauri-apps/plugin-dialog');
    dialog = mod;
  }
  return dialog.save(options);
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

export async function getProject(filter = 'unique', cpl = 60) {
  return tauriInvoke('get_project', { enzymeFilter: filter, cpl });
}

export async function openFile(path) {
  return tauriInvoke('open_file', { path });
}

export async function saveFile(path) {
  return tauriInvoke('save_file', { path });
}

// ---------------------------------------------------------------------------
// Sequence
// ---------------------------------------------------------------------------

export async function updateSequence(sequence) {
  return tauriInvoke('update_sequence', { sequence });
}

// ---------------------------------------------------------------------------
// ROI
// ---------------------------------------------------------------------------

export async function setROI(start, end) {
  return tauriInvoke('set_roi', { start, end });
}

export async function clearROI() {
  return tauriInvoke('clear_roi');
}

// ---------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------

export async function getFeatures() {
  return tauriInvoke('get_features');
}

export async function addFeature(feature) {
  return tauriInvoke('add_feature', { feature });
}

export async function deleteFeature(id) {
  return tauriInvoke('delete_feature', { id });
}

// ---------------------------------------------------------------------------
// Primers
// ---------------------------------------------------------------------------

export async function getPrimers() {
  return tauriInvoke('get_primers');
}

export async function addPrimer(primer) {
  return tauriInvoke('add_primer', { primer });
}

export async function deletePrimer(id) {
  return tauriInvoke('delete_primer', { id });
}

// ---------------------------------------------------------------------------
// Methylation
// ---------------------------------------------------------------------------

export async function setMethylation(systems, overlap = 2) {
  return tauriInvoke('set_methylation', { systems, overlap });
}

// ---------------------------------------------------------------------------
// Multi-project
// ---------------------------------------------------------------------------

export async function getProjects() {
  return tauriInvoke('get_projects');
}

export async function getProjectById(id, filter = 'all') {
  return tauriInvoke('get_project_by_id', { id, enzymeFilter: filter });
}

export async function activateProject(id) {
  return tauriInvoke('activate_project', { id });
}

export async function deleteProject(id) {
  return tauriInvoke('delete_project', { id });
}

// ---------------------------------------------------------------------------
// Multi-window
// ---------------------------------------------------------------------------

export async function openInNewWindow(projectId) {
  return tauriInvoke('open_in_new_window', { projectId });
}

export async function getWindowProjectId() {
  return tauriInvoke('get_window_project_id');
}

// ---------------------------------------------------------------------------
// Tauri dialog helpers
// ---------------------------------------------------------------------------

export async function openFileDialog() {
  if (!isTauri) return null;
  const result = await tauriOpen({
    title: 'Open GenBank/DNA/FASTA files',
    filters: [
      { name: 'DNA Files', extensions: ['gbk', 'gb', 'dna', 'fasta', 'fa', 'fna', 'ab1'] },
      { name: 'All Files', extensions: ['*'] },
    ],
    multiple: true,
  });
  if (!result) return null;
  return Array.isArray(result) ? result : [result];
}

export async function saveFileDialog(defaultName = 'project.gbk') {
  if (!isTauri) return null;
  return tauriSave({
    title: 'Save GenBank file',
    defaultPath: defaultName,
    filters: [
      { name: 'GenBank', extensions: ['gbk'] },
    ],
  });
}

// ---------------------------------------------------------------------------
// Tauri event listener
// ---------------------------------------------------------------------------

export function listenProjectUpdates(callback) {
  let unlistenFn = null;
  tauriListen('project-update', (event) => {
    callback(event.payload);
  }).then(fn => {
    unlistenFn = fn;
  });
  return {
    close: () => {
      if (unlistenFn) unlistenFn();
    },
  };
}
