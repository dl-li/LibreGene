import { useMemo, useState, useEffect } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { ChevronRight } from 'lucide-react';
import { monoFont, splitEnzymeName } from './editorConstants';
import { getRelatedEnzymes } from './enzymeRelated';
import { PROVIDER_LABEL, PROVIDER_ORDER, findProviderEntry } from './enzymeProviders';

const CUT_TYPE_LABEL = { blunt: 'Blunt', '5overhang': "5' Overhang", '3overhang': "3' Overhang" };

function Field({ label, children }) {
  if (!children) return null;
  return (
    <div className="flex gap-2 py-0.5 text-sm">
      <span className="w-36 shrink-0 text-muted-foreground">{label}</span>
      <span className="min-w-0">{children}</span>
    </div>
  );
}

function EnzymeName({ name, className = '' }) {
  const s = splitEnzymeName(name);
  return (
    <span className={className} style={{ fontFamily: monoFont, fontWeight: 700 }}>
      {s.normal ? (
        <>
          <span style={{ fontStyle: 'italic' }}>{s.italic}</span>
          {s.normal}
        </>
      ) : (
        name
      )}
    </span>
  );
}

function formatCutPos(ci, len) {
  // cutIndex ci sits between 1-based bases ci and ci+1; origin cut is len^1.
  if (ci === 0 && len > 0) return `${len}^1`;
  return `${ci}^${ci + 1}`;
}

export default function EnzymeDetailDialog({
  open,
  onOpenChange,
  record,
  dbRecords,
  providerIndex,
  cutSites = null,
  plasmidLength = 0,
  currentProvider = 'all',
}) {
  const entry = useMemo(
    () => (record ? findProviderEntry(providerIndex, record.name) : null),
    [record, providerIndex],
  );
  const related = useMemo(() => {
    if (!record || !dbRecords?.length) return null;
    // Full-database lookup: every DB name is "in the current set".
    return getRelatedEnzymes(
      record.name,
      dbRecords.map((r) => r.name),
      dbRecords,
    );
  }, [record, dbRecords]);

  const providerKeys = PROVIDER_ORDER.filter((k) => entry?.providers?.[k]);
  const [expanded, setExpanded] = useState({});
  useEffect(() => {
    if (!record) return;
    const init = {};
    for (const k of PROVIDER_ORDER) init[k] = k === currentProvider;
    setExpanded(init);
  }, [record, currentProvider]);

  if (!record) return null;
  const aliases = (entry?.aliases || []).filter(
    (a) => a.toLowerCase() !== record.name.toLowerCase(),
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl max-h-[80vh] overflow-y-auto px-8">
        <DialogHeader>
          <DialogTitle className="flex flex-col items-start gap-0.5">
            <EnzymeName name={record.name} className="text-lg" />
            {aliases.length > 0 && (
              <span className="text-sm font-normal text-muted-foreground">
                Aliases: {aliases.join(', ')}
              </span>
            )}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-1">
          <Field label="Recognition Site">
            <span className="text-xs" style={{ fontFamily: monoFont }}>
              {record.site}
            </span>
          </Field>
          <Field label="Cut Notation">
            <span className="text-xs" style={{ fontFamily: monoFont }}>
              {record.elucidate || '—'}
            </span>
          </Field>
          <Field label="Cut Type">{CUT_TYPE_LABEL[record.cutType] || record.cutType || '—'}</Field>
          <Field label="Overhang">
            {record.overhangLen != null && record.overhangLen !== 0
              ? `${record.overhangLen} nt`
              : '—'}
          </Field>
          {cutSites !== null && (
            <Field label={`Cut Sites (${cutSites.length})`}>
              {cutSites.length ? (
                <span className="text-xs" style={{ fontFamily: monoFont }}>
                  {cutSites.map((ci) => formatCutPos(ci, plasmidLength)).join(',  ')}
                </span>
              ) : (
                'Does not cut the current plasmid'
              )}
            </Field>
          )}
          <Field label="Isoschizomers">
            {related?.isoschizomers?.length ? related.isoschizomers.join(', ') : '—'}
          </Field>
          <Field label="Isocaudomers">
            {related?.isocaudomers?.length ? related.isocaudomers.join(', ') : '—'}
          </Field>
        </div>

        <div className="mt-4 space-y-2">
          {providerKeys.length === 0 && (
            <div className="text-sm text-muted-foreground">
              No supplier data for this enzyme.
            </div>
          )}
          {providerKeys.map((key) => {
            const p = entry.providers[key];
            const isOpen = !!expanded[key];
            return (
              <div key={key} className="rounded-md border border-border/60">
                <button
                  type="button"
                  className="flex w-full items-center gap-1.5 px-3 py-2 text-sm font-semibold hover:bg-muted/50 rounded-md"
                  onClick={() => setExpanded((cur) => ({ ...cur, [key]: !cur[key] }))}
                >
                  <ChevronRight
                    className={`size-4 shrink-0 text-muted-foreground transition-transform ${isOpen ? 'rotate-90' : ''}`}
                  />
                  {PROVIDER_LABEL[key] || key}
                </button>
                {isOpen && (
                  <div className="px-3 pb-3 pt-1">
                    {(p.buffers || []).length > 0 && (
                      <div className="flex gap-2 py-0.5 text-sm">
                        <span className="w-36 shrink-0 text-muted-foreground">Buffers</span>
                        <span className="min-w-0 flex-1">
                          {p.buffers.map((b) => (
                            <div key={b.name} className="flex justify-between gap-4">
                              <span>{b.name}</span>
                              <span className="tabular-nums text-muted-foreground">
                                {b.activity}
                                {/^\d+(\.\d+)?$/.test(String(b.activity)) && '%'}
                              </span>
                            </div>
                          ))}
                        </span>
                      </div>
                    )}
                    <Field label="Working Temp">{p.workTemp && `${p.workTemp}°C`}</Field>
                    <Field label="Heat Inactivation">
                      {p.heatInactivation &&
                        (String(p.heatInactivation).includes('°')
                          ? p.heatInactivation
                          : `${p.heatInactivation}°C`)}
                    </Field>
                    <Field label="Methylation Effect">{p.methylation}</Field>
                    <Field label="Star Activity">{p.starActivity}</Field>
                    <Field label="Catalog">
                      {p.catalog && (
                        <span className="text-xs" style={{ fontFamily: monoFont }}>
                          {p.catalog}
                        </span>
                      )}
                    </Field>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </DialogContent>
    </Dialog>
  );
}
