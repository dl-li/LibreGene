import React, { useState, useEffect, useRef, useCallback, useMemo, useId } from 'react';
import {
  cw,
  startX,
  bgColor,
  sliceRange,
  featureSelRange,
  rangeLocString1based,
  enzymeActiveBlue,
  amplimerGreen,
} from './editorConstants';
import { alignmentGapSegments, insertionBases } from './editor/alignmentLayout';
import { matchedSeqOf, reverseComplement, truncatedLabel } from './editor/seqUtils';
import { HIGH_CONTRAST_TEXT_COLOR_PALETTE, TEXT_COLOR_PALETTE } from './editor/textColors';
import { buildCDSData, isTranslatable } from './editor/translation';
import useStreamLayout from './editor/useStreamLayout';
import useAlignmentNavigation from './editor/useAlignmentNavigation';
import useEditorSelection from './editor/useEditorSelection';
import useEnrichedPrimers from './editor/useEnrichedPrimers';
import useTrackPacking from './editor/useTrackPacking';
import useEnzymeGeometry from './editor/useEnzymeGeometry';
import useEditorViewport, { useViewportMapping } from './editor/useEditorViewport';
import { renderFeatures, renderFeatureLabels } from './editor/layers/features.jsx';
import { renderPrimers } from './editor/layers/primers.jsx';
import {
  renderEnzymeLines,
  renderEnzymeLabels,
  renderEnzymeOverlay,
  renderTooltips,
} from './editor/layers/enzymes.jsx';
import SeqBgLayer from './editor/layers/SeqBgLayer';
import { RulerLayer, ComplementLayer, DS_BLOCK_H, DS_BOTTOM_Y } from './editor/layers/DsLayers';
import ChromatogramLayers, { CHROM_TRACK_H, CHROM_GAP } from './editor/layers/ChromatogramLayers';
import AlignmentLayers from './editor/layers/AlignmentLayers';
import {
  CursorLayer,
  DesignPickedLayer,
  SelectionLayer,
  SeqSelLayer,
  TranslationSelectionLayer,
  AmplimerRegionLayer,
  SelectionInfoLayer,
  HoverIndexLayer,
} from './editor/layers/selectionLayers';
import { trackPlugins, renderTrackLanes } from './editor/layers/trackLanes';
import { pluginAvailable } from './plugins';
import EditorNavMenu from './EditorNavMenu';
import EditorStatusBar from './editor/EditorStatusBar';
import AlignmentNavigator from './editor/AlignmentNavigator';
import EditorDialogs from './editor/EditorDialogs';
import { DESIGN_MODES } from './plugins/builtin/primerDesign';
import { computeTm, blastSubmit, getEnzymeDatabase } from './tauriApi';
import { getRelatedEnzymes } from './enzymeRelated';
import { loadProviderData, buildProviderIndex } from './enzymeProviders';
import { buildSearchResults } from './searchUtils';
import WarningBadge from './editor/WarningBadge';
import MapWatermark from './editor/MapWatermark';
import FoldWatermark from './editor/FoldWatermark';
import SelectionLengthBadge from './editor/SelectionLengthBadge';
import useEditorClipboard from './editor/useEditorClipboard';
import { showContextMenu } from './contextMenu';
import { collectAnnotations, writeAnnotatedClipboard } from './clipboardAnnotations';
import {
  Check,
  Copy,
  CopyPlus,
  CopyMinus,
  CopyX,
  Eraser,
  Globe,
  Image,
  Paintbrush,
  Pencil,
  Scissors,
  Rows3,
  Tag,
  Trash2,
} from 'lucide-react';

// Extra vertical gap between alignment lanes and the feature tracks below.
const ALIGN_FEAT_GAP = 10;

const ROW_BUF = 8; // rows above/below viewport to pre-render

// Stable empty list for the optional textColors props (keeps layer memoization).
const NO_TEXT_COLORS = [];

// Context-menu swatch for a text-color palette entry.
const textColorSwatch = (color) => {
  const Swatch = () => (
    <svg viewBox="0 0 16 16" fill="none">
      <rect x="2" y="2" width="12" height="12" rx="3" fill={color} stroke="rgba(0,0,0,0.25)" />
    </svg>
  );
  return Swatch;
};

