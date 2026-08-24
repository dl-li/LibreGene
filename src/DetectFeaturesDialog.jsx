import { useCallback, useEffect, useRef, useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { InlineNotice } from '@/components/ui/notice';
import { LoaderCircle, ScanSearch, AlertTriangle } from 'lucide-react';
import { cn } from '@/lib/utils';
import { annotateFeatures } from './tauriApi';
import { locationString0based, locationString1based } from './editorConstants';

const EMPTY_ARRAY = [];

/**
 * A detected hit is "already present" when an existing feature shares its name
 * or covers exactly the same span. Wrapping hits (start > end) compare by the
 * overall bounds of their segments, matching how the model stores join() spans.
 */
function hitExists(hit, existing) {
  const segs = hit.segments?.length ? hit.segments : [{ start: hit.start, end: hit.end }];
  const ds = Math.min(...segs.map((s) => s.start));
  const de = Math.max(...segs.map((s) => s.end));
  return existing.some((f) => f.name === hit.name || (f.start === ds && f.end === de));
}

export default function DetectFeaturesDialog({
  open,
  onOpenChange,
  features = EMPTY_ARRAY,
  onAddFeature,
}) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [items, setItems] = useState(EMPTY_ARRAY); // [{ hit, exists }]
  const [selected, setSelected] = useState(() => new Set());
  const [adding, setAdding] = useState(false);
  const [addError, setAddError] = useState('');
  const reqRef = useRef(0);
  const featuresRef = useRef(features);
  useEffect(() => {
    featuresRef.current = features;
  }, [features]);

  const runDetection = useCallback(() => {
    const req = ++reqRef.current;
    setLoading(true);
    setError('');
    setAddError('');
    setItems(EMPTY_ARRAY);
    setSelected(new Set());
    annotateFeatures()
      .then((list) => {
        if (reqRef.current !== req) return;
        if (list && list.error) {
          setError(String(list.error));
          return;
        }
        const arr = Array.isArray(list) ? list : [];
        const enriched = arr.map((hit) => ({ hit, exists: hitExists(hit, featuresRef.current) }));
        enriched.sort((a, b) => Number(a.hit.fragment) - Number(b.hit.fragment));
        setItems(enriched);
        setSelected(
          new Set(
            enriched.map((it, i) => (it.exists || it.hit.fragment ? -1 : i)).filter((i) => i >= 0),
          ),
        );
      })
      .catch((e) => {
        if (reqRef.current !== req) return;
        setError(e?.message || String(e));
      })
      .finally(() => {
        if (reqRef.current === req) setLoading(false);
      });
  }, []);

  useEffect(() => {
    if (open) runDetection();
  }, [open, runDetection]);

  const toggle = useCallback((i) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(i)) {
        next.delete(i);
      } else {
        next.add(i);
      }
      return next;
    });
  }, []);

  const selectableCount = items.reduce((n, it) => (it.exists ? n : n + 1), 0);
  const allSelected = selectableCount > 0 && selected.size === selectableCount;

  const toggleAll = useCallback(() => {
    setSelected((prev) => {
      if (prev.size === selectableCount) return new Set();
      return new Set(items.map((it, i) => (it.exists ? -1 : i)).filter((i) => i >= 0));
    });
  }, [items, selectableCount]);

  const handleAdd = useCallback(async () => {
    if (!onAddFeature || adding) return;
    const chosen = items.filter((it, i) => selected.has(i));
    if (!chosen.length) return;
    setAdding(true);
    setAddError('');
    const failures = [];
    let lastOk = null;
    let lastOkIndex = -1;
    for (let i = 0; i < chosen.length; i++) {
      const { hit } = chosen[i];
      const feature = {
        id: `feature_${Date.now()}_${i}_${Math.random().toString(36).slice(2, 8)}`,
        name: hit.name,
        ftype: hit.ftype,
        color: hit.color,
        locationStr: locationString0based(hit),
      };
      try {
        await onAddFeature(feature, { recordHistory: i === chosen.length - 1 });
        lastOk = feature;
        lastOkIndex = i;
      } catch (e) {
        failures.push(`${hit.name} (${e?.message || e})`);
      }
    }
    // When the last item failed the batch still needs one history snapshot —
    // re-adding the last successful feature is an idempotent upsert (same id).
    if (lastOk && lastOkIndex !== chosen.length - 1) {
      try {
        await onAddFeature(lastOk, { recordHistory: true });
      } catch (e) {
        console.error('record history error:', e);
      }
    }
    setAdding(false);
    if (failures.length === 0) {
      onOpenChange(false);
    } else {
      setAddError(
        `Failed to add ${failures.length} of ${chosen.length} features: ${failures.join('; ')}`,
      );
    }
  }, [items, selected, adding, onAddFeature, onOpenChange]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl max-h-[80vh] flex flex-col px-8">
        <DialogHeader>
          <DialogTitle>Detect Common Features</DialogTitle>
        </DialogHeader>

        <div className="flex-1 overflow-auto -mx-8 px-8">
          {loading ? (
            <div className="flex items-center justify-center gap-2 py-12 text-sm text-muted-foreground">
              <LoaderCircle className="size-4 animate-spin" />
              Detecting common features…
            </div>
          ) : error ? (
            <div className="flex flex-col items-center gap-3 py-10">
              <div className="flex items-center gap-2 text-sm text-red-600">
                <AlertTriangle className="size-4" />
                <span>{error}</span>
              </div>
              <Button variant="outline" size="sm" onClick={runDetection}>
                Retry
              </Button>
            </div>
          ) : items.length === 0 ? (
            <div className="flex flex-col items-center gap-2 py-12 text-center text-sm text-muted-foreground">
              <ScanSearch className="size-5 opacity-50" />
              <span>No common features detected in this sequence.</span>
            </div>
          ) : (
            <table className="w-full border-collapse text-sm">
              <thead className="[&_th]:sticky [&_th]:top-0 [&_th]:z-10 [&_th]:bg-card">
                <tr className="border-b border-border/60 text-xs uppercase tracking-wider text-muted-foreground">
                  <th className="w-8 py-2 pr-3">
                    <input
                      type="checkbox"
                      checked={allSelected}
                      onChange={toggleAll}
                      disabled={selectableCount === 0}
                      aria-label="Select all"
                    />
                  </th>
                  <th className="w-6 py-2 pr-3" />
                  <th className="py-2 pr-3 text-left font-semibold">Name</th>
                  <th className="py-2 pr-3 text-left font-semibold">Type</th>
                  <th className="py-2 pr-3 text-left font-semibold">Strand</th>
                  <th className="py-2 pr-3 text-left font-semibold">Location</th>
                  <th className="py-2 pr-3 text-left font-semibold">Identity</th>
                  <th className="py-2 text-left font-semibold">Coverage</th>
                </tr>
              </thead>
              <tbody>
                {items.map(({ hit, exists }, i) => (
                  <tr key={i} className={cn('border-b border-border/30', exists && 'opacity-50')}>
                    <td className="py-2 pr-3">
                      <input
                        type="checkbox"
                        checked={selected.has(i)}
                        disabled={exists || adding}
                        onChange={() => toggle(i)}
                        aria-label={`Select ${hit.name}`}
                      />
                    </td>
                    <td className="py-2 pr-3">
                      <span
                        className="inline-block size-3 shrink-0 rounded-sm ring-1 ring-inset ring-black/10"
                        style={{ backgroundColor: hit.color }}
                      />
                    </td>
                    <td className="py-2 pr-3 whitespace-nowrap">
                      <span className="font-medium">{hit.name}</span>
                      {exists && (
                        <span className="ml-1.5 rounded bg-muted px-1.5 py-px text-[10px] text-muted-foreground">
                          Added
                        </span>
                      )}
                      {!exists && hit.fragment && (
                        <span className="ml-1.5 rounded bg-amber-100 px-1.5 py-px text-[10px] text-amber-800">
                          fragment
                        </span>
                      )}
                    </td>
                    <td className="py-2 pr-3 font-mono text-xs">{hit.ftype}</td>
                    <td className="py-2 pr-3">
                      <span
                        className={cn(
                          'font-mono text-xs font-bold',
                          hit.strand === '-' ? 'text-purple-700' : 'text-emerald-700',
                        )}
                      >
                        {hit.strand === '-' ? '−' : '+'}
                      </span>
                    </td>
                    <td className="py-2 pr-3 font-mono text-xs whitespace-nowrap">
                      {locationString1based(hit)}
                    </td>
                    <td className="py-2 pr-3 font-mono text-xs tabular-nums">
                      {hit.identity != null ? `${hit.identity.toFixed(1)}%` : '—'}
                    </td>
                    <td className="py-2 font-mono text-xs tabular-nums">
                      {hit.coverage != null ? `${hit.coverage.toFixed(1)}%` : '—'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>

        {addError && <InlineNotice tone="error">{addError}</InlineNotice>}

        <DialogFooter className="gap-2 sm:gap-2">
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={adding}>
            Cancel
          </Button>
          <Button onClick={handleAdd} disabled={adding || loading || selected.size === 0}>
            {adding && <LoaderCircle className="size-4 animate-spin" />}
            {adding ? 'Adding…' : `Add ${selected.size} Feature${selected.size === 1 ? '' : 's'}`}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
