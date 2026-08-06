import { useState, useEffect, useMemo, useRef } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { LoaderCircle, Sparkles, TriangleAlert } from 'lucide-react';
import { monoFont } from '../../editorConstants';
import { listCodonSpecies, previewCodonOptimization, applyCodonOptimization } from '../../tauriApi';

const SPECIES_LABELS = {
  b_subtilis: 'Bacillus subtilis',
  c_elegans: 'Caenorhabditis elegans',
  d_melanogaster: 'Drosophila melanogaster',
  e_coli: 'E. coli',
  g_gallus: 'Gallus gallus',
  h_sapiens: 'Homo sapiens',
  m_musculus: 'Mus musculus',
  m_musculus_domesticus: 'Mus musculus domesticus',
  s_cerevisiae: 'Saccharomyces cerevisiae',
};
const BUILTIN_SPECIES = Object.keys(SPECIES_LABELS);
const CUSTOM_SPECIES = '__custom__';

const METHODS = [
  {
    value: 'match_codon_usage',
    label: 'Match codon usage (recommended)',
    description:
      'Matches the target species’ codon-frequency distribution, so synonymous choice stays naturally diverse.',
  },
  {
    value: 'use_best_codon',
    label: 'Use best codon',
    description:
      'Uses the most frequent codon for every amino acid (CAI → 1); homopolymer runs and repeats are auto-repaired.',
  },
  {
    value: 'harmonize_rca',
    label: 'Harmonize RCA (rare ↔ rare)',
    description:
      'Keeps each codon’s relative codon adaptation close to the original sequence; requires choosing the original species.',
  },
];

const REASON_LABELS = {
  homopolymer: 'homopolymer runs',
  repeat: 'repeated k-mers',
  enzyme_site: 'enzyme sites',
  gc_window: 'GC window',
};

// Kazusa showcodon.cgi "triplet amino acid frequency" row, e.g. `UUU F 0.58 (  3456)`
const KAZUSA_LINE = /^\s*([ACGTUacgtu]{3})\s+([A-Za-z*])\s+(\d+(?:\.\d+)?)/;

function parseKazusaTable(text) {
  const rows = [];
  const badLines = [];
  text.split(/\r?\n/).forEach((raw, i) => {
    const line = raw.trim();
    if (!line || line.startsWith('#')) return;
    const m = KAZUSA_LINE.exec(line);
    if (!m) {
      badLines.push(i + 1);
      return;
    }
    rows.push([m[2].toUpperCase(), m[1].toUpperCase().replace(/U/g, 'T'), parseFloat(m[3])]);
  });
  return { rows, badLines };
}

