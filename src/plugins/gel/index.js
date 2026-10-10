import GelDialog from './GelDialog';

const gelPlugin = {
  id: 'gel',
  name: 'Digest and Run Gel',
  description: 'In-silico restriction digest and gel electrophoresis simulation',
  version: '1.0.0',
  dialogKey: 'gel',
  // Self-contained, distributable plugin (see manifest.json/README.md in this
  // directory): everything it needs lives under src/plugins/gel/.
  distributable: true,
  // Digest simulation is only meaningful for DNA projects.
  dnaOnly: true,
  // Entry point is the "Digest and Run Gel" item in the Diagrams menu
  // (navMenuOnly), wired via SequenceEditor.onOpenGel, not the sidebar.
  navMenuOnly: true,
  sidebarItems: [],
  dialog: GelDialog,
};

export default gelPlugin;
