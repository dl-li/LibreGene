import { useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { InlineNotice } from '@/components/ui/notice';
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
          <div className="space-y-1.5">
            <Label htmlFor="align-name" className="text-xs text-muted-foreground">
              Name (optional)
            </Label>
            <Input
              id="align-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. read-01.ab1"
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="align-seq" className="text-xs text-muted-foreground">
              Sequence (DNA or FASTA)
            </Label>
            <textarea
              id="align-seq"
              value={seq}
              onChange={(e) => setSeq(e.target.value)}
              placeholder={'Paste a DNA sequence or FASTA text…'}
              rows={8}
              spellCheck={false}
              className="w-full resize-none rounded-md border border-input bg-background px-3 py-2 font-mono text-xs leading-relaxed outline-none transition-shadow focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
            />
          </div>
        </div>

        {error && <InlineNotice tone="error">{error}</InlineNotice>}

        <DialogFooter className="gap-2 sm:gap-2">
          <span className="mr-auto text-[11px] text-muted-foreground/60">
            The reverse complement is also tried automatically
          </span>
          <Button variant="outline" size="sm" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button size="sm" onClick={handleSubmit} disabled={busy || !seq.trim()}>
            {busy && <LoaderCircle className="size-4 animate-spin" />}
            Align
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
