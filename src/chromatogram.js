// Sanger chromatogram helpers: strand orientation and SVG path building.
// A chromatogram is { traceA, traceC, traceG, traceT, peakLocations } with
// one peak sample index per called base (see Rust Chromatogram model).
//
// Trace-rendering approach ported from GenePad (https://github.com/GenePad),
// provided by the GenePad team / https://github.com/Masterchiefm.

export const TRACE_CHANNELS = [
  ['A', 'traceA', '#5cb87a'],
  ['C', 'traceC', '#4e7fff'],
  ['G', 'traceG', '#808080'],
  ['T', 'traceT', '#e85d75'],
];

const reverse = (arr) => {
  const out = new Array(arr.length);
  for (let i = 0; i < arr.length; i++) out[i] = arr[arr.length - 1 - i];
  return out;
};

// Re-orient a chromatogram to match a read displayed on the given strand.
// Alignment segment chars are stored oriented (rev-comp when strand is "-"),
// so the traces must be reversed and complement-swapped (A<->T, C<->G) the
// same way, with peak positions mirrored inside the trace.
export function orientChromatogram(chrom, strand) {
  if (!chrom || strand !== '-') return chrom;
  const traceLength = Math.max(
    chrom.traceA.length,
    chrom.traceC.length,
    chrom.traceG.length,
    chrom.traceT.length,
  );
  return {
    traceA: reverse(chrom.traceT),
    traceC: reverse(chrom.traceG),
    traceG: reverse(chrom.traceC),
    traceT: reverse(chrom.traceA),
    peakLocations: reverse(chrom.peakLocations).map((p) => traceLength - 1 - p),
  };
}

// Max absolute sample value across all channels in [from, to] (sample range).
export function traceRangeMax(chrom, from, to) {
  let max = 0;
  for (const [, key] of TRACE_CHANNELS) {
    const trace = chrom[key];
    for (let i = Math.max(0, from); i <= Math.min(to, trace.length - 1); i++) {
      const v = Math.abs(trace[i]);
      if (v > max) max = v;
    }
  }
  return max;
}

// Build an SVG polyline path for one channel over the anchors
// [{ x, q }] (pixel x, base query index, ascending in x). Trace samples are
// linearly interpolated between consecutive peak positions so each base's
// peak lands on its own pixel column. The polyline breaks where the read is
// discontinuous with the template: deletions (consecutive anchors farther
// apart than `maxAnchorGap` px) and insertions / circular join wraps (query
// index not advancing by exactly 1). Plain matches and mismatches stay
// connected.
export function buildTracePath(chrom, channelKey, anchors, baseY, scaleY, maxAnchorGap = 18) {
  const trace = chrom[channelKey];
  const peaks = chrom.peakLocations;
  if (!trace.length || anchors.length === 0) return '';

  let d = '';
  let open = false;

  const emitPoint = (x, sampleIdx) => {
    // Interpolated indices are fractional; array access needs an integer
    // (trace[10.5] is undefined and yields NaN coordinates, killing the path).
    const clamped = Math.round(Math.max(0, Math.min(trace.length - 1, sampleIdx)));
    const y = baseY - trace[clamped] * scaleY;
    d += `${open ? 'L' : 'M'} ${x.toFixed(1)} ${y.toFixed(1)}`;
    open = true;
  };

  for (let i = 0; i < anchors.length; i++) {
    const a = anchors[i];
    const peakA = peaks[a.q] ?? 0;
    if (i === 0) {
      emitPoint(a.x, peakA);
      continue;
    }
    const prev = anchors[i - 1];
    if (a.x - prev.x > maxAnchorGap || a.q - prev.q !== 1) {
      open = false;
      emitPoint(a.x, peakA); // start a new subpath at this base's peak
      continue;
    }
    const peakPrev = peaks[prev.q] ?? 0;
    const steps = Math.max(1, Math.round(a.x - prev.x));
    for (let s = 1; s <= steps; s++) {
      const x = prev.x + ((a.x - prev.x) * s) / steps;
      const sampleIdx = peakPrev + ((peakA - peakPrev) * s) / steps;
      emitPoint(x, sampleIdx);
    }
  }
  return d;
}

// Walk an alignment's segments in join order producing one ordered entry per
// chromatogram anchor: { col, q, ins, k } where `col` is the template column
// the anchor hangs on, `q` the read index, `ins` marks insertion bases (extra
// read bases before their template column — they consume q but have no
// template column of their own) and `k` is the base's index within its
// insertion run. Insertions anchored at a column the walk never visits
// (junctions, read tails) are spliced in before the first walked column past
// their anchor; q is assigned sequentially in the final order, so every read
// base appears exactly once and q order always matches read order. Mirrors
// GenePad's gapSegments design.
export function buildColumnAnchors(alignment) {
  const insByPos = new Map((alignment.insertions || []).map((ins) => [ins.pos, ins.bases]));
  const walked = new Set();
  const ordered = [];
  for (const seg of alignment.segments || []) {
    const chars = seg.chars || '';
    for (let i = 0; i < chars.length; i++) {
      const pos = seg.start + i;
      const ins = insByPos.get(pos);
      if (ins) {
        walked.add(pos);
        for (let k = 0; k < ins.length; k++) ordered.push({ col: pos, ins: true, k });
      }
      if (chars[i] !== '-') ordered.push({ col: pos, ins: false, k: 0 });
    }
  }
  for (const [pos, ins] of insByPos) {
    if (walked.has(pos)) continue;
    let idx = ordered.findIndex((e) => e.col > pos);
    if (idx < 0) idx = ordered.length;
    for (let k = 0; k < ins.length; k++) ordered.splice(idx + k, 0, { col: pos, ins: true, k });
  }
  let q = 0;
  for (const e of ordered) e.q = q++;
  return ordered;
}

// Map aligned template columns to read query indices. Walks segments in join
// order accumulating read bases ('-' = read gap, consumes no query base) and
// applying insertions (extra query bases before their template column).
// Returns a Map<templateCol, queryIndex> for non-gap columns.
export function buildColumnQueryMap(alignment) {
  const colToQuery = new Map();
  for (const e of buildColumnAnchors(alignment)) {
    if (!e.ins) colToQuery.set(e.col, e.q);
  }
  return colToQuery;
}
