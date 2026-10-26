import { describe, expect, it } from 'vitest';
import { buildSegmentRowIndex, entitiesInRowRange, visibleRowRange } from '../editor/rowIndex';

const rowOf = (position) => Math.floor(position / 10);

describe('row entity index', () => {
  const a = { id: 'a', segments: [{ start: 2, end: 22 }] };
  const b = {
    id: 'b',
    segments: [
      { start: 5, end: 7 },
      { start: 12, end: 14 },
    ],
  };
  const c = { id: 'c', segments: [{ start: 35, end: 36 }] };

  it('indexes each entity once per covered row and preserves insertion order', () => {
    const index = buildSegmentRowIndex([a, b, c], rowOf);
    expect(index[0]).toEqual([a, b]);
    expect(index[1]).toEqual([a, b]);
    expect(index[2]).toEqual([a]);
    expect(index[3]).toEqual([c]);
  });

  it('collects a deduplicated union for an overlapping range', () => {
    const index = buildSegmentRowIndex([a, b, c], rowOf);
    expect(entitiesInRowRange(index, 6, 13, rowOf)).toEqual([a, b]);
    expect(entitiesInRowRange(index, 15, 35, rowOf)).toEqual([a, b, c]);
  });

  it('supports custom primer segments with tails', () => {
    const primer = { id: 'p', matchSegs: [{ start: 8, end: 12 }] };
    const index = buildSegmentRowIndex([primer], rowOf, (p) => p.matchSegs);
    expect(index[0]).toEqual([primer]);
    expect(index[1]).toEqual([primer]);
  });
});

describe('visibleRowRange', () => {
  function naive(rowY, rowAbove, rowBelow, scrollY, viewportH) {
    let start = 0;
    let end = rowY.length - 1;
    const bottom = scrollY + (viewportH || 900);
    for (let r = 0; r < rowY.length; r++) {
      if (rowY[r] + (rowBelow[r] || 0) > scrollY) {
        start = Math.max(0, r);
        break;
      }
    }
    for (let r = rowY.length - 1; r >= 0; r--) {
      if (rowY[r] - (rowAbove[r] || 0) < bottom) {
        end = Math.min(rowY.length - 1, r + 1);
        break;
      }
    }
    return { start, end };
  }

  it('matches the previous linear scan at boundaries and beyond content', () => {
    const rowY = [60, 130, 230, 360, 500];
    const above = [30, 20, 40, 25, 35];
    const below = [20, 35, 20, 45, 20];
    for (const scrollY of [0, 59, 60, 90, 180, 400, 520, 1000]) {
      for (const height of [0, 50, 200, 900]) {
        expect(visibleRowRange(rowY, above, below, scrollY, height)).toEqual(
          naive(rowY, above, below, scrollY, height),
        );
      }
    }
  });
});
