// Unit tests for the alignment→chromatogram anchor mapping.
import { describe, it, expect } from 'vitest';
import { buildColumnAnchors, buildColumnQueryMap, buildTracePath } from '../chromatogram';
import { buildStreamLayout } from '../editor/alignmentLayout';

describe('buildColumnAnchors', () => {
  it('maps a simple single-segment alignment 1:1', () => {
    const aln = {
      segments: [{ start: 10, end: 14, chars: 'ACGTA' }],
      insertions: [],
    };
    const entries = buildColumnAnchors(aln);
    expect(entries.map((e) => [e.col, e.q, e.ins])).toEqual([
      [10, 0, false],
      [11, 1, false],
      [12, 2, false],
      [13, 3, false],
      [14, 4, false],
    ]);
  });

  it('emits insertion entries before their anchor column', () => {
    // Two bases inserted before template column 12; column 12 has a deletion
    // ('-' consumes no read base).
    const aln = {
      segments: [{ start: 10, end: 14, chars: 'AC-TA' }],
      insertions: [{ pos: 12, bases: 'GG' }],
    };
    const entries = buildColumnAnchors(aln);
    expect(entries.map((e) => [e.col, e.q, e.ins])).toEqual([
      [10, 0, false],
      [11, 1, false],
      [12, 2, true],
      [12, 3, true],
      [13, 4, false],
      [14, 5, false],
    ]);
    // The query map skips insertion and deletion columns.
    expect([...buildColumnQueryMap(aln)]).toEqual([
      [10, 0],
      [11, 1],
      [13, 4],
      [14, 5],
    ]);
  });

  it('appends a trailing insertion whose column is never walked', () => {
    // Read tail anchored after the last segment's end: the walk never
    // reaches column 20, so the tail entries are appended at the end.
    const aln = {
      segments: [{ start: 10, end: 14, chars: 'ACGTA' }],
      insertions: [{ pos: 20, bases: 'CCC' }],
    };
    const entries = buildColumnAnchors(aln);
    expect(entries.slice(-3).map((e) => [e.col, e.q, e.ins])).toEqual([
      [20, 5, true],
      [20, 6, true],
      [20, 7, true],
    ]);
  });

  it('flags a display discontinuity before the base after a read deletion', () => {
    // Column 11 is a deletion in the read: the display shows a dash there, so
    // the trace must break before column 12 — and only there.
    const aln = { segments: [{ start: 10, end: 14, chars: 'AC-TA' }], insertions: [] };
    expect(buildColumnAnchors(aln).map((e) => [e.col, e.brk])).toEqual([
      [10, false],
      [11, false],
      [13, true],
      [14, false],
    ]);
  });

  it('does not break for insertions, and breaks across a segment jump', () => {
    const aln = {
      segments: [
        { start: 10, end: 11, chars: 'AC' },
        { start: 40, end: 41, chars: 'GT' },
      ],
      insertions: [{ pos: 12, bases: 'GG' }],
    };
    expect(buildColumnAnchors(aln).map((e) => [e.col, e.ins, e.brk])).toEqual([
      [10, false, false],
      [11, false, false],
      [12, true, false],
      [12, true, false],
      [40, false, true],
      [41, false, false],
    ]);
  });

  it('breaks the trace exactly where the anchors flag it, in read order', () => {
    const chrom = {
      traceA: [0, 1, 2, 3, 4, 5, 6, 7],
      traceC: [0, 0, 0, 0, 0, 0, 0, 0],
      traceG: [0, 0, 0, 0, 0, 0, 0, 0],
      traceT: [0, 0, 0, 0, 0, 0, 0, 0],
      peakLocations: [0, 1, 2, 3, 4, 5, 6, 7],
    };
    const subpaths = (anchors) =>
      (buildTracePath(chrom, 'traceA', anchors, 10, 1).match(/M/g) || []).length;
    // One continuous run — including a minus-strand stretch, which runs right
    // to left in read order (x decreasing base by base).
    expect(
      subpaths([
        { x: 60, q: 0 },
        { x: 48, q: 1 },
        { x: 36, q: 2 },
      ]),
    ).toBe(1);
    // A flagged deletion starts a new subpath.
    expect(
      subpaths([
        { x: 0, q: 0 },
        { x: 12, q: 1 },
        { x: 60, q: 2, brk: true },
      ]),
    ).toBe(2);
  });

  it('numbers bases by their index in the read, not by walk order', () => {
    // A model saved before the aligner anchored its ends: the walk only
    // covers seq[3..], so numbering has to start there — the chromatogram
    // peaks are indexed by the read's own base number.
    const aln = {
      seq: 'TTTACGTAC',
      segments: [{ start: 10, end: 15, chars: 'ACGTAC' }],
      insertions: [],
    };
    expect(buildColumnAnchors(aln).map((e) => e.q)).toEqual([3, 4, 5, 6, 7, 8]);
  });

  it('draws a plain line between non-adjacent bases', () => {
    const chrom = {
      traceA: [0, 1, 2, 3, 4, 5, 6, 7],
      traceC: [0, 0, 0, 0, 0, 0, 0, 0],
      traceG: [0, 0, 0, 0, 0, 0, 0, 0],
      traceT: [0, 0, 0, 0, 0, 0, 0, 0],
      peakLocations: [0, 1, 2, 3, 4, 5, 6, 7],
    };
    const points = (anchors) =>
      (buildTracePath(chrom, 'traceA', anchors, 10, 1).match(/[ML]/g) || []).length;
    // Samples 0 and 4 are not neighbours (a merged block put them side by
    // side): one straight segment, no interpolated samples in between.
    expect(
      points([
        { x: 0, q: 0 },
        { x: 12, q: 4 },
      ]),
    ).toBe(2);
    // Consecutive bases keep their interpolation.
    expect(
      points([
        { x: 0, q: 0 },
        { x: 12, q: 1 },
      ]),
    ).toBeGreaterThan(2);
  });

  it('keeps every read base accounted across split segments (no holes)', () => {
    // 3 read bases leading junk, two segments with a junction insertion of
    // 2 bases, 2 trailing bases. Total read bases = 5 hit + 3 + 2 + 2 = 12.
    const aln = {
      segments: [
        { start: 100, end: 104, chars: 'ACGTA' },
        { start: 200, end: 201, chars: 'GG' },
      ],
      insertions: [
        { pos: 100, bases: 'TTT' },
        { pos: 200, bases: 'AA' },
        { pos: 202, bases: 'CC' },
      ],
    };
    const entries = buildColumnAnchors(aln);
    const insBases = entries.filter((e) => e.ins).length;
    const hitBases = entries.filter((e) => !e.ins).length;
    expect(hitBases).toBe(7);
    expect(insBases).toBe(7);
    // q strictly increases along the walk (read order preserved).
    for (let i = 1; i < entries.length; i++) {
      expect(entries[i].q).toBe(entries[i - 1].q + 1);
    }
  });
});

