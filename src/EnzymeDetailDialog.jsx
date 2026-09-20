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
    <span className={`text-teal-700 ${className}`} style={{ fontFamily: monoFont, fontWeight: 700 }}>
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

function MonoChip({ children }) {
  return (
    <span
      className="inline-block rounded bg-teal-50 px-1.5 py-0.5 text-xs text-teal-800 ring-1 ring-inset ring-teal-600/20"
      style={{ fontFamily: monoFont }}
    >
      {children}
    </span>
  );
}

function NameList({ items }) {
  const [open, setOpen] = useState(false);
  if (!items?.length) return <span className="text-muted-foreground/50">—</span>;
  const collapsible = items.length > 3;
  return (
    <span className="flex items-start gap-1">
      <span className={`min-w-0 flex-1 ${open || !collapsible ? '' : 'line-clamp-1'}`}>
        {items.join(', ')}
      </span>
      {collapsible && (
        <button
          type="button"
          className="shrink-0 text-xs text-teal-700 hover:underline"
          onClick={() => setOpen((v) => !v)}
        >
          {open ? 'less' : `show all ${items.length}`}
        </button>
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
  const [variantSel, setVariantSel] = useState({});
  useEffect(() => {
    if (!record) return;
    const init = {};
    for (const k of PROVIDER_ORDER) init[k] = k === currentProvider;
    setExpanded(init);
    const sel = {};
    for (const k of PROVIDER_ORDER) {
      const vs = entry?.providers?.[k]?.variants || [];
      const i = vs.findIndex((v) => v.name.toLowerCase() === record.name.toLowerCase());
      sel[k] = i >= 0 ? i : 0;
    }
    setVariantSel(sel);
  }, [record, currentProvider, entry]);

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
            <MonoChip>{record.site}</MonoChip>
          </Field>
          <Field label="Cut Notation">
            <MonoChip>{record.elucidate || '—'}</MonoChip>
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
                <span className="flex flex-wrap gap-1">
                  {cutSites.map((ci) => (
                    <MonoChip key={ci}>{formatCutPos(ci, plasmidLength)}</MonoChip>
                  ))}
                </span>
              ) : (
                'Does not cut the current plasmid'
              )}
            </Field>
          )}
          <Field label="Isoschizomers">
            <span className="flex-1">
              <NameList items={related?.isoschizomers} />
            </span>
          </Field>
          <Field label="Isocaudomers">
            <span className="flex-1">
              <NameList items={related?.isocaudomers} />
            </span>
          </Field>
        </div>

        <div className="mt-4 space-y-2">
          {providerKeys.length === 0 && (
            <div className="text-sm text-muted-foreground">
              No supplier data for this enzyme.
            </div>
          )}
          {providerKeys.map((key) => {
            const variants = entry.providers[key]?.variants || [];
            const selIdx = Math.min(variantSel[key] ?? 0, Math.max(variants.length - 1, 0));
            const p = variants[selIdx] || {};
            const isOpen = !!expanded[key];
            return (
              <div
                key={key}
                className={`rounded-md border ${isOpen ? 'border-teal-600/30 bg-teal-50/40' : 'border-border/60'}`}
              >
                <button
                  type="button"
                  className={`flex w-full items-center gap-1.5 rounded-md px-3 py-2 text-sm font-semibold hover:bg-muted/50 ${isOpen ? 'text-teal-700' : ''}`}
                  onClick={() => setExpanded((cur) => ({ ...cur, [key]: !cur[key] }))}
                >
                  <ChevronRight
                    className={`size-4 shrink-0 transition-transform ${isOpen ? 'rotate-90 text-teal-600' : 'text-muted-foreground'}`}
                  />
                  {PROVIDER_LABEL[key] || key}
                </button>
                {isOpen && (
                  <div className="px-3 pb-3 pt-1">
                    {variants.length > 1 && (
                      <div className="flex flex-wrap gap-1 pb-2">
                        {variants.map((v, i) => (
                          <button
                            key={v.name}
                            type="button"
                            className={`rounded-full px-2 py-0.5 text-xs ring-1 ring-inset transition-colors ${
                              i === selIdx
                                ? 'bg-teal-600 text-white ring-teal-600'
                                : 'bg-transparent text-teal-700 ring-teal-600/30 hover:bg-teal-50'
                            }`}
                            style={{ fontFamily: monoFont }}
                            onClick={() => setVariantSel((cur) => ({ ...cur, [key]: i }))}
                          >
                            {v.name}
                          </button>
                        ))}
                      </div>
                    )}
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