const SequenceEditor = React.memo(function SequenceEditor({
  sequence,
  features = [],
  enzymes = [],
  allEnzymes = [],
  primers = [],
  initialCharsPerLine = 60,
  layoutParams = {},
  layoutKey,
  onEditRequest,
  restoreState,
  onSave,
  onSaveAs,
  canDirectSave = true,
  onFeatureFtypeChange,
  onFeatureColorChange,
  // Per-strand sequence text colors (canonical 0-based inclusive runs).
  textColorsTop = NO_TEXT_COLORS,
  textColorsBottom = NO_TEXT_COLORS,
  onSetTextColor,
  onFeatureLocationChange,
  onFeatureNameChange,
  onFeatureStrandChange,
  onPrimerChange,
  onFeatureAdd,
  onFeatureDelete,
  onPrimerDelete,
  onPrimerDeleteRequest,
  primerSeedLength,
  primerOverviewOpen = false,
  projectId,
  tmParams = {},
  onSelectionChange,
  scrollContainerRef,
  onUndo,
  onRedo,
  canUndo,
  canRedo,
  showFeatures,
  onToggleFeatures,
  pluginToggles = {},
  pluginSettings = {},
  disabledPlugins = [],
  alwaysExpandFeatures = false,
  onToggleAlwaysExpandFeatures,
  alwaysExpandPrimers = false,
  onToggleAlwaysExpandPrimers,
  featureLabelsBelow = false,
  onFeatureLabelsBelowChange,
  thickFeatureLines = false,
  onToggleThickFeatureLines,
  showOrfs,
  onToggleOrfs,
  showPrimers,
  onTogglePrimers,
  showEnzymes,
  onToggleEnzymes,
  enzymeFilter,
  onEnzymeFilterChange,
  enzymeProvider,
  onEnzymeProviderChange,
  openPrimerEditorRef,
  openFeatureEditorRef,
  alignmentCacheRef,
  alignmentTracks = [],
  alignments = [],
  // Chromatogram of the project's own .ab1 source (rendered under the top
  // strand) and per-alignment-id traces (rendered in extra lanes).
  chromatogram = null,
  alignmentChromatograms = {},
  // Alignment ids whose trace .ab1 path resolved (chromatogram toggleable),
  // the one currently expanded (at most one at a time), and its toggler.
  alignmentTraceAvailable,
  expandedChromAlnId = null,
  onToggleAlignmentChrom,
  onHideAlignment,
  alignmentEnabled = true,
  primerDesignEnabled = true,
  mapWatermark = false,
  foldWatermark = false,
  background = 'none',
  backgroundOptions = [],
  onBackgroundChange,
  mapName = '',
  showAlignments = true,
  onToggleAlignments,
  hiddenAlignIds = [],
  onToggleAlignmentVisible,
  onAddAlignmentFile,
  onAddAlignmentText,
  onManageAlignments,
  onOpenRnaFold,
  onOpenDotplot,
  onOpenGel,
  onOpenMapView,
  onOpenSnapshots,
  onOpenSessionEditLog,
  onEnzymeHoverChange,
  blastEnabled = false,
  topology = 'linear',
  onToggleTopology,
  onOpenMyPrimers,
  onOpenPrimerOverview,
  onOpenDetectFeatures,
  onOpenCodonOptimization,
  onOpenMyEnzymes,
  onOpenEnzymeDatabase,
  onAddPrimerToMyPrimers,
  onAddAllPrimersToMyPrimers,
  autoAddPrimers = false,
  onToggleAutoAddPrimers,
  myEnzymes = [],
  moleculeType = 'dna',
  // Per-molecule-type case display preference: { dna, rna, protein } each
  // 'asis' | 'upper' | 'lower'.
  caseDisplay,
  // Agent-tab lock: the nav menu is replaced by a teal-outlined control pill
  // (same look as the primer-design pick bar), which also blocks nav-level
  // misoperation while the MCP agent works.
  agentLocked = false,
  onUnlockAgent,
  // Hidden workspaces stay mounted (CSS-only); global input listeners must not
  // respond while this editor is not the visible one.
  hidden = false,
  viewMode = 'wrap',
  onViewModeChange,
  // SnapGene-style three-line rows: top strand, ruler, complement strand.
  showDoubleStrand = false,
  onToggleDoubleStrand,
  // View-menu toggle for per-strand text colors (rendering gate only).
  showTextColor = true,
  onToggleTextColor,
  highContrastTextColors = false,
  // ProjectWorkspace's ref onto the editor container div (the horizontal
  // scroll host in continuous mode); merged with the internal containerRef.
  editorScrollRef,
  // ProjectWorkspace renders the continuous-mode alignment navigator next to
  // its top scrollbar; jumpToAlignSite is exposed through this ref.
  alignNavRef,
  // Width of the collapsed sidebar icon rail overlaying the content's left
  // edge (main window only); continuous-mode label clamping stays clear of it.
  leftViewportInset = 0,
}) {
  const isDna = moleculeType === 'dna';
  const continuous = viewMode === 'continuous';
  // The double-strand block (ruler + complement strand) hangs below the top
  // strand's baseline; every below-sequence offset shifts down by this much.
  const dsOn = isDna && showDoubleStrand;
  const dsShift = dsOn ? DS_BLOCK_H : 0;
  // Length unit for the sequence: base pairs (DNA), nucleotides (ss-RNA),
  // amino acids (protein).
  const seqUnit = isDna ? 'bp' : moleculeType === 'protein' ? 'aa' : 'nt';
  const containerRef = useRef(null);
  const mergeContainerRef = useCallback(
    (el) => {
      containerRef.current = el;
      if (editorScrollRef) editorScrollRef.current = el;
    },
    [editorScrollRef],
  );
  // Prefix for per-row SVG ids generated by track plugins (e.g. the GC-track
  // <linearGradient>s); multiple editors stay mounted at once, so ids must be
  // unique per instance.
  const trackIdPrefix = `track-${useId().replace(/[^a-zA-Z0-9]/g, '')}`;
  const [baseCpl, setBaseCpl] = useState(initialCharsPerLine);
  const [hoveredFeature, setHoveredFeature] = useState(null);
  const featureLeaveRef = useRef(null);
  // Feature whose range produced the current text selection (via feature
  // click); enables two-stage Backspace: first deletes the feature, then the
  // sequence. Guarded by exact selStart/selEnd equality at delete time.
  const featureSelRef = useRef(null);
  const [featureInfoFeature, setFeatureInfoFeature] = useState(null); // for FeatureInfoDialog (edit mode)
  const [enzymeDetailRecord, setEnzymeDetailRecord] = useState(null); // for EnzymeDetailDialog
  const [createFeatureLoc, setCreateFeatureLoc] = useState(null); // for FeatureInfoDialog (create mode, null=closed, string=location)
  const [primerAlignmentPrimer, setPrimerAlignmentPrimer] = useState(null); // for PrimerAlignmentDialog (edit mode)
  const [createPrimerSeq, setCreatePrimerSeq] = useState(null); // for PrimerAlignmentDialog (create mode, null=closed, '' or string=sequence)

  // Expose openPrimerEditor to parent via ref (used by PrimerOverviewDialog)
  useEffect(() => {
    if (openPrimerEditorRef) {
      openPrimerEditorRef.current = (primer) => {
        setCreatePrimerSeq(null);
        setPrimerAlignmentPrimer(primer);
      };
    }
    return () => {
      if (openPrimerEditorRef) openPrimerEditorRef.current = null;
    };
  }, [openPrimerEditorRef]);

  // Expose openFeatureEditor to parent via ref (used by MapView)
  useEffect(() => {
    if (openFeatureEditorRef) {
      openFeatureEditorRef.current = (feature) => {
        setCreateFeatureLoc(null);
        setFeatureInfoFeature(feature);
      };
    }
    return () => {
      if (openFeatureEditorRef) openFeatureEditorRef.current = null;
    };
  }, [openFeatureEditorRef]);
  const [hoveredPrimer, setHoveredPrimer] = useState(null);

  const [hoveredEnzyme, setHoveredEnzyme] = useState(null);
  const {
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
  } = useEditorViewport({ scrollContainerRef, containerRef, setBaseCpl, layoutKey, viewMode });
  const svgRef = useRef(null);

  const {
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
  } = useEditorSelection({ restoreState, onSelectionChange, topology });

  const handlePrimerDesign = useCallback(
    (mode) => {
      if (!primerDesignEnabled) return;
      if (DESIGN_MODES[mode]?.circularOnly && topology !== 'circular') return;
      setDesignPick({ mode, segments: [] });
      setDesignPickError(null);
      designCapturedRef.current = null;
      setSelStart(null);
      setSelEnd(null);
      setCursorIndex(null);
      setSelectionMode('none');
      setSelectedPrimerIds([]);
      setIsEnzymeSelection(false);
      setSelectedEnzymeIds([]);
      setTranslationSel(null);
      setIsTranslationDragging(false);
      clearCursorTimer();
    },
    [primerDesignEnabled, topology, clearCursorTimer],
  );

  const cancelDesignPick = useCallback(() => {
    setDesignPick(null);
    setDesignPickError(null);
    designCapturedRef.current = null;
  }, []);

  // Capture a settled selection (drag end, shift+click, or feature click) as a
  // primer-design segment; opens the dialog once enough segments are picked
  useEffect(() => {
    if (!designPick || isDragging || selectionMode !== 'text') return;
    if (selStart === null || selEnd === null) return;
    if (selStart > selEnd) return; // origin-wrapping selections not supported here
    const key = `${selStart}:${selEnd}`;
    if (designCapturedRef.current === key) return;
    const meta = DESIGN_MODES[designPick.mode];
    const len = selEnd - selStart + 1;
    if (meta.minLen && len < meta.minLen) {
      setDesignPickError(`Selection must be at least ${meta.minLen} bp`);
      return;
    }
    if (meta.maxLen && len > meta.maxLen) {
      setDesignPickError(`Selection must be at most ${meta.maxLen} bp`);
      return;
    }
    designCapturedRef.current = key;
    setDesignPickError(null);
    const feat = features.find((f) => f.start === selStart && f.end === selEnd);
    const segments = [
      ...designPick.segments,
      { start: selStart, end: selEnd, ...(feat?.name ? { name: feat.name } : {}) },
    ];
    setSelStart(null);
    setSelEnd(null);
    setCursorIndex(null);
    setSelectionMode('none');
    if (segments.length >= meta.segments) {
      setDesignPick(null);
      setDesignDialog({ mode: designPick.mode, segments });
    } else {
      setDesignPick({ mode: designPick.mode, segments });
      designCapturedRef.current = null;
    }
  }, [designPick, isDragging, selStart, selEnd, selectionMode, features]);

  const pp = useMemo(() => {
    const base = {
      fwdMatchY: 30,
      revMatchY: 26,
      misYDelta: 4,
      fwdBaseTextY: 8,
      revBaseTextY: 18,
      fwdLabelY: 8,
      revLabelY: 20,
      trackGap: 36,
      fwdAboveBase: 30,
      fwdAboveExtra: 23,
      fwdAboveNonTailExtra: 5,
      revBelowBase: 26,
      revBelowExtra: 25,
      revBelowNonTailExtra: 5,
      hoverExpand: 26,
      arrowHeadLen: 7,
      arrowHeadHeight: 5,
      // Label lift when a primer is selected — and in always-expand mode, where
      // every primer shows its label at the selected position.
      fwdSelLabelLift: 16,
      revSelLabelLift: 13,
      ...layoutParams,
    };
    // Reverse primers draw below the sequence; shift their whole band below
    // the double-strand block when it is on.
    if (dsShift) {
      base.revMatchY += dsShift;
      base.revBelowBase += dsShift;
    }
    // Expand-all keeps every band open with its label at the raised
    // (selected) position; the collapsed-mode pitch lets a raised label
    // (worst case: 5' tail, reverse strand) ride into the next track's open
    // band, so widen it to fit band + raised label + clearance.
    if (alwaysExpandPrimers) base.trackGap = Math.max(base.trackGap, 48);
    return base;
  }, [layoutParams, dsShift, alwaysExpandPrimers]);

  const lp = useMemo(() => {
    const base = {
      minRowGap: 27,
      rowContentGap: 12,
      featTrackHeight: 18,
      featBaseOffset: 14,
      featLabelPad: 12,
      featLabelBelowExtra: 8,
      enzTrackHeight: 18,
      enzLineGap: 18,
      enzLabelBase: 51,
      enzAbovePad: 10,
      minAboveSpace: 24,
      minBelowSpace: 14,
      ...layoutParams,
    };
    // featBaseOffset anchors everything rendered below the sequence (features,
    // alignment lanes, chromatograms, track-plugin lanes); shifting it moves
    // them all clear of the double-strand block, and the packing reservations
    // built on it grow by the same amount.
    if (dsShift) base.featBaseOffset += dsShift;
    return base;
  }, [layoutParams, dsShift]);

  // Display-case transform (setting: Sequence → case display). Everything
  // downstream of cleanSeq — rendering, copy, primer-template text extraction —
  // follows it; stored data is untouched. Case-insensitive consumers (search,
  // matching, Tm) are unaffected either way.
  const caseMode = caseDisplay?.[moleculeType] || (moleculeType === 'protein' ? 'upper' : 'asis');
  const cleanSeq = useMemo(() => {
    const s = sequence || '';
    if (caseMode === 'upper') return s.toUpperCase();
    if (caseMode === 'lower') return s.toLowerCase();
    return s;
  }, [sequence, caseMode]);

  // Async Tm computation via backend NN model (DNA only)
  useEffect(() => {
    if (!isDna) {
      setSelectionTm(null);
      return;
    }
    if (!isDragging || selStart === null || selEnd === null) {
      setSelectionTm(null);
      return;
    }
    const seq = sliceRange(cleanSeq, selStart, selEnd);
    if (seq.length < 2) {
      setSelectionTm(null);
      return;
    }
    tmSeqRef.current = seq;
    const run = () => {
      tmTimerRef.current = null;
      tmLastRunRef.current = Date.now();
      computeTm(seq, tmParams).then((tm) => {
        // Drop stale results: a newer selection was scheduled meanwhile.
        if (tmSeqRef.current === seq) setSelectionTm(tm);
      });
    };
    // Throttle to ~100ms while dragging. The trailing timer lives in a ref and
    // is not cleared by effect cleanup, so it fires once the drag settles (even
    // on mouseup) and computes the final position; stale timers/results are
    // dropped via tmSeqRef.
    const elapsed = Date.now() - tmLastRunRef.current;
    if (elapsed >= 100) run();
    else {
      if (tmTimerRef.current) clearTimeout(tmTimerRef.current);
      tmTimerRef.current = setTimeout(run, 100 - elapsed);
    }
  }, [isDragging, selStart, selEnd, cleanSeq, tmParams, isDna]);

  // Clear any pending trailing Tm timer on unmount.
  useEffect(() => () => clearTimeout(tmTimerRef.current), []);

  const { enrichedPrimers, unmatchedPrimers, primerAlignmentCache } = useEnrichedPrimers({
    primers,
    cleanSeq,
    alignmentCacheRef,
    primerSeedLength,
    tmParams,
    projectId,
    prefetchAlignments: !hidden && primerOverviewOpen,
  });

  const handleAddCurrentPrimerToMyPrimers = useCallback(() => {
    if (selectionMode !== 'primer' || selectedPrimerIds.length !== 1) return;
    const p = enrichedPrimers.find((pr) => pr.id === selectedPrimerIds[0]);
    if (!p) return;
    onAddPrimerToMyPrimers?.({
      id: p.id,
      name: p.name,
      type: p.type,
      primerSeq: p.primerSeq,
      color: p.color,
    });
  }, [selectionMode, selectedPrimerIds, enrichedPrimers, onAddPrimerToMyPrimers]);

  const cdsWarnings = useMemo(() => {
    // CDS codon-length/translation warnings only make sense for DNA.
    if (!isDna) return [];
    const result = [];
    for (const f of features || []) {
      if (!isTranslatable(f) || f.orf) continue;
      const segs = f.segments && f.segments.length ? f.segments : [{ start: f.start, end: f.end }];
      const totalLen = segs.reduce((sum, seg) => sum + (seg.end - seg.start + 1), 0);

      if (totalLen % 3 !== 0) {
        result.push({ type: 'cds_len', id: f.id, name: f.name || f.id });
        continue;
      }

      if (f.translation) {
        const fWithSegs = { ...f, segments: segs };
        const data = buildCDSData(fWithSegs, cleanSeq);
        const computedStr = data.trans.map((t) => t.aa).join('');
        const stripAlpha = (s) => s.replace(/[^a-zA-Z]/g, '');
        if (stripAlpha(computedStr) !== stripAlpha(f.translation)) {
          result.push({ type: 'cds_trans', id: f.id, name: f.name || f.id });
        }
      }
    }
    return result;
  }, [features, cleanSeq, isDna]);

  const combinedWarnings = useMemo(() => {
    const out = [];
    for (const p of unmatchedPrimers) {
      out.push({ type: 'primer', id: p.id, name: p.name || p.id });
    }
    for (const c of cdsWarnings) {
      out.push(c);
    }
    return out;
  }, [unmatchedPrimers, cdsWarnings]);

  const {
    insReserve,
    rowStarts,
    rowCounts,
    streamOf,
    rowOf,
    colOfAbs,
    visCpl,
    absFromStream,
    charsPerLine,
    numRows,
    svgWidth,
    colVis,
    colFromVis,
    colRuns,
    sp,
  } = useStreamLayout({
    isDna,
    alignmentTracks,
    seqLen: cleanSeq.length,
    baseCpl,
    viewMode,
  });
  numRowsRef.current = numRows;

  // Track plugin lanes (e.g. the GC-content gradient band). Every registered
  // track hook runs unconditionally in registry order — React hooks rules
  // forbid filtering the list by disabledPlugins first, so a disabled or
  // inactive plugin gets enabled=false here and returns null from its hook.
  const trackLanes = trackPlugins.map((plugin) =>
    plugin.track.useLane({
      cleanSeq,
      topology,
      moleculeType,
      enabled:
        pluginAvailable(plugin, { moleculeType, disabledPlugins }) &&
        (plugin.featuresMenuItem ? !!pluginToggles?.[plugin.id]?.checked : true),
      settingValue: pluginSettings?.[plugin.id]?.value,
    }),
  );
  const trackH = trackLanes.reduce((sum, lane) => sum + (lane?.height || 0), 0);

  // Per-row compact lane assignment: an alignment only reserves a lane in
  // rows where it actually has sequence, so partial alignments leave no gaps.
  // Alignments with loaded chromatograms additionally reserve a trace band
  // lane below the text lanes; the project's own trace (ab1 source) reserves
  // one band above them.
  const alignLaneInfo = useMemo(() => {
    const perRow = Array.from({ length: numRows }, () => new Map());
    const chromPerRow = Array.from({ length: numRows }, () => new Map());
    alignmentTracks.forEach((al, ti) => {
      const hasChrom = !!alignmentChromatograms[al.id];
      const rows = new Set();
      for (const seg of [...(al.segments || []), ...alignmentGapSegments(al, cleanSeq.length)]) {
        for (const v of sp(seg.start, seg.end)) rows.add(v.row);
      }
      // Insertion anchors can sit outside every segment (read tails past the
      // last aligned base) — and a wide block spans rows — so every row
      // touched by the anchor's reserved cells still needs a lane. Deduped by
      // anchor, matching the display walk and the trace numbering.
      for (const [pos, bases] of insertionBases(al)) {
        const slotN = insReserve.get(pos) || 0;
        const cell0 = streamOf(pos) - slotN;
        for (let k = 0; k < Math.min(slotN, bases.length); k++) {
          rows.add(Math.floor((cell0 + k) / charsPerLine));
        }
        // The anchor column's own read base can sit on a row of its own.
        rows.add(rowOf(pos));
      }
      for (const r of rows) {
        if (r < 0 || r >= numRows) continue;
        if (!perRow[r].has(ti)) perRow[r].set(ti, perRow[r].size);
        if (hasChrom && !chromPerRow[r].has(ti)) chromPerRow[r].set(ti, chromPerRow[r].size);
      }
    });
    const counts = perRow.map((m) => m.size);
    const chromCounts = chromPerRow.map((m) => m.size);
    const mainChromH = chromatogram ? CHROM_TRACK_H + CHROM_GAP : 0;
    // Extra below-sequence height contributed by track-plugin lanes (GC
    // content) + chromatogram bands.
    const chromBelow = chromCounts.map(
      (n) => trackH + mainChromH + n * (CHROM_TRACK_H + CHROM_GAP),
    );
    return { perRow, counts, chromPerRow, chromCounts, trackH, mainChromH, chromBelow };
  }, [
    alignmentTracks,
    numRows,
    sp,
    alignmentChromatograms,
    chromatogram,
    cleanSeq.length,
    trackH,
    insReserve,
    rowOf,
    streamOf,
    charsPerLine,
  ]);

  const {
    normFeatures,
    abutColors,
    primerTracks,
    featureRowTracks,
    revPrimerFeatOffsets,
    rowAbove,
    rowBelow,
    enzymeRowTracks,
    hiddenEnzKeys,
    rowY,
    visibleRows,
    visibleFeatures,
    solidLineCols,
  } = useTrackPacking({
    features,
    enrichedPrimers,
    enzymes,
    numRows,
    rowStarts,
    rowCounts,
    rowOf,
    sp,
    colVis,
    pp,
    lp,
    featureLabelsBelow: featureLabelsBelow || continuous,
    alwaysExpandPrimers,
    alignLaneInfo,
    scrollY,
    viewportH,
    continuous,
    ALIGN_FEAT_GAP,
    ROW_BUF,
    visibleCols,
    absFromStream,
    dsShift,
  });
  const {
    getSeqY,
    scrollToSeqIndex,
    rowAtSvgY,
    clientToSeqIndex,
    clientToCharIndex,
    visibleEnzymes,
    visiblePrimers,
  } = useViewportMapping({
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
  });

  const { jumpToAlignSite } = useAlignmentNavigation({
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
  });

  const handleSvgMouseDown = useCallback(
    (e) => {
      if (e.button !== 0) return;
      // Reset enzyme selection when clicking on sequence directly
      setIsEnzymeSelection(false);
      setSelectedEnzymeIds([]);
      lastEnzymeSelRef.current = null;
      // Reset primer / amplimer selection
      setSelectionMode('text');
      setSelectedPrimerIds([]);
      setIsPrimerDragging(false);
      isPrimerDraggingRef.current = false;
      primerDragRef.current = null;
      if (primerDimTimerRef.current) {
        clearTimeout(primerDimTimerRef.current);
        primerDimTimerRef.current = null;
      }
      setPrimerDimActive(false);
      // Reset translation selection
      setTranslationSel(null);
      translationDragRef.current = null;
      setIsTranslationDragging(false);
      const idx = clientToSeqIndex(e.clientX, e.clientY);
      if (idx === null) {
        // Clicked blank space inside the canvas: drop any selection/cursor.
        setSelStart(null);
        setSelEnd(null);
        setCursorIndex(null);
        clearCursorTimer();
        return;
      }
      if (e.shiftKey && cursorIndex !== null) {
        // Shift+click: select from cursor to click position
        const s = Math.min(cursorIndex, idx);
        const e = Math.max(cursorIndex, idx) - 1;
        if (s <= e) {
          setSelStart(s);
          setSelEnd(e);
        }
        setCursorIndex(idx);
        dragRef.current = { startIdx: idx, active: false };
        clearCursorTimer();
        return;
      }
      dragRef.current = { startIdx: idx, active: false };
      setIsDragging(false);
      setCursorIndex(idx);
      setSelStart(null);
      setSelEnd(null);
      resetCursorTimer();
    },
    [clientToSeqIndex, resetCursorTimer, clearCursorTimer, cursorIndex],
  );

  const handleSvgMouseMove = useCallback(
    (e) => {
      if (isDraggingRef.current || translationDragRef.current) {
        if (hoveredIndexRef.current !== null) {
          hoveredIndexRef.current = null;
          setHoveredIndex(null);
        }
        return;
      }
      const idx = clientToCharIndex(e.clientX, e.clientY);
      if (idx !== hoveredIndexRef.current) {
        hoveredIndexRef.current = idx;
        setHoveredIndex(idx);
      }
      // Same trigger range as the base-number badge: hovering any base of a
      // codon (not just the feature block) shows the AA number. Every
      // translatable feature covering the position shows its own number.
      let nextCodon = null;
      if (idx !== null) {
        const map = {};
        for (const [featureId, cds] of Object.entries(cdsFeatureDataRef.current)) {
          const codon = cds.codonMap.get(idx);
          if (codon !== undefined) map[featureId] = codon;
        }
        if (Object.keys(map).length) nextCodon = { key: `${idx}`, map };
      }
      setHoveredCodon((prev) => (prev?.key === nextCodon?.key ? prev : nextCodon));
    },
    [clientToCharIndex],
  );

  const handleSvgMouseLeave = useCallback(() => {
    hoveredIndexRef.current = null;
    setHoveredIndex(null);
    setHoveredCodon(null);
  }, []);

  const updateSeqDragSelection = useCallback(
    (clientX, clientY) => {
      if (dragRef.current.startIdx === null) return;
      const idx = clientToSeqIndex(clientX, clientY);
      if (idx === null) return;
      if (idx === dragRef.current.startIdx) {
        // Dragged back onto the origin: collapse the selection entirely
        // instead of leaving the minimum 1-char selection behind.
        if (dragRef.current.active) {
          dragRef.current.active = false;
          setIsDragging(false);
          setSelStart(null);
          setSelEnd(null);
          setCursorIndex(dragRef.current.startIdx);
          resetCursorTimer();
        }
        return;
      }
      dragRef.current.active = true;
      setIsDragging(true);
      // Cursor is at insertion point idx; selected chars are from min to max-1
      setSelStart(Math.min(dragRef.current.startIdx, idx));
      setSelEnd(Math.max(dragRef.current.startIdx, idx) - 1);
      setCursorIndex(idx);
      resetCursorTimer();
    },
    [clientToSeqIndex, resetCursorTimer],
  );

  useEffect(() => {
    const onMove = (e) => updateSeqDragSelection(e.clientX, e.clientY);
    window.addEventListener('mousemove', onMove);
    return () => window.removeEventListener('mousemove', onMove);
  }, [updateSeqDragSelection]);

  useEffect(() => {
    const onUp = () => {
      // Handle translation (codon) drag end
      if (translationDragRef.current) {
        translationDragRef.current = null;
        setIsTranslationDragging(false);
        return;
      }
      // Handle enzyme drag end
      if (enzymeDragRef.current?.active) {
        const dragData = enzymeDragRef.current;
        // Select recognition site if: never dragged, or dragged back to same label
        if (!dragData.didDrag || dragData.backToStart) {
          setSelStart(dragData.recStart);
          setSelEnd(dragData.recEnd);
          setSelectedEnzymeIds([dragData.entryId]);
        }
        // If dragged to another enzyme, selectedEnzymeIds already set by onMouseEnter
        lastEnzymeSelRef.current = {
          enzymeId: dragData.startEnzymeId,
          cutIdx: dragData.startCutIdx,
          name: dragData.startName,
          entryId: dragData.entryId,
        };
        enzymeDragRef.current = null;
        isDraggingRef.current = false;
        setIsDragging(false);
        setIsEnzymeDragging(false);
        setHoveredEnzyme(null);
        clearCursorTimer();
        return;
      }
      if (dragRef.current.startIdx === null) return;
      if (dragRef.current.active) {
        setCursorIndex(null);
        clearCursorTimer();
      }
      setIsDragging(false);
      dragRef.current = { startIdx: null, active: false };
    };
    window.addEventListener('mouseup', onUp);
    return () => window.removeEventListener('mouseup', onUp);
  }, [clearCursorTimer]);

  // --- primer drag effect ---
  useEffect(() => {
    if (!isPrimerDragging) return;
    const onMove = () => {
      if (primerDragRef.current) {
        primerDragRef.current.didDrag = true;
      }
    };
    const onUp = () => {
      if (primerDimTimerRef.current) {
        clearTimeout(primerDimTimerRef.current);
        primerDimTimerRef.current = null;
      }
      setPrimerDimActive(false);
      const ref = primerDragRef.current;
      if (ref) {
        if (ref.hoveredPrimerId && ref.hoveredPrimerId !== ref.startPrimerId) {
          const targetPrimer = enrichedPrimers.find((p) => p.id === ref.hoveredPrimerId);
          if (targetPrimer && targetPrimer.isFwd !== ref.startFwd) {
            const p1 = enrichedPrimers.find((p) => p.id === ref.startPrimerId);
            const p2 = targetPrimer;
            const fwdPrimer = p1.isFwd ? p1 : p2;
            const revPrimer = !p1.isFwd ? p1 : p2;
            setSelectedPrimerIds([fwdPrimer.id, revPrimer.id]);
            setSelectionMode('amplimer');
          } else {
            setSelectionMode('none');
            setSelectedPrimerIds([]);
          }
        } else if (ref.didDrag) {
          setSelectionMode('none');
          setSelectedPrimerIds([]);
        } else {
          setSelectionMode('primer');
          setSelectedPrimerIds([ref.startPrimerId]);
        }
      }
      setIsPrimerDragging(false);
      isPrimerDraggingRef.current = false;
      primerDragRef.current = null;
    };
    window.addEventListener('mousemove', onMove, { passive: true });
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, [isPrimerDragging, enrichedPrimers]);

  const {
    copySelection,
    copyAmplimer,
    writeClipboard,
    pasteFromClipboard,
    toUppercase,
    toLowercase,
  } = useEditorClipboard({
    cleanSeq,
    features,
    primers,
    translationSel,
    cdsFeatureDataRef,
    hasSelection,
    selStart,
    selEnd,
    hasTranslationSelection,
    selectionMode,
    translationBaseRange,
    selWraps,
    cursorIndex,
    onEditRequest,
    hidden,
    selectedPrimerIds,
    enrichedPrimers,
    topology,
  });

  // Submit a sequence to NCBI BLAST (fixed preset per molecule type:
  // megablast/core_nt for DNA, blastp/nr for protein); the official results
  // page opens in the system browser once NCBI returns a RID.
  const [blastBusy, setBlastBusy] = useState(false);
  const submitBlast = useCallback(
    (seq) => {
      if (!seq || blastBusy) return;
      setBlastBusy(true);
      blastSubmit(seq, moleculeType)
        .catch(async (err) => {
          try {
            const { message } = await import('@tauri-apps/plugin-dialog');
            message(String(err), { title: 'BLAST Search', kind: 'error' });
          } catch {
            console.error('BLAST submit error:', err);
          }
        })
        .finally(() => setBlastBusy(false));
    },
    [blastBusy, moleculeType],
  );

  // Select a feature as a text selection spanning its full extent (same as left-click)
  const selectFeature = useCallback(
    (f) => {
      const [fStart, fEnd] = featureSelRange(f);
      setSelStart(fStart);
      setSelEnd(fEnd);
      setCursorIndex(fEnd + 1);
      clearCursorTimer();
      setSelectionMode('text');
      setSelectedPrimerIds([]);
      setTranslationSel(null);
      translationDragRef.current = null;
      setIsTranslationDragging(false);
      isPrimerDraggingRef.current = false;
      primerDragRef.current = null;
      if (primerDimTimerRef.current) {
        clearTimeout(primerDimTimerRef.current);
        primerDimTimerRef.current = null;
      }
      setPrimerDimActive(false);
    },
    [clearCursorTimer],
  );

  const openFeatureMenu = useCallback(
    (e, f) => {
      e.preventDefault();
      e.stopPropagation();
      selectFeature(f);
      const [fStart, fEnd] = featureSelRange(f);
      // Feature sequence in join order (origin-wrapping features concatenate
      // the end segment after the start segment).
      const sense = (f.segments?.length ? f.segments : [{ start: f.start, end: f.end }])
        .map((s) => cleanSeq.substring(s.start, s.end + 1))
        .join('');
      const isRev = f.strand === '-';
      const items = [
        { icon: Tag, label: 'Copy Feature Name', onSelect: () => writeClipboard(f.name) },
        {
          icon: CopyPlus,
          label: 'Copy (+) Strand',
          bold: !isRev,
          onSelect: () => {
            const meta = collectAnnotations({ features, primers }, fStart, fEnd, cleanSeq.length);
            writeAnnotatedClipboard(sense, meta);
          },
        },
        {
          icon: CopyMinus,
          label: 'Copy (−) Strand',
          bold: isRev,
          onSelect: () => writeClipboard(reverseComplement(sense)),
        },
      ];
      const cds = cdsFeatureDataRef.current[f.id];
      if (cds && cds.trans.length) {
        items.push({
          icon: CopyX,
          label: 'Copy Translation',
          onSelect: () => writeClipboard(cds.trans.map((t) => t.aa).join('')),
        });
      }
      if (blastEnabled) {
        items.push({ type: 'separator' });
        items.push({
          icon: Globe,
          label: blastBusy ? 'Submitting BLAST…' : 'BLAST Feature',
          disabled: blastBusy,
          onSelect: () => submitBlast(sense),
        });
      }
      if (!f.orf) {
        items.push({ type: 'separator' });
        items.push({
          icon: Pencil,
          label: 'Edit Feature…',
          onSelect: () => {
            setCreateFeatureLoc(null);
            setFeatureInfoFeature(f);
          },
        });
      }
      showContextMenu(e.clientX, e.clientY, items);
    },
    [
      cleanSeq,
      selectFeature,
      writeClipboard,
      features,
      primers,
      blastEnabled,
      blastBusy,
      submitBlast,
    ],
  );

  // Select a primer (same as left-click on its arrow, without starting a drag)
  const selectPrimer = useCallback(
    (p) => {
      setSelStart(null);
      setSelEnd(null);
      setCursorIndex(null);
      setIsEnzymeSelection(false);
      setSelectedEnzymeIds([]);
      lastEnzymeSelRef.current = null;
      setTranslationSel(null);
      translationDragRef.current = null;
      setIsTranslationDragging(false);
      setSelectionMode('primer');
      setSelectedPrimerIds([p.id]);
      clearCursorTimer();
    },
    [clearCursorTimer],
  );

  const primerMenuItems = useCallback(
    (p) => [
      { icon: Tag, label: 'Copy Primer Name', onSelect: () => writeClipboard(p.name) },
      {
        icon: Copy,
        label: 'Copy Primer Sequence',
        onSelect: () => writeClipboard(p.primerSeq || matchedSeqOf(p, cleanSeq)),
      },
    ],
    [cleanSeq, writeClipboard],
  );

  const openPrimerMenu = useCallback(
    (e, p) => {
      e.preventDefault();
      e.stopPropagation();
      selectPrimer(p);
      showContextMenu(e.clientX, e.clientY, [
        ...primerMenuItems(p),
        { type: 'separator' },
        {
          icon: Pencil,
          label: 'Edit Primer…',
          onSelect: () => {
            setCreatePrimerSeq(null);
            setPrimerAlignmentPrimer(enrichedPrimers.find((ep) => ep.id === p.id) || null);
          },
        },
        { type: 'separator' },
        {
          icon: Trash2,
          label: 'Delete Primer…',
          danger: true,
          onSelect: () => onPrimerDeleteRequest?.(p.id),
        },
      ]);
    },
    [selectPrimer, primerMenuItems, enrichedPrimers, onPrimerDeleteRequest],
  );

  const enzymeDbRef = useRef(null); // null = not loaded, [] = load failed/empty
  const loadEnzymeDb = useCallback(async () => {
    if (enzymeDbRef.current) return enzymeDbRef.current;
    try {
      const data = await getEnzymeDatabase();
      enzymeDbRef.current = Array.isArray(data) ? data : [];
    } catch {
      enzymeDbRef.current = [];
    }
    return enzymeDbRef.current;
  }, []);

  const selectEnzymeSite = useCallback(
    (enzyme, entryId) => {
      // Sites wrapping the origin of a circular sequence (recEnd beyond the
      // sequence length) fall back to the display window. For circular
      // molecules wrap the coords into [0, tlen) so selStart > selEnd
      // expresses the cross-origin selection (mirrors the tooltip path).
      const tlen = cleanSeq.length;
      const recWraps = enzyme.recStart == null || enzyme.recEnd >= tlen;
      let start = recWraps ? enzyme.displayStart : enzyme.recStart;
      let end = recWraps ? enzyme.displayEnd : enzyme.recEnd;
      if (recWraps && topology === 'circular' && tlen > 0) {
        start = ((start % tlen) + tlen) % tlen;
        end = ((end % tlen) + tlen) % tlen;
      }
      setSelStart(start);
      setSelEnd(end);
      setCursorIndex(null);
      setIsEnzymeSelection(true);
      setSelectedEnzymeIds([entryId ?? enzyme.id]);
      clearCursorTimer();
    },
    [cleanSeq, topology, clearCursorTimer],
  );

  const handleOpenGel = useCallback(
    (names) => {
      if (!onOpenGel) return;
      // Radix onSelect passes an event — only a real array counts as preset.
      // selectedEnzymeIds holds label entry ids ("<enzymeId>_p<pairIndex>"),
      // so match each id back to its enzyme entry before collecting names.
      const preset = Array.isArray(names)
        ? names
        : [
            ...new Set(
              selectedEnzymeIds
                .map((id) => enzymes.find((x) => id === x.id || id.startsWith(`${x.id}_p`))?.name)
                .filter(Boolean),
            ),
          ];
      onOpenGel(preset);
    },
    [onOpenGel, enzymes, selectedEnzymeIds],
  );

  const openEnzymeMenu = useCallback(
    async (e, l) => {
      e.preventDefault();
      e.stopPropagation();
      const enzyme = enzymes.find((x) => x.id === l.groupId);
      // Right-clicking a label that is already part of the enzyme selection
      // keeps the selection (e.g. both sites of a two-enzyme fragment pick).
      const keepSelection = selectedEnzymeIds.includes(l.id);
      if (enzyme && !keepSelection) selectEnzymeSite(enzyme, l.id);
      const items = [
        { icon: CopyPlus, label: 'Copy (+) Strand', onSelect: () => copySelection('sense') },
      ];
      if (isDna) {
        items.push({
          icon: CopyMinus,
          label: 'Copy (−) Strand',
          onSelect: () => copySelection('antisense'),
        });
      }
      const db = await loadEnzymeDb();
      const related = db.length
        ? getRelatedEnzymes(l.name, [...new Set(enzymes.map((x) => x.name))], db)
        : null;
      if (onOpenGel || related) items.push({ type: 'separator' });
      if (onOpenGel) {
        items.push({
          icon: Rows3,
          label: 'Digest and Run Gel',
          onSelect: () => handleOpenGel(keepSelection ? undefined : [enzyme?.name ?? l.name]),
        });
      }
      if (related) {
        const nameItems = (names) =>
          names.length
            ? names.map((n) => ({
                label: n,
                onSelect: () => {
                  const target = enzymes.find((x) => x.name === n);
                  if (target) selectEnzymeSite(target);
                },
              }))
            : [{ label: 'None', disabled: true }];
        items.push({
          icon: Scissors,
          label: 'Related Enzymes',
          children: [
            { label: 'Isocaudomers', disabled: true },
            ...nameItems(related.isocaudomers),
            { type: 'separator' },
            { label: 'Isoschizomers', disabled: true },
            ...nameItems(related.isoschizomers),
          ],
        });
      }
      showContextMenu(e.clientX, e.clientY, items);
    },
    [
      enzymes,
      isDna,
      copySelection,
      loadEnzymeDb,
      selectEnzymeSite,
      onOpenGel,
      handleOpenGel,
      selectedEnzymeIds,
    ],
  );

  const providerIndexRef = useRef(null);
  const [enzymeDetailCutSites, setEnzymeDetailCutSites] = useState(null);
  const openEnzymeDetail = useCallback(
    async (e, l) => {
      e.stopPropagation();
      const enzyme = enzymes.find((x) => x.id === l.groupId);
      if (enzyme) selectEnzymeSite(enzyme, l.id);
      const db = await loadEnzymeDb();
      const rec = db.find((r) => r.name.toLowerCase() === l.name.toLowerCase());
      if (!rec) return;
      if (!providerIndexRef.current) {
        providerIndexRef.current = buildProviderIndex(await loadProviderData());
      }
      const sites = enzymes
        .filter((x) => x.name.toLowerCase() === l.name.toLowerCase())
        .flatMap((x) => (x.cutPairs || [{ topCutIndex: x.cutIndex }]).map((p) => p.topCutIndex))
        .sort((a, b) => a - b);
      setEnzymeDetailCutSites(sites);
      setEnzymeDetailRecord(rec);
    },
    [enzymes, loadEnzymeDb, selectEnzymeSite],
  );

  // Submit the current text selection to NCBI BLAST.
  const handleBlastSelection = useCallback(() => {
    if (!hasSelection) return;
    submitBlast(sliceRange(cleanSeq, selStart, selEnd));
  }, [hasSelection, cleanSeq, selStart, selEnd, submitBlast]);

  const handleTextColorChange = useCallback(
    async (strand, color) => {
      const changed = await onSetTextColor(strand, selStart, selEnd, color);
      if (!changed) return;
      setSelStart(null);
      setSelEnd(null);
      setCursorIndex(null);
      setSelectionMode('none');
      setIsEnzymeSelection(false);
      setSelectedEnzymeIds([]);
      lastEnzymeSelRef.current = null;
      clearCursorTimer();
    },
    [onSetTextColor, selStart, selEnd, clearCursorTimer],
  );

  // Generic right-click on the editor canvas: menu reflects the current selection.
  // Feature / primer elements attach their own menus and stopPropagation.
  const handleContextMenu = useCallback(
    (e) => {
      e.preventDefault();
      e.stopPropagation();
      const items = [];
      if (hasTranslationSelection) {
        items.push(
          { icon: CopyX, label: 'Copy Translation', onSelect: () => copySelection('translation') },
          { icon: CopyPlus, label: 'Copy (+) Strand', onSelect: () => copySelection('sense') },
        );
        if (isDna) {
          items.push({
            icon: CopyMinus,
            label: 'Copy (−) Strand',
            onSelect: () => copySelection('antisense'),
          });
        }
      } else if (selectionMode === 'amplimer' && selectedPrimerIds.length === 2) {
        items.push({ icon: Copy, label: 'Copy Amplimer', onSelect: copyAmplimer });
      } else if (selectionMode === 'primer' && selectedPrimerIds.length === 1) {
        const p = enrichedPrimers.find((pr) => pr.id === selectedPrimerIds[0]);
        if (p) items.push(...primerMenuItems(p));
      } else if (hasSelection) {
        items.push({
          icon: CopyPlus,
          label: 'Copy (+) Strand',
          onSelect: () => copySelection('sense'),
        });
        if (isDna) {
          items.push({
            icon: CopyMinus,
            label: 'Copy (−) Strand',
            onSelect: () => copySelection('antisense'),
          });
        }
        if (isEnzymeSelection && onOpenGel && selectedEnzymeIds.length > 0) {
          items.push({
            icon: Rows3,
            label: 'Digest and Run Gel',
            onSelect: () => handleOpenGel(),
          });
        }
        if (blastEnabled) {
          items.push({ type: 'separator' });
          items.push({
            icon: Globe,
            label: blastBusy ? 'Submitting BLAST…' : 'BLAST Selection',
            disabled: blastBusy,
            onSelect: handleBlastSelection,
          });
        }
        // Sequence text colors (per strand; cross-origin selections unsupported).
        if (onSetTextColor && !selWraps) {
          const palette = highContrastTextColors
            ? HIGH_CONTRAST_TEXT_COLOR_PALETTE
            : TEXT_COLOR_PALETTE;
          const paletteItems = (strand) => [
            ...palette.map((c) => ({
              icon: textColorSwatch(c.color),
              label: c.label,
              onSelect: () => handleTextColorChange(strand, c.color),
            })),
            { type: 'separator' },
            {
              icon: Eraser,
              label: 'Clear Color',
              onSelect: () => handleTextColorChange(strand, ''),
            },
          ];
          items.push({ type: 'separator' });
          items.push({
            icon: Paintbrush,
            label: 'Text Color',
            children: dsOn
              ? [
                  { label: '(+) Strand', children: paletteItems('+') },
                  { label: '(−) Strand', children: paletteItems('-') },
                  { label: 'Both Strands', children: paletteItems('both') },
                ]
              : paletteItems(isDna ? 'both' : '+'),
          });
        }
      }
      if (backgroundOptions.length > 0 && onBackgroundChange) {
        if (items.length) items.push({ type: 'separator' });
        items.push({
          icon: Image,
          label: 'Background',
          children: backgroundOptions.map((opt) => ({
            icon: background === opt.value ? Check : undefined,
            label: opt.label,
            onSelect: () => onBackgroundChange(opt.value),
          })),
        });
      }
      showContextMenu(e.clientX, e.clientY, items);
    },
    [
      hasSelection,
      hasTranslationSelection,
      selectionMode,
      selectedPrimerIds,
      enrichedPrimers,
      isDna,
      dsOn,
      copySelection,
      copyAmplimer,
      primerMenuItems,
      blastEnabled,
      blastBusy,
      handleBlastSelection,
      onSetTextColor,
      handleTextColorChange,
      highContrastTextColors,
      selWraps,
      selStart,
      selEnd,
      background,
      backgroundOptions,
      onBackgroundChange,
      isEnzymeSelection,
      selectedEnzymeIds,
      onOpenGel,
      handleOpenGel,
    ],
  );

  useEffect(() => {
    if (hidden) return undefined;
    const onKey = (e) => {
      // Ignore events from input/textarea (e.g. dialog textarea has focus)
      // Also skip when inside a dialog — let the browser handle text selection copy naturally
      const tag = e.target?.tagName?.toLowerCase();
      if (tag === 'input' || tag === 'textarea' || e.target?.isContentEditable) return;
      if (e.target?.closest?.('[role="dialog"]')) return;

      // --- Escape: cancel primer design pick mode ---
      if (e.key === 'Escape' && designPick) {
        e.preventDefault();
        cancelDesignPick();
        return;
      }

      // --- Arrow keys: cursor navigation ---
      if (
        e.key === 'ArrowLeft' ||
        e.key === 'ArrowRight' ||
        e.key === 'ArrowUp' ||
        e.key === 'ArrowDown'
      ) {
        if (cursorIndex === null) return;
        e.preventDefault();
        // Continuous: Up/Down page by the visible column count instead of a row.
        const pageCols = continuous ? Math.max(1, Math.floor(viewportW / cw)) : charsPerLine;
        let ni = cursorIndex;
        if (e.key === 'ArrowLeft') ni = Math.max(0, cursorIndex - 1);
        else if (e.key === 'ArrowRight') ni = Math.min(cleanSeq.length, cursorIndex + 1);
        else if (e.key === 'ArrowUp') ni = absFromStream(streamOf(cursorIndex) - pageCols);
        else if (e.key === 'ArrowDown') ni = absFromStream(streamOf(cursorIndex) + pageCols);
        if (ni !== cursorIndex) {
          setCursorIndex(ni);
          setSelStart(null);
          setSelEnd(null);
          setTranslationSel(null);
          translationDragRef.current = null;
          setIsTranslationDragging(false);
          resetCursorTimer();
        }
        return;
      }

      // --- Edit triggers: letter keys (insert/replace), paste, and Delete/Backspace (delete) ---
      if (!(e.ctrlKey || e.metaKey || e.altKey)) {
        // Letter key → replace (any selection) or insert (cursor only)
        if (e.key.length === 1 && /^[a-zA-Z]$/.test(e.key) && onEditRequest) {
          // Translation (codon) selection: replace the covered base range
          if (hasTranslationSelection) {
            const range = translationBaseRange();
            if (range) {
              const [minPos, maxPos] = range;
              e.preventDefault();
              onEditRequest({
                type: 'replace',
                cursorIndex: minPos,
                selStart: minPos,
                selEnd: maxPos,
                selectedText: cleanSeq.substring(minPos, maxPos + 1),
                clipboardText: e.key,
              });
              return;
            }
          }
          // Replace mode: any active selection (text, enzyme, etc.)
          if (hasSelection && !selWraps) {
            e.preventDefault();
            onEditRequest({
              type: 'replace',
              cursorIndex: selStart,
              selStart,
              selEnd,
              selectedText: cleanSeq.substring(selStart, selEnd + 1),
              clipboardText: e.key,
            });
            return;
          }
          // Insert mode: cursor must be visible
          if (cursorIndex !== null) {
            e.preventDefault();
            onEditRequest({
              type: 'insert',
              cursorIndex,
              clipboardText: e.key,
            });
            return;
          }
        }

        // Delete/Backspace with translation selection → delete the covered bases
        if (
          (e.key === 'Delete' || e.key === 'Backspace') &&
          hasTranslationSelection &&
          onEditRequest
        ) {
          const range = translationBaseRange();
          if (range) {
            const [minPos, maxPos] = range;
            e.preventDefault();
            onEditRequest({
              type: 'delete',
              selStart: minPos,
              selEnd: maxPos,
              selectedText: cleanSeq.substring(minPos, maxPos + 1),
            });
          }
          return;
        }

        // Delete/Backspace on a selected primer → ask to delete the primer
        if (
          (e.key === 'Delete' || e.key === 'Backspace') &&
          selectionMode === 'primer' &&
          selectedPrimerIds.length === 1 &&
          onPrimerDeleteRequest
        ) {
          e.preventDefault();
          onPrimerDeleteRequest(selectedPrimerIds[0]);
          return;
        }

        // Delete/Backspace with selection → delete
        if (
          (e.key === 'Delete' || e.key === 'Backspace') &&
          hasSelection &&
          !selWraps &&
          onEditRequest
        ) {
          // Two-stage delete: if the selection came from a feature click, the
          // first press deletes the feature itself, keeping the selection.
          if (featureSelRef.current) {
            const fs = featureSelRef.current;
            const feat = features.find((x) => x.id === fs.id);
            if (
              feat &&
              !feat.orf &&
              onFeatureDelete &&
              selStart === fs.selStart &&
              selEnd === fs.selEnd
            ) {
              e.preventDefault();
              onFeatureDelete(feat.id);
              featureSelRef.current = null;
              return;
            }
            featureSelRef.current = null;
          }
          e.preventDefault();
          onEditRequest({
            type: 'delete',
            selStart,
            selEnd,
            selectedText: cleanSeq.substring(selStart, selEnd + 1),
          });
          return;
        }
      }

      // --- Ctrl/Cmd+C: Copy ---
      if ((e.ctrlKey || e.metaKey) && e.key === 'c') {
        if (selectionMode === 'amplimer' && selectedPrimerIds.length === 2) {
          e.preventDefault();
          const fp = enrichedPrimers.find((p) => p.id === selectedPrimerIds[0]);
          const rp = enrichedPrimers.find((p) => p.id === selectedPrimerIds[1]);
          const fwdPrimer = fp && fp.isFwd ? fp : rp;
          const revPrimer = fp && !fp.isFwd ? fp : rp;
          if (fwdPrimer && revPrimer) {
            // Linear template: no valid amplicon when the fwd primer is not upstream
            if (topology !== 'circular' && fwdPrimer.matchEnd >= revPrimer.matchStart) return;
            const fSeq = fwdPrimer.primerSeq || matchedSeqOf(fwdPrimer, cleanSeq);
            const rSeq = revPrimer.primerSeq || matchedSeqOf(revPrimer, cleanSeq);
            let intervening;
            if (fwdPrimer.matchEnd < revPrimer.matchStart) {
              intervening = cleanSeq.substring(fwdPrimer.matchEnd + 1, revPrimer.matchStart);
            } else {
              intervening =
                cleanSeq.substring(fwdPrimer.matchEnd + 1) +
                cleanSeq.substring(0, revPrimer.matchStart);
            }
            const amplimer = fSeq + intervening + reverseComplement(rSeq);
            navigator.clipboard.writeText(amplimer).catch(() => {});
          }
          return;
        }
        if (selectionMode === 'primer' && selectedPrimerIds.length === 1) {
          e.preventDefault();
          const p = enrichedPrimers.find((pr) => pr.id === selectedPrimerIds[0]);
          if (p) {
            const seq = p.primerSeq || matchedSeqOf(p, cleanSeq);
            navigator.clipboard.writeText(seq).catch(() => {});
          }
          return;
        }
        if (selectionMode === 'translation' && translationSel) {
          e.preventDefault();
          copySelection('translation');
          return;
        }
        if (hasSelection) {
          e.preventDefault();
          copySelection('sense');
        }
      }

      // --- Cmd/Ctrl+R: Create new primer from selection (or empty) ---
      if (isDna && (e.ctrlKey || e.metaKey) && e.key === 'r') {
        e.preventDefault();
        setPrimerAlignmentPrimer(null); // clear edit mode
        if (hasSelection && selectionMode === 'text') {
          setCreatePrimerSeq(sliceRange(cleanSeq, selStart, selEnd));
        } else {
          setCreatePrimerSeq('');
        }
        return;
      }

      // --- Cmd/Ctrl+T: Create new feature from selection (or empty) ---
      if ((e.ctrlKey || e.metaKey) && e.key === 't') {
        e.preventDefault();
        setFeatureInfoFeature(null); // clear edit mode
        if (hasSelection && selectionMode === 'text') {
          // 1-based inclusive location string (user-visible convention)
          setCreateFeatureLoc(rangeLocString1based(selStart, selEnd, cleanSeq.length));
        } else {
          setCreateFeatureLoc('');
        }
        return;
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [
    cursorIndex,
    selStart,
    selEnd,
    hasSelection,
    selWraps,
    cleanSeq,
    charsPerLine,
    continuous,
    viewportW,
    streamOf,
    absFromStream,
    resetCursorTimer,
    selectionMode,
    selectedPrimerIds,
    enrichedPrimers,
    onEditRequest,
    onFeatureDelete,
    onPrimerDeleteRequest,
    features,
    copySelection,
    translationSel,
    hasTranslationSelection,
    translationBaseRange,
    designPick,
    cancelDesignPick,
    isDna,
    topology,
    hidden,
  ]);

  useEffect(() => {
    isDraggingRef.current = isDragging;
  }, [isDragging]);
  useEffect(
    () => () => {
      clearCursorTimer();
      clearTimeout(featureLeaveRef.current);
      clearTimeout(primerDimTimerRef.current);
    },
    [clearCursorTimer],
  );

  // Sync featureInfoFeature when features update (e.g., after ftype change)
  useEffect(() => {
    if (!featureInfoFeature) return;
    const updated = features.find((f) => f.id === featureInfoFeature.id);
    if (updated && updated !== featureInfoFeature) {
      setFeatureInfoFeature(updated);
    } else if (!updated) {
      setFeatureInfoFeature(null);
    }
  }, [features, featureInfoFeature]);

  const createFeature = useCallback(() => {
    setFeatureInfoFeature(null);
    if (hasSelection && selectionMode === 'text') {
      setCreateFeatureLoc(rangeLocString1based(selStart, selEnd, cleanSeq.length));
    } else {
      setCreateFeatureLoc('');
    }
  }, [hasSelection, selectionMode, selStart, selEnd, cleanSeq]);

  const createPrimer = useCallback(() => {
    setPrimerAlignmentPrimer(null);
    if (hasSelection && selectionMode === 'text') {
      setCreatePrimerSeq(sliceRange(cleanSeq, selStart, selEnd));
    } else {
      setCreatePrimerSeq('');
    }
  }, [hasSelection, selectionMode, cleanSeq, selStart, selEnd]);

  // --- Search: sequence (both strands, IUPAC) + feature/enzyme/primer names ---
  const [searchNav, setSearchNav] = useState({ query: '', index: -1, total: 0 });
  const searchStateRef = useRef({ query: '', scope: 'all', results: [], index: -1 });
  const openSearchRef = useRef(null);

  // Cached results key only on query+scope, so reset them when the searched
  // data (sequence / features) changes.
  useEffect(() => {
    const st = searchStateRef.current;
    st.query = '';
    st.results = [];
    st.index = -1;
  }, [cleanSeq, normFeatures]);

  useEffect(() => {
    if (hidden) return undefined;
    const onKeyDown = (e) => {
      if ((e.metaKey || e.ctrlKey) && (e.key === 'f' || e.key === 'F')) {
        e.preventDefault();
        openSearchRef.current?.();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [hidden]);

  const applySearchHit = useCallback(
    (hit) => {
      if (!hit) return;
      lastEnzymeSelRef.current = null;
      setTranslationSel(null);
      translationDragRef.current = null;
      setIsTranslationDragging(false);
      if (hit.type === 'primer') {
        setSelStart(null);
        setSelEnd(null);
        setCursorIndex(null);
        setIsEnzymeSelection(false);
        setSelectedEnzymeIds([]);
        setHoveredEnzyme(null);
        setSelectionMode('primer');
        setSelectedPrimerIds([hit.ref.id]);
        clearCursorTimer();
      } else if (hit.type === 'enzyme') {
        const displayed = enzymes.some((x) => x.id === hit.ref.id);
        setSelectedPrimerIds([]);
        setSelStart(hit.start);
        setSelEnd(hit.end);
        setCursorIndex(null);
        setSelectionMode('text');
        if (displayed) {
          const pairs = hit.ref.cutPairs || [];
          setIsEnzymeSelection(true);
          setSelectedEnzymeIds([pairs.length > 1 ? `${hit.ref.id}_p0` : hit.ref.id]);
          // Show the hover tooltip at the hit site: pick the cut pair nearest
          // to the hit (enzymeLayout entry ids are always `${id}_p${pairIndex}`)
          let best = 0;
          let bestD = Infinity;
          const plist = pairs.length
            ? pairs
            : [{ topCutIndex: hit.ref.cutIndex, botCutIndex: hit.ref.botCutIndex }];
          plist.forEach((cp, i) => {
            const d = Math.min(
              Math.abs(cp.topCutIndex - hit.start),
              Math.abs(cp.botCutIndex - hit.start),
            );
            if (d < bestD) {
              bestD = d;
              best = i;
            }
          });
          setHoveredEnzyme(`${hit.ref.id}_p${best}`);
        } else {
          setIsEnzymeSelection(false);
          setSelectedEnzymeIds([]);
          setHoveredEnzyme(null);
        }
        clearCursorTimer();
      } else {
        // 'seq' | 'feature'
        setSelectionMode('text');
        setIsEnzymeSelection(false);
        setSelectedEnzymeIds([]);
        setHoveredEnzyme(null);
        setSelectedPrimerIds([]);
        setSelStart(hit.start);
        setSelEnd(hit.end);
        setCursorIndex(hit.end + 1);
        resetCursorTimer();
      }
      scrollToSeqIndex(hit.start);
    },
    [enzymes, clearCursorTimer, resetCursorTimer, scrollToSeqIndex],
  );

  const handleSearch = useCallback(
    (query, direction = 'next', scope = 'all') => {
      const st = searchStateRef.current;
      if (st.query !== query || st.scope !== scope) {
        st.query = query;
        st.scope = scope;
        st.results = query
          ? buildSearchResults(
              query,
              {
                seq: cleanSeq,
                features: normFeatures,
                allEnzymes,
                primers,
              },
              scope,
            )
          : [];
        st.index = -1;
      }
      const { results } = st;
      if (direction === 'reset' || !results.length) {
        st.index = -1;
        setSearchNav({ query, index: -1, total: results.length });
        return;
      }
      if (st.index === -1) {
        const anchor = selStart !== null ? selStart : cursorIndex !== null ? cursorIndex : -1;
        if (direction === 'prev') {
          st.index = results.length - 1;
          for (let i = results.length - 1; i >= 0; i--) {
            if (results[i].start < anchor) {
              st.index = i;
              break;
            }
          }
        } else {
          st.index = 0;
          for (let i = 0; i < results.length; i++) {
            if (results[i].start > anchor) {
              st.index = i;
              break;
            }
          }
        }
      } else {
        st.index = (st.index + (direction === 'prev' ? -1 : 1) + results.length) % results.length;
      }
      setSearchNav({ query, index: st.index, total: results.length });
      applySearchHit(results[st.index]);
    },
    [cleanSeq, normFeatures, allEnzymes, primers, selStart, cursorIndex, applySearchHit],
  );

  const svgHeight = rowY[rowY.length - 1] + Math.max(40, rowBelow[numRows - 1] + 24);
  avgRowPitchRef.current = numRows > 1 ? (rowY[numRows - 1] - rowY[0]) / (numRows - 1) : 60;

  // --- enzyme track assignment is now in the spacing memo (enzymeRowTracks) ---

  // Pre-compute translation data (amino acid + codon index map) for translatable features
  const cdsFeatureData = useMemo(() => {
    // Codon-based translation display only applies to DNA; rna/protein are
    // single-strand sequences without a genetic-code readout.
    if (!isDna) return {};
    const map = {};
    for (const f of normFeatures) {
      if (!isTranslatable(f)) continue;
      const data = buildCDSData(f, sequence);
      if (data.trans.length > 0) map[f.id] = data;
    }
    return map;
  }, [normFeatures, sequence, isDna]);

  // Sync to a ref so early callbacks (e.g. copySelection) can read current CDS data
  // without creating a TDZ by referencing this later-defined constant.
  useEffect(() => {
    cdsFeatureDataRef.current = cdsFeatureData;
  }, [cdsFeatureData]);

  // --- translation (codon) drag effect ---
  // Placed after cdsFeatureData so the dependency array can reference it.
  const updateTranslationDrag = useCallback(
    (clientX, clientY) => {
      const drag = translationDragRef.current;
      if (!drag || !svgRef.current) return;
      const cds = cdsFeatureData[drag.featureId];
      if (!cds) return;

      // Compute row/col directly from SVG geometry so dragging works over the
      // feature bar and not only over the sequence text.
      const pt = svgRef.current.createSVGPoint();
      pt.x = clientX;
      pt.y = clientY;
      const ctm = svgRef.current.getScreenCTM();
      if (!ctm) return;
      const svgPt = pt.matrixTransform(ctm.inverse());
      const xRel = svgPt.x - startX;
      if (xRel < -cw / 2 || xRel > charsPerLine * cw + cw / 2) return;
      let vis = Math.floor(xRel / cw);
      if (xRel < 0) vis = 0;
      if (vis > charsPerLine) vis = charsPerLine - 1;

      const row = rowAtSvgY(svgPt.y);
      if (row < 0) return;

      const col = Math.max(0, Math.min(rowCounts[row] - 1, colFromVis(vis, row)));
      const idx = rowStarts[row] + col;
      const codon = cds.codonMap.get(idx);
      if (codon === undefined) return;
      const maxCodon = cds.trans.length - 1;
      const clamped = Math.max(0, Math.min(maxCodon, codon));
      setHoveredCodon({
        key: `drag:${drag.featureId}:${clamped}`,
        map: { [drag.featureId]: clamped },
      });
      setTranslationSel((prev) => {
        if (!prev || prev.featureId !== drag.featureId) return prev;
        return { featureId: drag.featureId, startCodon: drag.startCodon, endCodon: clamped };
      });
    },
    [cdsFeatureData, charsPerLine, rowAtSvgY, colFromVis, rowStarts, rowCounts],
  );

  useEffect(() => {
    const onMove = (e) => updateTranslationDrag(e.clientX, e.clientY);
    window.addEventListener('mousemove', onMove);
    return () => window.removeEventListener('mousemove', onMove);
  }, [updateTranslationDrag]);

  // --- auto-scroll while dragging near the top/bottom viewport edge ---
  // mousemove stops firing once the pointer leaves the window or the content
  // scrolls under a stationary pointer, so a rAF loop keeps scrolling and
  // re-evaluates the drag selection from the last stored pointer position.
  useEffect(() => {
    const EDGE = 48;
    const MIN_SPEED = 2;
    const MAX_SPEED = 20;
    let rafId = null;
    const anyDragActive = () =>
      dragRef.current.startIdx !== null ||
      isPrimerDraggingRef.current ||
      enzymeDragRef.current?.active ||
      translationDragRef.current !== null;
    const tick = () => {
      rafId = null;
      if (!anyDragActive()) return;
      const { clientX, clientY } = autoScrollRef.current;
      const scroller = scrollContainerRef?.current;
      const rect = scroller
        ? scroller.getBoundingClientRect()
        : { top: 0, bottom: window.innerHeight };
      let delta = 0;
      if (clientY < rect.top + EDGE) {
        const t = Math.min(1, (rect.top + EDGE - clientY) / EDGE);
        delta = -(MIN_SPEED + (MAX_SPEED - MIN_SPEED) * t);
      } else if (clientY > rect.bottom - EDGE) {
        const t = Math.min(1, (clientY - (rect.bottom - EDGE)) / EDGE);
        delta = MIN_SPEED + (MAX_SPEED - MIN_SPEED) * t;
      }
      let deltaX = 0;
      if (continuous) {
        const el = containerRef?.current;
        if (el) {
          const er = el.getBoundingClientRect();
          if (clientX < er.left + EDGE) {
            const t = Math.min(1, (er.left + EDGE - clientX) / EDGE);
            deltaX = -(MIN_SPEED + (MAX_SPEED - MIN_SPEED) * t);
          } else if (clientX > er.right - EDGE) {
            const t = Math.min(1, (clientX - (er.right - EDGE)) / EDGE);
            deltaX = MIN_SPEED + (MAX_SPEED - MIN_SPEED) * t;
          }
          if (deltaX !== 0) el.scrollLeft += deltaX;
        }
      }
      if (delta !== 0 || deltaX !== 0) {
        if (delta !== 0) {
          if (scroller) scroller.scrollTop += delta;
          else window.scrollBy(0, delta);
        }
        // Scrolling moves content under a stationary pointer without firing
        // mousemove, so re-run the drag selection from the stored position.
        // Primer/enzyme pair drags rely on mouseenter as elements move under
        // the pointer and need no explicit recompute here.
        if (dragRef.current.startIdx !== null) updateSeqDragSelection(clientX, clientY);
        else if (translationDragRef.current) updateTranslationDrag(clientX, clientY);
      }
      rafId = requestAnimationFrame(tick);
    };
    const onMove = (e) => {
      autoScrollRef.current = { clientX: e.clientX, clientY: e.clientY };
      if (rafId === null && anyDragActive()) rafId = requestAnimationFrame(tick);
    };
    window.addEventListener('mousemove', onMove, { passive: true });
    return () => {
      window.removeEventListener('mousemove', onMove);
      if (rafId !== null) window.cancelAnimationFrame(rafId);
    };
  }, [scrollContainerRef, containerRef, continuous, updateSeqDragSelection, updateTranslationDrag]);

  const renderedFeatures = useMemo(
    () =>
      renderFeatures({
        visibleFeatures,
        hoveredFeature,
        alwaysExpandFeatures,
        thickFeatureLines,
        sp,
        abutColors,
        rowStarts,
        getSeqY,
        alignLaneInfo,
        featureRowTracks,
        lp,
        ALIGN_FEAT_GAP,
        colRuns,
        colFromVis,
        colVis,
        solidLineCols,
        isDraggingRef,
        featureLeaveRef,
        setHoveredFeature,
        translationDragRef,
        cdsFeatureData,
        svgRef,
        setHoveredCodon,
        openFeatureMenu,
        featureSelRef,
        setSelStart,
        setSelEnd,
        setCursorIndex,
        clearCursorTimer,
        setSelectionMode,
        setSelectedPrimerIds,
        setTranslationSel,
        setIsTranslationDragging,
        isPrimerDraggingRef,
        primerDragRef,
        primerDimTimerRef,
        setPrimerDimActive,
        setCreateFeatureLoc,
        setFeatureInfoFeature,
        rowOf,
        hoveredCodon,
        startTranslationSelection,
      }),
    [
      visibleFeatures,
      hoveredFeature,
      featureRowTracks,
      getSeqY,
      sp,
      rowOf,
      rowStarts,
      clearCursorTimer,
      lp,
      cdsFeatureData,
      solidLineCols,
      colRuns,
      colVis,
      colFromVis,
      setSelectionMode,
      setTranslationSel,
      startTranslationSelection,
      alignLaneInfo,
      alwaysExpandFeatures,
      thickFeatureLines,
      hoveredCodon,
      openFeatureMenu,
      abutColors,
    ],
  );

  // Feature-label clamp window in SVG x coordinates (continuous mode): the
  // viewport's left/right edges, accounting for the container's left padding,
  // the sidebar icon rail, and a margin from the screen edges. `rightFeat` is
  // the clamp line for reverse-strand feature labels: pulled in by the same
  // reserve the alignment-track labels use (whether or not any are present),
  // so right-pinned feature labels end exactly at the alignment labels' right
  // edge.
  const labelViewport = useMemo(() => {
    if (!continuous) return null;
    const el = containerRef.current;
    const padL = el ? parseFloat(window.getComputedStyle(el).paddingLeft) || 0 : 16;
    const right = scrollX - padL + viewportW - 32;
    return {
      left: scrollX - padL + leftViewportInset + 48,
      right,
      rightFeat: right - (onHideAlignment ? 26 : 8),
    };
  }, [continuous, scrollX, viewportW, leftViewportInset, onHideAlignment]);

  const renderedFeatureLabels = useMemo(
    () =>
      renderFeatureLabels({
        visibleFeatures,
        hoveredFeature,
        featureLabelsBelow: featureLabelsBelow || continuous,
        labelViewport,
        sp,
        abutColors,
        getSeqY,
        alignLaneInfo,
        featureRowTracks,
        lp,
        ALIGN_FEAT_GAP,
        rowCounts,
        colVis,
        isDraggingRef,
        featureLeaveRef,
        setHoveredFeature,
        translationDragRef,
        openFeatureMenu,
        featureSelRef,
        setSelStart,
        setSelEnd,
        setCursorIndex,
        clearCursorTimer,
        setSelectionMode,
        setSelectedPrimerIds,
        setTranslationSel,
        setIsTranslationDragging,
        isPrimerDraggingRef,
        primerDragRef,
        primerDimTimerRef,
        setPrimerDimActive,
        setCreateFeatureLoc,
        setFeatureInfoFeature,
      }),
    [
      visibleFeatures,
      hoveredFeature,
      featureRowTracks,
      getSeqY,
      sp,
      clearCursorTimer,
      truncatedLabel,
      lp,
      cdsFeatureData,
      charsPerLine,
      startTranslationSelection,
      alignLaneInfo,
      openFeatureMenu,
      featureLabelsBelow,
      continuous,
      labelViewport,
      abutColors,
      rowCounts,
      colVis,
    ],
  );

  // Track-plugin lanes (e.g. the GC-content gradient band): each active
  // plugin renders all visible rows below the sequence.
  const renderedTracks = useMemo(
    () =>
      renderTrackLanes(
        {
          visibleRows,
          rowBuf: ROW_BUF,
          numRows,
          rowStarts,
          rowCounts,
          baseCpl,
          charsPerLine,
          seqLength: cleanSeq.length,
          getSeqY,
          lp,
          idPrefix: trackIdPrefix,
          colVis,
          colRuns,
          visibleCols,
        },
        trackLanes,
      ),
    // trackPlugins is a module constant, so spreading trackLanes keeps the
    // deps length fixed while keying on each lane's memoized identity.
    [
      visibleRows,
      numRows,
      rowStarts,
      rowCounts,
      baseCpl,
      charsPerLine,
      cleanSeq.length,
      getSeqY,
      lp,
      trackIdPrefix,
      colVis,
      colRuns,
      visibleCols,
      ...trackLanes,
    ],
  );

  const renderedPrimers = useMemo(
    () =>
      renderPrimers({
        visiblePrimers,
        hoveredPrimer,
        alwaysExpandPrimers,
        sp,
        rowStarts,
        rowCounts,
        openPrimerMenu,
        getSeqY,
        revPrimerFeatOffsets,
        alignLaneInfo,
        ALIGN_FEAT_GAP,
        lp,
        primerTracks,
        pp,
        colVis,
        selectedPrimerIds,
        selectionMode,
        isPrimerDragging,
        primerDimActive,
        primerDragRef,
        setSelStart,
        setSelEnd,
        setCursorIndex,
        setIsEnzymeSelection,
        setSelectedEnzymeIds,
        lastEnzymeSelRef,
        setTranslationSel,
        translationDragRef,
        setIsTranslationDragging,
        setSelectionMode,
        setSelectedPrimerIds,
        setIsPrimerDragging,
        isPrimerDraggingRef,
        setHoveredPrimer,
        primerDimTimerRef,
        clearCursorTimer,
        setPrimerDimActive,
        setCreatePrimerSeq,
        setPrimerAlignmentPrimer,
        enrichedPrimers,
        isDraggingRef,
      }),
    [
      visiblePrimers,
      hoveredPrimer,
      alwaysExpandPrimers,
      charsPerLine,
      pp,
      primerTracks,
      revPrimerFeatOffsets,
      getSeqY,
      sp,
      selectionMode,
      selectedPrimerIds,
      isPrimerDragging,
      primerDimActive,
      openPrimerMenu,
      alignLaneInfo,
      rowStarts,
      rowCounts,
      colVis,
      colRuns,
    ],
  );

  const { enzymeLayout, enzymeLinesPath, totalNameCounts } = useEnzymeGeometry({
    visibleEnzymes,
    enzymeRowTracks,
    hiddenEnzKeys,
    lp,
    charsPerLine,
    getSeqY,
    colVis,
    rowOf,
    rowStarts,
    enzymes,
  });

  useEffect(() => {
    if (!onEnzymeHoverChange) return;
    const entry = hoveredEnzyme ? enzymeLayout.find((l) => l.id === hoveredEnzyme) : null;
    if (!entry) {
      onEnzymeHoverChange(null);
      return;
    }
    const positions = new Set();
    for (const e of enzymes) {
      if (e.name !== entry.name) continue;
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      for (const cp of pairs) {
        positions.add(cp.topCutIndex);
        positions.add(cp.botCutIndex);
      }
    }
    onEnzymeHoverChange([...positions]);
  }, [hoveredEnzyme, enzymeLayout, enzymes, onEnzymeHoverChange]);

  const renderedEnzymes = useMemo(
    () => renderEnzymeLines({ enzymeLayout, enzymeLinesPath, lp }),
    [enzymeLayout, enzymeLinesPath, lp.enzLineGap],
  );

  const renderedEnzymeLabels = useMemo(
    () =>
      renderEnzymeLabels({
        hoveredEnzyme,
        enzymeLayout,
        enzymes,
        selectedEnzymeIds,
        totalNameCounts,
        openEnzymeMenu,
        openEnzymeDetail,
        enzymeDragRef,
        isDraggingRef,
        setHoveredEnzyme,
        setSelStart,
        setSelEnd,
        setCursorIndex,
        setSelectedEnzymeIds,
        isPrimerDraggingRef,
        primerDragRef,
        primerDimTimerRef,
        setPrimerDimActive,
        setTranslationSel,
        translationDragRef,
        setIsTranslationDragging,
        lastEnzymeSelRef,
        clearCursorTimer,
        setIsEnzymeSelection,
        setSelectedPrimerIds,
        cleanSeq,
        topology,
        setIsDragging,
        setIsEnzymeDragging,
      }),
    [
      enzymeLayout,
      enzymes,
      hoveredEnzyme,
      bgColor,
      selectedEnzymeIds,
      isEnzymeSelection,
      clearCursorTimer,
      cleanSeq,
      topology,
      openEnzymeMenu,
      openEnzymeDetail,
    ],
  );

  const renderedEnzymeOverlay = useMemo(
    () =>
      renderEnzymeOverlay({
        hoveredEnzyme,
        enzymeLayout,
        selectedEnzymeIds,
        enzymes,
        totalNameCounts,
        isEnzymeSelection,
      }),
    [
      hoveredEnzyme,
      selectedEnzymeIds,
      isEnzymeSelection,
      enzymeLayout,
      enzymes,
      bgColor,
      totalNameCounts,
    ],
  );

  const renderedTooltips = useMemo(
    () =>
      renderTooltips({
        hoveredEnzyme,
        isEnzymeDragging,
        enzymeLayout,
        enzymes,
        cleanSeq,
        dsBottomY: dsOn ? DS_BOTTOM_Y : 0,
      }),
    [hoveredEnzyme, enzymeLayout, enzymes, cleanSeq, charsPerLine, isEnzymeDragging, dsOn],
  );

  return (
    <>
      {/* Unmatched primers warning — rendered outside container to avoid contain: style breaking position: fixed */}
      {/* Selection length badge */}
      <SelectionLengthBadge
        selectionMode={selectionMode}
        isEnzymeSelection={isEnzymeSelection}
        selStart={selStart}
        selEnd={selEnd}
        cleanSeq={cleanSeq}
        selectedPrimerIds={selectedPrimerIds}
        enrichedPrimers={enrichedPrimers}
        enzymeActiveBlue={enzymeActiveBlue}
        amplimerGreen={amplimerGreen}
        hasWarningBelow={combinedWarnings.length > 0}
        topology={topology}
        unit={seqUnit}
        showGc={moleculeType !== 'protein'}
        showMw={moleculeType === 'protein'}
      />
      {combinedWarnings.length > 0 && <WarningBadge warnings={combinedWarnings} />}
      {mapWatermark && (
        <MapWatermark
          length={cleanSeq.length}
          features={features}
          topology={topology}
          name={mapName}
          sel={selStart != null && selEnd != null ? { start: selStart, end: selEnd } : null}
        />
      )}
      {foldWatermark && (
        <FoldWatermark
          sequence={cleanSeq}
          selStart={selectionMode === 'text' ? selStart : null}
          selEnd={selectionMode === 'text' ? selEnd : null}
        />
      )}
      <EditorStatusBar
        designPick={designPick}
        designPickError={designPickError}
        cancelDesignPick={cancelDesignPick}
        agentLocked={agentLocked}
        onUnlockAgent={onUnlockAgent}
        onOpenSessionEditLog={onOpenSessionEditLog}
      />
      {!designPick && !agentLocked && (
        <EditorNavMenu
          onSave={onSave}
          onSaveAs={onSaveAs}
          canDirectSave={canDirectSave}
          canUndo={canUndo}
          canRedo={canRedo}
          onUndo={onUndo}
          onRedo={onRedo}
          hasSelection={hasSelection}
          hasTextSelection={(hasSelection && selectionMode === 'text') || hasTranslationSelection}
          hasTranslationSelection={hasTranslationSelection}
          canPaste={hasSelection || cursorIndex !== null || hasTranslationSelection}
          onCopySense={() => copySelection('sense')}
          onCopyAntisense={() => copySelection('antisense')}
          onCopyTranslation={() => copySelection('translation')}
          onPaste={pasteFromClipboard}
          onToUppercase={toUppercase}
          onToLowercase={toLowercase}
          showFeatures={showFeatures}
          onToggleFeatures={onToggleFeatures}
          pluginToggles={pluginToggles}
          disabledPlugins={disabledPlugins}
          showOrfs={showOrfs}
          onToggleOrfs={onToggleOrfs}
          onCreateFeature={createFeature}
          showPrimers={showPrimers}
          onTogglePrimers={onTogglePrimers}
          onCreatePrimer={createPrimer}
          showEnzymes={showEnzymes}
          onToggleEnzymes={onToggleEnzymes}
          enzymeFilter={enzymeFilter}
          onEnzymeFilterChange={onEnzymeFilterChange}
          enzymeProvider={enzymeProvider}
          onEnzymeProviderChange={onEnzymeProviderChange}
          onSearch={handleSearch}
          searchNav={searchNav}
          openSearchRef={openSearchRef}
          alignments={alignments}
          alignmentEnabled={alignmentEnabled}
          showAlignments={showAlignments}
          onToggleAlignments={onToggleAlignments}
          hiddenAlignIds={hiddenAlignIds}
          onToggleAlignmentVisible={onToggleAlignmentVisible}
          onAddAlignmentFile={onAddAlignmentFile}
          onAddAlignmentText={onAddAlignmentText}
          onManageAlignments={onManageAlignments}
          onOpenRnaFold={onOpenRnaFold}
          onOpenDotplot={onOpenDotplot}
          onOpenGel={handleOpenGel}
          onOpenMapView={onOpenMapView}
          onOpenSnapshots={onOpenSnapshots}
          onOpenSessionEditLog={onOpenSessionEditLog}
          background={background}
          backgroundOptions={backgroundOptions}
          onBackgroundChange={onBackgroundChange}
          onPrimerDesign={handlePrimerDesign}
          primerDesignEnabled={primerDesignEnabled}
          onOpenMyPrimers={onOpenMyPrimers}
          onOpenPrimerOverview={onOpenPrimerOverview}
          onOpenDetectFeatures={onOpenDetectFeatures}
          onOpenCodonOptimization={onOpenCodonOptimization}
          onAddCurrentPrimerToMyPrimers={handleAddCurrentPrimerToMyPrimers}
          onAddAllPrimersToMyPrimers={onAddAllPrimersToMyPrimers}
          autoAddPrimers={autoAddPrimers}
          onToggleAutoAddPrimers={onToggleAutoAddPrimers}
          hasSelectedPrimer={selectionMode === 'primer' && selectedPrimerIds.length === 1}
          hasPrimers={(primers || []).length > 0}
          onOpenMyEnzymes={onOpenMyEnzymes}
          onOpenEnzymeDatabase={onOpenEnzymeDatabase}
          myEnzymes={myEnzymes}
          topology={topology}
          onToggleTopology={onToggleTopology}
          moleculeType={moleculeType}
          viewMode={viewMode}
          onViewModeChange={onViewModeChange}
          showDoubleStrand={showDoubleStrand}
          onToggleDoubleStrand={onToggleDoubleStrand}
          showTextColor={showTextColor}
          onToggleTextColor={onToggleTextColor}
          alwaysExpandFeatures={alwaysExpandFeatures}
          onToggleAlwaysExpandFeatures={onToggleAlwaysExpandFeatures}
          alwaysExpandPrimers={alwaysExpandPrimers}
          onToggleAlwaysExpandPrimers={onToggleAlwaysExpandPrimers}
          featureLabelsBelow={featureLabelsBelow}
          onFeatureLabelsBelowChange={onFeatureLabelsBelowChange}
          thickFeatureLines={thickFeatureLines}
          onToggleThickFeatureLines={onToggleThickFeatureLines}
        />
      )}
      <AlignmentNavigator
        show={isDna && alignmentTracks.length > 0}
        continuous={continuous}
        onJump={jumpToAlignSite}
      />
      <div
        ref={mergeContainerRef}
        className={continuous ? 'hide-scrollbar' : undefined}
        onContextMenu={handleContextMenu}
        onMouseDown={(e) => {
          // Blank margins outside the SVG: drop any selection/cursor. Clicks
          // inside the SVG are handled by handleSvgMouseDown (target = svg).
          if (e.button !== 0) return;
          const t = e.target;
          if (t === e.currentTarget || t.parentElement === e.currentTarget) {
            setSelStart(null);
            setSelEnd(null);
            setCursorIndex(null);
            clearCursorTimer();
          }
        }}
        style={{
          backgroundColor: bgColor,
          width: '100%',
          minHeight: '100vh',
          // Block + auto margins (not flex justify-center): when the row is
          // widened by insertion slots, flex centering makes the left
          // overflow unreachable while scrolling.
          margin: 0,
          padding: '0 1rem 4rem 1rem',
          overflowX: 'auto',
          userSelect: 'none',
          contain: 'layout style',
        }}
      >
        <div
          style={{
            width: svgWidth,
            position: 'relative',
            margin: '0 auto',
            // Continuous: centre the single block vertically when it is
            // shorter than the viewport.
            paddingTop: continuous ? Math.max(0, (viewportH - svgHeight) / 2) : 0,
          }}
        >
          <svg
            ref={svgRef}
            width="100%"
            height={svgHeight}
            style={{
              display: 'block',
              overflow: 'visible',
              willChange: 'transform',
              transform: 'translateZ(0)',
              fontFeatureSettings: '"calt" on, "ss01" on',
            }}
            onMouseDown={handleSvgMouseDown}
            onMouseMove={handleSvgMouseMove}
            onMouseLeave={handleSvgMouseLeave}
          >
            <style>{`
              @keyframes alignLabelScroll { from { transform: translateX(0); } to { transform: translateX(var(--align-label-scroll, 0px)); } }
            `}</style>
            <CursorLayer
              cursorIndex={cursorIndex}
              visible={cursorVisible}
              hasSelection={hasSelection}
              isDragging={isDragging}
              selectionMode={selectionMode}
              currentSelColor={currentSelColor}
              rowOf={rowOf}
              colOfAbs={colOfAbs}
              numRows={numRows}
              getSeqY={getSeqY}
              rowAbove={rowAbove}
              rowBelow={rowBelow}
            />
            <DesignPickedLayer
              designPick={designPick}
              getSeqY={getSeqY}
              sp={sp}
              colRuns={colRuns}
            />
            <SelectionLayer
              hasSelection={hasSelection}
              selectionMode={selectionMode}
              isEnzymeSelection={isEnzymeSelection}
              selStart={selStart}
              selEnd={selEnd}
              getSeqY={getSeqY}
              sp={sp}
              currentSelColor={currentSelColor}
              colRuns={colRuns}
              dsBottomY={dsOn ? DS_BOTTOM_Y : 0}
            />
            {/* rna/protein are single-strand: no alignment/enzyme/primer layers */}
            {isDna && (
              <AlignmentLayers
                alignmentTracks={alignmentTracks}
                visibleRows={visibleRows}
                rowBuf={ROW_BUF}
                numRows={numRows}
                sp={sp}
                getSeqY={getSeqY}
                lp={lp}
                rowStarts={rowStarts}
                visCpl={visCpl}
                streamOf={streamOf}
                colVis={colVis}
                colRuns={colRuns}
                colFromVis={colFromVis}
                insReserve={insReserve}
                alignLaneInfo={alignLaneInfo}
                sequence={sequence}
                cleanSeq={cleanSeq}
                alignmentTraceAvailable={alignmentTraceAvailable}
                expandedChromAlnId={expandedChromAlnId}
                onToggleAlignmentChrom={onToggleAlignmentChrom}
                onHideAlignment={onHideAlignment}
                continuous={continuous}
                visibleCols={visibleCols}
                labelViewport={labelViewport}
              />
            )}
            {isDna && (
              <ChromatogramLayers
                chromatogram={chromatogram}
                alignmentTracks={alignmentTracks}
                alignmentChromatograms={alignmentChromatograms}
                visibleRows={visibleRows}
                rowBuf={ROW_BUF}
                numRows={numRows}
                rowStarts={rowStarts}
                rowCounts={rowCounts}
                visCpl={visCpl}
                streamOf={streamOf}
                colVis={colVis}
                insReserve={insReserve}
                lp={lp}
                alignLaneInfo={alignLaneInfo}
                getSeqY={getSeqY}
              />
            )}
            {renderedTracks}
            {renderedFeatures}
            {renderedFeatureLabels}
            {isDna && renderedEnzymes}
            {isDna && renderedEnzymeLabels}
            {isDna && renderedEnzymeOverlay}
            {/* Primers paint above enzyme labels: their labels are avoided by
                reservation, and expanded (hover/selected) blocks cleanly
                occlude low-lying enzyme labels instead of interleaving. */}
            {isDna && renderedPrimers}
            <SeqBgLayer
              visibleRows={visibleRows}
              rowBuf={ROW_BUF}
              numRows={numRows}
              insReserve={insReserve}
              streamOf={streamOf}
              visCpl={visCpl}
              rowCounts={rowCounts}
              rowStarts={rowStarts}
              cleanSeq={cleanSeq}
              getSeqY={getSeqY}
              colRuns={colRuns}
              colFromVis={colFromVis}
              visibleCols={visibleCols}
              textColors={textColorsTop}
            />
            {dsOn && (
              <RulerLayer
                visibleRows={visibleRows}
                rowBuf={ROW_BUF}
                numRows={numRows}
                rowCounts={rowCounts}
                getSeqY={getSeqY}
                colRuns={colRuns}
                colFromVis={colFromVis}
                visibleCols={visibleCols}
              />
            )}
            {dsOn && (
              <ComplementLayer
                visibleRows={visibleRows}
                rowBuf={ROW_BUF}
                numRows={numRows}
                insReserve={insReserve}
                streamOf={streamOf}
                visCpl={visCpl}
                rowCounts={rowCounts}
                rowStarts={rowStarts}
                cleanSeq={cleanSeq}
                getSeqY={getSeqY}
                colRuns={colRuns}
                colFromVis={colFromVis}
                visibleCols={visibleCols}
                textColors={textColorsBottom}
              />
            )}
            <SeqSelLayer
              hasSelection={hasSelection}
              selectionMode={selectionMode}
              isEnzymeSelection={isEnzymeSelection}
              selStart={selStart}
              selEnd={selEnd}
              cleanSeq={cleanSeq}
              getSeqY={getSeqY}
              sp={sp}
              rowStarts={rowStarts}
              colVis={colVis}
              bottomY={dsOn ? DS_BOTTOM_Y : 0}
            />
            {isDna && (
              <TranslationSelectionLayer
                selectionMode={selectionMode}
                translationSel={translationSel}
                cdsFeatureData={cdsFeatureData}
                cleanSeq={cleanSeq}
                currentSelColor={currentSelColor}
                rowOf={rowOf}
                rowStarts={rowStarts}
                getSeqY={getSeqY}
                colRuns={colRuns}
                colVis={colVis}
              />
            )}
            {isDna && (
              <AmplimerRegionLayer
                selectionMode={selectionMode}
                selectedPrimerIds={selectedPrimerIds}
                enrichedPrimers={enrichedPrimers}
                cleanSeq={cleanSeq}
                topology={topology}
                getSeqY={getSeqY}
                sp={sp}
                rowStarts={rowStarts}
                colVis={colVis}
                colRuns={colRuns}
              />
            )}
            {renderedTooltips}
            <SelectionInfoLayer
              isDragging={isDragging}
              hasSelection={hasSelection}
              cursorIndex={cursorIndex}
              selectionMode={selectionMode}
              selStart={selStart}
              selEnd={selEnd}
              currentSelColor={currentSelColor}
              selectionTm={selectionTm}
              seqUnit={seqUnit}
              isDna={isDna}
              rowOf={rowOf}
              colOfAbs={colOfAbs}
              numRows={numRows}
              getSeqY={getSeqY}
              rowAbove={rowAbove}
              rowBelow={rowBelow}
            />
            <HoverIndexLayer
              hoveredIndex={hoveredIndex}
              isDragging={isDragging}
              isTranslationDragging={isTranslationDragging}
              rowOf={rowOf}
              colOfAbs={colOfAbs}
              getSeqY={getSeqY}
            />
          </svg>
        </div>
        <EditorDialogs
          featureInfoFeature={featureInfoFeature}
          createFeatureLoc={createFeatureLoc}
          setFeatureInfoFeature={setFeatureInfoFeature}
          setCreateFeatureLoc={setCreateFeatureLoc}
          onFtypeChange={onFeatureFtypeChange}
          onFeatureColorChange={onFeatureColorChange}
          onFeatureLocationChange={onFeatureLocationChange}
          onFeatureNameChange={onFeatureNameChange}
          onFeatureStrandChange={onFeatureStrandChange}
          onFeatureAdd={onFeatureAdd}
          onDeleteFeature={onFeatureDelete}
          features={features}
          moleculeType={moleculeType}
          enzymeDetailRecord={enzymeDetailRecord}
          setEnzymeDetailRecord={setEnzymeDetailRecord}
          enzymeDbRef={enzymeDbRef}
          providerIndexRef={providerIndexRef}
          enzymeDetailCutSites={enzymeDetailCutSites}
          cleanSeq={cleanSeq}
          enzymeProvider={enzymeProvider}
          primerAlignmentPrimer={primerAlignmentPrimer}
          createPrimerSeq={createPrimerSeq}
          setPrimerAlignmentPrimer={setPrimerAlignmentPrimer}
          setCreatePrimerSeq={setCreatePrimerSeq}
          primerAlignmentCache={primerAlignmentCache}
          seedLength={primerSeedLength}
          onPrimerChange={onPrimerChange}
          onPrimerDelete={onPrimerDelete}
          primers={primers}
          tmParams={tmParams}
          projectId={projectId}
          designDialog={designDialog}
          setDesignDialog={setDesignDialog}
          topology={topology}
        />
      </div>
    </>
  );
});

export default SequenceEditor;