describe('anchor ↔ lane-layout correspondence invariant', () => {
  // Replicates the SequenceEditor stream math: hit columns land on their own
  // stream cell, insertion base k on the slot cell left of its anchor column.
  const cellOf = (entry, stream, insReserve) =>
    entry.ins
      ? stream.streamOf(entry.col) - (insReserve.get(entry.col) || 0) + entry.k
      : stream.streamOf(entry.col);

  it('covers every read base exactly once in read order, no column overlap', () => {
    // Split read with all the hard cases: leading junk tail (21), two
    // segments, junction insertion (5), mid-segment 1bp insertions, and a
    // trailing tail (14).
    const templateLen = 3000;
    const readLen = 21 + 500 + 1 + 5 + 500 + 14;
    const aln = {
      segments: [
        { start: 400, end: 899, chars: 'A'.repeat(500) },
        { start: 1800, end: 2299, chars: 'C'.repeat(500) },
      ],
      insertions: [
        { pos: 400, bases: 'G'.repeat(21) },
        { pos: 600, bases: 'T' },
        { pos: 900, bases: 'ACGTA' },
        { pos: 2300, bases: 'N'.repeat(14) },
      ],
    };
    // Entries carry their index within the insertion run (e.k) natively.
    const entries = buildColumnAnchors(aln);

    // Every read base exactly once, in order.
    expect(entries.length).toBe(readLen);
    for (let i = 0; i < entries.length; i++) {
      expect(entries[i].q).toBe(i);
    }

    const insReserve = new Map(aln.insertions.map((i) => [i.pos, i.bases.length]));
    const visCpl = 60;
    const stream = buildStreamLayout(insReserve, templateLen, visCpl);
    // Group by stream row, check per-row strict visual-column monotonicity.
    const byRow = new Map();
    for (const e of entries) {
      const si = cellOf(e, stream, insReserve);
      const row = Math.floor(si / visCpl);
      if (!byRow.has(row)) byRow.set(row, []);
      byRow.get(row).push({ e, vis: si % visCpl });
    }
    expect(byRow.size).toBeGreaterThan(5);
    for (const [row, rowEntries] of byRow) {
      expect(row).toBeLessThan(stream.numRows);
      let prev = -1;
      let prevQ = -1;
      for (const { e, vis } of rowEntries) {
        expect(Number.isFinite(vis)).toBe(true);
        expect(vis).toBeGreaterThan(prev); // strictly increasing → no overlaps
        expect(vis).toBeLessThan(visCpl); // never overflows the row
        prev = vis;
        expect(e.q).toBeGreaterThan(prevQ);
        prevQ = e.q;
      }
    }
  });
});
