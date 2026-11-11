import { useEffect, useState } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Star } from 'lucide-react';
import { isTauri, openUrl } from '../tauriApi';
import { T } from '@/i18n';

const REPO_URL = 'https://github.com/dl-li/LibreGene';
const AUTHOR_URL = 'https://github.com/dl-li';

export default function AboutDialog({ open, onOpenChange }) {
  const [version, setVersion] = useState(null);
  useEffect(() => {
    if (!open || !isTauri || version) return undefined;
    let cancelled = false;
    import('@tauri-apps/api/app')
      .then((m) => m.getVersion())
      .then((v) => {
        if (!cancelled) setVersion(v);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [open, version]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-sm">
        <DialogHeader>
          <DialogTitle className="sr-only">
            <T>{'About LibreGene'}</T>
          </DialogTitle>
        </DialogHeader>
        <div className="flex flex-col items-center gap-1.5 py-1 text-center">
          <img src="/icon.png" alt="LibreGene" className="size-14" />
          <h2 className="text-base font-semibold tracking-tight">
            <T>{'LibreGene'}</T>
          </h2>
          <p className="font-mono text-xs text-muted-foreground">{version ? `v${version}` : '…'}</p>
          <p className="text-sm text-muted-foreground">
            <T>{'A free, open-source plasmid editor.'}</T>
          </p>
        </div>
        <div className="space-y-1.5 rounded-md border border-border/60 px-3 py-2.5 text-xs">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">
              <T>{'Developer'}</T>
            </span>
            <button
              type="button"
              onClick={() => openUrl(AUTHOR_URL).catch(() => {})}
              className="font-medium text-foreground hover:underline"
            >
              <T>{'dl-li'}</T>
            </button>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">
              <T>{'Repository'}</T>
            </span>
            <button
              type="button"
              onClick={() => openUrl(REPO_URL).catch(() => {})}
              className="font-mono text-foreground hover:underline"
            >
              <T>{'dl-li/LibreGene'}</T>
            </button>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">
              <T>{'License'}</T>
            </span>
            <span className="font-medium">
              <T>{'GPL-3.0'}</T>
            </span>
          </div>
        </div>
        <p className="text-center text-xs leading-relaxed text-muted-foreground">
          <T>{'Enjoying LibreGene? A star on GitHub means a lot.'}</T>
        </p>
        <Button onClick={() => openUrl(REPO_URL).catch(() => {})} className="w-full">
          <Star className="size-4" />
          <T>{'Star on GitHub'}</T>
        </Button>
      </DialogContent>
    </Dialog>
  );
}
