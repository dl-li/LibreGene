import { useMemo, useRef, useState, useCallback } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { bgColor, featLabelW, featureSelRange, rangeLen } from './editorConstants';

const TWO_PI = Math.PI * 2;
// Full-turn arcs are split at ~π: nudge the split off the antipodal endpoints
// so large-arc-flag is well-defined per the SVG spec.
const ARC_EPS = 1e-4;

// --- circular geometry helpers ---
// angle θ: 0 = top, clockwise, one full turn = sequence length
const pol = (cx, cy, r, th) => [cx + r * Math.sin(th), cy - r * Math.cos(th)];

// arrowhead base flares this many px beyond the band on each side
const HEAD_FLARE = 4;

function arcArrowPath(cx, cy, rOut, rIn, th0, th1, strand) {
  const rMid = (rOut + rIn) / 2;
  const head = Math.min(((rOut - rIn + HEAD_FLARE * 2) * 1.1) / rMid, (th1 - th0) * 0.6);
  if (th1 - th0 < 1e-4) return '';
  const large = th1 - th0 - head > Math.PI ? 1 : 0;
  if (strand === '-') {
    const [x0o, y0o] = pol(cx, cy, rOut, th0 + head);
    const [x1o, y1o] = pol(cx, cy, rOut, th1);
    const [x1i, y1i] = pol(cx, cy, rIn, th1);
    const [x0i, y0i] = pol(cx, cy, rIn, th0 + head);
    const [x0of, y0of] = pol(cx, cy, rOut + HEAD_FLARE, th0 + head);
    const [x0if, y0if] = pol(cx, cy, rIn - HEAD_FLARE, th0 + head);
    const [tx, ty] = pol(cx, cy, rMid, th0);
    return `M ${x0o} ${y0o} A ${rOut} ${rOut} 0 ${large} 1 ${x1o} ${y1o} L ${x1i} ${y1i} A ${rIn} ${rIn} 0 ${large} 0 ${x0i} ${y0i} L ${x0if} ${y0if} L ${tx} ${ty} L ${x0of} ${y0of} Z`;
  }
  const [x0o, y0o] = pol(cx, cy, rOut, th0);
  const [x1o, y1o] = pol(cx, cy, rOut, th1 - head);
  const [x1i, y1i] = pol(cx, cy, rIn, th1 - head);
  const [x0i, y0i] = pol(cx, cy, rIn, th0);
  const [x1of, y1of] = pol(cx, cy, rOut + HEAD_FLARE, th1 - head);
  const [x1if, y1if] = pol(cx, cy, rIn - HEAD_FLARE, th1 - head);
  const [tx, ty] = pol(cx, cy, rMid, th1);
  return `M ${x0o} ${y0o} A ${rOut} ${rOut} 0 ${large} 1 ${x1o} ${y1o} L ${x1of} ${y1of} L ${tx} ${ty} L ${x1if} ${y1if} L ${x1i} ${y1i} A ${rIn} ${rIn} 0 ${large} 0 ${x0i} ${y0i} Z`;
}

function arcSectorPath(cx, cy, rOut, rIn, th0, th1) {
  // A full-turn arc has identical start/end points and renders as nothing.
  // Draw the outer and inner circles as one path with opposite winding
  // (nonzero fill-rule fills the annulus) — no radial seam lines.
  if (th1 - th0 >= TWO_PI - 1e-6) {
    const a1 = th0 + Math.PI + ARC_EPS;
    const [x0o, y0o] = pol(cx, cy, rOut, th0);
    const [x1o, y1o] = pol(cx, cy, rOut, a1);
    const [x0i, y0i] = pol(cx, cy, rIn, th0);
    const [x1i, y1i] = pol(cx, cy, rIn, a1);
    return (
      `M ${x0o} ${y0o} A ${rOut} ${rOut} 0 1 1 ${x1o} ${y1o} A ${rOut} ${rOut} 0 0 1 ${x0o} ${y0o} Z ` +
      `M ${x0i} ${y0i} A ${rIn} ${rIn} 0 0 0 ${x1i} ${y1i} A ${rIn} ${rIn} 0 1 0 ${x0i} ${y0i} Z`
    );
  }
  const [x0o, y0o] = pol(cx, cy, rOut, th0);
  const [x1o, y1o] = pol(cx, cy, rOut, th1);
  const [x1i, y1i] = pol(cx, cy, rIn, th1);
  const [x0i, y0i] = pol(cx, cy, rIn, th0);
  const large = th1 - th0 > Math.PI ? 1 : 0;
  return `M ${x0o} ${y0o} A ${rOut} ${rOut} 0 ${large} 1 ${x1o} ${y1o} L ${x1i} ${y1i} A ${rIn} ${rIn} 0 ${large} 0 ${x0i} ${y0i} Z`;
}

