/**
 * Geneie Tauri-aware API layer.
 *
 * In Tauri mode, uses invoke() for IPC commands and listen() for events.
 * In browser mode, falls back to fetch() and WebSocket (requires geneie-server on :8765).
 */

// Detect if running inside Tauri
export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

let invoke;
let listen;
let dialog;

// Lazy-load Tauri modules so non-Tauri builds don't break
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
// Fallback fetch for browser mode
// ---------------------------------------------------------------------------

const BASE = "http://127.0.0.1:8765";

async function request(method, path, body) {
  const opts = { method, headers: { "Content-Type": "application/json" } };
  if (body !== undefined) opts.body = JSON.stringify(body);
  const res = await fetch(`${BASE}${path}`, opts);
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}`);
  return res.json();
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

export async function getProject(filter = 'unique', cpl = 60) {
  if (isTauri) {
    return tauriInvoke('get_project', {
      enzymeFilter: filter,
      cpl: cpl,
    });
  }
  return request("GET", `/project?enzyme_filter=${filter}&cpl=${cpl}`);
}

export async function openFile(path) {
  if (isTauri) {
    return tauriInvoke('open_file', { path });
  }
  return request("POST", `/open?path=${encodeURIComponent(path)}`);
}

export async function saveFile(path) {
  if (isTauri) {
    return tauriInvoke('save_file', { path });
  }
  return request("POST", `/save?path=${encodeURIComponent(path)}`);
}

// ---------------------------------------------------------------------------
// Sequence
// ---------------------------------------------------------------------------

export async function updateSequence(sequence) {
  if (isTauri) {
    return tauriInvoke('update_sequence', { sequence });
  }
  return request("PUT", "/sequence", { sequence });
}

// ---------------------------------------------------------------------------
// ROI
// ---------------------------------------------------------------------------

export async function setROI(start, end) {
  if (isTauri) {
    return tauriInvoke('set_roi', { start, end });
  }
  return request("POST", `/roi?s=${start}&e=${end}`);
}

export async function clearROI() {
  if (isTauri) {
    return tauriInvoke('clear_roi');
  }
  return request("POST", "/roi/clear");
}

// ---------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------

export async function getFeatures() {
  if (isTauri) return tauriInvoke('get_features');
  return request("GET", "/features");
}

export async function addFeature(feature) {
  if (isTauri) return tauriInvoke('add_feature', { feature });
  return request("POST", "/features", feature);
}

export async function deleteFeature(id) {
  if (isTauri) return tauriInvoke('delete_feature', { id });
  return request("DELETE", `/features/${encodeURIComponent(id)}`);
}

// ---------------------------------------------------------------------------
// Primers
// ---------------------------------------------------------------------------

export async function getPrimers() {
  if (isTauri) return tauriInvoke('get_primers');
  return request("GET", "/primers");
}

export async function addPrimer(primer) {
  if (isTauri) return tauriInvoke('add_primer', { primer });
  return request("POST", "/primers", primer);
}

export async function deletePrimer(id) {
  if (isTauri) return tauriInvoke('delete_primer', { id });
  return request("DELETE", `/primers/${encodeURIComponent(id)}`);
}

// ---------------------------------------------------------------------------
// Methylation
// ---------------------------------------------------------------------------

export async function setMethylation(systems, overlap = 2) {
  if (isTauri) {
    return tauriInvoke('set_methylation', { systems, overlap });
  }
  return request("POST", `/methylation?systems=${systems.join(',')}&overlap=${overlap}`);
}

// ---------------------------------------------------------------------------
// Multi-project
// ---------------------------------------------------------------------------

export async function getProjects() {
  if (isTauri) return tauriInvoke('get_projects');
  return request("GET", "/projects");
}

export async function getProjectById(id, filter = 'all') {
  if (isTauri) return tauriInvoke('get_project_by_id', { id, enzymeFilter: filter });
  return request("GET", `/project/${encodeURIComponent(id)}?enzyme_filter=${filter}`);
}

export async function activateProject(id) {
  if (isTauri) return tauriInvoke('activate_project', { id });
  return request("POST", `/projects/activate?id=${encodeURIComponent(id)}`);
}

export async function deleteProject(id) {
  if (isTauri) return tauriInvoke('delete_project', { id });
  return request("DELETE", `/projects/${encodeURIComponent(id)}`);
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
// Tauri event listener (replaces WebSocket)
// ---------------------------------------------------------------------------

export function listenProjectUpdates(callback) {
  if (!isTauri) {
    // Browser mode: return a WebSocket-based listener
    const ws = new WebSocket('ws://127.0.0.1:8765/ws');
    ws.onmessage = (e) => {
      try {
        const msg = JSON.parse(e.data);
        if (msg.type === 'project') {
          callback(msg);
        }
      } catch {}
    };
    return {
      close: () => ws.close(),
    };
  }
  // Tauri mode: use event listener
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
