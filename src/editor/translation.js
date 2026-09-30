import { complement } from '../editorConstants';

// ---------------------------------------------------------------------------
// Standard genetic code table
// ---------------------------------------------------------------------------
export const GENETIC_CODE = {
  ATA: 'I',
  ATC: 'I',
  ATT: 'I',
  ATG: 'M',
  ACA: 'T',
  ACC: 'T',
  ACG: 'T',
  ACT: 'T',
  AAC: 'N',
  AAT: 'N',
  AAA: 'K',
  AAG: 'K',
  AGC: 'S',
  AGT: 'S',
  AGA: 'R',
  AGG: 'R',
  CTA: 'L',
  CTC: 'L',
  CTG: 'L',
  CTT: 'L',
  CCA: 'P',
  CCC: 'P',
  CCG: 'P',
  CCT: 'P',
  CAC: 'H',
  CAT: 'H',
  CAA: 'Q',
  CAG: 'Q',
  CGA: 'R',
  CGC: 'R',
  CGG: 'R',
  CGT: 'R',
  GTA: 'V',
  GTC: 'V',
  GTG: 'V',
  GTT: 'V',
  GCA: 'A',
  GCC: 'A',
  GCG: 'A',
  GCT: 'A',
  GAC: 'D',
  GAT: 'D',
  GAA: 'E',
  GAG: 'E',
  GGA: 'G',
  GGC: 'G',
  GGG: 'G',
  GGT: 'G',
  TCA: 'S',
  TCC: 'S',
  TCG: 'S',
  TCT: 'S',
  TTC: 'F',
  TTT: 'F',
  TTA: 'L',
  TTG: 'L',
  TAC: 'Y',
  TAT: 'Y',
  TAA: '*',
  TAG: '*',
  TGC: 'C',
  TGT: 'C',
  TGA: '*',
  TGG: 'W',
};

/**
 * Build CDS data for a feature from the DNA sequence.
 * Never uses feature.translation — always calculates from scratch.
 * Returns { trans, codonMap, codingBases } where:
 *   trans: array of { aa, templatePos2, codonIndex, bases } for each codon
 *   codonMap: Map from template position to its codon index
 *   codingBases: template positions in 5'→3' order
 */
export function buildCDSData(feature, sequence) {
  const segs = feature.segments;
  if (!segs || !segs.length) return { trans: [], codonMap: new Map(), codingBases: [] };

  const isRev = feature.strand === '-';

  // Build CDS bases as template indices in 5'→3' order
  const codingBases = [];
  if (isRev) {
    for (let i = segs.length - 1; i >= 0; i--) {
      const seg = segs[i];
      for (let j = seg.end; j >= seg.start; j--) {
        if (j >= 0 && j < sequence.length) codingBases.push(j);
      }
    }
  } else {
    for (const seg of segs) {
      for (let j = seg.start; j <= seg.end; j++) {
        if (j >= 0 && j < sequence.length) codingBases.push(j);
      }
    }
  }

  const trans = [];
  const codonMap = new Map();
  for (let i = 0; i + 2 < codingBases.length; i += 3) {
    const t1 = codingBases[i];
    const t2 = codingBases[i + 1];
    const t3 = codingBases[i + 2];

    const b1 = isRev ? complement(sequence[t1]) : sequence[t1];
    const b2 = isRev ? complement(sequence[t2]) : sequence[t2];
    const b3 = isRev ? complement(sequence[t3]) : sequence[t3];

    const codon = (b1 + b2 + b3).toUpperCase();
    const aa = GENETIC_CODE[codon] || '?';
    const codonIndex = i / 3;
    trans.push({ aa, templatePos2: t2, codonIndex, bases: [t1, t2, t3] });
    codonMap.set(t1, codonIndex);
    codonMap.set(t2, codonIndex);
    codonMap.set(t3, codonIndex);
  }

  return { trans, codonMap, codingBases };
}

/** Feature types that get in-editor translation (codon) display. */
export function isTranslatable(f) {
  return f.ftype === 'CDS' || f.ftype === 'mRNA';
}
