import { useEffect, useMemo, useRef, useState } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { LoaderCircle } from 'lucide-react';
import { getEnzymeDatabase } from '../tauriApi';
import { monoFont } from '../editorConstants';
import { splitEnzName } from '../editor/seqUtils';
import {
  ENZYME_FILTER_OPTIONS,
  buildDigestionRows,
  formatDigestionCutPosition,
  sortDigestionRows,
} from '../enzymeDigestion';
import { findProviderEntry, hasProvider } from '../enzymeProviders';
import { T, useTranslation } from '@/i18n';

function EnzymeName({ enzyme }) {
  const split = splitEnzName(enzyme.name);
  const color = enzyme.gray
    ? '#9CA3AF'
    : enzyme.cutType === 'blunt'
      ? '#6B3A2A'
      : enzyme.iis
        ? '#0D6B6B'
        : undefined;
  return (
    <span
      style={{
        fontFamily: monoFont,
        fontWeight: enzyme.pairCount === 1 ? 700 : 350,
        color,
      }}
    >
      {split.normal ? (
        <>
          <span className="italic">{split.italic}</span>
          {split.normal}
        </>
      ) : (
        enzyme.name
      )}
      {enzyme.pairCount === 2 && <sup className="ml-px text-[0.7em]">2</sup>}
    </span>
  );
}

