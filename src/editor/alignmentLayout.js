/** Split an inclusive match range into linear segments; a range with
 *  end < start crosses the origin of a circular sequence. */
export function buildMatchSegs(ms, me, tlen) {
  if (me < ms && tlen > 0) {
    return [
      { start: ms, end: tlen - 1 },
      { start: 0, end: me },
    ];
  }
  return [{ start: ms, end: me }];
}

/** '-' placeholder segments covering template gaps between consecutive
 *  segments of one alignment (split-read deletions), split at the origin so
 *  each piece stays contiguous like real segments. */
export function alignmentGapSegments(al, tlen) {
  const segs = al.segments || [];
  const gaps = [];
  if (!tlen) return gaps;
  for (let i = 0; i + 1 < segs.length; i++) {
    const gap = (((segs[i + 1].start - segs[i].end - 1) % tlen) + tlen) % tlen;
    if (gap === 0) continue;
    const start = (segs[i].end + 1) % tlen;
    for (const { start: s, end: e } of buildMatchSegs(start, (start + gap - 1) % tlen, tlen)) {
      gaps.push({ start: s, end: e, chars: '-'.repeat(e - s + 1), gap: true });
    }
  }
  return gaps;
}

/** Highlight plate behind a read base that needs attention: a mismatch, a
 *  read gap, or a base sitting in an insertion cell (its template row shows
 *  '-'). One colour for all three. */
export const BASE_HILITE_BG = '#fecaca';

/** Anchor template columns of an alignment's "notable sites" for prev/next
 *  navigation: one anchor per run of consecutive attention-plated columns
 *  (the BASE_HILITE_BG mismatch/gap plates — same predicate as the lane
 *  renderer), one per insertion block not touching such a run, plus the
 *  track's first/last aligned columns. A read covering the whole template
 *  (end wraps back to start with no gaps) has no start/end sites. Sorted
 *  ascending, deduped. */
export function alignmentNotableSites(al, sequence, tlen) {
  const segs = al.segments || [];
  if (!segs.length || !tlen) return [];
  const red = new Set();
  for (const seg of [...segs, ...alignmentGapSegments(al, tlen)]) {
    const chars = seg.chars || '';
    for (let i = 0; i < chars.length; i++) {
      const c = chars[i];
      const col = (seg.start + i) % tlen;
      if (c === '-' || c.toUpperCase() !== (sequence[col] || '').toUpperCase()) red.add(col);
    }
  }
  const sites = new Set();
  for (const col of red) {
    if (!red.has((col - 1 + tlen) % tlen)) sites.add(col);
  }
  if (red.size && !sites.size) sites.add(segs[0].start); // whole circle plated
  for (const pos of insertionBases(al).keys()) {
    // The slot renders between pos-1 and pos; merge into a touching red run.
    if (!red.has(pos) && !red.has((pos - 1 + tlen) % tlen)) sites.add(pos);
  }
  const start = segs[0].start;
  const end = segs[segs.length - 1].end;
  if (alignmentGapSegments(al, tlen).length > 0 || (end + 1) % tlen !== start) {
    if (!red.has(start)) sites.add(start);
    if (!red.has(end)) sites.add(end);
  }
  return [...sites].sort((a, b) => a - b);
}

/** Per-anchor insertion width: the longest insertion anchored there across
 *  all alignments (GenePad's merged gap columns). */
export function perAnchorInsertWidths(alns, tlen) {
  const widths = new Map();
  for (const al of alns) {
    for (const ins of al.insertions || []) {
      if (!ins.bases || ins.pos < 0 || ins.pos >= tlen) continue;
      widths.set(ins.pos, Math.max(widths.get(ins.pos) || 0, ins.bases.length));
    }
  }
  return widths;
}

/** Insertion slots keyed by their anchor column, each sized to the longest
 *  insertion anchored there. Flank junk (unalignable read tails) reserves
 *  slots like any other insertion. */
export function alignmentInsertUnion(alns, tlen) {
  return perAnchorInsertWidths(alns, tlen);
}

/** An alignment's insertions deduped by template column — the same map the
 *  display walk and the chromatogram numbering use, so a legacy model with
 *  two entries at one column renders (and counts) exactly the bases the walk
 *  keeps. */
export function insertionBases(al) {
  return new Map((al.insertions || []).map((ins) => [ins.pos, ins.bases]));
}

/** Shared drift-space layout for insertion slots: template rows keep their
 *  grid; alignment lanes shift right past each slot (one column per reserved
 *  base). Insertion bases sit BETWEEN template columns pos-1 and pos — the
 *  model's "extra read bases before template column pos" — so the slot
 *  renders immediately LEFT of its anchor column: `drift(pos)` is the shift
 *  applied to column pos (slots anchored at or before it) and `slotBase(pos)`
 *  the drift-space column where pos's slot starts. Read order (q) and visual
 *  column order therefore always agree. */
