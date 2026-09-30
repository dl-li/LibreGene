// Unit tests for the alignment lane drift layout (insertion slot expansion).
import { describe, it, expect } from 'vitest';
import {
  alignmentInsertUnion,
  alignmentLaneLayout,
  buildStreamLayout,
} from '../editor/alignmentLayout';

const ins = (pos, bases) => ({ pos, bases });
const seg = { start: 5, end: 50, chars: '' };

describe('alignmentInsertUnion', () => {
  it('keeps the longest insertion per template column across alignments', () => {
    const union = alignmentInsertUnion(
      [
        { segments: [seg], insertions: [ins(10, 'AA'), ins(20, 'C')] },
        { segments: [seg], insertions: [ins(10, 'GGG')] },
      ],
      100,
    );
    expect(union.get(10)).toBe(3);
    expect(union.get(20)).toBe(1);
    expect(union.size).toBe(2);
  });

  it('keeps flank junk anchored outside the aligned range', () => {
    const union = alignmentInsertUnion(
      [
        {
          segments: [{ start: 10, end: 50, chars: '' }],
          insertions: [ins(10, 'AA'), ins(60, 'CC'), ins(30, 'G')],
        },
      ],
      100,
    );
    expect(union.size).toBe(3);
    expect(union.get(10)).toBe(2);
    expect(union.get(30)).toBe(1);
    expect(union.get(60)).toBe(2);
  });

  it('ignores out-of-range anchors', () => {
    const union = alignmentInsertUnion(
      [
        {
          segments: [{ start: 10, end: 99, chars: '' }],
          insertions: [ins(100, 'AAA'), ins(-1, 'C')],
        },
      ],
      100,
    );
    expect(union.size).toBe(0);
  });

  it('handles an empty union', () => {
    const layout = alignmentLaneLayout(new Map());
    expect(layout.insTotal).toBe(0);
    expect(layout.drift(5)).toBe(0);
    expect(layout.slotBase.size).toBe(0);
  });
});

describe('buildStreamLayout', () => {
  // A 70-base slot block anchored at column 0 of a 100-base sequence with
  // 60 visual columns per row: the block itself fills the first row (which
  // therefore holds no template column at all).
  const stream = buildStreamLayout(new Map([[0, 70]]), 100, 60);

  it('fills rows with visual cells, including pure-slot rows', () => {
    expect(stream.streamLen).toBe(170);
    expect(stream.numRows).toBe(3);
    expect(stream.rowCounts[0]).toBe(0);
    expect(stream.rowCounts[1]).toBe(50);
    expect(stream.rowCounts[2]).toBe(50);
    // Backfilled so row 0 points at its block's anchor column.
    expect(stream.rowStarts[0]).toBe(0);
    expect(stream.rowStarts[1]).toBe(0);
    expect(stream.rowStarts[2]).toBe(50);
  });

  it('maps template columns to stream cells and back', () => {
    expect(stream.streamOf(0)).toBe(70);
    expect(stream.rowOf(0)).toBe(1);
    expect(stream.colOfAbs(0)).toBe(10);
    expect(stream.rowOf(49)).toBe(1);
    expect(stream.rowOf(50)).toBe(2);
    expect(stream.colOfAbs(50)).toBe(0);
    // Beyond the last base: the insert point, clamped to the last row.
    expect(stream.rowOf(100)).toBe(2);
    expect(stream.absFromStream(70)).toBe(0);
    expect(stream.absFromStream(119)).toBe(49);
  });

  it('never lets a row exceed its visual width', () => {
    for (let r = 0; r < stream.numRows; r++) {
      if (!stream.rowCounts[r]) continue;
      expect(stream.colVis(stream.rowCounts[r] - 1, r) + 1).toBeLessThanOrEqual(60);
    }
  });

  it('splits a range at row edges, skipping pure-slot rows', () => {
    expect(stream.sp(0, 99)).toEqual([
      { row: 1, colStart: 0, colEnd: 49, strOffset: 0, len: 50 },
      { row: 2, colStart: 0, colEnd: 49, strOffset: 50, len: 50 },
    ]);
  });
});
