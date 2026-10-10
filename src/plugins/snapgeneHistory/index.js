import SnapshotsDialog from './SnapshotsDialog';

const snapgeneHistoryPlugin = {
  id: 'snapgeneHistory',
  name: 'SnapGene History',
  description: 'Browse historical snapshots of SnapGene .dna files',
  version: '1.0.0',
  dialogKey: 'snapshots',
  // SnapGene .dna files are DNA sequences.
  dnaOnly: true,
  // Entry point is the "History" item in the Edit nav menu (navMenuOnly),
  // wired via SequenceEditor.onOpenSnapshots, not the sidebar.
  navMenuOnly: true,
  sidebarItems: [],
  dialog: SnapshotsDialog,
  // The dialog only makes sense for projects opened from a .dna file and for
  // snapshot projects spawned from one (which carry the history subtree).
  dialogVisible: ({ projectId, isTauri }) =>
    isTauri && (/\.dna$/i.test(projectId || '') || /^snapshot-/.test(projectId || '')),
};

export default snapgeneHistoryPlugin;
