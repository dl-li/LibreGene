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
  sliceRange,
  rangeLen,
  featureSelRange,
  rangeLocString1based,
  enzymeActiveBlue,
  amplimerGreen,
  peptideMassKda,
} from './editorConstants';
import FeatureInfoDialog from './FeatureInfoDialog';
import PrimerAlignmentDialog from './PrimerAlignmentDialog';
import EditorNavMenu from './EditorNavMenu';
import PrimerDesignDialog from './plugins/primerDesign/PrimerDesignDialog';
import { DESIGN_MODES } from './plugins/primerDesign';
import { computePrimerAlignment, computeTm, blastSubmit, getEnzymeDatabase } from './tauriApi';
import { TRACE_CHANNELS, traceRangeMax, buildTracePath, buildColumnQueryMap } from './chromatogram';
import { getRelatedEnzymes } from './enzymeRelated';
import { CircularMap, LinearMap } from './MapView';
import FornaView from './plugins/rnaFold/FornaView';
import useRnaFold, { MAX_INTERACTIVE_NT } from './plugins/rnaFold/useRnaFold';
import { buildSearchResults } from './searchUtils';
import { showContextMenu } from './contextMenu';
import {
  collectAnnotations,
  writeAnnotatedClipboard,
  readClipboardMeta,
  parseMetaFromPasteEvent,
} from './clipboardAnnotations';
import {
  AlertTriangle,
  Bot,
  Check,
  Copy,
  CopyPlus,
  CopyMinus,
  CopyX,
  EyeOff,
  Globe,
  Image,
  LockOpen,
  Pencil,
  Scissors,
  Tag,
} from 'lucide-react';

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

/** '-' placeholder segments covering template gaps between consecutive
 *  segments of one alignment (split-read deletions), split at the origin so
 *  each piece stays contiguous like real segments. */
function alignmentGapSegments(al, tlen) {
  const segs = al.segments || [];
  const gaps = [];
  if (!tlen) return gaps;
  for (let i = 0; i + 1 < segs.length; i++) {
    const gap = (((segs[i + 1].start - segs[i].end - 1) % tlen) + tlen) % tlen;
    if (gap === 0) continue;
    const start = (segs[i].end + 1) % tlen;
    for (const { start: s, end: e } of buildMatchSegs(start, (start + gap - 1) % tlen, tlen)) {
      gaps.push({ start: s, end: e, chars: '-'.repeat(e - s + 1), gap: true });
    }
  }
  return gaps;
}

/** Insertion placeholder dot in an alignment read lane; hover is lifted to
 *  the caller so every dot of an insertion group reacts together. */
function InsDot({ x, onMouseDown, onMouseEnter, onMouseLeave }) {
  return (
    <tspan
      x={x}
      textAnchor="middle"
      fill="#1f2937"
      fillOpacity={0.55}
      style={{ cursor: 'pointer' }}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
      onMouseDown={onMouseDown}
    >
      ·
    </tspan>
  );
}

// Insertions this close together (chained, same alignment) form one hover
// group — e.g. the junction insertion and a nearby 1 bp insertion.
const INS_GROUP_GAP = 25;

/** Map of insertion pos -> group index, grouping nearby insertions. */
function insertionGroups(insertions) {
  const sorted = [...insertions].sort((a, b) => a.pos - b.pos);
  const groupOf = new Map();
  let g = -1;
  let last = -Infinity;
  for (const ins of sorted) {
    if (ins.pos - last > INS_GROUP_GAP) g += 1;
    groupOf.set(ins.pos, g);
    last = ins.pos;
  }
  return groupOf;
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

// Extra vertical gap between alignment lanes and the feature tracks below.
const ALIGN_FEAT_GAP = 10;

// Chromatogram (ab1 trace) band geometry: band height and the gap between
// stacked bands / to the next block below the sequence.
const CHROM_TRACK_H = 46;
const CHROM_GAP = 4;

// ---------------------------------------------------------------------------
// MapWatermark — non-interactive plasmid map rendered as a faint overlay on
// top of the editor (toggled from the Map dialog footer). Lives outside the
// main container because `contain: layout style` breaks position: fixed.
// ---------------------------------------------------------------------------
const noop = () => {};

const MapWatermark = React.memo(function MapWatermark({ length, features, topology, name, sel }) {
  if (!length) return null;
  return (
    <div
      aria-hidden
      className="[&_*]:pointer-events-none"
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 10,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        pointerEvents: 'none',
      }}
    >
      <div style={{ width: 680, opacity: 0.1 }}>
        {topology === 'circular' ? (
          <CircularMap
            length={length}
            features={features}
            name={name}
            selection={sel}
            bg="transparent"
            onSelect={noop}
            onClear={noop}
            onFeatureOpen={noop}
            hideLabels
          />
        ) : (
          <LinearMap
            length={length}
            features={features}
            name={name}
            selection={sel}
            onSelect={noop}
            onClear={noop}
            onFeatureOpen={noop}
            hideLabels
          />
        )}
      </div>
    </div>
  );
});

// ---------------------------------------------------------------------------
// FoldWatermark — non-interactive RNA secondary structure rendered as a faint
// overlay (toggled from the RNA Folding dialog footer; mutually exclusive
// with the map watermark). Same fixed-overlay rationale as MapWatermark.
// Selected bases are highlighted: dark-brown circle, letter in bgColor.
// ---------------------------------------------------------------------------
const FoldWatermark = React.memo(function FoldWatermark({ sequence, selStart, selEnd }) {
  const { result } = useRnaFold(sequence, !!sequence && sequence.length <= MAX_INTERACTIVE_NT);
  // Sized to the viewport (the overlay is fixed and centered; forna re-fits
  // via its own resize handling).
  const [vp] = useState(() => ({ w: window.innerWidth, h: window.innerHeight }));
  const selRanges = useMemo(() => {
    if (selStart == null || selEnd == null) return null;
    if (selStart <= selEnd) return [[selStart, selEnd]];
    // Circular wrap-around selection: highlight both arms.
    return [
      [selStart, sequence.length - 1],
      [0, selEnd],
    ];
  }, [selStart, selEnd, sequence.length]);
  if (!sequence || !result) return null;
  return (
    <div
      aria-hidden
      className="[&_*]:pointer-events-none"
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 10,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        pointerEvents: 'none',
      }}
    >
      <div style={{ width: Math.round(vp.w * 0.85), opacity: 0.1 }}>
        <FornaView
          sequence={sequence}
          structure={result.structure}
          height={Math.round(vp.h * 0.8)}
          settleMs={4000}
          interactive={false}
          selectionRanges={selRanges}
          selectionTextColor={bgColor}
        />
      </div>
    </div>
  );
});

