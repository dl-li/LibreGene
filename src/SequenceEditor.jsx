import React, { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { cw, startX, baseSeqY, bgColor, monoFont, sansFont, springAnim, getX, complement, measureWidth, enzLabelW, primerLabelW, splitRange, enzymeActiveBlue } from './editorConstants';

const complementStr = (s) => s.split('').map(c => complement(c)).join('');
const reverseComplement = (s) => complementStr(s).split('').reverse().join('');

const isIISEnzyme = (e) => {
  if (!e) return false;
  const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
  return pairs.some(cp => {
    const dTopRight = Math.max(0, cp.topCutIndex - e.recEnd);
    const dTopLeft = Math.max(0, e.recStart - cp.topCutIndex);
    const dBotRight = Math.max(0, cp.botCutIndex - e.recEnd);
    const dBotLeft = Math.max(0, e.recStart - cp.botCutIndex);
    return Math.max(dTopRight, dTopLeft, dBotRight, dBotLeft) >= 2;
  });
};

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

const SequenceEditor = React.memo(function SequenceEditor({ sequence, features = [], enzymes = [], primers = [], initialCharsPerLine = 60, layoutParams = {}, layoutKey }) {
  const containerRef = useRef(null);
  const [charsPerLine, setCharsPerLine] = useState(initialCharsPerLine);
  const [hoveredFeature, setHoveredFeature] = useState(null);
  const featureLeaveRef = useRef(null);
  const [hoveredPrimer, setHoveredPrimer] = useState(null);
  const [hoveredEnzyme, setHoveredEnzyme] = useState(null);
  const [scrollY, setScrollY] = useState(0);
  const scrollTickingRef = useRef(false);
  const lastVisibleStartRef = useRef(-1);
  const lastVisibleEndRef = useRef(-1);
  const numRowsRef = useRef(1);
  const svgRef = useRef(null);

  // --- selection state ---
  const [cursorIndex, setCursorIndex] = useState(null);
  const [selStart, setSelStart] = useState(null);
  const [selEnd, setSelEnd] = useState(null);
  const [isDragging, setIsDragging] = useState(false);
  const dragRef = useRef({ startIdx: null, active: false });
  const isDraggingRef = useRef(false);
  const cursorTimerRef = useRef(null);
  const resetCursorTimer = useCallback(() => {
    if (cursorTimerRef.current) clearTimeout(cursorTimerRef.current);
    cursorTimerRef.current = setTimeout(() => setCursorIndex(null), 5000);
  }, []);

  const clearCursorTimer = useCallback(() => {
    if (cursorTimerRef.current) { clearTimeout(cursorTimerRef.current); cursorTimerRef.current = null; }
  }, []);

  // --- enzyme selection state ---
  const [isEnzymeSelection, setIsEnzymeSelection] = useState(false);
  const [isEnzymeDragging, setIsEnzymeDragging] = useState(false);
  const [selectedEnzymeIds, setSelectedEnzymeIds] = useState([]); // persists after click (can be 1 or 2)
  const enzymeDragRef = useRef(null);
  const lastEnzymeSelRef = useRef(null); // { enzymeId, cutIdx, name } for shift+click

  const hasSelection = selStart !== null && selEnd !== null && selStart <= selEnd;
  const currentSelColor = isEnzymeSelection ? enzymeActiveBlue : '#3E2723';

  const pp = useMemo(() => ({
    fwdMatchY: 30, revMatchY: 26, misYDelta: 4,
    fwdBaseTextY: 8, revBaseTextY: 18,
    fwdLabelY: 8, revLabelY: 20,
    trackGap: 36,
    fwdAboveBase: 30, fwdAboveExtra: 23, fwdAboveNonTailExtra: 5,
    revBelowBase: 26, revBelowExtra: 25, revBelowNonTailExtra: 5,
    hoverExpand: 26, arrowHeadLen: 7, arrowHeadHeight: 5,
    ...layoutParams,
  }), [layoutParams]);

  const lp = useMemo(() => ({
    minRowGap: 27,
    rowContentGap: 12,
    featTrackHeight: 18,
    featBaseOffset: 14,
    featLabelPad: 12,
    enzTrackHeight: 18,
    enzLineGap: 18,
    enzLabelBase: 51,
    enzAbovePad: 10,
    minAboveSpace: 24,
    minBelowSpace: 14,
    ...layoutParams,
  }), [layoutParams]);

  useEffect(() => {
    const handleResize = () => {
      if (containerRef.current) {
        setCharsPerLine(Math.max(20, Math.floor((containerRef.current.clientWidth - startX * 2) / cw)));
      }
    };
    const handleScroll = () => {
      if (!scrollTickingRef.current) {
        scrollTickingRef.current = true;
        requestAnimationFrame(() => {
          const sy = window.scrollY;
          const vh = window.innerHeight || 900;
          const nr = numRowsRef.current;
          const estRowH = 60;
          const buf = 8;
          const estStart = Math.max(0, Math.floor(sy / estRowH) - buf - 1);
          const estEnd = Math.min(nr - 1, Math.floor((sy + vh) / estRowH) + buf + 1);
          if (estStart !== lastVisibleStartRef.current || estEnd !== lastVisibleEndRef.current) {
            lastVisibleStartRef.current = estStart;
            lastVisibleEndRef.current = estEnd;
            setScrollY(sy);
          }
          scrollTickingRef.current = false;
        });
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

  // Recalculate layout when parent padding changes (e.g. sidebar pin)
  useEffect(() => {
    if (layoutKey === undefined) return;
    if (containerRef.current) {
      setCharsPerLine(Math.max(20, Math.floor((containerRef.current.clientWidth - startX * 2) / cw)));
    }
  }, [layoutKey]);

  const cleanSeq = sequence || '';

  // Enrich primers with flat fields from bindingSites data model (v2).
  const enrichedPrimers = useMemo(() => (primers || []).map(p => {
    // Already enriched (legacy flat fields or pre-computed).
    if (p.matchStart !== undefined && p.matchEnd !== undefined) return p;
    const bs = p.bindingSites?.[0];
    if (!bs) return p;
    // templateStart (inclusive), templateEnd (exclusive) — convert to legacy inclusive matchEnd
    const ms = bs.templateStart ?? bs.matchStart ?? 0;
    const me = (bs.templateEnd != null) ? bs.templateEnd - 1 : (bs.matchEnd ?? 0);
    const aln = bs.alignment || {};
    const ds = aln.displaySequence || '';
    const misSet = new Set(aln.mismatchIndices || []);
    // Build per-column render data for the binding region
    const renderCols = [];
    for (let i = 0; i < ds.length; i++) {
      const ch = ds[i];
      const tcol = ms + i;
      let kind, primerBase, insDetail;
      if (ch === '-') {
        kind = 'gap'; primerBase = '-';
      } else if (ch >= '0' && ch <= '9') {
        kind = 'insertion'; primerBase = ch; insDetail = aln.insertionMap?.[ch];
      } else if (misSet.has(i)) {
        kind = 'mismatch'; primerBase = ch;
      } else {
        kind = 'match'; primerBase = ch;
      }
      renderCols.push({ templateCol: tcol, kind, primerBase, insDetail, displayIdx: i });
    }
    const isFwd = (bs.strand ?? 1) === 1;
    return {
      ...p,
      matchStart: ms,
      matchEnd: me,
      isFwd, // actual binding direction (NOT declared type)
      matchStr: isFwd
        ? cleanSeq.substring(ms, me + 1)
        : complementStr(cleanSeq.substring(ms, me + 1)),
      // tails for rendering
      mismatchStr: bs.fivePrimeTail || '',
      threePrimeTail: bs.threePrimeTail || '',
      // rich alignment data
      renderCols,
      displaySequence: ds,
    };
  }), [primers, cleanSeq]);

  const numRows = Math.max(1, Math.ceil(cleanSeq.length / charsPerLine));
  numRowsRef.current = numRows;
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
    for (const isFwd of [true, false]) {
      const ofType = (enrichedPrimers || []).filter(p => p.isFwd === isFwd);
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
          const vs = isFwd ? p.matchStart - ml : p.matchStart;
          const ve = !isFwd ? p.matchEnd + ml : p.matchEnd;
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
    for (const p of (enrichedPrimers || []).filter(p => !p.isFwd)) {
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
            revFeatOff[p.id][r] = Math.max(revFeatOff[p.id][r] || 0, (ft + 1) * lp.featTrackHeight);
          }
        }
      }
    }

    return { processedFeatures: resultFeatures, primerTracks: pTracks, featureRowTracks: fRowTracks, revPrimerFeatOffsets: revFeatOff };
  }, [features, enrichedPrimers, numRows, charsPerLine, lp]);

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

  const { rowAbove, rowBelow, enzymeRowTracks } = useMemo(() => {
    // Per-row enzyme track assignment — cut-twice enzymes are expanded per pair
    const eTracks = {};
    for (let r = 0; r < numRows; r++) {
      const rEnz = enzymesByRow[r];
      if (!rEnz || !rEnz.length) continue;
      // Expand cut-twice enzymes into per-pair entries for independent track assignment
      const expanded = [];
      for (const e of rEnz) {
        const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
        pairs.forEach((cp, pi) => {
          if (Math.floor(cp.topCutIndex / charsPerLine) === r) {
            expanded.push({
              key: pairs.length > 1 ? `${e.id}_p${pi}` : e.id,
              cutIndex: cp.topCutIndex,
              name: e.name,
              isUnique: e.isUnique,
            });
          }
        });
      }
      const sorted = expanded.sort((a, b) => b.cutIndex - a.cutIndex || b.name.length - a.name.length);
      const occupied = [];
      for (const item of sorted) {
        const cs = item.cutIndex % charsPerLine;
        const ce = cs + Math.ceil(enzLabelW(item.name, item.isUnique) / cw);
        let t = 0;
        while (occupied.some(o => o.track === t && !(ce < o.cs || cs > o.ce))) t++;
        occupied.push({ track: t, cs, ce });
        if (!eTracks[item.key]) eTracks[item.key] = {};
        eTracks[item.key][r] = t;
      }
    }

    const above = new Array(numRows).fill(0);
    const below = new Array(numRows).fill(0);

    for (let r = 0; r < numRows; r++) {
      let ae = lp.minAboveSpace, be = lp.minBelowSpace;

      const rowPrimers = primersByRow[r];
      let maxFwdPrimerH = 0;

      if (rowPrimers) {
        for (const p of rowPrimers) {
          const t = (primerTracks[p.id] || {})[r] || 0;
          if (p.isFwd) {
            const hasTail = r === Math.floor(p.matchStart / charsPerLine);
            const extra = hasTail ? pp.fwdAboveExtra : pp.fwdAboveNonTailExtra;
            const h = pp.fwdAboveBase + t * pp.trackGap + extra;
            maxFwdPrimerH = Math.max(maxFwdPrimerH, h);
            ae = Math.max(ae, h);
          } else {
            const hasTail = r === Math.floor(p.matchEnd / charsPerLine);
            const extra = hasTail ? pp.revBelowExtra : pp.revBelowNonTailExtra;
            const featOff = (revPrimerFeatOffsets[p.id] || {})[r] || 0;
            be = Math.max(be, pp.revBelowBase + t * pp.trackGap + extra + featOff);
          }
        }
      }

      // Enzymes: above fwd primers; per-row tracks from eTracks
      const rowEnz = enzymesByRow[r];
      if (rowEnz && rowEnz.length > 0) {
        let maxEnzTrack = 0;
        for (const e of rowEnz) {
          const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
          pairs.forEach((cp, pi) => {
            if (Math.floor(cp.topCutIndex / charsPerLine) !== r) return;
            const key = pairs.length > 1 ? `${e.id}_p${pi}` : e.id;
            const t = (eTracks[key] || {})[r] || 0;
            maxEnzTrack = Math.max(maxEnzTrack, t);
          });
        }
        const enzDefaultAbove = lp.enzLabelBase + lp.enzAbovePad;
        const enzBase = maxFwdPrimerH > 0 ? Math.max(enzDefaultAbove, maxFwdPrimerH + 38) : enzDefaultAbove;
        ae = Math.max(ae, enzBase + maxEnzTrack * lp.enzTrackHeight);
      }

      // Features: below sequence
      const rowFeats = featuresByRow[r];
      if (rowFeats) {
        for (const { feature: f } of rowFeats) {
          const t = (featureRowTracks[f.id] || {})[r] || 0;
          be = Math.max(be, lp.featBaseOffset + t * lp.featTrackHeight + lp.featLabelPad);
        }
      }

      above[r] = ae;
      below[r] = be;
    }

    return { rowAbove: above, rowBelow: below, enzymeRowTracks: eTracks };
  }, [numRows, enzymesByRow, primersByRow, featuresByRow, primerTracks, featureRowTracks, revPrimerFeatOffsets, charsPerLine, pp, lp]);

  const rowY = useMemo(() => {
    const y = [Math.max(baseSeqY, rowAbove[0] + lp.minRowGap)];
    for (let r = 0; r < numRows - 1; r++) {
      y.push(y[r] + Math.max(lp.minRowGap, rowBelow[r] + rowAbove[r + 1] + lp.rowContentGap));
    }
    return y;
  }, [numRows, rowAbove, rowBelow, lp.minRowGap, lp.rowContentGap]);
  const getSeqY = useCallback((row) => rowY[Math.min(row, rowY.length - 1)], [rowY]);

  // --- selection: coordinate conversion & event handlers ---
  const clientToSeqIndex = useCallback((clientX, clientY) => {
    if (!svgRef.current) return null;
    const pt = svgRef.current.createSVGPoint();
    pt.x = clientX;
    pt.y = clientY;
    const ctm = svgRef.current.getScreenCTM();
    if (!ctm) return null;
    const svgPt = pt.matrixTransform(ctm.inverse());
    const xRel = svgPt.x - startX;
    if (xRel < -cw / 2 || xRel > charsPerLine * cw + cw / 2) return null;
    const xInCell = (xRel % cw + cw) % cw;
    const colBase = Math.floor(xRel / cw);
    const side = xInCell < cw / 2 ? 0 : 1;
    const col = Math.max(0, Math.min(charsPerLine, colBase + side));
    // Content-based row boundaries: top of current row → top of next row
    // Row spacing already ensures a gap between row content areas
    let row = -1;
    for (let r = 0; r < numRows; r++) {
      const sy = getSeqY(r);
      const top = sy - rowAbove[r];
      const bottom = r < numRows - 1 ? getSeqY(r + 1) - rowAbove[r + 1] : Infinity;
      if (svgPt.y >= top && svgPt.y < bottom) { row = r; break; }
    }
    if (row < 0) return null;
    const idx = row * charsPerLine + col;
    return Math.max(0, Math.min(cleanSeq.length, idx));
  }, [charsPerLine, numRows, getSeqY, cleanSeq]);

  const handleSvgMouseDown = useCallback((e) => {
    if (e.button !== 0) return;
    // Reset enzyme selection when clicking on sequence directly
    setIsEnzymeSelection(false);
    setSelectedEnzymeIds([]);
    lastEnzymeSelRef.current = null;
    const idx = clientToSeqIndex(e.clientX, e.clientY);
    if (idx === null) return;
    if (e.shiftKey && cursorIndex !== null) {
      // Shift+click: select from cursor to click position
      const s = Math.min(cursorIndex, idx);
      const e = Math.max(cursorIndex, idx) - 1;
      if (s <= e) {
        setSelStart(s);
        setSelEnd(e);
      }
      setCursorIndex(idx);
      dragRef.current = { startIdx: idx, active: false };
      clearCursorTimer();
      return;
    }
    dragRef.current = { startIdx: idx, active: false };
    setIsDragging(false);
    setCursorIndex(idx);
    setSelStart(null);
    setSelEnd(null);
    resetCursorTimer();
  }, [clientToSeqIndex, resetCursorTimer, clearCursorTimer, cursorIndex]);

  useEffect(() => {
    const onMove = (e) => {
      if (dragRef.current.startIdx === null) return;
      const idx = clientToSeqIndex(e.clientX, e.clientY);
      if (idx === null) return;
      const dist = Math.abs(idx - dragRef.current.startIdx);
      if (dist > 0) {
        dragRef.current.active = true;
        setIsDragging(true);
        // Cursor is at insertion point idx; selected chars are from min to max-1
        setSelStart(Math.min(dragRef.current.startIdx, idx));
        setSelEnd(Math.max(dragRef.current.startIdx, idx) - 1);
        setCursorIndex(idx);
        resetCursorTimer();
      }
    };
    window.addEventListener('mousemove', onMove);
    return () => window.removeEventListener('mousemove', onMove);
  }, [clientToSeqIndex, resetCursorTimer]);

  useEffect(() => {
    const onUp = () => {
      // Handle enzyme drag end
      if (enzymeDragRef.current?.active) {
        const dragData = enzymeDragRef.current;
        // Select recognition site if: never dragged, or dragged back to same label
        if (!dragData.didDrag || dragData.backToStart) {
          setSelStart(dragData.recStart);
          setSelEnd(dragData.recEnd);
          setSelectedEnzymeIds([dragData.entryId]);
        }
        // If dragged to another enzyme, selectedEnzymeIds already set by onMouseEnter
        lastEnzymeSelRef.current = { enzymeId: dragData.startEnzymeId, cutIdx: dragData.startCutIdx, name: dragData.startName, entryId: dragData.entryId };
        enzymeDragRef.current = null;
        isDraggingRef.current = false;
        setIsDragging(false);
        setIsEnzymeDragging(false);
        setHoveredEnzyme(null);
        clearCursorTimer();
        return;
      }
      if (dragRef.current.startIdx === null) return;
      if (dragRef.current.active) {
        setCursorIndex(null);
        clearCursorTimer();
      }
      setIsDragging(false);
      dragRef.current = { startIdx: null, active: false };
    };
    window.addEventListener('mouseup', onUp);
    return () => window.removeEventListener('mouseup', onUp);
  }, [clearCursorTimer]);

  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'ArrowLeft' || e.key === 'ArrowRight' || e.key === 'ArrowUp' || e.key === 'ArrowDown') {
        if (cursorIndex === null) return;
        e.preventDefault();
        let ni = cursorIndex;
        if (e.key === 'ArrowLeft') ni = Math.max(0, cursorIndex - 1);
        else if (e.key === 'ArrowRight') ni = Math.min(cleanSeq.length, cursorIndex + 1);
        else if (e.key === 'ArrowUp') ni = Math.max(0, cursorIndex - charsPerLine);
        else if (e.key === 'ArrowDown') ni = Math.min(cleanSeq.length, cursorIndex + charsPerLine);
        if (ni !== cursorIndex) {
          setCursorIndex(ni);
          setSelStart(null);
          setSelEnd(null);
          resetCursorTimer();
        }
      }
      if ((e.ctrlKey || e.metaKey) && e.key === 'c') {
        if (hasSelection) {
          e.preventDefault();
          navigator.clipboard.writeText(cleanSeq.substring(selStart, selEnd + 1)).catch(() => {});
        }
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [cursorIndex, selStart, selEnd, hasSelection, cleanSeq, charsPerLine, resetCursorTimer]);

  useEffect(() => { isDraggingRef.current = isDragging; }, [isDragging]);
  useEffect(() => () => { clearCursorTimer(); clearTimeout(featureLeaveRef.current); }, [clearCursorTimer]);

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

  // --- enzyme track assignment is now in the spacing memo (enzymeRowTracks) ---

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

  const renderedFeatures = useMemo(() => {
    if (!visibleFeatures.length) return null;
    return visibleFeatures.map(f => {
      const isHovered = hoveredFeature === f.id;
      const dataSegs = f.segments;

      const visuals = [];
      const seenRows = new Set();
      for (let di = 0; di < dataSegs.length; di++) {
        const ds = dataSegs[di];
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
        for (const vs of sp(ds.start, ds.end)) {
          const showLabel = !seenRows.has(vs.row);
          seenRows.add(vs.row);
          const segColor = dataSegs[di].color || f.color || ensureReadableColor('#60A5FA');
          visuals.push({ type: 'solid', row: vs.row, colStart: vs.colStart, colEnd: vs.colEnd, showLabel, color: segColor });
        }
      }

      visuals.sort((a, b) => (a.row * charsPerLine + a.colStart) - (b.row * charsPerLine + b.colStart));
      if (!visuals.length) return null;

      return (
        <g key={f.id}>
          {visuals.map((v) => {
            const x = getX(v.colStart);
            const w = (v.colEnd - v.colStart + 1) * cw;
            const sy = getSeqY(v.row);
            const rowTo = ((featureRowTracks[f.id] || {})[v.row] || 0) * lp.featTrackHeight;
            const y = sy + lp.featBaseOffset + rowTo;
            const isGap = v.type === 'gap';

            return (
              <g key={`${v.type}-${v.row}-${v.colStart}`}
                onMouseEnter={() => { if (isDraggingRef.current) return; clearTimeout(featureLeaveRef.current); setHoveredFeature(f.id); }}
                onMouseLeave={() => { featureLeaveRef.current = setTimeout(() => setHoveredFeature(null), 250); }}
                onMouseDown={(e) => {
                  e.stopPropagation(); e.preventDefault();
                  const fStart = Math.min(...f.segments.map(s => s.start));
                  const fEnd = Math.max(...f.segments.map(s => s.end));
                  setSelStart(fStart); setSelEnd(fEnd);
                  setCursorIndex(fEnd + 1); clearCursorTimer();
                }}
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
  }, [visibleFeatures, hoveredFeature, featureRowTracks, getSeqY, sp, charsPerLine, clearCursorTimer, lp]);

  const truncatedLabel = useCallback((name, isRev, isFwd, maxLen = 12) => {
    const full = isRev ? `< ${name}` : isFwd ? `${name} >` : name;
    if (name.length <= maxLen) return { full, short: full };
    const short = isRev ? `< ${name.slice(0, maxLen)}··` : isFwd ? `${name.slice(0, maxLen)}·· >` : `${name.slice(0, maxLen)}··`;
    return { full, short };
  }, []);

  const renderedFeatureLabels = useMemo(() => {
    if (!visibleFeatures.length) return null;
    const seen = new Set();
    return visibleFeatures.flatMap(f => {
      const isRev = f.strand === '-';
      const isFwd = f.strand === '+';
      const labelColor = computeDominantColor(f);
      const isHovered = hoveredFeature === f.id;
      const { full: fullText, short: shortText } = truncatedLabel(f.name, isRev, isFwd);
      const labelText = isHovered ? fullText : shortText;

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
        const rowTo = ((featureRowTracks[f.id] || {})[vs.row] || 0) * lp.featTrackHeight;
        const y = sy + lp.featBaseOffset + rowTo;
        const textProps = { y: y + 4, fontSize: "12px", fontFamily: "TeX Gyre Heros", fontWeight: "600" };
        if (isRev) {
          const xr = getX(vs.colEnd + 1);
          return (
            <g key={key}
              onMouseEnter={() => { if (isDraggingRef.current) return; clearTimeout(featureLeaveRef.current); setHoveredFeature(f.id); }}
              onMouseLeave={() => { featureLeaveRef.current = setTimeout(() => setHoveredFeature(null), 250); }}
              onMouseDown={(e) => {
                e.stopPropagation(); e.preventDefault();
                const fStart = Math.min(...f.segments.map(s => s.start));
                const fEnd = Math.max(...f.segments.map(s => s.end));
                setSelStart(fStart); setSelEnd(fEnd);
                setCursorIndex(fEnd + 1); clearCursorTimer();
              }}
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
            onMouseDown={(e) => {
              e.stopPropagation(); e.preventDefault();
              const fStart = Math.min(...f.segments.map(s => s.start));
              const fEnd = Math.max(...f.segments.map(s => s.end));
              setSelStart(fStart); setSelEnd(fEnd);
              setCursorIndex(fEnd + 1); clearCursorTimer();
            }}
            className="cursor-pointer">
            <text x={x - 8} {...textProps} textAnchor="end" fill="none" stroke={bgColor} strokeWidth="5">{labelText}</text>
            <text x={x - 8} {...textProps} textAnchor="end" fill={labelColor} stroke="none">{labelText}</text>
          </g>
        );
      });
    });
  }, [visibleFeatures, hoveredFeature, featureRowTracks, getSeqY, sp, clearCursorTimer, truncatedLabel, lp]);

  const renderedPrimers = useMemo(() => {
    if (!visiblePrimers.length) return null;
    return visiblePrimers.map((p) => {
      const isFwd = p.isFwd;
      const isHovered = hoveredPrimer === p.id;
      const misLen = p.mismatchStr?.length || 0;
      const hasMis = misLen > 0;
      const pColor = p.color || '#166534';
      const segs = sp(p.matchStart, p.matchEnd);
      if (p.renderCols) {
        let ci = 0;
        const rowEndOf = (s) => s.row * charsPerLine + s.colEnd;
        for (const seg of segs) {
          seg.renderCols = [];
          while (ci < p.renderCols.length && p.renderCols[ci].templateCol <= rowEndOf(seg)) {
            if (p.renderCols[ci].templateCol >= seg.colStart + seg.row * charsPerLine) {
              seg.renderCols.push(p.renderCols[ci]);
            }
            ci++;
          }
        }
      }
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
        <g key={p.id}>
          {segs.map(seg => {
            const isTail = seg === tailSeg, isArrow = seg === arrowSeg;
            const sy = getSeqY(seg.row);
            const featOff = isFwd ? 0 : ((revPrimerFeatOffsets[p.id] || {})[seg.row] || 0);
            const trackOff = ((primerTracks[p.id] || {})[seg.row] || 0) * pp.trackGap + featOff;
            const matchY = (isFwd ? sy - pp.fwdMatchY : sy + pp.revMatchY) + (isFwd ? -trackOff : trackOff);
            const misY = matchY + (isFwd ? -pp.misYDelta : pp.misYDelta);
            const x1 = getX(seg.colStart), x2 = getX(seg.colEnd);

            // Build per-column path points from renderCols
            let pts = [];
            let edge3x, edge5x;
            const hasRenderCols = seg.renderCols && seg.renderCols.length > 0;
            if (hasRenderCols) {
              // Per-column zigzag path
              const cols = isFwd ? seg.renderCols : [...seg.renderCols].reverse();
              const firstCol = cols[0], lastCol = cols[cols.length - 1];
              edge5x = isFwd
                ? getX(firstCol.templateCol % charsPerLine)               // fwd: left edge of leftmost
                : getX(firstCol.templateCol % charsPerLine) + cw;         // rev: right edge of rightmost
              edge3x = isFwd
                ? getX(lastCol.templateCol % charsPerLine) + cw            // fwd: right edge of rightmost
                : getX(lastCol.templateCol % charsPerLine);               // rev: left edge of leftmost
              // 5' tail
              if (isTail && hasMis && drawMisLen > 0) {
                if (isFwd) pts.push([x1 - drawMisLen * cw, misY], [x1 - cw / 2, misY]);
                else pts.push([getX(seg.colEnd + drawMisLen + 1), misY], [x2 + cw * 1.5, misY]);
              }
              pts.push([edge5x, cols[0].kind === 'match' ? matchY : misY]);
              for (const rc of cols) {
                const cx = getX(rc.templateCol % charsPerLine) + cw / 2;
                const cy = rc.kind === 'match' ? matchY : misY;
                pts.push([cx, cy]);
              }
              pts.push([edge3x, cols[cols.length - 1].kind === 'match' ? matchY : misY]);
            } else {
              // Fallback: straight line
              if (isTail && hasMis && drawMisLen > 0) {
                if (isFwd) pts.push([x1 - drawMisLen * cw, misY], [x1 - cw / 2, misY]);
                else pts.push([getX(seg.colEnd + drawMisLen + 1), misY], [x2 + cw * 1.5, misY]);
              }
              if (isFwd) pts.push([x1 + cw / 2, matchY], [x2 + cw, matchY]);
              else pts.push([x2 + cw, matchY], [x1, matchY]);
            }
            if (pts.length < 2) return null;

            const pathStr = `M ${pts.map(p => `${p[0]} ${p[1]}`).join(' L ')}`;
            const expD = isFwd ? -1 : 1;
            const curExp = isHovered ? pp.hoverExpand : 0;
            const last = pts[pts.length - 1];
            const hoverPath = pathStr +
              ` L ${last[0]} ${last[1] + expD * curExp} ` +
              [...pts].reverse().map(p => `L ${p[0]} ${p[1] + expD * curExp}`).join(' ') + ' Z';

            const arrowTipY = (hasRenderCols && seg.renderCols.length > 0)
              ? (seg.renderCols[seg.renderCols.length - 1].kind === 'match' ? matchY : misY)
              : matchY;
            const arrowBaseX = hasRenderCols ? edge3x : (isFwd ? x2 + cw : x1);
            const arrowPath = isArrow
              ? `M ${arrowBaseX} ${arrowTipY} L ${isFwd ? arrowBaseX - pp.arrowHeadLen : arrowBaseX + pp.arrowHeadLen} ${arrowTipY + expD * pp.arrowHeadHeight}` : '';

            const visMis = hasMis && drawMisLen > 0 ? p.mismatchStr.slice(misLen - drawMisLen) : '';

            return (
              <g key={`${seg.row}-${seg.colStart}`}>
                <path d={hoverPath} fill={bgColor} style={{ transition: springAnim }} />
                <path d={hoverPath} fill={pColor} fillOpacity={0.1} style={{ transition: springAnim }} />

                <text fill={pColor} fontSize="14px" fontFamily={monoFont} fontWeight="bold"
                  style={{ opacity: isHovered ? 1 : 0, transition: 'opacity 0.2s ease-in-out', pointerEvents: 'none' }}>
                  {/* 5' tail */}
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
                  {/* Per-column alignment rendering */}
                  {seg.renderCols && seg.renderCols.map((rc) => {
                    const isOffset = rc.kind === 'mismatch' || rc.kind === 'gap' || rc.kind === 'insertion';
                    const y = (isOffset ? misY : matchY) + (isFwd ? -pp.fwdBaseTextY : pp.revBaseTextY);
                    const x = getX(rc.templateCol % charsPerLine) + cw / 2;
                    const isGap = rc.kind === 'gap';
                    const isIns = rc.kind === 'insertion';
                    return (
                      <tspan key={`aln-${rc.templateCol}`} x={x} y={y} textAnchor="middle"
                        fill={isGap ? '#9ca3af' : undefined} fontWeight={isGap ? '200' : undefined}
                        fontSize={isIns ? '10px' : undefined}>
                        {isIns ? (rc.insDetail?.insertedBases || rc.primerBase) : rc.primerBase}
                      </tspan>
                    );
                  })}
                  {/* 3' tail */}
                  {isArrow && p.threePrimeTail && (
                    (() => {
                      const tail3 = p.threePrimeTail;
                      const tailLen = tail3.length;
                      return tail3.split('').map((c, k) => (
                        <tspan key={`3t-${k}`}
                          x={(isFwd ? getX(seg.colEnd + k + 1) : getX(seg.colStart - tailLen + k)) + cw / 2}
                          y={misY + (isFwd ? -pp.fwdBaseTextY : pp.revBaseTextY)} textAnchor="middle">{c}</tspan>
                      ));
                    })()
                  )}
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
                  onMouseEnter={() => { if (isDraggingRef.current) return; setHoveredPrimer(p.id); }}
                  onMouseLeave={() => setHoveredPrimer(null)}
                  className="cursor-pointer" />
              </g>
            );
          })}
        </g>
      );
    });
  }, [visiblePrimers, hoveredPrimer, charsPerLine, pp, primerTracks, revPrimerFeatOffsets, getSeqY, sp]);

  // Pre-compute enzyme geometry — one entry per cut pair (cut-twice enzymes get 2 entries)
  const enzymeLayout = useMemo(() => {
    const entries = [];
    for (const e of visibleEnzymes) {
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      pairs.forEach((cp, pi) => {
        const row = Math.floor(cp.topCutIndex / charsPerLine);
        const cutX = getX(cp.topCutIndex % charsPerLine);
        const sy = getSeqY(row);
        const enzTrack = (enzymeRowTracks[pairs.length > 1 ? `${e.id}_p${pi}` : e.id] || {})[row] || 0;

        // Push enzyme label up to avoid overlapping fwd primers on the same row
        let avoidOff = 0;
        const enzW = enzLabelW(e.name, e.isUnique);
        const rp = primersByRow[row];
        if (rp) {
          for (const p of rp) {
            if (!p.isFwd) continue;
            const segs = sp(p.matchStart, p.matchEnd);
            for (const seg of segs) {
              if (seg.row !== row) continue;
              const ml = p.mismatchStr?.length || 0;
              const isTail = seg === segs[0];
              const drawMisLen = isTail ? Math.min(ml, seg.colStart + 5) : 0;
              const nameW = primerLabelW(p.name);
              const nameX = (isTail && drawMisLen > 0) ? getX(seg.colStart - drawMisLen) : getX(seg.colStart) + cw / 2;
              if (cutX < nameX + nameW + 4 && cutX + enzW > nameX) {
                const pt = (primerTracks[p.id] || {})[row] || 0;
                const hasTail = isTail && ml > 0;
                avoidOff = Math.max(avoidOff, (hasTail ? 40 : 36) + pt * pp.trackGap);
              }
            }
          }
        }

        // Clamp label top so it never overlaps the sequence text of the row above
        const minTop = row > 0 ? getSeqY(row - 1) + 8 : -Infinity;
        const yTop = Math.max(sy - lp.enzLabelBase - avoidOff - enzTrack * lp.enzTrackHeight, minTop);
        entries.push({
          id: `${e.id}_p${pi}`,
          groupId: e.id,
          pairIndex: pi,
          name: e.name,
          cutX, sy, row,
          yTop,
          isUnique: e.isUnique,
          enzW,
          topCutIndex: cp.topCutIndex,
          botCutIndex: cp.botCutIndex,
        });
      });
    }
    return entries;
  }, [visibleEnzymes, enzymeRowTracks, lp, charsPerLine, getSeqY, primersByRow, primerTracks, sp, pp.trackGap]);

  // Batched enzyme lines
  const enzymeLinesPath = useMemo(() => {
    let d = '';
    for (const l of enzymeLayout) {
      const yBot = l.sy - lp.enzLineGap;
      d += `M${l.cutX} ${l.yTop} L${l.cutX} ${yBot}`;
    }
    return d;
  }, [enzymeLayout, lp.enzLineGap]);

  const renderedEnzymes = useMemo(() => {
    if (!enzymeLayout.length) return null;
    return (
      <g>
        <path d={enzymeLinesPath} fill="none" stroke="#333" strokeWidth="0.8"
          style={{ pointerEvents: 'none' }} />
        {enzymeLayout.filter(l => l.isUnique).map(l => (
          <line key={`u-${l.id}`} x1={l.cutX} x2={l.cutX} y1={l.yTop} y2={l.sy - lp.enzLineGap}
            stroke="#333" strokeWidth="1" style={{ pointerEvents: 'none' }} />
        ))}
      </g>
    );
  }, [enzymeLayout, enzymeLinesPath, lp.enzLineGap]);

  const totalNameCounts = useMemo(() => {
    const m = new Map();
    for (const e of enzymes) {
      const nPairs = (e.cutPairs && e.cutPairs.length) || 1;
      m.set(e.name, (m.get(e.name) || 0) + nPairs);
    }
    return m;
  }, [enzymes]);

  // Precise label width: measures italic + normal parts separately, includes ²
  const exactLabelW = useCallback((name, isUnique, showTwo) => {
    const s = splitEnzName(name);
    const baseFont = `${isUnique ? '700' : '350'} 14px Cascadia Code`;
    let w;
    if (s.normal) {
      w = measureWidth(s.italic, `italic ${baseFont}`) + measureWidth(s.normal, baseFont);
    } else {
      w = measureWidth(name, baseFont);
    }
    if (showTwo) {
      w += measureWidth('²', '12px Cascadia Code');
    }
    return w;
  }, []);

  const renderedEnzymeLabels = useMemo(() => {
    const hoveredName = hoveredEnzyme ? enzymeLayout.find(l => l.id === hoveredEnzyme)?.name : null;
    return enzymeLayout.map(l => {
      const e = enzymes.find(x => x.id === l.groupId);
      const isGray = e && (e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length));
      const isHoveredGroup = hoveredName != null && l.name === hoveredName;
      const isBlunt = e && e.cutType === 'blunt';
      const isIIS = isIISEnzyme(e);
      const showTwo = totalNameCounts.get(l.name) === 2;
      let labelColor = '#333';
      if (isGray) labelColor = '#9CA3AF';
      else if (isHoveredGroup || selectedEnzymeIds.includes(l.id)) labelColor = enzymeActiveBlue;
      else if (isBlunt) labelColor = '#6B3A2A';
      else if (isIIS) labelColor = '#0D6B6B';
      return (
        <g key={l.id}
          onMouseEnter={() => {
            if (enzymeDragRef.current?.active) {
              // During enzyme drag: highlight and update selection
              enzymeDragRef.current.hoveredId = l.id;
              enzymeDragRef.current.didDrag = true;
              setHoveredEnzyme(l.id);
              // Update selection between start and target cut sites
              const startIdx = enzymeDragRef.current.startCutIdx;
              const targetIdx = l.topCutIndex;
              if (targetIdx !== startIdx) {
                const s = Math.min(startIdx, targetIdx);
                const e = Math.max(startIdx, targetIdx) - 1;
                if (s <= e) { setSelStart(s); setSelEnd(e); setCursorIndex(null); }
                // Both start and target labels selected
                setSelectedEnzymeIds([enzymeDragRef.current.entryId, l.id]);
                enzymeDragRef.current.backToStart = false;
              } else {
                // Same cut site: select recognition site range
                const rs = enzymeDragRef.current.recStart;
                const re = enzymeDragRef.current.recEnd;
                if (rs != null && re != null) { setSelStart(rs); setSelEnd(re); }
                setSelectedEnzymeIds([l.id]);
                enzymeDragRef.current.backToStart = true;
              }
            } else if (!isDraggingRef.current) {
              setHoveredEnzyme(l.id);
            }
          }}
          onMouseLeave={() => {
            if (enzymeDragRef.current?.active && enzymeDragRef.current.hoveredId === l.id) {
              enzymeDragRef.current.hoveredId = null;
            }
            setHoveredEnzyme(null);
          }}
          onMouseDown={(e) => {
            if (e.button !== 0) return;
            e.stopPropagation(); e.preventDefault();
            const enzyme = enzymes.find(x => x.id === l.groupId);
            if (!enzyme) return;
            const pairs = enzyme.cutPairs || [{ topCutIndex: enzyme.cutIndex, botCutIndex: enzyme.botCutIndex }];
            const isCutTwice = pairs.length > 1;
            const cutIdx = l.topCutIndex;

            // Shift+click: extend from previous enzyme selection
            if (e.shiftKey && lastEnzymeSelRef.current && lastEnzymeSelRef.current.cutIdx !== cutIdx) {
              const prevCutIdx = lastEnzymeSelRef.current.cutIdx;
              const prevEntryId = lastEnzymeSelRef.current.entryId;
              const s = Math.min(prevCutIdx, cutIdx);
              const ed = Math.max(prevCutIdx, cutIdx) - 1;
              if (s <= ed) {
                setSelStart(s); setSelEnd(ed);
                setCursorIndex(null);
                setIsEnzymeSelection(true);
                setSelectedEnzymeIds(prevEntryId ? [prevEntryId, l.id] : [l.id]);
                setHoveredEnzyme(l.id);
                clearCursorTimer();
                lastEnzymeSelRef.current = { enzymeId: l.groupId, cutIdx, name: l.name, entryId: l.id };
              }
              return;
            }

            // Cut-twice enzyme: directly select between two cut positions
            if (isCutTwice) {
              const otherPair = pairs[l.pairIndex === 0 ? 1 : 0];
              const cut1 = cutIdx;
              const cut2 = otherPair.topCutIndex;
              const s = Math.min(cut1, cut2);
              const ed = Math.max(cut1, cut2) - 1;
              // Find the other pair's layout entry for dual selection
              const otherEntryId = `${l.groupId}_p${l.pairIndex === 0 ? 1 : 0}`;
              setSelStart(s); setSelEnd(ed);
              setCursorIndex(null);
              setIsEnzymeSelection(true);
              setSelectedEnzymeIds([l.id, otherEntryId]);
              setHoveredEnzyme(l.id);
              clearCursorTimer();
              lastEnzymeSelRef.current = { enzymeId: l.groupId, cutIdx, name: l.name, entryId: l.id };
              return;
            }

            // Start enzyme drag — immediately select recognition site range
            setSelStart(enzyme.displayStart);
            setSelEnd(enzyme.displayEnd);
            setCursorIndex(null);
            enzymeDragRef.current = {
              active: true,
              startEnzymeId: l.groupId,
              startName: l.name,
              startCutIdx: cutIdx,
              recStart: enzyme.displayStart,
              recEnd: enzyme.displayEnd,
              didDrag: false,
              backToStart: false,
              hoveredId: l.id,
              entryId: l.id,
            };
            isDraggingRef.current = true;
            setIsDragging(true);
            setIsEnzymeDragging(true);
            setIsEnzymeSelection(true);
            setSelectedEnzymeIds([l.id]);
            setHoveredEnzyme(l.id);
            clearCursorTimer();
          }}
          style={{ cursor: 'pointer' }}>
          <rect x={l.cutX + 3} y={l.yTop - 10} width={l.enzW + (showTwo ? 10 : 2)} height={18} fill="transparent" />
          {(() => {
            const enzText = { x: l.cutX + 6, y: l.yTop + 5, fontSize: "14px", fontFamily: "Cascadia Code", fontWeight: l.isUnique ? '700' : '350', style: { pointerEvents: 'none' } };
            const nameContent = (() => { const s = splitEnzName(l.name); return s.normal ? [<tspan key="i" fontStyle="italic">{s.italic}</tspan>, <tspan key="n">{s.normal}</tspan>] : l.name; })();
            const content = showTwo ? [...(Array.isArray(nameContent) ? nameContent : [nameContent]), <tspan key="two" fontSize="12" dy="-2">²</tspan>] : nameContent;
            const isEnzActive = selectedEnzymeIds.includes(l.id);
            return <>
              {isEnzActive ? (
                <>
                  <rect x={l.cutX + 2} y={l.yTop - 11} width={exactLabelW(l.name, l.isUnique, showTwo) + 8} height={22}
                    fill={enzymeActiveBlue} rx="3" style={{ pointerEvents: 'none' }} />
                  <text {...enzText} fill={bgColor} stroke="none" style={{ pointerEvents: 'none' }}>{content}</text>
                </>
              ) : (
                <>
                  <text {...enzText} fill="none" stroke={bgColor} strokeWidth="5">{content}</text>
                  <text {...enzText} fill={labelColor} stroke="none">{content}</text>
                </>
              )}
            </>;
          })()}
        </g>
      );
    });
  }, [enzymeLayout, enzymes, hoveredEnzyme, bgColor, selectedEnzymeIds, clearCursorTimer, exactLabelW]);

  const renderedEnzymeOverlay = useMemo(() => {
    // Collect enzyme names to render lines for (from hover or selected ids)
    const namesToRender = new Set();
    if (hoveredEnzyme) {
      const entry = enzymeLayout.find(l => l.id === hoveredEnzyme);
      if (entry) namesToRender.add(entry.name);
    }
    for (const id of selectedEnzymeIds) {
      const entry = enzymeLayout.find(l => l.id === id);
      if (entry) namesToRender.add(entry.name);
    }
    if (namesToRender.size === 0) return null;

    // Compute hover text content (only for hovered enzyme)
    let hoverTextContent = null;
    if (hoveredEnzyme) {
      const hoveredEntry = enzymeLayout.find(l => l.id === hoveredEnzyme);
      if (hoveredEntry) {
        const e = enzymes.find(x => x.name === hoveredEntry.name);
        if (e) {
          const isGray = e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length);
          const ovColor = isGray ? '#9CA3AF' : enzymeActiveBlue;
          const showTwoOv = totalNameCounts.get(hoveredEntry.name) === 2;
          const ovNameContent = (() => {
            const s = splitEnzName(e.name);
            const parts = s.normal ? [<tspan key="i" fontStyle="italic">{s.italic}</tspan>, <tspan key="n">{s.normal}</tspan>] : [e.name];
            if (showTwoOv) parts.push(<tspan key="two" fontSize="12" dy="-2">²</tspan>);
            return parts;
          })();
          const methParts = [];
          if (e.methylationBlocked && e.methylationSources?.length) {
            methParts.push('[' + e.methylationSources.join('/') + ' Blocked]');
          }
          if (e.methylationRequired && e.methylRequiredSources?.length) {
            methParts.push('[' + e.methylRequiredSources.join('/') + ' Required]');
          }
          const methText = methParts.length ? '  ' + methParts.join(' ') : '';
          hoverTextContent = { hoveredEntry, ovColor, ovNameContent, methText };
        }
      }
    }

    return (
      <g style={{ pointerEvents: 'none' }}>
        {/* Render lines for each enzyme name */}
        {[...namesToRender].map(name => {
          const e = enzymes.find(x => x.name === name);
          if (!e) return null;
          const isGray = e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length);
          const ovColor = isGray ? '#9CA3AF' : enzymeActiveBlue;
          const nameEntries = enzymeLayout.filter(l => l.name === name);
          return nameEntries.map(l => (
            <React.Fragment key={`ov-${l.id}`}>
              <line x1={l.cutX} x2={l.cutX} y1={l.yTop} y2={l.sy + 5} stroke={bgColor} strokeWidth="6" strokeLinecap="square" />
              <line x1={l.cutX} x2={l.cutX} y1={l.yTop} y2={l.sy + 5} stroke={ovColor} strokeWidth={e.isUnique ? '2' : '1'} />
            </React.Fragment>
          ));
        })}
        {/* Hover text */}
        {hoverTextContent && (
          <React.Fragment>
            <text x={hoverTextContent.hoveredEntry.cutX + 6} y={hoverTextContent.hoveredEntry.yTop + 5} fill="none" stroke={bgColor} strokeWidth="5"
              fontSize="14px" fontFamily="Cascadia Code" fontWeight={hoverTextContent.hoveredEntry.isUnique ? '700' : '350'}>
              {hoverTextContent.ovNameContent}
              {hoverTextContent.methText}
            </text>
            <text x={hoverTextContent.hoveredEntry.cutX + 6} y={hoverTextContent.hoveredEntry.yTop + 5} fill={hoverTextContent.ovColor} stroke="none"
              fontSize="14px" fontFamily="Cascadia Code" fontWeight={hoverTextContent.hoveredEntry.isUnique ? '700' : '350'}>
              {hoverTextContent.ovNameContent}
              {hoverTextContent.methText}
            </text>
          </React.Fragment>
        )}
      </g>
    );
  }, [hoveredEnzyme, selectedEnzymeIds, enzymeLayout, enzymes, bgColor, totalNameCounts]);

  const renderedTooltips = useMemo(() => {
    if (!hoveredEnzyme || isEnzymeDragging) return null;
    const hoveredEntry = enzymeLayout.find(l => l.id === hoveredEnzyme);
    if (!hoveredEntry) return null;
    const e = enzymes.find(x => x.id === hoveredEntry.groupId);
    if (!e || e.displayStart === undefined) return null;
    const isGray = e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length);
    const ttColor = isGray ? '#9CA3AF' : (isEnzymeDragging ? enzymeActiveBlue : '#2563EB');

    const dispLen = e.displayEnd - e.displayStart + 1;
    const sw = e.isUnique ? '2' : '1';
    const pad = 6;
    const ttH = 44;
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

    const groupEntries = enzymeLayout.filter(l => l.groupId === hoveredEntry.groupId);

    return (
      <g style={{ pointerEvents: 'none' }}>
        {groupEntries.map((entry) => {
          const sy = entry.sy;
          const ttY = sy - 19;
          const hp = cutPairs[entry.pairIndex] || cutPairs[0];
          const charsBeforeCut = hp.topCutIndex - e.displayStart;
          const baseX = entry.cutX - charsBeforeCut * cw;
          const leftX = baseX - pad;
          const ttW = dispLen * cw + pad * 2;

          const polyEntries = cutPairs.map((cp, i) => {
            const tGapX = baseX + (cp.topCutIndex - e.displayStart) * cw;
            const bGapX = baseX + (cp.botCutIndex - e.displayStart) * cw;
            const isLocal = i === entry.pairIndex;
            return {
              tGapX, bGapX, isLocal,
              path: [
                `M ${tGapX} ${ttY - 2}`,
                `L ${tGapX} ${sy + 3}`,
                `L ${bGapX} ${sy + 3}`,
                `L ${bGapX} ${sy + 19}`,
              ].join(' '),
            };
          });

          const uniqueGapXs = [...new Set(polyEntries.map(pe => pe.tGapX))].sort((a, b) => a - b);
          const gapHalfW = 4;
          const r = 8;
          let borderD = `M ${leftX + r} ${ttY}`;
          let curX = leftX + r;
          for (const gx of uniqueGapXs) {
            if (gx - gapHalfW > curX) {
              borderD += ` L ${gx - gapHalfW} ${ttY}`;
            }
            borderD += ` M ${gx + gapHalfW} ${ttY}`;
            curX = gx + gapHalfW;
          }
          if (curX < leftX + ttW - r) {
            borderD += ` L ${leftX + ttW - r} ${ttY}`;
          }
          borderD += ` A ${r} ${r} 0 0 1 ${leftX + ttW} ${ttY + r}`;
          borderD += ` L ${leftX + ttW} ${ttY + ttH - r}`;
          borderD += ` A ${r} ${r} 0 0 1 ${leftX + ttW - r} ${ttY + ttH}`;
          borderD += ` L ${leftX + r} ${ttY + ttH}`;
          borderD += ` A ${r} ${r} 0 0 1 ${leftX} ${ttY + ttH - r}`;
          borderD += ` L ${leftX} ${ttY + r}`;
          borderD += ` A ${r} ${r} 0 0 1 ${leftX + r} ${ttY}`;

          return (
            <g key={`tt-${entry.id}`}>
              <rect x={leftX} y={ttY} width={ttW} height={ttH} rx={8} fill="#FFFFFF" stroke="none" />
              <path d={borderD} fill="none" stroke={ttColor} strokeWidth={sw} strokeLinejoin="round" />
              {polyEntries.map((pe, i) => (
                <path key={`poly-${i}`} d={pe.path} fill="none" stroke={ttColor} strokeWidth={sw} strokeLinejoin="round" strokeLinecap="round" />
              ))}
              <text y={sy} fontFamily={monoFont} fontSize="14px">
                {sub.split('').map((c, i) => {
                  const bold = isRecBold(i);
                  return (
                    <tspan key={i} x={baseX + i * cw + cw / 2} textAnchor="middle"
                      fontWeight={bold ? '700' : '200'} fill={bold ? '#1f2937' : '#BFBFBF'}>{c}</tspan>
                  );
                })}
              </text>
              <text y={sy + 16} fontFamily={monoFont} fontSize="14px">
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
  }, [hoveredEnzyme, enzymeLayout, enzymes, cleanSeq, charsPerLine, isEnzymeDragging]);

  // --- cursor & selection renderers ---
  const renderedCursor = useMemo(() => {
    if (cursorIndex === null) return null;
    if (hasSelection && !isDragging) return null;
    const row = Math.floor(cursorIndex / charsPerLine);
    const col = cursorIndex % charsPerLine;
    const x = getX(col);
    const sy = getSeqY(row);
    const topY = sy - rowAbove[row];
    const botY = row === numRows - 1 ? sy + rowBelow[row] + 24 : getSeqY(row + 1) - rowAbove[row + 1];
    return (
      <g style={{ pointerEvents: 'none' }}>
        <line x1={x} x2={x} y1={topY} y2={botY} stroke={bgColor} strokeWidth="3" />
        <line x1={x} x2={x} y1={topY} y2={botY} stroke={currentSelColor} strokeWidth="1.5" />
      </g>
    );
  }, [cursorIndex, hasSelection, isDragging, charsPerLine, numRows, getSeqY, rowBelow]);

  const renderedSelection = useMemo(() => {
    if (!hasSelection) return null;
    const segs = sp(selStart, selEnd);
    return (
      <g style={{ pointerEvents: 'none' }}>
        {segs.map(seg => (
          <rect key={`selbg-${seg.row}-${seg.colStart}`}
            x={getX(seg.colStart)} y={getSeqY(seg.row) - 19}
            width={(seg.colEnd - seg.colStart + 1) * cw} height={28}
            fill={currentSelColor} rx="1" />
        ))}
      </g>
    );
  }, [hasSelection, selStart, selEnd, getSeqY, sp, currentSelColor]);

  const renderedSeq = useMemo(() => {
    const vs = Math.max(0, visibleRows.start - ROW_BUF);
    const ve = Math.min(numRows - 1, visibleRows.end + ROW_BUF);
    const rows = [];
    for (let r = vs; r <= ve; r++) {
      const rowStart = r * charsPerLine;
      const rowEnd = Math.min(cleanSeq.length, (r + 1) * charsPerLine);
      const chunk = cleanSeq.substring(rowStart, rowEnd);
      const sy = getSeqY(r);
      const chars = chunk.split('');
      const selInRow = hasSelection
        ? { s: Math.max(selStart, rowStart) - rowStart, e: Math.min(selEnd, rowEnd - 1) - rowStart }
        : null;
      const inSel = (i) => selInRow && i >= selInRow.s && i <= selInRow.e;
      rows.push(
        <text key={r} y={sy} fontFamily={monoFont} fontSize="14px" fontWeight="bold"
          style={{ userSelect: 'none', cursor: 'text' }}>
          {chars.map((c, i) => (
            <tspan key={i} x={getX(i) + cw / 2} textAnchor="middle"
              fill={inSel(i) ? bgColor : '#1f2937'}>{c}</tspan>
          ))}
        </text>
      );
    }
    return rows;
  }, [visibleRows, charsPerLine, numRows, cleanSeq, hasSelection, selStart, selEnd, getSeqY]);

  return (
    <div ref={containerRef} style={{ backgroundColor: bgColor, width: '100%', minHeight: '100vh', display: 'flex', justifyContent: 'center', alignItems: 'flex-start', padding: '0 1rem 4rem 1rem', overflowX: 'auto', userSelect: 'none', contain: 'layout style' }}>
      <div style={{ width: svgWidth }}>
        <svg ref={svgRef} width="100%" height={svgHeight} style={{ display: 'block', overflow: 'visible', willChange: 'transform', transform: 'translateZ(0)' }}
          onMouseDown={handleSvgMouseDown}>
          {renderedCursor}
          {renderedSelection}
          {renderedFeatures}
          {renderedFeatureLabels}
          {renderedEnzymes}
          {renderedPrimers}
          {renderedEnzymeLabels}
          {renderedEnzymeOverlay}
          {renderedSeq}
          {renderedTooltips}
        </svg>
      </div>
    </div>
  );
});

export default SequenceEditor;