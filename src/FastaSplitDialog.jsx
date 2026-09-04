import { Scissors } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
  DialogClose,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { fileNameOf } from './recentFiles';

// Shown when a multi-record FASTA is opened: split every record into its own
// project, or open only the first record (the historical behavior).
export default function FastaSplitDialog({ target, onChoice }) {
  const records = target?.records || [];
  return (
    <Dialog
      open={!!target}
      onOpenChange={(open) => {
        if (!open) onChoice(null);
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2.5">
            <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-amber-100 text-amber-600">
              <Scissors className="size-4" />
            </span>
            Multi-Record FASTA
          </DialogTitle>
          <DialogDescription>
            <span className="font-mono">{fileNameOf(target?.path || '')}</span> contains{' '}
            {records.length} sequence records. Split them into separate projects?
          </DialogDescription>
        </DialogHeader>
        <ul className="max-h-48 overflow-auto rounded-md border border-border/60 px-3 py-2 text-xs text-muted-foreground">
          {records.map((r, i) => (
            <li key={i} className="flex justify-between gap-2 py-1">
              <span className="truncate font-mono">{r.name || `Record ${i + 1}`}</span>
              <span className="shrink-0">
                {r.length} {r.moleculeType === 'protein' ? 'aa' : 'bp'}
              </span>
            </li>
          ))}
        </ul>
        <DialogFooter>
          <DialogClose asChild>
            <Button variant="outline" onClick={() => onChoice(null)}>
              Cancel
            </Button>
          </DialogClose>
          <Button variant="outline" onClick={() => onChoice('first')}>
            Open First Only
          </Button>
          <Button onClick={() => onChoice('split')}>Split All Records</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
