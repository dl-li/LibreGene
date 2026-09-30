// Per-file memory of hidden alignment tracks, persisted as names keyed by
// file path (alignment ids are positional and shift when a track is removed).
const HIDDEN_ALN_KEY = 'hiddenAlignments';
export function readHiddenAlnNames(pid) {
  try {
    const all = JSON.parse(localStorage.getItem(HIDDEN_ALN_KEY));
    return Array.isArray(all?.[pid]) ? all[pid] : [];
  } catch {
    return [];
  }
}
export function writeHiddenAlnNames(pid, names) {
  if (!pid) return;
  try {
    const all = JSON.parse(localStorage.getItem(HIDDEN_ALN_KEY)) || {};
    if (names.length) all[pid] = names;
    else delete all[pid];
    localStorage.setItem(HIDDEN_ALN_KEY, JSON.stringify(all));
  } catch {
    // storage may be unavailable; memory just won't persist
  }
}
