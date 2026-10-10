import DotplotDialog from './DotplotDialog';

const dotplotPlugin = {
  id: 'dotplot',
  name: 'Dotplot',
  description: 'Dot-plot comparison of two DNA/RNA sequences',
  version: '1.0.0',
  dialogKey: 'dotplot',
  // Self-contained, distributable plugin (see manifest.json/README.md in this
  // directory): everything it needs lives under src/plugins/dotplot/.
  distributable: true,
  // Dotplot compares nucleotide sequences; not meaningful for protein projects.
  notForProtein: true,
  // Entry point is the "Examine Dotplot" item in the Diagrams menu
  // (navMenuOnly), wired via SequenceEditor.onOpenDotplot, not the sidebar.
  navMenuOnly: true,
  sidebarItems: [],
  dialog: DotplotDialog,
};

export default dotplotPlugin;
