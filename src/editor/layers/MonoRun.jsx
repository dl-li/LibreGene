import { cw, getX, measureWidth } from '../../editorConstants';

// One <tspan> covering `text.length` visually contiguous cw cells starting at
// visual column visStart. textLength pins the run to exactly len*cw, so the
// per-cell pitch is exact regardless of font metrics; dx recentres the glyphs
// in their cells (plain lengthAdjust="spacing" would left-align each glyph).
// Collapsing a lane row into a handful of runs instead of one tspan per base
// keeps the DOM small when many alignment lanes are visible at once.
export default function MonoRun({ visStart, text, font, ...rest }) {
  const n = text.length;
  if (!n) return null;
  const adv = measureWidth('M'.repeat(16), font) / 16;
  return (
    <tspan
      x={getX(visStart) + (n * cw) / 2}
      dx={(cw - adv) / 2}
      textAnchor="middle"
      textLength={n * cw}
      lengthAdjust="spacing"
      {...rest}
    >
      {text}
    </tspan>
  );
}
