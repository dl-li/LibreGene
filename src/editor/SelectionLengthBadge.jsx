import { useEffect, useState } from 'react';
import { matchedSeqOf, reverseComplement } from './seqUtils';
import { sliceRange, rangeLen, bgColor, peptideMassKda } from '../editorConstants';
import { NavStateCapsule } from '../components/NavStateNotice';

// ---------------------------------------------------------------------------
// SelectionLengthBadge — capsule above the navigation bar showing selection
// length and GC% for nucleic acids or peptide molecular weight for proteins.
// ---------------------------------------------------------------------------
function getSelectionCapsule({
  selectionMode,
  isEnzymeSelection,
  selStart,
  selEnd,
  cleanSeq,
  selectedPrimerIds,
  enrichedPrimers,
  enzymeActiveBlue,
  amplimerGreen,
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

  // GC% for nucleic acids, molecular weight for peptides.
  let line2 = null;
  if (showGc) {
    const gc = (seqToCopy.match(/[GC]/gi) || []).length;
    const gcPct = seqToCopy.length > 0 ? Math.round((gc / seqToCopy.length) * 100) : 0;
    line2 = `${gcPct}% GC`;
  } else if (showMw) {
    const kda = peptideMassKda(seqToCopy);
    line2 = `${kda < 1 ? kda.toFixed(2) : kda.toFixed(1)} kDa`;
  }

  const text = `${len} ${unit}${line2 ? ` · ${line2}` : ''}`;
  return { key: `${bg}|${text}`, bg, text };
}

function SelectionLengthBadge(props) {
  const selection = getSelectionCapsule(props);
  const [displayedSelection, setDisplayedSelection] = useState(selection);
  const [leaving, setLeaving] = useState(false);

  if (selection && (selection.key !== displayedSelection?.key || leaving)) {
    setDisplayedSelection(selection);
    setLeaving(false);
  } else if (!selection && displayedSelection && !leaving) {
    setLeaving(true);
  }

  useEffect(() => {
    if (!leaving) return undefined;
    const timer = setTimeout(() => {
      setDisplayedSelection(null);
      setLeaving(false);
    }, 180);
    return () => clearTimeout(timer);
  }, [leaving]);

  if (!displayedSelection) return null;

  const { bg, text } = displayedSelection;
  const coveredByNavNotice = !!props.coveredByNavNotice;
  return (
    <div className="pointer-events-none fixed bottom-[4.375rem] left-1/2 z-30 -translate-x-1/2">
      <div
        aria-hidden={coveredByNavNotice || leaving}
        data-leaving={leaving}
        className="selection-presence-notice"
      >
        <NavStateCapsule
          leaving={coveredByNavNotice}
          motionClassName="selection-coverage"
          style={{
            backgroundColor: bg,
            backgroundImage: 'none',
            color: bgColor,
            boxShadow: `inset 0 1px 0 rgba(255,255,255,0.14), 0 10px 26px -10px ${bg}61, 0 4px 12px ${bg}29`,
          }}
        >
          {text}
        </NavStateCapsule>
      </div>
    </div>
  );
}

export default SelectionLengthBadge;