function arcPath(cx, cy, r, th0, th1) {
  // Full-turn arc: split into two arcs (same degenerate-endpoint issue),
  // nudged off the antipodal endpoints to keep large-arc-flag well-defined.
  if (th1 - th0 >= TWO_PI - 1e-6) {
    const mid = (th0 + th1) / 2 + ARC_EPS;
    return arcPath(cx, cy, r, th0, mid) + ' ' + arcPath(cx, cy, r, mid, th1);
  }
  const [x0, y0] = pol(cx, cy, r, th0);
  const [x1, y1] = pol(cx, cy, r, th1);
  const large = th1 - th0 > Math.PI ? 1 : 0;
  return `M ${x0} ${y0} A ${r} ${r} 0 ${large} 1 ${x1} ${y1}`;
}

function featureSegments(f) {
  return f.segments?.length ? f.segments : [{ start: f.start, end: f.end }];
}

const SEL_BROWN = '#3E2723';

// Selection pieces in sequence coords: a selection with start > end wraps the
// origin of a circular sequence.
function selPieces(sel, len) {
  if (!sel || sel.start == null || sel.end == null) return [];
  return sel.start <= sel.end
    ? [[sel.start, sel.end]]
    : [
        [sel.start, len - 1],
        [0, sel.end],
      ];
}

const segInSel = (s, sel, len) => selPieces(sel, len).some(([a, b]) => s.start <= b && a <= s.end);

// normalize segments: split wrap-around (end < start) at the origin
function normSegments(f, len) {
  const out = [];
  for (const s of featureSegments(f)) {
    if (s.end >= s.start) {
      out.push(s);
    } else {
      out.push({ start: s.start, end: len - 1 });
      out.push({ start: 0, end: s.end });
    }
  }
  return out.sort((a, b) => a.start - b.start);
}

// Unwrap origin-crossing segments onto a monotone coordinate line (values may
// exceed len-1), preserving join order, then merge touching pieces into runs.
// A feature crossing the origin becomes one continuous run.
function unwrapRuns(f, len) {
  const runs = [];
  let off = 0;
  let prevStart = -Infinity;
  for (const s of featureSegments(f)) {
    let a = s.start + off;
    let b = s.end + off;
    if (b < a) b += len; // the segment itself crosses the origin
    if (a < prevStart) {
      off += len;
      a += len;
      b += len;
    }
    prevStart = a;
    const last = runs[runs.length - 1];
    if (last && a <= last.end + 1) last.end = Math.max(last.end, b);
    else runs.push({ start: a, end: b });
  }
  return runs;
}

const featTotalLen = (f, len) => normSegments(f, len).reduce((a, s) => a + s.end - s.start + 1, 0);

// suppress the arrowhead when the tip end is covered by a longer feature
function tipBuried(f, features, len) {
  // Tip in join order: for '-' the first segment's start, for '+' the last
  // segment's end (origin-wrapping features keep their biological direction).
  const raw = featureSegments(f);
  const tip = f.strand === '-' ? raw[0].start : raw[raw.length - 1].end;
  const fl = featTotalLen(f, len);
  return features.some(
    (g) =>
      g.id !== f.id &&
      featTotalLen(g, len) > fl &&
      normSegments(g, len).some((s) => tip >= s.start && tip <= s.end),
  );
}

