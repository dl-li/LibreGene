import { useEffect, useRef } from 'react';
import useDotplot from './useDotplot';
import { tickStep, MAX_DOTS } from './dotplot';

const MARGIN = { top: 8, right: 12, bottom: 52, left: 64 };
const DIRECT_COLOR = 'rgba(22, 163, 74, 0.6)';
const REVCOMP_COLOR = 'rgba(147, 51, 234, 0.6)';

function geometry(canvas, sequenceLength) {
  const cssW = canvas.clientWidth;
  const cssH = canvas.clientHeight;
  const availW = cssW - MARGIN.left - MARGIN.right;
  const availH = cssH - MARGIN.top - MARGIN.bottom;
  const side = Math.max(Math.min(availW, availH), 0);
  return {
    cssW,
    cssH,
    side,
    x0: MARGIN.left + (availW - side) / 2,
    y0: MARGIN.top + (availH - side) / 2,
    sequenceLength,
  };
}

function resizeCanvas(canvas) {
  const dpr = window.devicePixelRatio || 1;
  const width = Math.round(canvas.clientWidth * dpr);
  const height = Math.round(canvas.clientHeight * dpr);
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  const ctx = canvas.getContext('2d');
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return ctx;
}

function drawStatic(canvas, seq, name, result) {
  if (!canvas.clientWidth || !canvas.clientHeight) return;
  const ctx = resizeCanvas(canvas);
  const { cssW, cssH, side, x0, y0 } = geometry(canvas, seq.length);
  const { direct, revcomp } = result;
  const directCount = direct.length / 2;
  const revcompCount = revcomp.length / 2;
  ctx.clearRect(0, 0, cssW, cssH);
  if (!side) return;

  const fg = window.getComputedStyle(canvas).color;
  ctx.font = '11px system-ui, sans-serif';
  ctx.strokeStyle = fg;
  ctx.lineWidth = 1;
  ctx.strokeRect(x0 + 0.5, y0 + 0.5, side - 1, side - 1);
  const step = tickStep(seq.length);
  ctx.beginPath();
  for (let v = step; v < seq.length; v += step) {
    const x = x0 + (v / seq.length) * side;
    const y = y0 + (v / seq.length) * side;
    ctx.moveTo(x, y0);
    ctx.lineTo(x, y0 - 4);
    ctx.moveTo(x, y0 + side);
    ctx.lineTo(x, y0 + side + 4);
    ctx.moveTo(x0, y);
    ctx.lineTo(x0 - 4, y);
  }
  ctx.stroke();

  ctx.fillStyle = fg;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'top';
  for (let v = step; v < seq.length; v += step) {
    ctx.fillText(v.toLocaleString(), x0 + (v / seq.length) * side, y0 + side + 8);
  }
  ctx.textAlign = 'right';
  ctx.textBaseline = 'middle';
  for (let v = step; v < seq.length; v += step) {
    ctx.fillText(v.toLocaleString(), x0 - 8, y0 + (v / seq.length) * side);
  }
  ctx.textAlign = 'center';
  ctx.textBaseline = 'alphabetic';
  ctx.font = '12px system-ui, sans-serif';
  ctx.fillText(`${name} (${seq.length.toLocaleString()} nt)`, cssW / 2, cssH - 8);
  ctx.save();
  ctx.translate(16, cssH / 2);
  ctx.rotate(-Math.PI / 2);
  ctx.fillText(`${name} (${seq.length.toLocaleString()} nt)`, 0, 0);
  ctx.restore();

  const cw = Math.max(side / seq.length, 1.5);
  const ch = Math.max(side / seq.length, 1.5);
  ctx.fillStyle = DIRECT_COLOR;
  for (let k = 0; k < directCount; k++) {
    ctx.fillRect(
      x0 + (direct[k * 2] / seq.length) * side,
      y0 + (direct[k * 2 + 1] / seq.length) * side,
      cw,
      ch,
    );
  }
  ctx.fillStyle = REVCOMP_COLOR;
  for (let k = 0; k < revcompCount; k++) {
    ctx.fillRect(
      x0 + (revcomp[k * 2] / seq.length) * side,
      y0 + (revcomp[k * 2 + 1] / seq.length) * side,
      cw,
      ch,
    );
  }
}

