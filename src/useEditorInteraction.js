import { useState, useEffect, useRef, useCallback } from 'react';
import { getX, cw, startX } from './editorConstants';

export default function useEditorInteraction({
  cleanSeq, charsPerLine, numRows, rowY, getSeqY, rowAbove, rowBelow,
  enzymes, primers,
  svgRef, containerRef,
  setCharsPerLine,
}) {
  const [hoveredFeature, setHoveredFeature] = useState(null);
  const [hoveredPrimer, setHoveredPrimer] = useState(null);
  const [hoveredEnzyme, setHoveredEnzyme] = useState(null);
  const [scrollY, setScrollY] = useState(0);
  const [optionHeld, setOptionHeld] = useState(false);
  const [cursorPos, setCursorPos] = useState(0);
  const [anchorPos, setAnchorPos] = useState(null);
  const [isDragging, setIsDragging] = useState(false);
  const [selectedPrimerId, setSelectedPrimerId] = useState(null);
  const [pairedPrimerId, setPairedPrimerId] = useState(null);
  const [pairingMode, setPairingMode] = useState(false);
  const [pairingHoveredId, setPairingHoveredId] = useState(null);
  const [shiftHeld, setShiftHeld] = useState(false);
  const [cursorIdle, setCursorIdle] = useState(false);
  const [selectedEnzymeId, setSelectedEnzymeId] = useState(null);
  const [enzymePairId, setEnzymePairId] = useState(null);
  const [enzymeDragMode, setEnzymeDragMode] = useState(false);
  const [enzymeDragHoverId, setEnzymeDragHoverId] = useState(null);

  const prevCursorIdle = useRef(false);
  useEffect(() => { prevCursorIdle.current = cursorIdle; });
  const isDraggingRef = useRef(false);
  const dragOriginRef = useRef(null);
  const scrollRafRef = useRef(null);
  const primerDragRef = useRef(null);
  const primerDragMovedRef = useRef(false);
  const primerDragStartXRef = useRef(0);
  const primerDragStartYRef = useRef(0);
  const pairingHoveredRef = useRef(null);
  const enzymeDragRef = useRef(null);
  const enzymeDragMovedRef = useRef(false);
  const enzymeDragStartXRef = useRef(0);
  const enzymeDragStartYRef = useRef(0);
  const enzymeDragHoverIdRef = useRef(null);

  useEffect(() => { enzymeDragHoverIdRef.current = enzymeDragHoverId; });
  const enzymeDragSourceCutIndex = useRef(null);

  // Refs to access latest values in window event handlers
  const cursorPosRef = useRef(cursorPos);
  cursorPosRef.current = cursorPos;
  const anchorPosRef = useRef(anchorPos);
  anchorPosRef.current = anchorPos;
  const rowYRef = useRef(rowY);
  rowYRef.current = rowY;
  const rowAboveRef = useRef(rowAbove);
  rowAboveRef.current = rowAbove;
  const rowBelowRef = useRef(rowBelow);
  rowBelowRef.current = rowBelow;
  const numRowsRef = useRef(numRows);
  numRowsRef.current = numRows;
  const charsPerLineRef = useRef(charsPerLine);
  charsPerLineRef.current = charsPerLine;
  const cleanSeqLenRef = useRef(cleanSeq.length);
  cleanSeqLenRef.current = cleanSeq.length;
  const selectedPrimerIdRef = useRef(null);
  selectedPrimerIdRef.current = selectedPrimerId;
  const pairedPrimerIdRef = useRef(null);
  pairedPrimerIdRef.current = pairedPrimerId;
  const primersRef = useRef(primers);
  primersRef.current = primers;
  const cleanSeqRef = useRef(cleanSeq);
  cleanSeqRef.current = cleanSeq;
  const enzymeSelectionRangeRef = useRef(null);
  const getSeqYRef = useRef(getSeqY);
  getSeqYRef.current = getSeqY;

  useEffect(() => {
    setCursorIdle(false);
    const timer = setTimeout(() => setCursorIdle(true), 4500);
    return () => clearTimeout(timer);
  }, [cursorPos]);

  // Full client → absolute index
  const clientToAbsIndex = useCallback((clientX, clientY) => {
    const svg = svgRef.current;
    if (!svg) return null;

    const gSeqY = getSeqYRef.current;
    const rAbove = rowAboveRef.current;
    const rBelow = rowBelowRef.current;
    if (!gSeqY) return null;

    const rect = svg.getBoundingClientRect();
    const svgX = clientX - rect.left;
    const svgY = clientY - rect.top;

    const cpl = charsPerLineRef.current;
    const nRows = numRowsRef.current;
    const seqLen = cleanSeqLenRef.current;

    let row = -1;
    for (let r = 0; r < nRows; r++) {
      if (svgY >= gSeqY(r) - rAbove[r] && svgY <= gSeqY(r) + rBelow[r]) { row = r; break; }
    }
    if (row < 0) {
      if (svgY < gSeqY(0) - rAbove[0]) row = 0;
      else if (svgY > gSeqY(nRows - 1) + rBelow[nRows - 1]) row = nRows - 1;
      else return null;
    }

    const col = Math.max(0, Math.min(cpl, Math.round((svgX - startX) / cw)));
    return Math.max(0, Math.min(seqLen, row * cpl + col));
  }, [svgRef]);

  const clientToAbsIndexRef = useRef(clientToAbsIndex);
  clientToAbsIndexRef.current = clientToAbsIndex;

  // ── Resize & scroll ─────────────────────────────────────────────
  useEffect(() => {
    const handleResize = () => {
      if (containerRef.current) {
        setCharsPerLine(Math.max(20, Math.floor((containerRef.current.clientWidth - startX * 2) / cw)));
      }
    };
    const handleScroll = () => {
      if (!scrollRafRef.current) {
        scrollRafRef.current = requestAnimationFrame(() => {
          setScrollY(window.scrollY);
          scrollRafRef.current = null;
        });
      }
    };
    handleResize();
    window.addEventListener('resize', handleResize);
    window.addEventListener('scroll', handleScroll, { passive: true });
    return () => {
      window.removeEventListener('resize', handleResize);
      window.removeEventListener('scroll', handleScroll);
      if (scrollRafRef.current) cancelAnimationFrame(scrollRafRef.current);
    };
  }, [containerRef, setCharsPerLine]);

  // ── Keyboard handlers ───────────────────────────────────────────
  useEffect(() => {
    const handleKeyDown = (e) => {
      if (e.key === 'Alt') { setOptionHeld(true); return; }
      if (e.key === 'Shift') { setShiftHeld(true); return; }
      if ((e.metaKey || e.ctrlKey) && e.key === 'c') {
        if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;
        e.preventDefault();
        const selId = selectedPrimerIdRef.current;
        const primersArr = primersRef.current;
        const seq = cleanSeqRef.current;
        let text = '';
        if (selId) {
          const pairId = pairedPrimerIdRef.current;
          if (pairId) {
            const fwdP = primersArr.find(p => p.id === selId);
            const revP = primersArr.find(p => p.id === pairId);
            const fwd = fwdP?.type === 'fwd' ? fwdP : revP;
            const rev = fwdP?.type === 'rev' ? fwdP : revP;
            if (fwd && rev && fwd.type === 'fwd' && rev.type === 'rev') {
              const fwdMatchEnd = fwd.bindingSites?.[0]?.matchEnd ?? 0;
              const revMatchStart = rev.bindingSites?.[0]?.matchStart ?? 0;
              const DNA_COMP = { A: 'T', T: 'A', G: 'C', C: 'G', a: 't', t: 'a', g: 'c', c: 'g' };
              const revComp = (s) => { let r = ''; for (let i = s.length - 1; i >= 0; i--) r += DNA_COMP[s[i]] || s[i]; return r; };
              const revRC = revComp(rev.primerSeq || '');
              let midTmpl;
              if (fwdMatchEnd < revMatchStart) {
                midTmpl = seq.substring(fwdMatchEnd + 1, revMatchStart);
              } else if (fwdMatchEnd > revMatchStart) {
                midTmpl = seq.substring(fwdMatchEnd + 1) + seq.substring(0, revMatchStart);
              } else {
                midTmpl = '';
              }
              text = (fwd.primerSeq || '') + midTmpl + revRC;
            }
          } else {
            const p = primersArr.find(p => p.id === selId);
            if (p) {
              text = p.primerSeq || '';
            }
          }
        } else {
          const esr = enzymeSelectionRangeRef.current;
          if (esr) {
            const { start, end } = esr;
            if (start <= end) text = seq.substring(start, end + 1);
          } else {
            const anchor = anchorPosRef.current;
            if (anchor !== null) {
              const start = Math.min(anchor, cursorPosRef.current);
              const end = Math.max(anchor, cursorPosRef.current);
              if (start < end) text = seq.substring(start, end);
            }
          }
        }
        if (text) navigator.clipboard.writeText(text).catch(() => {});
        return;
      }
      if (e.key === 'ArrowLeft' || e.key === 'ArrowRight' || e.key === 'ArrowUp' || e.key === 'ArrowDown') {
        e.preventDefault();
        const cp = cursorPosRef.current;
        const cpl = charsPerLineRef.current;
        const seqLen = cleanSeqLenRef.current;
        let newPos = cp;
        if (e.key === 'ArrowLeft') newPos = Math.max(0, cp - 1);
        else if (e.key === 'ArrowRight') newPos = Math.min(seqLen, cp + 1);
        else if (e.key === 'ArrowUp') newPos = Math.max(0, cp - cpl);
        else if (e.key === 'ArrowDown') newPos = Math.min(seqLen, cp + cpl);
        if (e.shiftKey) {
          setAnchorPos(prev => prev ?? cp);
        } else {
          setAnchorPos(null);
        }
        setCursorPos(newPos);
      }
    };
    const handleKeyUp = (e) => {
      if (e.key === 'Alt') setOptionHeld(false);
      if (e.key === 'Shift') setShiftHeld(false);
    };
    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('keyup', handleKeyUp);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('keyup', handleKeyUp);
    };
  }, []);

  // ── Mouse handlers ──────────────────────────────────────────────
  const handleSeqMouseDown = useCallback((e) => {
    if (e.button !== 0) return;
    const idx = clientToAbsIndexRef.current(e.clientX, e.clientY);
    if (idx === null) return;

    setSelectedPrimerId(null);
    setPairedPrimerId(null);
    setPairingMode(false);
    setPairingHoveredId(null);
    primerDragRef.current = null;

    setSelectedEnzymeId(null);
    setEnzymePairId(null);
    setEnzymeDragMode(false);
    setEnzymeDragHoverId(null);
    enzymeDragRef.current = null;

    e.preventDefault();
    dragOriginRef.current = idx;
    isDraggingRef.current = true;
    setIsDragging(true);
    setHoveredEnzyme(null);
    setHoveredPrimer(null);

    if (e.shiftKey) {
      setAnchorPos(prev => prev ?? cursorPosRef.current);
    } else {
      setAnchorPos(null);
    }
    setCursorPos(idx);
  }, []);

  const handleFeatureSelect = useCallback((feature) => {
    setSelectedPrimerId(null);
    setPairedPrimerId(null);
    setPairingMode(false);
    setSelectedEnzymeId(null);
    setEnzymePairId(null);
    setEnzymeDragMode(false);
    if (!feature?.segments?.length) return;
    const start = Math.min(...feature.segments.map(s => s.start));
    const end = Math.max(...feature.segments.map(s => s.end));
    setAnchorPos(start);
    setCursorPos(end + 1);
  }, []);

  const handlePrimerMouseDown = useCallback((primerId, e) => {
    e.stopPropagation();
    const primersArr = primersRef.current;
    const primer = primersArr.find(p => p.id === primerId);
    if (!primer) return;
    isDraggingRef.current = false;
    setIsDragging(false);
    setAnchorPos(null);
    setSelectedEnzymeId(null);
    setEnzymePairId(null);
    setEnzymeDragMode(false);
    setEnzymeDragHoverId(null);
    enzymeDragRef.current = null;

    const prevSelId = selectedPrimerIdRef.current;

    if (primerId !== prevSelId) {
      setSelectedPrimerId(primerId);
      setPairedPrimerId(null);
    }

    if (e.shiftKey && prevSelId && prevSelId !== primerId) {
      const prevPrimer = primersArr.find(p => p.id === prevSelId);
      if (prevPrimer && primer.type !== prevPrimer.type) {
        const fwd = prevPrimer.type === 'fwd' ? prevSelId : primerId;
        const rev = prevPrimer.type === 'rev' ? prevSelId : primerId;
        setSelectedPrimerId(fwd);
        setPairedPrimerId(rev);
        setPairingMode(false);
        setPairingHoveredId(null);
        primerDragRef.current = null;
        return;
      }
    }

    primerDragRef.current = primerId;
    primerDragMovedRef.current = false;
    primerDragStartXRef.current = e.clientX;
    primerDragStartYRef.current = e.clientY;
    setPairingHoveredId(null);
  }, []);

  const handlePairingHover = useCallback((primerId) => {
    if (primerId === null) {
      pairingHoveredRef.current = null;
      setPairingHoveredId(null);
      return;
    }
    const dragId = primerDragRef.current;
    if (!dragId) return;
    const primersArr = primersRef.current;
    const dragPrimer = primersArr.find(p => p.id === dragId);
    const hoveredP = primersArr.find(p => p.id === primerId);
    if (dragPrimer && hoveredP && hoveredP.type !== dragPrimer.type) {
      pairingHoveredRef.current = primerId;
      setPairingHoveredId(primerId);
    }
  }, []);

  const handleEnzymeMouseDown = useCallback((enzymeId, e) => {
    e.stopPropagation();
    e.preventDefault();
    setHoveredEnzyme(null);
    setSelectedPrimerId(null);
    setPairedPrimerId(null);
    setPairingMode(false);
    setPairingHoveredId(null);
    primerDragRef.current = null;
    setAnchorPos(null);

    if (e.shiftKey && selectedEnzymeId && selectedEnzymeId !== enzymeId) {
      setEnzymePairId(enzymeId);
      setEnzymeDragMode(false);
      setEnzymeDragHoverId(null);
      enzymeDragRef.current = null;
      return;
    }

    setSelectedEnzymeId(enzymeId);
    setEnzymePairId(null);
    setEnzymeDragMode(false);
    setEnzymeDragHoverId(null);

    enzymeDragRef.current = enzymeId;
    enzymeDragMovedRef.current = false;
    enzymeDragStartXRef.current = e.clientX;
    enzymeDragStartYRef.current = e.clientY;
    const srcEnzyme = enzymes.find(x => x.id === enzymeId);
    enzymeDragSourceCutIndex.current = srcEnzyme ? srcEnzyme.cutIndex : null;
  }, [selectedEnzymeId, enzymes]);

  const handleEnzymeDragEnter = useCallback((enzymeId) => {
    if (!enzymeDragRef.current || enzymeId === enzymeDragRef.current) return;
    const e = enzymes.find(x => x.id === enzymeId);
    if (e && e.cutIndex === enzymeDragSourceCutIndex.current) return;
    enzymeDragHoverIdRef.current = enzymeId;
    setEnzymeDragHoverId(enzymeId);
  }, [enzymes]);

  const handleEnzymeDragLeave = useCallback(() => {
    enzymeDragHoverIdRef.current = null;
    setEnzymeDragHoverId(null);
  }, []);

  // ── Mouse move/up listeners ─────────────────────────────────────
  useEffect(() => {
    const DRAG_THRESHOLD = 3;
    const handleMouseMove = (e) => {
      if (enzymeDragRef.current) {
        if (!enzymeDragMovedRef.current) {
          const dx = e.clientX - enzymeDragStartXRef.current;
          const dy = e.clientY - enzymeDragStartYRef.current;
          if (dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD) {
            enzymeDragMovedRef.current = true;
            setEnzymeDragMode(true);
          }
        }
        return;
      }
      if (primerDragRef.current) {
        if (!primerDragMovedRef.current) {
          const dx = e.clientX - primerDragStartXRef.current;
          const dy = e.clientY - primerDragStartYRef.current;
          if (dx * dx + dy * dy < DRAG_THRESHOLD * DRAG_THRESHOLD) return;
          primerDragMovedRef.current = true;
          setPairingMode(true);
        }
        return;
      }
      if (!isDraggingRef.current) return;
      const idx = clientToAbsIndexRef.current(e.clientX, e.clientY);
      if (idx === null) return;
      if (anchorPosRef.current === null && idx !== dragOriginRef.current) {
        setAnchorPos(dragOriginRef.current);
      }
      setCursorPos(idx);
    };

    const handleMouseUp = (e) => {
      if (enzymeDragRef.current) {
        if (enzymeDragMovedRef.current) {
          const targetId = enzymeDragHoverIdRef.current;
          if (targetId) {
            setEnzymePairId(targetId);
          } else {
            setSelectedEnzymeId(null);
          }
        }
        enzymeDragRef.current = null;
        enzymeDragMovedRef.current = false;
        enzymeDragHoverIdRef.current = null;
        setEnzymeDragMode(false);
        setEnzymeDragHoverId(null);
        return;
      }
      if (primerDragRef.current) {
        if (primerDragMovedRef.current) {
          const targetId = pairingHoveredRef.current;
          if (targetId) {
            const dragId = primerDragRef.current;
            const primersArr = primersRef.current;
            const dragPrimer = primersArr.find(p => p.id === dragId);
            const targetPrimer = primersArr.find(p => p.id === targetId);
            if (dragPrimer && targetPrimer && dragPrimer.type !== targetPrimer.type) {
              const fwd = dragPrimer.type === 'fwd' ? dragId : targetId;
              const rev = dragPrimer.type === 'rev' ? dragId : targetId;
              setSelectedPrimerId(fwd);
              setPairedPrimerId(rev);
            }
          } else {
            setSelectedPrimerId(null);
            setPairedPrimerId(null);
            setHoveredPrimer(null);
          }
        }
        primerDragRef.current = null;
        primerDragMovedRef.current = false;
        pairingHoveredRef.current = null;
        setPairingMode(false);
        setPairingHoveredId(null);
        return;
      }
      if (!isDraggingRef.current) return;
      isDraggingRef.current = false;
      setIsDragging(false);
    };

    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
    };
  }, []);

  return {
    hoveredFeature, setHoveredFeature,
    hoveredPrimer, setHoveredPrimer,
    hoveredEnzyme, setHoveredEnzyme,
    scrollY,
    optionHeld,
    cursorPos, setCursorPos,
    anchorPos, setAnchorPos,
    isDragging,
    selectedPrimerId, pairedPrimerId,
    pairingMode, pairingHoveredId,
    shiftHeld,
    cursorIdle, prevCursorIdle,
    selectedEnzymeId, enzymePairId,
    enzymeDragMode, enzymeDragHoverId,
    enzymeDragSourceCutIndex,
    enzymeSelectionRangeRef,
    handleSeqMouseDown,
    handleFeatureSelect,
    handlePrimerMouseDown,
    handlePairingHover,
    handleEnzymeMouseDown,
    handleEnzymeDragEnter,
    handleEnzymeDragLeave,
    isDraggingRef,
  };
}
