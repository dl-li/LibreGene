import { useMemo } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { LoaderCircle } from 'lucide-react';
import FornaView from './FornaView';
import useRnaFold, { countPairs, MAX_INTERACTIVE_NT } from './useRnaFold';

export default function RnaFoldDialog({
  open,
  onOpenChange,
  sequence,
  fileName,
  watermark = false,
  onToggleWatermark,
}) {
  const { result, error, busy } = useRnaFold(sequence, open);

  const pairCount = useMemo(() => (result ? countPairs(result.structure) : 0), [result]);
  const canWatermark = !!result && sequence.length <= MAX_INTERACTIVE_NT;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-4xl max-h-[85vh] flex flex-col px-8">
        <DialogHeader>
          <DialogTitle>RNA Secondary Structure{fileName ? ` — ${fileName}` : ''}</DialogTitle>
        </DialogHeader>

        {busy && (
          <div className="flex items-center gap-2 py-12 justify-center text-muted-foreground">
            <LoaderCircle className="size-4 animate-spin" />
            <span className="text-sm">Folding {sequence?.length ?? 0} nt…</span>
          </div>
        )}
        {error && <p className="py-8 text-center text-sm text-destructive">{error}</p>}

        {!busy && !error && result && (
          <>
            <div className="flex gap-6 pb-3 text-sm">
              <span>
                MFE <span className="font-mono font-semibold">{result.mfe.toFixed(2)}</span>{' '}
                kcal/mol
              </span>
              <span className="text-muted-foreground">
                {pairCount} base pairs / {sequence.length} nt · Turner 2004 (RibossFold)
              </span>
            </div>
            <div className="flex-1 overflow-auto -mx-8 px-8">
              {sequence.length <= MAX_INTERACTIVE_NT ? (
                <FornaView sequence={sequence} structure={result.structure} />
              ) : (
                <p className="py-6 text-center text-sm text-muted-foreground">
                  Sequence too long for interactive layout ({sequence.length} nt &gt;{' '}
                  {MAX_INTERACTIVE_NT} nt).
                </p>
              )}
              <div className="mt-4 select-all whitespace-pre-wrap break-all font-mono text-[11px] leading-5">
                <div>{sequence}</div>
                <div className="text-[#8a5cf6]">{result.structure}</div>
              </div>
            </div>
          </>
        )}

        <DialogFooter className="sm:justify-start">
          <button
            role="switch"
            aria-checked={watermark}
            disabled={!canWatermark}
            onClick={() => onToggleWatermark?.()}
            className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground disabled:opacity-50 disabled:hover:text-muted-foreground"
            title={
              canWatermark
                ? 'Show the folding as a watermark behind the sequence editor'
                : sequence.length > MAX_INTERACTIVE_NT
                  ? `Sequence too long for the background layout (> ${MAX_INTERACTIVE_NT} nt)`
                  : 'Fold the sequence first'
            }
          >
            <span
              className={`relative inline-flex h-4 w-7 items-center rounded-full transition-colors ${
                watermark ? 'bg-primary' : 'bg-input'
              }`}
            >
              <span
                className={`inline-block size-3 rounded-full bg-background shadow transition-transform ${
                  watermark ? 'translate-x-3.5' : 'translate-x-0.5'
                }`}
              />
            </span>
            <span>Show as Background</span>
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
