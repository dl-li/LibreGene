import { describe, expect, it } from 'vitest';
import {
  buildDigestionRows,
  formatDigestionCutPosition,
  sortDigestionRows,
} from '../enzymeDigestion';

const records = [
  { name: 'BamHI', site: 'GGATCC', fst5: 1, fst3: -1, cutType: '5overhang' },
  { name: 'BbsI', site: 'GAAGAC', fst5: 8, fst3: 6, cutType: '5overhang' },
  { name: 'EcoRI', site: 'GAATTC', fst5: 1, fst3: -1, cutType: '5overhang' },
  { name: 'NotI', site: 'GCGGCCGC', fst5: 2, fst3: -2, cutType: '5overhang' },
];

const enzymes = [
  {
    name: 'EcoRI',
    recSeq: 'GAATTC',
    cutIndex: 20,
    botCutIndex: 24,
    cutType: '5overhang',
  },
  {
    name: 'EcoRI',
    recSeq: 'GAATTC',
    cutIndex: 5,
    botCutIndex: 9,
    cutType: '5overhang',
  },
  {
    name: 'BbsI',
    recSeq: 'GAAGAC',
    recStart: 30,
    recEnd: 35,
    cutIndex: 38,
    botCutIndex: 42,
    cutType: '5overhang',
  },
];

describe('digestion overview rows', () => {
  it('merges detected cuts into the complete enzyme set and retains non-cutters', () => {
    const rows = buildDigestionRows(records, enzymes, 'all');

    expect(rows.find((row) => row.name === 'EcoRI').cuts).toEqual([5, 20]);
    expect(rows.find((row) => row.name === 'BamHI').cuts).toEqual([]);
    expect(rows.find((row) => row.name === 'NotI').pairCount).toBe(0);
  });

  it('applies recognition-length and personal-set filters before cuts are considered', () => {
    expect(buildDigestionRows(records, enzymes, 'rec8p').map((row) => row.name)).toEqual(['NotI']);
    expect(
      buildDigestionRows(records, enzymes, 'myEnzymes', ['BamHI', 'NotI']).map((row) => row.name),
    ).toEqual(['BamHI', 'NotI']);
    expect(buildDigestionRows(records, enzymes, 'unique').map((row) => row.name)).toEqual(['BbsI']);
  });

  it('sorts by ascending cut count with name tie-breaking or by name', () => {
    const rows = buildDigestionRows(records, enzymes, 'all');

    expect(sortDigestionRows(rows, 'cuts').map((row) => row.name)).toEqual([
      'BamHI',
      'NotI',
      'BbsI',
      'EcoRI',
    ]);
    expect(sortDigestionRows(rows, 'name').map((row) => row.name)).toEqual([
      'BamHI',
      'BbsI',
      'EcoRI',
      'NotI',
    ]);
  });

  it('derives Type IIS styling metadata even when the enzyme has no cuts', () => {
    const rows = buildDigestionRows(records, [], 'iis');

    expect(rows.map((row) => row.name)).toEqual(['BbsI']);
    expect(rows[0].iis).toBe(true);
    expect(rows[0].cuts).toEqual([]);
  });

  it('formats cut positions using one-based flanking bases', () => {
    expect(formatDigestionCutPosition(5, 100)).toBe('5^6');
    expect(formatDigestionCutPosition(0, 100)).toBe('100^1');
  });
});
