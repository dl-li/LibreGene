import { useMemo } from 'react';
import { cw, getX } from '../../editorConstants';

export const SHOW_GC_CONTENT_KEY = 'showGcContent';
export const GC_WINDOW_SIZE_KEY = 'gcWindowSize';

// GC-content track: a full-width per-base gradient band in the first lane
// below the sequence (everything else shifts down by GC_TRACK_H + GC_GAP).
const GC_TRACK_H = 10;
const GC_GAP = 4;

// GC-content track color: 0% → blue, 50% → white, 100% → red. Mildly
// non-linear cubic keeps the 40–60% range flat and near-white, while GC
// below 30% / above 70% shifts color faster.
export const gcContentColor = (frac) => {
  const f = Math.min(1, Math.max(0, frac));
  const x = f - 0.5;
  const t = Math.min(1, Math.max(0, 0.5 + 0.636 * x + 1.458 * x * x * x));
  const lerp = (a, b, u) => Math.round(a + (b - a) * u);
  if (t <= 0.5) {
    const u = t * 2;
    return `rgb(${lerp(37, 255, u)},${lerp(99, 255, u)},${lerp(235, 255, u)})`;
  }
  const u = (t - 0.5) * 2;
  return `rgb(${lerp(255, 220, u)},${lerp(255, 38, u)},${lerp(255, 38, u)})`;
};

// Per-base local GC fractions for the GC-content track. Window = the base
// plus `half` flanks on each side; circular sequences wrap across the origin,
// linear ones truncate the window at the ends.
export function useGcLane({ cleanSeq, topology, moleculeType, enabled, windowSize = 11 }) {
  return useMemo(() => {
    if (!enabled || moleculeType === 'protein' || !cleanSeq.length) return null;
    const n = cleanSeq.length;
    const half = Math.max(0, Math.floor((windowSize - 1) / 2));
    const pre = new Float64Array(n + 1);
    for (let i = 0; i < n; i++) {
      const c = cleanSeq[i];
      pre[i + 1] = pre[i] + (c === 'G' || c === 'g' || c === 'C' || c === 'c' ? 1 : 0);
    }
    const fracs = new Float64Array(n);
    if (topology === 'circular') {
      const w = Math.min(2 * half + 1, n);
      for (let i = 0; i < n; i++) {
        const start = (((i - half) % n) + n) % n;
        const gc =
          start + w <= n ? pre[start + w] - pre[start] : pre[n] - pre[start] + pre[start + w - n];
        fracs[i] = gc / w;
      }
    } else {
      for (let i = 0; i < n; i++) {
        const lo = Math.max(0, i - half);
        const hi = Math.min(n - 1, i + half);
        fracs[i] = (pre[hi + 1] - pre[lo]) / (hi - lo + 1);
      }
    }
    return { height: GC_TRACK_H + GC_GAP, fracs };
  }, [enabled, moleculeType, cleanSeq, windowSize, topology]);
}

// One gradient per row spanning the row's full visual width, filled into one
// rect per contiguous run: per-base stops at drifted column centers make the
// track a continuous blue→white→red ramp that breaks at insertion-slot
// columns instead of shifting. The band is nudged up toward the sequence
// text; the reserved lane height (alignLaneInfo.trackH) is unchanged so
// nothing else moves.
// Wide rows (continuous mode = one row spanning the whole sequence) are split
// into GC_CHUNK-cell blocks, one gradient each: renderers degrade to solid
// blocks when a single gradient carries thousands of stops.
const GC_CHUNK = 256;

export function renderGcTrack(ctx, lane) {
  const fracs = lane?.fracs;
  if (!fracs) return null;
  const {
    visibleRows,
    rowBuf,
    numRows,
    rowStarts,
    rowCounts,
    getSeqY,
    lp,
    idPrefix,
    colVis,
    colRuns,
    visibleCols,
  } = ctx;
  const vs = Math.max(0, visibleRows.start - rowBuf);
  const ve = Math.min(numRows - 1, visibleRows.end + rowBuf);
  const rows = [];
  for (let r = vs; r <= ve; r++) {
    const count = rowCounts[r];
    if (count <= 0) continue;
    const rowStart = rowStarts[r];
    const y = getSeqY(r) + lp.featBaseOffset - 6;
    const rowVisW = colVis(count - 1, r) + 1;
    // Bucket columns into fixed-width visual blocks; narrow rows stay a
    // single block exactly as before.
    const nBlocks = rowVisW > GC_CHUNK * 2 ? Math.ceil(rowVisW / GC_CHUNK) : 1;
    const blocks = Array.from({ length: nBlocks }, () => []);
    for (let i = 0; i < count; i++) {
      blocks[Math.min(nBlocks - 1, Math.floor(colVis(i, r) / GC_CHUNK))].push(i);
    }
    for (let b = 0; b < nBlocks; b++) {
      const cols = blocks[b];
      if (!cols.length) continue;
      const bvs = b * GC_CHUNK;
      const bve = Math.min(bvs + GC_CHUNK, rowVisW);
      if (visibleCols && (bve < visibleCols.start || bvs > visibleCols.end)) continue;
      const gid = `${idPrefix}-gc-${r}-${b}`;
      const x0 = getX(bvs);
      const x1 = getX(bve);
      const stops = cols.map((i) => (
        <stop
          key={i}
          offset={(colVis(i, r) + 0.5 - bvs) / (bve - bvs)}
          stopColor={gcContentColor(fracs[rowStart + i])}
        />
      ));
      rows.push(
        <g key={`${r}-${b}`}>
          <defs>
            <linearGradient id={gid} gradientUnits="userSpaceOnUse" x1={x0} y1={0} x2={x1} y2={0}>
              {stops}
            </linearGradient>
          </defs>
          {colRuns(cols[0], cols[cols.length - 1], r).map(([visStart, len]) => (
            <rect
              key={visStart}
              x={getX(visStart)}
              y={y}
              width={len * cw}
              height={GC_TRACK_H}
              fill={`url(#${gid})`}
            />
          ))}
        </g>,
      );
    }
    // Border once per row (not per block) so chunked gradients show no seams.
    rows.push(
      <g key={`${r}-border`}>
        {colRuns(0, count - 1, r).map(([visStart, len]) => (
          <rect
            key={visStart}
            x={getX(visStart)}
            y={y}
            width={len * cw}
            height={GC_TRACK_H}
            fill="none"
            stroke="#000000"
            strokeWidth="1"
          />
        ))}
      </g>,
    );
  }
  return rows;
}
