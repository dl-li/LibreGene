import React from 'react';
import { cw, getX } from '../../editorConstants';

export const SEARCH_HIGHLIGHT_COLOR = '#FFF200';

const SearchHighlightLayer = React.memo(function SearchHighlightLayer({
  matches,
  visibleRows,
  rowBuf,
  numRows,
  getSeqY,
  sp,
  colRuns,
  colFromVis,
  visibleCols,
  dsBottomY = 0,
}) {
  if (!matches.length) return null;
  const vs = Math.max(0, visibleRows.start - rowBuf);
  const ve = Math.min(numRows - 1, visibleRows.end + rowBuf);
  const vStart = visibleCols ? Math.max(0, visibleCols.start) : -1;
  const vEnd = visibleCols ? visibleCols.end : Infinity;
  const height = dsBottomY ? 19 + dsBottomY + (19 - 14 * 0.7) : 28;
  const rects = [];

  for (let i = 0; i < matches.length; i++) {
    const match = matches[i];
    for (const seg of sp(match.start, match.end)) {
      if (seg.row < vs || seg.row > ve) continue;
      let c0 = seg.colStart;
      let c1 = seg.colEnd;
      if (visibleCols && colFromVis) {
        c0 = Math.max(c0, colFromVis(vStart, seg.row));
        c1 = Math.min(c1, colFromVis(vEnd, seg.row));
      }
      if (c1 < c0) continue;
      for (const [visStart, len] of colRuns(c0, c1, seg.row)) {
        rects.push(
          <rect
            key={`search-${i}-${seg.row}-${c0}-${visStart}`}
            x={getX(visStart)}
            y={getSeqY(seg.row) - 19}
            width={len * cw}
            height={height}
            fill={SEARCH_HIGHLIGHT_COLOR}
            rx="1"
          />,
        );
      }
    }
  }

  return <g style={{ pointerEvents: 'none' }}>{rects}</g>;
});

export default SearchHighlightLayer;
