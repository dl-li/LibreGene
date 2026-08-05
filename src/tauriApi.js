/**
 * LibreGene Tauri IPC API layer.
 *
 * Tauri-desktop-only. Uses invoke() for IPC commands and listen() for events.
 */

// Detect if running inside Tauri
export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

// Native operations (window title change, open/save panels) reset AppKit's
// traffic-light layout on macOS without emitting a window event the
// decoration plugin listens to; re-assert the tuned inset afterwards.
const reassertTrafficLights = () => {
  if (isTauri) tauriInvoke('reassert_traffic_lights').catch(() => {});
};

let invoke;
let listen;
let dialog;

const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);

/**
 * Set the Tauri window title.
 * No-op on macOS: the titlebar is hidden (Overlay + hiddenTitle) and the
 * title is drawn by TitleBar itself, while native setTitle triggers an
 * AppKit titlebar relayout that visibly displaces the traffic lights.
 */
export async function setWindowTitle(title) {
  if (!isTauri || isMac) return;
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow().setTitle(title);
    reassertTrafficLights();
  } catch {
    /* ignore */
  }
}

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

export async function writeTextFile(path, contents) {
  return tauriInvoke('write_text_file', { path, contents });
}

// ---------------------------------------------------------------------------
// Sequence
// ---------------------------------------------------------------------------

