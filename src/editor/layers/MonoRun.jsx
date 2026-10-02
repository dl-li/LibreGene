import { cw, getX } from '../../editorConstants';

// One <tspan> covering `text.length` visually contiguous cw cells starting at
// visual column visStart. The x attribute carries one cell-centre coordinate
// per character, so every glyph is anchored exactly like a per-char tspan —
// no font metrics involved — while the DOM pays for a single element per run
// instead of one per base. (Verified: glyph centres match per-char tspans to
// 0.00px for both the bold 14px template font and the italic 350 13px read
// font; a textLength-based variant drifted when canvas font parsing fell
// back to a proportional font.)
export default function MonoRun({ visStart, text, ...rest }) {
  const n = text.length;
  if (!n) return null;
  const xs = [];
  for (let i = 0; i < n; i++) xs.push(getX(visStart + i) + cw / 2);
  return (
    <tspan x={xs.join(' ')} textAnchor="middle" {...rest}>
      {text}
    </tspan>
  );
}
