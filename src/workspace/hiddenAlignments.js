// Per-file alignment visibility, keyed by project path. Track names are stored
// instead of positional ids, which shift when a track is removed.
const HIDDEN_ALN_KEY = 'hiddenAlignments';
const SHOW_ALN_KEY = 'showAlignments';

function readStore(key) {
  try {
    const all = JSON.parse(localStorage.getItem(key));
    return all && typeof all === 'object' && !Array.isArray(all) ? all : {};
  } catch {
    return {};
  }
}

function writeStore(key, all) {
  try {
    localStorage.setItem(key, JSON.stringify(all));
  } catch {
    // storage may be unavailable; memory just won't persist
  }
}

export function readHiddenAlnNames(pid) {
  const names = readStore(HIDDEN_ALN_KEY)[pid];
  return Array.isArray(names) ? names : [];
}

export function writeHiddenAlnNames(pid, names) {
  if (!pid) return;
  const all = readStore(HIDDEN_ALN_KEY);
  if (names.length) all[pid] = names;
  else delete all[pid];
  writeStore(HIDDEN_ALN_KEY, all);
}

export function readShowAlignments(pid) {
  const visible = readStore(SHOW_ALN_KEY)[pid];
  return typeof visible === 'boolean' ? visible : true;
}

export function writeShowAlignments(pid, visible) {
  if (!pid) return;
  const all = readStore(SHOW_ALN_KEY);
  all[pid] = visible;
  writeStore(SHOW_ALN_KEY, all);
}

export function copyAlignmentVisibility(sourcePid, targetPid) {
  if (!sourcePid || !targetPid || sourcePid === targetPid) return;
  writeHiddenAlnNames(targetPid, readHiddenAlnNames(sourcePid));
  writeShowAlignments(targetPid, readShowAlignments(sourcePid));
}

export function shouldRevealAlignments(showAlignments, alignments, hiddenAlignIds) {
  return (
    !showAlignments &&
    alignments.length > 0 &&
    alignments.every((alignment) => hiddenAlignIds.includes(alignment.id))
  );
}
