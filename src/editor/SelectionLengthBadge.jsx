import { matchedSeqOf, reverseComplement } from './seqUtils';
import {
  measureWidth,
  monoFont,
  sliceRange,
  rangeLen,
  bgColor,
  peptideMassKda,
} from '../editorConstants';

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

export default SelectionLengthBadge;
