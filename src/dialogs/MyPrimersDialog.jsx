import { useCallback, useMemo } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Trash2, Plus, LoaderCircle, ArrowRight, Check, ArrowRightLeft } from 'lucide-react';

const DIR_COLORS = { fwd: '#166534', rev: '#4A148C' };

function truncateSeq(seq, max = 28) {
  if (!seq) return '';
  if (seq.length <= max) return seq;
  const half = Math.floor((max - 1) / 2);
  return seq.slice(0, half) + '…' + seq.slice(-half);
}

export default function MyPrimersDialog({
  open,
  onOpenChange,
  myPrimers = [],
  currentPrimers = [],
  binding = { loading: false, results: [] },
  onAddPrimer,
  onAddAllBinding,
  onDelete,
}) {
  const inFile = useMemo(
    () => new Set((currentPrimers || []).map((p) => String(p.primerSeq || '').toUpperCase())),
    [currentPrimers],
  );

  const bindById = useMemo(() => {
    const m = new Map();
    for (const r of binding.results || []) m.set(r.id, r);
    return m;
  }, [binding.results]);

  // name → set of distinct sequences that use it (for the "重名" hint)
  const nameSeqMap = useMemo(() => {
    const m = new Map();
    for (const e of myPrimers || []) {
      for (const n of e.names || []) {
        if (!m.has(n)) m.set(n, new Set());
        m.get(n).add(String(e.seq || '').toUpperCase());
      }
    }
    return m;
  }, [myPrimers]);
  const isDupName = useCallback((name) => (nameSeqMap.get(name)?.size || 0) > 1, [nameSeqMap]);

  const rows = useMemo(() => {
    const enriched = (myPrimers || []).map((p) => {
      const r = bindById.get(p.id);
      return {
        ...p,
        binds: !!r?.binds,
        site: r?.site || null,
        inFile: inFile.has(String(p.seq || '').toUpperCase()),
      };
    });
    return [...enriched].sort((a, b) => {
      if (a.binds !== b.binds) return a.binds ? -1 : 1;
      return (a.names?.[0] || '').localeCompare(b.names?.[0] || '');
    });
  }, [myPrimers, bindById, inFile]);

  const bindingCount = rows.filter((r) => r.binds && !r.inFile).length;

  const handleAddOne = useCallback(
    (p) => {
      onAddPrimer?.(p);
    },
    [onAddPrimer],
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl max-h-[80vh] flex flex-col px-8">
        <DialogHeader>
          <DialogTitle>My Primer Collection ({myPrimers.length})</DialogTitle>
        </DialogHeader>

        {myPrimers.length === 0 ? (
          <div className="flex flex-col items-center gap-3 py-10 text-center">
            <div className="flex size-11 items-center justify-center rounded-full bg-muted text-muted-foreground">
              <ArrowRightLeft className="size-5" />
            </div>
            <div className="text-sm font-medium">Your primer library is empty</div>
            <div className="max-w-md text-xs leading-relaxed text-muted-foreground">
              Add primers from the Primers → My Primer Collection menu: “Add Current Primer” or “Add
              All from This File”, or turn on “Auto-add from Opened Files”.
            </div>
          </div>
        ) : (
          <>
            <div className="flex-1 overflow-auto -mx-8 px-8">
              <table className="w-full border-collapse text-sm">
                <thead className="[&_th]:sticky [&_th]:top-0 [&_th]:z-10 [&_th]:bg-card">
                  <tr className="border-b border-border/60 text-xs uppercase tracking-wider text-muted-foreground">
                    <th className="text-left font-semibold py-2 pr-3">Name</th>
                    <th className="text-left font-semibold py-2 pr-3">Sequence</th>
                    <th className="text-left font-semibold py-2 pr-3">Binding</th>
                    <th className="text-left font-semibold py-2 pr-3">Tm</th>
                    <th className="text-left font-semibold py-2 pr-3"></th>
                    <th className="text-right font-semibold py-2"></th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((p) => (
                    <tr
                      key={p.id}
                      className={`border-b border-border/30 ${p.binds ? '' : 'opacity-60'}`}
                    >
                      <td className="py-2.5 pr-3 whitespace-nowrap">
                        <div className="flex items-center gap-1.5">
                          {(p.names || []).map((n, i) => (
                            <span key={`${n}-${i}`} className="inline-flex items-center">
                              {i > 0 && (
                                <span className="mr-1.5 text-xs text-muted-foreground/40">/</span>
                              )}
                              <span className="text-sm font-medium">{n}</span>
                              {isDupName(n) && (
                                <span className="ml-1 rounded bg-amber-100 px-1 py-px text-[10px] leading-tight text-amber-700">
                                  Dup
                                </span>
                              )}
                            </span>
                          ))}
                          <span
                            className="inline-flex items-center justify-center size-5 rounded text-[10px] font-bold text-white"
                            style={{ backgroundColor: DIR_COLORS[p.type] || '#666' }}
                          >
                            {p.type === 'fwd' ? 'F' : 'R'}
                          </span>
                        </div>
                      </td>
                      <td className="py-2.5 pr-3 whitespace-nowrap font-mono text-xs text-muted-foreground">
                        {truncateSeq(p.seq)}
                        <span className="ml-1.5 text-muted-foreground/60">({p.seq.length} bp)</span>
                      </td>
                      <td className="py-2.5 pr-3 whitespace-nowrap">
                        {binding.loading ? (
                          <span className="text-xs text-muted-foreground/60">Checking…</span>
                        ) : p.inFile ? (
                          <span className="inline-flex items-center gap-1 text-xs text-emerald-600">
                            <Check className="size-3.5" /> In file
                          </span>
                        ) : p.binds && p.site ? (
                          <span className="font-mono text-xs" style={{ color: DIR_COLORS[p.type] }}>
                            {p.site.strand === -1 ? 'R' : 'F'} {p.site.templateStart + 1}..
                            {p.site.templateEnd}
                          </span>
                        ) : (
                          <span className="text-xs text-muted-foreground/60">No binding</span>
                        )}
                      </td>
                      <td className="py-2.5 pr-3 font-mono text-xs tabular-nums">
                        {!binding.loading && p.binds && p.site?.tm != null
                          ? `${p.site.tm.toFixed(1)}°C`
                          : '—'}
                      </td>
                      <td className="py-2.5 pr-3">
                        {p.binds && !p.inFile && (
                          <button
                            type="button"
                            onClick={() => handleAddOne(p)}
                            className="inline-flex items-center gap-1 rounded-md bg-emerald-600 px-2 py-1 text-[11px] font-medium text-white transition-colors hover:bg-emerald-700"
                          >
                            <Plus className="size-3" /> Add
                          </button>
                        )}
                      </td>
                      <td className="py-2.5 text-right">
                        <button
                          type="button"
                          title="Remove from My Primers"
                          onClick={() => onDelete?.(p.id)}
                          className="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
                        >
                          <Trash2 className="size-3.5" />
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {binding.loading && (
                <div className="flex items-center justify-center gap-2 py-4 text-xs text-muted-foreground">
                  <LoaderCircle className="size-3.5 animate-spin" />
                  Checking binding against current sequence…
                </div>
              )}
            </div>

            <DialogFooter className="mt-1">
              <div className="flex w-full items-center gap-2">
                <div className="mr-auto text-xs text-muted-foreground">
                  {bindingCount > 0
                    ? `${bindingCount} primer${bindingCount > 1 ? 's' : ''} can bind to the current sequence`
                    : 'No binding primers to add'}
                </div>
                <Button
                  size="sm"
                  disabled={bindingCount === 0 || binding.loading}
                  onClick={onAddAllBinding}
                >
                  <ArrowRight className="size-3.5" />
                  Add All Binding Primers
                </Button>
              </div>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
