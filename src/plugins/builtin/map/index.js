const mapPlugin = {
  id: 'map',
  name: 'Plasmid Map',
  description: 'Circular/linear plasmid map view and watermark overlay',
  version: '1.0.0',
  // Entry point is the "Map" button in EditorNavMenu, wired via
  // SequenceEditor.onOpenMapView, not the sidebar.
  sidebarItems: [],
};

export default mapPlugin;
