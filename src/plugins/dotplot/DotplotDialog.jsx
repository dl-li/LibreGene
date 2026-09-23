import { useEffect, useMemo, useRef, useState } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Label } from '@/components/ui/label';
import { InlineNotice } from '@/components/ui/notice';
import {
  buildDotplot,
  normalizeSequence,
  tickStep,
  MIN_WINDOW,
  MAX_WINDOW,
  DEFAULT_WINDOW,
  MAX_DOTS,
} from './dotplot';

const MARGIN = { top: 8, right: 12, bottom: 52, left: 64 };

const DIRECT_COLOR = 'rgba(22, 163, 74, 0.6)'; // saturated green, semi-transparent
const REVCOMP_COLOR = 'rgba(147, 51, 234, 0.6)'; // saturated purple, semi-transparent

// Canvas-based plot: dots are drawn imperatively (hundreds of thousands of
// SVG nodes froze WebKit), and the canvas stretches to its flex box so the
// whole plot is always visible without scrolling. The plot itself is a
// centered square with a mouse crosshair instead of grid lines.
function DotplotCanvas({ seq, name, windowSize }) {
  const canvasRef = useRef(null);
  const hoverRef = useRef(null);
  const { direct, revcomp, truncated } = useMemo(
    () => buildDotplot(seq, seq, windowSize),
    [seq, windowSize],
  );
  const directCount = direct.length / 2;
  const revcompCount = revcomp.length / 2;
  const dotCount = directCount + revcompCount;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return undefined;
    const ctx = canvas.getContext('2d');
    if (!ctx) return undefined;

    const draw = () => {
      const cssW = canvas.clientWidth;
      const cssH = canvas.clientHeight;
      if (!cssW || !cssH) return;
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.round(cssW * dpr);
      canvas.height = Math.round(cssH * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

      const fg = window.getComputedStyle(canvas).color;
      ctx.font = '11px system-ui, sans-serif';

      // Centered square plot inside the canvas.
      const availW = cssW - MARGIN.left - MARGIN.right;
      const availH = cssH - MARGIN.top - MARGIN.bottom;
      const side = Math.max(Math.min(availW, availH), 0);
      const x0 = MARGIN.left + (availW - side) / 2;
      const y0 = MARGIN.top + (availH - side) / 2;
      if (!side) return;

      // Frame + tick marks, ~8 ticks per axis. No grid lines — the mouse
      // crosshair serves as the position indicator.
      ctx.strokeStyle = fg;
      ctx.lineWidth = 1;
      ctx.strokeRect(x0 + 0.5, y0 + 0.5, side - 1, side - 1);
      const stepX = tickStep(seq.length);
      const stepY = tickStep(seq.length);
      ctx.beginPath();
      for (let v = stepX; v < seq.length; v += stepX) {
        const x = x0 + (v / seq.length) * side;
        ctx.moveTo(x, y0 + side);
        ctx.lineTo(x, y0 + side + 4);
      }
      for (let v = stepY; v < seq.length; v += stepY) {
        const y = y0 + (v / seq.length) * side;
        ctx.moveTo(x0 - 4, y);
        ctx.lineTo(x0, y);
      }
      ctx.stroke();

      // Tick labels + axis titles.
      ctx.fillStyle = fg;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'top';
      for (let v = stepX; v < seq.length; v += stepX) {
        ctx.fillText(v.toLocaleString(), x0 + (v / seq.length) * side, y0 + side + 8);
      }
      ctx.textAlign = 'right';
      ctx.textBaseline = 'middle';
      for (let v = stepY; v < seq.length; v += stepY) {
        ctx.fillText(v.toLocaleString(), x0 - 8, y0 + (v / seq.length) * side);
      }
      ctx.textAlign = 'center';
      ctx.textBaseline = 'alphabetic';
      ctx.font = '12px system-ui, sans-serif';
      ctx.fillText(`${name} (${seq.length.toLocaleString()} nt)`, x0 + side / 2, cssH - 8);
      ctx.save();
      ctx.translate(16, y0 + side / 2);
      ctx.rotate(-Math.PI / 2);
      ctx.fillText(`${name} (${seq.length.toLocaleString()} nt)`, 0, 0);
      ctx.restore();

      // Dots: green for direct window matches, purple for reverse-complement
      // (inverted repeat) matches. Both are semi-transparent, so a window pair
      // satisfying both conditions blends the two colors.
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

      // Mouse crosshair + coordinate readout.
      const hover = hoverRef.current;
      if (hover && hover.x >= x0 && hover.x <= x0 + side && hover.y >= y0 && hover.y <= y0 + side) {
        const posX = Math.min(Math.floor(((hover.x - x0) / side) * seq.length), seq.length - 1);
        const posY = Math.min(Math.floor(((hover.y - y0) / side) * seq.length), seq.length - 1);
        ctx.strokeStyle = fg;
        ctx.globalAlpha = 0.5;
        ctx.setLineDash([4, 4]);
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
        ctx.fillText(
          `${(posX + 1).toLocaleString()}, ${(posY + 1).toLocaleString()}`,
          hover.x + 8,
          hover.y - 6,
        );
      }
    };

    draw();
    const onMove = (e) => {
      const rect = canvas.getBoundingClientRect();
      hoverRef.current = { x: e.clientX - rect.left, y: e.clientY - rect.top };
      draw();
    };
    const onLeave = () => {
      hoverRef.current = null;
      draw();
    };
    canvas.addEventListener('mousemove', onMove);
    canvas.addEventListener('mouseleave', onLeave);
    const ro = new ResizeObserver(draw);
    ro.observe(canvas);
    const mo = new window.MutationObserver(draw);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });
    return () => {
      canvas.removeEventListener('mousemove', onMove);
      canvas.removeEventListener('mouseleave', onLeave);
      ro.disconnect();
      mo.disconnect();
    };
  }, [direct, revcomp, directCount, revcompCount, seq, name]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-1.5">
      <canvas ref={canvasRef} className="min-h-0 w-full flex-1 text-muted-foreground" />
      <div className="flex shrink-0 items-center justify-between gap-4 text-[11px] text-muted-foreground">
        <span>
          {dotCount === 0
            ? 'No matches at this window size'
            : `${dotCount.toLocaleString()} matching ${windowSize}-mers ` +
              `(${directCount.toLocaleString()} direct, ${revcompCount.toLocaleString()} rev-comp)`}
        </span>
        <span className="flex items-center gap-3">
          <span className="flex items-center gap-1">
            <span
              className="inline-block size-2.5 rounded-sm"
              style={{ backgroundColor: DIRECT_COLOR }}
            />
            direct match
          </span>
          <span className="flex items-center gap-1">
            <span
              className="inline-block size-2.5 rounded-sm"
              style={{ backgroundColor: REVCOMP_COLOR }}
            />
            rev-comp match
          </span>
          {truncated && <span>Showing the first {MAX_DOTS.toLocaleString()} dots</span>}
        </span>
      </div>
    </div>
  );
}

