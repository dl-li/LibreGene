import { useEffect, useMemo, useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { ArrowDown, ArrowUp, Eye, EyeOff, Pin, PinOff, Trash2 } from 'lucide-react';
import { computePairwiseMatrix, parseFastaRecords } from './msaAnalysis';

export function AddSequencesDialog({ open, onOpenChange, onConfirm }) {
  const [raw, setRaw] = useState('');
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const records = useMemo(() => parseFastaRecords(raw), [raw]);

  useEffect(() => {
    if (open) {
      setRaw('');
      setError('');
      setSubmitting(false);
    }
  }, [open]);

  const submit = async () => {
    if (!records.length) return;
    setSubmitting(true);
    const result = await onConfirm(records);
    if (result?.ok) {
      onOpenChange(false);
    } else {
      setError(result?.error || 'Failed to add sequences');
    }
    setSubmitting(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Add Sequences</DialogTitle>
          <DialogDescription>
            Paste one or more FASTA records. Existing gaps are removed; run Align afterwards.
          </DialogDescription>
        </DialogHeader>
        <textarea
          value={raw}
          onChange={(event) => setRaw(event.target.value)}
          placeholder=">sequence_2 description&#10;ACGT...&#10;>sequence_3&#10;ACGT..."
          className="min-h-64 w-full resize-y rounded-md border border-input bg-background p-3 font-mono text-sm outline-none focus:border-ring focus:ring-[3px] focus:ring-ring/50"
        />
        <div className="flex items-center justify-between text-xs text-muted-foreground">
          <span>
            {records.length} sequence{records.length === 1 ? '' : 's'}
          </span>
          {error && <span className="text-destructive">{error}</span>}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={!records.length || submitting}>
            Add Sequences
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function MsaSequencesDialog({
  open,
  onOpenChange,
  rows,
  hiddenRowIds,
  referenceRowId,
  onToggleHidden,
  onSetReference,
  onChange,
}) {
  const move = (index, delta) => {
    const target = index + delta;
    if (target < 0 || target >= rows.length) return;
    const next = [...rows];
    [next[index], next[target]] = [next[target], next[index]];
    onChange(next);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex flex-col max-h-[80vh] sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Sequences</DialogTitle>
          <DialogDescription>
            Rename, reorder, hide, remove, or pin a sequence as the display reference.
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 space-y-1 overflow-y-auto">
          {rows.map((row, index) => (
            <div
              key={row.id}
              className="flex items-center gap-1.5 rounded-md border border-border/60 p-1.5"
            >
              <Input
                key={`${row.id}-${row.name}`}
                defaultValue={row.name}
                aria-label={`Name of sequence ${index + 1}`}
                onBlur={(event) => {
                  const name = event.target.value.trim();
                  if (name && name !== row.name) {
                    onChange(rows.map((item) => (item.id === row.id ? { ...item, name } : item)));
                  }
                }}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') event.currentTarget.blur();
                }}
                className="h-8 min-w-0 flex-1"
              />
              <span className="w-16 shrink-0 text-right text-xs text-muted-foreground">
                {row.sequence.length} residues
              </span>
              <Button
                variant="ghost"
                size="icon"
                className="size-8"
                title="Move up"
                disabled={index === 0}
                onClick={() => move(index, -1)}
              >
                <ArrowUp className="size-4" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="size-8"
                title="Move down"
                disabled={index === rows.length - 1}
                onClick={() => move(index, 1)}
              >
                <ArrowDown className="size-4" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="size-8"
                title={hiddenRowIds.has(row.id) ? 'Show' : 'Hide'}
                onClick={() => onToggleHidden(row.id)}
              >
                {hiddenRowIds.has(row.id) ? (
                  <EyeOff className="size-4" />
                ) : (
                  <Eye className="size-4" />
                )}
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className={referenceRowId === row.id ? 'size-8 text-teal-600' : 'size-8'}
                title={referenceRowId === row.id ? 'Clear reference' : 'Set as reference'}
                onClick={() => onSetReference(referenceRowId === row.id ? null : row.id)}
              >
                {referenceRowId === row.id ? (
                  <PinOff className="size-4" />
                ) : (
                  <Pin className="size-4" />
                )}
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="size-8 text-destructive"
                title="Remove"
                disabled={rows.length === 1}
                onClick={() => onChange(rows.filter((item) => item.id !== row.id))}
              >
                <Trash2 className="size-4" />
              </Button>
            </div>
          ))}
        </div>
        <DialogFooter>
          <Button onClick={() => onOpenChange(false)}>Done</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function PairwiseStatsDialog({ open, onOpenChange, rows, aligned, moleculeType }) {
  const [metric, setMetric] = useState('identity');
  const matrix = useMemo(
    () => (open ? computePairwiseMatrix(rows, aligned, moleculeType) : []),
    [open, rows, aligned, moleculeType],
  );
  const format = (value) => (value == null ? '—' : `${(value * 100).toFixed(1)}%`);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex flex-col max-h-[85vh] max-w-[90vw] sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>Pairwise Sequence Statistics</DialogTitle>
          <DialogDescription>
            Columns with a gap in either sequence are excluded. Protein similarity uses BLOSUM62
            positive scores.
          </DialogDescription>
        </DialogHeader>
        {moleculeType === 'protein' && (
          <div className="flex gap-1">
            <Button
              size="sm"
              variant={metric === 'identity' ? 'default' : 'outline'}
              onClick={() => setMetric('identity')}
            >
              Identity
            </Button>
            <Button
              size="sm"
              variant={metric === 'similarity' ? 'default' : 'outline'}
              onClick={() => setMetric('similarity')}
            >
              Similarity
            </Button>
          </div>
        )}
        <div className="min-h-0 flex-1 overflow-auto rounded-md border border-border/60">
          <table className="border-collapse text-xs">
            <thead className="sticky top-0 z-10 bg-background">
              <tr>
                <th className="sticky left-0 z-20 min-w-40 border-b border-r bg-background p-2 text-left">
                  Sequence
                </th>
                {rows.map((row) => (
                  <th key={row.id} className="min-w-20 border-b p-2 font-medium">
                    {row.name}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row, i) => (
                <tr key={row.id}>
                  <th className="sticky left-0 z-10 max-w-52 truncate border-r bg-background p-2 text-left font-medium">
                    {row.name}
                  </th>
                  {rows.map((other, j) => (
                    <td
                      key={other.id}
                      className="border-b p-2 text-center tabular-nums"
                      title={`${matrix[i]?.[j]?.compared || 0} compared columns`}
                    >
                      {format(matrix[i]?.[j]?.[metric])}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <DialogFooter>
          <Button onClick={() => onOpenChange(false)}>Close</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
