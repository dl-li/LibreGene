import React, { useState, useEffect, useRef, useCallback, useMemo } from 'react';

const cw = 14;
const startX = 220;
const baseSeqY = 100;
const bgColor = '#fdfbf7';
const primerTrackGap = 36;
const monoFont = '"Cascadia Code", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace';

// Canvas-based text measurement cache — replaces name.length * N approximations
const _ctx = typeof document !== 'undefined' ? document.createElement('canvas').getContext('2d') : null;
const _wCache = {};
const measureWidth = (text, font) => {
  if (!_ctx) return text.length * 8; // SSR fallback
  const key = `${font}|${text}`;
  if (_wCache[key] !== undefined) return _wCache[key];
  _ctx.font = font;
  return (_wCache[key] = _ctx.measureText(text).width);
};
const enzLabelW = (name, isUnique) => measureWidth(name, `${isUnique ? '700 ' : '350 '}14px Cascadia Code`) + 4;
const primerLabelW = (name) => measureWidth(name, 'italic 600 12px TeX Gyre Heros');
const sansFont = 'sans-serif';
const springAnim = 'all 0.3s cubic-bezier(0.16, 1, 0.3, 1)';

const getX = (col) => startX + col * cw;
const complement = (c) => c === 'A' ? 'T' : c === 'T' ? 'A' : c === 'G' ? 'C' : c === 'C' ? 'G' : c;

