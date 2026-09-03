import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { InlineNotice } from '@/components/ui/notice';
import {
  LoaderCircle,
  ScanSearch,
  AlertTriangle,
  Map as MapIcon,
  Table as TableIcon,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { annotateSequenceText } from './tauriApi';
import { locationString1based } from './editorConstants';
import { CircularMap, LinearMap } from './MapView';
import FornaView from './plugins/rnaFold/FornaView';
import useRnaFold, { countPairs, MAX_INTERACTIVE_NT } from './plugins/rnaFold/useRnaFold';

const EMPTY_ARRAY = [];
const NOOP = () => {};

const DNA_IUPAC = 'ACGTURYSWKMBDHVN';

/** FASTA or plain text → uppercase sequence: skip `>` headers, drop whitespace and digits. */
function parseSequence(raw) {
  let out = '';
  for (const line of raw.split(/\r?\n/)) {
    const t = line.trim();
    if (t.startsWith('>')) continue;
    out += t;
  }
  return out.replace(/[\s\d]/g, '').toUpperCase();
}

function validateSequence(molType, seq) {
  if (!seq) return { valid: false, sequence: '', invalidChars: [] };
  if (molType === 'protein') {
    // A single trailing '*' is a stop codon marker — strip it; any other '*' is illegal.
    const stripped = seq.endsWith('*') ? seq.slice(0, -1) : seq;
    const invalidChars = [...new Set([...stripped].filter((c) => !/[A-Z]/.test(c)))];
    return { valid: invalidChars.length === 0, sequence: stripped, invalidChars };
  }
  const invalidChars = [...new Set([...seq].filter((c) => !DNA_IUPAC.includes(c)))];
  return { valid: invalidChars.length === 0, sequence: seq, invalidChars };
}

/**
 * Empty-page "New Sequence" dialog: choose molecule type / topology, paste a
 * FASTA or plain sequence, live-annotate (debounced) and create the project
 * with the checked features. `onConfirm` receives
 * `{ name, sequence, moleculeType, topology, features }` and resolves with
 * `{ ok, error? }`.
 */
export default function NewSequenceDialog({ open, onOpenChange, onConfirm }) {
  const [molType, setMolType] = useState('dna');
  const [name, setName] = useState('Untitled');
  const [topology, setTopology] = useState('circular');
  const [raw, setRaw] = useState('');
  const [items, setItems] = useState(EMPTY_ARRAY);
  const [selected, setSelected] = useState(() => new Set());
  const [viewMode, setViewMode] = useState('map');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState('');
  const reqRef = useRef(0);

  const parsed = useMemo(() => parseSequence(raw), [raw]);
  const validation = useMemo(() => validateSequence(molType, parsed), [molType, parsed]);
  const effectiveSeq = validation.sequence;
  const unit = molType === 'protein' ? 'aa' : molType === 'rna' ? 'nt' : 'bp';
  const isRna = molType === 'rna';

  // RNA: the right panel shows a live folding preview (auto-annotation is
  // DNA/protein-only), debounced like the annotation below.
  const fold = useRnaFold(effectiveSeq, open && isRna && validation.valid && !!effectiveSeq);

  useEffect(() => {
    if (open) {
      setMolType('dna');
      setName('Untitled');
      setTopology('circular');
      setRaw('');
      setItems(EMPTY_ARRAY);
      setSelected(new Set());
      setViewMode('map');
      setLoading(false);
      setError('');
      setSubmitError('');
      reqRef.current++;
    }
  }, [open]);

  // Debounced live annotation on a valid sequence.
  useEffect(() => {
    if (!open) return;
    if (!validation.valid || !effectiveSeq || isRna) {
      reqRef.current++;
      setItems(EMPTY_ARRAY);
      setLoading(false);
      setError('');
      return;
    }
    const req = ++reqRef.current;
    setLoading(true);
    setError('');
    const timer = setTimeout(() => {
      annotateSequenceText(effectiveSeq, topology === 'circular', molType)
        .then((list) => {
          if (reqRef.current !== req) return;
          const arr = Array.isArray(list) ? list : [];
          arr.sort((a, b) => Number(a.fragment) - Number(b.fragment));
          setItems(arr);
          setSelected(new Set(arr.map((h, i) => (h.fragment ? -1 : i)).filter((i) => i >= 0)));
        })
        .catch((e) => {
          if (reqRef.current !== req) return;
          setError(e?.message || String(e));
          setItems(EMPTY_ARRAY);
        })
        .finally(() => {
          if (reqRef.current === req) setLoading(false);
        });
    }, 500);
    return () => {
      clearTimeout(timer);
      reqRef.current++;
    };
  }, [open, molType, effectiveSeq, topology, validation.valid]);

  const toggle = useCallback((i) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });
  }, []);

  // checked features, shaped for the map components
  const mapFeatures = useMemo(
    () =>
      items
        .map((h, i) => ({ h, i }))
        .filter(({ i }) => selected.has(i))
        .map(({ h, i }) => ({
          id: `nf-${i}`,
          name: h.name,
          ftype: h.ftype,
          color: h.color,
          strand: h.strand,
          start: h.start,
          end: h.end,
          segments: h.segments?.length ? h.segments : [{ start: h.start, end: h.end }],
        })),
    [items, selected],
  );

  const handleConfirm = useCallback(async () => {
    if (!validation.valid || submitting) return;
    setSubmitting(true);
    setSubmitError('');
    const features = items
      .filter((h, i) => selected.has(i))
      .map((h) => ({
        name: h.name,
        ftype: h.ftype,
        color: h.color,
        strand: h.strand,
        segments: h.segments?.length ? h.segments : [{ start: h.start, end: h.end }],
      }));
    try {
      const res = await onConfirm({
        name: name.trim() || 'Untitled',
        sequence: effectiveSeq,
        moleculeType: molType,
        topology: molType === 'dna' ? topology : 'linear',
        features,
      });
      if (res && res.ok) {
        onOpenChange(false);
      } else {
        setSubmitError(res?.error || 'Failed to create project');
      }
    } catch (e) {
      setSubmitError(e?.message || String(e));
    } finally {
      setSubmitting(false);
    }
  }, [
    validation.valid,
    submitting,
    items,
    selected,
    onConfirm,
    name,
    effectiveSeq,
    molType,
    topology,
    onOpenChange,
  ]);

  const segBtn = (value, label, group) => (
    <button
      key={value}
      type="button"
      onClick={() => (group === 'mol' ? setMolType(value) : setTopology(value))}
      className={cn(
        'rounded-md px-2 py-1.5 text-xs font-medium capitalize transition-colors',
        (group === 'mol' ? molType : topology) === value
          ? 'bg-background text-foreground shadow-sm'
          : 'text-muted-foreground hover:text-foreground',
      )}
    >
      {label}
    </button>
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-4xl max-h-[85vh] flex flex-col px-8">
        <DialogHeader>
          <DialogTitle>New Sequence</DialogTitle>
          <DialogDescription>
            Paste a sequence to create a new project. Detected features can be added automatically.
          </DialogDescription>
        </DialogHeader>

        <div className="grid min-h-0 flex-1 gap-5 sm:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]">
          <div className="flex min-h-0 flex-col gap-4 overflow-y-auto px-1">
            <div>
              <Label className="text-xs text-muted-foreground">Molecule type</Label>
              <div className="mt-1.5 grid grid-cols-3 gap-1 rounded-lg border border-border bg-muted/40 p-1">
                {segBtn('dna', 'DNA', 'mol')}
                {segBtn('rna', 'RNA', 'mol')}
                {segBtn('protein', 'Peptide', 'mol')}
              </div>
            </div>
            <div>
              <Label htmlFor="new-seq-name" className="text-xs text-muted-foreground">
                Name
              </Label>
              <Input
                id="new-seq-name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                className="mt-1.5"
                placeholder="Untitled"
              />
            </div>
            {molType === 'dna' && (
              <div>
                <Label className="text-xs text-muted-foreground">Topology</Label>
                <div className="mt-1.5 grid grid-cols-2 gap-1 rounded-lg border border-border bg-muted/40 p-1">
                  {segBtn('circular', 'Circular', 'topo')}
                  {segBtn('linear', 'Linear', 'topo')}
                </div>
              </div>
            )}
            <div className="flex min-h-0 flex-1 flex-col">
              <Label htmlFor="new-seq-input" className="text-xs text-muted-foreground">
                Sequence (FASTA or plain text)
              </Label>
              <textarea
                id="new-seq-input"
                value={raw}
                onChange={(e) => setRaw(e.target.value)}
                placeholder=">Example plasmid&#10;ATGCATGCATGCATGCATGC"
                className="mt-1.5 min-h-[160px] flex-1 resize-none rounded-md border border-input bg-background px-3 py-2 font-mono text-xs leading-relaxed outline-none focus-visible:ring-2 focus-visible:ring-ring"
                spellCheck={false}
              />
              <div className="mt-1.5 flex flex-wrap items-center justify-between gap-2 text-xs">
                <span
                  className={
                    validation.valid ? 'text-muted-foreground' : 'font-medium text-red-600'
                  }
                >
                  {validation.valid
                    ? `${effectiveSeq.length} ${unit}`
                    : validation.invalidChars.length
                      ? `Invalid characters: ${validation.invalidChars.join(', ')}`
                      : 'Enter a sequence'}
                </span>
                {!validation.valid && validation.invalidChars.length > 0 && (
                  <span className="text-muted-foreground">
                    Allowed:{' '}
                    {molType === 'protein' ? 'A–Z (* only as trailing stop)' : 'ACGTURYSWKMBDHVN'}
                  </span>
                )}
              </div>
            </div>
          </div>

          <div className="flex min-h-0 flex-col overflow-hidden rounded-lg border border-border/60 bg-muted/20">
            <div className="flex items-center justify-between border-b border-border/60 px-3 py-2">
              <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                {isRna ? 'RNA Folding' : 'Detected Features'}
              </span>
              {!isRna && (
                <div className="grid grid-cols-2 gap-1 rounded-md border border-border bg-muted/40 p-0.5">
                  {[
                    { value: 'map', label: 'Map', Icon: MapIcon },
                    { value: 'table', label: 'Table', Icon: TableIcon },
                  ].map(({ value, label, Icon }) => (
                    <button
                      key={value}
                      type="button"
                      onClick={() => setViewMode(value)}
                      className={cn(
                        'flex items-center gap-1 rounded px-2 py-0.5 text-xs font-medium transition-colors',
                        viewMode === value
                          ? 'bg-background text-foreground shadow-sm'
                          : 'text-muted-foreground hover:text-foreground',
                      )}
                    >
                      <Icon className="size-3" />
                      {label}
                    </button>
                  ))}
                </div>
              )}
            </div>
            <div className="min-h-0 flex-1 overflow-auto">
              {isRna ? (
                fold.busy ? (
                  <div className="flex items-center justify-center gap-2 py-12 text-sm text-muted-foreground">
                    <LoaderCircle className="size-4 animate-spin" />
                    Folding {effectiveSeq.length} nt…
                  </div>
                ) : fold.error ? (
                  <div className="flex items-center justify-center gap-2 py-12 text-sm text-red-600">
                    <AlertTriangle className="size-4" />
                    <span>{fold.error}</span>
                  </div>
                ) : !fold.result ? (
                  <div className="flex flex-col items-center gap-2 py-12 text-center text-sm text-muted-foreground">
                    <ScanSearch className="size-5 opacity-50" />
                    <span>Enter a sequence to fold</span>
                  </div>
                ) : (
                  <div className="flex min-h-full flex-col p-3">
                    <div className="flex flex-wrap gap-x-4 gap-y-1 pb-2 text-xs">
                      <span>
                        MFE{' '}
                        <span className="font-mono font-semibold">
                          {fold.result.mfe.toFixed(2)}
                        </span>{' '}
                        kcal/mol
                      </span>
                      <span className="text-muted-foreground">
                        {countPairs(fold.result.structure)} base pairs / {effectiveSeq.length} nt
                      </span>
                    </div>
                    {effectiveSeq.length <= MAX_INTERACTIVE_NT ? (
                      <FornaView
                        sequence={effectiveSeq}
                        structure={fold.result.structure}
                        height={380}
                      />
                    ) : (
                      <p className="py-4 text-center text-xs text-muted-foreground">
                        Sequence too long for interactive layout ({effectiveSeq.length} nt &gt;{' '}
                        {MAX_INTERACTIVE_NT} nt).
                      </p>
                    )}
                  </div>
                )
              ) : loading ? (
                <div className="flex items-center justify-center gap-2 py-12 text-sm text-muted-foreground">
                  <LoaderCircle className="size-4 animate-spin" />
                  Detecting common features…
                </div>
              ) : error ? (
                <div className="flex items-center justify-center gap-2 py-12 text-sm text-red-600">
                  <AlertTriangle className="size-4" />
                  <span>{error}</span>
                </div>
              ) : items.length === 0 ? (
                <div className="flex flex-col items-center gap-2 py-12 text-center text-sm text-muted-foreground">
                  <ScanSearch className="size-5 opacity-50" />
                  <span>No common features detected</span>
                </div>
              ) : viewMode === 'map' ? (
                <div className="flex min-h-full items-center justify-center p-3">
                  {molType === 'dna' && topology === 'circular' ? (
                    <CircularMap
                      length={effectiveSeq.length}
                      features={mapFeatures}
                      name={name.trim() || 'Untitled'}
                      selection={null}
                      onSelect={NOOP}
                      onClear={NOOP}
                      onFeatureOpen={NOOP}
                      bg="transparent"
                    />
                  ) : (
                    <LinearMap
                      length={effectiveSeq.length}
                      features={mapFeatures}
                      selection={null}
                      onSelect={NOOP}
                      onClear={NOOP}
                      onFeatureOpen={NOOP}
                      bg="transparent"
                    />
                  )}
                </div>
              ) : (
                <table className="w-full border-collapse text-sm">
                  <thead className="[&_th]:sticky [&_th]:top-0 [&_th]:z-10 [&_th]:bg-muted">
                    <tr className="border-b border-border/60 text-xs uppercase tracking-wider text-muted-foreground">
                      <th className="w-8 py-2 pl-3 pr-3" />
                      <th className="py-2 pr-3 text-left font-semibold">Name</th>
                      <th className="py-2 pr-3 text-left font-semibold">Type</th>
                      <th className="py-2 pr-3 text-left font-semibold">Strand</th>
                      <th className="py-2 pr-3 text-left font-semibold">Location</th>
                      <th className="py-2 text-left font-semibold">Identity</th>
                    </tr>
                  </thead>
                  <tbody>
                    {items.map((hit, i) => (
                      <tr key={i} className="border-b border-border/30">
                        <td className="py-2 pl-3 pr-3">
                          <input
                            type="checkbox"
                            checked={selected.has(i)}
                            disabled={submitting}
                            onChange={() => toggle(i)}
                            aria-label={`Select ${hit.name}`}
                          />
                        </td>
                        <td className="py-2 pr-3 whitespace-nowrap">
                          <span
                            className="mr-2 inline-block size-3 shrink-0 rounded-sm align-middle ring-1 ring-inset ring-black/10"
                            style={{ backgroundColor: hit.color }}
                          />
                          <span className="font-medium">{hit.name}</span>
                          {hit.fragment && (
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
                        <td className="py-2 font-mono text-xs tabular-nums">
                          {hit.identity != null ? `${hit.identity.toFixed(1)}%` : '—'}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          </div>
        </div>

        {submitError && <InlineNotice tone="error">{submitError}</InlineNotice>}

        <DialogFooter className="gap-2 sm:gap-2">
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={submitting}>
            Cancel
          </Button>
          <Button
            onClick={handleConfirm}
            disabled={!validation.valid || !effectiveSeq || submitting}
          >
            {submitting && <LoaderCircle className="size-4 animate-spin" />}
            {submitting ? 'Creating…' : 'Create Project'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
