import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { enzymeActiveBlue } from '../editorConstants';

export default function useEditorSelection({ restoreState, onSelectionChange, topology }) {
  const [cursorIndex, setCursorIndex] = useState(null);
  const [selStart, setSelStart] = useState(null);
  const [selEnd, setSelEnd] = useState(null);
  const [isDragging, setIsDragging] = useState(false);
  const [selectionTm, setSelectionTm] = useState(null);
  const tmLastRunRef = useRef(0);
  const tmTimerRef = useRef(null);
  const tmSeqRef = useRef(null);
  const dragRef = useRef({ startIdx: null, active: false });
  const isDraggingRef = useRef(false);
  const autoScrollRef = useRef({ clientX: 0, clientY: 0 });
  const [hoveredIndex, setHoveredIndex] = useState(null);
  const hoveredIndexRef = useRef(null);
  const cursorTimerRef = useRef(null);
  const [cursorVisible, setCursorVisible] = useState(true);

  const [designPick, setDesignPick] = useState(null);
  const [designPickError, setDesignPickError] = useState(null);
  const [designDialog, setDesignDialog] = useState(null);
  const designCapturedRef = useRef(null);

  const [translationSel, setTranslationSel] = useState(null);
  const [isTranslationDragging, setIsTranslationDragging] = useState(false);
  const translationDragRef = useRef(null);
  const [hoveredCodon, setHoveredCodon] = useState(null);
  const cdsFeatureDataRef = useRef({});

  const [isEnzymeSelection, setIsEnzymeSelection] = useState(false);
  const [isEnzymeDragging, setIsEnzymeDragging] = useState(false);
  const [selectedEnzymeIds, setSelectedEnzymeIds] = useState([]);
  const enzymeDragRef = useRef(null);
  const lastEnzymeSelRef = useRef(null);

  const [selectionMode, setSelectionMode] = useState('none');
  const [selectedPrimerIds, setSelectedPrimerIds] = useState([]);
  const [isPrimerDragging, setIsPrimerDragging] = useState(false);
  const [primerDimActive, setPrimerDimActive] = useState(false);
  const primerDimTimerRef = useRef(null);
  const primerDragRef = useRef(null);
  const isPrimerDraggingRef = useRef(false);

  const resetCursorTimer = useCallback(() => {
    clearTimeout(cursorTimerRef.current);
    setCursorVisible(true);
    cursorTimerRef.current = setTimeout(() => setCursorVisible(false), 5000);
  }, []);

  const clearCursorTimer = useCallback(() => {
    clearTimeout(cursorTimerRef.current);
    cursorTimerRef.current = null;
    setCursorVisible(true);
  }, []);

  const startTranslationSelection = useCallback(
    (featureId, codon) => {
      setSelectionMode('translation');
      setTranslationSel({ featureId, startCodon: codon, endCodon: codon });
      translationDragRef.current = { featureId, startCodon: codon };
      setIsTranslationDragging(true);
      setSelStart(null);
      setSelEnd(null);
      setCursorIndex(null);
      clearCursorTimer();
      setIsEnzymeSelection(false);
      setSelectedEnzymeIds([]);
      lastEnzymeSelRef.current = null;
      setSelectedPrimerIds([]);
      isPrimerDraggingRef.current = false;
      primerDragRef.current = null;
      clearTimeout(primerDimTimerRef.current);
      primerDimTimerRef.current = null;
      setPrimerDimActive(false);
    },
    [clearCursorTimer],
  );

  const restoreVersionRef = useRef(0);
  const scrollToSeqIndexRef = useRef(null);
  useLayoutEffect(() => {
    if (!restoreState || restoreState.version === restoreVersionRef.current) return;
    restoreVersionRef.current = restoreState.version;
    setCursorIndex(restoreState.cursorIndex ?? null);
    setSelStart(restoreState.selStart ?? null);
    setSelEnd(restoreState.selEnd ?? null);
    setSelectionMode(restoreState.selectionMode ?? 'text');
    setSelectedPrimerIds(restoreState.selectedPrimerIds ?? []);
    setIsEnzymeSelection(restoreState.isEnzymeSelection ?? false);
    setSelectedEnzymeIds(restoreState.selectedEnzymeIds ?? []);
    setTranslationSel(restoreState.translationSel ?? null);
    translationDragRef.current = null;
    setIsTranslationDragging(false);
    if (restoreState.cursorIndex != null) resetCursorTimer();
    if (restoreState.scrollToIndex != null) {
      scrollToSeqIndexRef.current?.(restoreState.scrollToIndex);
    }
  }, [restoreState, resetCursorTimer]);

  const prevSelSnapshotRef = useRef(null);
  useEffect(() => {
    if (!onSelectionChange) return;
    const snap = JSON.stringify([
      cursorIndex,
      selStart,
      selEnd,
      selectionMode,
      selectedPrimerIds,
      isEnzymeSelection,
      selectedEnzymeIds,
      translationSel,
    ]);
    if (snap === prevSelSnapshotRef.current) return;
    prevSelSnapshotRef.current = snap;
    onSelectionChange({
      cursorIndex,
      selStart,
      selEnd,
      selectionMode,
      selectedPrimerIds,
      isEnzymeSelection,
      selectedEnzymeIds,
      translationSel,
    });
  }, [
    onSelectionChange,
    cursorIndex,
    selStart,
    selEnd,
    selectionMode,
    selectedPrimerIds,
    isEnzymeSelection,
    selectedEnzymeIds,
    translationSel,
  ]);

  const hasSelection =
    selStart !== null &&
    selEnd !== null &&
    (selStart <= selEnd || topology === 'circular') &&
    (selectionMode === 'text' || isEnzymeSelection);
  const selWraps = hasSelection && selStart > selEnd;
  const hasTranslationSelection = selectionMode === 'translation' && translationSel !== null;
  const currentSelColor = designPick ? '#0f766e' : isEnzymeSelection ? enzymeActiveBlue : '#3E2723';

  const translationBaseRange = useCallback(() => {
    if (!translationSel) return null;
    const cds = cdsFeatureDataRef.current[translationSel.featureId];
    if (!cds) return null;
    const start = Math.min(translationSel.startCodon, translationSel.endCodon);
    const end = Math.max(translationSel.startCodon, translationSel.endCodon);
    let minPos = Infinity;
    let maxPos = -Infinity;
    for (let i = start; i <= end; i++) {
      const t = cds.trans[i];
      if (t) {
        for (const b of t.bases) {
          if (b < minPos) minPos = b;
          if (b > maxPos) maxPos = b;
        }
      }
    }
    return minPos <= maxPos ? [minPos, maxPos] : null;
  }, [translationSel]);

  return {
    cursorIndex,
    setCursorIndex,
    selStart,
    setSelStart,
    selEnd,
    setSelEnd,
    isDragging,
    setIsDragging,
    selectionTm,
    setSelectionTm,
    tmLastRunRef,
    tmTimerRef,
    tmSeqRef,
    dragRef,
    isDraggingRef,
    autoScrollRef,
    hoveredIndex,
    setHoveredIndex,
    hoveredIndexRef,
    cursorTimerRef,
    cursorVisible,
    designPick,
    setDesignPick,
    designPickError,
    setDesignPickError,
    designDialog,
    setDesignDialog,
    designCapturedRef,
    translationSel,
    setTranslationSel,
    isTranslationDragging,
    setIsTranslationDragging,
    translationDragRef,
    hoveredCodon,
    setHoveredCodon,
    cdsFeatureDataRef,
    isEnzymeSelection,
    setIsEnzymeSelection,
    isEnzymeDragging,
    setIsEnzymeDragging,
    selectedEnzymeIds,
    setSelectedEnzymeIds,
    enzymeDragRef,
    lastEnzymeSelRef,
    selectionMode,
    setSelectionMode,
    selectedPrimerIds,
    setSelectedPrimerIds,
    isPrimerDragging,
    setIsPrimerDragging,
    primerDimActive,
    setPrimerDimActive,
    primerDimTimerRef,
    primerDragRef,
    isPrimerDraggingRef,
    resetCursorTimer,
    clearCursorTimer,
    startTranslationSelection,
    scrollToSeqIndexRef,
    hasSelection,
    selWraps,
    hasTranslationSelection,
    currentSelColor,
    translationBaseRange,
  };
}
