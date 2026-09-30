import { useMemo } from 'react';
import { enzLabelW, getX, measureWidth, monoFont } from '../editorConstants';
import { splitEnzName } from './seqUtils';

// Enzyme cut geometry: one layout entry per cut pair (cut-twice enzymes get 2
// entries), the batched vertical line path, and name → cut-pair counts for the
// ² badge.
export default function useEnzymeGeometry({
  visibleEnzymes,
  enzymeRowTracks,
  lp,
  charsPerLine,
  getSeqY,
  colVis,
  rowOf,
  rowStarts,
  enzymes,
}) {
  // Pre-compute enzyme geometry — one entry per cut pair (cut-twice enzymes get 2 entries)
  const enzymeLayout = useMemo(() => {
    const entries = [];
    for (const e of visibleEnzymes) {
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      pairs.forEach((cp, pi) => {
        const row = rowOf(cp.topCutIndex);
        const cutX = getX(colVis(cp.topCutIndex - rowStarts[row], row));
        const sy = getSeqY(row);
        const enzLift =
          (enzymeRowTracks[pairs.length > 1 ? `${e.id}_p${pi}` : e.id] || {})[row] || 0;

        const enzW = enzLabelW(e.name, e.isUnique);
        // Pre-compute exact label width (italic + normal parts)
        const s = splitEnzName(e.name);
        const baseFont = `${e.isUnique ? '700' : '350'} 14px ${monoFont}`;
        let exactW;
        if (s.normal) {
          exactW = measureWidth(s.italic, `italic ${baseFont}`) + measureWidth(s.normal, baseFont);
        } else {
          exactW = measureWidth(e.name, baseFont);
        }

        // Clamp label top so it never overlaps the sequence text of the row above
        const minTop = row > 0 ? getSeqY(row - 1) + 8 : -Infinity;
        const yTop = Math.max(sy - lp.enzLabelBase - enzLift, minTop);
        entries.push({
          id: `${e.id}_p${pi}`,
          groupId: e.id,
          pairIndex: pi,
          name: e.name,
          cutX,
          sy,
          row,
          yTop,
          isUnique: e.isUnique,
          enzW,
          exactW,
          topCutIndex: cp.topCutIndex,
          botCutIndex: cp.botCutIndex,
        });
      });
    }
    return entries;
  }, [visibleEnzymes, enzymeRowTracks, lp, charsPerLine, getSeqY, colVis, rowOf, rowStarts]);

  // Batched enzyme lines
  const enzymeLinesPath = useMemo(() => {
    let d = '';
    for (const l of enzymeLayout) {
      const yBot = l.sy - lp.enzLineGap;
      d += `M${l.cutX} ${l.yTop} L${l.cutX} ${yBot}`;
    }
    return d;
  }, [enzymeLayout, lp.enzLineGap]);

  const totalNameCounts = useMemo(() => {
    const m = new Map();
    for (const e of enzymes) {
      const nPairs = (e.cutPairs && e.cutPairs.length) || 1;
      m.set(e.name, (m.get(e.name) || 0) + nPairs);
    }
    return m;
  }, [enzymes]);

  return { enzymeLayout, enzymeLinesPath, totalNameCounts };
}
