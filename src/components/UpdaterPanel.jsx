import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { cn } from '@/lib/utils';
import { T, useTranslation } from '@/i18n';
import { RefreshCw } from 'lucide-react';

export default function UpdaterPanel({ updater = {} }) {
  const { t } = useTranslation();
  const busy = ['checking', 'downloading', 'installing'].includes(updater.status);
  const downloadPercent = updater.total
    ? Math.min(100, Math.round((updater.downloaded / updater.total) * 100))
    : null;

  return (
    <section className="space-y-3 border-t border-border/60 pt-4" aria-live="polite">
      <div className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <RefreshCw className="size-3.5" />
          <T>{'Updates'}</T>
        </div>
        <label className="flex items-center gap-2 text-xs text-muted-foreground">
          <T>{'Automatically check for updates'}</T>
          <Checkbox
            checked={updater.autoCheck !== false}
            onCheckedChange={(value) => updater.setAutoCheck?.(!!value)}
            aria-label={t('Automatically check for updates')}
          />
        </label>
      </div>
      <div className="flex items-center justify-between gap-4">
        <p className="text-xs text-muted-foreground">
          <T>{'Check at startup and daily'}</T>
        </p>
        <Button
          variant="outline"
          size="sm"
          disabled={busy}
          onClick={() => updater.checkForUpdates?.()}
        >
          {updater.status === 'checking' ? <T>{'Checking…'}</T> : <T>{'Check for updates'}</T>}
        </Button>
      </div>
      {updater.status === 'latest' && (
        <p className="text-xs text-muted-foreground">
          <T>{'LibreGene is up to date.'}</T>
        </p>
      )}
      {updater.update && (
        <div className="space-y-2.5">
          <div className="flex items-center justify-between gap-4">
            <div>
              <p className="text-[13px] font-medium">
                {t('Version {version} is available', { version: updater.update.version })}
              </p>
              {updater.isLinux && (
                <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                  <T>{'Use Flatpak to install the update, or open the latest release.'}</T>
                </p>
              )}
            </div>
            <Button size="sm" disabled={busy} onClick={() => updater.downloadAndInstall?.()}>
              {updater.isLinux ? <T>{'Open latest release'}</T> : <T>{'Download & install'}</T>}
            </Button>
          </div>
          {updater.update.body && (
            <pre className="max-h-36 overflow-auto whitespace-pre-wrap rounded-md bg-muted/50 p-3 font-sans text-xs leading-relaxed text-muted-foreground">
              {updater.update.body}
            </pre>
          )}
        </div>
      )}
      {(updater.status === 'downloading' || updater.status === 'installing') && (
        <div className="space-y-2">
          <div className="flex justify-between text-xs text-muted-foreground">
            <span>
              {updater.status === 'installing' ? (
                <T>{'Installing update…'}</T>
              ) : (
                <T>{'Downloading update…'}</T>
              )}
            </span>
            {downloadPercent !== null && <span>{downloadPercent}%</span>}
          </div>
          <div
            className="h-1.5 overflow-hidden rounded-full bg-muted"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={downloadPercent ?? 0}
          >
            <div
              className={cn(
                'h-full rounded-full bg-primary transition-[width]',
                downloadPercent === null && 'w-1/3 animate-pulse',
              )}
              style={downloadPercent === null ? undefined : { width: `${downloadPercent}%` }}
            />
          </div>
        </div>
      )}
      {updater.error && (
        <p className="text-xs leading-relaxed text-destructive">
          <T>{updater.error}</T>
        </p>
      )}
    </section>
  );
}
