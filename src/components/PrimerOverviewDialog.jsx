import { useCallback, useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Copy, Check } from 'lucide-react';

const DIR_COLORS = { fwd: '#166534', rev: '#4A148C' };
const MAX_SEQ_DISPLAY = 30;

function truncateSeq(seq) {
  if (!seq) return '';
  if (seq.length <= MAX_SEQ_DISPLAY) return seq;
  const half = Math.floor((MAX_SEQ_DISPLAY - 1) / 2);
  return seq.slice(0, half) + '…' + seq.slice(-half);
}

function CopyCell({ text, mono }) {
  const [copied, setCopied] = useState(false);
  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    });
  }, [text]);
  const display = mono ? truncateSeq(text) : text;
  return (
    <button
      type="button"
      onClick={(e) => { e.stopPropagation(); handleCopy(); }}
      className="group inline-flex items-center gap-1.5 text-left cursor-pointer outline-none focus:outline-none focus-visible:outline-none"
      tabIndex={-1}
    >
      <span className={mono ? 'font-mono text-xs' : 'text-sm'}>{display}</span>
      {copied ? (
        <Check className="size-3 shrink-0 text-emerald-500" />
      ) : (
        <Copy className="size-3 shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
      )}
    </button>
  );
}

function siteStart(site) {
  return site.templateStart ?? site.start;
}
function siteEnd(site) {
  return (site.templateEnd ?? site.end + 1) - 1;
}
function siteTm(site) {
  return site.tm;
}

function formatSite(site) {
  if (!site) return '—';
  const s = siteStart(site);
  const e = siteEnd(site);
  if (s == null || e == null || isNaN(s) || isNaN(e)) return '—';
  return `${s + 1}..${e + 1}`;
}

function formatTm(site) {
  if (!site) return '—';
  const t = siteTm(site);
  if (t == null || isNaN(t)) return '—';
  return `${t.toFixed(1)}°C`;
}

export default function PrimerOverviewDialog({
  open,
  onOpenChange,
  primers = [],
  alignmentCacheRef,
  onEditPrimer,
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-4xl max-h-[80vh] flex flex-col px-8">
        <DialogHeader>
          <DialogTitle>Primer Overview ({primers.length})</DialogTitle>
        </DialogHeader>

        <div className="flex-1 overflow-auto -mx-8 px-8">
          <table className="w-full border-collapse text-sm">
            <thead>
              <tr className="border-b border-border/60 text-xs uppercase tracking-wider text-muted-foreground">
                <th className="text-left font-semibold py-2 pr-3">Name</th>
                <th className="text-left font-semibold py-2 pr-3">Sequence</th>
                <th className="text-left font-semibold py-2 pr-3">Length</th>
                <th className="text-left font-semibold py-2 pr-3 whitespace-nowrap">Primary Site</th>
                <th className="text-left font-semibold py-2 pr-3">Dir</th>
                <th className="text-left font-semibold py-2 pr-3">Tm</th>
                <th className="text-left font-semibold py-2">Other Sites</th>
              </tr>
            </thead>
            <tbody>
              {primers.map((p) => {
                const bs = p.bindingSites?.[0];
                const cacheData = alignmentCacheRef?.current?.[p.id];
                const cacheCur = cacheData?.current;
                // prefer bindingSites[0] for primary, fall back to cache current
                const primary = bs || cacheCur;
                const cacheAlts = cacheData?.alternatives || [];
                const alts = [
                  ...(p.bindingSites?.slice(1) || []),
                  ...cacheAlts,
                ];
                const dirColor = DIR_COLORS[p.type] || '#666';
                return (
                  <tr
                    key={p.id}
                    className="border-b border-border/30 hover:bg-muted/50 cursor-pointer transition-colors"
                    onDoubleClick={() => onEditPrimer?.(p)}
                    title="Double-click to edit"
                  >
                    <td className="py-2.5 pr-3 whitespace-nowrap">
                      <CopyCell text={p.name} />
                    </td>
                    <td className="py-2.5 pr-3 whitespace-nowrap">
                      <CopyCell text={p.primerSeq || ''} mono />
                    </td>
                    <td className="py-2.5 pr-3 font-mono text-xs tabular-nums text-muted-foreground">
                      {(p.primerSeq || '').length} bp
                    </td>
                    <td className="py-2.5 pr-3 font-mono text-xs whitespace-nowrap" style={{ color: dirColor }}>
                      {primary ? formatSite(primary) : '—'}
                    </td>
                    <td className="py-2.5 pr-3">
                      <span
                        className="inline-flex items-center justify-center size-5 rounded text-[10px] font-bold text-white"
                        style={{ backgroundColor: dirColor }}
                      >
                        {p.type === 'fwd' ? 'F' : 'R'}
                      </span>
                    </td>
                    <td className="py-2.5 pr-3 font-mono text-xs tabular-nums">
                      {primary ? formatTm(primary) : '—'}
                    </td>
                    <td className="py-2.5 text-xs">
                      {alts.length > 0 ? (
                        <div className="flex flex-wrap gap-x-1.5 gap-y-1">
                          {alts.map((alt, i) => {
                            const dir = alt.strand === -1 ? 'R' : 'F';
                            return (
                              <span
                                key={i}
                                className="inline-flex items-center gap-0.5 rounded-md border border-amber-200 bg-amber-50 px-1.5 py-0.5 font-mono text-[11px] text-amber-800"
                              >
                                {dir} {formatSite(alt)}
                              </span>
                            );
                          })}
                        </div>
                      ) : (
                        <span className="text-muted-foreground/40">—</span>
                      )}
                    </td>
                  </tr>
                );
              })}
              {primers.length === 0 && (
                <tr>
                  <td colSpan={7} className="py-8 text-center text-sm text-muted-foreground">
                    No primers found
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        <div className="pt-2 text-[11px] text-muted-foreground/60 text-center">
          Double-click a row for details
        </div>
      </DialogContent>
    </Dialog>
  );
}
