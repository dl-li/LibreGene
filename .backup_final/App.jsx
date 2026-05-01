import React from 'react';
import SequenceEditor from './SequenceEditor';

const baseSeq = "TACGAATTCGCCACCATGGCCATGAAGCTTGAGCTCGGATCCTCTAGAGCTGATCGATCGTAGCTAGCTAGCTGATCGATCGTAGCTAGCTAGCTAGCTACGATCGATCGATCGTAGCTAGCTAGCTGATCCTAGCTAGCTAGCTGATCGATCGTAGCATCGTAGC" +
  "ACGTGTGCTAGCTAGCGCTATATATATAGCGCGCGCGTATATAGCTAGCTAGCTCGATCGATCGTAGCTAGCTGCATCGATCGTAGCTGATCGTAGCTGATCGATCGTAGCTAGCTAGCTGATCATATCATGATCGATCGTATGCGCGCGCTATTAGCTAGCTGAT" +
  "CCGGAATTCCTCGAGAAGCTTTTCTAGAGGATCCTAGCCTAGCCTTAGCTAGCTAGCTGATCGATCGTCTAGAGCTCGAATTC";

const features = [
  { id: 'Promoter', name: 'T7 Promoter', start: 10, end: 40, color: '#34d399' },
  { id: 'CDS_Huge', name: 'Monomeric GFP', start: 105, end: 280, color: '#60A5FA' },
  { id: 'CDS_Split', name: 'Split CDS', segments: [{ start: 120, end: 155 }, { start: 195, end: 240 }], color: '#f59e0b' },
  { id: 'Intron', name: 'Intron 1', start: 150, end: 190, color: '#f472b6' },
  { id: 'TATA', name: 'TATA Box', start: 200, end: 220, color: '#a78bfa' },
  { id: 'Terminator', name: 'Terminator', start: 300, end: 320, color: '#9ca3af' },
];

const enzymes = [
  { id: 'EcoRI', name: 'EcoRI', cutIndex: 4, seq: 'GAATTC', compSeq: 'CTTAAG', topCut: 1, botCut: 5, isUnique: true },
  { id: 'HindIII', name: 'HindIII', cutIndex: 23, seq: 'AAGCTT', compSeq: 'TTCGAA', topCut: 1, botCut: 5, isUnique: true },
  { id: 'BamHI', name: 'BamHI', cutIndex: 40, seq: 'GGATCC', compSeq: 'CCTAGG', topCut: 1, botCut: 5, isUnique: false },
  { id: 'XbaI_Cross', name: 'XbaI', cutIndex: 58, seq: 'TCTAGA', compSeq: 'AGATCT', topCut: 1, botCut: 5, isUnique: false },
  { id: 'EcoRI_2', name: 'EcoRI', cutIndex: 334, seq: 'GAATTC', compSeq: 'CTTAAG', topCut: 1, botCut: 5, isUnique: false },
  { id: 'BsaI_Exo', name: 'BsaI', cutIndex: 65, botCutIndex: 69, recStart: 51, seq: 'GGTCTC', compSeq: 'CCAGAG', topCut: 1, botCut: 5, isUnique: true },
  { id: 'Exo_Up', name: 'UpExo', cutIndex: 78, botCutIndex: 74, recStart: 86, seq: 'GATCGT', compSeq: 'CTAGCA', topCut: -8, botCut: -12, isUnique: true },
  { id: 'SameSeq_DiffCut_1', name: 'IsoA', cutIndex: 110, seq: 'ATCGAT', compSeq: 'TAGCTA', topCut: 2, botCut: 4, isUnique: false },
  { id: 'SameSeq_DiffCut_2', name: 'IsoB', cutIndex: 112, seq: 'ATCGAT', compSeq: 'TAGCTA', topCut: 4, botCut: 6, isUnique: false },
  { id: 'SameCut_DiffSeq_1', name: 'SymA', cutIndex: 150, seq: 'GCGCGC', compSeq: 'CGCGCG', topCut: 3, botCut: 5, isUnique: false },
  { id: 'SameCut_DiffSeq_2', name: 'SymB', cutIndex: 150, seq: 'GTATAT', compSeq: 'CATATA', topCut: 1, botCut: 3, isUnique: false },
  { id: 'TailBlock', name: 'TailBlock', cutIndex: 160, seq: 'ATCGAT', compSeq: 'TAGCTA', topCut: 1, botCut: 5, isUnique: false },
  { id: 'DraIII', name: 'DraIII', cutIndex: 250, seq: 'CACNNNGTG', compSeq: 'GTGNNNCAC', topCut: 6, botCut: 3, spacer: { start: 3, end: 6 }, isUnique: true },
];

const primers = [
  { id: 'Fwd1', name: 'Fwd Start', type: 'fwd', matchStart: 0, matchEnd: 15, mismatchStr: 'cagta', matchStr: 'TACGAATTCGCCACCA', color: '#166534' },
  { id: 'Fwd1b', name: 'Fwd Overlap', type: 'fwd', matchStart: 5, matchEnd: 22, mismatchStr: 'ggcc', matchStr: baseSeq.substring(5, 23), color: '#166534' },
  { id: 'Rev1', name: 'Rev Mid', type: 'rev', matchStart: 48, matchEnd: 65, mismatchStr: 'tttt', matchStr: baseSeq.substring(48, 66), color: '#166534' },
  { id: 'Rev1b', name: 'Rev Overlap', type: 'rev', matchStart: 52, matchEnd: 70, mismatchStr: 'aaaa', matchStr: baseSeq.substring(52, 71), color: '#166534' },
  { id: 'Fwd2', name: 'Fwd Inner HA', type: 'fwd', matchStart: 180, matchEnd: 195, mismatchStr: 'gatactatacgatgttccagattacgctctgc', mismatchFeatures: [{ name: 'HA tag', start: 6, end: 14 }], matchStr: baseSeq.substring(180, 196), color: '#166534' },
  { id: 'Rev2', name: 'Rev End BamHI', type: 'rev', matchStart: 270, matchEnd: 295, mismatchStr: 'ggatccatcg', mismatchFeatures: [{ name: 'BamHI', start: 0, end: 5 }], matchStr: baseSeq.substring(270, 296), color: '#166534' },
];

export default function App() {
  return (
    <div className="w-full min-h-screen bg-[#fdfbf7]">
      <SequenceEditor
        sequence={baseSeq}
        features={features}
        enzymes={enzymes}
        primers={primers}
        charsPerLine={60}
      />
    </div>
  );
}