const splitEnzName = (name) => {
  // Italic: everything before the first digit or uppercase letter (beyond position 0)
  let at = name.length;
  for (let i = 1; i < name.length; i++) {
    const c = name[i];
    if ((c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9')) { at = i; break; }
  }
  return { italic: name.slice(0, at), normal: name.slice(at) };
};


function splitRange(start, end, charsPerLine) {
  const segments = [];
  let curr = start;
  while (curr <= end) {
    const row = Math.floor(curr / charsPerLine);
    const rowEnd = Math.min(end, (row + 1) * charsPerLine - 1);
    segments.push({
      row,
      colStart: curr % charsPerLine,
      colEnd: rowEnd % charsPerLine,
      strOffset: curr - start,
      len: rowEnd - curr + 1,
    });
    curr = rowEnd + 1;
  }
  return segments;
}

export default function SequenceEditor({ sequence, features = [], enzymes = [], primers = [], initialCharsPerLine = 60 }) {
  const containerRef = useRef(null);
  const [charsPerLine, setCharsPerLine] = useState(initialCharsPerLine);
  const [hoveredFeature, setHoveredFeature] = useState(null);
  const [hoveredPrimer, setHoveredPrimer] = useState(null);
  const [hoveredEnzyme, setHoveredEnzyme] = useState(null);
  const [scrollY, setScrollY] = useState(0);

  useEffect(() => {
    const handleResize = () => {
      if (containerRef.current) {
        setCharsPerLine(Math.max(20, Math.floor((containerRef.current.clientWidth - startX * 2) / cw)));
      }
    };
    const handleScroll = () => setScrollY(window.scrollY);
    handleResize();
    window.addEventListener('resize', handleResize);
    window.addEventListener('scroll', handleScroll, { passive: true });
    return () => {
      window.removeEventListener('resize', handleResize);
      window.removeEventListener('scroll', handleScroll);
    };
  }, []);

  const cleanSeq = sequence || '';
  const numRows = Math.max(1, Math.ceil(cleanSeq.length / charsPerLine));
  const svgWidth = startX + charsPerLine * cw + startX;

  const sp = useCallback((s, e) => splitRange(s, e, charsPerLine), [charsPerLine]);

  // --- collision avoidance: features + primers ---
  // Normalize features to always have a segments array
  const normFeatures = useMemo(() => (features || []).map(f => {
    if (f.segments && f.segments.length) return f;
    return { ...f, segments: [{ start: f.start, end: f.end }] };
  }), [features]);

  const { processedFeatures, primerTracks } = useMemo(() => {
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

    const pTracks = {};
    for (const type of ['fwd', 'rev']) {
      const ofType = (primers || []).filter(p => p.type === type);
      if (!ofType.length) continue;
      const sorted = [...ofType].sort((a, b) => {
        const la = (a.matchEnd - a.matchStart) + (a.mismatchStr?.length || 0);
        const lb = (b.matchEnd - b.matchStart) + (b.mismatchStr?.length || 0);
        return lb - la || a.matchStart - b.matchStart;
      });
      const tracks = [];
      for (const p of sorted) {
        const ml = p.mismatchStr?.length || 0;
        const vs = p.matchStart - ml, ve = p.matchEnd;
        let placed = false;
        for (let i = 0; i < tracks.length; i++) {
          if (!tracks[i].some(t => !(ve < t.start || vs > t.end))) {
            tracks[i].push({ start: vs, end: ve });
            pTracks[p.id] = i;
            placed = true;
            break;
          }
        }
        if (!placed) {
          tracks.push([{ start: vs, end: ve }]);
          pTracks[p.id] = tracks.length - 1;
        }
      }
    }

    return { processedFeatures: resultFeatures, primerTracks: pTracks };
  }, [features, primers]);

  // --- adaptive row spacing ---
  const rowAbove = useMemo(() => new Array(numRows).fill(0), [numRows]);
  const rowBelow = useMemo(() => new Array(numRows).fill(0), [numRows]);

  for (let r = 0; r < numRows; r++) {
    const rs = r * charsPerLine, re = (r + 1) * charsPerLine - 1;
    let ae = 46, be = 28;

    const rowEnz = enzymes.filter(e => Math.floor(e.cutIndex / charsPerLine) === r);
    if (rowEnz.length) {
      const et = {};
      for (const e of rowEnz) {
        const col = e.cutIndex % charsPerLine;
        let t = 0;
        while (Object.values(et).some(o => o.track === t && Math.abs(o.col - col) < 8)) t++;
        et[e.id] = { track: t, col };
      }
      ae = Math.max(ae, 46 + Math.max(0, ...Object.values(et).map(o => o.track)) * 16);
    }

    for (const p of (primers || [])) {
      const ml = p.mismatchStr?.length || 0;
      const vs = p.type === 'fwd' ? p.matchStart - ml : p.matchStart;
      const ve = p.type === 'rev' ? p.matchEnd + ml : p.matchEnd;
      if (!(ve < rs || vs > re)) {
        const t = primerTracks[p.id] || 0;
        if (p.type === 'fwd') ae = Math.max(ae, 30 + t * primerTrackGap + 23);
        else be = Math.max(be, 26 + t * primerTrackGap + 25);
      }
    }
    for (const f of processedFeatures) {
      for (const seg of f.segments) {
        if (!(seg.end < rs || seg.start > re)) be = Math.max(be, 14 + (f.trackIdx || 0) * 18 + 26);
      }
    }
    rowAbove[r] = ae;
    rowBelow[r] = be;
  }

  const rowY = [Math.max(baseSeqY, rowAbove[0] + 50)];
  for (let r = 0; r < numRows - 1; r++) {
    // Only enforce inter-row gap; rowAbove[r] is accommodated by previous row height
    rowY.push(rowY[r] + Math.max(24, rowBelow[r] + rowAbove[r + 1] + 14));
  }
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
      const r = Math.floor(e.cutIndex / charsPerLine);
      return r >= visibleRows.start && r <= visibleRows.end;
    });
  }, [enzymes, visibleRows, charsPerLine]);
  const svgHeight = rowY[rowY.length - 1] + rowBelow[numRows - 1] + 74;

  // --- shared: highest obstacle Y for enzyme label ---
  const computeHighestY = useCallback((row, cutX, enzNameW) => {
    let hy = getSeqY(row) - 26;
    for (const p of (primers || []).filter(p => p.type === 'fwd')) {
      const segs = sp(p.matchStart, p.matchEnd);
      for (const seg of segs) {
        if (seg.row !== row) continue;
        const ml = p.mismatchStr?.length || 0;
        const isTail = seg === segs[0];
        const nameW = primerLabelW(p.name);
        const nameX = (isTail && ml > 0) ? getX(seg.colStart - ml) : getX(seg.colStart) + cw / 2;
        if (cutX < nameX + nameW + 4 && cutX + enzNameW > nameX) {
          const to = (primerTracks[p.id] || 0) * primerTrackGap;
          hy = Math.min(hy, (isTail && ml > 0 ? getSeqY(row) - 42 - to : getSeqY(row) - 38 - to) - 24);
        }
      }
    }
    return hy;
  }, [primers, primerTracks, charsPerLine, sp, getSeqY]);

  // --- enzyme track assignment ---
  const enzymeTracks = useMemo(() => {
    const tracks = {};
    const occupied = [];
    const sorted = [...enzymes].sort((a, b) => b.cutIndex - a.cutIndex || b.name.length - a.name.length);
    for (const e of sorted) {
      const row = Math.floor(e.cutIndex / charsPerLine);
      const cs = e.cutIndex % charsPerLine;
      const ce = cs + Math.ceil(measureWidth(e.name, '350 14px Cascadia Code') / cw) + 1;
      let t = 0;
      while (occupied.some(o => o.row === row && o.track === t && !(ce < o.cs || cs > o.ce))) t++;
      occupied.push({ row, track: t, cs, ce });
      tracks[e.id] = t;
    }
    return tracks;
  }, [enzymes, charsPerLine]);

  // --- render helpers ---
  const renderFeatures = () => {
    if (!features?.length) return null;
    return processedFeatures.map(f => {
      const isHovered = hoveredFeature === f.id;
      const to = (f.trackIdx || 0) * 18;
      const fc = f.color || '#60A5FA';
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
              visuals.push({ type: 'gap', row: vs.row, colStart: vs.colStart, colEnd: vs.colEnd, showLabel: showL });
            }
          }
        }

        // Solid segment
        for (const vs of sp(ds.start, ds.end)) {
          const showLabel = !seenRows.has(vs.row);
          seenRows.add(vs.row);
          visuals.push({ type: 'solid', row: vs.row, colStart: vs.colStart, colEnd: vs.colEnd, showLabel });
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
            const y = sy + 14 + to;
            const isGap = v.type === 'gap';
            const showLabel = v.showLabel;

            return (
              <g key={`${v.type}-${v.row}-${v.colStart}`}
                onMouseEnter={() => setHoveredFeature(f.id)}
                onMouseLeave={() => setHoveredFeature(null)}
                className="cursor-pointer">
                <rect x={x} y={(isHovered && !isGap) ? sy - 18 : y} width={w}
                  height={(isHovered && !isGap) ? y - (sy - 18) : 0} fill={fc}
                  fillOpacity={isHovered ? (isGap ? 0 : 0.15) : 0}
                  style={{ transition: springAnim }} />
                <line x1={x} x2={x + w} y1={y} y2={y} stroke={fc}
                  strokeWidth="5" opacity={isGap ? 0.25 : 1} />
                {showLabel && (
                  <text x={x - 8} y={y + 4} fill={fc} fontSize="12px" fontFamily="TeX Gyre Heros" fontWeight="600" textAnchor="end">
                    {f.name}</text>
                )}
                <line x1={x} x2={x + w} y1={y} y2={y} stroke="transparent" strokeWidth="20" />
              </g>
            );
          })}
        </g>
      );
    });
  };

  const renderPrimers = () => {
    if (!primers?.length) return null;
    return primers.map((p, idx) => {
      const isFwd = p.type === 'fwd';
      const isHovered = hoveredPrimer === p.id;
      const misLen = p.mismatchStr?.length || 0;
      const hasMis = misLen > 0;
      const pColor = p.color || '#166534';
      const trackOff = (primerTracks[p.id] || 0) * primerTrackGap;
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
            const matchY = (isFwd ? sy - 30 : sy + 26) + (isFwd ? -trackOff : trackOff);
            const misY = matchY + (isFwd ? -4 : 4);
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
            const curExp = isHovered ? 26 : 0;
            const last = pts[pts.length - 1];
            const hoverPath = pathStr +
              ` L ${last[0]} ${last[1] + expD * curExp} ` +
              [...pts].reverse().map(p => `L ${p[0]} ${p[1] + expD * curExp}`).join(' ') + ' Z';

            const arrowPath = isArrow
              ? `M ${isFwd ? x2 + cw : x1} ${matchY} L ${isFwd ? x2 + cw - 7 : x1 + 7} ${matchY + expD * 5}` : '';

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
                        y={misY + (isFwd ? -8 : 18)} textAnchor="middle">···</tspan>}
                      {visMis.split('').map((c, k) => (
                        <tspan key={`mis-${k}`}
                          x={(isFwd ? getX(seg.colStart - drawMisLen + k) : getX(seg.colEnd + drawMisLen - k)) + cw / 2}
                          y={misY + (isFwd ? -8 : 18)} textAnchor="middle">{c}</tspan>
                      ))}
                    </>
                  )}
                  {isFwd
                    ? p.matchStr?.substring(seg.strOffset, seg.strOffset + seg.len).split('').map((c, k) => (
                      <tspan key={`mat-${k}`} x={getX(seg.colStart + k) + cw / 2} y={matchY - 8} textAnchor="middle">{c}</tspan>
                    ))
                    : p.matchStr && Array.from({ length: seg.len }, (_, k) => {
                      const ci = seg.colEnd + seg.row * charsPerLine - k - p.matchStart;
                      return <tspan key={`mat-${k}`} x={getX(seg.colEnd - k) + cw / 2} y={matchY + 18} textAnchor="middle">
                        {p.matchStr[ci] || ''}</tspan>;
                    })
                  }
                </text>

                <path d={pathStr} fill="none" stroke={bgColor} strokeWidth="6" strokeLinejoin="round" />
                {isArrow && <path d={arrowPath} fill="none" stroke={bgColor} strokeWidth="6" strokeLinecap="round" strokeLinejoin="round" />}
                <path d={pathStr} fill="none" stroke={pColor} strokeWidth="2.5" />
                {isArrow && <path d={arrowPath} fill="none" stroke={pColor} strokeWidth="2.5" strokeLinecap="round" />}

                <rect
                  x={(isFwd ? pts[0][0] : pts[0][0] - primerLabelW(p.name)) - 2}
                  y={pts[0][1] + (isFwd ? -18 : 12) - 2}
                  width={primerLabelW(p.name) + 4} height={14} fill={bgColor}
                  style={{ opacity: isHovered ? 0 : 1, transition: springAnim, pointerEvents: 'none' }} />
                <text x={pts[0][0]} y={pts[0][1] + (isFwd ? -8 : 20)}
                  fill={pColor} fontSize="12px" fontFamily="TeX Gyre Heros" fontWeight="600" fontStyle="italic"
                  textAnchor={isFwd ? 'start' : 'end'}
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

  // Pre-compute enzyme geometry (expensive — done once per enzyme-list change, not on hover)
  const enzymeLayout = useMemo(() => {
    return visibleEnzymes.map(e => {
      const row = Math.floor(e.cutIndex / charsPerLine);
      const cutX = getX(e.cutIndex % charsPerLine);
      const sy = getSeqY(row);
      const hy = computeHighestY(row, cutX, enzLabelW(e.name, e.isUnique));
      const to = (enzymeTracks[e.id] || 0) * 16;
      return {
        id: e.id, name: e.name, cutX, sy,
        yTop: hy - 25 - to, isUnique: e.isUnique,
        enzW: enzLabelW(e.name, e.isUnique),
      };
    });
  }, [enzymes, enzymeTracks, charsPerLine, getSeqY, computeHighestY]);

  // Batched enzyme lines — single <path> replaces 571 <line> elements
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

  // Enzyme label backgrounds + hit areas above primers
  const renderEnzymeLabels = () => {
    return enzymeLayout.map(l => (
      <g key={l.id} onMouseEnter={() => setHoveredEnzyme(l.id)} onMouseLeave={() => setHoveredEnzyme(null)}>
        <rect x={l.cutX + 3} y={l.yTop - 10} width={l.enzW + 2} height={18} fill={bgColor} />
        <text x={l.cutX + 6} y={l.yTop + 5}
          fill="#333" fontSize="14px" fontFamily="Cascadia Code"
          fontWeight={l.isUnique ? '700' : '350'} style={{ pointerEvents: 'none' }}>
          {(() => { const s = splitEnzName(l.name); return s.normal ? [<tspan key="i" fontStyle="italic">{s.italic}</tspan>, <tspan key="n">{s.normal}</tspan>] : l.name; })()}
        </text>
      </g>
    ));
  };

  const renderEnzymeOverlay = () => {
    if (!hoveredEnzyme) return null;
    const e = enzymes.find(x => x.id === hoveredEnzyme);
    if (!e) return null;
    const row = Math.floor(e.cutIndex / charsPerLine);
    const cutX = getX(e.cutIndex % charsPerLine);
    const sy = getSeqY(row);
    const hy = computeHighestY(row, cutX, enzLabelW(e.name, e.isUnique));
    const to = (enzymeTracks[e.id] || 0) * 16;
    const yTop = hy - 25 - to;
    const ttTop = sy - 22;  // tooltip polyPath start — overlay meets here
    return (
      <g style={{ pointerEvents: 'none' }}>
        <line x1={cutX} x2={cutX} y1={yTop} y2={ttTop} stroke={bgColor} strokeWidth="6" strokeLinecap="square" />
        <line x1={cutX} x2={cutX} y1={yTop} y2={ttTop} stroke="#2563EB" strokeWidth={e.isUnique ? '2' : '1'} />
        <text x={cutX + 6} y={yTop + 5} fill="#2563EB" fontSize="14px" fontFamily="Cascadia Code"
          fontWeight={e.isUnique ? '700' : '350'}>
            {(() => { const s = splitEnzName(e.name); return s.normal ? [<tspan key="i" fontStyle="italic">{s.italic}</tspan>, <tspan key="n">{s.normal}</tspan>] : e.name; })()}
          </text>
      </g>
    );
  };

  const renderTooltips = () => {
    if (!hoveredEnzyme) return null;
    const e = enzymes.find(x => x.id === hoveredEnzyme);
    if (!e || e.displayStart === undefined) return null;

    const dispLen = e.displayEnd - e.displayStart + 1;
    const sw = e.isUnique ? '2' : '1';
    const swM = e.isUnique ? '3' : '1.5';
    const pad = 8;

    // Anchor: tooltip on the cut's row.  Cut gap MUST land at getX(cutIndex % charsPerLine)
    // to align with the enzyme label vertical line.
    const cutRow = Math.floor(e.cutIndex / charsPerLine);
    const sy = getSeqY(cutRow);
    const topGapX = getX(e.cutIndex % charsPerLine);

    // Work backwards from the cut gap to position the text
    const charsBeforeCut = e.cutIndex - e.displayStart;   // characters in display before the gap
    const baseX = topGapX - charsBeforeCut * cw;           // left edge of first display character
    const leftX = baseX - pad;
    const ttW = dispLen * cw + pad * 2;
    const ttY = sy - 23;
    const ttH = 53;

    // Bottom cut gap — relative to the same text baseline
    const botGapX = baseX + (e.botCutIndex - e.displayStart) * cw;

    // Polyline cut indicator: starts above tooltip, ends before bottom edge
    const polyPath = [
      `M ${topGapX} ${ttY - 2}`,
      `L ${topGapX} ${sy + 3}`,
      `L ${botGapX} ${sy + 3}`,
      `L ${botGapX} ${sy + 24}`,
    ].join(' ');

    const sub = cleanSeq.substring(e.displayStart, e.displayEnd + 1);
    const comp = sub.split('').map(complement).join('');
    const pattern = e.recSeqPattern || '';
    const recOffset = e.recStart - e.displayStart;
    const recLen = e.recEnd - e.recStart + 1;

    // Bold for recognition bases that are NOT N in the pattern
    const isRecBold = (i) => {
      if (i < recOffset || i >= recOffset + recLen) return false;
      const pi = i - recOffset;
      return pi < pattern.length && pattern[pi] !== 'N' && pattern[pi] !== 'n';
    };

    return (
      <g key={`tt-${e.id}`} style={{ pointerEvents: 'none' }}>
        {/* Tooltip box */}
        <rect x={leftX} y={ttY} width={ttW} height={ttH} rx={8} fill="#FFFFFF" stroke="#2563EB" strokeWidth={sw} />
        {/* White notch at top where cut line enters */}
        <line x1={topGapX - 3} x2={topGapX + 3} y1={ttY} y2={ttY} stroke="#FFFFFF" strokeWidth={swM} />

        {/* Cut indicator polyline */}
        <path d={polyPath} fill="none" stroke="#2563EB" strokeWidth={sw} strokeLinejoin="round" strokeLinecap="round" />

        {/* Template sequence — bold=black, light=25% gray. Aligned with main sequence at y=sy */}
        <text y={sy} fontFamily={monoFont} fontSize="16px">
          {sub.split('').map((c, i) => {
            const bold = isRecBold(i);
            return (
              <tspan key={i} x={baseX + i * cw + cw / 2} textAnchor="middle"
                fontWeight={bold ? '700' : '200'} fill={bold ? '#1f2937' : '#BFBFBF'}>{c}</tspan>
            );
          })}
        </text>

        {/* Complement sequence — same weight/color logic */}
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
  };

  const renderSeq = () => (
    Array.from({ length: numRows }, (_, r) => {
      const chunk = cleanSeq.substring(r * charsPerLine, (r + 1) * charsPerLine);
      return (
        <text key={r} y={getSeqY(r)} fontFamily={monoFont} fontSize="16px" fontWeight="bold" fill="#1f2937"
          style={{ userSelect: 'text', cursor: 'text' }}>
          {chunk.split('').map((char, i) => <tspan key={i} x={getX(i) + cw / 2} textAnchor="middle">{char}</tspan>)}
        </text>
      );
    })
  );

  return (
    <div ref={containerRef} style={{ backgroundColor: bgColor, width: '100%', minHeight: '100vh', display: 'flex', justifyContent: 'center', alignItems: 'flex-start', padding: '2rem 2rem 4rem 2rem', overflowX: 'auto', userSelect: 'none' }} className="select-none">
      <div style={{ width: svgWidth }}>
        <svg width="100%" height={svgHeight} style={{ display: 'block', overflow: 'visible' }}>
          {renderFeatures()}
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