export default function DotplotCanvas({ seq, name, windowSize }) {
  const staticRef = useRef(null);
  const overlayRef = useRef(null);
  const hoverRef = useRef(null);
  const frameRef = useRef(null);
  const { result, pending } = useDotplot(seq, windowSize);
  const { direct, revcomp, truncated } = result;
  const directCount = direct.length / 2;
  const revcompCount = revcomp.length / 2;
  const dotCount = directCount + revcompCount;

  useEffect(() => {
    const canvas = staticRef.current;
    if (!canvas) return undefined;
    const draw = () => drawStatic(canvas, seq, name, result);
    draw();
    const ro = new ResizeObserver(draw);
    ro.observe(canvas);
    const mo = new MutationObserver(draw);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });
    return () => {
      ro.disconnect();
      mo.disconnect();
    };
  }, [seq, name, result]);

  useEffect(() => {
    const canvas = overlayRef.current;
    if (!canvas) return undefined;
    const draw = () => {
      frameRef.current = null;
      const ctx = resizeCanvas(canvas);
      const { cssW, cssH, side, x0, y0 } = geometry(canvas, seq.length);
      ctx.clearRect(0, 0, cssW, cssH);
      const hover = hoverRef.current;
      if (
        !side ||
        !hover ||
        hover.x < x0 ||
        hover.x > x0 + side ||
        hover.y < y0 ||
        hover.y > y0 + side
      )
        return;
      const posX = Math.min(Math.floor(((hover.x - x0) / side) * seq.length), seq.length - 1);
      const posY = Math.min(Math.floor(((hover.y - y0) / side) * seq.length), seq.length - 1);
      const fg = window.getComputedStyle(canvas).color;
      ctx.strokeStyle = fg;
      ctx.globalAlpha = 0.5;
      ctx.setLineDash([5, 4]);
      ctx.beginPath();
      ctx.moveTo(hover.x, y0);
      ctx.lineTo(hover.x, y0 + side);
      ctx.moveTo(x0, hover.y);
      ctx.lineTo(x0 + side, hover.y);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.globalAlpha = 1;
      ctx.fillStyle = fg;
      ctx.font = '11px system-ui, sans-serif';
      ctx.textAlign = 'left';
      ctx.textBaseline = 'bottom';
      ctx.fillText(`(${posX + 1}, ${posY + 1})`, hover.x + 8, hover.y - 4);
    };
    const schedule = () => {
      if (frameRef.current === null) frameRef.current = requestAnimationFrame(draw);
    };
    const onMove = (event) => {
      const rect = canvas.getBoundingClientRect();
      hoverRef.current = { x: event.clientX - rect.left, y: event.clientY - rect.top };
      schedule();
    };
    const onLeave = () => {
      hoverRef.current = null;
      schedule();
    };
    canvas.addEventListener('mousemove', onMove);
    canvas.addEventListener('mouseleave', onLeave);
    const ro = new ResizeObserver(schedule);
    ro.observe(canvas);
    return () => {
      canvas.removeEventListener('mousemove', onMove);
      canvas.removeEventListener('mouseleave', onLeave);
      ro.disconnect();
      if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    };
  }, [seq]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-1.5">
      <div className="relative min-h-0 flex-1">
        <canvas ref={staticRef} className="absolute inset-0 size-full text-muted-foreground" />
        <canvas ref={overlayRef} className="absolute inset-0 size-full text-muted-foreground" />
      </div>
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
        <span>
          {pending
            ? 'Computing…'
            : dotCount === 0
              ? 'No matches at this window size'
              : `${dotCount.toLocaleString()} matching ${windowSize}-mers (${directCount.toLocaleString()} direct, ${revcompCount.toLocaleString()} rev-comp)`}
        </span>
        <span className="flex items-center gap-3">
          <span className="flex items-center gap-1">
            <span className="size-1.5 rounded-full" style={{ backgroundColor: DIRECT_COLOR }} />
            direct match
          </span>
          <span className="flex items-center gap-1">
            <span className="size-1.5 rounded-full" style={{ backgroundColor: REVCOMP_COLOR }} />
            rev-comp
          </span>
          {truncated && <span>Showing the first {MAX_DOTS.toLocaleString()} dots</span>}
        </span>
      </div>
    </div>
  );
}
