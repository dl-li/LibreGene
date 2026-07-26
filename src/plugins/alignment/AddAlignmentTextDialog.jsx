import { useState } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { LoaderCircle } from 'lucide-react';

export default function AddAlignmentTextDialog({ open, onOpenChange, onSubmit }) {
  const [name, setName] = useState('');
  const [seq, setSeq] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  const handleSubmit = async () => {
    setError('');
    setBusy(true);
    try {
      await onSubmit?.(name.trim(), seq);
      setName('');
      setSeq('');
      onOpenChange(false);
    } catch (e) {
      setError(String(e?.message || e || 'No significant alignment found'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg px-8">
        <DialogHeader>
          <DialogTitle>Add Alignment from Text</DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-3">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Name (optional)"
            className="rounded-md border border-border bg-background px-3 py-1.5 text-sm outline-none focus:border-foreground/30"
          />
          <textarea
            value={seq}
            onChange={(e) => setSeq(e.target.value)}
            placeholder={'Paste a DNA sequence or FASTA text…'}
            rows={8}
            className="resize-none rounded-md border border-border bg-background px-3 py-2 font-mono text-xs outline-none focus:border-foreground/30"
          />
        </div>

        {error && <div className="pt-1 text-xs text-destructive">{error}</div>}

        <div className="flex items-center justify-between pt-2">
          <span className="text-[11px] text-muted-foreground/60">
            The reverse complement is also tried automatically
          </span>
          <Button size="sm" onClick={handleSubmit} disabled={busy || !seq.trim()}>
            {busy && <LoaderCircle className="size-4 animate-spin" />}
            Align
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
