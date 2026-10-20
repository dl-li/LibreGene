// Per-strand sequence text colors (ProjectData.textColorsTop/Bottom):
// [{start, end, color}] runs, 0-based inclusive, canonical (sorted,
// non-overlapping, no adjacent same-color runs — enforced by the backend's
// strand_colors::canonicalize and by the writers below).

// Preset palette for the selection context menu; hex values match the
// SnapGene named-color mapping in backend strand_colors.rs.
export const TEXT_COLOR_PALETTE = [
  { label: 'Green', color: '#00b050' },
  { label: 'Blue', color: '#0070c0' },
  { label: 'Orange', color: '#ffa500' },
  { label: 'Red', color: '#ff0000' },
  { label: 'Purple', color: '#7030a0' },
  { label: 'Cyan', color: '#00b0f0' },
  { label: 'Yellow', color: '#e6e600' },
  { label: 'Gray', color: '#808080' },
];

export const DEFAULT_TEXT_FILL = '#1f2937';

/**
 * Shift/clip color runs after replacing [editStart, editEnd] (0-based
 * inclusive, oldLen bases) with newLen bases. Mirrors the backend
 * utils::adjust_text_colors_for_edit (same span math as adjustAnnotations):
 * equal-length edits are a no-op, runs fully inside the replaced span drop,
 * runs spanning it stretch over the inserted bases.
 */
export function adjustTextColorsForEdit(ranges, editStart, editEnd, oldLen, newLen) {
  const delta = newLen - oldLen;
  if (delta === 0) return ranges;
  const out = [];
  for (const r of ranges || []) {
    const s = r.start;
    const e = r.end;
    if (e < editStart) {
      out.push(r);
      continue;
    }
    if (s > editEnd) {
      out.push({ ...r, start: s + delta, end: e + delta });
      continue;
    }
    const ns = s < editStart ? s : editStart + newLen;
    const ne = e > editEnd ? e + delta : editStart + newLen - 1;
    if (ns > ne) continue;
    out.push({ ...r, start: ns, end: ne });
  }
  // Re-merge adjacent same-color runs produced by clipping.
  out.sort((a, b) => a.start - b.start);
  const merged = [];
  for (const r of out) {
    const last = merged[merged.length - 1];
    if (last && last.color === r.color && last.end + 1 >= r.start) {
      last.end = Math.max(last.end, r.end);
      continue;
    }
    merged.push({ ...r });
  }
  return merged;
}

/**
 * Split the `len` columns starting at absolute template column `absStart`
 * into pieces of uniform color. Returns [{off, len, color}] where color is
 * null for default-colored stretches. `ranges` must be canonical.
 */
export function splitByTextColor(absStart, len, ranges) {
  const pieces = [];
  let pos = 0;
  for (const r of ranges || []) {
    const s = Math.max(r.start, absStart) - absStart;
    const e = Math.min(r.end, absStart + len - 1) - absStart;
    if (s > e) continue;
    if (s > pos) pieces.push({ off: pos, len: s - pos, color: null });
    pieces.push({ off: s, len: e - s + 1, color: r.color });
    pos = e + 1;
  }
  if (pos < len) pieces.push({ off: pos, len: len - pos, color: null });
  return pieces;
}