export default function DigestionOverviewDialog({
  open,
  onOpenChange,
  enzymes = [],
  sequenceLength = 0,
  enzymeFilter = 'unique+twice',
  onEnzymeFilterChange,
  myEnzymes = [],
  enzymeProvider = 'all',
  providerIndex,
  onCutSelect,
}) {
  const { t } = useTranslation();
  const [records, setRecords] = useState(null);
  const [error, setError] = useState('');
  const [sortBy, setSortBy] = useState('cuts');
  const cutPositionsHeaderRef = useRef(null);

  const syncCutPositionsHeader = (event) => {
    if (cutPositionsHeaderRef.current) {
      cutPositionsHeaderRef.current.style.transform = `translateX(${-event.currentTarget.scrollLeft}px)`;
    }
  };

  useEffect(() => {
    if (!open || records !== null) return undefined;
    let cancelled = false;
    setError('');
    getEnzymeDatabase()
      .then((data) => {
        if (!cancelled) setRecords(Array.isArray(data) ? data : []);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [open, records]);

  const groupOptions = useMemo(() => {
    const options = [...ENZYME_FILTER_OPTIONS];
    if (myEnzymes.length || enzymeFilter === 'myEnzymes') {
      options.splice(1, 0, { value: 'myEnzymes', label: 'My Enzymes' });
    }
    return options;
  }, [myEnzymes, enzymeFilter]);

  const rows = useMemo(() => {
    const allRows = buildDigestionRows(records || [], enzymes, enzymeFilter, myEnzymes);
    const providerRows =
      enzymeProvider === 'all'
        ? allRows
        : allRows.filter((row) =>
            hasProvider(findProviderEntry(providerIndex, row.name), enzymeProvider),
          );
    return sortDigestionRows(providerRows, sortBy);
  }, [records, enzymes, enzymeFilter, myEnzymes, enzymeProvider, providerIndex, sortBy]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[80vh] flex-col px-8 sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>
            <T>{'Digestion Overview ('}</T>
            {records ? rows.length : '…'})
          </DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-3 pb-2 sm:flex-row">
          <label className="flex flex-1 items-center gap-2 text-sm">
            <span className="shrink-0 text-muted-foreground">
              <T>{'Enzyme Set'}</T>
            </span>
            <Select value={enzymeFilter} onValueChange={onEnzymeFilterChange}>
              <SelectTrigger className="flex-1" aria-label={t('Enzyme Set')}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {groupOptions.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    <T>{option.label}</T>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>
          <label className="flex flex-1 items-center gap-2 text-sm">
            <span className="shrink-0 text-muted-foreground">
              <T>{'Sort by'}</T>
            </span>
            <Select value={sortBy} onValueChange={setSortBy}>
              <SelectTrigger className="flex-1" aria-label={t('Sort by')}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="cuts">
                  <T>{'Cut count (low to high)'}</T>
                </SelectItem>
                <SelectItem value="name">
                  <T>{'Name (A–Z)'}</T>
                </SelectItem>
              </SelectContent>
            </Select>
          </label>
        </div>

        <div className="flex min-h-0 flex-1 flex-col">
          {error ? (
            <div className="py-8 text-center text-sm text-destructive">{error}</div>
          ) : !records ? (
            <div className="flex items-center justify-center gap-2 py-8 text-sm text-muted-foreground">
              <LoaderCircle className="size-4 animate-spin" />
              <T>{'Loading enzyme database…'}</T>
            </div>
          ) : (
            <div
              role="table"
              className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-border/60 text-sm"
            >
              <div
                role="row"
                className="flex w-full shrink-0 bg-muted text-xs uppercase tracking-wider text-muted-foreground"
              >
                <div
                  role="columnheader"
                  className="w-44 shrink-0 border-r border-border/60 bg-muted px-4 py-2.5 text-left font-semibold"
                >
                  <T>{'Name'}</T>
                </div>
                <div role="columnheader" className="min-w-0 flex-1 overflow-hidden">
                  <div
                    ref={cutPositionsHeaderRef}
                    className="w-max px-4 py-2.5 text-left font-semibold will-change-transform"
                  >
                    <T>{'Cut Positions'}</T>
                  </div>
                </div>
              </div>
              <div
                role="rowgroup"
                className="min-h-0 flex-1 overflow-auto"
                onScroll={syncCutPositionsHeader}
              >
                <div className="w-max min-w-full">
                  {rows.map((enzyme) => (
                    <div
                      role="row"
                      key={enzyme.name}
                      className="group flex w-full border-t border-border/40"
                    >
                      <div
                        role="cell"
                        className="sticky left-0 z-10 w-44 shrink-0 border-r border-border/60 bg-card px-4 py-2.5 whitespace-nowrap group-hover:bg-muted"
                      >
                        <EnzymeName enzyme={enzyme} />
                      </div>
                      <div
                        role="cell"
                        className="flex items-center whitespace-nowrap px-4 py-2 group-hover:bg-muted/50"
                      >
                        {enzyme.cuts.length ? (
                          <div className="flex w-max flex-nowrap items-center gap-1.5">
                            <span className="w-6 text-center font-mono text-xs text-muted-foreground tabular-nums">
                              {enzyme.cuts.length}
                            </span>
                            {enzyme.cuts.map((cut) => {
                              const position = formatDigestionCutPosition(cut, sequenceLength);
                              return (
                                <button
                                  key={cut}
                                  type="button"
                                  title={t('Jump to cut {position}', { position })}
                                  aria-label={t('Go to {name} cut at {position}', {
                                    name: enzyme.name,
                                    position,
                                  })}
                                  onClick={(event) => {
                                    if (event.detail <= 1) onCutSelect?.(cut);
                                  }}
                                  onDoubleClick={() => onCutSelect?.(cut, { close: true })}
                                  className="shrink-0 rounded bg-blue-50 px-1.5 py-0.5 font-mono text-xs text-blue-800 ring-1 ring-blue-600/20 transition-colors outline-none ring-inset hover:bg-blue-100 focus-visible:ring-2"
                                >
                                  {position}
                                </button>
                              );
                            })}
                          </div>
                        ) : (
                          <div className="flex items-center gap-1.5 py-0.5">
                            <span className="w-6 text-center font-mono text-xs text-muted-foreground tabular-nums">
                              0
                            </span>
                            <span className="text-xs text-muted-foreground">
                              <T>{'No cut sites'}</T>
                            </span>
                          </div>
                        )}
                      </div>
                    </div>
                  ))}
                  {rows.length === 0 && (
                    <div className="sticky left-0 min-w-full border-t border-border/40 py-8 text-center text-sm text-muted-foreground">
                      <T>{'No enzymes in this set'}</T>
                    </div>
                  )}
                </div>
              </div>
            </div>
          )}
        </div>

        <div className="pt-2 text-center text-[11px] text-muted-foreground/60">
          <T>{'Single-click to jump; double-click to jump and close'}</T>
        </div>
      </DialogContent>
    </Dialog>
  );
}
