/**
 * Map alignment models onto an edited sequence without re-aligning.
 *
 * An edit removes the inclusive template column range `[editStart, editEnd]`
 * (`oldLen` columns) and inserts `newLen` bases at `editStart`. Features and
 * primers already get this treatment locally (`adjustAnnotations`) so the
 * editor stays coherent while the backend recomputes; alignments were left
 * alone, which meant the lanes were drawn from the old model against the new
 * sequence until the recomputed one arrived — a visible scramble for as long
 * as the recompute takes.
 *
 * Columns after the edit shift by the delta; columns inside it disappear. The
 * read bases they held, plus the insertions anchored inside the range, are
 * merged back into one insertion at the edit point — in read order, so a
 * deletion of a stretch the read covers comes out exactly like the model the
 * aligner would produce for a template deletion. New columns the read cannot
 * cover show up as a gap inside a segment that spans the edit, or simply as
 * uncovered columns when it does not.
 *
 * The read's bases are preserved, in read order — the invariant the display
 * walk (and the chromatogram numbering riding on it) depends on.
 */
export function adjustAlignmentsForEdit(alignments, editStart, editEnd, oldLen, newLen) {
  if (!alignments || !alignments.length) return alignments;
  const delta = newLen - oldLen;
  // Equal-length edits (a substitution, or a no-op) leave every coordinate
  // where it is: the template's letters changed, not the layout, so the read's
  // bases stay on their columns and simply show up as mismatches.
  if (delta === 0) return alignments;

  return alignments.map((al) => {
    const removedBases = new Map(); // removed column -> read base
    const segments = [];
    let changed = false;
    for (const seg of al.segments || []) {
      const s = seg.start;
      const e = seg.end;
      const chars = seg.chars || '';
      if (e < editStart) {
        segments.push(seg);
        continue;
      }
      if (s > editEnd) {
        changed = true;
        segments.push({ ...seg, start: s + delta, end: e + delta });
        continue;
      }
      // Columns before the edit keep their place, columns after it shift, and
      // the removed ones in between give up their read bases.
      const preLen = Math.max(0, Math.min(e, editStart - 1) - s + 1);
      const sufLen = Math.max(0, e - Math.max(s, editEnd + 1) + 1);
      const pre = chars.slice(0, preLen);
      const mid = chars.slice(preLen, chars.length - sufLen);
      const suf = chars.slice(chars.length - sufLen);
      for (let k = 0; k < mid.length; k++) {
        if (mid[k] !== '-') removedBases.set(s + preLen + k, mid[k]);
      }
      changed = true;
      if (!pre && !suf) continue;
      const parts = [];
      if (pre) parts.push(pre);
      // A segment spanning the edit keeps its two flanks adjacent across the
      // inserted columns, which the read has no bases for.
      if (pre && suf && newLen > 0) parts.push('-'.repeat(newLen));
      if (suf) parts.push(suf);
      segments.push({
        ...seg,
        start: pre ? s : editStart + newLen,
        end: suf ? e + delta : editStart - 1,
        chars: parts.join(''),
      });
    }

    // Contents that end up at the same anchor must be merged in read order:
    // an insertion's bases sit just before its own column, and the removed
    // stretch's bases span its columns, so ordering by the original position
    // (the removed stretch keyed at its first column) keeps the read intact.
    const at = [];
    for (const ins of al.insertions || []) {
      if (ins.pos < editStart) {
        at.push({ newPos: ins.pos, key: ins.pos - 0.5, bases: ins.bases });
      } else if (ins.pos > editEnd) {
        changed = true;
        at.push({ newPos: ins.pos + delta, key: ins.pos - 0.5, bases: ins.bases });
      } else {
        changed = true;
        at.push({ newPos: editStart, key: ins.pos - 0.5, bases: ins.bases, inside: true });
      }
    }
    if (removedBases.size || at.some((e) => e.inside)) {
      changed = true;
      // Read order across the removed range: at each column, an insertion
      // anchored there comes first, then that column's base.
      let merged = '';
      for (let c = editStart; c <= editEnd; c++) {
        for (const e of at) if (e.inside && e.key === c - 0.5) merged += e.bases;
        if (removedBases.has(c)) merged += removedBases.get(c);
      }
      at.push({ newPos: editStart, key: editStart, bases: merged, merged: true });
    }
    at.sort((a, b) => a.newPos - b.newPos || a.key - b.key);
    const insertions = [];
    for (const e of at) {
      if (e.inside && !e.merged) continue; // folded into the merged stretch
      const last = insertions[insertions.length - 1];
      if (last && last.pos === e.newPos) last.bases += e.bases;
      else insertions.push({ pos: e.newPos, bases: e.bases });
    }
    insertions.sort((a, b) => a.pos - b.pos);

    return changed ? { ...al, segments, insertions } : al;
  });
}