// ---------------------------------------------------------------------------
// SelectionLengthBadge — top-right badge showing "xx bp" for the current
// selection (text / enzyme / primer / amplimer).  The second line shows GC%
// for nucleic acids or the peptide molecular weight (kDa) for proteins.
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
  topology = 'linear',
  unit = 'bp',
  showGc = true,
  showMw = false,
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
    } else if (topology === 'circular') {
      intervening =
        cleanSeq.substring(fwdPrimer.matchEnd + 1) + cleanSeq.substring(0, revPrimer.matchStart);
    } else {
      // Linear template: primers overlapping / facing away — no valid amplicon
      return null;
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
    len = rangeLen(selStart, selEnd, cleanSeq.length);
    seqToCopy = sliceRange(cleanSeq, selStart, selEnd);
    bg = enzymeActiveBlue;
  } else if (selectionMode === 'text' && selStart !== null && selEnd !== null) {
    len = rangeLen(selStart, selEnd, cleanSeq.length);
    seqToCopy = sliceRange(cleanSeq, selStart, selEnd);
    bg = '#3E2723';
  } else {
    return null;
  }

  // Second line: GC% for nucleic acids, molecular weight for peptides.
  let line2 = null;
  if (showGc) {
    const gc = (seqToCopy.match(/[GC]/gi) || []).length;
    const gcPct = seqToCopy.length > 0 ? Math.round((gc / seqToCopy.length) * 100) : 0;
    line2 = `${gcPct}% GC`;
  } else if (showMw) {
    const kda = peptideMassKda(seqToCopy);
    line2 = `${kda < 1 ? kda.toFixed(2) : kda.toFixed(1)} kDa`;
  }

  // Measure the default label width so the badge has stable width
  const line1 = `${len} ${unit}`;
  const w1 = measureWidth(line1, `600 11px ${monoFont}`);
  const w2 = showGc
    ? measureWidth('100% GC', `600 11px ${monoFont}`)
    : line2
      ? measureWidth(line2, `600 11px ${monoFont}`)
      : 0;
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
      {line2 && <div>{line2}</div>}
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

