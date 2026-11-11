import { useState } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Plus, Trash2, LoaderCircle } from 'lucide-react';
import { T, useTranslation } from '@/i18n';

export default function AlignmentDialog({
  open,
  onOpenChange,
  alignments = [],
  onAddAlignment,
  onRemoveAlignment,
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  const handleAdd = async () => {
    setError('');
    setBusy(true);
    try {
      await onAddAlignment?.();
    } catch (e) {
      setError(String(e?.message || e || 'No significant alignment found'));
    } finally {
      setBusy(false);
    }
  };

  const handleRemove = async (id) => {
    setError('');
    try {
      await onRemoveAlignment?.(id);
    } catch (e) {
      setError(String(e?.message || e));
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl max-h-[80vh] flex flex-col px-8">
        <DialogHeader>
          <DialogTitle>
            <T>{'Alignments ('}</T>
            {alignments.length})
          </DialogTitle>
        </DialogHeader>

        <div className="flex-1 overflow-auto -mx-8 px-8">
          <table className="w-full border-collapse text-sm">
            <thead className="[&_th]:sticky [&_th]:top-0 [&_th]:z-10 [&_th]:bg-card">
              <tr className="border-b border-border/60 text-xs uppercase tracking-wider text-muted-foreground">
                <th className="text-left font-semibold py-2 pr-3">
                  <T>{'Name'}</T>
                </th>
                <th className="text-left font-semibold py-2 pr-3">
                  <T>{'Length'}</T>
                </th>
                <th className="text-left font-semibold py-2 pr-3">
                  <T>{'Strand'}</T>
                </th>
                <th className="text-left font-semibold py-2 pr-3">
                  <T>{'Identity'}</T>
                </th>
                <th className="py-2 w-8" />
              </tr>
            </thead>
            <tbody>
              {alignments.map((a) => (
                <tr
                  key={a.id}
                  className="border-b border-border/30 hover:bg-muted/50 transition-colors"
                >
                  <td className="py-2.5 pr-3 whitespace-nowrap">{a.name}</td>
                  <td className="py-2.5 pr-3 font-mono text-xs tabular-nums text-muted-foreground">
                    {a.length} <T>{'bp'}</T>
                  </td>
                  <td className="py-2.5 pr-3 font-mono text-xs">{a.strand}</td>
                  <td className="py-2.5 pr-3 font-mono text-xs tabular-nums">
                    {a.identity != null ? `${(a.identity * 100).toFixed(1)}%` : '—'}
                  </td>
                  <td className="py-2.5 text-right">
                    <button
                      type="button"
                      onClick={() => handleRemove(a.id)}
                      className="inline-flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-destructive"
                      title={t('Remove alignment')}
                    >
                      <Trash2 className="size-3.5" />
                    </button>
                  </td>
                </tr>
              ))}
              {alignments.length === 0 && (
                <tr>
                  <td colSpan={5} className="py-8 text-center text-sm text-muted-foreground">
                    <T>{'No alignments yet'}</T>
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        {error && <div className="pt-2 text-xs text-destructive">{error}</div>}

        <div className="flex items-center justify-between pt-2">
          <span className="text-[11px] text-muted-foreground/60">
            <T>
              {'Add a read or sequence file (.ab1, .fasta, .gbk…) to align against the template'}
            </T>
          </span>
          <Button size="sm" onClick={handleAdd} disabled={busy}>
            {busy ? <LoaderCircle className="size-4 animate-spin" /> : <Plus className="size-4" />}
            <T>{'Add…'}</T>
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
