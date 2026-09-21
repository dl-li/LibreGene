import { useEffect, useMemo, useRef, useState } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { InlineNotice } from '@/components/ui/notice';
import {
  buildDotplot,
  reverseComplement,
  normalizeSequence,
  tickStep,
  MIN_WINDOW,
  MAX_WINDOW,
  DEFAULT_WINDOW,
  MAX_DOTS,
} from './dotplot';

const MARGIN = { top: 8, right: 12, bottom: 52, left: 64 };

// Canvas-based plot: dots are drawn imperatively (hundreds of thousands of
// SVG nodes froze WebKit), and the canvas stretches to its flex box so the
// whole plot is always visible without scrolling. The plot itself is a
// centered square with a mouse crosshair instead of grid lines.
function DotplotCanvas({ seq1, seq2, name1, name2, windowSize }) {
  const canvasRef = useRef(null);
  const hoverRef = useRef(null);
  const { dots, truncated } = useMemo(
    () => buildDotplot(seq1, seq2, windowSize),
    [seq1, seq2, windowSize],
  );
  const dotCount = dots.length / 2;

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
      const dot = document.documentElement.classList.contains('dark') ? '#38bdf8' : '#0284c7';
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
      const stepX = tickStep(seq1.length);
      const stepY = tickStep(seq2.length);
      ctx.beginPath();
      for (let v = stepX; v < seq1.length; v += stepX) {
        const x = x0 + (v / seq1.length) * side;
        ctx.moveTo(x, y0 + side);
        ctx.lineTo(x, y0 + side + 4);
      }
      for (let v = stepY; v < seq2.length; v += stepY) {
        const y = y0 + (v / seq2.length) * side;
        ctx.moveTo(x0 - 4, y);
        ctx.lineTo(x0, y);
      }
      ctx.stroke();

      // Tick labels + axis titles.
      ctx.fillStyle = fg;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'top';
      for (let v = stepX; v < seq1.length; v += stepX) {
        ctx.fillText(v.toLocaleString(), x0 + (v / seq1.length) * side, y0 + side + 8);
      }
      ctx.textAlign = 'right';
      ctx.textBaseline = 'middle';
      for (let v = stepY; v < seq2.length; v += stepY) {
        ctx.fillText(v.toLocaleString(), x0 - 8, y0 + (v / seq2.length) * side);
      }
      ctx.textAlign = 'center';
      ctx.textBaseline = 'alphabetic';
      ctx.font = '12px system-ui, sans-serif';
      ctx.fillText(`${name1} (${seq1.length.toLocaleString()} nt)`, x0 + side / 2, cssH - 8);
      ctx.save();
      ctx.translate(16, y0 + side / 2);
      ctx.rotate(-Math.PI / 2);
      ctx.fillText(`${name2} (${seq2.length.toLocaleString()} nt)`, 0, 0);
      ctx.restore();

      // Dots.
      ctx.fillStyle = dot;
      const cw = Math.max(side / seq1.length, 1.5);
      const ch = Math.max(side / seq2.length, 1.5);
      for (let k = 0; k < dotCount; k++) {
        ctx.fillRect(
          x0 + (dots[k * 2] / seq1.length) * side,
          y0 + (dots[k * 2 + 1] / seq2.length) * side,
          cw,
          ch,
        );
      }

      // Mouse crosshair + coordinate readout.
      const hover = hoverRef.current;
      if (hover && hover.x >= x0 && hover.x <= x0 + side && hover.y >= y0 && hover.y <= y0 + side) {
        const posX = Math.min(Math.floor(((hover.x - x0) / side) * seq1.length), seq1.length - 1);
        const posY = Math.min(Math.floor(((hover.y - y0) / side) * seq2.length), seq2.length - 1);
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
  }, [dots, dotCount, seq1, seq2, name1, name2]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-1.5">
      <canvas ref={canvasRef} className="min-h-0 w-full flex-1 text-muted-foreground" />
      <div className="flex shrink-0 items-center justify-between text-[11px] text-muted-foreground">
        <span>
          {dotCount === 0
            ? 'No matches at this window size'
            : `${dotCount.toLocaleString()} matching ${windowSize}-mers`}
        </span>
        {truncated && <span>Showing the first {MAX_DOTS.toLocaleString()} dots</span>}
      </div>
    </div>
  );
}

export default function DotplotDialog({ open, onOpenChange, sequence, fileName }) {
  const [windowSize, setWindowSize] = useState(DEFAULT_WINDOW);
  const [revComp, setRevComp] = useState(false);

  const seq1 = useMemo(() => normalizeSequence(sequence || ''), [sequence]);
  const seq2 = useMemo(() => (revComp ? reverseComplement(seq1) : seq1), [revComp, seq1]);
  const name1 = fileName || 'Sequence 1';
  const name2 = revComp ? `${name1} (rev-comp)` : `${name1} (self)`;

  const tooShort = seq1.length > 0 && seq1.length < windowSize;

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
            <label className="flex items-center gap-2 text-xs text-muted-foreground">
              <Checkbox checked={revComp} onCheckedChange={(v) => setRevComp(!!v)} />
              Reverse complement (reveals inverted repeats)
            </label>
          </div>

          {tooShort && (
            <InlineNotice tone="error">
              The sequence must be at least {windowSize} nt long for a window size of {windowSize}.
            </InlineNotice>
          )}

          {!tooShort && seq1.length > 0 && (
            <DotplotCanvas
              seq1={seq1}
              seq2={seq2}
              name1={name1}
              name2={name2}
              windowSize={windowSize}
            />
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