// Nudge lightness so two abutting same-colored bars stay distinguishable.
// Input colors are the readability-mapped ones (normFeatures); the shift is
// applied on top of that mapping.
const shiftAbutLightness = (hex) => {
  if (!/^#[0-9a-fA-F]{6}$/.test(hex)) return hex;
  const [r, g, b] = _hexToRgb(hex);
  const [h, s, l] = _rgbToHsl(r, g, b);
  const nl = l <= 0.7 ? Math.min(1, l + 0.1) : Math.max(0, l - 0.1);
  return _rgbToHex(..._hslToRgb(h, s, nl));
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
  featureLabelsBelow = false,
  showOrfs,
  onToggleOrfs,
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
  onOpenMapView,
  onEnzymeHoverChange,
  blastEnabled = false,
  topology = 'linear',
  onToggleTopology,
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
  moleculeType = 'dna',
  // Agent-tab lock: the nav menu is replaced by a teal-outlined control pill
  // (same look as the primer-design pick bar), which also blocks nav-level
  // misoperation while the MCP agent works.
  agentLocked = false,
  onUnlockAgent,
  // Hidden workspaces stay mounted (CSS-only); global input listeners must not
  // respond while this editor is not the visible one.
  hidden = false,
}) {
  const isDna = moleculeType === 'dna';
  // Length unit for the sequence: base pairs (DNA), nucleotides (ss-RNA),
  // amino acids (protein).
  const seqUnit = isDna ? 'bp' : moleculeType === 'protein' ? 'aa' : 'nt';
  const containerRef = useRef(null);
  const [charsPerLine, setCharsPerLine] = useState(initialCharsPerLine);
  const [hoveredFeature, setHoveredFeature] = useState(null);
  const featureLeaveRef = useRef(null);
  // Feature whose range produced the current text selection (via feature
  // click); enables two-stage Backspace: first deletes the feature, then the
  // sequence. Guarded by exact selStart/selEnd equality at delete time.
  const featureSelRef = useRef(null);
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
  const [hoverInsGroup, setHoverInsGroup] = useState(null); // `${alignmentId}:${insertionGroup}`

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
  const avgRowPitchRef = useRef(60); // average row pitch, kept in sync at svgHeight
  const svgRef = useRef(null);

  // --- selection state ---
  const [cursorIndex, setCursorIndex] = useState(null);
  const [selStart, setSelStart] = useState(null);
  const [selEnd, setSelEnd] = useState(null);
  const [isDragging, setIsDragging] = useState(false);
  const [selectionTm, setSelectionTm] = useState(null);
  const tmLastRunRef = useRef(0); // last computeTm dispatch time (drag throttle)
  const tmTimerRef = useRef(null); // pending trailing Tm timer (kept across re-runs)
  const tmSeqRef = useRef(null); // latest selection seq a Tm was scheduled for
  const dragRef = useRef({ startIdx: null, active: false });
  const isDraggingRef = useRef(false);
  const autoScrollRef = useRef({ clientX: 0, clientY: 0 });
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
  const [hoveredCodon, setHoveredCodon] = useState(null); // { featureId, codonIndex }
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
    if (restoreState.cursorIndex != null) {
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
    (selStart <= selEnd || topology === 'circular') &&
    (selectionMode === 'text' || isEnzymeSelection);
  // Wrap selection (start > end on a circular sequence): display/copy work,
  // but edit operations (replace/delete/paste) stay linear-only.
  const selWraps = hasSelection && selStart > selEnd;
  const hasTranslationSelection = selectionMode === 'translation' && translationSel !== null;
  const currentSelColor = designPick ? '#0f766e' : isEnzymeSelection ? enzymeActiveBlue : '#3E2723';

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
      featLabelBelowExtra: 8,
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
      setCharsPerLine(
        Math.max(20, Math.floor((containerRef.current.clientWidth - startX * 2) / cw)),
      );
    }
    const scroller = scrollContainerRef?.current;
    if (scroller) setViewportH(scroller.clientHeight || 900);
  }, [layoutKey]);

  const cleanSeq = moleculeType === 'protein' ? (sequence || '').toUpperCase() : sequence || '';

  // Ensure primer color is a valid non-black hex, falling back to default green
  const safePrimerColor = (c) => {
    if (!c || c === '#000000' || c === '#000' || c === 'black') return '#166534';
    if (/^#[0-9a-f]{6}$/i.test(c)) return c;
    return '#166534';
  };

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

  const numRows = Math.max(1, Math.ceil(cleanSeq.length / charsPerLine));
  numRowsRef.current = numRows;
  const svgWidth = startX + charsPerLine * cw + startX;

  const sp = useCallback(
    (s, e) =>
      s <= e
        ? splitRange(s, e, charsPerLine)
        : [...splitRange(s, cleanSeq.length - 1, charsPerLine), ...splitRange(0, e, charsPerLine)],
    [charsPerLine, cleanSeq.length],
  );

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
      for (const r of rows) {
        if (r < 0 || r >= numRows) continue;
        if (!perRow[r].has(ti)) perRow[r].set(ti, perRow[r].size);
        if (hasChrom && !chromPerRow[r].has(ti)) chromPerRow[r].set(ti, chromPerRow[r].size);
      }
    });
    const counts = perRow.map((m) => m.size);
    const chromCounts = chromPerRow.map((m) => m.size);
    const mainChromH = chromatogram ? CHROM_TRACK_H + CHROM_GAP : 0;
    // Extra below-sequence height contributed by chromatogram bands.
    const chromBelow = chromCounts.map((n) => mainChromH + n * (CHROM_TRACK_H + CHROM_GAP));
    return { perRow, counts, chromPerRow, chromCounts, mainChromH, chromBelow };
  }, [alignmentTracks, numRows, sp, alignmentChromatograms, chromatogram, cleanSeq.length]);

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

  // Lightness-nudge for abutting (touching, non-overlapping) same-colored bars
  // so they stay distinguishable. Chain rule: A-B-C-D same color → A base,
  // B shifted, C base (B already differs), D shifted. Segment level applies in
  // both modes (joined segments that touch); feature level only in
  // labels-below mode.
  const abutColors = useMemo(() => {
    const feat = {};
    if (featureLabelsBelow) {
      const ordered = [...normFeatures].sort(
        (a, b) =>
          Math.min(...a.segments.map((s) => s.start)) - Math.min(...b.segments.map((s) => s.start)),
      );
      const shownByEnd = new Map();
      for (const f of ordered) {
        const start = Math.min(...f.segments.map((s) => s.start));
        const end = Math.max(...f.segments.map((s) => s.end));
        const color = f.color || ensureReadableColor('#60A5FA');
        const abutters = shownByEnd.get(start - 1);
        const shown = abutters && abutters.includes(color) ? shiftAbutLightness(color) : color;
        if (shown !== color) feat[f.id] = shown;
        if (!shownByEnd.has(end)) shownByEnd.set(end, []);
        shownByEnd.get(end).push(shown);
      }
    }
    const seg = {};
    for (const f of normFeatures) {
      const order = f.segments
        .map((_, i) => i)
        .sort((a, b) => f.segments[a].start - f.segments[b].start);
      const cols = new Array(f.segments.length);
      let prevDi = -1;
      for (const di of order) {
        const s = f.segments[di];
        const base = s.color || feat[f.id] || f.color || ensureReadableColor('#60A5FA');
        cols[di] =
          prevDi >= 0 && s.start === f.segments[prevDi].end + 1 && base === cols[prevDi]
            ? shiftAbutLightness(base)
            : base;
        prevDi = di;
      }
      seg[f.id] = cols;
    }
    return { seg, feat };
  }, [normFeatures, featureLabelsBelow]);

  const { processedFeatures, primerTracks, featureRowTracks, revPrimerFeatOffsets } =
    useMemo(() => {
      const resultFeatures = [];
      const bottomTracks = [];

      if (normFeatures.length > 0) {
        const sorted = [...normFeatures].sort((a, b) => {
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
              const rawVs = isFwd && mi === 0 ? m.start - ml : m.start;
              const rawVe = !isFwd && mi === segs.length - 1 ? m.end + ml : m.end;
              if (rawVe < rs || rawVs > re) return;
              // 1nt judgment buffer on the 3' (arrow-tip) side so two abutting
              // primers don't share a track and bleed into each other
              const vs = !isFwd && mi === 0 ? rawVs - 1 : rawVs;
              const ve = isFwd && mi === segs.length - 1 ? rawVe + 1 : rawVe;
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
          // Below-line labels sit flush under the bar (fwd: left end aligned,
          // rev: right end aligned); the whole feature interval extends one
          // track down, so nothing in the next track sits under this feature.
          // Below mode uses no half-column margins: abutting (non-overlapping)
          // features may share a track.
          const hangsBelow = featureLabelsBelow && !f.orf;
          // Below-mode labels are never truncated: reserve the exact rendered
          // label width instead of the padded estimate.
          const labelCols = hangsBelow
            ? Math.ceil(
                primerLabelW(isFRev ? `< ${f.name}` : f.strand === '+' ? `${f.name} >` : f.name) /
                  cw,
              )
            : Math.ceil(primerLabelW(f.name) / cw) + 2 + (f.strand && f.strand !== '.' ? 2 : 0);
          const es = hangsBelow
            ? isFRev
              ? Math.min(segStart, Math.max(rs, segEnd - labelCols))
              : segStart
            : isFRev
              ? segStart - 0.5
              : Math.max(rs, segStart - labelCols) - 0.5;
          const ee = hangsBelow
            ? isFRev
              ? segEnd
              : Math.max(segEnd, segStart + labelCols)
            : isFRev
              ? segEnd + labelCols + 0.5
              : segEnd + 0.5;
          const overlaps = (track, s, e) =>
            rowTracks[track] && rowTracks[track].some((t) => !(e < t.start || s > t.end));
          let placed = false;
          for (let i = 0; i < rowTracks.length; i++) {
            if (!overlaps(i, es, ee) && (!hangsBelow || !overlaps(i + 1, es, ee))) {
              rowTracks[i].push({ start: es, end: ee });
              if (hangsBelow) {
                if (!rowTracks[i + 1]) rowTracks[i + 1] = [];
                rowTracks[i + 1].push({ start: es, end: ee });
              }
              if (!fRowTracks[f.id]) fRowTracks[f.id] = {};
              fRowTracks[f.id][r] = i;
              placed = true;
              break;
            }
          }
          if (!placed) {
            rowTracks.push([{ start: es, end: ee }]);
            if (hangsBelow) rowTracks.push([{ start: es, end: ee }]);
            if (!fRowTracks[f.id]) fRowTracks[f.id] = {};
            fRowTracks[f.id][r] = rowTracks.length - (hangsBelow ? 2 : 1);
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
          // 1nt buffer on the 3' (arrow-tip) side: an abutting feature still
          // counts as overlapping, so the primer drops below its track
          const vs = mi === 0 ? m.start - 1 : m.start,
            ve = mi === psegs.length - 1 ? m.end + ml : m.end;
          for (const f of resultFeatures) {
            for (const fseg of f.segments) {
              const isFRev = f.strand === '-';
              const hangsBelow = featureLabelsBelow && !f.orf;
              const labelCols = hangsBelow
                ? Math.ceil(
                    primerLabelW(
                      isFRev ? `< ${f.name}` : f.strand === '+' ? `${f.name} >` : f.name,
                    ) / cw,
                  )
                : Math.ceil(primerLabelW(f.name) / cw) + 2 + (f.strand && f.strand !== '.' ? 2 : 0);
              const fvs = hangsBelow
                ? isFRev
                  ? Math.min(fseg.start, fseg.end - labelCols)
                  : fseg.start
                : isFRev
                  ? fseg.start
                  : fseg.start - labelCols;
              const fve = hangsBelow
                ? isFRev
                  ? fseg.end
                  : Math.max(fseg.end, fseg.start + labelCols)
                : isFRev
                  ? fseg.end + labelCols
                  : fseg.end;
              if (fve < vs || fvs > ve) continue;
              const sr = Math.floor(fseg.start / charsPerLine);
              const er = Math.floor(fseg.end / charsPerLine);
              for (let r = sr; r <= er; r++) {
                const ft = (fRowTracks[f.id] || {})[r] || 0;
                // below-line labels hang one extra track lower, clear them too
                const tracksToClear = ft + (featureLabelsBelow && !f.orf ? 2 : 1);
                revFeatOff[p.id][r] = Math.max(
                  revFeatOff[p.id][r] || 0,
                  tracksToClear * lp.featTrackHeight,
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
    }, [features, enrichedPrimers, numRows, charsPerLine, lp, featureLabelsBelow]);

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

  // Pre-compute fwd primer occupied x-ranges per row so enzyme track assignment
  // can lift labels clear of a primer. Two tiers, both reserved up-front so
  // expanding a primer never shifts other elements:
  //  - label zone (label + 5' tail): a selected primer lifts its label ~16px,
  //    top ≈ 69px above the sequence.
  //  - body zone: only the expanded block reaches here, top ≈ 56px.
  //  The lift formula adds the clearance gap on top of these. Expanded content
  //  also paints above enzyme labels as a backstop. The label renders once per
  //  segment (multi-row primers repeat it), so occupancy covers every segment.
  const primerLabelOcc = useMemo(() => {
    const occ = {}; // { [row]: [{x1, x2, topOffset}] }
    for (const [rowStr, primers] of Object.entries(primersByRow)) {
      const row = parseInt(rowStr, 10);
      const entries = [];
      for (const p of primers) {
        if (!p.isFwd) continue;
        const segs = (p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }]).flatMap((m) =>
          sp(m.start, m.end),
        );
        const ml = p.mismatchStr?.length || 0;
        const pt = (primerTracks[p.id] || {})[row] || 0;
        const nameW = primerLabelW(p.name);
        for (const seg of segs) {
          if (seg.row !== row) continue;
          const drawMisLen = seg === segs[0] ? Math.min(ml, seg.colStart + 5) : 0;
          const labelX = getX(seg.colStart - drawMisLen);
          const off = pt * pp.trackGap;
          entries.push({
            x1: labelX,
            x2: Math.max(labelX + nameW, getX(seg.colStart)),
            topOffset: 69 + off,
          });
          entries.push({
            x1: getX(seg.colStart),
            x2: getX(seg.colEnd) + cw + pp.arrowHeadLen,
            topOffset: 56 + off,
          });
        }
      }
      if (entries.length) occ[row] = entries;
    }
    return occ;
  }, [primersByRow, primerTracks, sp, pp.trackGap, pp.arrowHeadLen]);

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
        // Lift labels whose x-range overlaps a fwd primer label/body. The
        // enzyme text baseline sits (enzLabelBase-5)+lift above the sequence;
        // lift is continuous — just enough to clear the occupancy top by 6px.
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
        let lift = avoidOff > 0 ? avoidOff + 6 - (lp.enzLabelBase - 5) : 0;
        // Enzyme-vs-enzyme: keep a full track of vertical separation between
        // x-overlapping labels, stacked on actual lifts.
        for (;;) {
          let bump = 0;
          for (const o of occupied) {
            if (Math.abs(o.lift - lift) < lp.enzTrackHeight && !(ce < o.cs || cs > o.ce)) {
              bump = Math.max(bump, o.lift + lp.enzTrackHeight);
            }
          }
          if (!bump) break;
          lift = Math.max(lift, bump);
        }
        occupied.push({ lift, cs, ce });
        if (!eTracks[item.key]) eTracks[item.key] = {};
        eTracks[item.key][r] = lift;
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
                alignLaneInfo.chromBelow[r] +
                alignLaneInfo.counts[r] * lp.featTrackHeight +
                (alignLaneInfo.counts[r] > 0 ? ALIGN_FEAT_GAP : 0),
            );
          }
        }
      }

      // Enzymes: above fwd primers; per-row lifts from eTracks
      const rowEnz = enzymesByRow[r];
      if (rowEnz && rowEnz.length > 0) {
        let maxEnzLift = 0;
        for (const e of rowEnz) {
          const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
          pairs.forEach((cp, pi) => {
            if (Math.floor(cp.topCutIndex / charsPerLine) !== r) return;
            const key = pairs.length > 1 ? `${e.id}_p${pi}` : e.id;
            const lift = (eTracks[key] || {})[r] || 0;
            maxEnzLift = Math.max(maxEnzLift, lift);
          });
        }
        const enzDefaultAbove = lp.enzLabelBase + lp.enzAbovePad;
        const enzBase =
          maxFwdPrimerH > 0 ? Math.max(enzDefaultAbove, maxFwdPrimerH + 38) : enzDefaultAbove;
        ae = Math.max(ae, enzBase + maxEnzLift);
      }

      // Features: below sequence (shifted down by alignment lanes and any
      // chromatogram bands)
      const nAlign = alignLaneInfo.counts[r];
      const chromBelow = alignLaneInfo.chromBelow[r];
      if (nAlign > 0 || chromBelow > 0) {
        be = Math.max(
          be,
          lp.featBaseOffset +
            chromBelow +
            nAlign * lp.featTrackHeight +
            (nAlign > 0 ? ALIGN_FEAT_GAP : 0) +
            lp.featLabelPad,
        );
      }
      const rowFeats = featuresByRow[r];
      if (rowFeats) {
        for (const { feature: f } of rowFeats) {
          const t = (featureRowTracks[f.id] || {})[r] || 0;
          be = Math.max(
            be,
            lp.featBaseOffset +
              chromBelow +
              (t + nAlign) * lp.featTrackHeight +
              (nAlign > 0 ? ALIGN_FEAT_GAP : 0) +
              lp.featLabelPad +
              (featureLabelsBelow && !f.orf ? lp.featLabelBelowExtra : 0),
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
    featureLabelsBelow,
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
      const col = Math.max(0, Math.min(charsPerLine, colBase + side));
      // Content-based row boundaries: top of current row → top of next row
      // Row spacing already ensures a gap between row content areas
      const row = rowAtSvgY(svgPt.y);
      if (row < 0) return null;
      const idx = row * charsPerLine + col;
      return Math.max(0, Math.min(cleanSeq.length, idx));
    },
    [charsPerLine, rowAtSvgY, cleanSeq],
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
      const row = rowAtSvgY(svgPt.y);
      if (row < 0) return null;
      const idx = row * charsPerLine + col;
      return Math.max(0, Math.min(cleanSeq.length - 1, idx));
    },
    [charsPerLine, rowAtSvgY, cleanSeq],
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
      // codon (not just the feature block) shows the AA number.
      let nextCodon = null;
      if (idx !== null) {
        for (const [featureId, cds] of Object.entries(cdsFeatureDataRef.current)) {
          const codon = cds.codonMap.get(idx);
          if (codon !== undefined) {
            nextCodon = { featureId, codonIndex: codon };
            break;
          }
        }
      }
      setHoveredCodon((prev) =>
        prev?.featureId === nextCodon?.featureId && prev?.codonIndex === nextCodon?.codonIndex
          ? prev
          : nextCodon,
      );
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
          if (mode === 'sense' && positions.length > 0) {
            const rangeStart = positions[0];
            const rangeEnd = positions[positions.length - 1];
            const meta = collectAnnotations({ features, primers }, rangeStart, rangeEnd);
            writeAnnotatedClipboard(text, meta);
          } else {
            navigator.clipboard.writeText(text).catch(() => {});
          }
          return;
        }
      }

      if (!hasSelection) return;
      const sense = sliceRange(cleanSeq, selStart, selEnd);
      let text;
      if (mode === 'antisense') {
        text = reverseComplement(sense);
      } else {
        text = sense;
      }
      if (mode === 'sense' && hasSelection) {
        const meta = collectAnnotations({ features, primers }, selStart, selEnd, cleanSeq.length);
        writeAnnotatedClipboard(text, meta);
      } else {
        navigator.clipboard.writeText(text).catch(() => {});
      }
    },
    [
      hasSelection,
      cleanSeq,
      selStart,
      selEnd,
      translationSel,
      cdsFeatureDataRef,
      features,
      primers,
    ],
  );

  // --- Custom context menus (right-click) ---
  const writeClipboard = useCallback((text) => {
    if (text) navigator.clipboard.writeText(text).catch(() => {});
  }, []);

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
      ]);
    },
    [selectPrimer, primerMenuItems, enrichedPrimers],
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
      // sequence length) fall back to the display window.
      const recWraps = enzyme.recStart == null || enzyme.recEnd >= cleanSeq.length;
      setSelStart(recWraps ? enzyme.displayStart : enzyme.recStart);
      setSelEnd(recWraps ? enzyme.displayEnd : enzyme.recEnd);
      setCursorIndex(null);
      setIsEnzymeSelection(true);
      setSelectedEnzymeIds([entryId ?? enzyme.id]);
      clearCursorTimer();
    },
    [cleanSeq, clearCursorTimer],
  );

  const openEnzymeMenu = useCallback(
    async (e, l) => {
      e.preventDefault();
      e.stopPropagation();
      const enzyme = enzymes.find((x) => x.id === l.groupId);
      if (enzyme) selectEnzymeSite(enzyme, l.id);
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
        items.push(
          { type: 'separator' },
          {
            icon: Scissors,
            label: 'Related Enzymes',
            children: [
              { label: 'Isocaudomers', disabled: true },
              ...nameItems(related.isocaudomers),
              { type: 'separator' },
              { label: 'Isoschizomers', disabled: true },
              ...nameItems(related.isoschizomers),
            ],
          },
        );
      }
      showContextMenu(e.clientX, e.clientY, items);
    },
    [enzymes, isDna, copySelection, loadEnzymeDb, selectEnzymeSite],
  );

  const copyAmplimer = useCallback(() => {
    if (selectedPrimerIds.length !== 2) return;
    const fp = enrichedPrimers.find((p) => p.id === selectedPrimerIds[0]);
    const rp = enrichedPrimers.find((p) => p.id === selectedPrimerIds[1]);
    const fwdPrimer = fp && fp.isFwd ? fp : rp;
    const revPrimer = fp && !fp.isFwd ? fp : rp;
    if (!fwdPrimer || !revPrimer) return;
    // Linear template: no valid amplicon when the fwd primer is not upstream
    if (topology !== 'circular' && fwdPrimer.matchEnd >= revPrimer.matchStart) return;
    const fSeq = fwdPrimer.primerSeq || matchedSeqOf(fwdPrimer, cleanSeq);
    const rSeq = revPrimer.primerSeq || matchedSeqOf(revPrimer, cleanSeq);
    let intervening;
    if (fwdPrimer.matchEnd < revPrimer.matchStart) {
      intervening = cleanSeq.substring(fwdPrimer.matchEnd + 1, revPrimer.matchStart);
    } else {
      intervening =
        cleanSeq.substring(fwdPrimer.matchEnd + 1) + cleanSeq.substring(0, revPrimer.matchStart);
    }
    writeClipboard(fSeq + intervening + reverseComplement(rSeq));
  }, [selectedPrimerIds, enrichedPrimers, cleanSeq, writeClipboard, topology]);

  // Submit the current text selection to NCBI BLAST.
  const handleBlastSelection = useCallback(() => {
    if (!hasSelection) return;
    submitBlast(sliceRange(cleanSeq, selStart, selEnd));
  }, [hasSelection, cleanSeq, selStart, selEnd, submitBlast]);

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
        if (blastEnabled) {
          items.push({ type: 'separator' });
          items.push({
            icon: Globe,
            label: blastBusy ? 'Submitting BLAST…' : 'BLAST Selection',
            disabled: blastBusy,
            onSelect: handleBlastSelection,
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
      copySelection,
      copyAmplimer,
      primerMenuItems,
      blastEnabled,
      blastBusy,
      handleBlastSelection,
      background,
      backgroundOptions,
      onBackgroundChange,
    ],
  );

  // --- Paste: build insert/replace request from clipboard text ---
  const requestPaste = useCallback(
    (clipboardText, clipboardMeta = null) => {
      if (!clipboardText || !onEditRequest) return;
      if (selWraps) return; // replacing across the origin is not supported
      if (hasSelection) {
        onEditRequest({
          type: 'replace',
          cursorIndex: selStart,
          selStart,
          selEnd,
          selectedText: cleanSeq.substring(selStart, selEnd + 1),
          clipboardText,
          clipboardMeta,
        });
      } else if (cursorIndex !== null) {
        onEditRequest({
          type: 'insert',
          cursorIndex,
          clipboardText,
          clipboardMeta,
        });
      }
    },
    [onEditRequest, hasSelection, selWraps, cursorIndex, selStart, selEnd, cleanSeq],
  );

  const pasteFromClipboard = useCallback(() => {
    navigator.clipboard
      .readText()
      .then((t) => {
        if (!t) return;
        const meta = readClipboardMeta(t);
        requestPaste(t, meta);
      })
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
      if (!hasSelection || selWraps || selectionMode !== 'text' || !onEditRequest) return;
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
      selWraps,
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
    resetCursorTimer,
    selectionMode,
    selectedPrimerIds,
    enrichedPrimers,
    onEditRequest,
    onFeatureDelete,
    features,
    copySelection,
    translationSel,
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

  // --- Paste event: read clipboard and trigger insert/replace dialog ---
  useEffect(() => {
    if (!onEditRequest || hidden) return undefined;
    const onPaste = (e) => {
      // Ignore paste in input/textarea (e.g. dialog textarea, search input)
      const tag = e.target?.tagName?.toLowerCase();
      if (tag === 'input' || tag === 'textarea' || e.target?.isContentEditable) return;

      // Get clipboard text synchronously from the paste event
      const clipboardText = e.clipboardData?.getData('text') || '';
      if (!clipboardText) return;

      const meta = parseMetaFromPasteEvent(e) || readClipboardMeta(clipboardText);
      e.preventDefault();
      requestPaste(clipboardText, meta);
    };
    window.addEventListener('paste', onPaste);
    return () => window.removeEventListener('paste', onPaste);
  }, [onEditRequest, requestPaste, hidden]);

  // Visible row range for enzyme virtualization.
  // Hold a stable object identity while start/end are unchanged: downstream
  // memos (visibleEnzymes/visibleFeatures/renderedSeqBg/...) key on this
  // object, and scrollY churns every frame — a fresh object would recompute
  // all of them even when the visible range didn't actually move.
  const visibleRowsRef = useRef({ start: 0, end: 0 });
  const visibleRows = useMemo(() => {
    let start = 0,
      end = numRows - 1;
    if (rowY.length) {
      const vh = viewportH || 900;
      const top = scrollY;
      const bot = top + vh;
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
    }
    const prev = visibleRowsRef.current;
    if (prev.start === start && prev.end === end) return prev;
    visibleRowsRef.current = { start, end };
    return visibleRowsRef.current;
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
  avgRowPitchRef.current = numRows > 1 ? (rowY[numRows - 1] - rowY[0]) / (numRows - 1) : 60;

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
      let col = Math.floor(xRel / cw);
      if (xRel < 0) col = 0;
      if (col >= charsPerLine) col = charsPerLine - 1;

      const row = rowAtSvgY(svgPt.y);
      if (row < 0) return;

      const idx = row * charsPerLine + col;
      const codon = cds.codonMap.get(idx);
      if (codon === undefined) return;
      const maxCodon = cds.trans.length - 1;
      const clamped = Math.max(0, Math.min(maxCodon, codon));
      setHoveredCodon({ featureId: drag.featureId, codonIndex: clamped });
      setTranslationSel((prev) => {
        if (!prev || prev.featureId !== drag.featureId) return prev;
        return { featureId: drag.featureId, startCodon: drag.startCodon, endCodon: clamped };
      });
    },
    [cdsFeatureData, charsPerLine, rowAtSvgY],
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
      if (delta !== 0) {
        if (scroller) scroller.scrollTop += delta;
        else window.scrollBy(0, delta);
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
  }, [scrollContainerRef, updateSeqDragSelection, updateTranslationDrag]);

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
                color: abutColors.feat[f.id] || f.color || ensureReadableColor('#60A5FA'),
              });
            }
          }
        }
        for (const vs of sp(ds.start, ds.end)) {
          const showLabel = !seenRows.has(vs.row);
          seenRows.add(vs.row);
          const segColor =
            abutColors.seg[f.id]?.[di] ||
            dataSegs[di].color ||
            f.color ||
            ensureReadableColor('#60A5FA');
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
              alignLaneInfo.chromBelow[v.row] +
              (((featureRowTracks[f.id] || {})[v.row] || 0) + alignLaneInfo.counts[v.row]) *
                lp.featTrackHeight +
              (alignLaneInfo.counts[v.row] > 0 ? ALIGN_FEAT_GAP : 0);
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
                onMouseMove={(e) => {
                  if (isDraggingRef.current || translationDragRef.current) return;
                  const cds = cdsFeatureData[f.id];
                  if (!cds || !svgRef.current) return;
                  const pt = svgRef.current.createSVGPoint();
                  pt.x = e.clientX;
                  pt.y = e.clientY;
                  const ctm = svgRef.current.getScreenCTM();
                  if (!ctm) return;
                  const svgPt = pt.matrixTransform(ctm.inverse());
                  let col = Math.floor((svgPt.x - startX) / cw);
                  col = Math.max(v.colStart, Math.min(v.colEnd, col));
                  const codon = cds.codonMap.get(v.row * charsPerLine + col);
                  const next =
                    codon === undefined || codon === null
                      ? null
                      : { featureId: f.id, codonIndex: codon };
                  setHoveredCodon((prev) =>
                    prev?.featureId === next?.featureId && prev?.codonIndex === next?.codonIndex
                      ? prev
                      : next,
                  );
                }}
                onMouseLeave={() => {
                  setHoveredCodon(null);
                  featureLeaveRef.current = setTimeout(() => setHoveredFeature(null), 250);
                }}
                onContextMenu={(e) => openFeatureMenu(e, f)}
                onMouseDown={(e) => {
                  if (e.button !== 0) return;
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

                  const [fStart, fEnd] = featureSelRange(f);
                  setSelStart(fStart);
                  setSelEnd(fEnd);
                  setCursorIndex(fEnd + 1);
                  featureSelRef.current = f.orf
                    ? null
                    : { id: f.id, selStart: fStart, selEnd: fEnd };
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
              const cov = visuals.find(
                (v) => v.row === r && c >= v.colStart && c <= v.colEnd && v.type !== 'gap',
              );
              if (!cov) return [];
              const sy = getSeqY(r);
              const rowTo =
                alignLaneInfo.chromBelow[r] +
                (((featureRowTracks[f.id] || {})[r] || 0) + alignLaneInfo.counts[r]) *
                  lp.featTrackHeight +
                (alignLaneInfo.counts[r] > 0 ? ALIGN_FEAT_GAP : 0);
              const y = sy + lp.featBaseOffset + rowTo;
              const isCodonHovered =
                hoveredCodon?.featureId === f.id && hoveredCodon?.codonIndex === t.codonIndex;
              return (
                <text
                  key={`tr-${t.templatePos2}`}
                  x={getX(c) + cw / 2}
                  y={y}
                  fontSize={10}
                  fontWeight="900"
                  fontFamily={monoFont}
                  fill={f.orf ? bgColor : cov.color}
                  stroke={f.orf ? 'none' : bgColor}
                  strokeWidth={f.orf ? 0 : 3}
                  paintOrder="stroke"
                  textAnchor="middle"
                  dominantBaseline="central"
                  style={{ pointerEvents: 'none' }}
                >
                  {isCodonHovered ? t.codonIndex + 1 : t.aa}
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
    hoveredCodon,
    openFeatureMenu,
    abutColors,
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
    const labelsFor = (f) => {
      if (f.orf) return [];
      const isRev = f.strand === '-';
      const isFwd = f.strand === '+';
      const isHovered = hoveredFeature === f.id;
      const { full: fullText, short: shortText } = truncatedLabel(
        f.name,
        isRev,
        isFwd,
        featureLabelsBelow ? Infinity : 12,
      );
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
                rowLabels[vs.row] = {
                  ...vs,
                  color: abutColors.feat[f.id] || f.color || ensureReadableColor('#60A5FA'),
                };
              }
            }
          }
        }
        for (const vs of sp(ds.start, ds.end)) {
          if (!seenRows.has(vs.row) || isRev) {
            seenRows.add(vs.row);
            // label takes the color of the bar segment nearest to it
            rowLabels[vs.row] = {
              ...vs,
              color:
                abutColors.seg[f.id]?.[di] || ds.color || f.color || ensureReadableColor('#60A5FA'),
            };
          }
        }
      }

      return Object.values(rowLabels).map((vs) => {
        const key = `${f.id}-${vs.row}`;
        if (seen.has(key)) return null;
        seen.add(key);
        const labelColor = vs.color;
        const sy = getSeqY(vs.row);
        const rowTo =
          alignLaneInfo.chromBelow[vs.row] +
          (((featureRowTracks[f.id] || {})[vs.row] || 0) + alignLaneInfo.counts[vs.row]) *
            lp.featTrackHeight +
          (alignLaneInfo.counts[vs.row] > 0 ? ALIGN_FEAT_GAP : 0);
        const y = sy + lp.featBaseOffset + rowTo;
        const textProps = {
          y: featureLabelsBelow ? y + 17 : y + 4,
          fontSize: '12px',
          fontFamily: 'TeX Gyre Heros',
          fontWeight: '600',
        };
        if (isRev) {
          const xr = getX(vs.colEnd + 1);
          const lx = featureLabelsBelow ? xr : xr + 8;
          const lAnchor = featureLabelsBelow ? 'end' : 'start';
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
              onContextMenu={(e) => openFeatureMenu(e, f)}
              onMouseDown={(e) => {
                if (e.button !== 0) return;
                e.stopPropagation();
                e.preventDefault();

                const [fStart, fEnd] = featureSelRange(f);
                setSelStart(fStart);
                setSelEnd(fEnd);
                setCursorIndex(fEnd + 1);
                featureSelRef.current = { id: f.id, selStart: fStart, selEnd: fEnd };
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
                x={lx}
                {...textProps}
                textAnchor={lAnchor}
                fill="none"
                stroke={bgColor}
                strokeWidth="5"
              >
                {labelText}
              </text>
              <text x={lx} {...textProps} textAnchor={lAnchor} fill={labelColor} stroke="none">
                {labelText}
              </text>
            </g>
          );
        }
        const x = getX(vs.colStart);
        const lx = featureLabelsBelow ? x : x - 8;
        const lAnchor = featureLabelsBelow ? 'start' : 'end';
        return (
          <g
            key={key}
            onMouseEnter={() => setHoveredFeature(f.id)}
            onMouseLeave={() => setHoveredFeature(null)}
            onContextMenu={(e) => openFeatureMenu(e, f)}
            onMouseDown={(e) => {
              if (e.button !== 0) return;
              e.stopPropagation();
              e.preventDefault();

              const [fStart, fEnd] = featureSelRange(f);
              setSelStart(fStart);
              setSelEnd(fEnd);
              setCursorIndex(fEnd + 1);
              featureSelRef.current = { id: f.id, selStart: fStart, selEnd: fEnd };
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
              x={lx}
              {...textProps}
              textAnchor={lAnchor}
              fill="none"
              stroke={bgColor}
              strokeWidth="5"
            >
              {labelText}
            </text>
            <text x={lx} {...textProps} textAnchor={lAnchor} fill={labelColor} stroke="none">
              {labelText}
            </text>
          </g>
        );
      });
    };
    // Paint order = z-order: labels of the hovered feature render last so
    // they are never occluded by other features' labels.
    const rest = [];
    const hovered = [];
    for (const f of visibleFeatures) {
      (hoveredFeature === f.id ? hovered : rest).push(...labelsFor(f));
    }
    return rest.concat(hovered);
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
    openFeatureMenu,
    featureLabelsBelow,
    abutColors,
  ]);

  const renderedAlignments = useMemo(() => {
    if (!alignmentTracks.length) return null;
    const vs = Math.max(0, visibleRows.start - ROW_BUF);
    const ve = Math.min(numRows - 1, visibleRows.end + ROW_BUF);
    return alignmentTracks.map((al, ti) => {
      const insMap = new Map((al.insertions || []).map((ins) => [ins.pos, ins.bases]));
      const insGroupOf = insertionGroups(al.insertions || []);
      const rows = [];
      const segs = [...(al.segments || []), ...alignmentGapSegments(al, cleanSeq.length)];
      for (const seg of segs) {
        for (const v of sp(seg.start, seg.end)) {
          if (v.row < vs || v.row > ve) continue;
          const sy = getSeqY(v.row);
          const lane = alignLaneInfo.perRow[v.row]?.get(ti) ?? 0;
          const y =
            sy + lp.featBaseOffset + alignLaneInfo.mainChromH + lane * lp.featTrackHeight + 8;
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
              {mismatches.map((col) => {
                const gI = v.row * charsPerLine + col;
                const insAt = insMap.has(gI) ? gI : insMap.has(gI + 1) ? gI + 1 : -1;
                const hot =
                  insAt >= 0 && hoverInsGroup === `${al.id}:${insGroupOf.get(insAt)}`;
                return (
                  <rect
                    key={col}
                    x={getX(col)}
                    y={y - 11}
                    width={cw}
                    height={14}
                    fill={hot ? '#fca5a5' : '#fecaca'}
                    fillOpacity={hot ? 0.95 : 0.6}
                    style={{ pointerEvents: 'none' }}
                  />
                );
              })}
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
                      <InsDot
                        key={col}
                        x={getX(col) + cw / 2}
                        onMouseEnter={() => setHoverInsGroup(`${al.id}:${insGroupOf.get(insPos)}`)}
                        onMouseLeave={() => setHoverInsGroup(null)}
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
                      />
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
    cleanSeq.length,
    hoverInsGroup,
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
            // Labels sit a few px above the lane baseline to visually align
            // with the alignment text track.
            const y =
              sy + lp.featBaseOffset + alignLaneInfo.mainChromH + lane * lp.featTrackHeight + 6.5;
            const hKey = `${al.id}:${v.row}`;
            const labelHover = hoverAlignLabel === hKey;
            const hovered = truncated && labelHover;
            // Fixed position right of the row's last column (the empty right
            // margin), so labels never overlap read chars or gap dashes.
            const labelX = getX(charsPerLine) + 8;
            const clipId = `align-label-clip-${al.id}-${v.row}`;
            const scrollW = hovered ? featLabelW(al.name) - featLabelW(short) + 4 : 0;
            // Trace toggle: labels of alignments whose .ab1 resolved are
            // underlined and clickable; the expanded track's label is inverted
            // (rect in label color, text in bgColor).
            const traceable = !!alignmentTraceAvailable?.has(al.id);
            const expanded = al.id === expandedChromAlnId;
            const labelText = hovered ? al.name : short;
            const labelW = Math.min(featLabelW(labelText), LABEL_MAX_W);
            const textEl = (
              <text
                x={labelX}
                y={y}
                textAnchor="start"
                {...textProps}
                fill={expanded ? bgColor : textProps.fill}
                style={{
                  userSelect: 'none',
                  pointerEvents: 'auto',
                  textDecoration: traceable ? 'underline' : undefined,
                  ...(hovered
                    ? {
                        '--align-label-scroll': `-${scrollW}px`,
                        animation: 'alignLabelScroll 2.5s linear infinite alternate',
                      }
                    : {}),
                }}
                onClick={
                  traceable && onToggleAlignmentChrom
                    ? () => onToggleAlignmentChrom(al.id)
                    : undefined
                }
              >
                {labelText}
              </text>
            );
            return (
              <g
                key={v.row}
                style={{ cursor: traceable ? 'pointer' : 'default' }}
                onMouseEnter={() => setHoverAlignLabel(hKey)}
                onMouseLeave={() => setHoverAlignLabel(null)}
              >
                {truncated && (
                  <defs>
                    <clipPath id={clipId}>
                      <rect x={labelX} y={y - 12} width={LABEL_MAX_W} height={16} />
                    </clipPath>
                  </defs>
                )}
                {expanded && (
                  <rect
                    x={labelX - 4}
                    y={y - 12}
                    width={labelW + 16}
                    height={16}
                    fill={textProps.fill}
                  />
                )}
                {/* Invisible hit area bridging label and eye-off icon, so the
                    icon stays reachable while the pointer moves toward it. */}
                <rect
                  x={labelX - 4}
                  y={y - 12}
                  width={labelW + (expanded ? 44 : 34)}
                  height={16}
                  fill="transparent"
                />
                {truncated ? <g clipPath={`url(#${clipId})`}>{textEl}</g> : textEl}
                {labelHover && onHideAlignment && (
                  <EyeOff
                    x={labelX + labelW + (expanded ? 20 : 10)}
                    y={y - 11}
                    width={13}
                    height={13}
                    color={textProps.fill}
                    style={{ cursor: 'pointer' }}
                    onClick={(e) => {
                      e.stopPropagation();
                      onHideAlignment(al.id);
                    }}
                  />
                )}
              </g>
            );
          })}
        </g>
      );
    });
  }, [
    alignmentTracks,
    visibleRows,
    numRows,
    sp,
    getSeqY,
    lp,
    alignLaneInfo,
    hoverAlignLabel,
    alignmentTraceAvailable,
    expandedChromAlnId,
    onToggleAlignmentChrom,
    onHideAlignment,
    charsPerLine,
  ]);

  // Chromatogram bands: the project's own trace directly under the top
  // strand (ab1 source files), and one warped trace band per alignment
  // with loaded trace data, placed below the alignment text lanes. Trace
  // samples are interpolated between peak anchors so every base's peak sits
  // on its own column; read gaps (deletions) break the polyline.
  // Band rendering ported from GenePad (https://github.com/GenePad),
  // provided by the GenePad team / https://github.com/Masterchiefm.
  const renderedChromatograms = useMemo(() => {
    const hasAlignChrom = alignmentTracks.some((al) => alignmentChromatograms[al.id]);
    if (!chromatogram && !hasAlignChrom) return null;
    const vs = Math.max(0, visibleRows.start - ROW_BUF);
    const ve = Math.min(numRows - 1, visibleRows.end + ROW_BUF);
    const bands = [];

    const renderBand = (key, chrom, anchors, y) => {
      if (anchors.length === 0) return;
      const peaks = chrom.peakLocations;
      const p0 = Math.max(0, (peaks[anchors[0].q] ?? 0) - 14);
      const p1 = (peaks[anchors[anchors.length - 1].q] ?? 0) + 14;
      const maxVal = traceRangeMax(chrom, p0, p1);
      if (maxVal <= 0) return;
      const baseY = y + CHROM_TRACK_H - 4;
      const scaleY = (CHROM_TRACK_H - 8) / maxVal;
      bands.push(
        <g key={key}>
          <line
            x1={anchors[0].x - cw / 2}
            x2={anchors[anchors.length - 1].x + cw / 2}
            y1={baseY}
            y2={baseY}
            stroke="#d6d3d1"
            strokeWidth="1"
          />
          {TRACE_CHANNELS.map(([base, channelKey, color]) => (
            <path
              key={base}
              d={buildTracePath(chrom, channelKey, anchors, baseY, scaleY)}
              fill="none"
              stroke={color}
              strokeWidth="1"
              strokeLinejoin="round"
            />
          ))}
        </g>,
      );
    };

    if (chromatogram) {
      const peakCount = chromatogram.peakLocations.length;
      for (let r = vs; r <= ve; r++) {
        const rowStart = r * charsPerLine;
        const rowEnd = Math.min(cleanSeq.length, (r + 1) * charsPerLine, peakCount) - 1;
        if (rowEnd < rowStart) continue;
        const anchors = [];
        for (let pos = rowStart; pos <= rowEnd; pos++) {
          anchors.push({ x: getX(pos - rowStart) + cw / 2, q: pos });
        }
        renderBand(`chrom-main-${r}`, chromatogram, anchors, getSeqY(r) + lp.featBaseOffset);
      }
    }

    alignmentTracks.forEach((al, ti) => {
      const chrom = alignmentChromatograms[al.id];
      if (!chrom) return;
      const colMap = buildColumnQueryMap(al);
      const byRow = new Map();
      for (const [pos, q] of colMap) {
        const row = Math.floor(pos / charsPerLine);
        if (row < vs || row > ve) continue;
        if (!byRow.has(row)) byRow.set(row, []);
        byRow.get(row).push({ x: getX(pos - row * charsPerLine) + cw / 2, q });
      }
      for (const [row, anchors] of byRow) {
        anchors.sort((a, b) => a.x - b.x);
        const lane = alignLaneInfo.chromPerRow[row]?.get(ti) ?? 0;
        const y =
          getSeqY(row) +
          lp.featBaseOffset +
          alignLaneInfo.mainChromH +
          alignLaneInfo.counts[row] * lp.featTrackHeight +
          2 +
          lane * (CHROM_TRACK_H + CHROM_GAP);
        renderBand(`chrom-${al.id}-${row}`, chrom, anchors, y);
      }
    });

    if (!bands.length) return null;
    return <g style={{ pointerEvents: 'none' }}>{bands}</g>;
  }, [
    chromatogram,
    alignmentTracks,
    alignmentChromatograms,
    visibleRows,
    numRows,
    charsPerLine,
    cleanSeq.length,
    getSeqY,
    lp,
    alignLaneInfo,
  ]);

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
        <g key={p.id} onContextMenu={(e) => openPrimerMenu(e, p)}>
          {segs.map((seg) => {
            const isTail = seg === tailSeg,
              isArrow = seg === arrowSeg;
            const sy = getSeqY(seg.row);
            const featOff =
              (isFwd ? 0 : (revPrimerFeatOffsets[p.id] || {})[seg.row] || 0) +
              (isFwd
                ? 0
                : alignLaneInfo.chromBelow[seg.row] +
                  (alignLaneInfo.counts[seg.row] > 0
                    ? alignLaneInfo.counts[seg.row] * lp.featTrackHeight + ALIGN_FEAT_GAP
                    : 0));
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
    openPrimerMenu,
    alignLaneInfo,
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
        const enzLift =
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
        const yTop = Math.max(sy - lp.enzLabelBase - enzLift, minTop);
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
          onContextMenu={(e) => openEnzymeMenu(e, l)}
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

            // Start enzyme drag — immediately select the recognition site.
            // Sites wrapping the origin of a circular sequence (recEnd beyond
            // the sequence length) fall back to the display window.
            const recWraps = enzyme.recStart == null || enzyme.recEnd >= cleanSeq.length;
            const recSelStart = recWraps ? enzyme.displayStart : enzyme.recStart;
            const recSelEnd = recWraps ? enzyme.displayEnd : enzyme.recEnd;
            setSelStart(recSelStart);
            setSelEnd(recSelEnd);
            setCursorIndex(null);
            enzymeDragRef.current = {
              active: true,
              startEnzymeId: l.groupId,
              startName: l.name,
              startCutIdx: cutIdx,
              recStart: recSelStart,
              recEnd: recSelEnd,
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
    cleanSeq,
    openEnzymeMenu,
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
    if (topology === 'circular' && fwdPrimer.matchEnd >= revPrimer.matchStart) {
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
  }, [
    selectionMode,
    selectedPrimerIds,
    enrichedPrimers,
    cleanSeq,
    charsPerLine,
    getSeqY,
    sp,
    topology,
  ]);

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
    rowAbove,
    rowBelow,
    selectionMode,
    currentSelColor,
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
  }, [hoveredIndex, isDragging, isTranslationDragging, charsPerLine, getSeqY]);

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
    let label = `${len} ${seqUnit}`;
    if (isDna && showTm) label += `, ${tm}°C`;
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
    seqUnit,
    isDna,
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
      ) : agentLocked ? (
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
            <Bot className="size-4 shrink-0 text-teal-700" />
            <span className="text-sm font-semibold text-teal-800">Controlled by an MCP agent</span>
            <span className="text-xs text-muted-foreground">
              Viewing and selecting work as usual; editing is disabled
            </span>
            <button
              type="button"
              onClick={onUnlockAgent}
              className="rounded-full bg-muted/60 px-3 py-1 text-sm text-foreground/80 transition-colors hover:bg-accent hover:text-foreground"
            >
              <span className="flex items-center gap-1.5">
                <LockOpen className="size-3.5" />
                Unlock
              </span>
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
          onOpenMapView={onOpenMapView}
          onPrimerDesign={handlePrimerDesign}
          primerDesignEnabled={primerDesignEnabled}
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
          onToggleTopology={onToggleTopology}
          moleculeType={moleculeType}
        />
      )}
      <div
        ref={containerRef}
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
            {/* rna/protein are single-strand: no alignment/enzyme/primer layers */}
            {isDna && renderedAlignments}
            {isDna && renderedAlignmentLabels}
            {isDna && renderedChromatograms}
            {renderedFeatures}
            {renderedFeatureLabels}
            {isDna && renderedEnzymes}
            {isDna && renderedEnzymeLabels}
            {isDna && renderedEnzymeOverlay}
            {/* Primers paint above enzyme labels: their labels are avoided by
                reservation, and expanded (hover/selected) blocks cleanly
                occlude low-lying enzyme labels instead of interleaving. */}
            {isDna && renderedPrimers}
            {renderedSeqBg}
            {renderedSeqSel}
            {isDna && renderedTranslationSelection}
            {isDna && renderedAmplimerRegion}
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
          moleculeType={moleculeType}
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
