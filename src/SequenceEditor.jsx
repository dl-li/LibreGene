import React, { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { cw, startX, baseSeqY, bgColor, monoFont, sansFont, springAnim, getX, complement, measureWidth, enzLabelW, primerLabelW, splitRange } from './editorConstants';

const complementStr = (s) => s.split('').map(c => complement(c)).join('');
const reverseComplement = (s) => complementStr(s).split('').reverse().join('');

const splitEnzName = (name) => {
  // Italic: everything before the first digit or uppercase letter (beyond position 0)
  let at = name.length;
  for (let i = 1; i < name.length; i++) {
    const c = name[i];
    if ((c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9')) { at = i; break; }
  }
  return { italic: name.slice(0, at), normal: name.slice(at) };
};


// sRGB → relative luminance (WCAG 2.1)
const _hexToRgb = (h) => [parseInt(h.slice(1,3),16)/255, parseInt(h.slice(3,5),16)/255, parseInt(h.slice(5,7),16)/255];
const _linearize = (c) => c <= 0.04045 ? c/12.92 : ((c+0.055)/1.055)**2.4;
const relLuminance = (r, g, b) => 0.2126*_linearize(r) + 0.7152*_linearize(g) + 0.0722*_linearize(b);
const _rgbToHsl = (r, g, b) => { const M=Math.max(r,g,b), m=Math.min(r,g,b), d=M-m, l=(M+m)/2; if(!d) return [0,0,l]; const s=l>.5?d/(2-M-m):d/(M+m); let h; if(M===r) h=((g-b)/d+(g<b?6:0))/6; else if(M===g) h=((b-r)/d+2)/6; else h=((r-g)/d+4)/6; return [h,s,l]; };
const _hslToRgb = (h, s, l) => { if(!s) return [l,l,l]; const q=l<.5?l*(1+s):l+s-l*s, p=2*l-q; const hue2rgb=(t)=>{if(t<0)t++;if(t>1)t--;if(t<1/6)return p+(q-p)*6*t;if(t<1/2)return q;if(t<2/3)return p+(q-p)*(2/3-t)*6;return p;}; return [hue2rgb(h+1/3),hue2rgb(h),hue2rgb(h-1/3)]; };
const _rgbToHex = (r, g, b) => '#' + [r,g,b].map(c => Math.round(c*255).toString(16).padStart(2,'0')).join('');

const ensureReadableColor = (hex, bgHex = '#fdfbf7') => {
  const [r, g, b] = _hexToRgb(hex);
  const [br, bg, bb] = _hexToRgb(bgHex);
  const bgLum = relLuminance(br, bg, bb);
  const lum = relLuminance(r, g, b);
  const contrast = (bgLum + 0.05) / (lum + 0.05);
  if (contrast >= 2.0) return hex;
  const [h, s, l] = _rgbToHsl(r, g, b);
  // Gently darken: cap total reduction at 0.18, small steps
  const minL = Math.max(0.1, l - 0.18);
  let newL = l;
  while (newL > minL) {
    newL = Math.max(minL, newL - 0.02);
    const [nr, ng, nb] = _hslToRgb(h, Math.min(1, s + 0.02), newL);
    if ((bgLum + 0.05) / (relLuminance(nr, ng, nb) + 0.05) >= 2.0) return _rgbToHex(nr, ng, nb);
  }
  return _rgbToHex(..._hslToRgb(h, Math.min(1, s + 0.04), minL));
};

export default function SequenceEditor({ sequence, features = [], enzymes = [], primers = [], initialCharsPerLine = 60, primerParams }) {
  const containerRef = useRef(null);
  const [charsPerLine, setCharsPerLine] = useState(initialCharsPerLine);
  const [hoveredFeature, setHoveredFeature] = useState(null);
  const [hoveredPrimer, setHoveredPrimer] = useState(null);
  const [hoveredEnzyme, setHoveredEnzyme] = useState(null);
  const [scrollY, setScrollY] = useState(0);
  const scrollTickingRef = useRef(false);

  const pp = useMemo(() => ({
    fwdMatchY: 30, revMatchY: 26, misYDelta: 4,
    fwdBaseTextY: 8, revBaseTextY: 18,
    fwdLabelY: 8, revLabelY: 20,
    trackGap: 36,
    fwdAboveBase: 30, fwdAboveExtra: 23, fwdAboveNonTailExtra: 5,
    revBelowBase: 26, revBelowExtra: 25, revBelowNonTailExtra: 5,
    hoverExpand: 26, arrowHeadLen: 7, arrowHeadHeight: 5,
    highestYBase: 26, highestYFwdTail: 42, highestYFwdNoTail: 38, highestYExtra: 24,
    ...primerParams,
  }), [primerParams]);

  useEffect(() => {
    const handleResize = () => {
      if (containerRef.current) {
        setCharsPerLine(Math.max(20, Math.floor((containerRef.current.clientWidth - startX * 2) / cw)));
      }
    };
    const handleScroll = () => {
      if (!scrollTickingRef.current) {
        scrollTickingRef.current = true;
        requestAnimationFrame(() => { setScrollY(window.scrollY); scrollTickingRef.current = false; });
      }
    };
    handleResize();
    window.addEventListener('resize', handleResize);
    window.addEventListener('scroll', handleScroll, { passive: true });
    return () => {
      window.removeEventListener('resize', handleResize);
      window.removeEventListener('scroll', handleScroll);
    };
  }, []);

  const cleanSeq = sequence || '';

  // Enrich primers with flat fields from new nested bindingSites data model.
  const enrichedPrimers = useMemo(() => (primers || []).map(p => {
    if (p.matchStart !== undefined && p.matchEnd !== undefined) return p;
    const bs = p.bindingSites?.[0];
    if (!bs) return p;
    const ms = bs.matchStart, me = bs.matchEnd;
    return {
      ...p,
      matchStart: ms,
      matchEnd: me,
      mismatchStr: bs.fivePrimeTail || '',
      matchStr: p.type === 'fwd'
        ? cleanSeq.substring(ms, me + 1)
        : complementStr(cleanSeq.substring(ms, me + 1)),
    };
  }), [primers, cleanSeq]);

  const numRows = Math.max(1, Math.ceil(cleanSeq.length / charsPerLine));
  const svgWidth = startX + charsPerLine * cw + startX;

  const sp = useCallback((s, e) => splitRange(s, e, charsPerLine), [charsPerLine]);

  // --- collision avoidance: features + primers ---
  // Normalize features to always have a segments array
  const normFeatures = useMemo(() => (features || []).map(f => {
    const isRepeat = /repeat/i.test(f.ftype || '');
    const fixColor = (c) => (c && !isRepeat) ? ensureReadableColor(c) : c;
    const fixed = {
      ...f,
      color: fixColor(f.color),
      segments: (f.segments && f.segments.length ? f.segments : [{ start: f.start, end: f.end }]).map(seg => ({
        ...seg,
        color: fixColor(seg.color),
      })),
    };
    return fixed;
  }), [features]);

  const { processedFeatures, primerTracks, featureRowTracks, revPrimerFeatOffsets } = useMemo(() => {
    const resultFeatures = [];
    const bottomTracks = [];

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
      for (const f of sorted) {
        const allStarts = f.segments.map(s => s.start);
        const allEnds = f.segments.map(s => s.end);
        const es = Math.min(...allStarts) - 0.5, ee = Math.max(...allEnds) + 0.5;
        let placed = false;
        for (let i = 0; i < bottomTracks.length; i++) {
          if (!bottomTracks[i].some(t => !(ee < t.start || es > t.end))) {
            bottomTracks[i].push({ start: es, end: ee });
            resultFeatures.push({ ...f, trackIdx: i });
            placed = true;
            break;
          }
        }
        if (!placed) {
          bottomTracks.push([{ start: es, end: ee }]);
          resultFeatures.push({ ...f, trackIdx: bottomTracks.length - 1 });
        }
      }
    }

    // Per-row primer track assignment — primers on different rows can share tracks.
    const pTracks = {}; // { [primerId]: { [row]: trackIndex } }
    for (const type of ['fwd', 'rev']) {
      const ofType = (enrichedPrimers || []).filter(p => p.type === type);
      if (!ofType.length) continue;
      const sorted = [...ofType].sort((a, b) => {
        const la = (a.matchEnd - a.matchStart) + (a.mismatchStr?.length || 0);
        const lb = (b.matchEnd - b.matchStart) + (b.mismatchStr?.length || 0);
        return lb - la || a.matchStart - b.matchStart;
      });
      for (let r = 0; r < numRows; r++) {
        const rs = r * charsPerLine, re = (r + 1) * charsPerLine - 1;
        const rowTracks = [];
        for (const p of sorted) {
          const ml = p.mismatchStr?.length || 0;
          const vs = p.type === 'fwd' ? p.matchStart - ml : p.matchStart;
          const ve = p.type === 'rev' ? p.matchEnd + ml : p.matchEnd;
          if (ve < rs || vs > re) continue;
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

    // Per-row feature track assignment — features only reserve space where they actually overlap
    const fRowTracks = {};
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
        const isFRev = f.strand === '-';
        const labelCols = Math.ceil(primerLabelW(f.name) / cw) + 2 + (f.strand && f.strand !== '.' ? 2 : 0);
        const es = isFRev ? segStart - 0.5 : Math.max(rs, segStart - labelCols) - 0.5;
        const ee = isFRev ? segEnd + labelCols + 0.5 : segEnd + 0.5;
        let placed = false;
        for (let i = 0; i < rowTracks.length; i++) {
          if (!rowTracks[i].some(t => !(ee < t.start || es > t.end))) {
            rowTracks[i].push({ start: es, end: ee });
            if (!fRowTracks[f.id]) fRowTracks[f.id] = {};
            fRowTracks[f.id][r] = i;
            placed = true;
            break;
          }
        }
        if (!placed) {
          rowTracks.push([{ start: es, end: ee }]);
          if (!fRowTracks[f.id]) fRowTracks[f.id] = {};
          fRowTracks[f.id][r] = rowTracks.length - 1;
        }
      }
    }

    // Rev primer offset when overlapping with features on the same row
    const revFeatOff = {};
    for (const p of (enrichedPrimers || []).filter(p => p.type === 'rev')) {
      if (p.matchStart === undefined) continue;
      const ml = p.mismatchStr?.length || 0;
      const vs = p.matchStart, ve = p.matchEnd + ml;
      revFeatOff[p.id] = {};
      for (const f of resultFeatures) {
        for (const fseg of f.segments) {
          const isFRev = f.strand === '-';
          const labelCols = Math.ceil(primerLabelW(f.name) / cw) + 2 + (f.strand && f.strand !== '.' ? 2 : 0);
          const fvs = isFRev ? fseg.start : fseg.start - labelCols;
          const fve = isFRev ? fseg.end + labelCols : fseg.end;
          if (fve < vs || fvs > ve) continue;
          const sr = Math.floor(fseg.start / charsPerLine);
          const er = Math.floor(fseg.end / charsPerLine);
          for (let r = sr; r <= er; r++) {
            const ft = (fRowTracks[f.id] || {})[r] || 0;
            revFeatOff[p.id][r] = Math.max(revFeatOff[p.id][r] || 0, (ft + 1) * 18);
          }
        }
      }
    }

    return { processedFeatures: resultFeatures, primerTracks: pTracks, featureRowTracks: fRowTracks, revPrimerFeatOffsets: revFeatOff };
  }, [features, enrichedPrimers, numRows, charsPerLine]);

  // --- adaptive row spacing (memoized with pre-indexed lookups) ---
  const enzymesByRow = useMemo(() => {
    const map = {};
    for (const e of enzymes) {
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      const rows = new Set();
      for (const cp of pairs) {
        rows.add(Math.floor(cp.topCutIndex / charsPerLine));
      }
      for (const r of rows) {
        (map[r] || (map[r] = [])).push(e);
      }
    }
    return map;
  }, [enzymes, charsPerLine]);

  const primersByRow = useMemo(() => {
    const map = {};
    for (const p of (enrichedPrimers || [])) {
      if (p.matchStart === undefined || p.matchEnd === undefined) continue;
      // Only include rows that actually render primer segments (the match range).
      // Tail characters beyond the match segment's row are truncated by rendering.
      const sr = Math.floor(p.matchStart / charsPerLine);
      const er = Math.floor(p.matchEnd / charsPerLine);
      for (let r = sr; r <= er; r++) {
        (map[r] || (map[r] = [])).push(p);
      }
    }
    return map;
  }, [enrichedPrimers, charsPerLine]);

  const featuresByRow = useMemo(() => {
    const map = {};
    for (const f of processedFeatures) {
      for (const seg of f.segments) {
        const sr = Math.floor(seg.start / charsPerLine);
        const er = Math.floor(seg.end / charsPerLine);
        for (let r = sr; r <= er; r++) {
          (map[r] || (map[r] = [])).push({ feature: f, seg });
        }
      }
    }
    return map;
  }, [processedFeatures, charsPerLine]);

  const { rowAbove, rowBelow } = useMemo(() => {
    const above = new Array(numRows).fill(0);
    const below = new Array(numRows).fill(0);

    for (let r = 0; r < numRows; r++) {
      let ae = 12, be = 14;

      const rowEnz = enzymesByRow[r];
      if (rowEnz) {
        const sorted = [...rowEnz].sort((a, b) => b.cutIndex - a.cutIndex || b.name.length - a.name.length);
        const occupied = [];
        let maxEnzTrack = 0;
        for (const e of sorted) {
          const cs = e.cutIndex % charsPerLine;
          const ce = cs + Math.ceil(enzLabelW(e.name, e.isUnique) / cw);
          let t = 0;
          while (occupied.some(o => o.track === t && !(ce < o.cs || cs > o.ce))) t++;
          occupied.push({ track: t, cs, ce });
          maxEnzTrack = Math.max(maxEnzTrack, t);
        }
        // Label top = getSeqY(row) - 61 - track*18
        ae = Math.max(ae, 61 + maxEnzTrack * 18);
      }

      const rowPrimers = primersByRow[r];
      if (rowPrimers) {
        for (const p of rowPrimers) {
          const t = (primerTracks[p.id] || {})[r] || 0;
          if (p.type === 'fwd') {
            const hasTail = r === Math.floor(p.matchStart / charsPerLine);
            const extra = hasTail ? pp.fwdAboveExtra : pp.fwdAboveNonTailExtra;
            ae = Math.max(ae, pp.fwdAboveBase + t * pp.trackGap + extra);
          } else {
            const hasTail = r === Math.floor(p.matchEnd / charsPerLine);
            const extra = hasTail ? pp.revBelowExtra : pp.revBelowNonTailExtra;
            const featOff = (revPrimerFeatOffsets[p.id] || {})[r] || 0;
            be = Math.max(be, pp.revBelowBase + t * pp.trackGap + extra + featOff);
          }
        }
      }

      const rowFeats = featuresByRow[r];
      if (rowFeats) {
        for (const { feature: f } of rowFeats) {
          const t = (featureRowTracks[f.id] || {})[r] || 0;
          be = Math.max(be, 14 + t * 18 + 12);
        }
      }

      above[r] = ae;
      below[r] = be;
    }

    return { rowAbove: above, rowBelow: below };
  }, [numRows, enzymesByRow, primersByRow, featuresByRow, primerTracks, featureRowTracks, revPrimerFeatOffsets, charsPerLine, pp]);

  const rowY = useMemo(() => {
    const y = [Math.max(baseSeqY, rowAbove[0] + 18)];
    for (let r = 0; r < numRows - 1; r++) {
      y.push(y[r] + Math.max(18, rowBelow[r] + rowAbove[r + 1] + 12));
    }
    return y;
  }, [numRows, rowAbove, rowBelow]);
  const getSeqY = useCallback((row) => rowY[Math.min(row, rowY.length - 1)], [rowY]);

  // Visible row range for enzyme virtualization
  const visibleRows = useMemo(() => {
    if (!rowY.length) return { start: 0, end: numRows - 1 };
    const vh = window.innerHeight || 900;
    const top = scrollY;
    const bot = top + vh;
    let start = 0, end = numRows - 1;
    for (let r = 0; r < rowY.length; r++) {
      if (rowY[r] + (rowBelow[r] || 0) > top) { start = Math.max(0, r); break; }
    }
    for (let r = rowY.length - 1; r >= 0; r--) {
      if (rowY[r] - (rowAbove[r] || 0) < bot) { end = Math.min(numRows - 1, r + 1); break; }
    }
    return { start, end };
  }, [rowY, rowAbove, rowBelow, scrollY, numRows]);

  // Filter enzymes to visible row range only
  const visibleEnzymes = useMemo(() => {
    if (!enzymes || !enzymes.length) return [];
    return enzymes.filter(e => {
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      return pairs.some(cp => {
        const r = Math.floor(cp.topCutIndex / charsPerLine);
        return r >= visibleRows.start && r <= visibleRows.end;
      });
    });
  }, [enzymes, visibleRows, charsPerLine]);
  const svgHeight = rowY[rowY.length - 1] + Math.max(40, rowBelow[numRows - 1] + 24);

  const ROW_BUF = 8; // rows above/below viewport to pre-render

  // Virtualize features: only render those overlapping visible rows
  const visibleFeatures = useMemo(() => {
    if (!processedFeatures.length) return [];
    const vs = Math.max(0, visibleRows.start - ROW_BUF);
    const ve = Math.min(numRows - 1, visibleRows.end + ROW_BUF);
    return processedFeatures.filter(f =>
      f.segments.some(seg => {
        const sr = Math.floor(seg.start / charsPerLine);
        const er = Math.floor(seg.end / charsPerLine);
        return !(er < vs || sr > ve);
      })
    );
  }, [processedFeatures, visibleRows, numRows, charsPerLine]);

  // Virtualize primers: only render those overlapping visible rows
  const visiblePrimers = useMemo(() => {
    if (!enrichedPrimers.length) return [];
    const vs = Math.max(0, visibleRows.start - ROW_BUF);
    const ve = Math.min(numRows - 1, visibleRows.end + ROW_BUF);
    return enrichedPrimers.filter(p => {
      if (p.matchStart === undefined || p.matchEnd === undefined) return false;
      const sr = Math.floor(p.matchStart / charsPerLine);
      const er = Math.floor(p.matchEnd / charsPerLine);
      const ml = (p.mismatchStr?.length || 0);
      return !(er < vs - Math.ceil(ml / charsPerLine) || sr > ve + Math.ceil(ml / charsPerLine));
    });
  }, [enrichedPrimers, visibleRows, numRows, charsPerLine]);

  // --- shared: highest obstacle Y for enzyme label ---
  const computeHighestY = useCallback((row, cutX, enzNameW) => {
    let hy = getSeqY(row) - pp.highestYBase;
    const rowPrimers = primersByRow[row];
    if (rowPrimers) {
      for (const p of rowPrimers) {
        if (p.type !== 'fwd') continue;
        const segs = sp(p.matchStart, p.matchEnd);
        for (const seg of segs) {
          if (seg.row !== row) continue;
          const ml = p.mismatchStr?.length || 0;
          const isTail = seg === segs[0];
          const drawMisLen = isTail ? Math.min(ml, seg.colStart + 5) : 0;
          const nameW = primerLabelW(p.name);
          const nameX = (isTail && drawMisLen > 0) ? getX(seg.colStart - drawMisLen) : getX(seg.colStart) + cw / 2;
          if (cutX < nameX + nameW + 4 && cutX + enzNameW > nameX) {
            const to = ((primerTracks[p.id] || {})[row] || 0) * pp.trackGap;
            hy = Math.min(hy, (isTail && ml > 0 ? getSeqY(row) - pp.highestYFwdTail - to : getSeqY(row) - pp.highestYFwdNoTail - to) - pp.highestYExtra);
          }
        }
      }
    }
    return hy;
  }, [primersByRow, primerTracks, charsPerLine, sp, getSeqY, pp]);

  // --- enzyme track assignment ---
  // Group enzymes by their first cut pair's row (cutIndex row) to compute tracks.
  const enzymeTracks = useMemo(() => {
    const byRow = {};
    for (const e of enzymes) {
      const r = Math.floor(e.cutIndex / charsPerLine);
      (byRow[r] || (byRow[r] = [])).push(e);
    }
    const tracks = {};
    for (const rowEnz of Object.values(byRow)) {
      const sorted = [...rowEnz].sort((a, b) => b.cutIndex - a.cutIndex || b.name.length - a.name.length);
      const occupied = [];
      for (const e of sorted) {
        const cs = e.cutIndex % charsPerLine;
        const ce = cs + Math.ceil(enzLabelW(e.name, e.isUnique) / cw);
        let t = 0;
        while (occupied.some(o => o.track === t && !(ce < o.cs || cs > o.ce))) t++;
        occupied.push({ track: t, cs, ce });
        tracks[e.id] = t;
      }
    }
    return tracks;
  }, [enzymes, charsPerLine]);

  // --- render helpers ---
  const computeDominantColor = (f) => {
    const colorLen = {};
    for (const seg of f.segments) {
      const c = seg.color || f.color || ensureReadableColor('#60A5FA');
      colorLen[c] = (colorLen[c] || 0) + (seg.end - seg.start + 1);
    }
    let best = f.color || ensureReadableColor('#60A5FA'), bestLen = 0;
    for (const [c, len] of Object.entries(colorLen)) {
      if (len > bestLen) { best = c; bestLen = len; }
    }
    return best;
  };

  const renderFeatures = () => {
    if (!visibleFeatures.length) return null;
    return visibleFeatures.map(f => {
      const isHovered = hoveredFeature === f.id;
      const dataSegs = f.segments;

      // Build ordered visual elements: real segments + gap segments between them
      const visuals = []; // { type: 'solid'|'gap', row, colStart, colEnd, showLabel }
      const seenRows = new Set();
      for (let di = 0; di < dataSegs.length; di++) {
        const ds = dataSegs[di];

        // Gap between previous data segment and this one
        if (di > 0) {
          const prevEnd = dataSegs[di - 1].end;
          if (ds.start > prevEnd + 1) {
            for (const vs of sp(prevEnd + 1, ds.start - 1)) {
              const showL = !seenRows.has(vs.row);
              seenRows.add(vs.row);
              visuals.push({ type: 'gap', row: vs.row, colStart: vs.colStart, colEnd: vs.colEnd, showLabel: showL, color: f.color || ensureReadableColor('#60A5FA') });
            }
          }
        }

        // Solid segment
        for (const vs of sp(ds.start, ds.end)) {
          const showLabel = !seenRows.has(vs.row);
          seenRows.add(vs.row);
          const segColor = dataSegs[di].color || f.color || ensureReadableColor('#60A5FA');
          visuals.push({ type: 'solid', row: vs.row, colStart: vs.colStart, colEnd: vs.colEnd, showLabel, color: segColor });
        }
      }

      // Sort by absolute index
      visuals.sort((a, b) => (a.row * charsPerLine + a.colStart) - (b.row * charsPerLine + b.colStart));

      if (!visuals.length) return null;

      return (
        <g key={f.id}>
          {visuals.map((v, vi) => {
            const x = getX(v.colStart);
            const w = (v.colEnd - v.colStart + 1) * cw;
            const sy = getSeqY(v.row);
            const rowTo = ((featureRowTracks[f.id] || {})[v.row] || 0) * 18;
            const y = sy + 14 + rowTo;
            const isGap = v.type === 'gap';

            return (
              <g key={`${v.type}-${v.row}-${v.colStart}`}
                onMouseEnter={() => setHoveredFeature(f.id)}
                onMouseLeave={() => setHoveredFeature(null)}
                className="cursor-pointer">
                <rect x={x} y={(isHovered && !isGap) ? sy - 18 : y} width={w}
                  height={(isHovered && !isGap) ? y - (sy - 18) : 0} fill={v.color}
                  fillOpacity={isHovered ? (isGap ? 0 : 0.15) : 0}
                  style={{ transition: springAnim, pointerEvents: 'none' }} />
                <line x1={x} x2={x + w} y1={y} y2={y} stroke={v.color}
                  strokeWidth="5" opacity={isGap ? 0.25 : 1} />
                <line x1={x} x2={x + w} y1={y} y2={y} stroke="transparent" strokeWidth="8" />
              </g>
            );
          })}
        </g>
      );
    });
  };

  // Feature labels rendered on top of all feature color blocks
  const renderFeatureLabels = () => {
    if (!visibleFeatures.length) return null;
    const seen = new Set();
    return visibleFeatures.flatMap(f => {
      const isRev = f.strand === '-';
      const isFwd = f.strand === '+';
      const labelColor = computeDominantColor(f);
      const labelText = isRev ? `< ${f.name}` : isFwd ? `${f.name} >` : f.name;

      // Per row: collect the label position.
      // Forward/unknown → keep the first visual; reverse → keep overwriting to get the last.
      const rowLabels = {};
      const seenRows = new Set();

      for (let di = 0; di < f.segments.length; di++) {
        const ds = f.segments[di];
        if (di > 0) {
          const prevEnd = f.segments[di - 1].end;
          if (ds.start > prevEnd + 1) {
            for (const vs of sp(prevEnd + 1, ds.start - 1)) {
              if (!seenRows.has(vs.row) || isRev) { seenRows.add(vs.row); rowLabels[vs.row] = vs; }
            }
          }
        }
        for (const vs of sp(ds.start, ds.end)) {
          if (!seenRows.has(vs.row) || isRev) { seenRows.add(vs.row); rowLabels[vs.row] = vs; }
        }
      }

      return Object.values(rowLabels).map(vs => {
        const key = `${f.id}-${vs.row}`;
        if (seen.has(key)) return null;
        seen.add(key);
        const sy = getSeqY(vs.row);
        const rowTo = ((featureRowTracks[f.id] || {})[vs.row] || 0) * 18;
        const y = sy + 14 + rowTo;
        const textProps = { y: y + 4, fontSize: "12px", fontFamily: "TeX Gyre Heros", fontWeight: "600" };
        if (isRev) {
          const xr = getX(vs.colEnd + 1);
          return (
            <g key={key}
              onMouseEnter={() => setHoveredFeature(f.id)}
              onMouseLeave={() => setHoveredFeature(null)}
              className="cursor-pointer">
              <text x={xr + 8} {...textProps} textAnchor="start" fill="none" stroke={bgColor} strokeWidth="5">{labelText}</text>
              <text x={xr + 8} {...textProps} textAnchor="start" fill={labelColor} stroke="none">{labelText}</text>
            </g>
          );
        }
        const x = getX(vs.colStart);
        return (
          <g key={key}
            onMouseEnter={() => setHoveredFeature(f.id)}
            onMouseLeave={() => setHoveredFeature(null)}
            className="cursor-pointer">
            <text x={x - 8} {...textProps} textAnchor="end" fill="none" stroke={bgColor} strokeWidth="5">{labelText}</text>
            <text x={x - 8} {...textProps} textAnchor="end" fill={labelColor} stroke="none">{labelText}</text>
          </g>
        );
      });
    });
  };

  const renderPrimers = () => {
    if (!visiblePrimers.length) return null;
    return visiblePrimers.map((p, idx) => {
      const isFwd = p.type === 'fwd';
      const isHovered = hoveredPrimer === p.id;
      const misLen = p.mismatchStr?.length || 0;
      const hasMis = misLen > 0;
      const pColor = p.color || '#166534';
      const segs = sp(p.matchStart, p.matchEnd);
      const tailSeg = isFwd ? segs[0] : segs[segs.length - 1];
      const arrowSeg = isFwd ? segs[segs.length - 1] : segs[0];

      let drawMisLen = misLen, showMisDots = false;
      if (hasMis) {
        if (isFwd) {
          const max = tailSeg.colStart + 5;
          if (misLen > max) { drawMisLen = max; showMisDots = true; }
        } else {
          const max = (charsPerLine - 1 - tailSeg.colEnd) + 5;
          if (misLen > max) { drawMisLen = max; showMisDots = true; }
        }
      }

      return (
        <g key={`${p.id}-${idx}`}>
          {segs.map(seg => {
            const isTail = seg === tailSeg, isArrow = seg === arrowSeg;
            const sy = getSeqY(seg.row);
            const featOff = isFwd ? 0 : ((revPrimerFeatOffsets[p.id] || {})[seg.row] || 0);
            const trackOff = ((primerTracks[p.id] || {})[seg.row] || 0) * pp.trackGap + featOff;
            const matchY = (isFwd ? sy - pp.fwdMatchY : sy + pp.revMatchY) + (isFwd ? -trackOff : trackOff);
            const misY = matchY + (isFwd ? -pp.misYDelta : pp.misYDelta);
            const x1 = getX(seg.colStart), x2 = getX(seg.colEnd);

            const pts = [];
            if (isTail && hasMis && drawMisLen > 0) {
              if (isFwd) pts.push([x1 - drawMisLen * cw, misY], [x1 - cw / 2, misY]);
              else pts.push([getX(seg.colEnd + drawMisLen + 1), misY], [x2 + cw * 1.5, misY]);
            }
            if (isFwd) pts.push([x1 + cw / 2, matchY], [x2 + cw, matchY]);
            else pts.push([x2 + cw, matchY], [x1, matchY]);
            if (pts.length < 2) return null;

            const pathStr = `M ${pts.map(p => `${p[0]} ${p[1]}`).join(' L ')}`;
            const expD = isFwd ? -1 : 1;
            const curExp = isHovered ? pp.hoverExpand : 0;
            const last = pts[pts.length - 1];
            const hoverPath = pathStr +
              ` L ${last[0]} ${last[1] + expD * curExp} ` +
              [...pts].reverse().map(p => `L ${p[0]} ${p[1] + expD * curExp}`).join(' ') + ' Z';

            const arrowPath = isArrow
              ? `M ${isFwd ? x2 + cw : x1} ${matchY} L ${isFwd ? x2 + cw - pp.arrowHeadLen : x1 + pp.arrowHeadLen} ${matchY + expD * pp.arrowHeadHeight}` : '';

            const visMis = hasMis && drawMisLen > 0 ? p.mismatchStr.slice(misLen - drawMisLen) : '';

            return (
              <g key={`${seg.row}-${seg.colStart}`}>
                <path d={hoverPath} fill={bgColor} style={{ transition: springAnim }} />
                <path d={hoverPath} fill={pColor} fillOpacity={0.1} style={{ transition: springAnim }} />

                <text fill={pColor} fontSize="14px" fontFamily={monoFont} fontWeight="bold"
                  style={{ opacity: isHovered ? 1 : 0, transition: 'opacity 0.2s ease-in-out', pointerEvents: 'none' }}>
                  {isTail && hasMis && drawMisLen > 0 && (
                    <>
                      {showMisDots && <tspan
                        x={isFwd ? getX(seg.colStart - drawMisLen - 1.5) : getX(seg.colEnd + drawMisLen + 2.5)}
                        y={misY + (isFwd ? -pp.fwdBaseTextY : pp.revBaseTextY)} textAnchor="middle">···</tspan>}
                      {visMis.split('').map((c, k) => (
                        <tspan key={`mis-${k}`}
                          x={(isFwd ? getX(seg.colStart - drawMisLen + k) : getX(seg.colEnd + drawMisLen - k)) + cw / 2}
                          y={misY + (isFwd ? -pp.fwdBaseTextY : pp.revBaseTextY)} textAnchor="middle">{c}</tspan>
                      ))}
                    </>
                  )}
                  {isFwd
                    ? p.matchStr?.substring(seg.strOffset, seg.strOffset + seg.len).split('').map((c, k) => (
                      <tspan key={`mat-${k}`} x={getX(seg.colStart + k) + cw / 2} y={matchY - pp.fwdBaseTextY} textAnchor="middle">{c}</tspan>
                    ))
                    : p.matchStr && Array.from({ length: seg.len }, (_, k) => {
                      const ci = seg.colEnd + seg.row * charsPerLine - k - p.matchStart;
                      return <tspan key={`mat-${k}`} x={getX(seg.colEnd - k) + cw / 2} y={matchY + pp.revBaseTextY} textAnchor="middle">
                        {p.matchStr[ci] || ''}</tspan>;
                    })
                  }
                </text>

                <path d={pathStr} fill="none" stroke={bgColor} strokeWidth="6" strokeLinejoin="round" />
                {isArrow && <path d={arrowPath} fill="none" stroke={bgColor} strokeWidth="6" strokeLinecap="round" strokeLinejoin="round" />}
                <path d={pathStr} fill="none" stroke={pColor} strokeWidth="2.5" />
                {isArrow && <path d={arrowPath} fill="none" stroke={pColor} strokeWidth="2.5" strokeLinecap="round" />}

                <text x={pts[0][0]} y={pts[0][1] + (isFwd ? -pp.fwdLabelY : pp.revLabelY)}
                  fontSize="12px" fontFamily="TeX Gyre Heros" fontWeight="600" fontStyle="italic"
                  textAnchor={isFwd ? 'start' : 'end'}
                  fill="none" stroke={bgColor} strokeWidth="5"
                  style={{ opacity: isHovered ? 0 : 1, transition: springAnim, pointerEvents: 'none' }}>
                  {p.name}</text>
                <text x={pts[0][0]} y={pts[0][1] + (isFwd ? -pp.fwdLabelY : pp.revLabelY)}
                  fontSize="12px" fontFamily="TeX Gyre Heros" fontWeight="600" fontStyle="italic"
                  textAnchor={isFwd ? 'start' : 'end'}
                  fill={pColor} stroke="none"
                  style={{ opacity: isHovered ? 0 : 1, transition: springAnim, pointerEvents: 'none' }}>
                  {p.name}</text>

                <path d={pathStr} fill="none" stroke="transparent" strokeWidth="20" />
                <rect
                  x={Math.min(...pts.map(p => p[0])) - 4} y={Math.min(...pts.map(p => p[1])) - 20}
                  width={Math.max(...pts.map(p => p[0])) - Math.min(...pts.map(p => p[0])) + 8}
                  height={Math.max(...pts.map(p => p[1])) - Math.min(...pts.map(p => p[1])) + 40}
                  fill="transparent"
                  onMouseEnter={() => setHoveredPrimer(p.id)}
                  onMouseLeave={() => setHoveredPrimer(null)}
                  className="cursor-pointer" />
              </g>
            );
          })}
        </g>
      );
    });
  };

  // Pre-compute enzyme geometry — one entry per cut pair (cut-twice enzymes get 2 entries)
  const enzymeLayout = useMemo(() => {
    const entries = [];
    for (const e of visibleEnzymes) {
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      pairs.forEach((cp, pi) => {
        const row = Math.floor(cp.topCutIndex / charsPerLine);
        const cutX = getX(cp.topCutIndex % charsPerLine);
        const sy = getSeqY(row);
        const hy = computeHighestY(row, cutX, enzLabelW(e.name, e.isUnique));
        const to = (enzymeTracks[e.id] || 0) * 18;
        // Clamp label top so it never overlaps the sequence text of the row above
        const minTop = row > 0 ? getSeqY(row - 1) + 8 : -Infinity;
        const yTop = Math.max(hy - 25 - to, minTop);
        entries.push({
          id: `${e.id}_p${pi}`,
          groupId: e.id,
          pairIndex: pi,
          name: e.name,
          cutX, sy,
          yTop,
          isUnique: e.isUnique,
          enzW: enzLabelW(e.name, e.isUnique),
        });
      });
    }
    return entries;
  }, [visibleEnzymes, enzymeTracks, charsPerLine, getSeqY, computeHighestY]);

  // Batched enzyme lines
  const enzymeLinesPath = useMemo(() => {
    let d = '';
    for (const l of enzymeLayout) {
      const yBot = l.sy - 18;
      d += `M${l.cutX} ${l.yTop} L${l.cutX} ${yBot}`;
    }
    return d;
  }, [enzymeLayout]);

  const renderEnzymes = () => {
    if (!enzymeLayout.length) return null;
    return (
      <g>
        <path d={enzymeLinesPath} fill="none" stroke="#333" strokeWidth="0.8"
          style={{ pointerEvents: 'none' }} />
        {enzymeLayout.filter(l => l.isUnique).map(l => (
          <line key={`u-${l.id}`} x1={l.cutX} x2={l.cutX} y1={l.yTop} y2={l.sy - 18}
            stroke="#333" strokeWidth="1" style={{ pointerEvents: 'none' }} />
        ))}
      </g>
    );
  };

  // Enzyme label backgrounds + hit areas — hover stores entry id, overlay/tooltip resolve groupId from it
  const renderEnzymeLabels = () => {
    return enzymeLayout.map(l => {
      const e = enzymes.find(x => x.id === l.groupId);
      const isGray = e && (e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length));
      const labelColor = isGray ? '#9CA3AF' : '#333';
      return (
        <g key={l.id} onMouseEnter={() => setHoveredEnzyme(l.id)} onMouseLeave={() => setHoveredEnzyme(null)}>
          <rect x={l.cutX + 3} y={l.yTop - 10} width={l.enzW + 2} height={18} fill="transparent" />
          {(() => {
            const enzText = { x: l.cutX + 6, y: l.yTop + 5, fontSize: "14px", fontFamily: "Cascadia Code", fontWeight: l.isUnique ? '700' : '350', style: { pointerEvents: 'none' } };
            const content = (() => { const s = splitEnzName(l.name); return s.normal ? [<tspan key="i" fontStyle="italic">{s.italic}</tspan>, <tspan key="n">{s.normal}</tspan>] : l.name; })();
            return <>
              <text {...enzText} fill="none" stroke={bgColor} strokeWidth="5">{content}</text>
              <text {...enzText} fill={labelColor} stroke="none">{content}</text>
            </>;
          })()}
        </g>
      );
    });
  };

  // Hover overlay — resolve groupId from hovered entry, highlight ALL cut lines of that enzyme
  const renderEnzymeOverlay = () => {
    if (!hoveredEnzyme) return null;
    const hoveredEntry = enzymeLayout.find(l => l.id === hoveredEnzyme);
    if (!hoveredEntry) return null;
    const groupId = hoveredEntry.groupId;
    const groupEntries = enzymeLayout.filter(l => l.groupId === groupId);
    const e = enzymes.find(x => x.id === groupId);
    if (!e) return null;
    const isGray = e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length);
    const ovColor = isGray ? '#9CA3AF' : '#2563EB';
    return (
      <g style={{ pointerEvents: 'none' }}>
        {groupEntries.map(l => {
          const tTop = l.sy - 22;
          return (
            <React.Fragment key={`ov-${l.id}`}>
              <line x1={l.cutX} x2={l.cutX} y1={l.yTop} y2={tTop} stroke={bgColor} strokeWidth="6" strokeLinecap="square" />
              <line x1={l.cutX} x2={l.cutX} y1={l.yTop} y2={tTop} stroke={ovColor} strokeWidth={e.isUnique ? '2' : '1'} />
            </React.Fragment>
          );
        })}
        {/* Overlay label on hovered entry's position */}
        {(() => {
          const methParts = [];
          if (e.methylationBlocked && e.methylationSources?.length) {
            methParts.push('[' + e.methylationSources.join('/') + ' Blocked]');
          }
          if (e.methylationRequired && e.methylRequiredSources?.length) {
            methParts.push('[' + e.methylRequiredSources.join('/') + ' Required]');
          }
          const methText = methParts.length ? '  ' + methParts.join(' ') : '';
          return (
            <React.Fragment>
              {/* Background stroke */}
              <text x={hoveredEntry.cutX + 6} y={hoveredEntry.yTop + 5} fill="none" stroke={bgColor} strokeWidth="5"
                fontSize="14px" fontFamily="Cascadia Code" fontWeight="700">
                {(() => { const s = splitEnzName(e.name); return s.normal ? [<tspan key="i" fontStyle="italic">{s.italic}</tspan>, <tspan key="n">{s.normal}</tspan>] : e.name; })()}
                {methText}
              </text>
              {/* Foreground text */}
              <text x={hoveredEntry.cutX + 6} y={hoveredEntry.yTop + 5} fill={ovColor} stroke="none"
                fontSize="14px" fontFamily="Cascadia Code" fontWeight="700">
                {(() => { const s = splitEnzName(e.name); return s.normal ? [<tspan key="i" fontStyle="italic">{s.italic}</tspan>, <tspan key="n">{s.normal}</tspan>] : e.name; })()}
                {methText}
              </text>
            </React.Fragment>
          );
        })()}
      </g>
    );
  };

  const renderTooltips = () => {
    if (!hoveredEnzyme) return null;
    const hoveredEntry = enzymeLayout.find(l => l.id === hoveredEnzyme);
    if (!hoveredEntry) return null;
    const e = enzymes.find(x => x.id === hoveredEntry.groupId);
    if (!e || e.displayStart === undefined) return null;
    const isGray = e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length);
    const ttColor = isGray ? '#9CA3AF' : '#2563EB';

    // Tooltip shared data
    const dispLen = e.displayEnd - e.displayStart + 1;
    const sw = e.isUnique ? '2' : '1';
    const swM = e.isUnique ? '3' : '1.5';
    const pad = 8;
    const ttH = 53;
    const cutPairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
    const sub = cleanSeq.substring(e.displayStart, e.displayEnd + 1);
    const comp = sub.split('').map(complement).join('');
    const pattern = e.recSeqPattern || '';
    const recOffset = e.recStart - e.displayStart;
    const recLen = e.recEnd - e.recStart + 1;
    const isRecBold = (i) => {
      if (i < recOffset || i >= recOffset + recLen) return false;
      const pi = i - recOffset;
      return pi < pattern.length && pattern[pi] !== 'N' && pattern[pi] !== 'n';
    };

    // All layout entries for this enzyme (one per cut pair)
    const groupEntries = enzymeLayout.filter(l => l.groupId === hoveredEntry.groupId);

    // Render one tooltip per cut pair, anchored at its row
    return (
      <g style={{ pointerEvents: 'none' }}>
        {groupEntries.map((entry) => {
          const sy = entry.sy;
          const ttY = sy - 23;
          const hp = cutPairs[entry.pairIndex] || cutPairs[0];
          const charsBeforeCut = hp.topCutIndex - e.displayStart;
          const baseX = entry.cutX - charsBeforeCut * cw;
          const leftX = baseX - pad;
          const ttW = dispLen * cw + pad * 2;

          // Polylines for all cut pairs relative to this tooltip's row.
          // The "local" pair (matching this entry) extends above tooltip + gets a notch.
          // Other pairs retract to the tooltip top edge.
          const polyEntries = cutPairs.map((cp, i) => {
            const tGapX = baseX + (cp.topCutIndex - e.displayStart) * cw;
            const bGapX = baseX + (cp.botCutIndex - e.displayStart) * cw;
            const isLocal = i === entry.pairIndex;
            return {
              tGapX, bGapX, isLocal,
              path: [
                `M ${tGapX} ${isLocal ? ttY - 2 : ttY + 6}`,
                `L ${tGapX} ${sy + 3}`,
                `L ${bGapX} ${sy + 3}`,
                `L ${bGapX} ${sy + 24}`,
              ].join(' '),
            };
          });

          return (
            <g key={`tt-${entry.id}`}>
              {/* Tooltip box */}
              <rect x={leftX} y={ttY} width={ttW} height={ttH} rx={8} fill="#FFFFFF" stroke={ttColor} strokeWidth={sw} />
              {/* White notch only for the local pair */}
              {polyEntries.filter(pe => pe.isLocal).map(pe => (
                <line key={`notch-${pe.tGapX}`} x1={pe.tGapX - 3} x2={pe.tGapX + 3} y1={ttY} y2={ttY} stroke="#FFFFFF" strokeWidth={swM} />
              ))}
              {/* Cut polylines */}
              {polyEntries.map((pe, i) => (
                <path key={`poly-${i}`} d={pe.path} fill="none" stroke={ttColor} strokeWidth={sw} strokeLinejoin="round" strokeLinecap="round" />
              ))}
              {/* Template sequence */}
              <text y={sy} fontFamily={monoFont} fontSize="16px">
                {sub.split('').map((c, i) => {
                  const bold = isRecBold(i);
                  return (
                    <tspan key={i} x={baseX + i * cw + cw / 2} textAnchor="middle"
                      fontWeight={bold ? '700' : '200'} fill={bold ? '#1f2937' : '#BFBFBF'}>{c}</tspan>
                  );
                })}
              </text>
              {/* Complement sequence */}
              <text y={sy + 18} fontFamily={monoFont} fontSize="16px">
                {comp.split('').map((c, i) => {
                  const bold = isRecBold(i);
                  return (
                    <tspan key={i} x={baseX + i * cw + cw / 2} textAnchor="middle"
                      fontWeight={bold ? '700' : '200'} fill={bold ? '#1f2937' : '#BFBFBF'}>{c}</tspan>
                  );
                })}
              </text>
            </g>
          );
        })}
      </g>
    );
  };

  const renderSeq = () => {
    const vs = Math.max(0, visibleRows.start - ROW_BUF);
    const ve = Math.min(numRows - 1, visibleRows.end + ROW_BUF);
    const rows = [];
    for (let r = vs; r <= ve; r++) {
      const chunk = cleanSeq.substring(r * charsPerLine, (r + 1) * charsPerLine);
      rows.push(
        <text key={r} y={getSeqY(r)} fontFamily={monoFont} fontSize="16px" fontWeight="bold" fill="#1f2937"
          style={{ userSelect: 'text', cursor: 'text' }}>
          {chunk.split('').map((char, i) => <tspan key={i} x={getX(i) + cw / 2} textAnchor="middle">{char}</tspan>)}
        </text>
      );
    }
    return rows;
  };

  return (
    <div ref={containerRef} style={{ backgroundColor: bgColor, width: '100%', minHeight: '100vh', display: 'flex', justifyContent: 'center', alignItems: 'flex-start', padding: '2rem 2rem 4rem 2rem', overflowX: 'auto', userSelect: 'none' }} className="">
      <div style={{ width: svgWidth }}>
        <svg width="100%" height={svgHeight} style={{ display: 'block', overflow: 'visible' }}>
          {renderFeatures()}
          {renderFeatureLabels()}
          {renderEnzymes()}
          {renderPrimers()}
          {renderEnzymeLabels()}
          {renderEnzymeOverlay()}
          {renderSeq()}
          {renderTooltips()}
        </svg>
      </div>
    </div>
  );
}