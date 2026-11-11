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
import { fileNameOf } from '../recentFiles';
import { T, useTranslation } from '@/i18n';

// Shown when a multi-record FASTA is opened: split records, open the first,
// or open the collection as an MSA.
export default function FastaSplitDialog({ target, onChoice }) {
  const { t } = useTranslation();
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
            <T>{'Multi-Record FASTA'}</T>
          </DialogTitle>
          <DialogDescription>
            {t('{name} contains {count} sequence records. Choose how to open them.', {
              name: fileNameOf(target?.path || ''),
              count: records.length,
            })}
          </DialogDescription>
        </DialogHeader>
        <ul className="max-h-48 overflow-auto rounded-md border border-border/60 px-3 py-2 text-xs text-muted-foreground">
          {records.map((r, i) => (
            <li key={i} className="flex justify-between gap-2 py-1">
              <span className="truncate font-mono">
                {r.name || t('Record {number}', { number: i + 1 })}
              </span>
              <span className="shrink-0">
                {r.length} {r.moleculeType === 'protein' ? <T>{'aa'}</T> : <T>{'bp'}</T>}
              </span>
            </li>
          ))}
        </ul>
        <DialogFooter className="grid grid-cols-1 gap-2 sm:grid-cols-2 sm:space-x-0">
          <DialogClose asChild>
            <Button variant="outline" onClick={() => onChoice(null)}>
              <T>{'Cancel'}</T>
            </Button>
          </DialogClose>
          <Button variant="outline" onClick={() => onChoice('first')}>
            <T>{'Open First Only'}</T>
          </Button>
          <Button variant="outline" onClick={() => onChoice('msa')}>
            <T>{'Open as Alignment'}</T>
          </Button>
          <Button onClick={() => onChoice('split')}>
            <T>{'Split All Records'}</T>
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
