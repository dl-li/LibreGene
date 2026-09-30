import React, { useMemo, useState } from 'react';
import FornaView from '../plugins/rnaFold/FornaView';
import useRnaFold, { MAX_INTERACTIVE_NT } from '../plugins/rnaFold/useRnaFold';
import { bgColor } from '../editorConstants';

// ---------------------------------------------------------------------------
// FoldWatermark — non-interactive RNA secondary structure rendered as a faint
// overlay (toggled from the RNA Folding dialog footer; mutually exclusive
// with the map watermark). Same fixed-overlay rationale as MapWatermark.
// Selected bases are highlighted: dark-brown circle, letter in bgColor.
// ---------------------------------------------------------------------------
const FoldWatermark = React.memo(function FoldWatermark({ sequence, selStart, selEnd }) {
  const { result } = useRnaFold(sequence, !!sequence && sequence.length <= MAX_INTERACTIVE_NT);
  // Sized to the viewport (the overlay is fixed and centered; forna re-fits
  // via its own resize handling).
  const [vp] = useState(() => ({ w: window.innerWidth, h: window.innerHeight }));
  const selRanges = useMemo(() => {
    if (selStart == null || selEnd == null) return null;
    if (selStart <= selEnd) return [[selStart, selEnd]];
    // Circular wrap-around selection: highlight both arms.
    return [
      [selStart, sequence.length - 1],
      [0, selEnd],
    ];
  }, [selStart, selEnd, sequence.length]);
  if (!sequence || !result) return null;
  return (
    <div
      aria-hidden
      className="[&_*]:pointer-events-none"
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 10,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        pointerEvents: 'none',
      }}
    >
      <div style={{ width: Math.round(vp.w * 0.85), opacity: 0.1 }}>
        <FornaView
          sequence={sequence}
          structure={result.structure}
          height={Math.round(vp.h * 0.8)}
          settleMs={4000}
          interactive={false}
          selectionRanges={selRanges}
          selectionTextColor={bgColor}
        />
      </div>
    </div>
  );
});

export default FoldWatermark;
