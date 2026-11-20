import { describe, expect, it } from 'vitest';
import {
  findInvalidSequenceCharacters,
  parseSequenceShortcuts,
  replaceSequenceShortcut,
  stripValidSequenceShortcuts,
} from '../sequenceShortcuts';

const enzymes = [
  { name: 'EcoRI', site: 'GAATTC' },
  { name: 'HHHHHH', site: 'GGGCCC' },
];
const zero = () => 0;
const iupac = (ch) => /[ACGTURYSWKMBDHVN]/i.test(ch);

describe('sequence shortcuts', () => {
  it('creates random-base and case-insensitive enzyme badges', () => {
    const { groups, tokens } = parseSequenceShortcuts('[5N, ecori]', enzymes, zero);

    expect(groups).toHaveLength(1);
    expect(groups[0].valid).toBe(true);
    expect(tokens).toHaveLength(2);
    expect(tokens[0].options).toEqual([{ kind: 'random', replacement: 'AAAAA', display: '5N' }]);
    expect(tokens[1].options).toEqual([
      { kind: 'enzyme', replacement: 'GAATTC', display: 'EcoRI' },
    ]);
    expect(findInvalidSequenceCharacters('[5N, ecori]', iupac, groups)).toEqual([]);
  });

  it('uses random synonymous codons for amino acids and supports stop codons', () => {
    const { tokens } = parseSequenceShortcuts('[HHHHHH, *]', enzymes.slice(0, 1), zero);

    expect(tokens[0].options).toEqual([
      { kind: 'aminoAcids', replacement: 'CAC'.repeat(6), display: 'HHHHHH' },
    ]);
    expect(tokens[1].options).toEqual([{ kind: 'aminoAcids', replacement: 'TAA', display: '*' }]);
  });

  it('returns both enzyme and amino-acid options for an ambiguous code', () => {
    const { tokens } = parseSequenceShortcuts('[hhhhhh]', enzymes, zero);

    expect(tokens).toHaveLength(1);
    expect(tokens[0].options).toEqual([
      { kind: 'enzyme', replacement: 'GGGCCC', display: 'HHHHHH' },
      { kind: 'aminoAcids', replacement: 'CAC'.repeat(6), display: 'HHHHHH' },
    ]);
  });

  it('keeps unknown codes invalid', () => {
    const { groups, tokens } = parseSequenceShortcuts('AC[not-a-code]GT', enzymes, zero);

    expect(groups[0].valid).toBe(false);
    expect(tokens).toHaveLength(0);
    expect(findInvalidSequenceCharacters('AC[not-a-code]GT', iupac, groups)).toEqual([
      '[',
      'o',
      '-',
      'e',
      ']',
    ]);
  });

  it('replaces individual codes in order without leaving group punctuation', () => {
    const initial = parseSequenceShortcuts('[5N, EcoRI]', enzymes, zero);
    const enzymeReplaced = replaceSequenceShortcut('[5N, EcoRI]', initial.tokens[1], 'GAATTC');
    expect(enzymeReplaced).toBe('[5N]GAATTC');

    const remaining = parseSequenceShortcuts(enzymeReplaced, enzymes, zero);
    const fullyReplaced = replaceSequenceShortcut(enzymeReplaced, remaining.tokens[0], 'AAAAA');
    expect(fullyReplaced).toBe('AAAAAGAATTC');
    expect(parseSequenceShortcuts(fullyReplaced, enzymes, zero).groups).toHaveLength(0);
  });

  it('preserves both sides when replacing the middle code', () => {
    const value = '[HH, EcoRI, *]';
    const { tokens } = parseSequenceShortcuts(value, enzymes, zero);

    expect(replaceSequenceShortcut(value, tokens[1], 'GAATTC')).toBe('[HH]GAATTC[*]');
  });

  it('removes valid groups from plain sequence validation but retains invalid groups', () => {
    const valid = parseSequenceShortcuts('AA[5N]CC', enzymes, zero);
    expect(stripValidSequenceShortcuts('AA[5N]CC', valid.groups)).toBe('AACC');

    const invalid = parseSequenceShortcuts('AA[bad!]CC', enzymes, zero);
    expect(stripValidSequenceShortcuts('AA[bad!]CC', invalid.groups)).toBe('AA[bad!]CC');
  });
});
