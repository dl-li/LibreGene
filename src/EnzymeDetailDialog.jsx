import { useMemo } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { monoFont } from './editorConstants';
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

export default function EnzymeDetailDialog({ open, onOpenChange, record, dbRecords, providerIndex }) {
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

  if (!record) return null;
  const aliases = (entry?.aliases || []).filter(
    (a) => a.toLowerCase() !== record.name.toLowerCase(),
  );
  const providerKeys = PROVIDER_ORDER.filter((k) => entry?.providers?.[k]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl max-h-[80vh] overflow-y-auto px-8">
        <DialogHeader>
          <DialogTitle>
            {record.name}
            {aliases.length > 0 && (
              <span className="ml-2 text-sm font-normal text-muted-foreground">
                also: {aliases.join(', ')}
              </span>
            )}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-1">
          <Field label="Recognition Site">
            <span className="font-mono text-xs" style={{ fontFamily: monoFont }}>
              {record.site}
            </span>
          </Field>
          <Field label="Cut Notation">
            <span className="font-mono text-xs" style={{ fontFamily: monoFont }}>
              {record.elucidate || '—'}
            </span>
          </Field>
          <Field label="Cut Type">{CUT_TYPE_LABEL[record.cutType] || record.cutType || '—'}</Field>
          <Field label="Overhang">
            {record.overhangLen != null && record.overhangLen !== 0
              ? `${record.overhangLen} nt`
              : '—'}
          </Field>
          <Field label="Isoschizomers (同裂酶)">
            {related?.isoschizomers?.length ? related.isoschizomers.join(', ') : '—'}
          </Field>
          <Field label="Isocaudomers (同尾酶)">
            {related?.isocaudomers?.length ? related.isocaudomers.join(', ') : '—'}
          </Field>
        </div>

        <div className="mt-4 space-y-4">
          {providerKeys.length === 0 && (
            <div className="text-sm text-muted-foreground">
              No supplier data for this enzyme.
            </div>
          )}
          {providerKeys.map((key) => {
            const p = entry.providers[key];
            return (
              <div key={key} className="rounded-md border border-border/60 p-3">
                <div className="pb-1 text-sm font-semibold">{PROVIDER_LABEL[key] || key}</div>
                <Field label="Recommended Buffer">{p.recommendedBuffer}</Field>
                {(p.buffers || []).length > 0 && (
                  <div className="flex gap-2 py-0.5 text-sm">
                    <span className="w-36 shrink-0 text-muted-foreground">Buffers</span>
                    <span className="min-w-0">
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
                    <span className="font-mono text-xs" style={{ fontFamily: monoFont }}>
                      {p.catalog}
                    </span>
                  )}
                </Field>
              </div>
            );
          })}
        </div>
      </DialogContent>
    </Dialog>
  );
}
