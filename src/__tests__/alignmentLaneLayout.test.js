// Unit tests for the alignment lane drift layout (insertion slot expansion).
import { describe, it, expect } from 'vitest';
import { alignmentInsertUnion, alignmentLaneLayout } from '../SequenceEditor';

const ins = (pos, bases) => ({ pos, bases });

describe('alignmentInsertUnion', () => {
  it('keeps the longest insertion per template column across alignments', () => {
    const union = alignmentInsertUnion(
      [
        { insertions: [ins(10, 'AA'), ins(20, 'C')] },
        { insertions: [ins(10, 'GGG')] },
      ],
      100,
    );
    expect(union.get(10)).toBe(3);
    expect(union.get(20)).toBe(1);
    expect(union.size).toBe(2);
  });

  it('ignores out-of-range anchors', () => {
    const union = alignmentInsertUnion([{ insertions: [ins(100, 'AAA'), ins(-1, 'C')] }], 100);
    expect(union.size).toBe(0);
  });
});

describe('alignmentLaneLayout', () => {
  it('places each reserved base in its own column left of the anchor', () => {
    const layout = alignmentLaneLayout(new Map([[10, 2], [20, 1]]));
    expect(layout.insTotal).toBe(3);
    // Template columns before the first slot have zero drift.
    expect(layout.drift(0)).toBe(0);
    expect(layout.drift(9)).toBe(0);
    // The anchor column shifts past its own slot (the slot renders between
    // the previous column and the anchor).
    expect(layout.drift(10)).toBe(2);
    expect(layout.drift(11)).toBe(2);
    expect(layout.drift(19)).toBe(2);
    expect(layout.drift(20)).toBe(3);
    expect(layout.drift(21)).toBe(3);
    // Visual columns (col + drift) are strictly increasing: col 9 → 9,
    // col 10 → 12 with the slot at 10-11, col 11 → 13.
    let prev = -1;
    for (let c = 0; c < 30; c++) {
      const visual = c + layout.drift(c);
      expect(visual).toBeGreaterThan(prev);
      prev = visual;
    }
  });

  it('exposes absolute slot base columns shared by text and chromatogram lanes', () => {
    const layout = alignmentLaneLayout(new Map([[10, 2], [20, 1]]));
    // slot@10 occupies drift columns 10-11 (left of col 10's visual 12);
    // slot@20 occupies column 22 (left of col 20's visual 23).
    expect(layout.slotBase.get(10)).toBe(10);
    expect(layout.slotBase.get(20)).toBe(22);
  });

  it('handles an empty union', () => {
    const layout = alignmentLaneLayout(new Map());
    expect(layout.insTotal).toBe(0);
    expect(layout.drift(5)).toBe(0);
    expect(layout.slotBase.size).toBe(0);
  });
});
