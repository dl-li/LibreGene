import React from 'react';
import { cw, monoFont, getX, complement } from '../../editorConstants';
import { DEFAULT_TEXT_FILL, splitByTextColor } from '../textColors';
import MonoRun from './MonoRun';

// Vertical geometry of the double-strand block, measured from the top
// strand's text baseline (getSeqY(row)): the ruler's horizontal line (between
// the strands), tick half-length (ticks cross the line, SnapGene-style),
// bottom strand baseline, and the total height the row must reserve below.
export const DS_RULER_Y = 10;
export const DS_TICK_HALF = 3;
export const DS_BOTTOM_Y = 28;
export const DS_BLOCK_H = 33;

// SnapGene-style ruler between the strands: one horizontal line across the
// row with a small tick per template column. No numbers. Insertion slots
// carry no template coordinate, so they get no tick (the line spans them).
export const RulerLayer = React.memo(function RulerLayer({
  visibleRows,
  rowBuf,
  numRows,
  rowCounts,
  getSeqY,
  colRuns,
  colFromVis,
  visibleCols,
}) {
  const vs = Math.max(0, visibleRows.start - rowBuf);
  const ve = Math.min(numRows - 1, visibleRows.end + rowBuf);
  const vStart = visibleCols ? Math.max(0, visibleCols.start) : -1;
  const vEnd = visibleCols ? visibleCols.end : Infinity;
  const rows = [];
  for (let r = vs; r <= ve; r++) {
    const count = rowCounts[r];
    if (!count) continue;
    let c0 = 0;
    let c1 = count - 1;
    if (visibleCols && colFromVis) {
      c0 = Math.min(colFromVis(vStart, r), count);
      c1 = Math.min(colFromVis(vEnd, r), count - 1);
    }
    if (c1 < c0) continue;
    const sy = getSeqY(r);
    const ry = sy + DS_RULER_Y;
    const ticks = [];
    for (const [visStart, runLen] of colRuns(c0, c1, r)) {
      for (let i = 0; i < runLen; i++) {
        ticks.push(`M ${getX(visStart + i) + cw / 2} ${ry - DS_TICK_HALF} v ${DS_TICK_HALF * 2}`);
      }
    }
    rows.push(
      <g key={r} style={{ userSelect: 'none', pointerEvents: 'none' }}>
        <line x1={getX(c0)} x2={getX(c1) + cw} y1={ry} y2={ry} stroke="#4b5563" strokeWidth="1.5" />
        <path d={ticks.join(' ')} stroke="#4b5563" strokeWidth="1.5" />
      </g>,
    );
  }
  return rows;
});

// Bottom strand: per-column complement of the top strand (not reversed), in
// the same color; insertion slots get the same '-' placeholder as the top.
export const ComplementLayer = React.memo(function ComplementLayer({
  visibleRows,
  rowBuf,
  numRows,
  insReserve,
  streamOf,
  visCpl,
  rowCounts,
  rowStarts,
  cleanSeq,
  getSeqY,
  colRuns,
  colFromVis,
  visibleCols,
  // Bottom-strand text colors (canonical 0-based inclusive runs).
  textColors,
}) {
  const vs = Math.max(0, visibleRows.start - rowBuf);
  const ve = Math.min(numRows - 1, visibleRows.end + rowBuf);
  const vStart = visibleCols ? Math.max(0, visibleCols.start) : -1;
  const vEnd = visibleCols ? visibleCols.end : Infinity;
  const dashByRow = new Map();
  if (insReserve.size > 0) {
    for (const [pos, n] of insReserve) {
      const cell0 = streamOf(pos) - n;
      for (let k = 0; k < n; k++) {
        const si = cell0 + k;
        const row = Math.floor(si / visCpl);
        if (row < vs || row > ve) continue;
        if (!dashByRow.has(row)) dashByRow.set(row, []);
        const vis = si % visCpl;
        if (vis < vStart || vis > vEnd) continue;
        dashByRow.get(row).push(vis);
      }
    }
  }
  const rows = [];
  for (let r = vs; r <= ve; r++) {
    const count = rowCounts[r];
    const rowStart = rowStarts[r];
    let c0 = 0;
    let c1 = count - 1;
    if (visibleCols && colFromVis && count > 0) {
      c0 = Math.min(colFromVis(vStart, r), count);
      c1 = Math.min(colFromVis(vEnd, r), count - 1);
    }
    const chunk = c1 >= c0 && count > 0 ? cleanSeq.substring(rowStart + c0, rowStart + c1 + 1) : '';
    const dashes = dashByRow.get(r) || [];
    if (!chunk && !dashes.length) continue;
    const sy = getSeqY(r) + DS_BOTTOM_Y;
    let cc = 0;
    const tspans = [];
    const hasColors = textColors && textColors.length > 0;
    for (const [visStart, runLen] of colRuns(c0, c1, r)) {
      for (let off = 0; off < runLen; off += 200) {
        const n = Math.min(200, runLen - off);
        const complementText = (s) => s.split('').map(complement).join('');
        if (hasColors) {
          for (const piece of splitByTextColor(rowStart + c0 + cc, n, textColors)) {
            const text = complementText(chunk.slice(cc + piece.off, cc + piece.off + piece.len));
            tspans.push(
              <MonoRun
                key={visStart + off + piece.off}
                visStart={visStart + off + piece.off}
                text={text}
                fill={piece.color || DEFAULT_TEXT_FILL}
              />,
            );
          }
        } else {
          const text = complementText(chunk.slice(cc, cc + n));
          tspans.push(
            <MonoRun
              key={visStart + off}
              visStart={visStart + off}
              text={text}
              fill={DEFAULT_TEXT_FILL}
            />,
          );
        }
        cc += n;
      }
    }
    rows.push(
      <text
        key={r}
        y={sy}
        fontFamily={monoFont}
        fontSize="14px"
        fontWeight="bold"
        style={{ userSelect: 'none', cursor: 'text' }}
      >
        {tspans}
        {dashes.map((vis) => (
          <tspan
            key={`ins-${vis}`}
            x={getX(vis) + cw / 2}
            textAnchor="middle"
            fill="#9CA3AF"
            fillOpacity={0.7}
          >
            -
          </tspan>
        ))}
      </text>,
    );
  }
  return rows;
});
