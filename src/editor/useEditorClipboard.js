import { useCallback, useEffect } from 'react';
import { sliceRange } from '../editorConstants';
import { matchedSeqOf, reverseComplement } from './seqUtils';
import {
  collectAnnotations,
  parseMetaFromPasteEvent,
  readClipboardMeta,
  writeAnnotatedClipboard,
} from '../clipboardAnnotations';

export default function useEditorClipboard({
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
}) {
  const writeClipboard = useCallback((text) => {
    if (text) navigator.clipboard.writeText(text).catch(() => {});
  }, []);

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
        writeClipboard(aa);
        return;
      }

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
            const meta = collectAnnotations(
              { features, primers },
              positions[0],
              positions[positions.length - 1],
            );
            writeAnnotatedClipboard(text, meta);
          } else {
            writeClipboard(text);
          }
          return;
        }
      }

      if (!hasSelection) return;
      const sense = sliceRange(cleanSeq, selStart, selEnd);
      const text = mode === 'antisense' ? reverseComplement(sense) : sense;
      if (mode === 'sense') {
        const meta = collectAnnotations({ features, primers }, selStart, selEnd, cleanSeq.length);
        writeAnnotatedClipboard(text, meta);
      } else {
        writeClipboard(text);
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
      writeClipboard,
    ],
  );

  const copyAmplimer = useCallback(() => {
    if (selectedPrimerIds.length !== 2) return;
    const fp = enrichedPrimers.find((p) => p.id === selectedPrimerIds[0]);
    const rp = enrichedPrimers.find((p) => p.id === selectedPrimerIds[1]);
    const fwdPrimer = fp && fp.isFwd ? fp : rp;
    const revPrimer = fp && !fp.isFwd ? fp : rp;
    if (!fwdPrimer || !revPrimer) return;
    if (topology !== 'circular' && fwdPrimer.matchEnd >= revPrimer.matchStart) return;
    const fSeq = fwdPrimer.primerSeq || matchedSeqOf(fwdPrimer, cleanSeq);
    const rSeq = revPrimer.primerSeq || matchedSeqOf(revPrimer, cleanSeq);
    const intervening =
      fwdPrimer.matchEnd < revPrimer.matchStart
        ? cleanSeq.substring(fwdPrimer.matchEnd + 1, revPrimer.matchStart)
        : cleanSeq.substring(fwdPrimer.matchEnd + 1) + cleanSeq.substring(0, revPrimer.matchStart);
    writeClipboard(fSeq + intervening + reverseComplement(rSeq));
  }, [selectedPrimerIds, enrichedPrimers, cleanSeq, writeClipboard, topology]);

  const requestPaste = useCallback(
    (clipboardText, clipboardMeta = null) => {
      if (!clipboardText || !onEditRequest || selWraps) return;
      if (hasTranslationSelection) {
        const range = translationBaseRange();
        if (!range) return;
        const [minPos, maxPos] = range;
        onEditRequest({
          type: 'replace',
          cursorIndex: minPos,
          selStart: minPos,
          selEnd: maxPos,
          selectedText: cleanSeq.substring(minPos, maxPos + 1),
          clipboardText,
          clipboardMeta,
        });
        return;
      }
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
        onEditRequest({ type: 'insert', cursorIndex, clipboardText, clipboardMeta });
      }
    },
    [
      onEditRequest,
      hasSelection,
      hasTranslationSelection,
      translationBaseRange,
      selWraps,
      cursorIndex,
      selStart,
      selEnd,
      cleanSeq,
    ],
  );

  const pasteFromClipboard = useCallback(() => {
    navigator.clipboard
      .readText()
      .then((t) => {
        if (!t) return;
        requestPaste(t, readClipboardMeta(t));
      })
      .catch(() => {});
  }, [requestPaste]);

  const convertSelectionCase = useCallback(
    (toUpper) => {
      if (hasTranslationSelection) {
        const range = translationBaseRange();
        if (!range || !onEditRequest) return;
        const [minPos, maxPos] = range;
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
      translationBaseRange,
      onEditRequest,
      cleanSeq,
      selStart,
      selEnd,
    ],
  );

  useEffect(() => {
    if (!onEditRequest || hidden) return undefined;
    const onPaste = (e) => {
      const tag = e.target?.tagName?.toLowerCase();
      if (tag === 'input' || tag === 'textarea' || e.target?.isContentEditable) return;
      const clipboardText = e.clipboardData?.getData('text') || '';
      if (!clipboardText) return;
      const meta = parseMetaFromPasteEvent(e) || readClipboardMeta(clipboardText);
      e.preventDefault();
      requestPaste(clipboardText, meta);
    };
    window.addEventListener('paste', onPaste);
    return () => window.removeEventListener('paste', onPaste);
  }, [onEditRequest, requestPaste, hidden]);

  return {
    copySelection,
    copyAmplimer,
    writeClipboard,
    pasteFromClipboard,
    toUppercase: useCallback(() => convertSelectionCase(true), [convertSelectionCase]),
    toLowercase: useCallback(() => convertSelectionCase(false), [convertSelectionCase]),
  };
}