export default function CodonOptimizationDialog({
  open,
  onOpenChange,
  features = [],
  onProjectChanged,
}) {
  const [speciesList, setSpeciesList] = useState(BUILTIN_SPECIES);
  const [featureId, setFeatureId] = useState('');
  const [species, setSpecies] = useState('e_coli');
  const [customText, setCustomText] = useState('');
  const [method, setMethod] = useState('match_codon_usage');
  const [originalSpecies, setOriginalSpecies] = useState('');
  const [avoidSites, setAvoidSites] = useState('');
  const [preview, setPreview] = useState(null);
  const [busy, setBusy] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState('');
  const previewParamsRef = useRef(null);

  const codingFeatures = useMemo(
    () => (features || []).filter((f) => f.ftype === 'CDS' || f.ftype === 'mRNA'),
    [features],
  );

  const invalidate = () => {
    setPreview(null);
    previewParamsRef.current = null;
  };

  // Load the species list and keep a valid feature selected each time the dialog opens.
  useEffect(() => {
    if (!open) return;
    setError('');
    setPreview(null);
    previewParamsRef.current = null;
    setFeatureId((cur) =>
      codingFeatures.some((f) => f.id === cur) ? cur : (codingFeatures[0]?.id ?? ''),
    );
    let cancelled = false;
    listCodonSpecies()
      .then((list) => {
        if (!cancelled && Array.isArray(list) && list.length) setSpeciesList(list);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [open, codingFeatures]);

  const currentMethod = useMemo(() => METHODS.find((m) => m.value === method), [method]);

  const parseAvoidSites = () => {
    const sites = avoidSites
      .split(',')
      .map((s) => s.trim().toUpperCase())
      .filter(Boolean);
    return sites.length ? sites : null;
  };

  const buildParams = () => {
    if (!featureId) {
      setError('Select a CDS or mRNA feature first.');
      return null;
    }
    const params = {
      featureId,
      species: species === CUSTOM_SPECIES ? 'custom' : species,
      method,
      originalSpecies: method === 'harmonize_rca' ? originalSpecies || null : null,
      avoidEnzymeSites: parseAvoidSites(),
    };
    if (species === CUSTOM_SPECIES) {
      const { rows, badLines } = parseKazusaTable(customText);
      if (!rows.length) {
        setError('Paste a Kazusa "triplet amino acid frequency" table into the text area first.');
        return null;
      }
      if (badLines.length) {
        setError(
          `Skipped ${badLines.length} unparseable line(s): ${badLines.slice(0, 5).join(', ')}${badLines.length > 5 ? ', …' : ''}`,
        );
        return null;
      }
      params.customTable = rows;
    }
    return params;
  };

  const handlePreview = async () => {
    setError('');
    const params = buildParams();
    if (!params) return;
    setBusy(true);
    try {
      const res = await previewCodonOptimization(params);
      previewParamsRef.current = params;
      setPreview(res);
    } catch (e) {
      setError(String(e?.message || e));
      setPreview(null);
      previewParamsRef.current = null;
    } finally {
      setBusy(false);
    }
  };

  const handleApply = async () => {
    const params = previewParamsRef.current;
    if (!params) return;
    setError('');
    setApplying(true);
    try {
      await applyCodonOptimization(params);
      onProjectChanged?.();
      onOpenChange(false);
    } catch (e) {
      setError(String(e?.message || e));
    } finally {
      setApplying(false);
    }
  };

  const repairCounts = useMemo(() => {
    const counts = {};
    for (const r of preview?.repairs || []) counts[r.reason] = (counts[r.reason] || 0) + 1;
    return counts;
  }, [preview]);

  const speciesLabel = (key) => SPECIES_LABELS[key] || key;
  const hasFeatures = codingFeatures.length > 0;
  const canApply = !!preview && !busy && !applying;
  const unresolved = preview?.unresolved || [];

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl max-h-[85vh] flex flex-col px-8">
        <DialogHeader>
          <DialogTitle>Codon Optimization</DialogTitle>
        </DialogHeader>

        <div className="flex-1 overflow-auto -mx-8 px-8 space-y-4">
          <div className="space-y-1.5">
            <Label className="text-xs font-medium text-muted-foreground">CDS / mRNA feature</Label>
            {hasFeatures ? (
              <Select
                value={featureId || undefined}
                onValueChange={(v) => {
                  setFeatureId(v);
                  invalidate();
                }}
                disabled={busy || applying}
              >
                <SelectTrigger className="h-9">
                  <SelectValue placeholder="Select a feature…" />
                </SelectTrigger>
                <SelectContent>
                  {codingFeatures.map((f) => (
                    <SelectItem key={f.id} value={f.id}>
                      {f.name || f.ftype} ({f.start + 1}..{f.end + 1})
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : (
              <div className="rounded-md border border-dashed px-3 py-2 text-xs text-muted-foreground">
                No CDS or mRNA features in this project.
              </div>
            )}
          </div>

          <div className="space-y-1.5">
            <Label className="text-xs font-medium text-muted-foreground">Target species</Label>
            <Select
              value={species}
              onValueChange={(v) => {
                setSpecies(v);
                invalidate();
              }}
              disabled={busy || applying}
            >
              <SelectTrigger className="h-9">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {speciesList.map((key) => (
                  <SelectItem key={key} value={key}>
                    {speciesLabel(key)} ({key})
                  </SelectItem>
                ))}
                <SelectItem value={CUSTOM_SPECIES}>Custom (Kazusa table)</SelectItem>
              </SelectContent>
            </Select>
            {species === CUSTOM_SPECIES && (
              <div className="space-y-1.5 pt-1">
                <textarea
                  value={customText}
                  onChange={(e) => {
                    setCustomText(e.target.value);
                    invalidate();
                  }}
                  disabled={busy || applying}
                  spellCheck={false}
                  placeholder={
                    'Paste Kazusa "triplet amino acid frequency" rows, e.g.\nUUU F 0.58 (  3456)\nUUC F 0.44 (  2591)\n…'
                  }
                  className="h-28 w-full resize-y rounded-md border border-input bg-transparent px-3 py-2 font-mono text-xs outline-none transition-shadow focus:border-ring focus:ring-[3px] focus:ring-ring/50"
                />
                <p className="text-[11px] text-muted-foreground">
                  Format: <span className="font-mono">codon · amino acid · frequency</span> — rows
                  with a U are converted to T automatically.
                </p>
              </div>
            )}
          </div>

          <div className="space-y-1.5">
            <Label className="text-xs font-medium text-muted-foreground">Method</Label>
            <Select
              value={method}
              onValueChange={(v) => {
                setMethod(v);
                invalidate();
              }}
              disabled={busy || applying}
            >
              <SelectTrigger className="h-9">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {METHODS.map((m) => (
                  <SelectItem key={m.value} value={m.value}>
                    {m.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-[11px] text-muted-foreground">{currentMethod?.description}</p>
            {method === 'harmonize_rca' && (
              <div className="space-y-1.5 pt-1">
                <Label className="text-xs font-medium text-muted-foreground">
                  Original species (source of the current codons)
                </Label>
                <Select
                  value={originalSpecies || undefined}
                  onValueChange={(v) => {
                    setOriginalSpecies(v);
                    invalidate();
                  }}
                  disabled={busy || applying}
                >
                  <SelectTrigger className="h-9">
                    <SelectValue placeholder="Select the original species…" />
                  </SelectTrigger>
                  <SelectContent>
                    {BUILTIN_SPECIES.map((key) => (
                      <SelectItem key={key} value={key}>
                        {speciesLabel(key)} ({key})
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}
          </div>

          <div className="space-y-1.5">
            <Label className="text-xs font-medium text-muted-foreground">
              Avoid restriction sites (comma-separated, IUPAC allowed)
            </Label>
            <Input
              value={avoidSites}
              onChange={(e) => {
                setAvoidSites(e.target.value);
                invalidate();
              }}
              disabled={busy || applying}
              placeholder="e.g. GAATTC, GGATCC"
              className="h-9"
            />
          </div>

          <div className="flex items-center gap-2 pt-1">
            <Button size="sm" onClick={handlePreview} disabled={busy || applying || !hasFeatures}>
              {busy ? (
                <LoaderCircle className="size-4 animate-spin" />
              ) : (
                <Sparkles className="size-4" />
              )}
              Preview
            </Button>
            {preview && !busy && (
              <span className="text-[11px] text-muted-foreground">
                Previewed with {speciesLabel(preview.species)}
              </span>
            )}
          </div>

          {preview && !busy && (
            <div className="space-y-3 rounded-md border border-border/60 p-3">
              <div className="text-xs font-medium text-muted-foreground">
                Translation ({preview.codonCount} codons)
              </div>
              <div
                className="max-h-32 overflow-auto break-all rounded-md border border-border/40 bg-muted/40 p-2.5 text-xs leading-5"
                style={{ fontFamily: monoFont }}
              >
                {preview.aa}
              </div>

              <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
                <div className="rounded-md border border-border/40 p-2">
                  <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
                    CAI
                  </div>
                  <div className="font-mono text-sm tabular-nums">
                    {preview.caiBefore.toFixed(3)} <span className="text-muted-foreground">→</span>{' '}
                    <span className="font-semibold">{preview.caiAfter.toFixed(3)}</span>
                  </div>
                </div>
                <div className="rounded-md border border-border/40 p-2">
                  <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
                    GC
                  </div>
                  <div className="font-mono text-sm tabular-nums">
                    {(preview.gcBefore * 100).toFixed(1)}%{' '}
                    <span className="text-muted-foreground">→</span>{' '}
                    <span className="font-semibold">{(preview.gcAfter * 100).toFixed(1)}%</span>
                  </div>
                </div>
                <div className="rounded-md border border-border/40 p-2">
                  <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
                    Repairs
                  </div>
                  <div className="font-mono text-sm tabular-nums">
                    {(preview.repairs || []).length}
                  </div>
                </div>
                <div className="rounded-md border border-border/40 p-2">
                  <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
                    Unresolved
                  </div>
                  <div className="font-mono text-sm tabular-nums">{unresolved.length}</div>
                </div>
              </div>

              {Object.keys(repairCounts).length > 0 && (
                <div className="flex flex-wrap gap-1.5">
                  {Object.entries(repairCounts).map(([reason, n]) => (
                    <span
                      key={reason}
                      className="rounded-full border border-border/50 bg-muted/60 px-2 py-0.5 text-[11px] text-muted-foreground"
                    >
                      {n}× {REASON_LABELS[reason] || reason}
                    </span>
                  ))}
                </div>
              )}

              {unresolved.length > 0 && (
                <div className="flex items-start gap-2 rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-800">
                  <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
                  <span>
                    {unresolved.length} violation(s) could not be repaired:{' '}
                    {unresolved.slice(0, 6).join('; ')}
                    {unresolved.length > 6 ? ', …' : ''}
                  </span>
                </div>
              )}
            </div>
          )}

          {error && <div className="text-xs text-destructive">{error}</div>}
        </div>

        <DialogFooter className="mt-1">
          <div className="flex w-full items-center gap-2">
            <span className="mr-auto text-[11px] text-muted-foreground/60">
              Equal-length synonymous substitution — feature coordinates are unchanged.
            </span>
            <Button
              variant="outline"
              size="sm"
              onClick={() => onOpenChange(false)}
              disabled={applying}
            >
              Cancel
            </Button>
            <Button size="sm" onClick={handleApply} disabled={!canApply}>
              {applying ? <LoaderCircle className="size-4 animate-spin" /> : null}
              Apply changes
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
