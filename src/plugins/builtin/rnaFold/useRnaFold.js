import { useEffect, useRef, useState } from 'react';

export const MAX_INTERACTIVE_NT = 1500;
// foldSeq is a synchronous main-thread WASM call (Zuker DP, O(n³) time /
// O(n²) memory). A multi-kb RNA freezes the whole webview — including any
// agent tabs sharing the process — and tens of kb can OOM it. Cap the fold
// itself; anything longer should go to an external tool.
export const MAX_FOLD_NT = 3000;

export function countPairs(db) {
  return db.split('').filter((c) => c === ')').length;
}

/**
 * Debounced MFE fold of an RNA sequence via ribossfold-wasm (lazy-loaded on
 * first use). Returns { result, error, busy }; result is null while disabled,
 * empty, too long, or stale (a newer fold is pending).
 */
export default function useRnaFold(sequence, enabled, debounceMs = 400) {
  const [result, setResult] = useState(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const reqRef = useRef(0);

  useEffect(() => {
    if (!enabled || !sequence) {
      reqRef.current++;
      setResult(null);
      setError('');
      setBusy(false);
      return undefined;
    }
    if (sequence.length > MAX_FOLD_NT) {
      reqRef.current++;
      setResult(null);
      setBusy(false);
      setError(
        `Sequence too long to fold in-app (${sequence.length} nt > ${MAX_FOLD_NT} nt). ` +
          'Folding runs on the UI thread and would freeze the app; use an external tool for long RNAs.',
      );
      return undefined;
    }
    const req = ++reqRef.current;
    setBusy(true);
    setError('');
    const timer = setTimeout(() => {
      // ribossfold-wasm is CommonJS; the pre-bundled ESM only has a default export.
      import('ribossfold-wasm')
        .then((m) => (m.fold ?? m.default.fold)(sequence))
        .then((r) => {
          if (reqRef.current === req) setResult(r);
        })
        .catch((e) => {
          if (reqRef.current === req) setError(String(e?.message || e));
        })
        .finally(() => {
          if (reqRef.current === req) setBusy(false);
        });
    }, debounceMs);
    return () => clearTimeout(timer);
  }, [sequence, enabled, debounceMs]);

  return { result, error, busy };
}