// SnapGene-style label layout: split labels into right/left halves by angle,
// stack each half vertically along a label circle with a minimum gap, and
// connect each label to its feature with an elbow (radial + bent) polyline.
// Layout space is centered at (0,0); the viewBox is sized to fit everything.
function layoutCircularLabels(features, length, R, half) {
  const labelR = R + 40;
  const GAP = 17;
  const items = features.map((f) => {
    // Midpoint along the unwrapped span: origin-crossing features unwrap to a
    // coordinate line past len, so the midpoint lands on the feature's actual
    // arc instead of the opposite side of the circle.
    const runs = unwrapRuns(f, length);
    const mid = ((runs[0].start + runs[runs.length - 1].end + 1) / 2) % length;
    const th = (mid / length) * TWO_PI;
    return { f, th, w: featLabelW(f.name) };
  });
  for (const sign of [1, -1]) {
    const arr = items
      .filter((it) => (Math.sin(it.th) >= 0 ? 1 : -1) === sign)
      .sort((a, b) => -Math.cos(a.th) + Math.cos(b.th));
    for (const it of arr) it.ly = -labelR * Math.cos(it.th);
    for (let i = 1; i < arr.length; i++) arr[i].ly = Math.max(arr[i].ly, arr[i - 1].ly + GAP);
    // Recenter the stack within the label circle as far as possible; labels that
    // still overflow go into a vertical side column instead of collapsing onto
    // the vertical centerline (which crosses the circle and looks broken)
    if (arr.length) {
      const overBottom = arr[arr.length - 1].ly - labelR;
      if (overBottom > 0) {
        const headroom = arr[0].ly + labelR;
        const shift = -Math.min(overBottom, Math.max(headroom, 0));
        for (const it of arr) it.ly += shift;
      }
      const overTop = -labelR - arr[0].ly;
      if (overTop > 0) {
        const headroom = labelR - arr[arr.length - 1].ly;
        const shift = Math.min(overTop, Math.max(headroom, 0));
        for (const it of arr) it.ly += shift;
      }
    }
    for (const it of arr) {
      const ay = Math.abs(it.ly);
      // Labels within ~37° of the top/bottom centerline join the side column,
      // so the column and the circle positions blend into one continuous line
      it.lx =
        ay <= labelR * 0.8 ? sign * Math.sqrt(labelR * labelR - ay * ay) : sign * labelR * 0.85;
      it.side = sign;
    }
  }
  let minX = -(R + half);
  let maxX = R + half;
  let minY = -(R + half);
  let maxY = R + half;
  for (const it of items) {
    const tx0 = it.side > 0 ? it.lx + 6 : it.lx - 6 - it.w;
    minX = Math.min(minX, tx0);
    maxX = Math.max(maxX, tx0 + it.w);
    minY = Math.min(minY, it.ly - 9);
    maxY = Math.max(maxY, it.ly + 9);
  }
  const pad = 14;
  return {
    labels: items,
    labelR,
    viewBox: { x: minX - pad, y: minY - pad, w: maxX - minX + pad * 2, h: maxY - minY + pad * 2 },
  };
}