export default function DotplotDialog({ open, onOpenChange, sequence, fileName }) {
  const [windowSize, setWindowSize] = useState(DEFAULT_WINDOW);

  const seq = useMemo(() => normalizeSequence(sequence || ''), [sequence]);
  const name = fileName || 'Sequence 1';

  const tooShort = seq.length > 0 && seq.length < windowSize;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[85vh] flex-col overflow-hidden px-8 sm:max-w-4xl">
        <DialogHeader className="shrink-0">
          <DialogTitle>Dotplot</DialogTitle>
        </DialogHeader>

        <div className="flex min-h-0 flex-1 flex-col gap-3">
          <div className="flex shrink-0 flex-wrap items-center gap-x-6 gap-y-2">
            <div className="flex items-center gap-2">
              <Label htmlFor="dotplot-window" className="text-xs text-muted-foreground">
                Window size: {windowSize}
              </Label>
              <input
                id="dotplot-window"
                type="range"
                min={MIN_WINDOW}
                max={MAX_WINDOW}
                value={windowSize}
                onChange={(e) => setWindowSize(Number(e.target.value))}
                className="w-40 accent-primary"
              />
            </div>
          </div>

          {tooShort && (
            <InlineNotice tone="error">
              The sequence must be at least {windowSize} nt long for a window size of {windowSize}.
            </InlineNotice>
          )}

          {!tooShort && seq.length > 0 && (
            <DotplotCanvas seq={seq} name={name} windowSize={windowSize} />
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
