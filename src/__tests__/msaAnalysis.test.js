import { describe, expect, it } from 'vitest';
import {
  averagePairwiseIdentity,
  computeAveragePairwiseIdentity,
  computeConsensus,
  computePairwiseMatrix,
  detectMsaMoleculeType,
  makeMsaRow,
  pairwiseMsaStats,
  parseFastaRecords,
  selectionToFasta,
} from '../msa/msaAnalysis';

describe('MSA FASTA input', () => {
  it('preserves names and descriptions while removing input gaps', () => {
    const rows = parseFastaRecords('>a first record\nAC-GT\n>b\nA--GT\n');
    expect(rows).toHaveLength(2);
    expect(rows[0]).toMatchObject({ name: 'a', description: 'first record', sequence: 'ACGT' });
    expect(rows[1].sequence).toBe('AGT');
    expect(rows[0].id).not.toBe(rows[1].id);
  });

  it('detects nucleic acids, RNA and protein, with manual override left to the UI', () => {
    expect(detectMsaMoleculeType([makeMsaRow('a', 'ACGT')])).toBe('dna');
    expect(detectMsaMoleculeType([makeMsaRow('a', 'ACGU')])).toBe('rna');
    expect(detectMsaMoleculeType([makeMsaRow('a', 'MVLIP')])).toBe('protein');
  });
});

describe('MSA column analysis', () => {
  it('computes majority consensus, conservation and gap fractions while ignoring gaps', () => {
    const rows = [
      { ...makeMsaRow('a', 'ACGT'), alignedSequence: 'AC-GT' },
      { ...makeMsaRow('b', 'AGT'), alignedSequence: 'A--GT' },
      { ...makeMsaRow('c', 'ATGT'), alignedSequence: 'AT-GT' },
    ];
    const result = computeConsensus(rows, true);
    expect(result.sequence).toBe('AC-GT');
    expect(result.conservation).toEqual([1, 0.5, 0, 1, 1]);
    expect(result.gapFractions).toEqual([0, 1 / 3, 1, 0, 0]);
  });

  it('excludes gaps from identity and uses BLOSUM62 positives for protein similarity', () => {
    const nucleotide = pairwiseMsaStats('AC-GT', 'A--GT', 'dna');
    expect(nucleotide).toEqual({ compared: 3, identity: 1, similarity: null });
    const protein = pairwiseMsaStats('D', 'E', 'protein');
    expect(protein.identity).toBe(0);
    expect(protein.similarity).toBe(1);
  });

  it('computes average pairwise identity without counting diagonal self-comparisons', () => {
    const rows = [
      { ...makeMsaRow('a', 'AAAA'), alignedSequence: 'AAAA' },
      { ...makeMsaRow('b', 'AAAT'), alignedSequence: 'AAAT' },
      { ...makeMsaRow('c', 'AATT'), alignedSequence: 'AATT' },
    ];
    const matrix = computePairwiseMatrix(rows, true, 'dna');
    expect(averagePairwiseIdentity(matrix)).toBeCloseTo((0.75 + 0.5 + 0.75) / 3);
    expect(computeAveragePairwiseIdentity(rows, true, 'dna')).toBe(averagePairwiseIdentity(matrix));
    expect(computeAveragePairwiseIdentity([rows[0]], true, 'dna')).toBeNull();
  });
});

describe('MSA selection export', () => {
  it('copies the selected rows and columns as FASTA', () => {
    const rows = [
      { ...makeMsaRow('a', 'ACGT', 'description'), alignedSequence: 'AC-GT' },
      { ...makeMsaRow('b', 'AGT'), alignedSequence: 'A--GT' },
    ];
    expect(selectionToFasta(rows, true, { startRow: 0, endRow: 1, startCol: 1, endCol: 3 })).toBe(
      '>a description\nC-G\n>b\n--G',
    );
  });
});
