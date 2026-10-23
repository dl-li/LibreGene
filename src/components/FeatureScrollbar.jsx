import { useRef, useEffect, useState } from 'react';

const W = 16;
// Approx offset from document top to first sequence row (baseSeqY ≈ 100 minus rowAbove spacing)
const CONTENT_OFFSET = 72;

// Vertical feature strip on the right edge of the main page scroller (wrap
// view mode). Continuous mode uses ContinuousOverview instead.
export default function FeatureScrollbar({
  scrollContainerRef,
  features,
  sequenceLength,
  highlightPositions,
}) {
  const rootRef = useRef(null);
  const canvasRef = useRef(null);
  const thumbRef = useRef(null);
  const [metrics, setMetrics] = useState({ pos: 0, vp: 200, sz: 200 });
  const [barLen, setBarLen] = useState(200);
  const [isDragging, setIsDragging] = useState(false);
  const dragOffRef = useRef(0);
  const tickRef = useRef(false);
  const metricsRef = useRef(metrics);
  const barLenRef = useRef(barLen);
  metricsRef.current = metrics;
  barLenRef.current = barLen;

  useEffect(() => {
    const el = scrollContainerRef?.current;
    if (!el) return;
    const sync = () => setMetrics({ pos: el.scrollTop, vp: el.clientHeight, sz: el.scrollHeight });
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
    const ro = new ResizeObserver(([entry]) => setBarLen(entry.contentRect.height));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    const cvs = canvasRef.current;
    if (!cvs || !sequenceLength || !barLen) return;
    const dpr = window.devicePixelRatio || 1;
    cvs.width = Math.round(W * dpr);
    cvs.height = Math.round(barLen * dpr);
    cvs.style.width = W + 'px';
    cvs.style.height = barLen + 'px';
    const ctx = cvs.getContext('2d');
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, W, barLen);
    for (const f of features || []) {
      const segs = f.segments && f.segments.length ? f.segments : [{ start: f.start, end: f.end }];
      for (const s of segs) {
        const c = s.color || f.color || '#60A5FA';
        const p1 = (Math.max(0, s.start) / sequenceLength) * barLen;
        const p2 = (Math.min(sequenceLength, s.end + 1) / sequenceLength) * barLen;
        ctx.globalAlpha = 0.35;
        ctx.fillStyle = c;
        ctx.fillRect(0, p1, W, Math.max(1.5, p2 - p1));
      }
    }
    ctx.globalAlpha = 1;
    if (highlightPositions?.length) {
      ctx.strokeStyle = '#2563EB';
      ctx.lineWidth = 1.5;
      for (const pos of highlightPositions) {
        const p = (pos / sequenceLength) * barLen;
        ctx.beginPath();
        ctx.moveTo(0, p);
        ctx.lineTo(W, p);
        ctx.stroke();
      }
    }
  }, [features, sequenceLength, barLen, highlightPositions]);

  const { pos: scrollPos, vp: viewportLen, sz: scrollLen } = metrics;
  const adjPos = Math.max(0, scrollPos - CONTENT_OFFSET);
  const docRange = scrollLen - viewportLen;
  const adjRange = Math.max(0, docRange - CONTENT_OFFSET * 2);
  const canScroll = adjRange > 0 && docRange > 0;
  const thumbLen = canScroll ? Math.max(24, (viewportLen / scrollLen) * barLen) : barLen;
  const thumbPos = canScroll ? (adjPos / adjRange) * (barLen - thumbLen) : 0;

  const onThumbDown = (e) => {
    e.preventDefault();
    e.stopPropagation();
    const r = thumbRef.current?.getBoundingClientRect();
    if (!r) return;
    dragOffRef.current = e.clientY - r.top;
    setIsDragging(true);
  };

  useEffect(() => {
    if (!isDragging) return;
    const el = scrollContainerRef?.current;
    const root = rootRef.current;
    if (!el || !root) return;
    const onMove = (e) => {
      const rr = root.getBoundingClientRect();
      const rel = e.clientY - rr.top - dragOffRef.current;
      const { sz, vp } = metricsRef.current;
      const dr = sz - vp;
      const ar = Math.max(0, dr - CONTENT_OFFSET * 2);
      if (ar <= 0) return;
      const th = Math.max(24, (vp / sz) * barLenRef.current);
      const track = rr.height - th;
      if (track <= 0) return;
      const ratio = Math.max(0, Math.min(1, rel / track));
      el.scrollTop = Math.round(CONTENT_OFFSET + ratio * ar);
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

  const onTrackDown = (e) => {
    if (!canScroll) return;
    if (thumbRef.current?.contains(e.target)) return;
    const el = scrollContainerRef?.current;
    const root = rootRef.current;
    if (!el || !root) return;
    e.preventDefault();
    const rr = root.getBoundingClientRect();
    const track = rr.height - thumbLen;
    if (track <= 0) return;
    const rel = e.clientY - rr.top - thumbLen / 2;
    const ratio = Math.max(0, Math.min(1, rel / track));
    el.scrollTop = Math.round(CONTENT_OFFSET + ratio * adjRange);
    // Keep the press alive as a drag: the jump just centred the thumb on the
    // pointer, so holding and moving scrubs from there.
    dragOffRef.current = thumbLen / 2;
    setIsDragging(true);
  };

  if (!sequenceLength) return null;

  return (
    <div
      ref={rootRef}
      className="absolute right-0 top-0 bottom-0 z-30 cursor-pointer select-none"
      style={{ width: W }}
      onMouseDown={onTrackDown}
    >
      <canvas ref={canvasRef} className="pointer-events-none absolute inset-0" />
      <div
        ref={thumbRef}
        className="absolute left-0 right-0 z-10"
        style={{
          height: thumbLen,
          top: thumbPos,
          background: isDragging ? 'rgba(0,0,0,0.35)' : 'rgba(0,0,0,0.2)',
          backdropFilter: 'blur(2px)',
          WebkitBackdropFilter: 'blur(2px)',
          transition: isDragging ? 'none' : 'background 0.15s',
        }}
        onMouseDown={onThumbDown}
      />
    </div>
  );
}
