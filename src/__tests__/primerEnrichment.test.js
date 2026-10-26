import { describe, expect, it } from 'vitest';
import { enrichPrimers } from '../editor/useEnrichedPrimers';

const site = (templateStart, templateEnd, strand = 1, alignment = {}) => ({
  templateStart,
  templateEnd,
  strand,
  alignment,
});

const primer = (bindingSites) => ({
  id: 'primer-1',
  name: 'Primer 1',
  bindingSites,
});

describe('enrichPrimers', () => {
  it('renders every binding site with independent direction and coordinates', () => {
    const result = enrichPrimers(
      [primer([site(1, 5), site(8, 12, -1), site(13, 16)])],
      'AAAACCCCGGGGTTTT',
    );

    expect(result).toHaveLength(3);
    expect(result.map((p) => [p.matchStart, p.matchEnd, p.isFwd])).toEqual([
      [1, 4, true],
      [8, 11, false],
      [13, 15, true],
    ]);
    expect(new Set(result.map((p) => p.renderKey)).size).toBe(3);
    expect(result.every((p) => p.id === 'primer-1')).toBe(true);
    expect(result.every((p) => p.color === '#C2410C')).toBe(true);
  });

  it('keeps direction colors for single-site primers', () => {
    const [fwd, rev] = enrichPrimers(
      [
        { ...primer([site(0, 4)]), id: 'fwd' },
        { ...primer([site(4, 8, -1)]), id: 'rev' },
      ],
      'AAAACCCCGGGGTTTT',
    );

    expect(fwd.color).toBe('#166534');
    expect(rev.color).toBe('#4A148C');
    expect(fwd.renderKey).toBe('fwd:site-0');
  });

  it('builds alignment columns separately for each site', () => {
    const result = enrichPrimers(
      [
        primer([
          site(0, 4, 1, { displaySequence: 'AT-G', mismatchIndices: [2] }),
          site(8, 12, -1, { displaySequence: 'GGGG' }),
        ]),
      ],
      'AAAACCCCGGGGTTTT',
    );

    expect(result[0].renderCols.map((col) => [col.templateCol, col.kind])).toEqual([
      [0, 'match'],
      [1, 'match'],
      [2, 'gap'],
      [3, 'match'],
    ]);
    expect(result[1].renderCols.map((col) => col.templateCol)).toEqual([8, 9, 10, 11]);
    expect(result.every((p) => p.color === '#C2410C')).toBe(true);
  });

  it('supports legacy flat primer fields', () => {
    const result = enrichPrimers(
      [{ id: 'legacy', matchStart: 2, matchEnd: 6, isFwd: false, bindingSites: [] }],
      'AAAACCCCGGGGTTTT',
    );

    expect(result).toHaveLength(1);
    expect(result[0]).toMatchObject({ renderKey: 'legacy', color: '#4A148C' });
  });
});
