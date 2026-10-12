import CodonOptimizationDialog from './CodonOptimizationDialog';

const codonOptimizationPlugin = {
  id: 'codonOptimization',
  name: 'Codon Optimization',
  description: 'Optimize a CDS/mRNA feature’s codon usage for a target species',
  version: '1.0.0',
  dialogKey: 'codonOptimization',
  // DNA-only: synonymous codon optimization applies to CDS/mRNA nucleotide
  // sequences, not amino-acid chains.
  dnaOnly: true,
  // Entry point is the editor Edit menu, not the sidebar.
  sidebarItems: [],
  dialog: CodonOptimizationDialog,
};

export default codonOptimizationPlugin;
