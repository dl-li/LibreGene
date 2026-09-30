import React from 'react';
import { cw, monoFont, getX } from '../../editorConstants';
import { INSERT_DASH_HIDE_MAX } from '../alignmentLayout';

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
  colVis,
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
      if (n <= INSERT_DASH_HIDE_MAX) continue;
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
    rows.push(
      <text
        key={r}
        y={sy}
        fontFamily={monoFont}
        fontSize="14px"
        fontWeight="bold"
        style={{ userSelect: 'none', cursor: 'text' }}
      >
        {chunk.split('').map((c, i) => (
          <tspan key={i} x={getX(colVis(i, r)) + cw / 2} textAnchor="middle" fill="#1f2937">
            {c}
          </tspan>
        ))}
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
