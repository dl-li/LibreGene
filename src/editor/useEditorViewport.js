import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { cw, startX } from '../editorConstants';

// Scroll/viewport state: rAF-throttled scroll tracking, resize measurement and
// the layoutKey re-measure. Must be called before useTrackPacking (which
// consumes scrollY/viewportH).
export default function useEditorViewport({
  scrollContainerRef,
  containerRef,
  setBaseCpl,
  layoutKey,
}) {
  const [scrollY, setScrollY] = useState(0);
  const [viewportH, setViewportH] = useState(900);
  const scrollTickingRef = useRef(false);
  const liveScrollTopRef = useRef(0);
  const lastVisibleStartRef = useRef(-1);
  const lastVisibleEndRef = useRef(-1);
  const numRowsRef = useRef(1);
  const avgRowPitchRef = useRef(60); // average row pitch, kept in sync at svgHeight

  useEffect(() => {
    const scroller = scrollContainerRef?.current;
    const handleResize = () => {
      if (containerRef.current) {
        setBaseCpl(Math.max(20, Math.floor((containerRef.current.clientWidth - startX * 2) / cw)));
      }
      if (scroller) setViewportH(scroller.clientHeight || 900);
    };
    const handleScroll = () => {
      liveScrollTopRef.current = scroller ? scroller.scrollTop : window.scrollY;
      if (!scrollTickingRef.current) {
        scrollTickingRef.current = true;
        requestAnimationFrame(() => {
          const sy = scroller ? scroller.scrollTop : window.scrollY;
          if (scroller) setViewportH(scroller.clientHeight || 900);
          const nr = numRowsRef.current;
          const estRowH = avgRowPitchRef.current || 60;
          const buf = 8;
          const vh = scroller ? scroller.clientHeight || 900 : window.innerHeight || 900;
          const estStart = Math.max(0, Math.floor(sy / estRowH) - buf - 1);
          const estEnd = Math.min(nr - 1, Math.floor((sy + vh) / estRowH) + buf + 1);
          if (estStart !== lastVisibleStartRef.current || estEnd !== lastVisibleEndRef.current) {
            lastVisibleStartRef.current = estStart;
            lastVisibleEndRef.current = estEnd;
            setScrollY(sy);
          }
          scrollTickingRef.current = false;
        });
      }
    };
    handleResize();
    window.addEventListener('resize', handleResize);
    const scrollTarget = scroller || window;
    scrollTarget.addEventListener('scroll', handleScroll, { passive: true });
    return () => {
      window.removeEventListener('resize', handleResize);
      scrollTarget.removeEventListener('scroll', handleScroll);
    };
  }, [scrollContainerRef]);

  // Recalculate layout when parent padding changes (e.g. sidebar pin)
  useEffect(() => {
    if (layoutKey === undefined) return;
    if (containerRef.current) {
      setBaseCpl(Math.max(20, Math.floor((containerRef.current.clientWidth - startX * 2) / cw)));
    }
    const scroller = scrollContainerRef?.current;
    if (scroller) setViewportH(scroller.clientHeight || 900);
  }, [layoutKey]);

  return { scrollY, setScrollY, viewportH, liveScrollTopRef, numRowsRef, avgRowPitchRef };
}

