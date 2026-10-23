import { useEffect, useRef, useState } from 'react';
import { HEAD_FLARE, tipBuried, unwrapRuns } from '../MapView';

const H = 64;
const MARGIN = 12;
const PAD_X = 14;
const LINE_Y = H / 2;
const BAND_H = 9;
const VIEW_BOX_H = 36;
const MIN_VIEW_W = 24;

// Continuous-mode floating overview: the LinearMap rendering (backbone +
// arrow features, no labels) stretched edge to edge, with the editor's
// visible window marked by a black rectangle. Click/drag scrubs the editor's
// horizontal scroll position.
export default function ContinuousOverview({
  scrollContainerRef,
  features,
  sequenceLength,
  highlightPositions,
  // Width of the collapsed sidebar icon rail overlaying the content's left
  // edge: the widget's left margin is measured from the rail's right edge.
  leftInset = 0,
}) {
  const rootRef = useRef(null);
  const viewRef = useRef(null);
  const [barW, setBarW] = useState(0);
  const [metrics, setMetrics] = useState({ pos: 0, vp: 200, sz: 200 });
  const [isDragging, setIsDragging] = useState(false);
  const dragOffRef = useRef(0);
  const tickRef = useRef(false);
  const metricsRef = useRef(metrics);
  metricsRef.current = metrics;

  useEffect(() => {
    const el = scrollContainerRef?.current;
    if (!el) return;
    const sync = () => setMetrics({ pos: el.scrollLeft, vp: el.clientWidth, sz: el.scrollWidth });
    const onScroll = () => {
      if (!tickRef.current) {
        tickRef.current = true;
        requestAnimationFrame(() => {
          sync();
          tickRef.current = false;
        });
      }
    };
    sync();
    el.addEventListener('scroll', onScroll, { passive: true });
    // display:none <-> visible toggles don't fire scroll events; resync on any size change
    const ro = new ResizeObserver(sync);
    ro.observe(el);
    if (el.firstElementChild) ro.observe(el.firstElementChild);
    return () => {
      el.removeEventListener('scroll', onScroll);
      ro.disconnect();
    };
  }, [scrollContainerRef]);

  useEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => setBarW(entry.contentRect.width));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    if (!isDragging) return;
    const el = scrollContainerRef?.current;
    const root = rootRef.current;
    if (!el || !root) return;
    const onMove = (e) => {
      const rr = root.getBoundingClientRect();
      const ml = rr.width - PAD_X * 2;
      const { sz, vp } = metricsRef.current;
      const sr = sz - vp;
      if (sr <= 0 || ml <= 0) return;
      const vw = Math.max(MIN_VIEW_W, Math.min(ml, (vp / sz) * ml));
      const track = ml - vw;
      if (track <= 0) return;
      const rel = e.clientX - rr.left - PAD_X - dragOffRef.current;
      const ratio = Math.max(0, Math.min(1, rel / track));
      el.scrollLeft = Math.round(ratio * sr);
    };
    const onUp = () => setIsDragging(false);
    document.body.style.cursor = 'grabbing';
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
      document.body.style.cursor = '';
    };
  }, [isDragging, scrollContainerRef]);

  if (!sequenceLength) return null;

  const mapLen = Math.max(0, barW - PAD_X * 2);
  const px = (pos) => PAD_X + (pos / sequenceLength) * mapLen;

  const { pos, vp, sz } = metrics;
  const scrollRange = Math.max(0, sz - vp);
  const canScroll = scrollRange > 0 && mapLen > 0;
  const viewW = canScroll ? Math.max(MIN_VIEW_W, Math.min(mapLen, (vp / sz) * mapLen)) : mapLen;
  const viewX = canScroll
    ? PAD_X + Math.min(1, Math.max(0, pos / scrollRange)) * (mapLen - viewW)
    : PAD_X;

  const onViewDown = (e) => {
    e.preventDefault();
    e.stopPropagation();
    const r = viewRef.current?.getBoundingClientRect();
    if (!r) return;
    dragOffRef.current = e.clientX - r.left;
    setIsDragging(true);
  };

  const onTrackDown = (e) => {
    if (!canScroll) return;
    if (viewRef.current?.contains(e.target)) return;
    const el = scrollContainerRef?.current;
    const root = rootRef.current;
    if (!el || !root) return;
    e.preventDefault();
    const rr = root.getBoundingClientRect();
    const track = mapLen - viewW;
    if (track <= 0) return;
    const rel = e.clientX - rr.left - PAD_X - viewW / 2;
    const ratio = Math.max(0, Math.min(1, rel / track));
    el.scrollLeft = Math.round(ratio * scrollRange);
    // Keep the press alive as a drag: the jump just centred the view box on
    // the pointer, so holding and moving scrubs from there.
    dragOffRef.current = viewW / 2;
    setIsDragging(true);
  };

  return (
    <div
      ref={rootRef}
      className="pointer-events-auto relative cursor-pointer select-none overflow-hidden rounded-xl border border-border/60 bg-background/70 shadow-md backdrop-blur-md transition-shadow duration-200 hover:shadow-lg"
      style={{
        height: H,
        marginLeft: leftInset + MARGIN,
        marginRight: MARGIN,
        marginTop: MARGIN,
      }}
      onMouseDown={onTrackDown}
    >
      {barW > 0 && (
        <svg width={barW} height={H} className="pointer-events-none block">
          <line
            x1={PAD_X}
            y1={LINE_Y}
            x2={barW - PAD_X}
            y2={LINE_Y}
            stroke="#8a8577"
            strokeWidth="1.5"
          />
          {(features || []).map((f) => {
            const hasDir = f.strand === '+' || f.strand === '-';
            const buried = hasDir && tipBuried(f, features, sequenceLength);
            const runs = unwrapRuns(f, sequenceLength);
            const wraps = runs.some((r) => r.end >= sequenceLength);
            const pieces = [];
            if (wraps) {
              for (const r of runs) {
                pieces.push({ s: r.start, e: Math.min(r.end, sequenceLength - 1) });
                if (r.end >= sequenceLength) pieces.push({ s: 0, e: r.end - sequenceLength });
              }
            } else {
              pieces.push({ s: f.start, e: f.end });
            }
            return pieces.map((p, i) => {
              const fx0 = px(p.s);
              const fx1 = px(p.e + 1);
              const head = Math.min(10, Math.max(2, (fx1 - fx0) / 2));
              const h = BAND_H;
              const hf = h + HEAD_FLARE;
              const isTip =
                hasDir && !buried && (f.strand === '-' ? i === 0 : i === pieces.length - 1);
              const points = !isTip
                ? `${fx0},${LINE_Y - h} ${fx1},${LINE_Y - h} ${fx1},${LINE_Y + h} ${fx0},${LINE_Y + h}`
                : f.strand !== '-'
                  ? `${fx0},${LINE_Y - h} ${fx1 - head},${LINE_Y - h} ${fx1 - head},${LINE_Y - hf} ${fx1},${LINE_Y} ${fx1 - head},${LINE_Y + hf} ${fx1 - head},${LINE_Y + h} ${fx0},${LINE_Y + h}`
                  : `${fx1},${LINE_Y - h} ${fx0 + head},${LINE_Y - h} ${fx0 + head},${LINE_Y - hf} ${fx0},${LINE_Y} ${fx0 + head},${LINE_Y + hf} ${fx0 + head},${LINE_Y + h} ${fx1},${LINE_Y + h}`;
              return (
                <polygon
                  key={`${f.id}-${i}`}
                  points={points}
                  fill={f.color || '#9e9e9e'}
                  fillOpacity={0.85}
                  stroke="#333"
                  strokeWidth={1}
                />
              );
            });
          })}
          {(highlightPositions || []).map((p, i) => {
            const x = px(p);
            return (
              <line
                key={`hl-${i}`}
                x1={x}
                y1={LINE_Y - BAND_H - HEAD_FLARE - 3}
                x2={x}
                y2={LINE_Y + BAND_H + HEAD_FLARE + 3}
                stroke="#2563EB"
                strokeWidth="1.5"
              />
            );
          })}
        </svg>
      )}
      <div
        ref={viewRef}
        className="absolute z-10"
        style={{
          left: viewX,
          top: LINE_Y - VIEW_BOX_H / 2,
          width: viewW,
          height: VIEW_BOX_H,
          border: '2px solid #000',
          borderRadius: 6,
          background: isDragging ? 'rgba(0,0,0,0.14)' : 'rgba(0,0,0,0.06)',
          transition: isDragging ? 'none' : 'background 0.15s',
        }}
        onMouseDown={onViewDown}
      />
    </div>
  );
}
