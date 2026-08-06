import React, { useState, useEffect, useLayoutEffect, useRef, useCallback, useMemo } from 'react';
import {
  cw,
  startX,
  baseSeqY,
  bgColor,
  monoFont,
  springAnim,
  getX,
  complement,
  measureWidth,
  featLabelW,
  enzLabelW,
  primerLabelW,
  splitRange,
  enzymeActiveBlue,
  amplimerGreen,
} from './editorConstants';
import FeatureInfoDialog from './FeatureInfoDialog';
import PrimerAlignmentDialog from './PrimerAlignmentDialog';
import EditorNavMenu from './EditorNavMenu';
import PrimerDesignDialog from './plugins/primerDesign/PrimerDesignDialog';
import { DESIGN_MODES } from './plugins/primerDesign';
import { computePrimerAlignment, computeTm } from './tauriApi';
import { buildSearchResults } from './searchUtils';
import { AlertTriangle } from 'lucide-react';

// ---------------------------------------------------------------------------
// Standard genetic code table
// ---------------------------------------------------------------------------
const GENETIC_CODE = {
  ATA: 'I',
  ATC: 'I',
  ATT: 'I',
  ATG: 'M',
  ACA: 'T',
  ACC: 'T',
  ACG: 'T',
  ACT: 'T',
  AAC: 'N',
  AAT: 'N',
  AAA: 'K',
  AAG: 'K',
  AGC: 'S',
  AGT: 'S',
  AGA: 'R',
  AGG: 'R',
  CTA: 'L',
  CTC: 'L',
  CTG: 'L',
  CTT: 'L',
  CCA: 'P',
  CCC: 'P',
  CCG: 'P',
  CCT: 'P',
  CAC: 'H',
  CAT: 'H',
  CAA: 'Q',
  CAG: 'Q',
  CGA: 'R',
  CGC: 'R',
  CGG: 'R',
  CGT: 'R',
  GTA: 'V',
  GTC: 'V',
  GTG: 'V',
  GTT: 'V',
  GCA: 'A',
  GCC: 'A',
  GCG: 'A',
  GCT: 'A',
  GAC: 'D',
  GAT: 'D',
  GAA: 'E',
  GAG: 'E',
  GGA: 'G',
  GGC: 'G',
  GGG: 'G',
  GGT: 'G',
  TCA: 'S',
  TCC: 'S',
  TCG: 'S',
  TCT: 'S',
  TTC: 'F',
  TTT: 'F',
  TTA: 'L',
  TTG: 'L',
  TAC: 'Y',
  TAT: 'Y',
  TAA: '*',
  TAG: '*',
  TGC: 'C',
  TGT: 'C',
  TGA: '*',
  TGG: 'W',
};

/**
 * Build CDS data for a feature from the DNA sequence.
 * Never uses feature.translation — always calculates from scratch.
 * Returns { trans, codonMap, codingBases } where:
 *   trans: array of { aa, templatePos2, codonIndex, bases } for each codon
 *   codonMap: Map from template position to its codon index
 *   codingBases: template positions in 5'→3' order
 */
function buildCDSData(feature, sequence) {
  const segs = feature.segments;
  if (!segs || !segs.length) return { trans: [], codonMap: new Map(), codingBases: [] };

  const isRev = feature.strand === '-';

  // Build CDS bases as template indices in 5'→3' order
  const codingBases = [];
  if (isRev) {
    for (let i = segs.length - 1; i >= 0; i--) {
      const seg = segs[i];
      for (let j = seg.end; j >= seg.start; j--) {
        if (j >= 0 && j < sequence.length) codingBases.push(j);
      }
    }
  } else {
    for (const seg of segs) {
      for (let j = seg.start; j <= seg.end; j++) {
        if (j >= 0 && j < sequence.length) codingBases.push(j);
      }
    }
  }

  const trans = [];
  const codonMap = new Map();
  for (let i = 0; i + 2 < codingBases.length; i += 3) {
    const t1 = codingBases[i];
    const t2 = codingBases[i + 1];
    const t3 = codingBases[i + 2];

    const b1 = isRev ? complement(sequence[t1]) : sequence[t1];
    const b2 = isRev ? complement(sequence[t2]) : sequence[t2];
    const b3 = isRev ? complement(sequence[t3]) : sequence[t3];

    const codon = (b1 + b2 + b3).toUpperCase();
    const aa = GENETIC_CODE[codon] || '?';
    const codonIndex = i / 3;
    trans.push({ aa, templatePos2: t2, codonIndex, bases: [t1, t2, t3] });
    codonMap.set(t1, codonIndex);
    codonMap.set(t2, codonIndex);
    codonMap.set(t3, codonIndex);
  }

  return { trans, codonMap, codingBases };
}

/** Feature types that get in-editor translation (codon) display. */
function isTranslatable(f) {
  return f.ftype === 'CDS' || f.ftype === 'mRNA';
}

/** Split an inclusive match range into linear segments; a range with
 *  end < start crosses the origin of a circular sequence. */
function buildMatchSegs(ms, me, tlen) {
  if (me < ms && tlen > 0) {
    return [
      { start: ms, end: tlen - 1 },
      { start: 0, end: me },
    ];
  }
  return [{ start: ms, end: me }];
}

/** Template sequence covered by a primer match (origin-crossing aware). */
function matchedSeqOf(p, cleanSeq) {
  return (p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }])
    .map((m) => cleanSeq.substring(m.start, m.end + 1))
    .join('');
}

