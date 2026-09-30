import { useEffect, useMemo, useState } from 'react';
import { buildMatchSegs } from './alignmentLayout';
import { complementStr } from './seqUtils';
import { computePrimerAlignment } from '../tauriApi';

// Primer enrichment (flat render fields from the bindingSites data model) plus
// the pre-computed primer alignment cache consumed by PrimerAlignmentDialog.
export default function useEnrichedPrimers({
  primers,
  cleanSeq,
  alignmentCacheRef,
  primerSeedLength,
  tmParams,
}) {
  const [primerAlignmentCache, setPrimerAlignmentCache] = useState({});

  // Sync alignment cache to parent ref (used by PrimerOverviewDialog)
  useEffect(() => {
    if (alignmentCacheRef) {
      alignmentCacheRef.current = primerAlignmentCache;
    }
  }, [primerAlignmentCache, alignmentCacheRef]);

  // Enrich primers with flat fields from bindingSites data model (v2).
  const enrichedPrimers = useMemo(
    () =>
      (primers || []).map((p) => {
        // Already enriched (legacy flat fields or pre-computed).
        if (p.matchStart !== undefined && p.matchEnd !== undefined) {
          const matchSegs = buildMatchSegs(p.matchStart, p.matchEnd, cleanSeq.length);
          if (p.isFwd === false) return { ...p, color: '#4A148C', matchSegs };
          return { ...p, color: '#166534', matchSegs };
        }
        const bs = p.bindingSites?.[0];
        if (!bs) return p;
        // templateStart (inclusive), templateEnd (exclusive) — convert to legacy inclusive matchEnd
        const ms = bs.templateStart ?? bs.matchStart ?? 0;
        const me = bs.templateEnd != null ? bs.templateEnd - 1 : (bs.matchEnd ?? 0);
        const tlen = cleanSeq.length;
        const aln = bs.alignment || {};
        const ds = aln.displaySequence || '';
        const misSet = new Set(aln.mismatchIndices || []);
        // Build per-column render data for the binding region
        const renderCols = [];
        for (let i = 0; i < ds.length; i++) {
          const ch = ds[i];
          const tcol = tlen > 0 ? (ms + i) % tlen : ms + i;
          let kind, primerBase, insDetail;
          if (ch === '-') {
            kind = 'gap';
            primerBase = '-';
          } else if (ch >= '0' && ch <= '9') {
            kind = 'insertion';
            primerBase = ch;
            insDetail = aln.insertionMap?.[ch];
          } else if (misSet.has(i)) {
            kind = 'mismatch';
            primerBase = ch;
          } else {
            kind = 'match';
            primerBase = ch;
          }
          renderCols.push({ templateCol: tcol, kind, primerBase, insDetail, displayIdx: i });
        }
        const isFwd = (bs.strand ?? 1) === 1;
        const matchSegs = buildMatchSegs(ms, me, tlen);
        const matchedBases = matchSegs.map((m) => cleanSeq.substring(m.start, m.end + 1)).join('');
        return {
          ...p,
          matchStart: ms,
          matchEnd: me,
          matchSegs,
          isFwd, // actual binding direction (NOT declared type)
          // Primer colors follow the app-wide direction convention: fwd green,
          // rev deep purple. Imported file colors (e.g. SnapGene notes) are not
          // surfaced on the sequence view.
          color: isFwd ? '#166534' : '#4A148C',
          matchStr: isFwd ? matchedBases : complementStr(matchedBases),
          // tails for rendering
          mismatchStr: bs.fivePrimeTail || '',
          threePrimeTail: bs.threePrimeTail || '',
          // rich alignment data
          renderCols,
          displaySequence: ds,
        };
      }),
    [primers, cleanSeq],
  );

  // Primers that have no binding sites at all — won't appear on the sequence
  const unmatchedPrimers = useMemo(() => {
    return (primers || []).filter((p) => !p.bindingSites?.length);
  }, [primers]);

  // Clear alignment cache when primers change to avoid stale data.
  useEffect(() => {
    setPrimerAlignmentCache({});
  }, [primers]);

  // Pre-compute primer alignment data so the dialog has zero flash/width-jump.
  useEffect(() => {
    let cancelled = false;
    async function prefetchAll() {
      const cache = {};
      const items = primers || [];
      // Use Promise.allSettled for parallelism, but limit concurrency.
      const concurrency = 4;
      for (let i = 0; i < items.length; i += concurrency) {
        const batch = items.slice(i, i + concurrency);
        const results = await Promise.allSettled(
          batch.map((p) =>
            computePrimerAlignment(p.id, primerSeedLength, undefined, undefined, tmParams),
          ),
        );
        if (cancelled) return;
        for (let j = 0; j < batch.length; j++) {
          if (results[j].status === 'fulfilled') {
            cache[batch[j].id] = results[j].value;
          }
        }
      }
      if (!cancelled) setPrimerAlignmentCache(cache);
    }
    prefetchAll();
    return () => {
      cancelled = true;
    };
  }, [primers, primerSeedLength, tmParams]);

  return { enrichedPrimers, unmatchedPrimers, primerAlignmentCache };
}
