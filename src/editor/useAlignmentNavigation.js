import { useCallback, useEffect, useMemo, useRef } from 'react';
import { cw, startX } from '../editorConstants';
import { alignmentNotableSites } from './alignmentLayout';

export default function useAlignmentNavigation({
  isDna,
  alignmentTracks,
  sequence,
  cleanSeq,
  continuous,
  alignNavRef,
  containerRef,
  scrollContainerRef,
  absFromStream,
  colVis,
  rowStarts,
  numRows,
  rowOf,
  rowY,
  rowAbove,
  rowAtSvgY,
}) {
  const sites = useMemo(() => {
    if (!isDna || !alignmentTracks.length) return [];
    const all = new Set();
    for (const al of alignmentTracks) {
      for (const s of alignmentNotableSites(al, sequence, cleanSeq.length)) all.add(s);
    }
    return [...all].sort((a, b) => a - b);
  }, [isDna, alignmentTracks, sequence, cleanSeq]);

  const rows = useMemo(() => {
    if (!sites.length) return [];
    const result = [];
    let lastRow = -1;
    for (const s of sites) {
      const r = Math.min(numRows - 1, rowOf(s));
      if (r !== lastRow) {
        result.push(r);
        lastRow = r;
      }
    }
    return result;
  }, [sites, rowOf, numRows]);

  const lastRef = useRef(null);
  const animRef = useRef(null);
  useEffect(() => () => clearTimeout(animRef.current), []);

  const jumpToAlignSite = useCallback(
    (dir) => {
      if (continuous) {
        if (!sites.length) return;
        const el = containerRef?.current;
        if (!el) return;
        const leftNow = el.scrollLeft;
        let baseSite;
        const last = lastRef.current;
        if (last && (animRef.current != null || Math.abs(leftNow - last.left) < 2)) {
          baseSite = last.site;
        } else {
          baseSite = absFromStream(Math.max(0, (leftNow + el.clientWidth / 2 - startX) / cw));
        }
        let target;
        if (dir > 0) {
          for (const s of sites) {
            if (s > baseSite) {
              target = s;
              break;
            }
          }
        } else {
          for (let i = sites.length - 1; i >= 0; i--) {
            if (sites[i] < baseSite) {
              target = sites[i];
              break;
            }
          }
        }
        if (target == null) return;
        const newLeft = Math.max(
          0,
          startX + colVis(target - rowStarts[0], 0) * cw - el.clientWidth / 2,
        );
        el.scrollTo({ left: newLeft, behavior: 'smooth' });
        lastRef.current = { site: target, left: newLeft };
        clearTimeout(animRef.current);
        animRef.current = setTimeout(() => {
          animRef.current = null;
        }, 500);
        return;
      }
      if (!rows.length) return;
      const scroller = scrollContainerRef?.current;
      const topNow = scroller ? scroller.scrollTop : window.scrollY;
      let baseRow;
      const last = lastRef.current;
      if (last && (animRef.current != null || Math.abs(topNow - last.top) < 2)) {
        baseRow = last.row;
      } else {
        const row = rowAtSvgY(topNow);
        baseRow = row >= 0 ? row : 0;
      }
      let target;
      if (dir > 0) {
        for (const r of rows) {
          if (r > baseRow) {
            target = r;
            break;
          }
        }
      } else {
        for (let i = rows.length - 1; i >= 0; i--) {
          if (rows[i] < baseRow) {
            target = rows[i];
            break;
          }
        }
      }
      if (target == null) return;
      const newTop = Math.max(0, rowY[target] - (rowAbove[target] || 0) - 40);
      if (scroller) scroller.scrollTo({ top: newTop, behavior: 'smooth' });
      else window.scrollTo({ top: newTop, behavior: 'smooth' });
      lastRef.current = { row: target, top: newTop };
      clearTimeout(animRef.current);
      animRef.current = setTimeout(() => {
        animRef.current = null;
      }, 500);
    },
    [
      rows,
      sites,
      scrollContainerRef,
      containerRef,
      continuous,
      rowAtSvgY,
      rowY,
      rowAbove,
      absFromStream,
      colVis,
      rowStarts,
    ],
  );

  useEffect(() => {
    if (alignNavRef) alignNavRef.current = jumpToAlignSite;
  }, [alignNavRef, jumpToAlignSite]);

  return { alignNavSites: sites, alignNavRows: rows, jumpToAlignSite };
}