// ---------------------------------------------------------------------------
// WarningBadge — floating indicator in bottom-right corner for various
// warnings (e.g. primers with no binding sites, CDS non-triplet length).
// Click to expand, mouse-leave to close.
// ---------------------------------------------------------------------------
function WarningBadge({ warnings }) {
  const [expanded, setExpanded] = useState(false);
  const ref = useRef(null);

  const onClick = useCallback(() => setExpanded((v) => !v), []);

  useEffect(() => {
    if (!expanded) return;
    const el = ref.current;
    if (!el) return;
    const handler = () => setExpanded(false);
    el.addEventListener('mouseleave', handler);
    return () => el.removeEventListener('mouseleave', handler);
  }, [expanded]);

  const primerWarnings = warnings.filter((w) => w.type === 'primer');
  const cdsLenWarnings = warnings.filter((w) => w.type === 'cds_len');
  const cdsTransWarnings = warnings.filter((w) => w.type === 'cds_trans');

  return (
    <div ref={ref} style={{ position: 'fixed', bottom: 14, right: 28, zIndex: 40 }}>
      {!expanded && (
        <div
          onClick={onClick}
          style={{
            backgroundColor: '#fef3c7',
            color: '#92400e',
            border: '1px solid #fde68a',
            borderRadius: 8,
            fontSize: '11px',
            lineHeight: '1.2',
            padding: '3px 7px',
            cursor: 'pointer',
            display: 'flex',
            alignItems: 'center',
            gap: 3,
          }}
        >
          <AlertTriangle className="size-3.5" />
          <span>{warnings.length}</span>
        </div>
      )}
      {expanded && (
        <div
          onClick={onClick}
          style={{
            backgroundColor: '#fef3c7',
            color: '#92400e',
            border: '1px solid #fde68a',
            borderRadius: 8,
            fontSize: '12px',
            lineHeight: '1.4',
            padding: '6px 10px',
            boxShadow: '0 2px 8px rgba(0,0,0,0.12)',
            maxWidth: 320,
            cursor: 'pointer',
          }}
        >
          {primerWarnings.length > 0 && (
            <>
              <div className="flex items-center gap-1.5 mb-1">
                <AlertTriangle className="size-3.5 shrink-0" />
                <span className="font-semibold">Unmatched Primers</span>
              </div>
              <ul style={{ margin: 0, paddingLeft: 18, listStyle: 'disc' }}>
                {primerWarnings.map((w) => (
                  <li key={w.id} style={{ fontFamily: monoFont, fontSize: '11px' }}>
                    {w.name}
                  </li>
                ))}
              </ul>
            </>
          )}
          {(cdsLenWarnings.length > 0 || cdsTransWarnings.length > 0) && (
            <>
              {primerWarnings.length > 0 && <div style={{ height: 6 }} />}
              <div className="flex items-center gap-1.5 mb-1">
                <AlertTriangle className="size-3.5 shrink-0" />
                <span className="font-semibold">Check Translation:</span>
              </div>
              <ul style={{ margin: 0, paddingLeft: 18, listStyle: 'disc' }}>
                {[...cdsLenWarnings, ...cdsTransWarnings].map((w) => (
                  <li key={w.id} style={{ fontFamily: monoFont, fontSize: '11px' }}>
                    {w.name}
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      )}
    </div>
  );
}

const complementStr = (s) =>
  s
    .split('')
    .map((c) => complement(c))
    .join('');
const reverseComplement = (s) => complementStr(s).split('').reverse().join('');

// ---------------------------------------------------------------------------
// SelectionLengthBadge — top-right badge showing "xx bp" for the current
// selection (text / enzyme / primer / amplimer).  Hover shows "Copy", click
// copies the corresponding sequence, then shows ✓ briefly.
// ---------------------------------------------------------------------------
function SelectionLengthBadge({
  selectionMode,
  isEnzymeSelection,
  selStart,
  selEnd,
  cleanSeq,
  selectedPrimerIds,
  enrichedPrimers,
  enzymeActiveBlue,
  amplimerGreen,
  hasWarningBelow,
}) {
  // Compute the display length + colour and the sequence for GC calculation.
  let len, bg, seqToCopy;

  if (selectionMode === 'amplimer' && selectedPrimerIds.length === 2) {
    const fp = enrichedPrimers.find((p) => p.id === selectedPrimerIds[0]);
    const rp = enrichedPrimers.find((p) => p.id === selectedPrimerIds[1]);
    if (!fp || !rp) return null;
    const fwdPrimer = fp.isFwd ? fp : rp;
    const revPrimer = fp.isFwd ? rp : fp;
    if (!fwdPrimer || !revPrimer) return null;
    const fSeq = fwdPrimer.primerSeq || matchedSeqOf(fwdPrimer, cleanSeq);
    const rSeq = revPrimer.primerSeq || matchedSeqOf(revPrimer, cleanSeq);
    let intervening;
    if (fwdPrimer.matchEnd < revPrimer.matchStart) {
      intervening = cleanSeq.substring(fwdPrimer.matchEnd + 1, revPrimer.matchStart);
    } else {
      intervening =
        cleanSeq.substring(fwdPrimer.matchEnd + 1) + cleanSeq.substring(0, revPrimer.matchStart);
    }
    len = fSeq.length + intervening.length + rSeq.length;
    seqToCopy = fSeq + intervening + reverseComplement(rSeq);
    bg = amplimerGreen;
  } else if (selectionMode === 'primer' && selectedPrimerIds.length === 1) {
    const p = enrichedPrimers.find((pr) => pr.id === selectedPrimerIds[0]);
    if (!p) return null;
    seqToCopy =
      p.primerSeq ||
      (p.matchStart !== undefined && p.matchEnd !== undefined ? matchedSeqOf(p, cleanSeq) : '');
    len =
      (p.primerSeq || '').length ||
      (p.matchStart !== undefined && p.matchEnd !== undefined
        ? matchedSeqOf(p, cleanSeq).length
        : 0);
    bg = p.isFwd === false ? '#4A148C' : '#166534';
  } else if (isEnzymeSelection) {
    if (selStart === null || selEnd === null) return null;
    len = selEnd - selStart + 1;
    seqToCopy = cleanSeq.substring(selStart, selEnd + 1);
    bg = enzymeActiveBlue;
  } else if (selectionMode === 'text' && selStart !== null && selEnd !== null) {
    len = selEnd - selStart + 1;
    seqToCopy = cleanSeq.substring(selStart, selEnd + 1);
    bg = '#3E2723';
  } else {
    return null;
  }

  // Compute GC content from the selected sequence
  const gc = (seqToCopy.match(/[GC]/gi) || []).length;
  const gcPct = seqToCopy.length > 0 ? Math.round((gc / seqToCopy.length) * 100) : 0;

  // Measure the default label width so the badge has stable width
  const line1 = `${len} bp`;
  const line2 = '100% GC';
  const w1 = measureWidth(line1, `600 11px ${monoFont}`);
  const w2 = measureWidth(line2, `600 11px ${monoFont}`);
  const minBadgeWidth = Math.max(w1, w2) + 10;

  return (
    <div
      style={{
        position: 'fixed',
        bottom: hasWarningBelow ? 42 : 14,
        right: 28,
        zIndex: 40,
        backgroundColor: bg,
        color: bgColor,
        border: `1px solid ${bg}`,
        borderRadius: 5,
        fontSize: '11px',
        lineHeight: '1.2',
        padding: '1px 5px',
        fontFamily: monoFont,
        minWidth: minBadgeWidth,
        textAlign: 'center',
        userSelect: 'none',
      }}
    >
      <div>{line1}</div>
      <div>{`${gcPct}% GC`}</div>
    </div>
  );
}

const isIISEnzyme = (e) => {
  if (!e) return false;
  const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
  return pairs.some((cp) => {
    const dTopRight = Math.max(0, cp.topCutIndex - e.recEnd);
    const dTopLeft = Math.max(0, e.recStart - cp.topCutIndex);
    const dBotRight = Math.max(0, cp.botCutIndex - e.recEnd);
    const dBotLeft = Math.max(0, e.recStart - cp.botCutIndex);
    return Math.max(dTopRight, dTopLeft, dBotRight, dBotLeft) >= 2;
  });
};

const splitEnzName = (name) => {
  // Italic: everything before the first digit or uppercase letter (beyond position 0)
  let at = name.length;
  for (let i = 1; i < name.length; i++) {
    const c = name[i];
    if ((c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9')) {
      at = i;
      break;
    }
  }
  return { italic: name.slice(0, at), normal: name.slice(at) };
};

// sRGB → relative luminance (WCAG 2.1)
const _hexToRgb = (h) => [
  parseInt(h.slice(1, 3), 16) / 255,
  parseInt(h.slice(3, 5), 16) / 255,
  parseInt(h.slice(5, 7), 16) / 255,
];
const _linearize = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const relLuminance = (r, g, b) =>
  0.2126 * _linearize(r) + 0.7152 * _linearize(g) + 0.0722 * _linearize(b);
const _rgbToHsl = (r, g, b) => {
  const M = Math.max(r, g, b),
    m = Math.min(r, g, b),
    d = M - m,
    l = (M + m) / 2;
  if (!d) return [0, 0, l];
  const s = l > 0.5 ? d / (2 - M - m) : d / (M + m);
  let h;
  if (M === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
  else if (M === g) h = ((b - r) / d + 2) / 6;
  else h = ((r - g) / d + 4) / 6;
  return [h, s, l];
};
const _hslToRgb = (h, s, l) => {
  if (!s) return [l, l, l];
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s,
    p = 2 * l - q;
  const hue2rgb = (t) => {
    if (t < 0) t++;
    if (t > 1) t--;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };
  return [hue2rgb(h + 1 / 3), hue2rgb(h), hue2rgb(h - 1 / 3)];
};
const _rgbToHex = (r, g, b) =>
  '#' +
  [r, g, b]
    .map((c) =>
      Math.round(c * 255)
        .toString(16)
        .padStart(2, '0'),
    )
    .join('');

const ensureReadableColor = (hex, bgHex = '#fdfbf7') => {
  const [r, g, b] = _hexToRgb(hex);
  const [br, bg, bb] = _hexToRgb(bgHex);
  const bgLum = relLuminance(br, bg, bb);
  const lum = relLuminance(r, g, b);
  const MIN_CONTRAST = 3.0; // WCAG non-text/UI-component minimum
  if ((bgLum + 0.05) / (lum + 0.05) >= MIN_CONTRAST) return hex;
  const [h, s, l] = _rgbToHsl(r, g, b);
  // Darken (keeping hue/saturation) until the contrast target is met.
  let newL = l;
  while (newL > 0.1) {
    newL = Math.max(0.1, newL - 0.02);
    const [nr, ng, nb] = _hslToRgb(h, s, newL);
    if ((bgLum + 0.05) / (relLuminance(nr, ng, nb) + 0.05) >= MIN_CONTRAST)
      return _rgbToHex(nr, ng, nb);
  }
  return _rgbToHex(..._hslToRgb(h, s, 0.1));
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
  onFeatureLocationChange,
  onFeatureNameChange,
  onFeatureStrandChange,
  onPrimerChange,
  onFeatureAdd,
  onFeatureDelete,
  onPrimerDelete,
  primerSeedLength,
  tmParams = {},
  onSelectionChange,
  scrollContainerRef,
  onUndo,
  onRedo,
  canUndo,
  canRedo,
  showFeatures,
  onToggleFeatures,
  alwaysExpandFeatures = false,
  onToggleAlwaysExpandFeatures,
  showPrimers,
  onTogglePrimers,
  showEnzymes,
  onToggleEnzymes,
  enzymeFilter,
  onEnzymeFilterChange,
  openPrimerEditorRef,
  openFeatureEditorRef,
  alignmentCacheRef,
  alignmentTracks = [],
  alignments = [],
  alignmentEnabled = true,
  showAlignments = true,
  onToggleAlignments,
  hiddenAlignIds = [],
  onToggleAlignmentVisible,
  onAddAlignmentFile,
  onAddAlignmentText,
  onManageAlignments,
  onEnzymeHoverChange,
  topology = 'linear',
  onOpenMyPrimers,
  onOpenPrimerOverview,
  onOpenDetectFeatures,
  onOpenMyEnzymes,
  onOpenEnzymeDatabase,
  onAddPrimerToMyPrimers,
  onAddAllPrimersToMyPrimers,
  autoAddPrimers = false,
  onToggleAutoAddPrimers,
  myEnzymes = [],
}) {
  const containerRef = useRef(null);
  const [charsPerLine, setCharsPerLine] = useState(initialCharsPerLine);
  const [hoveredFeature, setHoveredFeature] = useState(null);
  const featureLeaveRef = useRef(null);
  const [featureInfoFeature, setFeatureInfoFeature] = useState(null); // for FeatureInfoDialog (edit mode)
  const [createFeatureLoc, setCreateFeatureLoc] = useState(null); // for FeatureInfoDialog (create mode, null=closed, string=location)
  const [primerAlignmentPrimer, setPrimerAlignmentPrimer] = useState(null); // for PrimerAlignmentDialog (edit mode)
  const [createPrimerSeq, setCreatePrimerSeq] = useState(null); // for PrimerAlignmentDialog (create mode, null=closed, '' or string=sequence)
  const [primerAlignmentCache, setPrimerAlignmentCache] = useState({});

  // Sync alignment cache to parent ref (used by PrimerOverviewDialog)
  useEffect(() => {
    if (alignmentCacheRef) {
      alignmentCacheRef.current = primerAlignmentCache;
    }
  }, [primerAlignmentCache, alignmentCacheRef]);

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
  const [insPopover, setInsPopover] = useState(null); // { x, y, bases } for alignment insertions
  const [hoverAlignLabel, setHoverAlignLabel] = useState(null); // `${alignmentId}:${row}`

  useEffect(() => {
    if (!insPopover) return;
    const close = () => setInsPopover(null);
    const onKey = (e) => {
      if (e.key === 'Escape') close();
    };
    const timer = setTimeout(() => window.addEventListener('mousedown', close), 0);
    window.addEventListener('keydown', onKey);
    return () => {
      clearTimeout(timer);
      window.removeEventListener('mousedown', close);
      window.removeEventListener('keydown', onKey);
    };
  }, [insPopover]);
  const [hoveredEnzyme, setHoveredEnzyme] = useState(null);
  const [scrollY, setScrollY] = useState(0);
  const [viewportH, setViewportH] = useState(900);
  const scrollTickingRef = useRef(false);
  const liveScrollTopRef = useRef(0);
  const lastVisibleStartRef = useRef(-1);
  const lastVisibleEndRef = useRef(-1);
  const numRowsRef = useRef(1);
  const svgRef = useRef(null);

  // --- selection state ---
  const [cursorIndex, setCursorIndex] = useState(null);
  const [selStart, setSelStart] = useState(null);
  const [selEnd, setSelEnd] = useState(null);
  const [isDragging, setIsDragging] = useState(false);
  const [selectionTm, setSelectionTm] = useState(null);
  const dragRef = useRef({ startIdx: null, active: false });
  const isDraggingRef = useRef(false);
  const [hoveredIndex, setHoveredIndex] = useState(null);
  const hoveredIndexRef = useRef(null);
  const cursorTimerRef = useRef(null);

  // --- primer design pick-mode state ---
  const [designPick, setDesignPick] = useState(null); // { mode, segments: [] }
  const [designPickError, setDesignPickError] = useState(null);
  const [designDialog, setDesignDialog] = useState(null); // { mode, segments }
  const designCapturedRef = useRef(null);

  // --- translation (codon) selection state ---
  const [translationSel, setTranslationSel] = useState(null); // { featureId, startCodon, endCodon }
  const [isTranslationDragging, setIsTranslationDragging] = useState(false);
  const translationDragRef = useRef(null); // { featureId, startCodon }
  const cdsFeatureDataRef = useRef({}); // mirror of cdsFeatureData for early callbacks
  const resetCursorTimer = useCallback(() => {
    if (cursorTimerRef.current) clearTimeout(cursorTimerRef.current);
    cursorTimerRef.current = setTimeout(() => setCursorIndex(null), 5000);
  }, []);

  const clearCursorTimer = useCallback(() => {
    if (cursorTimerRef.current) {
      clearTimeout(cursorTimerRef.current);
      cursorTimerRef.current = null;
    }
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
      if (primerDimTimerRef.current) {
        clearTimeout(primerDimTimerRef.current);
        primerDimTimerRef.current = null;
      }
      setPrimerDimActive(false);
    },
    [clearCursorTimer],
  );

  // Restore cursor/selection from undo/redo or project switch (external restoreState)
  const restoreVersionRef = useRef(0);
  const scrollToSeqIndexRef = useRef(null);
  useEffect(() => {
    if (!restoreState) return;
    if (restoreState.version === restoreVersionRef.current) return;
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
    if (restoreState.cursorIndex !== null) {
      resetCursorTimer();
    }
    if (restoreState.scrollToIndex != null) {
      scrollToSeqIndexRef.current?.(restoreState.scrollToIndex);
    }
  }, [restoreState, resetCursorTimer]);

  // --- enzyme selection state ---
  const [isEnzymeSelection, setIsEnzymeSelection] = useState(false);
  const [isEnzymeDragging, setIsEnzymeDragging] = useState(false);
  const [selectedEnzymeIds, setSelectedEnzymeIds] = useState([]); // persists after click (can be 1 or 2)
  const enzymeDragRef = useRef(null);
  const lastEnzymeSelRef = useRef(null); // { enzymeId, cutIdx, name } for shift+click

  // --- primer / amplimer selection state ---
  // selectionMode: 'none' | 'text' | 'enzyme' | 'primer' | 'amplimer'
  const [selectionMode, setSelectionMode] = useState('none');
  const [selectedPrimerIds, setSelectedPrimerIds] = useState([]); // 1 for single, 2 for amplimer
  const [isPrimerDragging, setIsPrimerDragging] = useState(false);
  const [primerDimActive, setPrimerDimActive] = useState(false); // 100ms delayed dimming
  const primerDimTimerRef = useRef(null);
  const primerDragRef = useRef(null); // { startPrimerId, startFwd, didDrag, hoveredPrimerId }
  const isPrimerDraggingRef = useRef(false);

  // Notify parent of selection changes (for per-project state persistence)
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
    selStart <= selEnd &&
    (selectionMode === 'text' || isEnzymeSelection);
  const hasTranslationSelection = selectionMode === 'translation' && translationSel !== null;
  const currentSelColor = designPick ? '#0f766e' : isEnzymeSelection ? enzymeActiveBlue : '#3E2723';

  const handlePrimerDesign = useCallback(
    (mode) => {
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
    [topology, clearCursorTimer],
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

  const pp = useMemo(
    () => ({
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
      ...layoutParams,
    }),
    [layoutParams],
  );

  const lp = useMemo(
    () => ({
      minRowGap: 27,
      rowContentGap: 12,
      featTrackHeight: 18,
      featBaseOffset: 14,
      featLabelPad: 12,
      enzTrackHeight: 18,
      enzLineGap: 18,
      enzLabelBase: 51,
      enzAbovePad: 10,
      minAboveSpace: 24,
      minBelowSpace: 14,
      ...layoutParams,
    }),
    [layoutParams],
  );

  useEffect(() => {
    const scroller = scrollContainerRef?.current;
    const handleResize = () => {
      if (containerRef.current) {
        setCharsPerLine(
          Math.max(20, Math.floor((containerRef.current.clientWidth - startX * 2) / cw)),
        );
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
          const estRowH = 60;
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
      setCharsPerLine(
        Math.max(20, Math.floor((containerRef.current.clientWidth - startX * 2) / cw)),
      );
    }
    const scroller = scrollContainerRef?.current;
    if (scroller) setViewportH(scroller.clientHeight || 900);
  }, [layoutKey]);

  const cleanSeq = sequence || '';

  // Ensure primer color is a valid non-black hex, falling back to default green
  const safePrimerColor = (c) => {
    if (!c || c === '#000000' || c === '#000' || c === 'black') return '#166534';
    if (/^#[0-9a-f]{6}$/i.test(c)) return c;
    return '#166534';
  };

  // Async Tm computation via backend NN model
  useEffect(() => {
    if (!isDragging || selStart === null || selEnd === null) {
      setSelectionTm(null);
      return;
    }
    const seq = cleanSeq.substring(selStart, selEnd + 1);
    if (seq.length < 2) {
      setSelectionTm(null);
      return;
    }
    let cancelled = false;
    computeTm(seq, tmParams).then((tm) => {
      if (!cancelled) setSelectionTm(tm);
    });
    return () => {
      cancelled = true;
    };
  }, [isDragging, selStart, selEnd, cleanSeq, tmParams]);

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
  }, [features, cleanSeq]);

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

  const numRows = Math.max(1, Math.ceil(cleanSeq.length / charsPerLine));
  numRowsRef.current = numRows;
  const svgWidth = startX + charsPerLine * cw + startX;

  const sp = useCallback((s, e) => splitRange(s, e, charsPerLine), [charsPerLine]);

  // Per-row compact lane assignment: an alignment only reserves a lane in
  // rows where it actually has sequence, so partial alignments leave no gaps.
  const alignLaneInfo = useMemo(() => {
    const perRow = Array.from({ length: numRows }, () => new Map());
    alignmentTracks.forEach((al, ti) => {
      const rows = new Set();
      for (const seg of al.segments || []) {
        for (const v of sp(seg.start, seg.end)) rows.add(v.row);
      }
      for (const r of rows) {
        if (r >= 0 && r < numRows && !perRow[r].has(ti)) perRow[r].set(ti, perRow[r].size);
      }
    });
    const counts = perRow.map((m) => m.size);
    return { perRow, counts };
  }, [alignmentTracks, numRows, sp]);

  // --- collision avoidance: features + primers ---
  // Normalize features and pre-compute colors once
  const normFeatures = useMemo(
    () =>
      (features || []).map((f) => {
        const isRepeat = /repeat/i.test(f.ftype || '');
        const fixColor = (c) => (c && !isRepeat && !f.orf ? ensureReadableColor(c) : c);
        const fixedColor = fixColor(f.color);
        const segments = (
          f.segments && f.segments.length ? f.segments : [{ start: f.start, end: f.end }]
        ).map((seg) => ({
          ...seg,
          color: fixColor(seg.color),
        }));
        // Pre-compute dominant color (longest segment color)
        const colorLen = {};
        for (const seg of segments) {
          const c = seg.color || fixedColor || '#60A5FA';
          colorLen[c] = (colorLen[c] || 0) + (seg.end - seg.start + 1);
        }
        let dominantColor = fixedColor || '#60A5FA',
          bestLen = 0;
        for (const [c, len] of Object.entries(colorLen)) {
          if (len > bestLen) {
            dominantColor = c;
            bestLen = len;
          }
        }
        return { ...f, color: fixedColor, segments, dominantColor };
      }),
    [features],
  );

  const { processedFeatures, primerTracks, featureRowTracks, revPrimerFeatOffsets } =
    useMemo(() => {
      const resultFeatures = [];
      const bottomTracks = [];

      if (normFeatures.length > 0) {
        const sorted = [...normFeatures]
          .filter((f, i, arr) => {
            if (f.orf) return true;
            const fa = f.segments.flatMap((s) => [s.start, s.end]);
            return (
              arr.findIndex((x) => {
                if (x.orf) return false;
                const xa = x.segments.flatMap((s) => [s.start, s.end]);
                return fa.length === xa.length && fa.every((v, j) => v === xa[j]);
              }) === i
            );
          })
          .sort((a, b) => {
            // ORFs get lowest track priority (bottom-most)
            if (!!a.orf !== !!b.orf) return a.orf ? 1 : -1;
            const la = a.segments.reduce((s, seg) => s + seg.end - seg.start, 0);
            const lb = b.segments.reduce((s, seg) => s + seg.end - seg.start, 0);
            return lb - la || a.segments[0].start - b.segments[0].start;
          });
        for (const f of sorted) {
          const allStarts = f.segments.map((s) => s.start);
          const allEnds = f.segments.map((s) => s.end);
          const es = Math.min(...allStarts) - 0.5,
            ee = Math.max(...allEnds) + 0.5;
          let placed = false;
          for (let i = 0; i < bottomTracks.length; i++) {
            if (!bottomTracks[i].some((t) => !(ee < t.start || es > t.end))) {
              bottomTracks[i].push({ start: es, end: ee });
              resultFeatures.push({ ...f, trackIdx: i });
              placed = true;
              break;
            }
          }
          if (!placed) {
            bottomTracks.push([{ start: es, end: ee }]);
            resultFeatures.push({ ...f, trackIdx: bottomTracks.length - 1 });
          }
        }
      }

      // Per-row primer track assignment — primers on different rows can share tracks.
      const pTracks = {}; // { [primerId]: { [row]: trackIndex } }
      for (const isFwd of [true, false]) {
        const ofType = (enrichedPrimers || []).filter((p) => p.isFwd === isFwd);
        if (!ofType.length) continue;
        const matchLen = (p) =>
          (p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }]).reduce(
            (s, m) => s + m.end - m.start + 1,
            0,
          );
        const sorted = [...ofType].sort((a, b) => {
          const la = matchLen(a) + (a.mismatchStr?.length || 0);
          const lb = matchLen(b) + (b.mismatchStr?.length || 0);
          return lb - la || a.matchStart - b.matchStart;
        });
        for (let r = 0; r < numRows; r++) {
          const rs = r * charsPerLine,
            re = (r + 1) * charsPerLine - 1;
          const rowTracks = [];
          for (const p of sorted) {
            const ml = p.mismatchStr?.length || 0;
            const segs = p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }];
            segs.forEach((m, mi) => {
              // 5' tail extends left of matchStart (fwd) / right of matchEnd (rev)
              const vs = isFwd && mi === 0 ? m.start - ml : m.start;
              const ve = !isFwd && mi === segs.length - 1 ? m.end + ml : m.end;
              if (ve < rs || vs > re) return;
              if (!pTracks[p.id]) pTracks[p.id] = {};
              let placed = false;
              for (let i = 0; i < rowTracks.length; i++) {
                if (!rowTracks[i].some((t) => !(ve < t.start || vs > t.end))) {
                  rowTracks[i].push({ start: vs, end: ve });
                  if (pTracks[p.id][r] === undefined || i < pTracks[p.id][r]) {
                    pTracks[p.id][r] = i;
                  }
                  placed = true;
                  break;
                }
              }
              if (!placed) {
                rowTracks.push([{ start: vs, end: ve }]);
                if (pTracks[p.id][r] === undefined) pTracks[p.id][r] = rowTracks.length - 1;
              }
            });
          }
        }
      }

      // Per-row feature track assignment — features only reserve space where they actually overlap
      const fRowTracks = {};
      for (let r = 0; r < numRows; r++) {
        const rs = r * charsPerLine,
          re = (r + 1) * charsPerLine - 1;
        const rowFeats = resultFeatures.filter((f) =>
          f.segments.some((seg) => !(seg.end < rs || seg.start > re)),
        );
        rowFeats.sort((a, b) => {
          // ORFs get lowest track priority (bottom-most)
          if (!!a.orf !== !!b.orf) return a.orf ? 1 : -1;
          const la = a.segments.reduce((s, seg) => s + seg.end - seg.start, 0);
          const lb = b.segments.reduce((s, seg) => s + seg.end - seg.start, 0);
          return lb - la || a.segments[0].start - b.segments[0].start;
        });
        const rowTracks = [];
        for (const f of rowFeats) {
          const rowSegs = f.segments.filter((seg) => !(seg.end < rs || seg.start > re));
          const segStart = Math.min(...rowSegs.map((s) => s.start));
          const segEnd = Math.max(...rowSegs.map((s) => s.end));
          const isFRev = f.strand === '-';
          const labelCols =
            Math.ceil(primerLabelW(f.name) / cw) + 2 + (f.strand && f.strand !== '.' ? 2 : 0);
          const es = isFRev ? segStart - 0.5 : Math.max(rs, segStart - labelCols) - 0.5;
          const ee = isFRev ? segEnd + labelCols + 0.5 : segEnd + 0.5;
          let placed = false;
          for (let i = 0; i < rowTracks.length; i++) {
            if (!rowTracks[i].some((t) => !(ee < t.start || es > t.end))) {
              rowTracks[i].push({ start: es, end: ee });
              if (!fRowTracks[f.id]) fRowTracks[f.id] = {};
              fRowTracks[f.id][r] = i;
              placed = true;
              break;
            }
          }
          if (!placed) {
            rowTracks.push([{ start: es, end: ee }]);
            if (!fRowTracks[f.id]) fRowTracks[f.id] = {};
            fRowTracks[f.id][r] = rowTracks.length - 1;
          }
        }
      }

      // Rev primer offset when overlapping with features on the same row
      const revFeatOff = {};
      for (const p of (enrichedPrimers || []).filter((p) => !p.isFwd)) {
        if (p.matchStart === undefined) continue;
        const ml = p.mismatchStr?.length || 0;
        const psegs = p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }];
        revFeatOff[p.id] = {};
        psegs.forEach((m, mi) => {
          const vs = m.start,
            ve = mi === psegs.length - 1 ? m.end + ml : m.end;
          for (const f of resultFeatures) {
            for (const fseg of f.segments) {
              const isFRev = f.strand === '-';
              const labelCols =
                Math.ceil(primerLabelW(f.name) / cw) + 2 + (f.strand && f.strand !== '.' ? 2 : 0);
              const fvs = isFRev ? fseg.start : fseg.start - labelCols;
              const fve = isFRev ? fseg.end + labelCols : fseg.end;
              if (fve < vs || fvs > ve) continue;
              const sr = Math.floor(fseg.start / charsPerLine);
              const er = Math.floor(fseg.end / charsPerLine);
              for (let r = sr; r <= er; r++) {
                const ft = (fRowTracks[f.id] || {})[r] || 0;
                revFeatOff[p.id][r] = Math.max(
                  revFeatOff[p.id][r] || 0,
                  (ft + 1) * lp.featTrackHeight,
                );
              }
            }
          }
        });
      }

      return {
        processedFeatures: resultFeatures,
        primerTracks: pTracks,
        featureRowTracks: fRowTracks,
        revPrimerFeatOffsets: revFeatOff,
      };
    }, [features, enrichedPrimers, numRows, charsPerLine, lp]);

  // --- adaptive row spacing (memoized with pre-indexed lookups) ---
  const enzymesByRow = useMemo(() => {
    const map = {};
    for (const e of enzymes) {
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      const rows = new Set();
      for (const cp of pairs) {
        rows.add(Math.floor(cp.topCutIndex / charsPerLine));
      }
      for (const r of rows) {
        (map[r] || (map[r] = [])).push(e);
      }
    }
    return map;
  }, [enzymes, charsPerLine]);

  const primersByRow = useMemo(() => {
    const map = {};
    for (const p of enrichedPrimers || []) {
      if (p.matchStart === undefined || p.matchEnd === undefined) continue;
      // Only include rows that actually render primer segments (the match range).
      // Tail characters beyond the match segment's row are truncated by rendering.
      for (const m of p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }]) {
        const sr = Math.floor(m.start / charsPerLine);
        const er = Math.floor(m.end / charsPerLine);
        for (let r = sr; r <= er; r++) {
          if (!map[r]) map[r] = [];
          if (!map[r].includes(p)) map[r].push(p);
        }
      }
    }
    return map;
  }, [enrichedPrimers, charsPerLine]);

  const featuresByRow = useMemo(() => {
    const map = {};
    for (const f of processedFeatures) {
      for (const seg of f.segments) {
        const sr = Math.floor(seg.start / charsPerLine);
        const er = Math.floor(seg.end / charsPerLine);
        for (let r = sr; r <= er; r++) {
          (map[r] || (map[r] = [])).push({ feature: f, seg });
        }
      }
    }
    return map;
  }, [processedFeatures, charsPerLine]);

  // Pre-compute fwd primer label x-ranges per row so enzyme track assignment
  // can lift labels that would overlap a primer label
  const primerLabelOcc = useMemo(() => {
    const occ = {}; // { [row]: [{x1, x2, topOffset}] }
    const rp = primersByRow;
    for (const [rowStr, primers] of Object.entries(rp)) {
      const row = parseInt(rowStr, 10);
      const entries = [];
      for (const p of primers) {
        if (!p.isFwd) continue;
        const segs = (p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }]).flatMap((m) =>
          sp(m.start, m.end),
        );
        const firstSeg = segs[0];
        if (!firstSeg || firstSeg.row !== row) continue;
        const ml = p.mismatchStr?.length || 0;
        const drawMisLen = Math.min(ml, firstSeg.colStart + 5);
        const nameW = primerLabelW(p.name);
        const nameX =
          drawMisLen > 0 ? getX(firstSeg.colStart - drawMisLen) : getX(firstSeg.colStart) + cw / 2;
        const pt = (primerTracks[p.id] || {})[row] || 0;
        const hasTail = ml > 0;
        entries.push({
          x1: nameX,
          x2: nameX + nameW,
          topOffset: (hasTail ? 40 : 36) + pt * pp.trackGap,
        });
      }
      if (entries.length) occ[row] = entries;
    }
    return occ;
  }, [primersByRow, primerTracks, sp, pp.trackGap]);

  const { rowAbove, rowBelow, enzymeRowTracks } = useMemo(() => {
    // Per-row enzyme track assignment — cut-twice enzymes are expanded per pair
    const eTracks = {};
    for (let r = 0; r < numRows; r++) {
      const rEnz = enzymesByRow[r];
      if (!rEnz || !rEnz.length) continue;
      // Expand cut-twice enzymes into per-pair entries for independent track assignment
      const expanded = [];
      for (const e of rEnz) {
        const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
        pairs.forEach((cp, pi) => {
          if (Math.floor(cp.topCutIndex / charsPerLine) === r) {
            expanded.push({
              key: pairs.length > 1 ? `${e.id}_p${pi}` : e.id,
              cutIndex: cp.topCutIndex,
              name: e.name,
              isUnique: e.isUnique,
            });
          }
        });
      }
      const sorted = expanded.sort(
        (a, b) => b.cutIndex - a.cutIndex || b.name.length - a.name.length,
      );
      const occupied = [];
      for (const item of sorted) {
        const cs = item.cutIndex % charsPerLine;
        const ce = cs + Math.ceil(enzLabelW(item.name, item.isUnique) / cw);
        // Lift labels whose x-range overlaps a fwd primer label by whole
        // tracks, so the lift participates in collision checks and row spacing
        let avoidOff = 0;
        const occ = primerLabelOcc[r];
        if (occ) {
          const cutX = getX(cs);
          const enzW = enzLabelW(item.name, item.isUnique);
          for (const o of occ) {
            if (cutX < o.x2 + 4 && cutX + enzW > o.x1) {
              avoidOff = Math.max(avoidOff, o.topOffset);
            }
          }
        }
        let t = Math.ceil(avoidOff / lp.enzTrackHeight);
        while (occupied.some((o) => o.track === t && !(ce < o.cs || cs > o.ce))) t++;
        occupied.push({ track: t, cs, ce });
        if (!eTracks[item.key]) eTracks[item.key] = {};
        eTracks[item.key][r] = t;
      }
    }

    const above = new Array(numRows).fill(0);
    const below = new Array(numRows).fill(0);

    for (let r = 0; r < numRows; r++) {
      let ae = lp.minAboveSpace,
        be = lp.minBelowSpace;

      const rowPrimers = primersByRow[r];
      let maxFwdPrimerH = 0;

      if (rowPrimers) {
        for (const p of rowPrimers) {
          const t = (primerTracks[p.id] || {})[r] || 0;
          if (p.isFwd) {
            const hasTail = r === Math.floor(p.matchStart / charsPerLine);
            const extra = hasTail ? pp.fwdAboveExtra : pp.fwdAboveNonTailExtra;
            const h = pp.fwdAboveBase + t * pp.trackGap + extra;
            maxFwdPrimerH = Math.max(maxFwdPrimerH, h);
            ae = Math.max(ae, h);
          } else {
            const hasTail = r === Math.floor(p.matchEnd / charsPerLine);
            const extra = hasTail ? pp.revBelowExtra : pp.revBelowNonTailExtra;
            const featOff = (revPrimerFeatOffsets[p.id] || {})[r] || 0;
            be = Math.max(
              be,
              pp.revBelowBase +
                t * pp.trackGap +
                extra +
                featOff +
                alignLaneInfo.counts[r] * lp.featTrackHeight,
            );
          }
        }
      }

      // Enzymes: above fwd primers; per-row tracks from eTracks
      const rowEnz = enzymesByRow[r];
      if (rowEnz && rowEnz.length > 0) {
        let maxEnzTrack = 0;
        for (const e of rowEnz) {
          const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
          pairs.forEach((cp, pi) => {
            if (Math.floor(cp.topCutIndex / charsPerLine) !== r) return;
            const key = pairs.length > 1 ? `${e.id}_p${pi}` : e.id;
            const t = (eTracks[key] || {})[r] || 0;
            maxEnzTrack = Math.max(maxEnzTrack, t);
          });
        }
        const enzDefaultAbove = lp.enzLabelBase + lp.enzAbovePad;
        const enzBase =
          maxFwdPrimerH > 0 ? Math.max(enzDefaultAbove, maxFwdPrimerH + 38) : enzDefaultAbove;
        ae = Math.max(ae, enzBase + maxEnzTrack * lp.enzTrackHeight);
      }

      // Features: below sequence (shifted down by alignment lanes)
      const nAlign = alignLaneInfo.counts[r];
      if (nAlign > 0) {
        be = Math.max(be, lp.featBaseOffset + nAlign * lp.featTrackHeight + lp.featLabelPad);
      }
      const rowFeats = featuresByRow[r];
      if (rowFeats) {
        for (const { feature: f } of rowFeats) {
          const t = (featureRowTracks[f.id] || {})[r] || 0;
          be = Math.max(
            be,
            lp.featBaseOffset + (t + nAlign) * lp.featTrackHeight + lp.featLabelPad,
          );
        }
      }

      above[r] = ae;
      below[r] = be;
    }

    return { rowAbove: above, rowBelow: below, enzymeRowTracks: eTracks };
  }, [
    numRows,
    enzymesByRow,
    primersByRow,
    featuresByRow,
    primerTracks,
    featureRowTracks,
    revPrimerFeatOffsets,
    primerLabelOcc,
    charsPerLine,
    pp,
    lp,
    alignLaneInfo,
  ]);

  const rowY = useMemo(() => {
    const y = [Math.max(baseSeqY, rowAbove[0] + lp.minRowGap)];
    for (let r = 0; r < numRows - 1; r++) {
      y.push(y[r] + Math.max(lp.minRowGap, rowBelow[r] + rowAbove[r + 1] + lp.rowContentGap));
    }
    return y;
  }, [numRows, rowAbove, rowBelow, lp.minRowGap, lp.rowContentGap]);
  const getSeqY = useCallback((row) => rowY[Math.min(row, rowY.length - 1)], [rowY]);

  // Scroll so the row containing `seqIndex` is inside the viewport (vertical only)
  const scrollToSeqIndex = useCallback(
    (seqIndex) => {
      if (seqIndex == null || !rowY.length) return;
      const row = Math.min(rowY.length - 1, Math.floor(seqIndex / charsPerLine));
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
    [rowY, rowAbove, rowBelow, charsPerLine, scrollContainerRef, viewportH],
  );
  scrollToSeqIndexRef.current = scrollToSeqIndex;

  // Keep the same rows in view when row spacing changes (feature/primer/enzyme toggles, resize)
  const rowAnchorRef = useRef(null);
  useLayoutEffect(() => {
    const prev = rowAnchorRef.current;
    rowAnchorRef.current = { rowY, rowAbove, charsPerLine };
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
    const newR = Math.min(rowY.length - 1, Math.floor((r * prev.charsPerLine) / charsPerLine));
    const newTop = Math.max(0, rowY[newR] - (rowAbove[newR] || 0) + delta);
    if (Math.abs(newTop - st) < 1) return;
    if (scroller) scroller.scrollTop = newTop;
    else window.scrollTo(0, newTop);
    setScrollY(newTop);
  }, [rowY, rowAbove, charsPerLine, scrollContainerRef]);

  // --- selection: coordinate conversion & event handlers ---
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
      const col = Math.max(0, Math.min(charsPerLine, colBase + side));
      // Content-based row boundaries: top of current row → top of next row
      // Row spacing already ensures a gap between row content areas
      let row = -1;
      for (let r = 0; r < numRows; r++) {
        const sy = getSeqY(r);
        const top = sy - rowAbove[r];
        const bottom = r < numRows - 1 ? getSeqY(r + 1) - rowAbove[r + 1] : Infinity;
        if (svgPt.y >= top && svgPt.y < bottom) {
          row = r;
          break;
        }
      }
      if (row < 0) return null;
      const idx = row * charsPerLine + col;
      return Math.max(0, Math.min(cleanSeq.length, idx));
    },
    [charsPerLine, numRows, getSeqY, cleanSeq],
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
      let col = Math.floor(xRel / cw);
      if (xRel < 0) col = 0;
      if (col >= charsPerLine) col = charsPerLine - 1;
      let row = -1;
      for (let r = 0; r < numRows; r++) {
        const sy = getSeqY(r);
        const top = sy - rowAbove[r];
        const bottom = r < numRows - 1 ? getSeqY(r + 1) - rowAbove[r + 1] : Infinity;
        if (svgPt.y >= top && svgPt.y < bottom) {
          row = r;
          break;
        }
      }
      if (row < 0) return null;
      const idx = row * charsPerLine + col;
      return Math.max(0, Math.min(cleanSeq.length - 1, idx));
    },
    [charsPerLine, numRows, getSeqY, cleanSeq],
  );

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
      if (idx === null) return;
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
    },
    [clientToCharIndex],
  );

  const handleSvgMouseLeave = useCallback(() => {
    hoveredIndexRef.current = null;
    setHoveredIndex(null);
  }, []);

  useEffect(() => {
    const onMove = (e) => {
      if (dragRef.current.startIdx === null) return;
      const idx = clientToSeqIndex(e.clientX, e.clientY);
      if (idx === null) return;
      const dist = Math.abs(idx - dragRef.current.startIdx);
      if (dist > 0) {
        dragRef.current.active = true;
        setIsDragging(true);
        // Cursor is at insertion point idx; selected chars are from min to max-1
        setSelStart(Math.min(dragRef.current.startIdx, idx));
        setSelEnd(Math.max(dragRef.current.startIdx, idx) - 1);
        setCursorIndex(idx);
        resetCursorTimer();
      }
    };
    window.addEventListener('mousemove', onMove);
    return () => window.removeEventListener('mousemove', onMove);
  }, [clientToSeqIndex, resetCursorTimer]);

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

  // --- Copy selection: mode = 'sense' | 'antisense' | 'translation' ---
  const copySelection = useCallback(
    (mode) => {
      if (mode === 'translation') {
        if (!translationSel) return;
        const cds = cdsFeatureDataRef.current[translationSel.featureId];
        if (!cds) return;
        const s = Math.min(translationSel.startCodon, translationSel.endCodon);
        const e = Math.max(translationSel.startCodon, translationSel.endCodon);
        const aa = cds.trans
          .slice(s, e + 1)
          .map((t) => t.aa)
          .join('');
        navigator.clipboard.writeText(aa).catch(() => {});
        return;
      }

      // When translation selection is active, sense/antisense should copy the
      // selected codon bases (without gap regions).
      if (translationSel && (mode === 'sense' || mode === 'antisense')) {
        const cds = cdsFeatureDataRef.current[translationSel.featureId];
        if (cds) {
          const start = Math.min(translationSel.startCodon, translationSel.endCodon);
          const end = Math.max(translationSel.startCodon, translationSel.endCodon);
          const posSet = new Set();
          for (let i = start; i <= end; i++) {
            const t = cds.trans[i];
            if (t) for (const b of t.bases) posSet.add(b);
          }
          const positions = [...posSet].sort((a, b) => a - b);
          const dna = positions.map((p) => cleanSeq[p]).join('');
          const text = mode === 'antisense' ? reverseComplement(dna) : dna;
          navigator.clipboard.writeText(text).catch(() => {});
          return;
        }
      }

      if (!hasSelection) return;
      const sense = cleanSeq.substring(selStart, selEnd + 1);
      let text;
      if (mode === 'antisense') {
        text = reverseComplement(sense);
      } else {
        text = sense;
      }
      navigator.clipboard.writeText(text).catch(() => {});
    },
    [hasSelection, cleanSeq, selStart, selEnd, translationSel, cdsFeatureDataRef],
  );

  // --- Paste: build insert/replace request from clipboard text ---
  const requestPaste = useCallback(
    (clipboardText) => {
      if (!clipboardText || !onEditRequest) return;
      if (hasSelection) {
        onEditRequest({
          type: 'replace',
          cursorIndex: selStart,
          selStart,
          selEnd,
          selectedText: cleanSeq.substring(selStart, selEnd + 1),
          clipboardText,
        });
      } else if (cursorIndex !== null) {
        onEditRequest({
          type: 'insert',
          cursorIndex,
          clipboardText,
        });
      }
    },
    [onEditRequest, hasSelection, cursorIndex, selStart, selEnd, cleanSeq],
  );

  const pasteFromClipboard = useCallback(() => {
    navigator.clipboard
      .readText()
      .then((t) => t && requestPaste(t))
      .catch(() => {});
  }, [requestPaste]);

  // --- Case conversion on the current text selection (goes through replace dialog) ---
  const convertSelectionCase = useCallback(
    (toUpper) => {
      if (hasTranslationSelection) {
        // Use the min … max base range of selected codons (includes gaps).
        const cds = cdsFeatureDataRef.current[translationSel.featureId];
        if (!cds || !onEditRequest) return;
        const start = Math.min(translationSel.startCodon, translationSel.endCodon);
        const end = Math.max(translationSel.startCodon, translationSel.endCodon);
        let minPos = Infinity,
          maxPos = -Infinity;
        for (let i = start; i <= end; i++) {
          const t = cds.trans[i];
          if (t)
            for (const b of t.bases) {
              if (b < minPos) minPos = b;
              if (b > maxPos) maxPos = b;
            }
        }
        if (minPos > maxPos) return;
        const selectedText = cleanSeq.substring(minPos, maxPos + 1);
        onEditRequest({
          type: 'replace',
          cursorIndex: minPos,
          selStart: minPos,
          selEnd: maxPos,
          selectedText,
          clipboardText: toUpper ? selectedText.toUpperCase() : selectedText.toLowerCase(),
        });
        return;
      }
      if (!hasSelection || selectionMode !== 'text' || !onEditRequest) return;
      const selectedText = cleanSeq.substring(selStart, selEnd + 1);
      onEditRequest({
        type: 'replace',
        cursorIndex: selStart,
        selStart,
        selEnd,
        selectedText,
        clipboardText: toUpper ? selectedText.toUpperCase() : selectedText.toLowerCase(),
      });
    },
    [
      hasSelection,
      selectionMode,
      hasTranslationSelection,
      translationSel,
      onEditRequest,
      cleanSeq,
      selStart,
      selEnd,
    ],
  );

  const toUppercase = useCallback(() => convertSelectionCase(true), [convertSelectionCase]);
  const toLowercase = useCallback(() => convertSelectionCase(false), [convertSelectionCase]);

  useEffect(() => {
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
        let ni = cursorIndex;
        if (e.key === 'ArrowLeft') ni = Math.max(0, cursorIndex - 1);
        else if (e.key === 'ArrowRight') ni = Math.min(cleanSeq.length, cursorIndex + 1);
        else if (e.key === 'ArrowUp') ni = Math.max(0, cursorIndex - charsPerLine);
        else if (e.key === 'ArrowDown') ni = Math.min(cleanSeq.length, cursorIndex + charsPerLine);
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
          // Replace mode: any active selection (text, enzyme, etc.)
          if (hasSelection) {
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

        // Delete/Backspace with selection → delete
        if ((e.key === 'Delete' || e.key === 'Backspace') && hasSelection && onEditRequest) {
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
      if ((e.ctrlKey || e.metaKey) && e.key === 'r') {
        e.preventDefault();
        setPrimerAlignmentPrimer(null); // clear edit mode
        if (hasSelection && selectionMode === 'text') {
          setCreatePrimerSeq(cleanSeq.substring(selStart, selEnd + 1));
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
          // Generate 1-based GenBank location: selStart+1..selEnd+1
          setCreateFeatureLoc(`${selStart + 1}..${selEnd + 1}`);
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
    cleanSeq,
    charsPerLine,
    resetCursorTimer,
    selectionMode,
    selectedPrimerIds,
    enrichedPrimers,
    onEditRequest,
    copySelection,
    translationSel,
    designPick,
    cancelDesignPick,
  ]);

  useEffect(() => {
    isDraggingRef.current = isDragging;
  }, [isDragging]);
  useEffect(
    () => () => {
      clearCursorTimer();
      clearTimeout(featureLeaveRef.current);
    },
    [clearCursorTimer],
  );

  // Sync featureInfoFeature when features update (e.g., after ftype change)
  useEffect(() => {
    if (featureInfoFeature && features.length) {
      const updated = features.find((f) => f.id === featureInfoFeature.id);
      if (updated && updated !== featureInfoFeature) {
        setFeatureInfoFeature(updated);
      }
    }
  }, [features, featureInfoFeature]);

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

  const createFeature = useCallback(() => {
    setFeatureInfoFeature(null);
    if (hasSelection && selectionMode === 'text') {
      setCreateFeatureLoc(`${selStart + 1}..${selEnd + 1}`);
    } else {
      setCreateFeatureLoc('');
    }
  }, [hasSelection, selectionMode, selStart, selEnd]);

  const createPrimer = useCallback(() => {
    setPrimerAlignmentPrimer(null);
    if (hasSelection && selectionMode === 'text') {
      setCreatePrimerSeq(cleanSeq.substring(selStart, selEnd + 1));
    } else {
      setCreatePrimerSeq('');
    }
  }, [hasSelection, selectionMode, cleanSeq, selStart, selEnd]);

  // --- Search: sequence (both strands, IUPAC) + feature/enzyme/primer names ---
  const [searchNav, setSearchNav] = useState({ query: '', index: -1, total: 0 });
  const searchStateRef = useRef({ query: '', scope: 'all', results: [], index: -1 });
  const openSearchRef = useRef(null);

  useEffect(() => {
    const onKeyDown = (e) => {
      if ((e.metaKey || e.ctrlKey) && (e.key === 'f' || e.key === 'F')) {
        e.preventDefault();
        openSearchRef.current?.();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

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

  // --- Paste event: read clipboard and trigger insert/replace dialog ---
  useEffect(() => {
    if (!onEditRequest) return;
    const onPaste = (e) => {
      // Ignore paste in input/textarea (e.g. dialog textarea, search input)
      const tag = e.target?.tagName?.toLowerCase();
      if (tag === 'input' || tag === 'textarea' || e.target?.isContentEditable) return;

      // Get clipboard text synchronously from the paste event
      const clipboardText = e.clipboardData?.getData('text') || '';
      if (!clipboardText) return;

      e.preventDefault();
      requestPaste(clipboardText);
    };
    window.addEventListener('paste', onPaste);
    return () => window.removeEventListener('paste', onPaste);
  }, [onEditRequest, requestPaste]);

  // Visible row range for enzyme virtualization
  const visibleRows = useMemo(() => {
    if (!rowY.length) return { start: 0, end: numRows - 1 };
    const vh = viewportH || 900;
    const top = scrollY;
    const bot = top + vh;
    let start = 0,
      end = numRows - 1;
    for (let r = 0; r < rowY.length; r++) {
      if (rowY[r] + (rowBelow[r] || 0) > top) {
        start = Math.max(0, r);
        break;
      }
    }
    for (let r = rowY.length - 1; r >= 0; r--) {
      if (rowY[r] - (rowAbove[r] || 0) < bot) {
        end = Math.min(numRows - 1, r + 1);
        break;
      }
    }
    return { start, end };
  }, [rowY, rowAbove, rowBelow, scrollY, viewportH, numRows]);

  // Filter enzymes to visible row range only
  const visibleEnzymes = useMemo(() => {
    if (!enzymes || !enzymes.length) return [];
    return enzymes.filter((e) => {
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      return pairs.some((cp) => {
        const r = Math.floor(cp.topCutIndex / charsPerLine);
        return r >= visibleRows.start && r <= visibleRows.end;
      });
    });
  }, [enzymes, visibleRows, charsPerLine]);
  const svgHeight = rowY[rowY.length - 1] + Math.max(40, rowBelow[numRows - 1] + 24);

  const ROW_BUF = 8; // rows above/below viewport to pre-render

  // Virtualize features: only render those overlapping visible rows
  const visibleFeatures = useMemo(() => {
    if (!processedFeatures.length) return [];
    const vs = Math.max(0, visibleRows.start - ROW_BUF);
    const ve = Math.min(numRows - 1, visibleRows.end + ROW_BUF);
    return processedFeatures.filter((f) =>
      f.segments.some((seg) => {
        const sr = Math.floor(seg.start / charsPerLine);
        const er = Math.floor(seg.end / charsPerLine);
        return !(er < vs || sr > ve);
      }),
    );
  }, [processedFeatures, visibleRows, numRows, charsPerLine]);

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
        const sr = Math.floor(m.start / charsPerLine);
        const er = Math.floor(m.end / charsPerLine);
        return !(er < vs - pad || sr > ve + pad);
      });
    });
  }, [enrichedPrimers, visibleRows, numRows, charsPerLine]);

  // --- enzyme track assignment is now in the spacing memo (enzymeRowTracks) ---

  // Pre-compute translation data (amino acid + codon index map) for translatable features
  const cdsFeatureData = useMemo(() => {
    const map = {};
    for (const f of normFeatures) {
      if (!isTranslatable(f)) continue;
      const data = buildCDSData(f, sequence);
      if (data.trans.length > 0) map[f.id] = data;
    }
    return map;
  }, [normFeatures, sequence]);

  // Sync to a ref so early callbacks (e.g. copySelection) can read current CDS data
  // without creating a TDZ by referencing this later-defined constant.
  useEffect(() => {
    cdsFeatureDataRef.current = cdsFeatureData;
  }, [cdsFeatureData]);

  // --- translation (codon) drag effect ---
  // Placed after cdsFeatureData so the dependency array can reference it.
  useEffect(() => {
    const onMove = (e) => {
      const drag = translationDragRef.current;
      if (!drag || !svgRef.current) return;
      const cds = cdsFeatureData[drag.featureId];
      if (!cds) return;

      // Compute row/col directly from SVG geometry so dragging works over the
      // feature bar and not only over the sequence text.
      const pt = svgRef.current.createSVGPoint();
      pt.x = e.clientX;
      pt.y = e.clientY;
      const ctm = svgRef.current.getScreenCTM();
      if (!ctm) return;
      const svgPt = pt.matrixTransform(ctm.inverse());
      const xRel = svgPt.x - startX;
      if (xRel < -cw / 2 || xRel > charsPerLine * cw + cw / 2) return;
      let col = Math.floor(xRel / cw);
      if (xRel < 0) col = 0;
      if (col >= charsPerLine) col = charsPerLine - 1;

      let row = -1;
      for (let r = 0; r < numRows; r++) {
        const top = getSeqY(r) - rowAbove[r];
        const bottom = r < numRows - 1 ? getSeqY(r + 1) - rowAbove[r + 1] : Infinity;
        if (svgPt.y >= top && svgPt.y < bottom) {
          row = r;
          break;
        }
      }
      if (row < 0) return;

      const idx = row * charsPerLine + col;
      const codon = cds.codonMap.get(idx);
      if (codon === undefined) return;
      const maxCodon = cds.trans.length - 1;
      const clamped = Math.max(0, Math.min(maxCodon, codon));
      setTranslationSel((prev) => {
        if (!prev || prev.featureId !== drag.featureId) return prev;
        return { featureId: drag.featureId, startCodon: drag.startCodon, endCodon: clamped };
      });
    };
    window.addEventListener('mousemove', onMove);
    return () => window.removeEventListener('mousemove', onMove);
  }, [cdsFeatureData, charsPerLine, numRows, getSeqY, rowAbove]);

  const renderedFeatures = useMemo(() => {
    if (!visibleFeatures.length) return null;
    return visibleFeatures.map((f) => {
      const isHovered = hoveredFeature === f.id;
      const isExpanded = alwaysExpandFeatures || isHovered;
      const dataSegs = f.segments;

      const visuals = [];
      const seenRows = new Set();
      for (let di = 0; di < dataSegs.length; di++) {
        const ds = dataSegs[di];
        if (di > 0) {
          const prevEnd = dataSegs[di - 1].end;
          if (ds.start > prevEnd + 1) {
            for (const vs of sp(prevEnd + 1, ds.start - 1)) {
              const showL = !seenRows.has(vs.row);
              seenRows.add(vs.row);
              visuals.push({
                type: 'gap',
                row: vs.row,
                colStart: vs.colStart,
                colEnd: vs.colEnd,
                showLabel: showL,
                color: f.color || ensureReadableColor('#60A5FA'),
              });
            }
          }
        }
        for (const vs of sp(ds.start, ds.end)) {
          const showLabel = !seenRows.has(vs.row);
          seenRows.add(vs.row);
          const segColor = dataSegs[di].color || f.color || ensureReadableColor('#60A5FA');
          visuals.push({
            type: 'solid',
            row: vs.row,
            colStart: vs.colStart,
            colEnd: vs.colEnd,
            showLabel,
            color: segColor,
          });
        }
      }

      visuals.sort(
        (a, b) => a.row * charsPerLine + a.colStart - (b.row * charsPerLine + b.colStart),
      );
      if (!visuals.length) return null;

      return (
        <g key={f.id}>
          {visuals.map((v) => {
            const x = getX(v.colStart);
            const w = (v.colEnd - v.colStart + 1) * cw;
            const sy = getSeqY(v.row);
            const rowTo =
              (((featureRowTracks[f.id] || {})[v.row] || 0) + alignLaneInfo.counts[v.row]) *
              lp.featTrackHeight;
            const y = sy + lp.featBaseOffset + rowTo;
            const isGap = v.type === 'gap';

            return (
              <g
                key={`${v.type}-${v.row}-${v.colStart}`}
                onMouseEnter={() => {
                  if (isDraggingRef.current) return;
                  clearTimeout(featureLeaveRef.current);
                  setHoveredFeature(f.id);
                }}
                onMouseLeave={() => {
                  featureLeaveRef.current = setTimeout(() => setHoveredFeature(null), 250);
                }}
                onMouseDown={(e) => {
                  e.stopPropagation();
                  e.preventDefault();

                  // Translatable features: start codon-unit selection
                  const cds = cdsFeatureData[f.id];
                  if (cds) {
                    // Compute clicked template index from the known visual row/column
                    // instead of re-detecting the row, which can be unreliable over the
                    // feature bar that sits below the sequence text.
                    const pt = svgRef.current.createSVGPoint();
                    pt.x = e.clientX;
                    pt.y = e.clientY;
                    const ctm = svgRef.current.getScreenCTM();
                    if (ctm) {
                      const svgPt = pt.matrixTransform(ctm.inverse());
                      let col = Math.floor((svgPt.x - startX) / cw);
                      col = Math.max(v.colStart, Math.min(v.colEnd, col));
                      const idx = v.row * charsPerLine + col;
                      const codon = cds.codonMap.get(idx);
                      if (codon !== undefined && codon !== null) {
                        startTranslationSelection(f.id, codon);
                        return;
                      }
                    }
                  }

                  const fStart = Math.min(...f.segments.map((s) => s.start));
                  const fEnd = Math.max(...f.segments.map((s) => s.end));
                  setSelStart(fStart);
                  setSelEnd(fEnd);
                  setCursorIndex(fEnd + 1);
                  clearCursorTimer();
                  // Clear primer / translation selection
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
                }}
                onDoubleClick={(e) => {
                  e.stopPropagation();
                  if (f.orf) return;
                  setCreateFeatureLoc(null);
                  setFeatureInfoFeature(f);
                }}
                className="cursor-pointer"
              >
                <rect
                  x={x}
                  y={isExpanded && !isGap && !f.orf ? sy - 18 : y}
                  width={w}
                  height={isExpanded && !isGap && !f.orf ? y - (sy - 18) : 0}
                  fill={v.color}
                  fillOpacity={isExpanded && !f.orf ? (isGap ? 0 : 0.15) : 0}
                  style={{ transition: springAnim, pointerEvents: 'none' }}
                />
                {alwaysExpandFeatures && isHovered && !isGap && !f.orf && (
                  <rect
                    x={x + 0.75}
                    y={sy - 18 + 0.75}
                    width={w - 1.5}
                    height={y - (sy - 18) - 1.5}
                    fill="none"
                    stroke={v.color}
                    strokeWidth={1.5}
                    style={{ pointerEvents: 'none' }}
                  />
                )}
                {f.orf ? (
                  <>
                    <line
                      x1={x}
                      x2={x + w}
                      y1={y}
                      y2={y}
                      stroke={v.color}
                      strokeWidth="13"
                      opacity={isGap ? 0.25 : 1}
                    />
                    <line x1={x} x2={x + w} y1={y} y2={y} stroke="transparent" strokeWidth="13" />
                  </>
                ) : (
                  <>
                    <line
                      x1={x}
                      x2={x + w}
                      y1={y}
                      y2={y}
                      stroke={isExpanded ? 'transparent' : bgColor}
                      strokeWidth="7"
                    />
                    <line
                      x1={x}
                      x2={x + w}
                      y1={y}
                      y2={y}
                      stroke={v.color}
                      strokeWidth="5"
                      opacity={isGap ? 0.25 : 1}
                    />
                    <line x1={x} x2={x + w} y1={y} y2={y} stroke="transparent" strokeWidth="10" />
                  </>
                )}
              </g>
            );
          })}

          {/* Feature translation — 1-letter AA centered on middle base of each codon */}
          {isTranslatable(f) &&
            (cdsFeatureData[f.id]?.trans || []).flatMap((t) => {
              const r = Math.floor(t.templatePos2 / charsPerLine);
              const c = t.templatePos2 % charsPerLine;
              const inVisual = visuals.some(
                (v) => v.row === r && c >= v.colStart && c <= v.colEnd && v.type !== 'gap',
              );
              if (!inVisual) return [];
              const sy = getSeqY(r);
              const rowTo =
                (((featureRowTracks[f.id] || {})[r] || 0) + alignLaneInfo.counts[r]) *
                lp.featTrackHeight;
              const y = sy + lp.featBaseOffset + rowTo;
              return (
                <text
                  key={`tr-${t.templatePos2}`}
                  x={getX(c) + cw / 2}
                  y={y}
                  fontSize={10}
                  fontWeight="900"
                  fontFamily={monoFont}
                  fill={f.orf ? bgColor : f.dominantColor}
                  stroke={f.orf ? 'none' : bgColor}
                  strokeWidth={f.orf ? 0 : 3}
                  paintOrder="stroke"
                  textAnchor="middle"
                  dominantBaseline="central"
                  style={{ pointerEvents: 'none' }}
                >
                  {t.aa}
                </text>
              );
            })}
        </g>
      );
    });
  }, [
    visibleFeatures,
    hoveredFeature,
    featureRowTracks,
    getSeqY,
    sp,
    charsPerLine,
    clearCursorTimer,
    lp,
    cdsFeatureData,
    setSelectionMode,
    setTranslationSel,
    startTranslationSelection,
    alignLaneInfo,
    alwaysExpandFeatures,
  ]);

  const truncatedLabel = useCallback((name, isRev, isFwd, maxLen = 12) => {
    const full = isRev ? `< ${name}` : isFwd ? `${name} >` : name;
    if (name.length <= maxLen) return { full, short: full };
    const short = isRev
      ? `< ${name.slice(0, maxLen)}··`
      : isFwd
        ? `${name.slice(0, maxLen)}·· >`
        : `${name.slice(0, maxLen)}··`;
    return { full, short };
  }, []);

  const renderedFeatureLabels = useMemo(() => {
    if (!visibleFeatures.length) return null;
    const seen = new Set();
    return visibleFeatures.flatMap((f) => {
      if (f.orf) return [];
      const isRev = f.strand === '-';
      const isFwd = f.strand === '+';
      const labelColor = f.dominantColor || f.color || '#60A5FA';
      const isHovered = hoveredFeature === f.id;
      const { full: fullText, short: shortText } = truncatedLabel(f.name, isRev, isFwd);
      const labelText = isHovered ? fullText : shortText;

      const rowLabels = {};
      const seenRows = new Set();

      for (let di = 0; di < f.segments.length; di++) {
        const ds = f.segments[di];
        if (di > 0) {
          const prevEnd = f.segments[di - 1].end;
          if (ds.start > prevEnd + 1) {
            for (const vs of sp(prevEnd + 1, ds.start - 1)) {
              if (!seenRows.has(vs.row) || isRev) {
                seenRows.add(vs.row);
                rowLabels[vs.row] = vs;
              }
            }
          }
        }
        for (const vs of sp(ds.start, ds.end)) {
          if (!seenRows.has(vs.row) || isRev) {
            seenRows.add(vs.row);
            rowLabels[vs.row] = vs;
          }
        }
      }

      return Object.values(rowLabels).map((vs) => {
        const key = `${f.id}-${vs.row}`;
        if (seen.has(key)) return null;
        seen.add(key);
        const sy = getSeqY(vs.row);
        const rowTo =
          (((featureRowTracks[f.id] || {})[vs.row] || 0) + alignLaneInfo.counts[vs.row]) *
          lp.featTrackHeight;
        const y = sy + lp.featBaseOffset + rowTo;
        const textProps = {
          y: y + 4,
          fontSize: '12px',
          fontFamily: 'TeX Gyre Heros',
          fontWeight: '600',
        };
        if (isRev) {
          const xr = getX(vs.colEnd + 1);
          return (
            <g
              key={key}
              onMouseEnter={() => {
                if (isDraggingRef.current) return;
                clearTimeout(featureLeaveRef.current);
                setHoveredFeature(f.id);
              }}
              onMouseLeave={() => {
                featureLeaveRef.current = setTimeout(() => setHoveredFeature(null), 250);
              }}
              onMouseDown={(e) => {
                e.stopPropagation();
                e.preventDefault();

                const fStart = Math.min(...f.segments.map((s) => s.start));
                const fEnd = Math.max(...f.segments.map((s) => s.end));
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
              }}
              onDoubleClick={(e) => {
                e.stopPropagation();
                setCreateFeatureLoc(null);
                setFeatureInfoFeature(f);
              }}
              className="cursor-pointer"
            >
              <text
                x={xr + 8}
                {...textProps}
                textAnchor="start"
                fill="none"
                stroke={bgColor}
                strokeWidth="5"
              >
                {labelText}
              </text>
              <text x={xr + 8} {...textProps} textAnchor="start" fill={labelColor} stroke="none">
                {labelText}
              </text>
            </g>
          );
        }
        const x = getX(vs.colStart);
        return (
          <g
            key={key}
            onMouseEnter={() => setHoveredFeature(f.id)}
            onMouseLeave={() => setHoveredFeature(null)}
            onMouseDown={(e) => {
              e.stopPropagation();
              e.preventDefault();

              const fStart = Math.min(...f.segments.map((s) => s.start));
              const fEnd = Math.max(...f.segments.map((s) => s.end));
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
            }}
            onDoubleClick={(e) => {
              e.stopPropagation();
              setCreateFeatureLoc(null);
              setFeatureInfoFeature(f);
            }}
            className="cursor-pointer"
          >
            <text
              x={x - 8}
              {...textProps}
              textAnchor="end"
              fill="none"
              stroke={bgColor}
              strokeWidth="5"
            >
              {labelText}
            </text>
            <text x={x - 8} {...textProps} textAnchor="end" fill={labelColor} stroke="none">
              {labelText}
            </text>
          </g>
        );
      });
    });
  }, [
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
  ]);

  const renderedAlignments = useMemo(() => {
    if (!alignmentTracks.length) return null;
    const vs = Math.max(0, visibleRows.start - ROW_BUF);
    const ve = Math.min(numRows - 1, visibleRows.end + ROW_BUF);
    return alignmentTracks.map((al, ti) => {
      const insMap = new Map((al.insertions || []).map((ins) => [ins.pos, ins.bases]));
      const rows = [];
      for (const seg of al.segments || []) {
        for (const v of sp(seg.start, seg.end)) {
          if (v.row < vs || v.row > ve) continue;
          const sy = getSeqY(v.row);
          const lane = alignLaneInfo.perRow[v.row]?.get(ti) ?? 0;
          const y = sy + lp.featBaseOffset + lane * lp.featTrackHeight + 8;
          const chars = (seg.chars || '').slice(v.strOffset, v.strOffset + v.len).split('');
          const mismatches = [];
          chars.forEach((c, i) => {
            const col = v.colStart + i;
            const gIdx = v.row * charsPerLine + col;
            if (
              insMap.has(gIdx) ||
              insMap.has(gIdx + 1) ||
              c === '-' ||
              c.toUpperCase() !== (sequence[gIdx] || '').toUpperCase()
            ) {
              mismatches.push(col);
            }
          });
          rows.push(
            <g key={`${v.row}-${v.colStart}`}>
              {mismatches.map((col) => (
                <rect
                  key={col}
                  x={getX(col)}
                  y={y - 11}
                  width={cw}
                  height={14}
                  fill="#fecaca"
                  fillOpacity={0.6}
                  style={{ pointerEvents: 'none' }}
                />
              ))}
              <text
                y={y}
                fontFamily="Cascadia Code"
                fontSize="13px"
                fontStyle="italic"
                fontWeight="350"
                style={{ userSelect: 'none' }}
              >
                {chars.map((c, i) => {
                  const col = v.colStart + i;
                  const gIdx = v.row * charsPerLine + col;
                  const insPos = insMap.has(gIdx) ? gIdx : insMap.has(gIdx + 1) ? gIdx + 1 : -1;
                  if (insPos >= 0) {
                    const insBases = insMap.get(insPos);
                    const display =
                      (insPos > 0 ? sequence[insPos - 1] || '' : '') +
                      insBases +
                      (sequence[insPos] || '');
                    return (
                      <tspan
                        key={col}
                        x={getX(col) + cw / 2}
                        textAnchor="middle"
                        fill="#1f2937"
                        fillOpacity={0.55}
                        style={{ cursor: 'pointer' }}
                        onMouseDown={(e) => {
                          e.stopPropagation();
                          e.preventDefault();
                          const insCol = insPos % charsPerLine;
                          setInsPopover({
                            x: insCol > 0 ? getX(insCol) : getX(col) + cw / 2,
                            y,
                            bases: display,
                          });
                        }}
                      >
                        ·
                      </tspan>
                    );
                  }
                  return (
                    <tspan
                      key={col}
                      x={getX(col) + cw / 2}
                      textAnchor="middle"
                      fill="#1f2937"
                      fillOpacity={0.55}
                    >
                      {c}
                    </tspan>
                  );
                })}
              </text>
            </g>,
          );
        }
      }
      return <g key={al.id}>{rows}</g>;
    });
  }, [
    alignmentTracks,
    visibleRows,
    numRows,
    sp,
    getSeqY,
    lp,
    charsPerLine,
    sequence,
    alignLaneInfo,
  ]);

  const renderedAlignmentLabels = useMemo(() => {
    if (!alignmentTracks.length) return null;
    const vs = Math.max(0, visibleRows.start - ROW_BUF);
    const ve = Math.min(numRows - 1, visibleRows.end + ROW_BUF);
    const textProps = {
      fontSize: '12px',
      fontFamily: 'TeX Gyre Heros',
      fontWeight: '600',
      fontStyle: 'italic',
      fill: '#78716C',
    };
    // Long names are middle-truncated to LABEL_MAX_W; hovering scrolls the
    // full name via a CSS marquee (distance = overflow width).
    const LABEL_MAX_W = 140;
    const middleTruncate = (name) => {
      if (featLabelW(name) <= LABEL_MAX_W) return name;
      let keep = name.length - 1;
      let s = name;
      while (keep > 4) {
        const l = Math.ceil(keep / 2);
        const r = Math.floor(keep / 2);
        s = `${name.slice(0, l)}…${name.slice(name.length - r)}`;
        if (featLabelW(s) <= LABEL_MAX_W) return s;
        keep--;
      }
      return s;
    };
    return alignmentTracks.map((al, ti) => {
      const rowLabels = {};
      for (const seg of al.segments || []) {
        for (const v of sp(seg.start, seg.end)) {
          if (v.row < vs || v.row > ve) continue;
          if (!rowLabels[v.row] || v.colEnd > rowLabels[v.row].colEnd) rowLabels[v.row] = v;
        }
      }
      const short = middleTruncate(al.name);
      const truncated = short !== al.name;
      return (
        <g key={al.id}>
          {Object.values(rowLabels).map((v) => {
            const sy = getSeqY(v.row);
            const lane = alignLaneInfo.perRow[v.row]?.get(ti) ?? 0;
            const y = sy + lp.featBaseOffset + lane * lp.featTrackHeight + 8;
            const hKey = `${al.id}:${v.row}`;
            const hovered = truncated && hoverAlignLabel === hKey;
            const labelX = getX(v.colEnd + 1) + 8;
            const clipId = `align-label-clip-${al.id}-${v.row}`;
            const scrollW = hovered ? featLabelW(al.name) - featLabelW(short) + 4 : 0;
            const textEl = (
              <text
                x={labelX}
                y={y}
                textAnchor="start"
                {...textProps}
                style={{
                  userSelect: 'none',
                  pointerEvents: truncated ? 'auto' : 'none',
                  cursor: truncated ? 'default' : undefined,
                  ...(hovered
                    ? {
                        '--align-label-scroll': `-${scrollW}px`,
                        animation: 'alignLabelScroll 2.5s linear infinite alternate',
                      }
                    : {}),
                }}
                onMouseEnter={truncated ? () => setHoverAlignLabel(hKey) : undefined}
                onMouseLeave={truncated ? () => setHoverAlignLabel(null) : undefined}
              >
                {hovered ? al.name : short}
              </text>
            );
            return (
              <g key={v.row}>
                {truncated && (
                  <defs>
                    <clipPath id={clipId}>
                      <rect x={labelX} y={y - 12} width={LABEL_MAX_W} height={16} />
                    </clipPath>
                  </defs>
                )}
                {truncated ? <g clipPath={`url(#${clipId})`}>{textEl}</g> : textEl}
              </g>
            );
          })}
        </g>
      );
    });
  }, [alignmentTracks, visibleRows, numRows, sp, getSeqY, lp, alignLaneInfo, hoverAlignLabel]);

  const renderedPrimers = useMemo(() => {
    if (!visiblePrimers.length) return null;
    return visiblePrimers.map((p) => {
      const isFwd = p.isFwd;
      const isHovered = hoveredPrimer === p.id;
      const misLen = p.mismatchStr?.length || 0;
      const hasMis = misLen > 0;
      const pColor = safePrimerColor(p.color);
      const segs = (p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }]).flatMap((m) =>
        sp(m.start, m.end),
      );
      if (p.renderCols) {
        for (const seg of segs) {
          const lo = seg.row * charsPerLine + seg.colStart;
          const hi = seg.row * charsPerLine + seg.colEnd;
          seg.renderCols = p.renderCols.filter(
            (rc) => rc.templateCol >= lo && rc.templateCol <= hi,
          );
        }
      }
      const tailSeg = isFwd ? segs[0] : segs[segs.length - 1];
      const arrowSeg = isFwd ? segs[segs.length - 1] : segs[0];

      let drawMisLen = misLen,
        showMisDots = false;
      if (hasMis) {
        if (isFwd) {
          const max = tailSeg.colStart + 5;
          if (misLen > max) {
            drawMisLen = max;
            showMisDots = true;
          }
        } else {
          const max = charsPerLine - 1 - tailSeg.colEnd + 5;
          if (misLen > max) {
            drawMisLen = max;
            showMisDots = true;
          }
        }
      }

      return (
        <g key={p.id}>
          {segs.map((seg) => {
            const isTail = seg === tailSeg,
              isArrow = seg === arrowSeg;
            const sy = getSeqY(seg.row);
            const featOff =
              (isFwd ? 0 : (revPrimerFeatOffsets[p.id] || {})[seg.row] || 0) +
              (isFwd ? 0 : alignLaneInfo.counts[seg.row] * lp.featTrackHeight);
            const trackOff = ((primerTracks[p.id] || {})[seg.row] || 0) * pp.trackGap + featOff;
            const matchY =
              (isFwd ? sy - pp.fwdMatchY : sy + pp.revMatchY) + (isFwd ? -trackOff : trackOff);
            const misY = matchY + (isFwd ? -pp.misYDelta : pp.misYDelta);
            const x1 = getX(seg.colStart),
              x2 = getX(seg.colEnd);

            // Build per-column path points from renderCols
            let pts = [];
            let edge3x, edge5x;
            const hasRenderCols = seg.renderCols && seg.renderCols.length > 0;
            if (hasRenderCols) {
              // Per-column zigzag path
              const cols = isFwd ? seg.renderCols : [...seg.renderCols].reverse();
              const firstCol = cols[0],
                lastCol = cols[cols.length - 1];
              edge5x = isFwd
                ? getX(firstCol.templateCol % charsPerLine) // fwd: left edge of leftmost
                : getX(firstCol.templateCol % charsPerLine) + cw; // rev: right edge of rightmost
              edge3x = isFwd
                ? getX(lastCol.templateCol % charsPerLine) + cw // fwd: right edge of rightmost
                : getX(lastCol.templateCol % charsPerLine); // rev: left edge of leftmost
              // 5' tail
              if (isTail && hasMis && drawMisLen > 0) {
                if (isFwd) pts.push([x1 - drawMisLen * cw, misY], [x1 - cw / 2, misY]);
                else pts.push([getX(seg.colEnd + drawMisLen + 1), misY], [x2 + cw * 1.5, misY]);
              }
              pts.push([edge5x, cols[0].kind === 'match' ? matchY : misY]);
              for (const rc of cols) {
                const cx = getX(rc.templateCol % charsPerLine) + cw / 2;
                const cy = rc.kind === 'match' ? matchY : misY;
                pts.push([cx, cy]);
              }
              pts.push([edge3x, cols[cols.length - 1].kind === 'match' ? matchY : misY]);
            } else {
              // Fallback: straight line
              if (isTail && hasMis && drawMisLen > 0) {
                if (isFwd) pts.push([x1 - drawMisLen * cw, misY], [x1 - cw / 2, misY]);
                else pts.push([getX(seg.colEnd + drawMisLen + 1), misY], [x2 + cw * 1.5, misY]);
              }
              if (isFwd) pts.push([x1 + cw / 2, matchY], [x2 + cw, matchY]);
              else pts.push([x2 + cw, matchY], [x1, matchY]);
            }
            if (pts.length < 2) return null;

            const isSelectedPrimer =
              selectedPrimerIds.includes(p.id) &&
              (selectionMode === 'primer' || selectionMode === 'amplimer');
            const isDimDuringDrag =
              isPrimerDragging &&
              primerDimActive &&
              !isSelectedPrimer &&
              p.isFwd === primerDragRef.current?.startFwd;

            const pathStr = `M ${pts.map((p) => `${p[0]} ${p[1]}`).join(' L ')}`;
            const expD = isFwd ? -1 : 1;
            const curExp = isHovered || isSelectedPrimer ? pp.hoverExpand : 0;
            const last = pts[pts.length - 1];
            const hoverPath =
              pathStr +
              ` L ${last[0]} ${last[1] + expD * curExp} ` +
              [...pts]
                .reverse()
                .map((p) => `L ${p[0]} ${p[1] + expD * curExp}`)
                .join(' ') +
              ' Z';

            const arrowTipY =
              hasRenderCols && seg.renderCols.length > 0
                ? seg.renderCols[seg.renderCols.length - 1].kind === 'match'
                  ? matchY
                  : misY
                : matchY;
            const arrowBaseX = hasRenderCols ? edge3x : isFwd ? x2 + cw : x1;
            const arrowPath = isArrow
              ? `M ${arrowBaseX} ${arrowTipY} L ${isFwd ? arrowBaseX - pp.arrowHeadLen : arrowBaseX + pp.arrowHeadLen} ${arrowTipY + expD * pp.arrowHeadHeight}`
              : '';

            const visMis = hasMis && drawMisLen > 0 ? p.mismatchStr.slice(misLen - drawMisLen) : '';
            const labelOff = isSelectedPrimer ? (isFwd ? -16 : 13) : 0;

            return (
              <g
                key={`${seg.row}-${seg.colStart}`}
                opacity={isDimDuringDrag ? 0.2 : undefined}
                style={isDimDuringDrag ? { pointerEvents: 'none' } : undefined}
              >
                <path
                  d={hoverPath}
                  fill={isSelectedPrimer ? pColor : bgColor}
                  style={{ transition: springAnim }}
                />
                {!isSelectedPrimer && (
                  <path
                    d={hoverPath}
                    fill={pColor}
                    fillOpacity={0.1}
                    style={{ transition: springAnim }}
                  />
                )}

                <text
                  fill={isSelectedPrimer ? bgColor : pColor}
                  fontSize="14px"
                  fontFamily={monoFont}
                  fontWeight="bold"
                  style={{
                    opacity: isHovered || isSelectedPrimer ? 1 : 0,
                    transition: 'opacity 0.2s ease-in-out',
                    pointerEvents: 'none',
                  }}
                >
                  {/* 5' tail */}
                  {isTail && hasMis && drawMisLen > 0 && (
                    <>
                      {showMisDots && (
                        <tspan
                          x={
                            isFwd
                              ? getX(seg.colStart - drawMisLen - 1.5)
                              : getX(seg.colEnd + drawMisLen + 2.5)
                          }
                          y={misY + (isFwd ? -pp.fwdBaseTextY : pp.revBaseTextY)}
                          textAnchor="middle"
                        >
                          ···
                        </tspan>
                      )}
                      {visMis.split('').map((c, k) => (
                        <tspan
                          key={`mis-${k}`}
                          x={
                            (isFwd
                              ? getX(seg.colStart - drawMisLen + k)
                              : getX(seg.colEnd + drawMisLen - k)) +
                            cw / 2
                          }
                          y={misY + (isFwd ? -pp.fwdBaseTextY : pp.revBaseTextY)}
                          textAnchor="middle"
                        >
                          {c}
                        </tspan>
                      ))}
                    </>
                  )}
                  {/* Per-column alignment rendering */}
                  {seg.renderCols &&
                    seg.renderCols.map((rc) => {
                      const isOffset =
                        rc.kind === 'mismatch' || rc.kind === 'gap' || rc.kind === 'insertion';
                      const y =
                        (isOffset ? misY : matchY) + (isFwd ? -pp.fwdBaseTextY : pp.revBaseTextY);
                      const x = getX(rc.templateCol % charsPerLine) + cw / 2;
                      const isGap = rc.kind === 'gap';
                      const isIns = rc.kind === 'insertion';
                      return (
                        <tspan
                          key={`aln-${rc.templateCol}`}
                          x={x}
                          y={y}
                          textAnchor="middle"
                          fill={isGap ? '#9ca3af' : undefined}
                          fontWeight={isGap ? '200' : undefined}
                          fontSize={isIns ? '10px' : undefined}
                        >
                          {isIns ? rc.insDetail?.insertedBases || rc.primerBase : rc.primerBase}
                        </tspan>
                      );
                    })}
                  {/* 3' tail */}
                  {isArrow &&
                    p.threePrimeTail &&
                    (() => {
                      const tail3 = p.threePrimeTail;
                      const tailLen = tail3.length;
                      return tail3.split('').map((c, k) => (
                        <tspan
                          key={`3t-${k}`}
                          x={
                            (isFwd ? getX(seg.colEnd + k + 1) : getX(seg.colStart - tailLen + k)) +
                            cw / 2
                          }
                          y={misY + (isFwd ? -pp.fwdBaseTextY : pp.revBaseTextY)}
                          textAnchor="middle"
                        >
                          {c}
                        </tspan>
                      ));
                    })()}
                </text>

                <path
                  d={pathStr}
                  fill="none"
                  stroke={bgColor}
                  strokeWidth="6"
                  strokeLinejoin="round"
                />
                {isArrow && (
                  <path
                    d={arrowPath}
                    fill="none"
                    stroke={bgColor}
                    strokeWidth="6"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  />
                )}
                <path d={pathStr} fill="none" stroke={pColor} strokeWidth="2.5" />
                {isArrow && (
                  <path
                    d={arrowPath}
                    fill="none"
                    stroke={pColor}
                    strokeWidth="2.5"
                    strokeLinecap="round"
                  />
                )}

                <text
                  x={pts[0][0]}
                  y={pts[0][1] + (isFwd ? -pp.fwdLabelY : pp.revLabelY) + labelOff}
                  fontSize="12px"
                  fontFamily="TeX Gyre Heros"
                  fontWeight="600"
                  fontStyle="italic"
                  textAnchor={isFwd ? 'start' : 'end'}
                  fill="none"
                  stroke={bgColor}
                  strokeWidth="5"
                  style={{
                    opacity: isHovered && !isSelectedPrimer ? 0 : 1,
                    transition: springAnim,
                    pointerEvents: 'none',
                  }}
                >
                  {p.name}
                </text>
                <text
                  x={pts[0][0]}
                  y={pts[0][1] + (isFwd ? -pp.fwdLabelY : pp.revLabelY) + labelOff}
                  fontSize="12px"
                  fontFamily="TeX Gyre Heros"
                  fontWeight="600"
                  fontStyle="italic"
                  textAnchor={isFwd ? 'start' : 'end'}
                  fill={pColor}
                  stroke="none"
                  style={{
                    opacity: isHovered && !isSelectedPrimer ? 0 : 1,
                    transition: springAnim,
                    pointerEvents: 'none',
                  }}
                >
                  {p.name}
                </text>

                <path d={pathStr} fill="none" stroke="transparent" strokeWidth="20" />
                <rect
                  x={Math.min(...pts.map((p) => p[0])) - 4}
                  y={Math.min(...pts.map((p) => p[1])) - 20}
                  width={Math.max(...pts.map((p) => p[0])) - Math.min(...pts.map((p) => p[0])) + 8}
                  height={
                    Math.max(...pts.map((p) => p[1])) - Math.min(...pts.map((p) => p[1])) + 40
                  }
                  fill="transparent"
                  onMouseDown={(e) => {
                    if (e.button !== 0) return;
                    e.stopPropagation();
                    e.preventDefault();
                    // Clear all other selections
                    setSelStart(null);
                    setSelEnd(null);
                    setCursorIndex(null);
                    setIsEnzymeSelection(false);
                    setSelectedEnzymeIds([]);
                    lastEnzymeSelRef.current = null;
                    setTranslationSel(null);
                    translationDragRef.current = null;
                    setIsTranslationDragging(false);
                    // Start primer drag
                    setSelectionMode('primer');
                    setSelectedPrimerIds([p.id]);
                    setIsPrimerDragging(true);
                    isPrimerDraggingRef.current = true;
                    setHoveredPrimer(p.id);
                    primerDragRef.current = {
                      startPrimerId: p.id,
                      startFwd: p.isFwd,
                      didDrag: false,
                      hoveredPrimerId: p.id,
                    };
                    // Delay dimming other primers by 500ms to prevent flash on click
                    if (primerDimTimerRef.current) clearTimeout(primerDimTimerRef.current);
                    primerDimTimerRef.current = setTimeout(() => setPrimerDimActive(true), 500);
                    clearCursorTimer();
                  }}
                  onMouseEnter={() => {
                    if (isPrimerDraggingRef.current) {
                      if (primerDragRef.current) {
                        primerDragRef.current.hoveredPrimerId = p.id;
                      }
                      setHoveredPrimer(p.id);
                      return;
                    }
                    if (isDraggingRef.current) return;
                    setHoveredPrimer(p.id);
                  }}
                  onMouseLeave={() => setHoveredPrimer(null)}
                  onDoubleClick={(e) => {
                    e.stopPropagation();
                    // Prevent drag activation on double-click
                    setPrimerDimActive(false);
                    if (primerDimTimerRef.current) clearTimeout(primerDimTimerRef.current);
                    setCreatePrimerSeq(null); // clear create mode
                    setPrimerAlignmentPrimer(enrichedPrimers.find((ep) => ep.id === p.id) || null);
                  }}
                  className="cursor-pointer"
                />
              </g>
            );
          })}
        </g>
      );
    });
  }, [
    visiblePrimers,
    hoveredPrimer,
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
  ]);

  // Pre-compute enzyme geometry — one entry per cut pair (cut-twice enzymes get 2 entries)
  const enzymeLayout = useMemo(() => {
    const entries = [];
    for (const e of visibleEnzymes) {
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      pairs.forEach((cp, pi) => {
        const row = Math.floor(cp.topCutIndex / charsPerLine);
        const cutX = getX(cp.topCutIndex % charsPerLine);
        const sy = getSeqY(row);
        const enzTrack =
          (enzymeRowTracks[pairs.length > 1 ? `${e.id}_p${pi}` : e.id] || {})[row] || 0;

        const enzW = enzLabelW(e.name, e.isUnique);
        // Pre-compute exact label width (italic + normal parts)
        const s = splitEnzName(e.name);
        const baseFont = `${e.isUnique ? '700' : '350'} 14px ${monoFont}`;
        let exactW;
        if (s.normal) {
          exactW = measureWidth(s.italic, `italic ${baseFont}`) + measureWidth(s.normal, baseFont);
        } else {
          exactW = measureWidth(e.name, baseFont);
        }

        // Clamp label top so it never overlaps the sequence text of the row above
        const minTop = row > 0 ? getSeqY(row - 1) + 8 : -Infinity;
        const yTop = Math.max(sy - lp.enzLabelBase - enzTrack * lp.enzTrackHeight, minTop);
        entries.push({
          id: `${e.id}_p${pi}`,
          groupId: e.id,
          pairIndex: pi,
          name: e.name,
          cutX,
          sy,
          row,
          yTop,
          isUnique: e.isUnique,
          enzW,
          exactW,
          topCutIndex: cp.topCutIndex,
          botCutIndex: cp.botCutIndex,
        });
      });
    }
    return entries;
  }, [visibleEnzymes, enzymeRowTracks, lp, charsPerLine, getSeqY]);

  // Batched enzyme lines
  const enzymeLinesPath = useMemo(() => {
    let d = '';
    for (const l of enzymeLayout) {
      const yBot = l.sy - lp.enzLineGap;
      d += `M${l.cutX} ${l.yTop} L${l.cutX} ${yBot}`;
    }
    return d;
  }, [enzymeLayout, lp.enzLineGap]);

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

  const renderedEnzymes = useMemo(() => {
    if (!enzymeLayout.length) return null;
    return (
      <g>
        <path
          d={enzymeLinesPath}
          fill="none"
          stroke="#333"
          strokeWidth="0.8"
          style={{ pointerEvents: 'none' }}
        />
        {enzymeLayout
          .filter((l) => l.isUnique)
          .map((l) => (
            <line
              key={`u-${l.id}`}
              x1={l.cutX}
              x2={l.cutX}
              y1={l.yTop}
              y2={l.sy - lp.enzLineGap}
              stroke="#333"
              strokeWidth="1"
              style={{ pointerEvents: 'none' }}
            />
          ))}
      </g>
    );
  }, [enzymeLayout, enzymeLinesPath, lp.enzLineGap]);

  const totalNameCounts = useMemo(() => {
    const m = new Map();
    for (const e of enzymes) {
      const nPairs = (e.cutPairs && e.cutPairs.length) || 1;
      m.set(e.name, (m.get(e.name) || 0) + nPairs);
    }
    return m;
  }, [enzymes]);

  const renderedEnzymeLabels = useMemo(() => {
    const hoveredName = hoveredEnzyme
      ? enzymeLayout.find((l) => l.id === hoveredEnzyme)?.name
      : null;
    return enzymeLayout.map((l) => {
      const e = enzymes.find((x) => x.id === l.groupId);
      const isGray =
        e && (e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length));
      const isHoveredGroup = hoveredName != null && l.name === hoveredName;
      const isSelected = selectedEnzymeIds.includes(l.id);
      const isBlunt = e && e.cutType === 'blunt';
      const isIIS = isIISEnzyme(e);
      const showTwo = totalNameCounts.get(l.name) === 2;

      // Color: selected (dark blue) > hovered (blue) > type-specific > default
      let labelColor = '#333';
      if (isGray) labelColor = '#9CA3AF';
      else if (isSelected) labelColor = enzymeActiveBlue;
      else if (isHoveredGroup) labelColor = '#2563EB';
      else if (isBlunt) labelColor = '#6B3A2A';
      else if (isIIS) labelColor = '#0D6B6B';

      return (
        <g
          key={l.id}
          onMouseEnter={() => {
            if (enzymeDragRef.current?.active) {
              // Regular enzyme can't drag to cut-twice enzyme
              const srcEnz = enzymes.find((x) => x.id === enzymeDragRef.current.startEnzymeId);
              if (srcEnz && !(srcEnz.cutPairs?.length > 1) && e?.cutPairs?.length > 1) return;
              // During drag: update selection between start and target
              enzymeDragRef.current.hoveredId = l.id;
              enzymeDragRef.current.didDrag = true;
              setHoveredEnzyme(l.id);
              const startIdx = enzymeDragRef.current.startCutIdx;
              const targetIdx = l.topCutIndex;
              if (targetIdx !== startIdx) {
                const s = Math.min(startIdx, targetIdx);
                const e = Math.max(startIdx, targetIdx) - 1;
                if (s <= e) {
                  setSelStart(s);
                  setSelEnd(e);
                  setCursorIndex(null);
                }
                setSelectedEnzymeIds([enzymeDragRef.current.entryId, l.id]);
                enzymeDragRef.current.backToStart = false;
              } else {
                const rs = enzymeDragRef.current.recStart;
                const re = enzymeDragRef.current.recEnd;
                if (rs != null && re != null) {
                  setSelStart(rs);
                  setSelEnd(re);
                }
                setSelectedEnzymeIds([l.id]);
                enzymeDragRef.current.backToStart = true;
              }
            } else if (!isDraggingRef.current) {
              setHoveredEnzyme(l.id);
            }
          }}
          onMouseLeave={() => {
            if (enzymeDragRef.current?.active && enzymeDragRef.current.hoveredId === l.id) {
              enzymeDragRef.current.hoveredId = null;
            }
            setHoveredEnzyme(null);
          }}
          onMouseDown={(e) => {
            if (e.button !== 0) return;
            e.stopPropagation();
            e.preventDefault();
            // Clear primer selection
            setSelectedPrimerIds([]);
            isPrimerDraggingRef.current = false;
            primerDragRef.current = null;
            if (primerDimTimerRef.current) {
              clearTimeout(primerDimTimerRef.current);
              primerDimTimerRef.current = null;
            }
            setPrimerDimActive(false);
            // Clear translation selection
            setTranslationSel(null);
            translationDragRef.current = null;
            setIsTranslationDragging(false);
            const enzyme = enzymes.find((x) => x.id === l.groupId);
            if (!enzyme) return;
            const pairs = enzyme.cutPairs || [
              { topCutIndex: enzyme.cutIndex, botCutIndex: enzyme.botCutIndex },
            ];
            const isCutTwice = pairs.length > 1;
            const cutIdx = l.topCutIndex;

            // Shift+click: extend from previous enzyme selection
            if (
              e.shiftKey &&
              lastEnzymeSelRef.current &&
              lastEnzymeSelRef.current.cutIdx !== cutIdx
            ) {
              const prevCutIdx = lastEnzymeSelRef.current.cutIdx;
              const prevEntryId = lastEnzymeSelRef.current.entryId;
              const s = Math.min(prevCutIdx, cutIdx);
              const ed = Math.max(prevCutIdx, cutIdx) - 1;
              if (s <= ed) {
                setSelStart(s);
                setSelEnd(ed);
                setCursorIndex(null);
                setIsEnzymeSelection(true);
                setSelectedEnzymeIds(prevEntryId ? [prevEntryId, l.id] : [l.id]);
                setHoveredEnzyme(null);
                clearCursorTimer();
                lastEnzymeSelRef.current = {
                  enzymeId: l.groupId,
                  cutIdx,
                  name: l.name,
                  entryId: l.id,
                };
              }
              return;
            }

            // Cut-twice enzyme: directly select between two cut positions, close tooltip
            if (isCutTwice) {
              const otherPair = pairs[l.pairIndex === 0 ? 1 : 0];
              const cut1 = cutIdx;
              const cut2 = otherPair.topCutIndex;
              const s = Math.min(cut1, cut2);
              const ed = Math.max(cut1, cut2) - 1;
              const otherEntryId = `${l.groupId}_p${l.pairIndex === 0 ? 1 : 0}`;
              setSelStart(s);
              setSelEnd(ed);
              setCursorIndex(null);
              setIsEnzymeSelection(true);
              setSelectedEnzymeIds([l.id, otherEntryId]);
              setHoveredEnzyme(null);
              clearCursorTimer();
              lastEnzymeSelRef.current = {
                enzymeId: l.groupId,
                cutIdx,
                name: l.name,
                entryId: l.id,
              };
              return;
            }

            // Start enzyme drag — immediately select recognition site range
            setSelStart(enzyme.displayStart);
            setSelEnd(enzyme.displayEnd);
            setCursorIndex(null);
            enzymeDragRef.current = {
              active: true,
              startEnzymeId: l.groupId,
              startName: l.name,
              startCutIdx: cutIdx,
              recStart: enzyme.displayStart,
              recEnd: enzyme.displayEnd,
              didDrag: false,
              backToStart: false,
              hoveredId: l.id,
              entryId: l.id,
            };
            isDraggingRef.current = true;
            setIsDragging(true);
            setIsEnzymeDragging(true);
            setIsEnzymeSelection(true);
            setSelectedEnzymeIds([l.id]);
            setHoveredEnzyme(l.id);
            clearCursorTimer();
          }}
          style={{ cursor: 'pointer' }}
        >
          <rect
            x={l.cutX + 3}
            y={l.yTop - 10}
            width={l.enzW + (showTwo ? 10 : 2)}
            height={18}
            fill="transparent"
          />
          {(() => {
            const enzText = {
              x: l.cutX + 6,
              y: l.yTop + 5,
              fontSize: '14px',
              fontFamily: monoFont,
              fontWeight: l.isUnique ? '700' : '350',
              style: { pointerEvents: 'none' },
            };
            const nameContent = (() => {
              const s = splitEnzName(l.name);
              return s.normal
                ? [
                    <tspan key="i" fontStyle="italic">
                      {s.italic}
                    </tspan>,
                    <tspan key="n">{s.normal}</tspan>,
                  ]
                : l.name;
            })();
            const content = showTwo
              ? [
                  ...(Array.isArray(nameContent) ? nameContent : [nameContent]),
                  <tspan key="two" fontSize="12" dy="-2">
                    ²
                  </tspan>,
                ]
              : nameContent;
            return (
              <>
                <text {...enzText} fill="none" stroke={bgColor} strokeWidth="5">
                  {content}
                </text>
                <text {...enzText} fill={labelColor} stroke="none">
                  {content}
                </text>
              </>
            );
          })()}
        </g>
      );
    });
  }, [
    enzymeLayout,
    enzymes,
    hoveredEnzyme,
    bgColor,
    selectedEnzymeIds,
    isEnzymeSelection,
    clearCursorTimer,
  ]);

  const renderedEnzymeOverlay = useMemo(() => {
    // Collect enzyme names to render lines for (from hover or selected ids)
    const namesToRender = new Set();
    if (hoveredEnzyme) {
      const entry = enzymeLayout.find((l) => l.id === hoveredEnzyme);
      if (entry) namesToRender.add(entry.name);
    }
    for (const id of selectedEnzymeIds) {
      const entry = enzymeLayout.find((l) => l.id === id);
      if (entry) namesToRender.add(entry.name);
    }
    if (namesToRender.size === 0) return null;

    // Compute hover text content (only for hovered enzyme)
    let hoverTextContent = null;
    if (hoveredEnzyme) {
      const hoveredEntry = enzymeLayout.find((l) => l.id === hoveredEnzyme);
      if (hoveredEntry) {
        const e = enzymes.find((x) => x.name === hoveredEntry.name);
        if (e) {
          const isGray =
            e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length);
          const isSelOv = selectedEnzymeIds.includes(hoveredEntry.id);
          const ovColor = isGray ? '#9CA3AF' : isSelOv ? enzymeActiveBlue : '#2563EB';
          const showTwoOv = totalNameCounts.get(hoveredEntry.name) === 2;
          const ovNameContent = (() => {
            const s = splitEnzName(e.name);
            const parts = s.normal
              ? [
                  <tspan key="i" fontStyle="italic">
                    {s.italic}
                  </tspan>,
                  <tspan key="n">{s.normal}</tspan>,
                ]
              : [e.name];
            if (showTwoOv)
              parts.push(
                <tspan key="two" fontSize="12" dy="-2">
                  ²
                </tspan>,
              );
            return parts;
          })();
          const methParts = [];
          if (e.methylationBlocked && e.methylationSources?.length) {
            methParts.push('[' + e.methylationSources.join('/') + ' Blocked]');
          }
          if (e.methylationRequired && e.methylRequiredSources?.length) {
            methParts.push('[' + e.methylRequiredSources.join('/') + ' Required]');
          }
          const methText = methParts.length ? '  ' + methParts.join(' ') : '';
          hoverTextContent = { hoveredEntry, ovColor, ovNameContent, methText };
        }
      }
    }

    return (
      <g style={{ pointerEvents: 'none' }}>
        {/* Render lines for each enzyme name */}
        {[...namesToRender].map((name) => {
          const e = enzymes.find((x) => x.name === name);
          if (!e) return null;
          const isGray =
            e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length);
          const hoveredName = hoveredEnzyme
            ? enzymeLayout.find((l) => l.id === hoveredEnzyme)?.name
            : null;
          let nameEntries = enzymeLayout.filter((l) => l.name === name);
          // In selection mode, only filter non-hovered entries (selected + hovered lines both show)
          if (isEnzymeSelection && name !== hoveredName) {
            nameEntries = nameEntries.filter((l) => selectedEnzymeIds.includes(l.id));
          }
          return nameEntries.map((l) => {
            const isSel = selectedEnzymeIds.includes(l.id);
            const lineColor = isGray ? '#9CA3AF' : isSel ? enzymeActiveBlue : '#2563EB';
            return (
              <React.Fragment key={`ov-${l.id}`}>
                <line
                  x1={l.cutX}
                  x2={l.cutX}
                  y1={l.yTop}
                  y2={l.sy + 5}
                  stroke={bgColor}
                  strokeWidth="4"
                  strokeLinecap="square"
                />
                <line
                  x1={l.cutX}
                  x2={l.cutX}
                  y1={l.yTop}
                  y2={l.sy + 5}
                  stroke={lineColor}
                  strokeWidth={e.isUnique ? '2' : '1'}
                />
              </React.Fragment>
            );
          });
        })}
        {/* Hover text */}
        {hoverTextContent && (
          <React.Fragment>
            <text
              x={hoverTextContent.hoveredEntry.cutX + 6}
              y={hoverTextContent.hoveredEntry.yTop + 5}
              fill="none"
              stroke={bgColor}
              strokeWidth="5"
              fontSize="14px"
              fontFamily={monoFont}
              fontWeight={hoverTextContent.hoveredEntry.isUnique ? '700' : '350'}
            >
              {hoverTextContent.ovNameContent}
              {hoverTextContent.methText}
            </text>
            <text
              x={hoverTextContent.hoveredEntry.cutX + 6}
              y={hoverTextContent.hoveredEntry.yTop + 5}
              fill={hoverTextContent.ovColor}
              stroke="none"
              fontSize="14px"
              fontFamily={monoFont}
              fontWeight={hoverTextContent.hoveredEntry.isUnique ? '700' : '350'}
            >
              {hoverTextContent.ovNameContent}
              {hoverTextContent.methText}
            </text>
          </React.Fragment>
        )}
      </g>
    );
  }, [
    hoveredEnzyme,
    selectedEnzymeIds,
    isEnzymeSelection,
    enzymeLayout,
    enzymes,
    bgColor,
    totalNameCounts,
  ]);

  const renderedTooltips = useMemo(() => {
    if (!hoveredEnzyme || isEnzymeDragging) return null;
    const hoveredEntry = enzymeLayout.find((l) => l.id === hoveredEnzyme);
    if (!hoveredEntry) return null;
    const e = enzymes.find((x) => x.id === hoveredEntry.groupId);
    if (!e || e.displayStart === undefined) return null;
    const isGray =
      e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length);
    const ttColor = isGray ? '#9CA3AF' : isEnzymeDragging ? enzymeActiveBlue : '#2563EB';

    const tlen = cleanSeq.length;
    const dispLen = e.displayEnd - e.displayStart + 1;
    const sw = e.isUnique ? '2' : '1';
    const pad = 6;
    const ttH = 44;
    const cutPairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
    // Circular display window may wrap the origin (displayEnd >= tlen).
    const sub =
      e.displayEnd < tlen
        ? cleanSeq.substring(e.displayStart, e.displayEnd + 1)
        : Array.from({ length: dispLen }, (_, i) => cleanSeq[(e.displayStart + i) % tlen]).join('');
    const comp = sub.split('').map(complement).join('');
    const pattern = e.recSeqPattern || '';
    const recOffset = e.recStart - e.displayStart;
    const recLen = e.recEnd - e.recStart + 1;
    // Position relative to displayStart, in window coordinates (handles wrap).
    const relPos = (idx) => (((idx - e.displayStart) % tlen) + tlen) % tlen;
    const isRecBold = (i) => {
      if (i < recOffset || i >= recOffset + recLen) return false;
      const pi = i - recOffset;
      return pi < pattern.length && pattern[pi] !== 'N' && pattern[pi] !== 'n';
    };

    const groupEntries = enzymeLayout.filter((l) => l.groupId === hoveredEntry.groupId);

    return (
      <g style={{ pointerEvents: 'none' }}>
        {groupEntries.map((entry) => {
          const sy = entry.sy;
          const ttY = sy - 19;
          const hp = cutPairs[entry.pairIndex] || cutPairs[0];
          const charsBeforeCut = relPos(hp.topCutIndex);
          const baseX = entry.cutX - charsBeforeCut * cw;
          const leftX = baseX - pad;
          const ttW = dispLen * cw + pad * 2;

          const polyEntries = cutPairs.map((cp, i) => {
            const tGapX = baseX + relPos(cp.topCutIndex) * cw;
            const bGapX = baseX + relPos(cp.botCutIndex) * cw;
            const isLocal = i === entry.pairIndex;
            return {
              tGapX,
              bGapX,
              isLocal,
              path: [
                `M ${tGapX} ${ttY - 2}`,
                `L ${tGapX} ${sy + 3}`,
                `L ${bGapX} ${sy + 3}`,
                `L ${bGapX} ${sy + 19}`,
              ].join(' '),
            };
          });

          const uniqueGapXs = [...new Set(polyEntries.map((pe) => pe.tGapX))].sort((a, b) => a - b);
          const gapHalfW = 4;
          const r = 8;
          let borderD = `M ${leftX + r} ${ttY}`;
          let curX = leftX + r;
          for (const gx of uniqueGapXs) {
            if (gx - gapHalfW > curX) {
              borderD += ` L ${gx - gapHalfW} ${ttY}`;
            }
            borderD += ` M ${gx + gapHalfW} ${ttY}`;
            curX = gx + gapHalfW;
          }
          if (curX < leftX + ttW - r) {
            borderD += ` L ${leftX + ttW - r} ${ttY}`;
          }
          borderD += ` A ${r} ${r} 0 0 1 ${leftX + ttW} ${ttY + r}`;
          borderD += ` L ${leftX + ttW} ${ttY + ttH - r}`;
          borderD += ` A ${r} ${r} 0 0 1 ${leftX + ttW - r} ${ttY + ttH}`;
          borderD += ` L ${leftX + r} ${ttY + ttH}`;
          borderD += ` A ${r} ${r} 0 0 1 ${leftX} ${ttY + ttH - r}`;
          borderD += ` L ${leftX} ${ttY + r}`;
          borderD += ` A ${r} ${r} 0 0 1 ${leftX + r} ${ttY}`;

          return (
            <g key={`tt-${entry.id}`}>
              <rect
                x={leftX}
                y={ttY}
                width={ttW}
                height={ttH}
                rx={8}
                fill="#FFFFFF"
                stroke="none"
              />
              <path
                d={borderD}
                fill="none"
                stroke={ttColor}
                strokeWidth={sw}
                strokeLinejoin="round"
              />
              {polyEntries.map((pe, i) => (
                <path
                  key={`poly-${i}`}
                  d={pe.path}
                  fill="none"
                  stroke={ttColor}
                  strokeWidth={sw}
                  strokeLinejoin="round"
                  strokeLinecap="round"
                />
              ))}
              <text y={sy} fontFamily={monoFont} fontSize="14px">
                {sub.split('').map((c, i) => {
                  const bold = isRecBold(i);
                  return (
                    <tspan
                      key={i}
                      x={baseX + i * cw + cw / 2}
                      textAnchor="middle"
                      fontWeight={bold ? '700' : '200'}
                      fill={bold ? '#1f2937' : '#BFBFBF'}
                    >
                      {c}
                    </tspan>
                  );
                })}
              </text>
              <text y={sy + 16} fontFamily={monoFont} fontSize="14px">
                {comp.split('').map((c, i) => {
                  const bold = isRecBold(i);
                  return (
                    <tspan
                      key={i}
                      x={baseX + i * cw + cw / 2}
                      textAnchor="middle"
                      fontWeight={bold ? '700' : '200'}
                      fill={bold ? '#1f2937' : '#BFBFBF'}
                    >
                      {c}
                    </tspan>
                  );
                })}
              </text>
            </g>
          );
        })}
      </g>
    );
  }, [hoveredEnzyme, enzymeLayout, enzymes, cleanSeq, charsPerLine, isEnzymeDragging]);

  // --- cursor & selection renderers ---
  // --- amplimer intervening region (deep green) ---
  // Rendered after SeqBg + SeqSel so white text overrides dark text
  const renderedAmplimerRegion = useMemo(() => {
    if (selectionMode !== 'amplimer' || selectedPrimerIds.length !== 2) return null;
    const fp = enrichedPrimers.find((p) => p.id === selectedPrimerIds[0]);
    const rp = enrichedPrimers.find((p) => p.id === selectedPrimerIds[1]);
    const fwdPrimer = fp && fp.isFwd ? fp : rp;
    const revPrimer = fp && !fp.isFwd ? fp : rp;
    if (!fwdPrimer || !revPrimer) return null;

    const segsByRange = (s, e) => {
      if (s > e || s >= cleanSeq.length || e < 0) return [];
      return sp(s, e);
    };

    let ranges;
    if (fwdPrimer.matchEnd >= revPrimer.matchStart) {
      // Circular: wrap from fwd end+1 to end of seq, then 0 to rev start-1
      ranges = [
        segsByRange(fwdPrimer.matchEnd + 1, cleanSeq.length - 1),
        revPrimer.matchStart > 0 ? segsByRange(0, revPrimer.matchStart - 1) : [],
      ];
    } else {
      const s = fwdPrimer.matchEnd + 1;
      const e = revPrimer.matchStart - 1;
      ranges = [s <= e ? segsByRange(s, e) : []];
    }

    const allSegs = ranges.flat();
    if (!allSegs.length) return null;

    return (
      <g style={{ pointerEvents: 'none' }}>
        {allSegs.map((seg) => (
          <rect
            key={`amp-${seg.row}-${seg.colStart}`}
            x={getX(seg.colStart)}
            y={getSeqY(seg.row) - 19}
            width={(seg.colEnd - seg.colStart + 1) * cw}
            height={28}
            fill={amplimerGreen}
            rx="1"
          />
        ))}
        {allSegs.map((seg) => {
          const rowStart = seg.row * charsPerLine;
          const chars = cleanSeq
            .substring(rowStart + seg.colStart, rowStart + seg.colEnd + 1)
            .split('');
          return (
            <text
              key={`amp-txt-${seg.row}-${seg.colStart}`}
              y={getSeqY(seg.row)}
              fontFamily={monoFont}
              fontSize="14px"
              fontWeight="bold"
              style={{ userSelect: 'none', pointerEvents: 'none' }}
            >
              {chars.map((c, i) => (
                <tspan
                  key={i}
                  x={getX(seg.colStart + i) + cw / 2}
                  textAnchor="middle"
                  fill={bgColor}
                >
                  {c}
                </tspan>
              ))}
            </text>
          );
        })}
      </g>
    );
  }, [selectionMode, selectedPrimerIds, enrichedPrimers, cleanSeq, charsPerLine, getSeqY, sp]);

  const renderedCursor = useMemo(() => {
    if (cursorIndex === null) return null;
    if (hasSelection && !isDragging) return null;
    if (selectionMode !== 'text' && selectionMode !== 'none') return null;
    const row = Math.floor(cursorIndex / charsPerLine);
    const col = cursorIndex % charsPerLine;
    const x = getX(col);
    const sy = getSeqY(row);
    const topY = sy - rowAbove[row];
    const botY =
      row === numRows - 1 ? sy + rowBelow[row] + 24 : getSeqY(row + 1) - rowAbove[row + 1];
    return (
      <g style={{ pointerEvents: 'none' }}>
        <line x1={x} x2={x} y1={topY} y2={botY} stroke={bgColor} strokeWidth="3" />
        <line x1={x} x2={x} y1={topY} y2={botY} stroke={currentSelColor} strokeWidth="1.5" />
      </g>
    );
  }, [
    cursorIndex,
    hasSelection,
    isDragging,
    charsPerLine,
    numRows,
    getSeqY,
    rowBelow,
    selectionMode,
  ]);

  const renderedHoverIndex = useMemo(() => {
    if (hoveredIndex === null || isDragging || isTranslationDragging) return null;
    const row = Math.floor(hoveredIndex / charsPerLine);
    const col = hoveredIndex % charsPerLine;
    const sy = getSeqY(row);
    return (
      <g style={{ pointerEvents: 'none' }}>
        <text
          x={getX(col) + cw / 2}
          y={sy - 22}
          fontFamily={monoFont}
          fontSize="9px"
          fontWeight="600"
          fill="#A8A29E"
          stroke={bgColor}
          strokeWidth="2"
          strokeLinejoin="round"
          paintOrder="stroke"
          textAnchor="middle"
          style={{ pointerEvents: 'none', userSelect: 'none' }}
        >
          {hoveredIndex + 1}
        </text>
      </g>
    );
  }, [hoveredIndex, isDragging, isTranslationDragging, charsPerLine, getSeqY, currentSelColor]);

  const renderedSelectionInfo = useMemo(() => {
    if (!isDragging || !hasSelection || cursorIndex === null) return null;
    if (selectionMode !== 'text') return null;
    const len = selEnd - selStart + 1;
    const tm = selectionTm;
    const showTm = tm !== null && tm >= 40 && tm <= 75;
    const row = Math.floor(cursorIndex / charsPerLine);
    const col = cursorIndex % charsPerLine;
    const sy = getSeqY(row);
    const botY =
      row === numRows - 1 ? sy + rowBelow[row] + 24 : getSeqY(row + 1) - rowAbove[row + 1];
    const x = getX(col);
    const fontSize = '11px';
    const fontStr = `600 ${fontSize} ${monoFont}`;
    let label = `${len} bp`;
    if (showTm) label += `, ${tm}°C`;
    const tw = measureWidth(label, fontStr);
    return (
      <g style={{ pointerEvents: 'none' }}>
        <text
          dominantBaseline="text-after-edge"
          x={x - 8 - tw}
          y={botY}
          fontFamily={monoFont}
          fontSize={fontSize}
          fontWeight="600"
          fill={currentSelColor}
          stroke={bgColor}
          strokeWidth="2.5"
          strokeLinejoin="round"
          paintOrder="stroke"
          style={{ userSelect: 'none', whiteSpace: 'nowrap' }}
        >
          {label}
        </text>
      </g>
    );
  }, [
    isDragging,
    hasSelection,
    cursorIndex,
    selStart,
    selEnd,
    cleanSeq,
    charsPerLine,
    getSeqY,
    numRows,
    rowBelow,
    rowAbove,
    currentSelColor,
    selectionTm,
  ]);

  const renderedSelection = useMemo(() => {
    if (!hasSelection || (selectionMode !== 'text' && !isEnzymeSelection)) return null;
    const segs = sp(selStart, selEnd);
    return (
      <g style={{ pointerEvents: 'none' }}>
        {segs.map((seg) => (
          <rect
            key={`selbg-${seg.row}-${seg.colStart}`}
            x={getX(seg.colStart)}
            y={getSeqY(seg.row) - 19}
            width={(seg.colEnd - seg.colStart + 1) * cw}
            height={28}
            fill={currentSelColor}
            rx="1"
          />
        ))}
      </g>
    );
  }, [hasSelection, selStart, selEnd, getSeqY, sp, currentSelColor, isEnzymeSelection]);

  const renderedDesignPicked = useMemo(() => {
    if (!designPick || designPick.segments.length === 0) return null;
    return (
      <g style={{ pointerEvents: 'none' }}>
        {designPick.segments.flatMap((picked, i) =>
          sp(picked.start, picked.end).map((seg) => (
            <rect
              key={`pickedbg-${i}-${seg.row}-${seg.colStart}`}
              x={getX(seg.colStart)}
              y={getSeqY(seg.row) - 19}
              width={(seg.colEnd - seg.colStart + 1) * cw}
              height={28}
              fill="#0f766e"
              fillOpacity={0.25}
              rx="1"
            />
          )),
        )}
      </g>
    );
  }, [designPick, getSeqY, sp]);

  // Stable background: all sequence text in dark color — doesn't depend on selection
  const renderedSeqBg = useMemo(() => {
    const vs = Math.max(0, visibleRows.start - ROW_BUF);
    const ve = Math.min(numRows - 1, visibleRows.end + ROW_BUF);
    const rows = [];
    for (let r = vs; r <= ve; r++) {
      const rowStart = r * charsPerLine;
      const rowEnd = Math.min(cleanSeq.length, (r + 1) * charsPerLine);
      const chunk = cleanSeq.substring(rowStart, rowEnd);
      const sy = getSeqY(r);
      rows.push(
        <text
          key={r}
          y={sy}
          fontFamily={monoFont}
          fontSize="14px"
          fontWeight="bold"
          style={{ userSelect: 'none', cursor: 'text' }}
        >
          {chunk.split('').map((c, i) => (
            <tspan key={i} x={getX(i) + cw / 2} textAnchor="middle" fill="#1f2937">
              {c}
            </tspan>
          ))}
        </text>,
      );
    }
    return rows;
  }, [visibleRows, charsPerLine, numRows, cleanSeq, getSeqY]);

  // Selection overlay: only renders selected characters in white (grouped by row)
  const renderedSeqSel = useMemo(() => {
    if (!hasSelection || (selectionMode !== 'text' && !isEnzymeSelection)) return null;
    const segs = sp(selStart, selEnd);
    // Group segments by row
    const byRow = {};
    for (const seg of segs) {
      (byRow[seg.row] || (byRow[seg.row] = [])).push(seg);
    }
    return Object.entries(byRow).map(([rowStr, rowSegs]) => {
      const row = parseInt(rowStr, 10);
      const sy = getSeqY(row);
      const rowStart = row * charsPerLine;
      return (
        <text
          key={`sel-${row}`}
          y={sy}
          fontFamily={monoFont}
          fontSize="14px"
          fontWeight="bold"
          style={{ userSelect: 'none', pointerEvents: 'none' }}
        >
          {rowSegs
            .map((seg) => {
              const chars = cleanSeq
                .substring(rowStart + seg.colStart, rowStart + seg.colEnd + 1)
                .split('');
              return chars.map((c, i) => (
                <tspan
                  key={`${seg.colStart + i}`}
                  x={getX(seg.colStart + i) + cw / 2}
                  textAnchor="middle"
                  fill={bgColor}
                >
                  {c}
                </tspan>
              ));
            })
            .flat()}
        </text>
      );
    });
  }, [hasSelection, selStart, selEnd, cleanSeq, charsPerLine, getSeqY, sp, isEnzymeSelection]);

  // --- translation (codon) selection render ---
  const renderedTranslationSelection = useMemo(() => {
    if (
      selectionMode !== 'translation' ||
      !translationSel ||
      !cdsFeatureData[translationSel.featureId]
    ) {
      return null;
    }
    const cds = cdsFeatureData[translationSel.featureId];
    const start = Math.min(translationSel.startCodon, translationSel.endCodon);
    const end = Math.max(translationSel.startCodon, translationSel.endCodon);
    const selectedBases = new Set();
    for (let i = start; i <= end; i++) {
      const t = cds.trans[i];
      if (!t) continue;
      for (const b of t.bases) selectedBases.add(b);
    }
    if (!selectedBases.size) return null;

    const byRow = {};
    for (const pos of selectedBases) {
      const row = Math.floor(pos / charsPerLine);
      const col = pos % charsPerLine;
      (byRow[row] || (byRow[row] = [])).push(col);
    }

    const rects = [];
    const texts = [];
    for (const [rowStr, cols] of Object.entries(byRow)) {
      const row = parseInt(rowStr, 10);
      const sy = getSeqY(row);
      const rowStart = row * charsPerLine;
      cols.sort((a, b) => a - b);

      const flush = (cs, ce) => {
        rects.push(
          <rect
            key={`trselbg-${row}-${cs}`}
            x={getX(cs)}
            y={sy - 19}
            width={(ce - cs + 1) * cw}
            height={28}
            fill={currentSelColor}
            rx="1"
          />,
        );
        const chars = cleanSeq.substring(rowStart + cs, rowStart + ce + 1).split('');
        texts.push(
          <text
            key={`trseltxt-${row}-${cs}`}
            y={sy}
            fontFamily={monoFont}
            fontSize="14px"
            fontWeight="bold"
            style={{ userSelect: 'none', pointerEvents: 'none' }}
          >
            {chars.map((c, i) => (
              <tspan key={i} x={getX(cs + i) + cw / 2} textAnchor="middle" fill={bgColor}>
                {c}
              </tspan>
            ))}
          </text>,
        );
      };

      let segStart = cols[0];
      let prev = cols[0];
      for (let i = 1; i < cols.length; i++) {
        if (cols[i] === prev + 1) {
          prev = cols[i];
        } else {
          flush(segStart, prev);
          segStart = prev = cols[i];
        }
      }
      flush(segStart, prev);
    }

    return (
      <g style={{ pointerEvents: 'none' }}>
        {rects}
        {texts}
      </g>
    );
  }, [
    selectionMode,
    translationSel,
    cdsFeatureData,
    charsPerLine,
    getSeqY,
    cleanSeq,
    currentSelColor,
  ]);

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
      />
      {combinedWarnings.length > 0 && <WarningBadge warnings={combinedWarnings} />}
      {designPick ? (
        <div
          style={{
            position: 'fixed',
            bottom: 16,
            left: '50%',
            transform: 'translateX(-50%)',
            zIndex: 40,
          }}
        >
          <div className="nav-bar-enter flex items-center gap-3 rounded-full border-2 border-teal-700 bg-background/80 px-4 py-2 shadow-[0_10px_40px_rgba(15,118,110,0.5)] ring-4 ring-teal-700/15 backdrop-blur-md">
            <span className="text-sm font-semibold text-teal-800">
              {DESIGN_MODES[designPick.mode].prompts[designPick.segments.length]}
            </span>
            {designPickError && <span className="text-xs text-destructive">{designPickError}</span>}
            <button
              type="button"
              onClick={cancelDesignPick}
              className="rounded-full bg-muted/60 px-3 py-1 text-sm text-foreground/80 transition-colors hover:bg-accent hover:text-foreground"
            >
              Cancel
            </button>
          </div>
        </div>
      ) : (
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
          alwaysExpandFeatures={alwaysExpandFeatures}
          onToggleAlwaysExpandFeatures={onToggleAlwaysExpandFeatures}
          onCreateFeature={createFeature}
          showPrimers={showPrimers}
          onTogglePrimers={onTogglePrimers}
          onCreatePrimer={createPrimer}
          showEnzymes={showEnzymes}
          onToggleEnzymes={onToggleEnzymes}
          enzymeFilter={enzymeFilter}
          onEnzymeFilterChange={onEnzymeFilterChange}
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
          onPrimerDesign={handlePrimerDesign}
          onOpenMyPrimers={onOpenMyPrimers}
          onOpenPrimerOverview={onOpenPrimerOverview}
          onOpenDetectFeatures={onOpenDetectFeatures}
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
        />
      )}
      <div
        ref={containerRef}
        style={{
          backgroundColor: bgColor,
          width: '100%',
          minHeight: '100vh',
          display: 'flex',
          justifyContent: 'center',
          alignItems: 'flex-start',
          padding: '0 1rem 4rem 1rem',
          overflowX: 'auto',
          userSelect: 'none',
          contain: 'layout style',
        }}
      >
        <div style={{ width: svgWidth, position: 'relative' }}>
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
            <style>{`@keyframes alignLabelScroll { from { transform: translateX(0); } to { transform: translateX(var(--align-label-scroll, 0px)); } }`}</style>
            {renderedCursor}
            {renderedDesignPicked}
            {renderedSelection}
            {renderedAlignments}
            {renderedAlignmentLabels}
            {renderedFeatures}
            {renderedFeatureLabels}
            {renderedEnzymes}
            {renderedPrimers}
            {renderedEnzymeLabels}
            {renderedEnzymeOverlay}
            {renderedSeqBg}
            {renderedSeqSel}
            {renderedTranslationSelection}
            {renderedAmplimerRegion}
            {renderedTooltips}
            {renderedSelectionInfo}
            {renderedHoverIndex}
          </svg>
          {insPopover && (
            <div
              onMouseDown={(e) => e.stopPropagation()}
              style={{
                position: 'absolute',
                left: insPopover.x,
                top: insPopover.y + 10,
                transform: 'translateX(-50%)',
                zIndex: 30,
                background: '#FFFFFF',
                border: '1px solid rgba(0,0,0,0.08)',
                borderRadius: 8,
                padding: '4px 10px',
                boxShadow: '0 4px 16px rgba(0,0,0,0.12)',
                fontFamily: monoFont,
                fontSize: '12px',
                color: '#1f2937',
                whiteSpace: 'nowrap',
              }}
            >
              {insPopover.bases}
            </div>
          )}
        </div>
        <FeatureInfoDialog
          feature={featureInfoFeature}
          open={featureInfoFeature !== null || createFeatureLoc !== null}
          onOpenChange={(open) => {
            if (!open) {
              setFeatureInfoFeature(null);
              setCreateFeatureLoc(null);
            }
          }}
          onFtypeChange={onFeatureFtypeChange}
          onFeatureColorChange={onFeatureColorChange}
          onFeatureLocationChange={onFeatureLocationChange}
          onFeatureNameChange={onFeatureNameChange}
          onFeatureStrandChange={onFeatureStrandChange}
          newFeatureLoc={createFeatureLoc}
          onFeatureAdd={onFeatureAdd}
          onDeleteFeature={onFeatureDelete}
          features={features}
        />
        <PrimerAlignmentDialog
          primer={primerAlignmentPrimer}
          alignmentData={primerAlignmentCache[primerAlignmentPrimer?.id]}
          open={primerAlignmentPrimer !== null || createPrimerSeq !== null}
          onOpenChange={(open) => {
            if (!open) {
              setPrimerAlignmentPrimer(null);
              setCreatePrimerSeq(null);
            }
          }}
          seedLength={primerSeedLength}
          newPrimerSeq={createPrimerSeq}
          onPrimerChange={onPrimerChange}
          onDeletePrimer={onPrimerDelete}
          primers={primers}
          tmParams={tmParams}
        />
        <PrimerDesignDialog
          open={designDialog !== null}
          onOpenChange={(open) => {
            if (!open) setDesignDialog(null);
          }}
          mode={designDialog?.mode}
          segments={designDialog?.segments}
          sequence={cleanSeq}
          topology={topology}
          tmParams={tmParams}
          onPrimerChange={onPrimerChange}
        />
      </div>
    </>
  );
});

export default SequenceEditor;
