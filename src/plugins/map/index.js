const mapPlugin = {
  id: 'map',
  name: 'Plasmid Map',
  description: 'Circular/linear plasmid map view and watermark overlay',
  version: '1.0.0',
  // Entry point is the "Map" button in EditorNavMenu (navMenuOnly), wired via
  // SequenceEditor.onOpenMapView, not the sidebar.
  navMenuOnly: true,
  sidebarItems: [],
};

export default mapPlugin;
