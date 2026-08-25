const STORAGE_KEY = 'clipboardAnnotations';

export function collectAnnotations({ features, primers }, start, end, totalLen = 0) {
  const collectedFeatures = [];
  const collectedPrimers = [];

  // start > end wraps the origin of a circular sequence: two pieces, with
  // output offsets into the concatenated copied text.
  const pieces =
    start <= end
      ? [[start, end, 0]]
      : [
          [start, totalLen - 1, 0],
          [0, end, totalLen - start],
        ];
  const outLen = start <= end ? end - start + 1 : totalLen - start + end + 1;

  if (features) {
    for (const f of features) {
      const segs =
        f.segments && f.segments.length > 0 ? f.segments : [{ start: f.start, end: f.end }];
      let hasOverlap = false;
      const clippedSegments = [];
      for (const [ps, pe, off] of pieces) {
        for (const seg of segs) {
          const s = Math.max(seg.start, ps);
          const e = Math.min(seg.end, pe);
          if (s <= e) {
            hasOverlap = true;
            clippedSegments.push({ start: s - ps + off, end: e - ps + off });
          }
        }
      }
      if (hasOverlap) {
        const feat = {
          name: f.name,
          ftype: f.ftype,
          color: f.color,
          strand: f.strand,
          notes: f.notes,
          qualifiers: f.qualifiers,
          segments: clippedSegments,
        };
        collectedFeatures.push(feat);
      }
    }
  }

  if (primers) {
    for (const p of primers) {
      const bs = p.bindingSites?.[0];
      if (!bs) continue;
      const matchStart = bs.templateStart ?? bs.matchStart;
      const matchEnd = bs.templateEnd != null ? bs.templateEnd - 1 : bs.matchEnd;
      if (matchStart == null || matchEnd == null) continue;
      if (pieces.some(([ps, pe]) => matchStart <= pe && matchEnd >= ps)) {
        collectedPrimers.push({ name: p.name, type: p.type, primerSeq: p.primerSeq });
      }
    }
  }

  if (collectedFeatures.length === 0 && collectedPrimers.length === 0) return null;

  return {
    app: 'libregene',
    version: 1,
    length: outLen,
    features: collectedFeatures,
    primers: collectedPrimers,
  };
}

export async function writeAnnotatedClipboard(text, meta) {
  if (meta) {
    try {
      await navigator.clipboard.write([
        new ClipboardItem({
          'text/plain': new Blob([text], { type: 'text/plain' }),
          'web application/x-libregene-annotations': new Blob([JSON.stringify(meta)], {
            type: 'application/json',
          }),
        }),
      ]);
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ text, meta }));
      return;
    } catch {
      // fall through to writeText
    }
  }
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // ignore
  }
  if (meta) {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ text, meta }));
  } else {
    localStorage.removeItem(STORAGE_KEY);
  }
}

export function readClipboardMeta(pastedText) {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const { text, meta } = JSON.parse(raw);
    if (!meta || !text) return null;
    if (text.replace(/\s/g, '') === pastedText.replace(/\s/g, '')) return meta;
  } catch {
    // ignore
  }
  return null;
}

/**
 * True when `text` is equivalent (whitespace/case-insensitive) to the sequence
 * the given clipboard meta was collected from. Uses the localStorage record
 * when the meta is the one stored there; otherwise falls back to
 * `fallbackText` (the text that accompanied the meta, e.g. from the same
 * paste event).
 */
export function metaMatchesText(meta, text, fallbackText) {
  if (!meta) return false;
  const norm = (s) => (s || '').replace(/\s/g, '').toUpperCase();
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const { text: storedText, meta: storedMeta } = JSON.parse(raw);
      if (storedMeta && storedText && JSON.stringify(storedMeta) === JSON.stringify(meta)) {
        return norm(storedText) === norm(text);
      }
    }
  } catch {
    // ignore
  }
  if (fallbackText != null) {
    return norm(fallbackText) === norm(text) && norm(text).length === meta.length;
  }
  return norm(text).length === meta.length;
}

export function parseMetaFromPasteEvent(e) {
  try {
    const raw = e.clipboardData?.getData('application/x-libregene-annotations');
    if (!raw) return null;
    const meta = JSON.parse(raw);
    if (meta && meta.app === 'libregene') return meta;
  } catch {
    // ignore
  }
  return null;
}
