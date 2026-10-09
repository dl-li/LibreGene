import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { flushSync } from 'react-dom';
import { cw, startX } from '../editorConstants';

// Horizontal virtualization buffer in stream cells (each side).
const COL_BUF = 50;

// Scroll/viewport state: rAF-throttled scroll tracking, resize measurement and
// the layoutKey re-measure. Must be called before useTrackPacking (which
// consumes scrollY/viewportH). In continuous viewMode the editor container
// itself is the horizontal scroll host: scrollX/viewportW track its
// scrollLeft and feed the visibleCols window.
export default function useEditorViewport({
  scrollContainerRef,
  containerRef,
  setBaseCpl,
  layoutKey,
  viewMode,
}) {
  const continuous = viewMode === 'continuous';
  const [scrollY, setScrollY] = useState(0);
  const [viewportH, setViewportH] = useState(900);
  const [scrollX, setScrollX] = useState(0);
  const [viewportW, setViewportW] = useState(1200);
  const scrollTickingRef = useRef(false);
  const liveScrollTopRef = useRef(0);
  const liveScrollLeftRef = useRef(0);
  const lastVisibleStartRef = useRef(-1);
  const lastVisibleEndRef = useRef(-1);
  const numRowsRef = useRef(1);
  const avgRowPitchRef = useRef(60); // average row pitch, kept in sync at svgHeight

  // clientWidth includes the container's horizontal padding; baseCpl must fit
  // the inner content box or the svg (width = startX*2 + baseCpl*cw) overflows
  // it by up to the padding width and a stray horizontal scrollbar appears.
  const measureCpl = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const cs = window.getComputedStyle(el);
    const padX = parseFloat(cs.paddingLeft) + parseFloat(cs.paddingRight);
    setBaseCpl(Math.max(20, Math.floor((el.clientWidth - padX - startX * 2) / cw)));
    setViewportW(el.clientWidth || 1200);
  }, [containerRef, setBaseCpl]);

  useEffect(() => {
    const scroller = scrollContainerRef?.current;
    const handleResize = () => {
      measureCpl();
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
  }, [scrollContainerRef, measureCpl]);

  // Horizontal tracking on the editor container (continuous mode only).
  useEffect(() => {
    const el = containerRef.current;
    if (!continuous || !el) return undefined;
    const handleScroll = () => {
      liveScrollLeftRef.current = el.scrollLeft;
      if (!scrollTickingRef.current) {
        scrollTickingRef.current = true;
        requestAnimationFrame(() => {
          // flushSync: the clamped feature labels derive their x from scrollX,
          // so the state must reach the DOM before this frame paints —
          // otherwise edge-pinned labels wobble by up to one column.
          flushSync(() => {
            setViewportW(el.clientWidth || 1200);
            setScrollX(el.scrollLeft);
          });
          scrollTickingRef.current = false;
        });
      }
    };
    handleScroll();
    el.addEventListener('scroll', handleScroll, { passive: true });
    const ro = new ResizeObserver(handleScroll);
    ro.observe(el);
    return () => {
      el.removeEventListener('scroll', handleScroll);
      ro.disconnect();
    };
  }, [continuous, containerRef]);

  // Wheel → horizontal: vertical-dominant wheel deltas scroll the row
  // sideways; trackpad horizontal gestures and Shift+wheel stay native.
  useEffect(() => {
    const el = containerRef.current;
    if (!continuous || !el) return undefined;
    const onWheel = (e) => {
      if (e.shiftKey) return;
      if (Math.abs(e.deltaY) >= Math.abs(e.deltaX)) {
        e.preventDefault();
        el.scrollLeft += e.deltaY + e.deltaX;
      }
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [continuous, containerRef]);

  // Recalculate layout when parent padding changes (e.g. sidebar pin)
  useEffect(() => {
    if (layoutKey === undefined) return;
    measureCpl();
    const scroller = scrollContainerRef?.current;
    if (scroller) setViewportH(scroller.clientHeight || 900);
  }, [layoutKey, measureCpl, scrollContainerRef]);

  // Visible stream-cell window (continuous mode; null in wrap mode).
  // Hold a stable object identity while start/end are unchanged: downstream
  // memos (visibleEnzymes/visiblePrimers/visibleFeatures/GC track/...) key on
  // this object, and scrollX churns every frame — a fresh object would
  // recompute all of them even when the window didn't actually move.
  const visibleColsRef = useRef(null);
  const visibleCols = useMemo(() => {
    if (!continuous) {
      visibleColsRef.current = null;
      return null;
    }
    const start = Math.max(0, Math.floor((scrollX - startX) / cw) - COL_BUF);
    const end = Math.ceil((scrollX + viewportW - startX) / cw) + COL_BUF;
    const last = visibleColsRef.current;
    if (last && last.start === start && last.end === end) return last;
    const next = { start, end };
    visibleColsRef.current = next;
    return next;
  }, [continuous, scrollX, viewportW]);

  return {
    scrollY,
    setScrollY,
    viewportH,
    scrollX,
    setScrollX,
    viewportW,
    liveScrollTopRef,
    liveScrollLeftRef,
    numRowsRef,
    avgRowPitchRef,
    visibleCols,
  };
}

// Coordinate mapping + scroll anchoring over the packed row layout. Must be
// called after useTrackPacking (consumes its rowY/rowAbove/rowBelow/rowStarts/
// rowCount/rowOf/visibleRows outputs).
export function useViewportMapping({
  ROW_BUF,
  viewMode,
  scrollContainerRef,
  containerRef,
  svgRef,
  scrollToSeqIndexRef,
  liveScrollTopRef,
  liveScrollLeftRef,
  setScrollY,
  setScrollX,
  viewportH,
  viewportW,
  rowY,
  rowAbove,
  rowBelow,
  rowStarts,
  rowCounts,
  rowOf,
  numRows,
  charsPerLine,
  colVis,
  colFromVis,
  streamOf,
  absFromStream,
  cleanSeq,
  visibleRows,
  visibleCols,
  enzymes,
  enrichedPrimers,
}) {
  const continuous = viewMode === 'continuous';
  const getSeqY = useCallback((row) => rowY[Math.min(row, rowY.length - 1)], [rowY]);

  // Scroll so the row containing `seqIndex` is inside the viewport
  const scrollToSeqIndex = useCallback(
    (seqIndex) => {
      if (seqIndex == null || !rowY.length) return;
      const row = Math.min(rowY.length - 1, rowOf(seqIndex));
      const scroller = scrollContainerRef?.current;
      const rowTop = rowY[row] - (rowAbove[row] || 0);
      const rowBottom = rowY[row] + (rowBelow[row] || 0);
      const st = liveScrollTopRef.current;
      if (!(rowTop >= st && rowBottom <= st + viewportH)) {
        const newTop = Math.max(0, rowTop - 40);
        if (scroller) scroller.scrollTop = newTop;
        else window.scrollTo(0, newTop);
        liveScrollTopRef.current = newTop;
        setScrollY(newTop);
      }
      if (continuous) {
        const el = containerRef?.current;
        if (!el) return;
        const M = 60;
        const x = startX + colVis(seqIndex - rowStarts[row], row) * cw;
        const sl = el.scrollLeft;
        const vw = el.clientWidth || viewportW;
        let nl = sl;
        if (x < sl + M) nl = Math.max(0, x - M);
        else if (x > sl + vw - M) nl = x - vw + M;
        if (nl !== sl) {
          el.scrollLeft = nl;
          liveScrollLeftRef.current = nl;
          setScrollX(nl);
        }
      }
    },
    [
      rowY,
      rowAbove,
      rowBelow,
      rowOf,
      scrollContainerRef,
      containerRef,
      viewportH,
      viewportW,
      continuous,
      colVis,
      rowStarts,
      liveScrollLeftRef,
      liveScrollTopRef,
      setScrollY,
      setScrollX,
    ],
  );
  scrollToSeqIndexRef.current = scrollToSeqIndex;

  // Keep the same sequence position in view when the layout changes (feature/
  // primer/enzyme toggles, resize, mode switch): wrap anchors the top row's
  // first template column, continuous anchors the left-edge stream position.
  const rowAnchorRef = useRef(null);
  useLayoutEffect(() => {
    const prev = rowAnchorRef.current;
    rowAnchorRef.current = { rowY, rowAbove, rowStarts, continuous, colVis, absFromStream };
    if (!prev || !rowY.length || !prev.rowY.length) return;
    const scroller = scrollContainerRef?.current;
    if (continuous) {
      const el = containerRef?.current;
      if (!el) return;
      // Use the last scroll-event value: after a shrink the DOM scrollLeft may
      // already be clamped to the new max, which would corrupt the anchor.
      const sl = liveScrollLeftRef.current;
      let abs;
      let delta;
      if (prev.continuous) {
        const vis = Math.max(0, (sl - startX) / cw);
        abs = prev.absFromStream(vis);
        delta = sl - (startX + prev.colVis(abs - prev.rowStarts[0], 0) * cw);
      } else {
        // Switching from wrap: anchor the row that held the viewport top.
        const st = liveScrollTopRef.current;
        let r = 0;
        for (let i = 0; i < prev.rowY.length; i++) {
          if (prev.rowY[i] - (prev.rowAbove[i] || 0) <= st) r = i;
          else break;
        }
        abs = prev.rowStarts[r];
        delta = 0;
        if (scroller) scroller.scrollTop = 0;
        liveScrollTopRef.current = 0;
        setScrollY(0);
      }
      const newLeft = Math.max(0, startX + colVis(abs - rowStarts[0], 0) * cw + delta);
      if (Math.abs(newLeft - sl) < 1) return;
      el.scrollLeft = newLeft;
      liveScrollLeftRef.current = newLeft;
      setScrollX(newLeft);
      return;
    }
    // Use the last scroll-event value: after a shrink the DOM scrollTop may already
    // be clamped to the new max, which would corrupt the anchor row
    let st = liveScrollTopRef.current;
    let anchorAbs = null;
    if (prev.continuous) {
      // Switching from continuous: anchor the template column at the left edge.
      const sl = liveScrollLeftRef.current;
      anchorAbs = prev.absFromStream(Math.max(0, (sl - startX) / cw));
      st = 0;
    }
    // Anchor row: the row whose block (sequence line + space above) contains the viewport top
    let r = 0;
    for (let i = 0; i < prev.rowY.length; i++) {
      if (prev.rowY[i] - (prev.rowAbove[i] || 0) <= st) r = i;
      else break;
    }
    const delta = prev.continuous ? 0 : st - (prev.rowY[r] - (prev.rowAbove[r] || 0));
    // Anchor by the row's first template column so a changed row layout
    // (resize, insertion slots) restores to the same sequence position.
    const newR = Math.min(rowY.length - 1, rowOf(anchorAbs ?? prev.rowStarts[r]));
    const newTop = Math.max(0, rowY[newR] - (rowAbove[newR] || 0) + delta);
    if (Math.abs(newTop - st) < 1) return;
    if (scroller) scroller.scrollTop = newTop;
    else window.scrollTo(0, newTop);
    setScrollY(newTop);
  }, [
    rowY,
    rowAbove,
    rowStarts,
    rowOf,
    scrollContainerRef,
    containerRef,
    continuous,
    colVis,
    absFromStream,
  ]);

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

  // Filter enzymes to the visible row range (wrap) or stream-cell window
  // (continuous): labels beyond it would render thousands of px off-screen.
  const visibleEnzymes = useMemo(() => {
    if (!enzymes || !enzymes.length) return [];
    return enzymes.filter((e) => {
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      return pairs.some((cp) => {
        if (continuous) {
          if (!visibleCols) return false;
          const s = streamOf(cp.topCutIndex);
          return s >= visibleCols.start && s <= visibleCols.end;
        }
        const r = rowOf(cp.topCutIndex);
        return r >= visibleRows.start && r <= visibleRows.end;
      });
    });
  }, [enzymes, visibleRows, visibleCols, rowOf, streamOf, continuous]);

  // Virtualize primers: only render those overlapping the visible range
  const visiblePrimers = useMemo(() => {
    if (!enrichedPrimers.length) return [];
    if (continuous) {
      if (!visibleCols) return [];
      return enrichedPrimers.filter((p) => {
        if (p.matchStart === undefined || p.matchEnd === undefined) return false;
        const ml = p.mismatchStr?.length || 0;
        return (p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }]).some((m) => {
          const s0 = streamOf(Math.max(0, m.start - ml));
          const s1 = streamOf(m.end);
          return !(s1 < visibleCols.start || s0 > visibleCols.end);
        });
      });
    }
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
  }, [
    enrichedPrimers,
    visibleRows,
    visibleCols,
    numRows,
    rowOf,
    charsPerLine,
    streamOf,
    continuous,
  ]);

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
