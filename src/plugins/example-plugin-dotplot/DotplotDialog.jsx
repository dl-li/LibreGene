import { useMemo, useState } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle, Label, InlineNotice } from './ui';
import DotplotCanvas from './DotplotCanvas';
import { normalizeSequence, MIN_WINDOW, MAX_WINDOW, DEFAULT_WINDOW } from './dotplot';

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
            <div className="flex min-w-0 flex-1 items-center gap-3">
              <Label htmlFor="dotplot-window" className="shrink-0 text-xs font-medium">
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
