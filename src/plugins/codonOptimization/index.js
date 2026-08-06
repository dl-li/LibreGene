import { Dna } from 'lucide-react';
import CodonOptimizationDialog from './CodonOptimizationDialog';

const codonOptimizationPlugin = {
  id: 'codonOptimization',
  name: 'Codon Optimization',
  dialogKey: 'codonOptimization',
  // DNA-only: synonymous codon optimization applies to CDS/mRNA nucleotide
  // sequences, not amino-acid chains.
  dnaOnly: true,
  sidebarItems: [
    {
      dialogKey: 'codonOptimization',
      label: 'Codon Optimization',
      tooltip: 'Optimize a CDS/mRNA feature’s codon usage for a target species',
      icon: Dna,
    },
  ],
  dialog: CodonOptimizationDialog,
};

export default codonOptimizationPlugin;
