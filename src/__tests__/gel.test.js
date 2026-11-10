import { describe, expect, it } from 'vitest';
import {
  categorizeEnzymes,
  digest,
  enzymeMatchesGroup,
  groupEnzymeSites,
} from '../plugins/example-plugin-gel/gel';

const cutTwiceSite = {
  name: 'BaeI',
  recSeq: 'ACNNNNGTAYC',
  cutIndex: 10,
  botCutIndex: 12,
  cutPairs: [
    { topCutIndex: 10, botCutIndex: 12 },
    { topCutIndex: 30, botCutIndex: 32 },
  ],
};

describe('gel cut-pair handling', () => {
  it('uses every top-strand cut when computing digest fragments', () => {
    const [group] = groupEnzymeSites([cutTwiceSite]);

    expect(group.cuts).toEqual([10, 30]);
    expect(group.pairCount).toBe(2);
    expect(digest({ length: 100, circular: false, cuts: group.cuts, efficiency: 1 })).toEqual([
      { length: 20, moles: 1, topo: 'linear' },
      { length: 70, moles: 1, topo: 'linear' },
      { length: 10, moles: 1, topo: 'linear' },
    ]);
  });

  it('matches editor unique and twice-cutter semantics by cut-pair count', () => {
    const [group] = groupEnzymeSites([cutTwiceSite]);

    expect(enzymeMatchesGroup(group, 'unique')).toBe(false);
    expect(enzymeMatchesGroup(group, 'twice')).toBe(true);
    expect(categorizeEnzymes([group]).double).toEqual([group]);
  });
});
