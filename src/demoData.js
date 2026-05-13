// Demo data for offline/development mode.
import { complement } from './editorConstants';

const reverseComplement = (s) => [...s].reverse().map(c => complement(c)).join('');

export const baseSeq =
  "TACGAATTCGCCACCATGGCCATGAAGCTTGAGCTCGGATCCTCTAGAGCTGATCGATCGTAGCTAGCTAGCTGATCGATCGTAGCTAGCTAGCTAGCTACGATCGATCGATCGTAGCTAGCTAGCTGATCCTAGCTAGCTAGCTGATCGATCGTAGCATCGTAGC" +
  "ACGTGTGCTAGCTAGCGCTATATATATAGCGCGCGCGTATATAGCTAGCTAGCTCGATCGATCGTAGCTAGCTGCATCGATCGTAGCTGATCGTAGCTGATCGATCGTAGCTAGCTAGCTGATCATATCATGATCGATCGTATGCGCGCGCTATTAGCTAGCTGAT" +
  "CCGGAATTCCTCGAGAAGCTTTTCTAGAGGATCCTAGCCTAGCCTTAGCTAGCTAGCTGATCGATCGTCTAGAGCTCGAATTC";

export const defaultFeatures = [
  { id: 'Promoter', name: 'T7 Promoter', start: 10, end: 40, color: '#34d399' },
  { id: 'CDS_Huge', name: 'Monomeric GFP', start: 105, end: 280, color: '#60A5FA' },
  { id: 'CDS_Split', name: 'Split CDS', segments: [{ start: 120, end: 155 }, { start: 195, end: 240 }], color: '#f59e0b' },
  { id: 'Intron', name: 'Intron 1', start: 150, end: 190, color: '#f472b6' },
  { id: 'TATA', name: 'TATA Box', start: 200, end: 220, color: '#a78bfa' },
  { id: 'Terminator', name: 'Terminator', start: 300, end: 320, color: '#9ca3af' },
];

