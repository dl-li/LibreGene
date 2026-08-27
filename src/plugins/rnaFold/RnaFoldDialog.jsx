import { useEffect, useMemo, useState } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { LoaderCircle } from 'lucide-react';
import FornaView from './FornaView';

const MAX_INTERACTIVE_NT = 1500;
// foldSeq is a synchronous main-thread WASM call (Zuker DP, O(n³) time /
// O(n²) memory). A multi-kb RNA freezes the whole webview — including any
// agent tabs sharing the process — and tens of kb can OOM it. Cap the fold
// itself; anything longer should go to an external tool.
const MAX_FOLD_NT = 3000;

function countPairs(db) {
  return db.split('').filter((c) => c === ')').length;
}

export default function RnaFoldDialog({ open, onOpenChange, sequence }) {
  const [result, setResult] = useState(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open || !sequence) return;
    if (sequence.length > MAX_FOLD_NT) {
      setResult(null);
      setError(
        `Sequence too long to fold in-app (${sequence.length} nt > ${MAX_FOLD_NT} nt). ` +
          'Folding runs on the UI thread and would freeze the app; use an external tool for long RNAs.',
      );
      return;
    }
    setBusy(true);
    setError('');
    // Lazy-load the ~130 kB wasm bundle only when the dialog opens.
    // ribossfold-wasm is CommonJS; the pre-bundled ESM only has a default export.
    import('ribossfold-wasm')
      .then((m) => (m.fold ?? m.default.fold)(sequence))
      .then(setResult)
      .catch((e) => setError(String(e?.message || e)))
      .finally(() => setBusy(false));
  }, [open, sequence]);

  const pairCount = useMemo(() => (result ? countPairs(result.structure) : 0), [result]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-4xl max-h-[85vh] flex flex-col px-8">
        <DialogHeader>
          <DialogTitle>RNA Secondary Structure</DialogTitle>
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
      </DialogContent>
    </Dialog>
  );
}
