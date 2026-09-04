/**
 * LibreGene Tauri IPC API layer.
 *
 * Tauri-desktop-only. Uses invoke() for IPC commands and listen() for events.
 */

// Detect if running inside Tauri
export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

// --- Agent-tab edit lock -----------------------------------------------------
// While the active project is bound to an MCP agent tab and locked, commands
// that would dirty the project are refused before reaching the backend
// (ProjectWorkspace also guards its own handlers ahead of optimistic UI
// updates; this is the catch-all for direct callers such as dialogs).
// Read-only commands, scrolling and selection are never affected.
let agentEditLocked = false;
export function setAgentEditLock(locked) {
  agentEditLocked = !!locked;
}
const AGENT_LOCK_MSG =
  'This project is controlled by an MCP agent. Unlock it from the bottom bar to edit.';
function assertEditable() {
  if (agentEditLocked) throw new Error(AGENT_LOCK_MSG);
}

// Native operations (window title change, open/save panels) reset AppKit's
// traffic-light layout on macOS without emitting a window event the
// decoration plugin listens to; re-assert the tuned inset afterwards.
const reassertTrafficLights = () => {
  if (isTauri) tauriInvoke('reassert_traffic_lights').catch(() => {});
};

let invoke;
let listen;
let emit;
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

export async function openFile(path, recordIndex) {
  return tauriInvoke('open_file', { path, recordIndex: recordIndex ?? null });
}

/**
 * Scan a FASTA file's records (name + length) without opening it, so the
 * caller can offer to split a multi-record file into separate projects.
 * Returns `{ records: [{name, length}] }` (empty for non-FASTA extensions).
 */
export async function peekFastaRecords(path) {
  return tauriInvoke('peek_fasta_records', { path });
}

/**
 * Create a new in-memory project from pasted sequence (Empty-page "New
 * Sequence" dialog). Returns the same shape as openFile plus `id` (the
 * generated project id, `untitled-<millis>`).
 * @param {object} args
 * @param {string} args.name
 * @param {string} args.sequence
 * @param {'dna'|'rna'|'protein'} args.moleculeType
 * @param {'circular'|'linear'} args.topology
 * @param {Array<{name:string,ftype:string,color:string,strand:string,segments:Array<{start:number,end:number}>}>} [args.features]
 */
export async function createProject(args = {}) {
  return tauriInvoke('create_project', {
    name: args.name,
    sequence: args.sequence,
    moleculeType: args.moleculeType,
    topology: args.topology,
    features: args.features ?? [],
  });
}

/**
 * Run automatic annotation on a bare sequence (no project required), for the
 * New Sequence dialog's live feature preview. Returns read-only AnnotatedFeature
 * (camelCase, 0-based inclusive).
 */
