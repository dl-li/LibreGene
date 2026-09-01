export const cw = 12;
export const startX = 220;
export const baseSeqY = 100;
export const titleBarH = 40;
export const bgColor = '#fdfbf7';
export const selBgColor = '#fef3c7';
export const enzymeSelColor = '#e0f2fe';
export const enzymeActiveBlue = '#1E40AF';
export const amplimerGreen = '#166534';
export const charHeight = cw;
export const monoFont =
  '"Cascadia Code", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace';
export const sansFont = 'sans-serif';
export const springAnim = 'all 0.3s cubic-bezier(0.16, 1, 0.3, 1)';

const _ctx =
  typeof document !== 'undefined' ? document.createElement('canvas').getContext('2d') : null;
if (_ctx) {
  _ctx.fontFeatureSettings = '"calt" on, "ss01" on';
}
const _wCache = new Map();
const CACHE_MAX = 2000;
const CACHE_PRUNE = 300;

export const getX = (col) => startX + col * cw;

export const complement = (c) =>
  c === 'A' ? 'T'
  : c === 'T' ? 'A'
  : c === 'G' ? 'C'
  : c === 'C' ? 'G'
  : c === 'a' ? 't'
  : c === 't' ? 'a'
  : c === 'g' ? 'c'
  : c === 'c' ? 'g'
  : c;

export const measureWidth = (text, font) => {
  if (!_ctx) return text.length * 8;
  const key = `${font}|${text}`;
  const cached = _wCache.get(key);
  if (cached !== undefined) return cached;
  if (_wCache.size >= CACHE_MAX) {
    let count = 0;
    for (const k of _wCache.keys()) {
      _wCache.delete(k);
      if (++count >= CACHE_PRUNE) break;
    }
  }
  _ctx.font = font;
  const w = _ctx.measureText(text).width;
  _wCache.set(key, w);
  return w;
};

export const enzLabelW = (name, isUnique) =>
  measureWidth(name, `${isUnique ? '700 ' : '350 '}14px ${monoFont}`) + 4;
export const primerLabelW = (name) => measureWidth(name, 'italic 600 12px TeX Gyre Heros');
export const featLabelW = (name) => measureWidth(name, 'italic 600 12px TeX Gyre Heros');

export function splitRange(start, end, charsPerLine) {
  const segments = [];
  let curr = start;
  while (curr <= end) {
    const row = Math.floor(curr / charsPerLine);
    const rowEnd = Math.min(end, (row + 1) * charsPerLine - 1);
    segments.push({
      row,
      colStart: curr % charsPerLine,
      colEnd: rowEnd % charsPerLine,
      strOffset: curr - start,
      len: rowEnd - curr + 1,
    });
    curr = rowEnd + 1;
  }
  return segments;
}

/** Wrap-aware inclusive-range slice: start > end crosses the origin of a
 *  circular sequence. */
export function sliceRange(seq, start, end) {
  return start <= end
    ? seq.substring(start, end + 1)
    : seq.substring(start) + seq.substring(0, end + 1);
}

/** Length of an inclusive range; start > end wraps the origin (totalLen needed). */
export function rangeLen(start, end, totalLen) {
  return start <= end ? end - start + 1 : totalLen - start + end + 1;
}

/** Selection range of a feature in join order: first segment's start .. last
 *  segment's end. start > end means the feature crosses the origin. */
export function featureSelRange(f) {
  const segs = f.segments?.length ? f.segments : [{ start: f.start, end: f.end }];
  return [segs[0].start, segs[segs.length - 1].end];
}

/** 1-based inclusive location string for a possibly origin-wrapping range. */
export function rangeLocString1based(start, end, totalLen) {
  return start <= end
    ? `${start + 1}..${end + 1}`
    : `join(${start + 1}..${totalLen},1..${end + 1})`;
}

/**
 * Location strings: the UI shows GenBank-style 1-based inclusive locations to
 * the user, while the Tauri IPC layer (`add_feature` / `update_feature_location`)
 * parses 0-based inclusive strings. These helpers convert at the UI boundary.
 */

/** Build a location string from a feature/hit ({start, end, segments?, strand?},
 * 0-based inclusive model coordinates). `base` is 0 or 1. */
function locationStringFromSegments(feature, base) {
  const segs = feature.segments?.length
    ? feature.segments
    : [{ start: feature.start, end: feature.end }];
  const parts = segs.map((s) => `${s.start + base}..${s.end + base}`);
  const joined = parts.length > 1 ? `join(${parts.join(', ')})` : parts[0];
  return feature.strand === '-' ? `complement(${joined})` : joined;
}

/** 1-based inclusive display string (GenBank convention), for rendering only. */
export const locationString1based = (feature) => locationStringFromSegments(feature, 1);

/** 0-based inclusive string for the Tauri IPC layer (add_feature locationStr). */
export const locationString0based = (feature) => locationStringFromSegments(feature, 0);

/** Convert a user-entered 1-based location string to 0-based for Tauri IPC.
 * Decrements every number token; join/order/complement nesting and single
 * points need no special handling. */
export function locationStringTo0based(str) {
  return str.replace(/\d+/g, (m) => String(Number(m) - 1));
}

export const DEFAULT_TM_PARAMS = {
  naConc: 0.05,
  mgConc: 0,
  dntpConc: 0,
  trisConc: 0,
  primerConc: 2.5e-7,
};

// Average isotopic residue masses (Da) of the 20 standard amino acids.
const AA_RESIDUE_MASS = {
  A: 71.0788,
  R: 156.1875,
  N: 114.1038,
  D: 115.0886,
  C: 103.1388,
  E: 129.1155,
  Q: 128.1307,
  G: 57.0519,
  H: 137.1411,
  I: 113.1594,
  L: 113.1594,
  K: 128.1741,
  M: 131.1926,
  F: 147.1766,
  P: 97.1167,
  S: 87.0782,
  T: 101.1051,
  W: 186.2132,
  Y: 163.176,
  V: 99.1326,
};
const WATER_MASS = 18.015;

/** Molecular weight of a peptide in kDa; non-standard residues are ignored. */
export function peptideMassKda(seq) {
  let da = WATER_MASS;
  for (const c of seq) da += AA_RESIDUE_MASS[c] || 0;
  return da / 1000;
}