export function CircularMap({
  length,
  features,
  name,
  selection,
  onSelect,
  onClear,
  onFeatureOpen,
  bg = bgColor,
}) {
  const R = 150;
  const half = 9;
  const svgRef = useRef(null);
  const dragRef = useRef(null); // { anchor, moved } in sequence coords
  const suppressClickRef = useRef(false);
  const [dragSel, setDragSel] = useState(null); // { start, end } while dragging
  const [hoverId, setHoverId] = useState(null);

  const { labels, labelR, viewBox } = useMemo(
    () => layoutCircularLabels(features, length, R, half),
    [features, length],
  );
  const cx = 0;
  const cy = 0;

  const toLocal = useCallback(
    (e) => {
      const rect = svgRef.current.getBoundingClientRect();
      return [
        viewBox.x + ((e.clientX - rect.left) / rect.width) * viewBox.w - cx,
        viewBox.y + ((e.clientY - rect.top) / rect.height) * viewBox.h - cy,
      ];
    },
    [viewBox, cx, cy],
  );
  const toPos = useCallback(
    (e) => {
      const [dx, dy] = toLocal(e);
      let th = Math.atan2(dx, -dy);
      if (th < 0) th += TWO_PI;
      return Math.min(length - 1, Math.floor((th / TWO_PI) * length));
    },
    [toLocal, length],
  );

  const handlePointerDown = (e) => {
    if (e.button !== 0) return;
    const [dx, dy] = toLocal(e);
    const dist = Math.hypot(dx, dy);
    if (dist < R - 30 || dist > R + 30) return;
    dragRef.current = { anchor: toPos(e), moved: false };
    svgRef.current.setPointerCapture(e.pointerId);
  };
  const handlePointerMove = (e) => {
    const d = dragRef.current;
    if (!d) return;
    const pos = toPos(e);
    if (pos !== d.anchor) d.moved = true;
    if (d.moved) {
      const a = d.anchor;
      setDragSel(a <= pos ? { start: a, end: pos } : { start: pos, end: a });
    }
  };
  const handlePointerUp = () => {
    const d = dragRef.current;
    dragRef.current = null;
    if (!d) return;
    if (d.moved && dragSel) {
      onSelect(dragSel.start, dragSel.end);
      suppressClickRef.current = true;
    }
    setDragSel(null);
  };

  const sel = dragSel || selection;
  const selTh =
    sel && sel.start != null && sel.end != null
      ? [
          (sel.start / length) * TWO_PI,
          ((sel.end + 1) / length) * TWO_PI + (sel.end < sel.start ? TWO_PI : 0),
        ]
      : null;

  return (
    <svg
      ref={svgRef}
      viewBox={`${viewBox.x} ${viewBox.y} ${viewBox.w} ${viewBox.h}`}
      className="w-full select-none touch-none"
      style={{ background: bg }}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onClick={() => {
        if (suppressClickRef.current) {
          suppressClickRef.current = false;
          return;
        }
        onClear?.();
      }}
    >
      <circle cx={cx} cy={cy} r={R} fill="none" stroke="#8a8577" strokeWidth="1.5" />
      {selTh && (
        <path
          d={arcPath(cx, cy, R, selTh[0], selTh[1])}
          fill="none"
          stroke={SEL_BROWN}
          strokeWidth={half * 2 + 8}
          pointerEvents="none"
        />
      )}
      {features.map((f) => {
        const runs = unwrapRuns(f, length);
        const hasDir = f.strand === '+' || f.strand === '-';
        const buried = hasDir && tipBuried(f, features, length);
        const hovered = hoverId === f.id;
        const runPath = (r, i) => {
          const th0 = (r.start / length) * TWO_PI;
          const th1 = ((r.end + 1) / length) * TWO_PI;
          const isTip = hasDir && !buried && (f.strand === '-' ? i === 0 : i === runs.length - 1);
          return isTip
            ? arcArrowPath(cx, cy, R + half, R - half, th0, th1, f.strand)
            : arcSectorPath(cx, cy, R + half, R - half, th0, th1);
        };
        return (
          <g
            key={f.id}
            pointerEvents="visiblePainted"
            style={{ cursor: 'pointer' }}
            onPointerDown={(e) => {
              e.stopPropagation();
              dragRef.current = null;
            }}
            onClick={(e) => {
              e.stopPropagation();
              const [fs, fe] = featureSelRange(f);
              onSelect(fs, fe);
            }}
            onDoubleClick={(e) => {
              e.stopPropagation();
              onFeatureOpen(f);
            }}
            onMouseEnter={() => setHoverId(f.id)}
            onMouseLeave={() => setHoverId(null)}
          >
            {runs.map((r, i) => (
              <path
                key={i}
                d={runPath(r, i)}
                fill={f.color || '#9e9e9e'}
                fillOpacity={hovered ? 1 : 0.85}
                stroke="#333"
                strokeWidth={hovered ? 1.8 : 1.2}
              />
            ))}
            {sel &&
              runs.map((r, i) => {
                const fd = runPath(r, i);
                // Overlap in unwrapped space: selection pieces are compared at
                // the base offset and one turn up, so origin-crossing runs and
                // origin-crossing selections both match.
                const pieces = selPieces(sel, length).flatMap(([a, b]) => [
                  [a, b],
                  [a + length, b + length],
                ]);
                return pieces.map(([ps, pe], pi) => {
                  const o0 = Math.max(r.start, ps);
                  const o1 = Math.min(r.end, pe);
                  if (o0 > o1) return null;
                  const clipId = `mapov-${f.id}-${i}-${pi}`;
                  return (
                    <g key={`ov-${i}-${pi}`} pointerEvents="none">
                      <clipPath id={clipId}>
                        <path
                          d={arcSectorPath(
                            cx,
                            cy,
                            R + half + HEAD_FLARE,
                            R - half - HEAD_FLARE,
                            (o0 / length) * TWO_PI,
                            ((o1 + 1) / length) * TWO_PI,
                          )}
                        />
                      </clipPath>
                      <path
                        d={fd}
                        fill={SEL_BROWN}
                        stroke={bgColor}
                        strokeWidth="1.2"
                        clipPath={`url(#${clipId})`}
                      />
                    </g>
                  );
                });
              })}
          </g>
        );
      })}
      {labels.map(({ f, th, lx, ly, side }) => {
        const [ex, ey] = pol(cx, cy, labelR - 12, th);
        return (
          <polyline
            key={`lead-${f.id}`}
            points={`${pol(cx, cy, R + half + 2, th)[0]},${pol(cx, cy, R + half + 2, th)[1]} ${ex},${ey} ${lx + side * 3},${ly}`}
            fill="none"
            stroke="#bbb"
            strokeWidth="0.7"
            pointerEvents="none"
          />
        );
      })}
      {labels.map(({ f, lx, ly, side }) => {
        const hovered = hoverId === f.id;
        const fSel = featureSegments(f).some((s) => segInSel(s, sel, length));
        return (
          <g
            key={f.id}
            style={{ cursor: 'pointer' }}
            onPointerDown={(e) => {
              e.stopPropagation();
              dragRef.current = null;
            }}
            onClick={(e) => {
              e.stopPropagation();
              const [fs, fe] = featureSelRange(f);
              onSelect(fs, fe);
            }}
            onDoubleClick={(e) => {
              e.stopPropagation();
              onFeatureOpen(f);
            }}
            onMouseEnter={() => setHoverId(f.id)}
            onMouseLeave={() => setHoverId(null)}
          >
            <text
              x={lx + side * 6}
              y={ly}
              textAnchor={side > 0 ? 'start' : 'end'}
              dominantBaseline="middle"
              fontSize="12"
              fontWeight={hovered || fSel ? '700' : '600'}
              fontFamily="TeX Gyre Heros"
              fill={fSel ? SEL_BROWN : hovered ? '#000' : '#333'}
              stroke={bgColor}
              strokeWidth="3"
              strokeLinejoin="round"
              paintOrder="stroke"
            >
              {f.name}
            </text>
          </g>
        );
      })}
      <text
        x={cx}
        y={cy - 11}
        textAnchor="middle"
        dominantBaseline="central"
        fontSize="17"
        fontWeight="bold"
        fontFamily="TeX Gyre Heros"
        fill="#222"
        pointerEvents="none"
      >
        {name}
      </text>
      <text
        x={cx}
        y={cy + 11}
        textAnchor="middle"
        dominantBaseline="central"
        fontSize="12"
        fontFamily="TeX Gyre Heros"
        fill="#555"
        pointerEvents="none"
      >
        {length} bp
      </text>
    </svg>
  );
}

