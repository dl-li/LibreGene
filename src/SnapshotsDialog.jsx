import { useCallback, useEffect, useRef, useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { InlineNotice } from '@/components/ui/notice';
import { ExternalLink, History, LoaderCircle } from 'lucide-react';
import { cn } from '@/lib/utils';
import { getSnapgeneHistory } from './tauriApi';

// Operation accent colors, following GenePad's history panel grouping.
const OPERATION_TONES = {
  invalid: 'bg-gray-400',
  insert: 'bg-blue-500',
  insertFragment: 'bg-blue-500',
  insertFragments: 'bg-blue-500',
  replace: 'bg-blue-500',
  newFileFromSelection: 'bg-blue-500',
  remove: 'bg-rose-500',
  digest: 'bg-rose-500',
  amplifyFragment: 'bg-amber-500',
  primerDirectedMutagenesis: 'bg-amber-500',
  ligateFragments: 'bg-violet-500',
  gibsonAssembly: 'bg-violet-500',
  inFusionCloning: 'bg-violet-500',
  goldenGateAssembly: 'bg-violet-500',
  restrictionCloning: 'bg-violet-500',
  taCloning: 'bg-violet-500',
  topoCloning: 'bg-violet-500',
  gatewayLRCloning: 'bg-violet-500',
  gatewayBPCloning: 'bg-violet-500',
  flip: 'bg-teal-500',
  changeTopology: 'bg-teal-500',
  changeMethylation: 'bg-teal-500',
  changePhosphorylation: 'bg-teal-500',
  changeStrandedness: 'bg-teal-500',
  makeDna: 'bg-emerald-500',
  makeRna: 'bg-emerald-500',
  makeProtein: 'bg-emerald-500',
};

function operationTone(operation) {
  return OPERATION_TONES[operation] ?? 'bg-gray-400';
}

/** How the parent edit consumed this input (InputSummary edge label). */
function edgeLabel(entry, index) {
  if (!entry.edge || index === 0) return null;
  const { manipulation, val1, val2 } = entry.edge;
  const range = val1 > 0 || val2 > 0 ? ` ${val1 + 1}\u2013${val2 + 1}` : '';
  return `${manipulation}${range}`;
}

/**
 * SnapGene .dna file history — a vertical timeline of the history tree
 * (root = current state at top, older ancestors below). Clicking a snapshot
 * opens it as a new in-memory project for Save As.
 * Rendering approach follows GenePad's history panel
 * (https://github.com/GenePad), without the per-node minimaps.
 */
export default function SnapshotsDialog({ open, onOpenChange, projectId, onOpenSnapshot }) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [entries, setEntries] = useState(null); // null = no history in file
  const [openingId, setOpeningId] = useState(null);
  const reqRef = useRef(0);

  useEffect(() => {
    if (!open || !projectId) return;
    const req = ++reqRef.current;
    setLoading(true);
    setError('');
    setEntries(null);
    setOpeningId(null);
    getSnapgeneHistory(projectId)
      .then((data) => {
        if (reqRef.current !== req) return;
        if (data?.error) {
          setError(String(data.error));
          return;
        }
        setEntries(Array.isArray(data?.entries) ? data.entries : null);
      })
      .catch((e) => {
        if (reqRef.current !== req) return;
        setError(e?.message || String(e));
      })
      .finally(() => {
        if (reqRef.current === req) setLoading(false);
      });
  }, [open, projectId]);

  const handleRowClick = useCallback(
    async (entry) => {
      if (!entry.hasSnapshot || openingId != null) return;
      setOpeningId(entry.id);
      try {
        const result = await onOpenSnapshot?.(projectId, entry.id);
        if (!result || result.ok) {
          onOpenChange?.(false);
        } else if (result.error) {
          setError(result.error);
        }
      } catch (e) {
        setError(e?.message || String(e));
      } finally {
        setOpeningId(null);
      }
    },
    [onOpenSnapshot, onOpenChange, projectId, openingId],
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[70vh] w-[480px] flex-col gap-0 sm:max-w-[480px]">
        <DialogHeader className="pb-2">
          <DialogTitle className="flex items-center gap-2 text-base">
            <History className="size-4" />
            SnapGene History
          </DialogTitle>
          <DialogDescription>
            Snapshots recorded in the .dna file. Click one to open it as a new project, then use
            Save As to keep a copy.
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 overflow-y-auto pr-1">
          {loading ? (
            <div className="flex items-center justify-center gap-2 py-10 text-sm text-muted-foreground">
              <LoaderCircle className="size-4 animate-spin" />
              Reading history…
            </div>
          ) : error ? (
            <InlineNotice tone="error" className="my-2">
              {error}
            </InlineNotice>
          ) : !entries || entries.length === 0 ? (
            <div className="py-10 text-center text-sm text-muted-foreground">
              This file has no recorded history.
            </div>
          ) : (
            <ul className="relative flex flex-col gap-0.5 py-1">
              <span aria-hidden className="absolute bottom-4 left-[11px] top-4 w-px bg-border" />
              {entries.map((entry, i) => (
                <li key={entry.id}>
                  <button
                    type="button"
                    disabled={!entry.hasSnapshot || openingId != null}
                    onClick={() => handleRowClick(entry)}
                    className={cn(
                      'group relative flex w-full items-center gap-2 rounded-md py-1.5 pl-8 pr-2 text-left text-sm transition-colors',
                      entry.hasSnapshot
                        ? 'cursor-pointer hover:bg-accent'
                        : 'cursor-default opacity-60',
                    )}
                    title={
                      entry.hasSnapshot ? 'Open this snapshot' : 'Snapshot sequence unavailable'
                    }
                  >
                    <span
                      className={cn(
                        'absolute left-[7px] top-1/2 size-[9px] -translate-y-1/2 rounded-full ring-2 ring-background',
                        operationTone(entry.operation),
                      )}
                    />
                    <span
                      className={cn(
                        'min-w-0 flex-1 truncate',
                        i === 0 ? 'font-semibold' : 'font-medium',
                      )}
                    >
                      {i === 0 ? 'Current' : entry.name || entry.operation}
                    </span>
                    <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
                      {entry.seqLen.toLocaleString()} bp
                    </span>
                    <span className="w-14 shrink-0 text-right text-xs text-muted-foreground">
                      {entry.circular ? 'circ.' : 'lin.'}
                    </span>
                    <span className="w-16 shrink-0 truncate text-right text-xs text-muted-foreground">
                      {edgeLabel(entry, i)}
                    </span>
                    {openingId === entry.id ? (
                      <LoaderCircle className="size-3.5 shrink-0 animate-spin text-muted-foreground" />
                    ) : (
                      <ExternalLink
                        className={cn(
                          'size-3.5 shrink-0 text-muted-foreground transition-opacity',
                          entry.hasSnapshot ? 'opacity-0 group-hover:opacity-100' : 'opacity-0',
                        )}
                      />
                    )}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
