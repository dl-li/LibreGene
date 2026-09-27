// Unit tests for the alignment→chromatogram anchor mapping.
import { describe, it, expect } from 'vitest';
import { buildColumnAnchors, buildColumnQueryMap } from '../chromatogram';
import { alignmentLaneLayout } from '../SequenceEditor';

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
  // Replicates the SequenceEditor column math: for each anchor entry the
  // visual column is `col + drift(col)` for hit columns and `slotBase + k`
  // for insertion bases (both row-local after subtracting rowDrift).
  const visualColumn = (entry, layout, rowStart, rowDrift) =>
    entry.ins
      ? layout.slotBase.get(entry.col) + entry.k - rowDrift - rowStart
      : entry.col + layout.drift(entry.col) - rowDrift - rowStart;

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
    const layout = alignmentLaneLayout(insReserve);
    const gridCpl = 60;
    // Group by row, check per-row strict visual-column monotonicity.
    const byRow = new Map();
    for (const e of entries) {
      const row = Math.floor(e.col / gridCpl);
      if (!byRow.has(row)) byRow.set(row, []);
      byRow.get(row).push(e);
    }
    for (const [row, rowEntries] of byRow) {
      // Editor rule: drift measured to the column before the row start.
      const rowDrift = layout.drift(row * gridCpl - 1);
      const rowStart = row * gridCpl;
      let prev = -1;
      let prevQ = -1;
      for (const e of rowEntries) {
        const v = visualColumn(e, layout, rowStart, rowDrift);
        expect(Number.isFinite(v)).toBe(true);
        expect(v).toBeGreaterThan(prev); // strictly increasing → no overlaps
        prev = v;
        expect(e.q).toBeGreaterThan(prevQ);
        prevQ = e.q;
      }
    }
  });
});
