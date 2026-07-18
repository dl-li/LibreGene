import { useRef, useEffect, useState } from 'react';

const W = 16;
// Approx offset from document top to first sequence row (baseSeqY ≈ 100 minus rowAbove spacing)
const CONTENT_OFFSET = 72;

export default function FeatureScrollbar({ scrollContainerRef, features, sequenceLength }) {
  const rootRef = useRef(null);
  const canvasRef = useRef(null);
  const thumbRef = useRef(null);
  const [metrics, setMetrics] = useState({ y: 0, vh: 200, sh: 200 });
  const [barH, setBarH] = useState(200);
  const [isDragging, setIsDragging] = useState(false);
  const dragOffRef = useRef(0);
  const tickRef = useRef(false);
  const metricsRef = useRef(metrics);
  const barHRef = useRef(barH);
  metricsRef.current = metrics;
  barHRef.current = barH;

  useEffect(() => {
    const el = scrollContainerRef?.current;
    if (!el) return;
    const sync = () => setMetrics({ y: el.scrollTop, vh: el.clientHeight, sh: el.scrollHeight });
    const onScroll = () => {
      if (!tickRef.current) {
        tickRef.current = true;
        requestAnimationFrame(() => { sync(); tickRef.current = false; });
      }
    };
    sync();
    el.addEventListener('scroll', onScroll, { passive: true });
    return () => el.removeEventListener('scroll', onScroll);
  }, [scrollContainerRef]);

  useEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => setBarH(entry.contentRect.height));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    const cvs = canvasRef.current;
    if (!cvs || !sequenceLength || !barH) return;
    const dpr = window.devicePixelRatio || 1;
    cvs.width = Math.round(W * dpr);
    cvs.height = Math.round(barH * dpr);
    cvs.style.width = W + 'px';
    cvs.style.height = barH + 'px';
    const ctx = cvs.getContext('2d');
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, W, barH);
    for (const f of features || []) {
      const segs = (f.segments && f.segments.length)
        ? f.segments
        : [{ start: f.start, end: f.end }];
      for (const s of segs) {
        const c = s.color || f.color || '#60A5FA';
        const y1 = (Math.max(0, s.start) / sequenceLength) * barH;
        const y2 = (Math.min(sequenceLength, s.end + 1) / sequenceLength) * barH;
        ctx.globalAlpha = 0.35;
        ctx.fillStyle = c;
        ctx.fillRect(0, y1, W, Math.max(1.5, y2 - y1));
      }
    }
    ctx.globalAlpha = 1;
  }, [features, sequenceLength, barH]);

  const { y: scrollY, vh: viewportH, sh: scrollH } = metrics;
  const adjY = Math.max(0, scrollY - CONTENT_OFFSET);
  const docRange = scrollH - viewportH;
  const adjRange = Math.max(0, docRange - CONTENT_OFFSET * 2);
  const canScroll = adjRange > 0 && docRange > 0;
  const thumbH = canScroll ? Math.max(24, (viewportH / scrollH) * barH) : barH;
  const thumbTop = canScroll ? (adjY / adjRange) * (barH - thumbH) : 0;

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
      const relY = e.clientY - rr.top - dragOffRef.current;
      const { sh, vh } = metricsRef.current;
      const dr = sh - vh;
      const ar = Math.max(0, dr - CONTENT_OFFSET * 2);
      if (ar <= 0) return;
      const th = Math.max(24, (vh / sh) * barHRef.current);
      const trackH = rr.height - th;
      if (trackH <= 0) return;
      const ratio = Math.max(0, Math.min(1, relY / trackH));
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
    const rr = root.getBoundingClientRect();
    const trackH = rr.height - thumbH;
    if (trackH <= 0) return;
    const relY = e.clientY - rr.top - thumbH / 2;
    const ratio = Math.max(0, Math.min(1, relY / trackH));
    el.scrollTop = Math.round(CONTENT_OFFSET + ratio * adjRange);
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
          height: thumbH,
          top: thumbTop,
          background: isDragging
            ? 'rgba(0,0,0,0.35)'
            : 'rgba(0,0,0,0.2)',
          backdropFilter: 'blur(6px)',
          WebkitBackdropFilter: 'blur(6px)',
          transition: isDragging ? 'none' : 'background 0.15s',
        }}
        onMouseDown={onThumbDown}
      />
    </div>
  );
}
