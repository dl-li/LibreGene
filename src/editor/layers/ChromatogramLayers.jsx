import React from 'react';
import { cw, getX } from '../../editorConstants';
import {
  TRACE_CHANNELS,
  traceRangeMax,
  buildTracePath,
  buildColumnAnchors,
} from '../../chromatogram';

// Chromatogram (ab1 trace) band geometry: band height and the gap between
// stacked bands / to the next block below the sequence.
export const CHROM_TRACK_H = 46;
export const CHROM_GAP = 4;

// Chromatogram bands: the project's own trace directly under the top
// strand (ab1 source files), and one warped trace band per alignment
// with loaded trace data, placed below the alignment text lanes. Trace
// samples are interpolated between peak anchors so every base's peak sits
// on its own column; read gaps (deletions) break the polyline.
// Band rendering ported from GenePad (https://github.com/GenePad),
// provided by the GenePad team / https://github.com/Masterchiefm.
const ChromatogramLayers = React.memo(function ChromatogramLayers({
  chromatogram,
  alignmentTracks,
  alignmentChromatograms,
  visibleRows,
  rowBuf,
  numRows,
  rowStarts,
  rowCounts,
  visCpl,
  streamOf,
  colVis,
  insReserve,
  lp,
  alignLaneInfo,
  getSeqY,
}) {
  const hasAlignChrom = alignmentTracks.some((al) => alignmentChromatograms[al.id]);
  if (!chromatogram && !hasAlignChrom) return null;
  const vs = Math.max(0, visibleRows.start - rowBuf);
  const ve = Math.min(numRows - 1, visibleRows.end + rowBuf);
  const bands = [];

  const renderBand = (key, chrom, anchors, y) => {
    if (anchors.length === 0) return;
    const peaks = chrom.peakLocations;
    // Min/max over ALL anchors: a circular read's join wrap makes the last
    // anchor's query index smaller than the first, so first/last alone can
    // yield an inverted (empty) sample range and the band vanishes.
    let p0 = Infinity;
    let p1 = -Infinity;
    for (const a of anchors) {
      const p = peaks[a.q] ?? 0;
      if (p < p0) p0 = p;
      if (p > p1) p1 = p;
    }
    p0 = Math.max(0, p0 - 14);
    p1 += 14;
    const maxVal = traceRangeMax(chrom, p0, p1);
    if (maxVal <= 0) return;
    const baseY = y + CHROM_TRACK_H - 4;
    const scaleY = (CHROM_TRACK_H - 8) / maxVal;
    bands.push(
      <g key={key}>
        <line
          x1={Math.min(...anchors.map((a) => a.x)) - cw / 2}
          x2={Math.max(...anchors.map((a) => a.x)) + cw / 2}
          y1={baseY}
          y2={baseY}
          stroke="#d6d3d1"
          strokeWidth="1"
        />
        {TRACE_CHANNELS.map(([base, channelKey, color]) => (
          <path
            key={base}
            d={buildTracePath(chrom, channelKey, anchors, baseY, scaleY)}
            fill="none"
            stroke={color}
            strokeWidth="1"
            strokeLinejoin="round"
          />
        ))}
      </g>,
    );
  };

  if (chromatogram) {
    const peakCount = chromatogram.peakLocations.length;
    for (let r = vs; r <= ve; r++) {
      const cols = rowCounts[r];
      if (cols === 0) continue;
      const rowStart = rowStarts[r];
      const rowEnd = Math.min(rowStart + cols, peakCount) - 1;
      if (rowEnd < rowStart) continue;
      const anchors = [];
      for (let pos = rowStart; pos <= rowEnd; pos++) {
        anchors.push({ x: getX(colVis(pos - rowStart, r)) + cw / 2, q: pos });
      }
      renderBand(
        `chrom-main-${r}`,
        chromatogram,
        anchors,
        getSeqY(r) + lp.featBaseOffset + alignLaneInfo.trackH,
      );
    }
  }

  alignmentTracks.forEach((al, ti) => {
    const chrom = alignmentChromatograms[al.id];
    if (!chrom) return;
    // Ordered anchor entries (hit columns + insertion bases). Every entry
    // maps to a stream cell: inserted bases fill the merged block's cells
    // left of their anchor column, exactly where the text lane renders them.
    const byRow = new Map();
    for (const e of buildColumnAnchors(al)) {
      const si = e.ins ? streamOf(e.col) - (insReserve.get(e.col) || 0) + e.k : streamOf(e.col);
      const row = Math.floor(si / visCpl);
      if (row < vs || row > ve) continue;
      if (!byRow.has(row)) byRow.set(row, []);
      byRow.get(row).push({ x: getX(si % visCpl) + cw / 2, q: e.q, brk: e.brk });
    }
    for (const [row, anchors] of byRow) {
      // Display-direction order: read order within a row runs with x for '+'
      // and against x for '-', except in the row where a circular read wraps
      // the origin — there plain read order jumps back across the row and the
      // polyline backtracks in a long diagonal. Sorting by display direction
      // is a no-op for ordinary rows and fixes the wrap row. `brk` still
      // breaks the line at read deletions (dashes) and segment jumps.
      anchors.sort((a, b) => (al.strand === '-' ? b.x - a.x : a.x - b.x));
      const lane = alignLaneInfo.chromPerRow[row]?.get(ti) ?? 0;
      const y =
        getSeqY(row) +
        lp.featBaseOffset +
        alignLaneInfo.trackH +
        alignLaneInfo.mainChromH +
        alignLaneInfo.counts[row] * lp.featTrackHeight +
        2 +
        lane * (CHROM_TRACK_H + CHROM_GAP);
      renderBand(`chrom-${al.id}-${row}`, chrom, anchors, y);
    }
  });

  if (!bands.length) return null;
  return <g style={{ pointerEvents: 'none' }}>{bands}</g>;
});

export default ChromatogramLayers;
