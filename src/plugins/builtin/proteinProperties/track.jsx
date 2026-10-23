import { useMemo } from 'react';
import { cw, getX } from '../../../editorConstants';
import { aaColor } from './scales';

// Same band metrics as the GC-content track so stacked track-plugin lanes
// have a uniform look.
const TRACK_H = 10;
const TRACK_GAP = 4;

// Factory for per-residue protein property tracks: one color lookup per
// residue (window = 1, no gradient). `scale` is an uppercase-letter → color
// table from scales.js; `legend` ([{ color, label }]) is shown as a
// non-interactive submenu next to the View-menu toggle.
export function makeProteinTrackPlugin({ id, name, description, menuLabel, scale, legend }) {
  const useLane = ({ cleanSeq, enabled }) =>
    useMemo(() => {
      if (!enabled || !cleanSeq.length) return null;
      const colors = new Array(cleanSeq.length);
      for (let i = 0; i < cleanSeq.length; i++) colors[i] = aaColor(scale, cleanSeq[i]);
      return { height: TRACK_H + TRACK_GAP, colors };
    }, [enabled, cleanSeq]);

  // Merge adjacent same-color template columns into spans; each span is
  // emitted through colRuns so insertion-slot columns break the band instead
  // of shifting it.
  const render = (ctx, lane) => {
    const colors = lane?.colors;
    if (!colors) return null;
    const {
      visibleRows,
      rowBuf,
      numRows,
      rowStarts,
      rowCounts,
      getSeqY,
      lp,
      colVis,
      colRuns,
      visibleCols,
      trackYOffset = 0,
    } = ctx;
    const vs = Math.max(0, visibleRows.start - rowBuf);
    const ve = Math.min(numRows - 1, visibleRows.end + rowBuf);
    const rows = [];
    for (let r = vs; r <= ve; r++) {
      const count = rowCounts[r];
      if (count <= 0) continue;
      const rowStart = rowStarts[r];
      const y = getSeqY(r) + lp.featBaseOffset - 6 + trackYOffset;
      const spans = [];
      let spanStart = 0;
      for (let i = 1; i <= count; i++) {
        if (i === count || colors[rowStart + i] !== colors[rowStart + spanStart]) {
          spans.push([spanStart, i - 1, colors[rowStart + spanStart]]);
          spanStart = i;
        }
      }
      rows.push(
        <g key={r}>
          {spans.flatMap(([cs, ce, color]) => {
            if (
              visibleCols &&
              (colVis(ce, r) < visibleCols.start || colVis(cs, r) > visibleCols.end)
            )
              return [];
            return colRuns(cs, ce, r).map(([visStart, len]) => (
              <rect
                key={visStart}
                x={getX(visStart)}
                y={y}
                width={len * cw}
                height={TRACK_H}
                fill={color}
              />
            ));
          })}
        </g>,
      );
    }
    return rows;
  };

  return {
    id,
    name,
    description,
    version: '1.0.0',
    dialogKey: null,
    proteinOnly: true,
    sidebarItems: [],
    featuresMenuItem: { label: menuLabel, legend },
    track: { useLane, render },
  };
}
