import AlignmentDialog from './AlignmentDialog';

const alignmentPlugin = {
  id: 'alignment',
  name: 'Alignment',
  dialogKey: 'alignment',
  // DNA-only: Sanger read / sequence alignment against the template assumes a
  // nucleotide template with strand/GC semantics.
  dnaOnly: true,
  sidebarItems: [],
  dialog: AlignmentDialog,
};

export default alignmentPlugin;
