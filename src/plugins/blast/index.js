const blastPlugin = {
  id: 'blast',
  name: 'BLAST Search',
  description: 'Identify the selected sequence via NCBI BLAST (opens results in your browser)',
  version: '1.0.0',
  // Entry point is the selection right-click menu in SequenceEditor, gated
  // there on disabledPlugins and molecule type (DNA/protein).
  navMenuOnly: true,
  sidebarItems: [],
};

export default blastPlugin;
