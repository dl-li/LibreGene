import { useMemo, useCallback } from 'react';
import { cw, getX, splitRange, measureWidth, enzLabelW, primerLabelW, featLabelW } from './editorConstants';

// Extract best binding site info from the new data model.
const best = (p) => {
  const bs = p.bindingSites?.[0];
  return {
    matchStart: bs?.matchStart ?? 0,
    matchEnd: bs?.matchEnd ?? 0,
    fivePrimeTail: bs?.fivePrimeTail || '',
    tailLen: (bs?.fivePrimeTail || '').length,
  };
};

export default function useEditorLayout({
  sequence, features, enzymes, primers,
  charsPerLine, baseSeqY, rPrimerBase, fPrimerBase,
  primerGap, primerExpand, tailYOffset,
  labelTextYFwd, labelTextYRev, featBottomPad,
}) {
  const sp = useCallback((s, e) => splitRange(s, e, charsPerLine), [charsPerLine]);
  const cleanSeq = sequence || '';
  const seqLen = cleanSeq.length;
  const numRows = Math.max(1, Math.ceil(seqLen / charsPerLine));

  const enzymesByRow = useMemo(() => {
    const map = {};
    for (let i = 0; i < enzymes.length; i++) {
      const e = enzymes[i];
      const r = Math.floor(e.cutIndex / charsPerLine);
      (map[r] || (map[r] = [])).push(e);
    }
    return map;
  }, [enzymes, charsPerLine]);

  const normFeatures = useMemo(() => (features || []).map(f => {
    if (f.segments && f.segments.length) return f;
    return { ...f, segments: [{ start: f.start, end: f.end }] };
  }), [features]);

  const { processedFeatures, featureRowTracks, primerTracks, primerFeatOffsets } = useMemo(() => {
    const resultFeatures = [];
    if (normFeatures.length > 0) {
      const sorted = [...normFeatures]
        .filter((f, i, arr) => {
          const fa = f.segments.flatMap(s => [s.start, s.end]);
          return arr.findIndex(x => {
            const xa = x.segments.flatMap(s => [s.start, s.end]);
            return fa.length === xa.length && fa.every((v, j) => v === xa[j]);
          }) === i;
        })
        .sort((a, b) => {
          const la = a.segments.reduce((s, seg) => s + seg.end - seg.start, 0);
          const lb = b.segments.reduce((s, seg) => s + seg.end - seg.start, 0);
          return lb - la || a.segments[0].start - b.segments[0].start;
        });
      for (const f of sorted) resultFeatures.push(f);
    }

    const fRowTracks = {};
    for (const f of resultFeatures) fRowTracks[f.id] = {};

    for (let r = 0; r < numRows; r++) {
      const rs = r * charsPerLine, re = (r + 1) * charsPerLine - 1;
      const rowFeats = resultFeatures.filter(f =>
        f.segments.some(seg => !(seg.end < rs || seg.start > re))
      );
      rowFeats.sort((a, b) => {
        const la = a.segments.reduce((s, seg) => s + seg.end - seg.start, 0);
        const lb = b.segments.reduce((s, seg) => s + seg.end - seg.start, 0);
        return lb - la || a.segments[0].start - b.segments[0].start;
      });

      const rowTracks = [];
      for (const f of rowFeats) {
        const rowSegs = f.segments.filter(seg => !(seg.end < rs || seg.start > re));
        const segStart = Math.min(...rowSegs.map(s => s.start));
        const segEnd = Math.max(...rowSegs.map(s => s.end));
        const labelCols = Math.ceil(featLabelW(f.name) / cw) + 2 + (f.strand && f.strand !== '.' ? 2 : 0);
        const isRev = f.strand === '-';
        const es = isRev ? segStart - 0.5 : Math.max(rs, segStart - labelCols) - 0.5;
        const ee = isRev ? segEnd + labelCols + 0.5 : segEnd + 0.5;

        let placed = false;
        for (let i = 0; i < rowTracks.length; i++) {
          if (!rowTracks[i].some(t => !(ee < t.start || es > t.end))) {
            rowTracks[i].push({ start: es, end: ee });
            fRowTracks[f.id][r] = i;
            placed = true;
            break;
          }
        }
        if (!placed) {
          rowTracks.push([{ start: es, end: ee }]);
          fRowTracks[f.id][r] = rowTracks.length - 1;
        }
      }
    }

    const pTracks = {};
    for (const type of ['fwd', 'rev']) {
      const ofType = (primers || []).filter(p => p.type === type);
      if (!ofType.length) continue;
      const sorted = [...ofType].sort((a, b) => {
        const la = (a.matchEnd - a.matchStart) + (best(a).tailLen);
        const lb = (b.matchEnd - b.matchStart) + (best(b).tailLen);
        return lb - la || a.matchStart - b.matchStart;
      });
      const isFwd = type === 'fwd';
      for (let r = 0; r < numRows; r++) {
        const rs = r * charsPerLine, re = (r + 1) * charsPerLine - 1;
        const rowTracks = [];
        for (const p of sorted) {
          const ml = best(p).tailLen;
          const vs = isFwd ? best(p).matchStart - ml : best(p).matchStart;
          const ve = isFwd ? best(p).matchEnd : best(p).matchEnd + ml;
          if (ve < rs || vs > re) continue;
          const segs = sp(best(p).matchStart, best(p).matchEnd);
          if (!segs.some(s => s.row === r)) continue;

          if (!pTracks[p.id]) pTracks[p.id] = {};

          let placed = false;
          for (let i = 0; i < rowTracks.length; i++) {
            if (!rowTracks[i].some(t => !(ve < t.start || vs > t.end))) {
              rowTracks[i].push({ start: vs, end: ve });
              pTracks[p.id][r] = i;
              placed = true;
              break;
            }
          }
          if (!placed) {
            rowTracks.push([{ start: vs, end: ve }]);
            pTracks[p.id][r] = rowTracks.length - 1;
          }
        }
      }
    }

    const pFeatOff = {};
    for (const p of (primers || []).filter(p => p.type === 'rev')) {
      pFeatOff[p.id] = {};
      const ml = best(p).tailLen;
      const segs = sp(best(p).matchStart, best(p).matchEnd);
      for (const seg of segs) {
        const r = seg.row;
        const rs = r * charsPerLine;
        const vs = best(p).matchStart, ve = best(p).matchEnd + ml;
        let maxFB = 0;
        for (const f of resultFeatures) {
          for (const fseg of f.segments) {
            const labelCols = Math.ceil(featLabelW(f.name) / cw) + 2 + (f.strand && f.strand !== '.' ? 2 : 0);
            const fIsRev = f.strand === '-';
            const fvs = fIsRev ? fseg.start : fseg.start - labelCols;
            const fve = fIsRev ? fseg.end + labelCols : fseg.end;
            if (!(fve < vs || fvs > ve)) {
              const ft = (fRowTracks[f.id] || {})[r];
              if (ft !== undefined) maxFB = Math.max(maxFB, 14 + ft * 18 + featBottomPad);
            }
          }
        }
        if (maxFB > 0) pFeatOff[p.id][r] = maxFB;
      }
    }

    return { processedFeatures: resultFeatures, featureRowTracks: fRowTracks, primerTracks: pTracks, primerFeatOffsets: pFeatOff };
  }, [normFeatures, primers, numRows, charsPerLine, sp, featBottomPad]);

  const enzymeTracks = useMemo(() => {
    const tracks = {};
    const ebr = enzymesByRow;
    const rows = Object.keys(ebr);
    for (let ri = 0; ri < rows.length; ri++) {
      const rowEnzymes = ebr[rows[ri]];
      const sorted = [...rowEnzymes].sort((a, b) => b.cutIndex - a.cutIndex || b.name.length - a.name.length);
      const occupied = [];
      for (let i = 0; i < sorted.length; i++) {
        const e = sorted[i];
        const cs = e.cutIndex % charsPerLine;
        const ce = cs + Math.ceil(measureWidth(e.name, '350 14px Cascadia Code') / cw) + 1;
        let t = 0;
        while (occupied.some(o => o.track === t && !(ce < o.cs || cs > o.ce))) t++;
        occupied.push({ track: t, cs, ce });
        tracks[e.id] = t;
      }
    }
    return tracks;
  }, [enzymesByRow, charsPerLine]);

  const { rowAbove, rowBelow } = useMemo(() => {
    const above = new Array(numRows).fill(0);
    const below = new Array(numRows).fill(0);

    for (let r = 0; r < numRows; r++) {
      const rs = r * charsPerLine, re = (r + 1) * charsPerLine - 1;
      let ae = 46, be = 28;

      const rowEnz = enzymesByRow[r];
      if (rowEnz && rowEnz.length) {
        let maxTrack = 0;
        let hasFwdOnRow = false;
        for (let i = 0; i < rowEnz.length; i++) {
          maxTrack = Math.max(maxTrack, enzymeTracks[rowEnz[i].id] || 0);
        }
        for (let i = 0; i < primers.length; i++) {
          const p = primers[i];
          if (p.type === 'fwd') {
            const ml = best(p).tailLen;
            if (!(best(p).matchEnd < rs || best(p).matchStart - ml > re)) { hasFwdOnRow = true; break; }
          }
        }
        const base = hasFwdOnRow ? 81 : 51;
        ae = Math.max(ae, base + maxTrack * 16);
      }

      for (let i = 0; i < primers.length; i++) {
        const p = primers[i];
        if (p.type !== 'fwd') continue;
        const ml = best(p).tailLen;
        const vs = best(p).matchStart - ml, ve = best(p).matchEnd;
        if (!(ve < rs || vs > re)) {
          const t = (primerTracks[p.id] || {})[r] || 0;
          const hasTail = ml > 0;
          const extraAbove = (hasTail ? tailYOffset : 0) + primerExpand + Math.abs(labelTextYFwd) + 2;
          ae = Math.max(ae, fPrimerBase + t * primerGap + extraAbove);
        }
      }

      let maxFeatTrack = -1;
      for (let i = 0; i < processedFeatures.length; i++) {
        const f = processedFeatures[i];
        for (let j = 0; j < f.segments.length; j++) {
          const seg = f.segments[j];
          if (!(seg.end < rs || seg.start > re)) {
            const t = (featureRowTracks[f.id] || {})[r];
            if (t !== undefined) maxFeatTrack = Math.max(maxFeatTrack, t);
          }
        }
      }
      const featBottom = maxFeatTrack >= 0 ? 14 + maxFeatTrack * 18 + 26 : 0;
      be = Math.max(be, featBottom);

      for (let i = 0; i < primers.length; i++) {
        const p = primers[i];
        if (p.type !== 'rev') continue;
        const ml = best(p).tailLen;
        const vs = best(p).matchStart, ve = best(p).matchEnd + ml;
        if (!(ve < rs || vs > re)) {
          const t = (primerTracks[p.id] || {})[r] || 0;
          const extra = (primerFeatOffsets[p.id] || {})[r] || 0;
          const hasTail = ml > 0;
          const extraBelow = (hasTail ? tailYOffset : 0) + primerExpand + labelTextYRev + 2;
          be = Math.max(be, rPrimerBase + t * primerGap + extraBelow + extra);
        }
      }

      above[r] = ae;
      below[r] = be;
    }

    return { rowAbove: above, rowBelow: below };
  }, [enzymesByRow, enzymeTracks, primers, primerTracks, primerFeatOffsets, processedFeatures, featureRowTracks, numRows, charsPerLine, rPrimerBase, fPrimerBase, primerGap, primerExpand, tailYOffset, labelTextYFwd, labelTextYRev]);

  const rowY = useMemo(() => {
    const y = [Math.max(baseSeqY, rowAbove[0] + 50)];
    for (let r = 0; r < numRows - 1; r++) {
      y.push(y[r] + Math.max(24, rowBelow[r] + rowAbove[r + 1] + 14));
    }
    return y;
  }, [rowAbove, rowBelow, numRows, baseSeqY]);

  const getSeqY = useCallback((row) => rowY[Math.min(row, rowY.length - 1)], [rowY]);

  const computeHighestY = useCallback((row, cutX, enzNameW) => {
    let hy = getSeqY(row) - 26;

    for (let i = 0; i < primers.length; i++) {
      const p = primers[i];
      if (p.type !== 'fwd') continue;
      const segs = sp(best(p).matchStart, best(p).matchEnd);
      for (let j = 0; j < segs.length; j++) {
        const seg = segs[j];
        if (seg.row !== row) continue;
        const ml = best(p).tailLen;
        const isTail = seg === segs[0];
        const nameW = primerLabelW(p.name);
        const nameX = (isTail && ml > 0) ? getX(seg.colStart - ml) : getX(seg.colStart) + cw / 2;
        if (cutX < nameX + nameW + 4 && cutX + enzNameW > nameX) {
          const to = ((primerTracks[p.id] || {})[row] || 0) * primerGap;
          const labelTop = (isTail && ml > 0) ? tailYOffset + Math.abs(labelTextYFwd) : Math.abs(labelTextYFwd);
          hy = Math.min(hy, getSeqY(row) - fPrimerBase - labelTop - to - 6);
        }
      }
    }

    return hy;
  }, [primers, primerTracks, charsPerLine, sp, getSeqY, fPrimerBase, primerGap, tailYOffset, labelTextYFwd]);

  return {
    cleanSeq, seqLen, numRows,
    enzymesByRow,
    processedFeatures, featureRowTracks,
    primerTracks, primerFeatOffsets,
    enzymeTracks,
    rowAbove, rowBelow, rowY, getSeqY,
    computeHighestY,
    sp,
  };
}