// Coordinate mapping + scroll anchoring over the packed row layout. Must be
// called after useTrackPacking (consumes its rowY/rowAbove/rowBelow/rowStarts/
// rowCount/rowOf/visibleRows outputs).
export function useViewportMapping({
  ROW_BUF,
  scrollContainerRef,
  svgRef,
  scrollToSeqIndexRef,
  liveScrollTopRef,
  setScrollY,
  viewportH,
  rowY,
  rowAbove,
  rowBelow,
  rowStarts,
  rowCounts,
  rowOf,
  numRows,
  charsPerLine,
  colFromVis,
  cleanSeq,
  visibleRows,
  enzymes,
  enrichedPrimers,
}) {
  const getSeqY = useCallback((row) => rowY[Math.min(row, rowY.length - 1)], [rowY]);

  // Scroll so the row containing `seqIndex` is inside the viewport (vertical only)
  const scrollToSeqIndex = useCallback(
    (seqIndex) => {
      if (seqIndex == null || !rowY.length) return;
      const row = Math.min(rowY.length - 1, rowOf(seqIndex));
      const scroller = scrollContainerRef?.current;
      const rowTop = rowY[row] - (rowAbove[row] || 0);
      const rowBottom = rowY[row] + (rowBelow[row] || 0);
      const st = liveScrollTopRef.current;
      if (rowTop >= st && rowBottom <= st + viewportH) return;
      const newTop = Math.max(0, rowTop - 40);
      if (scroller) scroller.scrollTop = newTop;
      else window.scrollTo(0, newTop);
      liveScrollTopRef.current = newTop;
      setScrollY(newTop);
    },
    [rowY, rowAbove, rowBelow, rowOf, scrollContainerRef, viewportH],
  );
  scrollToSeqIndexRef.current = scrollToSeqIndex;

  // Keep the same rows in view when row spacing changes (feature/primer/enzyme toggles, resize)
  const rowAnchorRef = useRef(null);
  useLayoutEffect(() => {
    const prev = rowAnchorRef.current;
    rowAnchorRef.current = { rowY, rowAbove, rowStarts };
    if (!prev || !rowY.length || !prev.rowY.length) return;
    const scroller = scrollContainerRef?.current;
    // Use the last scroll-event value: after a shrink the DOM scrollTop may already
    // be clamped to the new max, which would corrupt the anchor row
    const st = liveScrollTopRef.current;
    // Anchor row: the row whose block (sequence line + space above) contains the viewport top
    let r = 0;
    for (let i = 0; i < prev.rowY.length; i++) {
      if (prev.rowY[i] - (prev.rowAbove[i] || 0) <= st) r = i;
      else break;
    }
    const delta = st - (prev.rowY[r] - (prev.rowAbove[r] || 0));
    // Anchor by the row's first template column so a changed row layout
    // (resize, insertion slots) restores to the same sequence position.
    const newR = Math.min(rowY.length - 1, rowOf(prev.rowStarts[r]));
    const newTop = Math.max(0, rowY[newR] - (rowAbove[newR] || 0) + delta);
    if (Math.abs(newTop - st) < 1) return;
    if (scroller) scroller.scrollTop = newTop;
    else window.scrollTo(0, newTop);
    setScrollY(newTop);
  }, [rowY, rowAbove, rowStarts, rowOf, scrollContainerRef]);

  // --- selection: coordinate conversion & event handlers ---
  // Row tops (getSeqY(r) - rowAbove[r]) increase monotonically, so the row
  // containing a given y can be found by binary search.
  const rowAtSvgY = useCallback(
    (y) => {
      let lo = 0,
        hi = numRows - 1;
      while (lo <= hi) {
        const mid = (lo + hi) >> 1;
        const top = getSeqY(mid) - rowAbove[mid];
        if (y < top) {
          hi = mid - 1;
          continue;
        }
        const bottom = mid < numRows - 1 ? getSeqY(mid + 1) - rowAbove[mid + 1] : Infinity;
        if (y < bottom) return mid;
        lo = mid + 1;
      }
      return -1;
    },
    [numRows, getSeqY, rowAbove],
  );

  const clientToSeqIndex = useCallback(
    (clientX, clientY) => {
      if (!svgRef.current) return null;
      const pt = svgRef.current.createSVGPoint();
      pt.x = clientX;
      pt.y = clientY;
      const ctm = svgRef.current.getScreenCTM();
      if (!ctm) return null;
      const svgPt = pt.matrixTransform(ctm.inverse());
      const xRel = svgPt.x - startX;
      if (xRel < -cw / 2 || xRel > charsPerLine * cw + cw / 2) return null;
      const xInCell = ((xRel % cw) + cw) % cw;
      const colBase = Math.floor(xRel / cw);
      const side = xInCell < cw / 2 ? 0 : 1;
      const visCol = Math.max(0, Math.min(charsPerLine, colBase + side));
      // Content-based row boundaries: top of current row → top of next row
      // Row spacing already ensures a gap between row content areas
      const row = rowAtSvgY(svgPt.y);
      if (row < 0) return null;
      const col = colFromVis(visCol, row);
      const idx = rowStarts[row] + col;
      return Math.max(0, Math.min(cleanSeq.length, idx));
    },
    [charsPerLine, rowAtSvgY, cleanSeq, colFromVis, rowStarts],
  );

  // Returns which character the pointer is over (0-based char index), not the insertion point
  const clientToCharIndex = useCallback(
    (clientX, clientY) => {
      if (!svgRef.current) return null;
      const pt = svgRef.current.createSVGPoint();
      pt.x = clientX;
      pt.y = clientY;
      const ctm = svgRef.current.getScreenCTM();
      if (!ctm) return null;
      const svgPt = pt.matrixTransform(ctm.inverse());
      const xRel = svgPt.x - startX;
      if (xRel < -cw / 2 || xRel > charsPerLine * cw + cw / 2) return null;
      let vis = Math.floor(xRel / cw);
      if (xRel < 0) vis = 0;
      if (vis > charsPerLine) vis = charsPerLine - 1;
      const row = rowAtSvgY(svgPt.y);
      if (row < 0) return null;
      const col = Math.max(0, Math.min(rowCounts[row] - 1, colFromVis(vis, row)));
      const idx = rowStarts[row] + col;
      return Math.max(0, Math.min(cleanSeq.length - 1, idx));
    },
    [charsPerLine, rowAtSvgY, cleanSeq, colFromVis, rowStarts, rowCounts],
  );

  // Filter enzymes to visible row range only
  const visibleEnzymes = useMemo(() => {
    if (!enzymes || !enzymes.length) return [];
    return enzymes.filter((e) => {
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      return pairs.some((cp) => {
        const r = rowOf(cp.topCutIndex);
        return r >= visibleRows.start && r <= visibleRows.end;
      });
    });
  }, [enzymes, visibleRows, rowOf]);

  // Virtualize primers: only render those overlapping visible rows
  const visiblePrimers = useMemo(() => {
    if (!enrichedPrimers.length) return [];
    const vs = Math.max(0, visibleRows.start - ROW_BUF);
    const ve = Math.min(numRows - 1, visibleRows.end + ROW_BUF);
    return enrichedPrimers.filter((p) => {
      if (p.matchStart === undefined || p.matchEnd === undefined) return false;
      const ml = p.mismatchStr?.length || 0;
      const pad = Math.ceil(ml / charsPerLine);
      return (p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }]).some((m) => {
        const sr = rowOf(m.start);
        const er = rowOf(m.end);
        return !(er < vs - pad || sr > ve + pad);
      });
    });
  }, [enrichedPrimers, visibleRows, numRows, rowOf, charsPerLine]);

  return {
    getSeqY,
    scrollToSeqIndex,
    rowAtSvgY,
    clientToSeqIndex,
    clientToCharIndex,
    visibleEnzymes,
    visiblePrimers,
  };
}
