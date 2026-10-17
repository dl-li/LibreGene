import GelDialog from './GelDialog';

const gelPlugin = {
  id: 'gel',
  name: 'Digest and Run Gel',
  description: 'In-silico restriction digest and gel electrophoresis simulation',
  version: '1.1.0',
  author: 'dl-li',
  dialogKey: 'gel',
  // Self-contained, distributable plugin (see manifest.json/README.md in this
  // directory): everything it needs lives under src/plugins/example-plugin-gel/.
  distributable: true,
  // Digest simulation is only meaningful for DNA projects.
  dnaOnly: true,
  // Entry point is the "Digest and Run Gel" item in the Diagrams menu
  // wired via SequenceEditor.onOpenGel, not the sidebar.
  sidebarItems: [],
  dialog: GelDialog,
};

export default gelPlugin;