export async function updateSequence(sequence, features) {
  return tauriInvoke('update_sequence', { sequence, features: features || null });
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

export async function addFeature(feature, locationStr) {
  return tauriInvoke('add_feature', { feature, locationStr: locationStr || null });
}

export async function deleteFeature(id) {
  return tauriInvoke('delete_feature', { id });
}

export async function updateFeatureFtype(featureId, newFtype) {
  return tauriInvoke('update_feature_ftype', { featureId, newFtype });
}

export async function updateFeatureColor(featureId, newColor) {
  return tauriInvoke('update_feature_color', { featureId, newColor });
}

export async function updateFeatureLocation(featureId, locationStr) {
  return tauriInvoke('update_feature_location', { featureId, locationStr });
}

export async function updateFeatureName(featureId, newName) {
  return tauriInvoke('update_feature_name', { featureId, newName });
}

export async function updateFeatureStrand(featureId, strand) {
  return tauriInvoke('update_feature_strand', { featureId, strand });
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

export async function addPrimers(primers) {
  return tauriInvoke('add_primers', { primers });
}

export async function checkPrimersBinding(primers) {
  return tauriInvoke('check_primers_binding', { primers });
}

export async function computePrimerAlignment(
  primerId,
  seedLength,
  customSeq,
  customName,
  tmParams = {},
) {
  return tauriInvoke('compute_primer_alignment', {
    primerId: primerId || null,
    seedLength,
    customSeq: customSeq || null,
    customName: customName || null,
    naConc: tmParams.naConc ?? null,
    mgConc: tmParams.mgConc ?? null,
    dntpConc: tmParams.dntpConc ?? null,
    trisConc: tmParams.trisConc ?? null,
    primerConc: tmParams.primerConc ?? null,
  });
}

// ---------------------------------------------------------------------------
// Alignments
// ---------------------------------------------------------------------------

export async function addAlignment(path) {
  return tauriInvoke('add_alignment', { path });
}

export async function addAlignmentSeq(name, seq) {
  return tauriInvoke('add_alignment_seq', { name, seq });
}

export async function removeAlignment(alignmentId) {
  return tauriInvoke('remove_alignment', { alignmentId });
}

// ---------------------------------------------------------------------------
// Methylation
// ---------------------------------------------------------------------------

export async function setMethylation(systems, overlap = 2) {
  return tauriInvoke('set_methylation', { systems, overlap });
}

export async function getEnzymeDatabase() {
  return tauriInvoke('get_enzyme_database');
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

export async function rekeyProject(oldId, newId) {
  return tauriInvoke('rekey_project', { oldId, newId });
}

// ---------------------------------------------------------------------------
// Tm
// ---------------------------------------------------------------------------

export async function computeTm(seq, tmParams = {}) {
  return tauriInvoke('compute_tm', {
    seq,
    naConc: tmParams.naConc ?? null,
    mgConc: tmParams.mgConc ?? null,
    dntpConc: tmParams.dntpConc ?? null,
    trisConc: tmParams.trisConc ?? null,
    primerConc: tmParams.primerConc ?? null,
  });
}

// ---------------------------------------------------------------------------
// MCP server settings
// ---------------------------------------------------------------------------

export async function getMcpConfig() {
  return tauriInvoke('get_mcp_config');
}

export async function setMcpConfig(enabled, port) {
  return tauriInvoke('set_mcp_config', { enabled, port });
}

export async function getMcpToken() {
  return tauriInvoke('get_mcp_token');
}

export async function regenerateMcpToken() {
  return tauriInvoke('regenerate_mcp_token');
}

// ---------------------------------------------------------------------------
// ORF search / sequence search / primer design (backend-computed)
// ---------------------------------------------------------------------------

export async function findOrfs(minAa = 75) {
  return tauriInvoke('find_orfs', { minAa: minAa ?? null });
}

export async function searchSequence(query) {
  return tauriInvoke('search_sequence', { query });
}

/**
 * Generate primer design candidates in the backend.
 * @param {object} args
 * @param {'amplify'|'oepcr'|'mutagenesis'} args.mode
 * @param {{start:number,end:number}} args.seg  first segment (0-based inclusive)
 * @param {{start:number,end:number}} [args.seg2] second segment (oepcr)
 * @param {string} [args.name] amplify primer-pair name
 * @param {string} [args.name1] oepcr fragment 1 name
 * @param {string} [args.name2] oepcr fragment 2 name
 * @param {string} [args.siteName] mutagenesis site name
 * @param {number} [args.targetTm]
 * @param {number} [args.overlapLen] oepcr overlap length
 * @param {number} [args.armLen] mutagenesis homology arm length
 * @param {string} [args.mutSeq] mutagenesis replacement sequence
 * @param {object} [args.tmParams] concentration overrides (naConc/mgConc/dntpConc/trisConc/primerConc)
 */
export async function designPrimerCandidates(args = {}) {
  const tmParams = args.tmParams || {};
  return tauriInvoke('design_primer_candidates', {
    mode: args.mode,
    seg: args.seg ?? null,
    seg2: args.seg2 ?? null,
    name: args.name ?? null,
    name1: args.name1 ?? null,
    name2: args.name2 ?? null,
    siteName: args.siteName ?? null,
    targetTm: args.targetTm ?? 60,
    overlapLen: args.overlapLen ?? null,
    armLen: args.armLen ?? null,
    mutSeq: args.mutSeq ?? null,
    naConc: tmParams.naConc ?? null,
    mgConc: tmParams.mgConc ?? null,
    dntpConc: tmParams.dntpConc ?? null,
    trisConc: tmParams.trisConc ?? null,
    primerConc: tmParams.primerConc ?? null,
  });
}

// ---------------------------------------------------------------------------
// Tauri dialog helpers
// ---------------------------------------------------------------------------

export async function openFileDialog(defaultPath) {
  if (!isTauri) return null;
  const opts = {
    title: 'Open GenBank/DNA/FASTA files',
    filters: [
      { name: 'DNA Files', extensions: ['gbk', 'gb', 'dna', 'fasta', 'fa', 'fna', 'ab1'] },
      { name: 'All Files', extensions: ['*'] },
    ],
    multiple: true,
  };
  if (defaultPath) opts.defaultPath = defaultPath;
  const result = await tauriOpen(opts);
  reassertTrafficLights();
  if (!result) return null;
  return Array.isArray(result) ? result : [result];
}

export async function openAlignmentFileDialog() {
  if (!isTauri) return null;
  const result = await tauriOpen({
    title: 'Add alignment sequence',
    filters: [
      { name: 'Sequence Files', extensions: ['ab1', 'fasta', 'fa', 'fna', 'gbk', 'gb', 'dna'] },
      { name: 'All Files', extensions: ['*'] },
    ],
    multiple: false,
  });
  reassertTrafficLights();
  if (!result) return null;
  return Array.isArray(result) ? result[0] : result;
}

export async function saveFileDialog(defaultName = 'project.gbk') {
  if (!isTauri) return null;
  const result = await tauriSave({
    title: 'Save GenBank file',
    defaultPath: defaultName,
    filters: [{ name: 'GenBank', extensions: ['gbk'] }],
  });
  reassertTrafficLights();
  return result;
}

export async function saveTextDialog(defaultName = 'export.txt') {
  if (!isTauri) return null;
  const result = await tauriSave({
    title: 'Save text file',
    defaultPath: defaultName,
    filters: [{ name: 'Text', extensions: ['txt'] }],
  });
  reassertTrafficLights();
  return result;
}

// ---------------------------------------------------------------------------
// Tauri event listener
// ---------------------------------------------------------------------------

export function listenProjectUpdates(callback) {
  let closed = false;
  // Hold the listen() promise so close() can still unregister the listener
  // if the component unmounts before the promise resolves — otherwise the
  // unlisten function would be assigned to a discarded closure and the
  // backend subscription would leak.
  const ready = tauriListen('project-update', (event) => {
    if (!closed) callback(event.payload);
  });
  return {
    close: () => {
      closed = true;
      ready.then((fn) => fn()).catch(() => {});
    },
  };
}
