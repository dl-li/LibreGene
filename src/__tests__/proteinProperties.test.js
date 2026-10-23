import { describe, it, expect } from 'vitest';
import {
  AA_UNKNOWN_COLOR,
  CHARGE_COLORS,
  HYDROPHOBICITY_COLORS,
  POLARITY_COLORS,
  SECONDARY_STRUCTURE_COLORS,
  SIZE_COLORS,
  aaColor,
} from '../plugins/builtin/proteinProperties/scales';

const ALL_SCALES = [
  HYDROPHOBICITY_COLORS,
  CHARGE_COLORS,
  POLARITY_COLORS,
  SIZE_COLORS,
  SECONDARY_STRUCTURE_COLORS,
];
const STANDARD_AA = 'ACDEFGHIKLMNPQRSTVWY';

describe('protein property color scales', () => {
  it('every scale covers all 20 standard amino acids', () => {
    for (const scale of ALL_SCALES) {
      for (const aa of STANDARD_AA) {
        expect(scale[aa], `missing ${aa}`).toMatch(/^#[0-9A-Fa-f]{6}$/);
      }
    }
  });

  it('hydrophobicity: orange = hydrophobic, blue = hydrophilic', () => {
    expect(aaColor(HYDROPHOBICITY_COLORS, 'I')).toBe('#E8833A');
    expect(aaColor(HYDROPHOBICITY_COLORS, 'R')).toBe('#3A7BD5');
    expect(aaColor(HYDROPHOBICITY_COLORS, 'G')).toBe('#E8E8E8');
  });

  it('charge: blue = positive, red = negative', () => {
    expect(aaColor(CHARGE_COLORS, 'K')).toBe('#3B6FD4');
    expect(aaColor(CHARGE_COLORS, 'E')).toBe('#D43B3B');
    expect(aaColor(CHARGE_COLORS, 'A')).toBe('#E8E8E8');
  });

  it('size tiers go light → dark', () => {
    expect(aaColor(SIZE_COLORS, 'G')).toBe('#DCD0F0');
    expect(aaColor(SIZE_COLORS, 'W')).toBe('#4A2F8F');
  });

  it('is case-insensitive and maps unknown residues to neutral gray', () => {
    expect(aaColor(CHARGE_COLORS, 'k')).toBe('#3B6FD4');
    for (const scale of ALL_SCALES) {
      for (const ch of ['X', 'B', 'Z', 'J', 'U', 'O', '*', '-', '?']) {
        expect(aaColor(scale, ch)).toBe(AA_UNKNOWN_COLOR);
      }
    }
  });
});

import { makeProteinTrackPlugin } from '../plugins/builtin/proteinProperties/track';

describe('protein property track render', () => {
  const plugin = makeProteinTrackPlugin({
    id: 'testTrack',
    name: 'Test',
    description: '',
    menuLabel: 'Test',
    scale: CHARGE_COLORS,
  });
  // Flat mock layout: one row, visual column == template column.
  const mockCtx = (n) => ({
    visibleRows: { start: 0, end: 0 },
    rowBuf: 0,
    numRows: 1,
    rowStarts: [0],
    rowCounts: [n],
    getSeqY: () => 0,
    lp: { featBaseOffset: 14 },
    colVis: (i) => i,
    colRuns: (s, e) => [[s, e - s + 1]],
    visibleCols: null,
    trackYOffset: 28,
  });

  it('merges adjacent same-color residues into one rect per run', () => {
    // K K D E A → blue run (2), red run (2), gray run (1): 3 rects.
    const seq = 'KKDEA';
    const lane = { height: 14, colors: [...seq].map((c) => aaColor(CHARGE_COLORS, c)) };
    const rows = plugin.track.render(mockCtx(seq.length), lane);
    const rects = rows[0].props.children;
    expect(rects).toHaveLength(3);
    expect(rects.map((r) => [(r.props.x - 220) / 12, r.props.width / 12, r.props.fill])).toEqual([
      [0, 2, '#3B6FD4'],
      [2, 2, '#D43B3B'],
      [4, 1, '#E8E8E8'],
    ]);
    expect(rects[0].props.y).toBe(0 + 14 - 6 + 28);
  });

  it('returns null for a missing lane', () => {
    expect(plugin.track.render(mockCtx(3), null)).toBeNull();
  });
});
