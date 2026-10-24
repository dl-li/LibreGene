import { describe, expect, it } from 'vitest';
import { enzymeLabelIntersectsWindow, MAX_ENZ_LABEL_COLS } from '../editor/enzymeGeometry';

describe('continuous enzyme label virtualization', () => {
  it('keeps a label whose cut is left of the viewport but whose span intersects it', () => {
    const visibleCols = { start: 100, end: 200 };
    expect(enzymeLabelIntersectsWindow(100 - MAX_ENZ_LABEL_COLS, visibleCols, (x) => x)).toBe(true);
    expect(enzymeLabelIntersectsWindow(90, visibleCols, (x) => x)).toBe(true);
    expect(enzymeLabelIntersectsWindow(-50, visibleCols, (x) => x)).toBe(false);
  });

  it('includes cut points in range and rejects labels beyond the right edge', () => {
    const visibleCols = { start: 100, end: 200 };
    expect(enzymeLabelIntersectsWindow(200, visibleCols, (x) => x)).toBe(true);
    expect(enzymeLabelIntersectsWindow(201, visibleCols, (x) => x)).toBe(false);
    expect(enzymeLabelIntersectsWindow(150, null, (x) => x)).toBe(false);
  });

  it('measures label spans in stream cells', () => {
    expect(enzymeLabelIntersectsWindow(20, { start: 100, end: 120 }, (x) => x + 50)).toBe(true);
  });
});
