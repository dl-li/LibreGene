import { useEffect, useMemo, useState } from 'react';
import { buildMatchSegs } from './alignmentLayout';
import { complementStr } from './seqUtils';
import { computePrimerAlignment } from '../tauriApi';

const FWD_COLOR = '#166534';
const REV_COLOR = '#4A148C';
const MULTI_SITE_COLOR = '#C2410C';

export function enrichPrimers(primers, cleanSeq) {
  return (primers || []).flatMap((p) => {
    const sites = p.bindingSites || [];
    if (sites.length > 0) {
      const color = sites.length > 1 ? MULTI_SITE_COLOR : null;
      return sites.map((bs, siteIndex) => {
        const isFwd = (bs.strand ?? 1) === 1;
        const ms = bs.templateStart ?? bs.matchStart ?? 0;
        const me = bs.templateEnd != null ? bs.templateEnd - 1 : (bs.matchEnd ?? 0);
        const tlen = cleanSeq.length;
        const aln = bs.alignment || {};
        const ds = aln.displaySequence || '';
        const misSet = new Set(aln.mismatchIndices || []);
        const mod = (n) => (tlen > 0 ? ((n % tlen) + tlen) % tlen : n);
        const dStartRaw = ms + (aln.startOffset || 0);
        const dStart = mod(dStartRaw);
        const dEnd = ds.length > 0 ? mod(dStartRaw + ds.length - 1) : me;
        const renderCols = [];
        for (let i = 0; i < ds.length; i++) {
          const ch = ds[i];
          let kind;
          if (ch === '-') kind = 'gap';
          else if (ch === '·') kind = 'insDot';
          else if (misSet.has(i)) kind = 'mismatch';
          else kind = 'match';
          renderCols.push({
            templateCol: mod(dStartRaw + i),
            kind,
            primerBase: ch,
            displayIdx: i,
          });
        }
        const anchorSegs = buildMatchSegs(ms, me, tlen);
        const matchSegs = ds.length > 0 ? buildMatchSegs(dStart, dEnd, tlen) : anchorSegs;
        const matchedBases = anchorSegs.map((m) => cleanSeq.substring(m.start, m.end + 1)).join('');
        return {
          ...p,
          renderKey: `${p.id}:site-${siteIndex}`,
          matchStart: ms,
          matchEnd: me,
          matchSegs,
          isFwd,
          color: color || (isFwd ? FWD_COLOR : REV_COLOR),
          matchStr: isFwd ? matchedBases : complementStr(matchedBases),
          mismatchStr: bs.displayTail ?? bs.fivePrimeTail ?? '',
          threePrimeTail: bs.threePrimeTail || '',
          renderCols,
          displaySequence: ds,
        };
      });
    }
    if (p.matchStart === undefined || p.matchEnd === undefined) return p;
    return {
      ...p,
      renderKey: p.id,
      color: p.isFwd === false ? REV_COLOR : FWD_COLOR,
      matchSegs: buildMatchSegs(p.matchStart, p.matchEnd, cleanSeq.length),
    };
  });
}

// Primer enrichment (flat render fields from the bindingSites data model) plus
// the pre-computed primer alignment cache consumed by PrimerAlignmentDialog.
export default function useEnrichedPrimers({
  primers,
  cleanSeq,
  alignmentCacheRef,
  primerSeedLength,
  tmParams,
  projectId,
  prefetchAlignments = false,
}) {
  const [primerAlignmentCache, setPrimerAlignmentCache] = useState({});

  // Sync alignment cache to parent ref (used by PrimerOverviewDialog)
  useEffect(() => {
    if (alignmentCacheRef) {
      alignmentCacheRef.current = primerAlignmentCache;
    }
  }, [primerAlignmentCache, alignmentCacheRef]);

  const enrichedPrimers = useMemo(() => enrichPrimers(primers, cleanSeq), [primers, cleanSeq]);

  // Primers that have no binding sites at all — won't appear on the sequence
  const unmatchedPrimers = useMemo(() => {
    return (primers || []).filter((p) => !p.bindingSites?.length);
  }, [primers]);

  // Clear alignment cache when primers change to avoid stale data.
  useEffect(() => {
    setPrimerAlignmentCache({});
  }, [primers]);

  useEffect(() => {
    if (!prefetchAlignments) {
      setPrimerAlignmentCache({});
      return undefined;
    }
    let cancelled = false;
    async function prefetchAll() {
      const cache = {};
      const items = primers || [];
      const concurrency = 4;
      for (let i = 0; i < items.length; i += concurrency) {
        const batch = items.slice(i, i + concurrency);
        const results = await Promise.allSettled(
          batch.map((p) =>
            computePrimerAlignment(
              p.id,
              primerSeedLength,
              undefined,
              undefined,
              tmParams,
              projectId,
            ),
          ),
        );
        if (cancelled) return;
        for (let j = 0; j < batch.length; j++) {
          if (results[j].status === 'fulfilled') cache[batch[j].id] = results[j].value;
        }
      }
      if (!cancelled) setPrimerAlignmentCache(cache);
    }
    prefetchAll();
    return () => {
      cancelled = true;
    };
  }, [primers, primerSeedLength, tmParams, projectId, prefetchAlignments]);

  return { enrichedPrimers, unmatchedPrimers, primerAlignmentCache };
}