export function LinearMap({
  length,
  features,
  selection,
  onSelect,
  onClear,
  onFeatureOpen,
  bg = bgColor,
}) {
  const W = 640;
  const x0 = 24;
  const x1 = W - 24;
  const svgRef = useRef(null);
  const dragRef = useRef(null);
  const suppressClickRef = useRef(false);
  const [dragSel, setDragSel] = useState(null);
  const [hoverId, setHoverId] = useState(null);

  const px = useCallback((pos) => x0 + (pos / length) * (x1 - x0), [length, x0, x1]);

  // label stagger to avoid overlap; x clamped so text stays inside the view
  const labelLevels = useMemo(() => {
    const sorted = features
      .map((f) => {
        // midpoint along the unwrapped span (origin-crossing features land on
        // their actual piece, not the middle of the plasmid)
        const runs = unwrapRuns(f, length);
        return { f, mid: ((runs[0].start + runs[runs.length - 1].end + 1) / 2) % length };
      })
      .sort((a, b) => a.mid - b.mid);
    const levels = [];
    const levelEndX = [];
    for (const item of sorted) {
      const w = featLabelW(item.f.name);
      const cxp = Math.max(w / 2 + 4, Math.min(W - w / 2 - 4, px(item.mid)));
      let lvl = 0;
      while (levelEndX[lvl] !== undefined && cxp - w / 2 < levelEndX[lvl] + 6) lvl++;
      levelEndX[lvl] = cxp + w / 2;
      levels.push({ ...item, lvl, cxp });
    }
    return levels;
  }, [features, px, length]);

  const maxLvl = labelLevels.reduce((m, l) => Math.max(m, l.lvl), 0);
  const lineY = 74 + maxLvl * 16;
  const H = lineY + 34;
  const toPos = useCallback(
    (e) => {
      const rect = svgRef.current.getBoundingClientRect();
      const x = ((e.clientX - rect.left) / rect.width) * W;
      const t = Math.max(0, Math.min(1, (x - x0) / (x1 - x0)));
      return Math.min(length - 1, Math.floor(t * length));
    },
    [length, x0, x1],
  );

  const handlePointerDown = (e) => {
    if (e.button !== 0) return;
    dragRef.current = { anchor: toPos(e), moved: false };
    svgRef.current.setPointerCapture(e.pointerId);
  };
  const handlePointerMove = (e) => {
    const d = dragRef.current;
    if (!d) return;
    const pos = toPos(e);
    if (pos !== d.anchor) d.moved = true;
    if (d.moved) {
      const a = d.anchor;
      setDragSel(a <= pos ? { start: a, end: pos } : { start: pos, end: a });
    }
  };
  const handlePointerUp = () => {
    const d = dragRef.current;
    dragRef.current = null;
    if (d?.moved && dragSel) {
      onSelect(dragSel.start, dragSel.end);
      suppressClickRef.current = true;
    }
    setDragSel(null);
  };

  const sel = dragSel || selection;

  return (
    <svg
      ref={svgRef}
      viewBox={`0 0 ${W} ${H}`}
      className="w-full select-none touch-none"
      style={{ background: bg }}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onClick={() => {
        if (suppressClickRef.current) {
          suppressClickRef.current = false;
          return;
        }
        onClear?.();
      }}
    >
      <line x1={x0} y1={lineY} x2={x1} y2={lineY} stroke="#8a8577" strokeWidth="1.5" />
      {selPieces(sel, length).map(([ps, pe]) => (
        <rect
          key={`sel-${ps}`}
          x={px(ps)}
          y={lineY - 16}
          width={Math.max(1, px(pe + 1) - px(ps))}
          height={32}
          fill={SEL_BROWN}
          pointerEvents="none"
        />
      ))}
      {features.map((f) => {
        const hovered = hoverId === f.id;
        const hasDir = f.strand === '+' || f.strand === '-';
        const buried = hasDir && tipBuried(f, features, length);
        // Origin-crossing features draw as the pieces on each side of the
        // origin; others keep the single bounding polygon.
        const runs = unwrapRuns(f, length);
        const wraps = runs.some((r) => r.end >= length);
        const pieces = [];
        if (wraps) {
          for (const r of runs) {
            pieces.push({ s: r.start, e: Math.min(r.end, length - 1) });
            if (r.end >= length) pieces.push({ s: 0, e: r.end - length });
          }
        } else {
          pieces.push({ s: f.start, e: f.end });
        }
        const piecePts = (p, i) => {
          const fx0 = px(p.s);
          const fx1 = px(p.e + 1);
          const head = Math.min(10, Math.max(2, (fx1 - fx0) / 2));
          const h = 8;
          const hf = h + HEAD_FLARE;
          const isTip = hasDir && !buried && (f.strand === '-' ? i === 0 : i === pieces.length - 1);
          if (!isTip)
            return `${fx0},${lineY - h} ${fx1},${lineY - h} ${fx1},${lineY + h} ${fx0},${lineY + h}`;
          return f.strand !== '-'
            ? `${fx0},${lineY - h} ${fx1 - head},${lineY - h} ${fx1 - head},${lineY - hf} ${fx1},${lineY} ${fx1 - head},${lineY + hf} ${fx1 - head},${lineY + h} ${fx0},${lineY + h}`
            : `${fx1},${lineY - h} ${fx0 + head},${lineY - h} ${fx0 + head},${lineY - hf} ${fx0},${lineY} ${fx0 + head},${lineY + hf} ${fx0 + head},${lineY + h} ${fx1},${lineY + h}`;
        };
        return (
          <g
            key={f.id}
            style={{ cursor: 'pointer' }}
            onPointerDown={(e) => {
              e.stopPropagation();
              dragRef.current = null;
            }}
            onClick={(e) => {
              e.stopPropagation();
              const [fs, fe] = featureSelRange(f);
              onSelect(fs, fe);
            }}
            onDoubleClick={(e) => {
              e.stopPropagation();
              onFeatureOpen(f);
            }}
            onMouseEnter={() => setHoverId(f.id)}
            onMouseLeave={() => setHoverId(null)}
          >
            {pieces.map((p, i) => (
              <polygon
                key={i}
                points={piecePts(p, i)}
                fill={f.color || '#9e9e9e'}
                fillOpacity={hovered ? 1 : 0.85}
                stroke="#333"
                strokeWidth={hovered ? 1.8 : 1.2}
              />
            ))}
            {sel &&
              pieces.map((p, i) =>
                selPieces(sel, length).map(([ps, pe], pi) => {
                  const o0 = Math.max(p.s, ps);
                  const o1 = Math.min(p.e, pe);
                  if (o0 > o1) return null;
                  const clipId = `mapovl-${f.id}-${i}-${pi}`;
                  return (
                    <g key={`ov-${i}-${pi}`} pointerEvents="none">
                      <clipPath id={clipId}>
                        <rect
                          x={px(o0)}
                          y={lineY - 8 - HEAD_FLARE}
                          width={Math.max(1, px(o1 + 1) - px(o0))}
                          height={16 + HEAD_FLARE * 2}
                        />
                      </clipPath>
                      <polygon
                        points={piecePts(p, i)}
                        fill={SEL_BROWN}
                        stroke={bgColor}
                        strokeWidth="1.2"
                        clipPath={`url(#${clipId})`}
                      />
                    </g>
                  );
                }),
              )}
          </g>
        );
      })}
      {labelLevels.map(({ f, lvl, cxp }) => (
        <line
          key={`lead-${f.id}`}
          x1={cxp}
          y1={lineY - 10}
          x2={cxp}
          y2={lineY - 26 - lvl * 16}
          stroke="#aaa"
          strokeWidth="0.7"
          pointerEvents="none"
        />
      ))}
      {labelLevels.map(({ f, lvl, cxp }) => {
        const hovered = hoverId === f.id;
        const fSel = featureSegments(f).some((s) => segInSel(s, sel, length));
        return (
          <g
            key={f.id}
            style={{ cursor: 'pointer' }}
            onPointerDown={(e) => {
              e.stopPropagation();
              dragRef.current = null;
            }}
            onClick={(e) => {
              e.stopPropagation();
              const [fs, fe] = featureSelRange(f);
              onSelect(fs, fe);
            }}
            onDoubleClick={(e) => {
              e.stopPropagation();
              onFeatureOpen(f);
            }}
            onMouseEnter={() => setHoverId(f.id)}
            onMouseLeave={() => setHoverId(null)}
          >
            <text
              x={cxp}
              y={lineY - 30 - lvl * 16}
              textAnchor="middle"
              fontSize="11"
              fontWeight={hovered || fSel ? '700' : '400'}
              fontFamily="TeX Gyre Heros"
              fill={fSel ? SEL_BROWN : hovered ? '#000' : '#333'}
              stroke={bgColor}
              strokeWidth="3"
              strokeLinejoin="round"
              paintOrder="stroke"
            >
              {f.name}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

export default function MapView({
  open,
  onOpenChange,
  sequenceLength,
  features,
  topology,
  name,
  selection,
  onSelect,
  onClear,
  onFeatureOpen,
  moleculeType = 'dna',
  watermark = false,
  onToggleWatermark,
}) {
  const [forceView, setForceView] = useState(null); // null = follow topology
  const view = forceView ?? topology;
  const unit = moleculeType === 'dna' ? 'bp' : moleculeType === 'protein' ? 'aa' : 'nt';
  const sel =
    selection && selection.selStart != null && selection.selEnd != null
      ? { start: selection.selStart, end: selection.selEnd }
      : null;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className={`flex max-h-[85vh] flex-col gap-0 overflow-hidden p-0 [&>button]:top-1 [&>button]:right-2 ${view === 'linear' ? 'sm:max-w-[720px]' : 'sm:max-w-[560px]'}`}
      >
        <DialogHeader className="border-b border-border px-4 py-2.5">
          <DialogTitle>{name}</DialogTitle>
        </DialogHeader>
        <div className="flex-1 overflow-y-auto p-3" style={{ background: bgColor }}>
          {view === 'circular' ? (
            <CircularMap
              length={sequenceLength}
              features={features}
              name={name}
              selection={sel}
              onSelect={onSelect}
              onClear={onClear}
              onFeatureOpen={onFeatureOpen}
            />
          ) : (
            <LinearMap
              length={sequenceLength}
              features={features}
              selection={sel}
              onSelect={onSelect}
              onClear={onClear}
              onFeatureOpen={onFeatureOpen}
            />
          )}
        </div>
        <div className="flex items-center justify-between border-t border-border px-4 py-2 text-xs text-muted-foreground">
          <span>
            {sel
              ? `${sel.start + 1} .. ${sel.end + 1} = ${rangeLen(sel.start, sel.end, sequenceLength)} ${unit}`
              : view === 'circular'
                ? 'Circular'
                : 'Linear'}
          </span>
          <div className="flex items-center gap-4">
            <button
              role="switch"
              aria-checked={watermark}
              onClick={() => onToggleWatermark?.()}
              className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground"
              title="Show the map as a watermark behind the sequence editor"
            >
              <span
                className={`relative inline-flex h-4 w-7 items-center rounded-full transition-colors ${
                  watermark ? 'bg-primary' : 'bg-input'
                }`}
              >
                <span
                  className={`inline-block size-3 rounded-full bg-background shadow transition-transform ${
                    watermark ? 'translate-x-3.5' : 'translate-x-0.5'
                  }`}
                />
              </span>
              <span>Show as Background</span>
            </button>
            <button
              role="switch"
              aria-checked={view === 'linear'}
              onClick={() => setForceView(view === 'circular' ? 'linear' : 'circular')}
              className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground"
              title={view === 'circular' ? 'Switch to linear view' : 'Switch to circular view'}
            >
              <span>Circular</span>
              <span
                className={`relative inline-flex h-4 w-7 items-center rounded-full transition-colors ${
                  view === 'linear' ? 'bg-primary' : 'bg-input'
                }`}
              >
                <span
                  className={`inline-block size-3 rounded-full bg-background shadow transition-transform ${
                    view === 'linear' ? 'translate-x-3.5' : 'translate-x-0.5'
                  }`}
                />
              </span>
              <span>Linear</span>
            </button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