export async function annotateSequenceText(sequence, circular = false, moleculeType = 'dna') {
  return tauriInvoke('annotate_sequence', { sequence, circular, moleculeType });
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

export async function updateSequence(sequence, features, primers) {
  assertEditable();
  return tauriInvoke('update_sequence', {
    sequence,
    features: features || null,
    // Optional full replacement of the project's primers (undo/redo restores
    // the snapshot's primers); the backend recomputes binding sites.
    primers: primers || null,
  });
}

// ---------------------------------------------------------------------------
// ROI
// ---------------------------------------------------------------------------

export async function setROI(start, end) {
  assertEditable();
  return tauriInvoke('set_roi', { start, end });
}

export async function clearROI() {
  assertEditable();
  return tauriInvoke('clear_roi');
}

// ---------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------

export async function getFeatures() {
  return tauriInvoke('get_features');
}

export async function addFeature(feature, locationStr) {
  assertEditable();
  return tauriInvoke('add_feature', { feature, locationStr: locationStr || null });
}

export async function deleteFeature(id) {
  assertEditable();
  return tauriInvoke('delete_feature', { id });
}

export async function updateFeatureFtype(featureId, newFtype) {
  assertEditable();
  return tauriInvoke('update_feature_ftype', { featureId, newFtype });
}

export async function updateFeatureColor(featureId, newColor) {
  assertEditable();
  return tauriInvoke('update_feature_color', { featureId, newColor });
}

export async function updateFeatureLocation(featureId, locationStr) {
  assertEditable();
  return tauriInvoke('update_feature_location', { featureId, locationStr });
}

export async function updateFeatureName(featureId, newName) {
  assertEditable();
  return tauriInvoke('update_feature_name', { featureId, newName });
}

export async function updateFeatureStrand(featureId, strand) {
  assertEditable();
  return tauriInvoke('update_feature_strand', { featureId, strand });
}

// ---------------------------------------------------------------------------
// Primers
// ---------------------------------------------------------------------------

export async function getPrimers() {
  return tauriInvoke('get_primers');
}

export async function addPrimer(primer) {
  assertEditable();
  return tauriInvoke('add_primer', { primer });
}

export async function deletePrimer(id) {
  assertEditable();
  return tauriInvoke('delete_primer', { id });
}

export async function addPrimers(primers) {
  assertEditable();
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
  assertEditable();
  return tauriInvoke('add_alignment', { path });
}

export async function addAlignmentSeq(name, seq) {
  assertEditable();
  return tauriInvoke('add_alignment_seq', { name, seq });
}

export async function removeAlignment(alignmentId) {
  assertEditable();
  return tauriInvoke('remove_alignment', { alignmentId });
}

// ---------------------------------------------------------------------------
// Methylation
// ---------------------------------------------------------------------------

export async function setMethylation(systems, overlap = 2, projectId) {
  assertEditable();
  return tauriInvoke('set_methylation', { systems, overlap, projectId });
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

// --- Agent tabs (MCP-controlled, user-locked) ---

/** Returns {projectId, locked} when the project is bound as an agent tab, else null. */
export async function getAgentTabState(projectId) {
  return tauriInvoke('get_agent_tab_state', { projectId });
}

/** Unlock/lock the agent tab bound to the project (the on-screen button). */
export async function setAgentTabLocked(projectId, locked) {
  return tauriInvoke('set_agent_tab_locked', { projectId, locked });
}

/** Listen for agent-tab lock-state changes (app-wide, payload {projectId, locked}). */
export function listenAgentTabLock(callback) {
  let closed = false;
  const ready = tauriListen('agent-tab-lock', (event) => {
    if (!closed) callback(event.payload);
  });
  return {
    close: () => {
      closed = true;
      ready.then((fn) => fn()).catch(() => {});
    },
  };
}

export async function rekeyProject(oldId, newId) {
  return tauriInvoke('rekey_project', { oldId, newId });
}

/** Quit the app immediately, discarding unsaved changes (user-confirmed). */
export async function forceQuit() {
  return tauriInvoke('force_quit');
}

/** Listen for tray Quit requests blocked by unsaved changes (payload: dirty project ids). */
export function listenQuitRequested(callback) {
  let closed = false;
  const ready = tauriListen('quit-requested', (event) => {
    if (!closed) callback(event.payload);
  });
  return {
    close: () => {
      closed = true;
      ready.then((fn) => fn()).catch(() => {});
    },
  };
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
// BLAST (NCBI URL API; results open in the system browser)
// ---------------------------------------------------------------------------

export async function blastSubmit(sequence, moleculeType) {
  return tauriInvoke('blast_submit', { sequence, moleculeType });
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
// ORF search / sequence search / primer design / auto-annotation (backend-computed)
// ---------------------------------------------------------------------------

export async function findOrfs(minAa = 75) {
  return tauriInvoke('find_orfs', { minAa: minAa ?? null });
}

/**
 * Detect common features (promoters, CDS, origins, …) in the active project's
 * sequence against the embedded SnapGene feature database.
 * Returns a read-only array of AnnotatedFeature (camelCase).
 */
export async function annotateFeatures() {
  return tauriInvoke('annotate_features');
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
// Codon optimization
// ---------------------------------------------------------------------------

/** List built-in codon-usage species keys (e.g. "e_coli", "h_sapiens"). */
export async function listCodonSpecies() {
  return tauriInvoke('list_codon_species');
}

/**
 * Read-only preview of a CDS/mRNA feature's synonymous codon optimization.
 * @param {object} args
 * @param {string} args.featureId
 * @param {string} args.species  species key, or "custom" when customTable is given
 * @param {'use_best_codon'|'match_codon_usage'|'harmonize_rca'} args.method
 * @param {Array<[string,string,number]>} [args.customTable] parsed Kazusa rows (aa, codon, freq)
 * @param {string} [args.originalSpecies] required by harmonize_rca
 * @param {string[]} [args.avoidEnzymeSites] recognition sequences to avoid
 * @param {[number,number,number]} [args.gcWindow] (windowBp, minGC, maxGC)
 */
export async function previewCodonOptimization(args = {}) {
  return tauriInvoke('preview_codon_optimization', {
    featureId: args.featureId,
    species: args.species,
    method: args.method,
    customTable: args.customTable ?? null,
    originalSpecies: args.originalSpecies ?? null,
    avoidEnzymeSites: args.avoidEnzymeSites ?? null,
    gcWindow: args.gcWindow ?? null,
  });
}

/** Apply a codon optimization (same args as preview); returns summary + { ok, message }. */
export async function applyCodonOptimization(args = {}) {
  assertEditable();
  return tauriInvoke('apply_codon_optimization', {
    featureId: args.featureId,
    species: args.species,
    method: args.method,
    customTable: args.customTable ?? null,
    originalSpecies: args.originalSpecies ?? null,
    avoidEnzymeSites: args.avoidEnzymeSites ?? null,
    gcWindow: args.gcWindow ?? null,
  });
}

// ---------------------------------------------------------------------------
// Tauri dialog helpers
// ---------------------------------------------------------------------------

export async function openFileDialog(defaultPath) {
  if (!isTauri) return null;
  const opts = {
    title: 'Open GenBank/DNA/RNA/Protein files',
    filters: [
      {
        name: 'Sequence Files',
        extensions: [
          'gbk',
          'gb',
          'genbank',
          'gbf',
          'gbff',
          'dna',
          'rna',
          'prot',
          'gpt',
          'gp',
          'gpe',
          'gpff',
          'fasta',
          'fa',
          'fna',
          'fas',
          'ffn',
          'fsa',
          'faa',
          'frn',
          'seq',
          'ab1',
        ],
      },
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
      {
        name: 'Sequence Files',
        extensions: [
          'ab1',
          'fasta',
          'fa',
          'fna',
          'fas',
          'ffn',
          'fsa',
          'faa',
          'frn',
          'seq',
          'gbk',
          'gb',
          'genbank',
          'gbf',
          'gbff',
          'dna',
        ],
      },
      { name: 'All Files', extensions: ['*'] },
    ],
    multiple: true,
  });
  reassertTrafficLights();
  if (!result) return null;
  return Array.isArray(result) ? result : [result];
}

export async function saveFileDialog(defaultName = 'project.gbk', ext = 'gbk') {
  if (!isTauri) return null;
  const label = ext === 'gpt' ? 'Protein GenBank' : 'GenBank';
  const result = await tauriSave({
    title: 'Save GenBank file',
    defaultPath: defaultName,
    filters: [{ name: label, extensions: [ext] }],
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

/** Drain OS-opened file paths queued before the webview was ready. */
export async function takePendingOpens() {
  return tauriInvoke('take_pending_opens');
}

/** Listen for OS file-open events (Open With / double-click / second instance). */
export function listenFileOpened(callback) {
  let closed = false;
  const ready = tauriListen('file-opened', (event) => {
    if (!closed) callback(event.payload);
  });
  return {
    close: () => {
      closed = true;
      ready.then((fn) => fn()).catch(() => {});
    },
  };
}

// Mirrors SEQ_EXTS in src-tauri/src/lib.rs — keep the two lists in sync.
export const SEQ_FILE_EXTS = [
  'gbk',
  'gb',
  'genbank',
  'gbf',
  'gbff',
  'dna',
  'rna',
  'prot',
  'gpt',
  'gp',
  'gpe',
  'gpff',
  'fasta',
  'fa',
  'fna',
  'fas',
  'ffn',
  'fsa',
  'faa',
  'frn',
  'ab1',
  'seq',
];

export function isSequenceFilePath(path) {
  if (!path || typeof path !== 'string') return false;
  const base = path.replace(/\\/g, '/').split('/').pop() || '';
  const dot = base.lastIndexOf('.');
  if (dot <= 0) return false;
  return SEQ_FILE_EXTS.includes(base.slice(dot + 1).toLowerCase());
}

/** Listen for files dragged onto this webview window (all OSes). */
export function listenDragDrop(callback) {
  if (!isTauri) return { close: () => {} };
  let closed = false;
  const ready = import('@tauri-apps/api/webview')
    .then((mod) =>
      mod.getCurrentWebview().onDragDropEvent((event) => {
        if (closed || event.payload.type !== 'drop') return;
        callback(event.payload.paths || []);
      }),
    )
    .catch(() => () => {});
  return {
    close: () => {
      closed = true;
      ready.then((fn) => fn && fn()).catch(() => {});
    },
  };
}

/** Broadcast the per-molecule-type editor background map so other windows sync immediately. */
export function emitEditorBackground(backgrounds) {
  if (!isTauri) return;
  (async () => {
    if (!emit) {
      const mod = await import('@tauri-apps/api/event');
      emit = mod.emit;
    }
    await emit('editor-background-changed', backgrounds);
  })().catch(() => {});
}

/** Listen for editor-background changes broadcast from other windows. */
export function listenEditorBackground(callback) {
  let closed = false;
  const ready = tauriListen('editor-background-changed', (event) => {
    if (!closed) callback(event.payload);
  });
  return {
    close: () => {
      closed = true;
      ready.then((fn) => fn()).catch(() => {});
    },
  };
}
