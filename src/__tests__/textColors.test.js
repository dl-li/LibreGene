import { describe, it, expect } from 'vitest';
import { adjustTextColorsForEdit, splitByTextColor } from '../editor/textColors';

const tc = (start, end, color) => ({ start, end, color });

describe('adjustTextColorsForEdit', () => {
  const ranges = [tc(0, 9, '#ff0000'), tc(20, 29, '#00ff00')];

  it('is a no-op for equal-length edits', () => {
    expect(adjustTextColorsForEdit(ranges, 5, 7, 3, 3)).toBe(ranges);
  });

  it('clips and shifts on deletion', () => {
    expect(adjustTextColorsForEdit(ranges, 4, 13, 10, 0)).toEqual([
      tc(0, 3, '#ff0000'),
      tc(10, 19, '#00ff00'),
    ]);
  });

  it('drops runs fully covered by the deleted span', () => {
    expect(adjustTextColorsForEdit(ranges, 18, 32, 15, 0)).toEqual([tc(0, 9, '#ff0000')]);
  });

  it('stretches a run over bases inserted inside it', () => {
    expect(adjustTextColorsForEdit(ranges, 5, 4, 0, 5)).toEqual([
      tc(0, 14, '#ff0000'),
      tc(25, 34, '#00ff00'),
    ]);
  });

  it('leaves inserted bases uncolored at a run boundary', () => {
    expect(adjustTextColorsForEdit(ranges, 20, 19, 0, 3)).toEqual([
      tc(0, 9, '#ff0000'),
      tc(23, 32, '#00ff00'),
    ]);
  });

  it('merges clipped same-color neighbors', () => {
    const both = [tc(0, 9, '#ff0000'), tc(10, 19, '#ff0000')];
    expect(adjustTextColorsForEdit(both, 5, 14, 10, 0)).toEqual([tc(0, 9, '#ff0000')]);
  });

  it('handles empty input', () => {
    expect(adjustTextColorsForEdit([], 0, 5, 6, 0)).toEqual([]);
    expect(adjustTextColorsForEdit(undefined, 0, 5, 6, 0)).toEqual([]);
  });
});

describe('splitByTextColor', () => {
  const ranges = [tc(10, 19, '#ff0000'), tc(30, 39, '#00ff00')];

  it('splits a chunk into colored and default pieces', () => {
    expect(splitByTextColor(5, 20, ranges)).toEqual([
      { off: 0, len: 5, color: null },
      { off: 5, len: 10, color: '#ff0000' },
      { off: 15, len: 5, color: null },
    ]);
  });

  it('clips ranges to the chunk', () => {
    expect(splitByTextColor(15, 10, ranges)).toEqual([
      { off: 0, len: 5, color: '#ff0000' },
      { off: 5, len: 5, color: null },
    ]);
  });

  it('returns a single default piece when nothing overlaps', () => {
    expect(splitByTextColor(0, 5, ranges)).toEqual([{ off: 0, len: 5, color: null }]);
    expect(splitByTextColor(0, 5, [])).toEqual([{ off: 0, len: 5, color: null }]);
  });

  it('colors the whole chunk', () => {
    expect(splitByTextColor(10, 10, ranges)).toEqual([{ off: 0, len: 10, color: '#ff0000' }]);
  });
});
