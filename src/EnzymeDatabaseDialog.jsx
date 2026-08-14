import { useState, useEffect, useCallback, useMemo } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Search, LoaderCircle } from 'lucide-react';
import { monoFont } from './editorConstants';
import { getEnzymeDatabase } from './tauriApi';

const CUT_TYPE_LABEL = { blunt: 'Blunt', '5overhang': "5' Overhang", '3overhang': "3' Overhang" };

export default function EnzymeDatabaseDialog({ open, onOpenChange }) {
  const [records, setRecords] = useState(null);
  const [error, setError] = useState('');
  const [query, setQuery] = useState('');

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setRecords(null);
    setError('');
    setQuery('');
    getEnzymeDatabase()
      .then((data) => {
        if (cancelled) return;
        setRecords(Array.isArray(data) ? data : []);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  const filtered = useMemo(() => {
    if (!records) return [];
    const q = query.trim().toLowerCase();
    if (!q) return records;
    return records.filter(
      (e) =>
        (e.name || '').toLowerCase().includes(q) ||
        (e.site || '').toLowerCase().includes(q) ||
        (e.elucidate || '').toLowerCase().includes(q),
    );
  }, [records, query]);

  const cutTypeLabel = useCallback((e) => CUT_TYPE_LABEL[e.cutType] || e.cutType || '—', []);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-5xl max-h-[80vh] flex flex-col px-8">
        <DialogHeader>
          <DialogTitle>Enzyme Database ({records ? records.length : '…'})</DialogTitle>
        </DialogHeader>

        <div className="relative pb-2">
          <Search className="pointer-events-none absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter by name, recognition site…"
            spellCheck={false}
            className="h-9 w-full rounded-md border border-input bg-transparent pl-8 pr-3 text-sm shadow-xs outline-none transition-shadow placeholder:text-muted-foreground focus:border-ring focus:ring-[3px] focus:ring-ring/50"
          />
        </div>

        <div className="flex-1 overflow-auto -mx-8 px-8">
          {error ? (
            <div className="py-8 text-center text-sm text-destructive">{error}</div>
          ) : !records ? (
            <div className="flex items-center justify-center gap-2 py-8 text-sm text-muted-foreground">
              <LoaderCircle className="size-4 animate-spin" /> Loading enzyme database…
            </div>
          ) : (
            <table className="w-full border-collapse text-sm">
              <thead className="[&_th]:sticky [&_th]:top-0 [&_th]:z-10 [&_th]:bg-card">
                <tr className="border-b border-border/60 text-xs uppercase tracking-wider text-muted-foreground">
                  <th className="text-left font-semibold py-2 pr-3">Name</th>
                  <th className="text-left font-semibold py-2 pr-3">Recognition</th>
                  <th className="text-left font-semibold py-2 pr-3">Cut</th>
                  <th className="text-left font-semibold py-2 pr-3">Cut Type</th>
                  <th className="text-left font-semibold py-2 pr-3">Overhang</th>
                  <th className="text-left font-semibold py-2 pr-3">bp</th>
                  <th className="text-left font-semibold py-2 pr-3">Pal.</th>
                  <th className="text-left font-semibold py-2 pr-3">Cut ×2</th>
                  <th className="text-left font-semibold py-2">Methyl.</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((e) => (
                  <tr key={e.name} className="border-b border-border/30">
                    <td className="py-2 pr-3 whitespace-nowrap font-medium">{e.name}</td>
                    <td
                      className="py-2 pr-3 whitespace-nowrap font-mono text-xs"
                      style={{ fontFamily: monoFont }}
                    >
                      {e.site}
                    </td>
                    <td
                      className="py-2 pr-3 whitespace-nowrap font-mono text-xs text-muted-foreground"
                      style={{ fontFamily: monoFont }}
                    >
                      {e.elucidate || '—'}
                    </td>
                    <td className="py-2 pr-3 whitespace-nowrap text-xs">{cutTypeLabel(e)}</td>
                    <td className="py-2 pr-3 whitespace-nowrap font-mono text-xs tabular-nums text-muted-foreground">
                      {e.overhangLen != null && e.overhangLen !== 0 ? e.overhangLen : '—'}
                    </td>
                    <td className="py-2 pr-3 whitespace-nowrap font-mono text-xs tabular-nums text-muted-foreground">
                      {(e.site || '').length}
                    </td>
                    <td className="py-2 pr-3 whitespace-nowrap text-xs text-muted-foreground">
                      {e.isPalindromic ? 'Yes' : 'No'}
                    </td>
                    <td className="py-2 pr-3 whitespace-nowrap text-xs text-muted-foreground">
                      {e.isCutTwice ? 'Yes' : '—'}
                    </td>
                    <td className="py-2 whitespace-nowrap text-xs">
                      {e.methylation === 'sensitive' ? (
                        <span className="text-amber-700">Sensitive</span>
                      ) : e.methylationDependent ? (
                        <span className="text-teal-700">Required</span>
                      ) : (
                        <span className="text-muted-foreground/50">—</span>
                      )}
                    </td>
                  </tr>
                ))}
                {filtered.length === 0 && (
                  <tr>
                    <td colSpan={9} className="py-8 text-center text-sm text-muted-foreground">
                      No enzymes match “{query}”
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          )}
        </div>

        <DialogFooter className="mt-1">
          <div className="flex w-full items-center gap-2">
            <div className="mr-auto text-xs text-muted-foreground">
              {query
                ? `${filtered.length} of ${records ? records.length : 0} enzymes`
                : `${records ? records.length : 0} enzymes`}
            </div>
            <Button variant="outline" size="sm" onClick={() => onOpenChange(false)}>
              Close
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
