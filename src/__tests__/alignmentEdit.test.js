// Unit tests for the local (pre-recompute) mapping of alignment models onto
// an edited sequence.
import { describe, it, expect } from 'vitest';
import { adjustAlignmentsForEdit } from '../alignmentEdit';

const seq = (len) => 'ACGT'.repeat(Math.ceil(len / 4)).slice(0, len);

describe('adjustAlignmentsForEdit', () => {
  const plain = {
    id: 'aln-1',
    name: 'read',
    strand: '+',
    seq: seq(60),
    segments: [{ start: 0, end: 59, chars: seq(60) }],
    insertions: [],
  };

  it('shifts what follows a deletion and rides the removed bases as an insertion', () => {
    // Delete columns 20..29 of a fully covered read.
    const [aln] = adjustAlignmentsForEdit([plain], 20, 29, 10, 0);
    expect(aln.segments).toHaveLength(1);
    expect(aln.segments[0].start).toBe(0);
    expect(aln.segments[0].end).toBe(49);
    expect(aln.segments[0].chars.length).toBe(50);
    // Columns 20..29 are gone from the template, so the read keeps their bases
    // as one insertion at the edit point (displayed left of column 20).
    expect(aln.insertions).toEqual([{ pos: 20, bases: seq(60).slice(20, 30) }]);
  });

  it('leaves equal-length substitutions alone', () => {
    // Same coordinates before and after: the read's bases stay put and show up
    // as mismatches against the new letters.
    expect(adjustAlignmentsForEdit([plain], 20, 29, 10, 10)).toEqual([plain]);
    expect(adjustAlignmentsForEdit([plain], 20, 19, 0, 0)).toEqual([plain]);
  });

  it('leaves models that end before the edit alone and shifts the rest', () => {
    const before = { ...plain, id: 'a', segments: [{ start: 0, end: 19, chars: seq(20) }] };
    const after = { ...plain, id: 'b', segments: [{ start: 40, end: 59, chars: seq(20) }] };
    const [b, a] = adjustAlignmentsForEdit([before, after], 20, 29, 10, 0);
    expect(b).toBe(before);
    expect(a.segments[0]).toEqual({ start: 30, end: 49, chars: seq(20) });
  });

  it('shows inserted columns the read cannot cover as a gap in a spanning segment', () => {
    // Paste 4 bases at column 30.
    const [aln] = adjustAlignmentsForEdit([plain], 30, 29, 0, 4);
    expect(aln.segments[0].start).toBe(0);
    expect(aln.segments[0].end).toBe(63);
    expect(aln.segments[0].chars).toBe(seq(30) + '----' + seq(60).slice(30));
    expect(aln.insertions).toEqual([]);
  });

  it('re-anchors insertions whose column was removed and shifts the later ones', () => {
    const aln = {
      ...plain,
      insertions: [
        { pos: 10, bases: 'GG' },
        { pos: 25, bases: 'TTT' },
        { pos: 40, bases: 'C' },
      ],
    };
    const [out] = adjustAlignmentsForEdit([aln], 20, 29, 10, 0);
    const pos = out.insertions.map((i) => i.pos);
    expect(pos).toEqual([10, 20, 30]);
    // Read order across the removed range: the removed columns' bases with the
    // insertion that sat between them spliced back at its own column.
    expect(out.insertions[1].bases).toBe(seq(60).slice(20, 25) + 'TTT' + seq(60).slice(25, 30));
    expect(out.insertions[2].bases).toBe('C');
  });

  it('keeps every read base, in read order', () => {
    // A consistent model: the walk rebuilds the read exactly.
    const read = seq(60).slice(0, 30) + 'GGG' + seq(60).slice(30);
    const model = {
      id: 'aln-2',
      name: 'read',
      strand: '+',
      seq: read,
      segments: [{ start: 0, end: 59, chars: seq(60) }],
      insertions: [{ pos: 30, bases: 'GGG' }],
    };
    const walkRead = (aln) => {
      const insByPos = new Map((aln.insertions || []).map((i) => [i.pos, i.bases]));
      const used = new Set();
      let out = '';
      for (const seg of aln.segments || []) {
        for (let k = 0; k < (seg.chars || '').length; k++) {
          const col = seg.start + k;
          if (insByPos.has(col) && !used.has(col)) {
            used.add(col);
            out += insByPos.get(col);
          }
          if (seg.chars[k] !== '-') out += seg.chars[k];
        }
      }
      for (const [pos, bases] of insByPos) if (!used.has(pos)) out += bases;
      return out;
    };
    expect(walkRead(model)).toBe(read);
    for (const [s, e, len] of [
      [20, 29, 0],
      [30, 29, 4],
      [0, 9, 0],
      [50, 59, 12],
      [25, 35, 10],
    ]) {
      const [out] = adjustAlignmentsForEdit([model], s, e, Math.max(0, e - s + 1), len);
      expect(walkRead(out), `edit ${s}..${e} -> ${len}`).toBe(read);
    }
  });
});
