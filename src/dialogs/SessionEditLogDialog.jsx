import { useEffect, useMemo, useState } from 'react';
import { Bot, History, Undo2, User } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

const FILTERS = [
  { value: 'all', label: 'All' },
  { value: 'user', label: 'User' },
  { value: 'agent', label: 'Agent' },
];

function timeLabel(timestamp) {
  return new Date(timestamp).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

export default function SessionEditLogDialog({
  open,
  onOpenChange,
  entries,
  onRollback,
  agentLocked = false,
}) {
  const [filter, setFilter] = useState('all');
  const [pendingId, setPendingId] = useState(null);
  const [busyId, setBusyId] = useState(null);
  const [error, setError] = useState('');
  const latestId = entries.filter((entry) => !entry.saved).at(-1)?.id ?? null;
  const visibleEntries = useMemo(
    () => entries.filter((entry) => filter === 'all' || entry.origin === filter).reverse(),
    [entries, filter],
  );

  useEffect(() => {
    if (!open) {
      setPendingId(null);
      setBusyId(null);
      setError('');
    }
  }, [open]);

  const rollback = async (entry) => {
    setBusyId(entry.id);
    setError('');
    try {
      await onRollback(entry.id);
      setPendingId(null);
    } catch (e) {
      setError(String(e?.message || e));
    } finally {
      setBusyId(null);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[78vh] flex-col gap-4 sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <History className="size-4" /> Session Edit Log
          </DialogTitle>
          <DialogDescription>
            Changes from this session. Saved entries stay visible but cannot be rolled back.
          </DialogDescription>
        </DialogHeader>

        <div className="flex items-center gap-1 rounded-lg bg-muted/50 p-1">
          {FILTERS.map((item) => {
            const count =
              item.value === 'all'
                ? entries.length
                : entries.filter((entry) => entry.origin === item.value).length;
            return (
              <button
                key={item.value}
                type="button"
                onClick={() => {
                  setFilter(item.value);
                  setPendingId(null);
                }}
                className={cn(
                  'rounded-md px-3 py-1.5 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground',
                  filter === item.value && 'bg-background text-foreground shadow-sm',
                )}
              >
                {item.label} ({count})
              </button>
            );
          })}
        </div>

        <div className="min-h-40 flex-1 overflow-y-auto overscroll-contain pr-1">
          {visibleEntries.length === 0 ? (
            <div className="flex h-40 flex-col items-center justify-center gap-2 text-center text-sm text-muted-foreground">
              <History className="size-6 opacity-50" />
              {entries.length ? 'No changes match this filter.' : 'No edits in this session.'}
            </div>
          ) : (
            <ol className="space-y-2">
              {visibleEntries.map((entry) => {
                const isLatest = entry.id === latestId;
                const isPending = pendingId === entry.id;
                const newerCount = entries.filter(
                  (item) => !item.saved && item.id > entry.id,
                ).length;
                const busy = busyId === entry.id;
                return (
                  <li
                    key={entry.id}
                    className={cn(
                      'rounded-lg border border-border/70 bg-background/60 p-3',
                      entry.saved && 'opacity-70',
                    )}
                  >
                    <div className="flex items-start gap-3">
                      <div className="min-w-0 flex-1">
                        <div className="mb-1.5 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                          <span
                            className={cn(
                              'inline-flex items-center gap-1 rounded-full px-2 py-0.5 font-medium',
                              entry.origin === 'agent'
                                ? 'bg-teal-500/10 text-teal-700 dark:text-teal-400'
                                : 'bg-muted text-muted-foreground',
                            )}
                          >
                            {entry.origin === 'agent' ? <Bot /> : <User />}
                            {entry.origin === 'agent' ? 'Agent · MCP' : 'User'}
                          </span>
                          {entry.saved && (
                            <span className="rounded-full bg-muted px-2 py-0.5 font-medium">
                              Saved
                            </span>
                          )}
                          <time title={new Date(entry.timestamp).toLocaleString()}>
                            {timeLabel(entry.timestamp)}
                          </time>
                        </div>
                        <p className="text-sm font-medium leading-5">{entry.summary}</p>
                        {entry.details.length > 1 && (
                          <ul className="mt-1 space-y-0.5 text-xs leading-5 text-muted-foreground">
                            {entry.details.slice(1).map((detail, index) => (
                              <li key={`${entry.id}-${index}`}>· {detail}</li>
                            ))}
                          </ul>
                        )}
                      </div>
                      {!entry.saved && !isPending && (
                        <Button
                          type="button"
                          variant={isLatest ? 'outline' : 'ghost'}
                          size="sm"
                          className="shrink-0"
                          disabled={agentLocked || busyId !== null}
                          title={
                            agentLocked ? 'Unlock the agent tab to roll back changes' : undefined
                          }
                          onClick={() => {
                            if (isLatest) rollback(entry);
                            else {
                              setPendingId(entry.id);
                              setError('');
                            }
                          }}
                        >
                          <Undo2 /> {busy ? 'Rolling back…' : isLatest ? 'Undo' : 'Roll back'}
                        </Button>
                      )}
                    </div>
                    {isPending && (
                      <div className="mt-3 rounded-md bg-amber-500/10 p-3 text-xs">
                        <p className="leading-5 text-foreground/80">
                          Restore the state before this change? This also rolls back {newerCount}{' '}
                          newer {newerCount === 1 ? 'change' : 'changes'}.
                        </p>
                        <div className="mt-2 flex justify-end gap-2">
                          <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            onClick={() => setPendingId(null)}
                          >
                            Cancel
                          </Button>
                          <Button
                            type="button"
                            size="sm"
                            disabled={agentLocked || busy}
                            onClick={() => rollback(entry)}
                          >
                            <Undo2 /> {busy ? 'Rolling back…' : 'Confirm rollback'}
                          </Button>
                        </div>
                      </div>
                    )}
                  </li>
                );
              })}
            </ol>
          )}
        </div>

        {error && <p className="text-sm text-destructive">{error}</p>}
        <div className="border-t border-border/60 pt-3 text-xs text-muted-foreground">
          Session only · never written to the sequence file · cleared when the project closes
        </div>
      </DialogContent>
    </Dialog>
  );
}
