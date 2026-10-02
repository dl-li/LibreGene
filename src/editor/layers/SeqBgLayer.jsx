import React from 'react';
import { cw, monoFont, getX } from '../../editorConstants';
import MonoRun from './MonoRun';

// Stable background: all sequence text in dark color — doesn't depend on selection
const SeqBgLayer = React.memo(function SeqBgLayer({
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
}) {
  const vs = Math.max(0, visibleRows.start - rowBuf);
  const ve = Math.min(numRows - 1, visibleRows.end + rowBuf);
  // '-' placeholders in the template row keep it column-aligned with the
  // read lane's inserted bases (GenePad renders the same dashes in the
  // reference row). Each slot cell belongs to its own stream row, so a wide
  // insertion block scatters its dashes across rows just like its bases.
  const dashByRow = new Map();
  if (insReserve.size > 0) {
    for (const [pos, n] of insReserve) {
      const cell0 = streamOf(pos) - n;
      for (let k = 0; k < n; k++) {
        const si = cell0 + k;
        const row = Math.floor(si / visCpl);
        if (row < vs || row > ve) continue;
        if (!dashByRow.has(row)) dashByRow.set(row, []);
        dashByRow.get(row).push(si % visCpl);
      }
    }
  }
  const rows = [];
  for (let r = vs; r <= ve; r++) {
    const count = rowCounts[r];
    const rowStart = rowStarts[r];
    const chunk = count > 0 ? cleanSeq.substring(rowStart, rowStart + count) : '';
    const dashes = dashByRow.get(r) || [];
    if (!chunk && !dashes.length) continue;
    const sy = getSeqY(r);
    // Contiguous visual runs (split at insertion slots), one tspan per run —
    // the per-character x list keeps every glyph centred on its cell.
    let cc = 0;
    const tspans = colRuns(0, count - 1, r).map(([visStart, runLen]) => {
      const text = chunk.slice(cc, cc + runLen);
      cc += runLen;
      return <MonoRun key={visStart} visStart={visStart} text={text} fill="#1f2937" />;
    });
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
            className="ins-dash"
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

export default SeqBgLayer;
