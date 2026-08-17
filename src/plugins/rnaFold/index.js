import RnaFoldDialog from './RnaFoldDialog';

const rnaFoldPlugin = {
  id: 'rnaFold',
  name: 'RNA Folding',
  description: 'Predict MFE secondary structure (Turner 2004)',
  version: '1.0.0',
  dialogKey: 'rnaFold',
  // RNA-only: MFE secondary-structure prediction is meaningful only for
  // single-stranded RNA projects.
  rnaOnly: true,
  // Entry point is the "Folding" button in EditorNavMenu (navMenuOnly),
  // wired via SequenceEditor.onOpenRnaFold, not the sidebar.
  navMenuOnly: true,
  sidebarItems: [],
  dialog: RnaFoldDialog,
};

export default rnaFoldPlugin;