export const defaultEnzymes = [
  { id: 'EcoRI', name: 'EcoRI', recSeq: 'GAATTC', recSeqPattern: 'GAATTC', compSeq: 'CTTAAG', recStart: 3, recEnd: 8, displayStart: 3, displayEnd: 8, cutIndex: 4, botCutIndex: 8, cutPairs: [{ topCutIndex: 4, botCutIndex: 8 }], recognitionStrand: 'top', isUnique: true, cutTwice: false, isPalindromic: true, cutType: '5overhang' },
  { id: 'HindIII', name: 'HindIII', recSeq: 'AAGCTT', recSeqPattern: 'AAGCTT', compSeq: 'TTCGAA', recStart: 22, recEnd: 27, displayStart: 22, displayEnd: 27, cutIndex: 23, botCutIndex: 27, cutPairs: [{ topCutIndex: 23, botCutIndex: 27 }], recognitionStrand: 'top', isUnique: true, cutTwice: false, isPalindromic: true, cutType: '5overhang' },
  { id: 'BamHI', name: 'BamHI', recSeq: 'GGATCC', recSeqPattern: 'GGATCC', compSeq: 'CCTAGG', recStart: 41, recEnd: 46, displayStart: 41, displayEnd: 46, cutIndex: 42, botCutIndex: 46, cutPairs: [{ topCutIndex: 42, botCutIndex: 46 }], recognitionStrand: 'top', isUnique: false, cutTwice: false, isPalindromic: true, cutType: '5overhang' },
  { id: 'XbaI_Cross', name: 'XbaI', recSeq: 'TCTAGA', recSeqPattern: 'TCTAGA', compSeq: 'AGATCT', recStart: 57, recEnd: 62, displayStart: 57, displayEnd: 62, cutIndex: 58, botCutIndex: 62, cutPairs: [{ topCutIndex: 58, botCutIndex: 62 }], recognitionStrand: 'top', isUnique: false, cutTwice: false, isPalindromic: true, cutType: '5overhang' },
  { id: 'EcoRI_2', name: 'EcoRI', recSeq: 'GAATTC', recSeqPattern: 'GAATTC', compSeq: 'CTTAAG', recStart: 333, recEnd: 338, displayStart: 333, displayEnd: 338, cutIndex: 334, botCutIndex: 338, cutPairs: [{ topCutIndex: 334, botCutIndex: 338 }], recognitionStrand: 'top', isUnique: false, cutTwice: false, isPalindromic: true, cutType: '5overhang' },
  { id: 'BsaI_Exo', name: 'BsaI', recSeq: 'GGTCTC', recSeqPattern: 'GGTCTC', compSeq: 'CCAGAG', recStart: 51, recEnd: 56, displayStart: 51, displayEnd: 69, cutIndex: 65, botCutIndex: 69, cutPairs: [{ topCutIndex: 65, botCutIndex: 69 }], recognitionStrand: 'top', isUnique: true, cutTwice: false, isPalindromic: false, cutType: '5overhang' },
  { id: 'Exo_Up', name: 'UpExo', recSeq: 'GATCGT', recSeqPattern: 'GATCGT', compSeq: 'CTAGCA', recStart: 86, recEnd: 91, displayStart: 74, displayEnd: 91, cutIndex: 78, botCutIndex: 74, cutPairs: [{ topCutIndex: 78, botCutIndex: 74 }], recognitionStrand: 'bottom', isUnique: true, cutTwice: false, isPalindromic: false, cutType: '3overhang' },
  // Cut-twice demo: two cut pairs per recognition site
  { id: 'CT_Same', name: 'CutTwice', recSeq: 'TACGAATTCGC', recSeqPattern: 'TACGAATTCGC', compSeq: 'ATGCTTAAGCG', recStart: 0, recEnd: 10, displayStart: 0, displayEnd: 10, cutIndex: 0, botCutIndex: 10, cutPairs: [{ topCutIndex: 0, botCutIndex: 10 }, { topCutIndex: 4, botCutIndex: 8 }], recognitionStrand: 'top', isUnique: true, cutTwice: true, isPalindromic: false, cutType: '3overhang' },
  { id: 'CT_Cross', name: 'CutTwiceX', recSeq: 'GATCGTAG', recSeqPattern: 'GATCGTAG', compSeq: 'CTAGCATC', recStart: 55, recEnd: 62, displayStart: 55, displayEnd: 65, cutIndex: 55, botCutIndex: 57, cutPairs: [{ topCutIndex: 55, botCutIndex: 57 }, { topCutIndex: 62, botCutIndex: 65 }], recognitionStrand: 'top', isUnique: true, cutTwice: true, isPalindromic: false, cutType: '5overhang' },
  { id: 'SameSeq_DiffCut_1', name: 'IsoA', recSeq: 'ATCGAT', recSeqPattern: 'ATCGAT', compSeq: 'TAGCTA', recStart: 108, recEnd: 113, displayStart: 108, displayEnd: 113, cutIndex: 110, botCutIndex: 112, cutPairs: [{ topCutIndex: 110, botCutIndex: 112 }], recognitionStrand: 'top', isUnique: false, cutTwice: false, isPalindromic: false, cutType: '5overhang' },
  { id: 'SameSeq_DiffCut_2', name: 'IsoB', recSeq: 'ATCGAT', recSeqPattern: 'ATCGAT', compSeq: 'TAGCTA', recStart: 108, recEnd: 113, displayStart: 108, displayEnd: 113, cutIndex: 112, botCutIndex: 114, cutPairs: [{ topCutIndex: 112, botCutIndex: 114 }], recognitionStrand: 'top', isUnique: false, cutTwice: false, isPalindromic: false, cutType: '5overhang' },
  { id: 'SameCut_DiffSeq_1', name: 'SymA', recSeq: 'GCGCGC', recSeqPattern: 'GCGCGC', compSeq: 'CGCGCG', recStart: 147, recEnd: 152, displayStart: 147, displayEnd: 152, cutIndex: 150, botCutIndex: 152, cutPairs: [{ topCutIndex: 150, botCutIndex: 152 }], recognitionStrand: 'top', isUnique: false, cutTwice: false, isPalindromic: true, cutType: '5overhang' },
  { id: 'SameCut_DiffSeq_2', name: 'SymB', recSeq: 'GTATAT', recSeqPattern: 'GTATAT', compSeq: 'CATATA', recStart: 149, recEnd: 154, displayStart: 149, displayEnd: 154, cutIndex: 150, botCutIndex: 152, cutPairs: [{ topCutIndex: 150, botCutIndex: 152 }], recognitionStrand: 'top', isUnique: false, cutTwice: false, isPalindromic: false, cutType: '5overhang' },
  { id: 'TailBlock', name: 'TailBlock', recSeq: 'ATCGAT', recSeqPattern: 'ATCGAT', compSeq: 'TAGCTA', recStart: 159, recEnd: 164, displayStart: 159, displayEnd: 164, cutIndex: 160, botCutIndex: 164, cutPairs: [{ topCutIndex: 160, botCutIndex: 164 }], recognitionStrand: 'top', isUnique: false, cutTwice: false, isPalindromic: false, cutType: '5overhang' },
  { id: 'DraIII', name: 'DraIII', recSeq: 'CACAAAGTG', recSeqPattern: 'CACNNNGTG', compSeq: 'GTGTTTCAC', recStart: 248, recEnd: 256, displayStart: 248, displayEnd: 256, cutIndex: 250, botCutIndex: 255, cutPairs: [{ topCutIndex: 250, botCutIndex: 255 }], recognitionStrand: 'top', spacers: [], isUnique: true, cutTwice: false, isPalindromic: false, cutType: '5overhang' },
];

export const defaultPrimers = [
  { id: 'Fwd1', name: 'Fwd Start', type: 'fwd', primerSeq: 'cagtaTACGAATTCGCCACCA', color: '#166534' },
  { id: 'Fwd1b', name: 'Fwd Overlap', type: 'fwd', primerSeq: 'ggcc' + baseSeq.substring(5, 23), color: '#166534' },
  { id: 'Rev1', name: 'Rev Mid', type: 'rev', primerSeq: 'tttt' + reverseComplement(baseSeq.substring(48, 66)), color: '#166534' },
  { id: 'Rev1b', name: 'Rev Overlap', type: 'rev', primerSeq: 'aaaa' + reverseComplement(baseSeq.substring(52, 71)), color: '#166534' },
  { id: 'Fwd2', name: 'Fwd Inner HA', type: 'fwd', primerSeq: 'gatactatacgatgttccagattacgctctgc' + baseSeq.substring(180, 196), color: '#166534' },
  { id: 'Rev2', name: 'Rev End BamHI', type: 'rev', primerSeq: 'ggatccatcg' + reverseComplement(baseSeq.substring(270, 296)), color: '#166534' },
];