export function alignmentLaneLayout(insReserve) {
  const slots = [...insReserve.entries()].sort((a, b) => a[0] - b[0]);
  // cumLt[i] = reserved columns of slots anchored strictly before slots[i];
  // cumLe[i] = including slots[i] — the shift of slots[i]'s anchor column,
  // whose own slot renders just left of it.
  const cumLt = new Array(slots.length);
  const cumLe = new Array(slots.length);
  let acc = 0;
  for (let i = 0; i < slots.length; i++) {
    cumLt[i] = acc;
    acc += slots[i][1];
    cumLe[i] = acc;
  }
  const drift = (pos) => {
    let lo = 0;
    let hi = slots.length - 1;
    let found = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (slots[mid][0] <= pos) {
        found = mid;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
    return found < 0 ? 0 : cumLe[found];
  };
  const slotBase = new Map();
  for (let i = 0; i < slots.length; i++) {
    slotBase.set(slots[i][0], slots[i][0] + cumLt[i]);
  }
  return { slots, drift, slotBase, insTotal: acc };
}

/** Visual-stream layout: template columns and insertion-slot cells form one
 *  unit stream. Template column abs sits at stream index S(abs) = abs +
 *  drift(abs); the w slot cells anchored at abs occupy [S(abs)-w, S(abs)-1]
 *  (left of their anchor). Every row holds exactly visCpl stream units, so
 *  rows never overflow the viewport — a wide slot block simply spans rows,
 *  and the middle rows of such a block contain no template columns at all
 *  (rowCounts[row] === 0, rowStarts[row] = the block's anchor column). */
export function buildStreamLayout(insReserve, seqLen, visCpl) {
  const lane = alignmentLaneLayout(insReserve);
  const streamLen = seqLen + lane.insTotal;
  const numRows = Math.max(1, Math.ceil(streamLen / visCpl));
  const rowStarts = new Array(numRows);
  const rowCounts = new Array(numRows).fill(0);
  let si = 0;
  for (let abs = 0; abs < seqLen; abs++) {
    si += insReserve.get(abs) || 0;
    const row = Math.floor(si / visCpl);
    if (rowCounts[row] === 0) rowStarts[row] = abs;
    rowCounts[row]++;
    si++;
  }
  for (let r = numRows - 1, next = seqLen; r >= 0; r--) {
    if (rowCounts[r] === 0) rowStarts[r] = next;
    else next = rowStarts[r];
  }
  const streamOf = (abs) => (abs >= seqLen ? streamLen : abs + lane.drift(abs));
  // Clamped: the end-of-sequence insert point (abs === seqLen) can land one
  // past the last row when streamLen is an exact multiple of visCpl.
  const rowOf = (abs) => Math.min(numRows - 1, Math.floor(streamOf(abs) / visCpl));
  const colOfAbs = (abs) => streamOf(abs) % visCpl;
  // Largest template column whose stream index is <= si, clamped to
  // [0, seqLen] (seqLen = the insert point past the last base).
  const absFromStream = (s) => {
    const t = Math.max(0, Math.min(streamLen, s));
    if (t >= streamLen) return seqLen;
    let lo = 0,
      hi = seqLen - 1,
      found = 0;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (streamOf(mid) <= t) {
        found = mid;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
    return found;
  };
  const colVis = (col, row) => colOfAbs(rowStarts[row] + col);
  // Smallest row-local template column reaching visual column `vis`; slot
  // cells resolve to the anchor column right of them. May return
  // rowCounts[row] (one past the last column) — callers clamp.
  const colFromVis = (vis, row) => {
    let lo = 0,
      hi = rowCounts[row];
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (colVis(mid, row) < vis) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  };
  // splitRange-shaped pieces ({row, colStart, colEnd, strOffset, len}), split
  // where the stream crosses a row edge. s > e wraps the origin of a circular
  // sequence (two linear halves, each with its own strOffset base).
  const splitLinear = (s, e) => {
    const out = [];
    let cur = s;
    while (cur <= e) {
      const row = rowOf(cur);
      let lo = cur,
        hi = e;
      while (lo < hi) {
        const mid = (lo + hi + 1) >> 1;
        if (rowOf(mid) === row) lo = mid;
        else hi = mid - 1;
      }
      out.push({
        row,
        colStart: cur - rowStarts[row],
        colEnd: lo - rowStarts[row],
        strOffset: cur - s,
        len: lo - cur + 1,
      });
      cur = lo + 1;
    }
    return out;
  };
  const sp = (s, e) =>
    s <= e ? splitLinear(s, e) : [...splitLinear(s, seqLen - 1), ...splitLinear(0, e)];
  return {
    laneLayout: lane,
    insTotal: lane.insTotal,
    streamLen,
    numRows,
    rowStarts,
    rowCounts,
    visCpl,
    streamOf,
    rowOf,
    colOfAbs,
    absFromStream,
    colVis,
    colFromVis,
    sp,
  };
}
