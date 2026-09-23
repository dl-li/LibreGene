import { useGcLane, renderGcTrack } from './track';

export { SHOW_GC_CONTENT_KEY, GC_WINDOW_SIZE_KEY } from './track';

const gcContentPlugin = {
  id: 'gcContent',
  name: 'GC Content',
  description: '在序列下方显示逐碱基 GC 含量渐变轨道',
  version: '1.0.0',
  dialogKey: null,
  // Per-base GC fraction is meaningless for protein sequences.
  notForProtein: true,
  // Entry points are the Features nav menu toggle and the settings field
  // (navMenuOnly), wired via the featuresMenuItem/settingsField hooks below.
  navMenuOnly: true,
  sidebarItems: [],
  featuresMenuItem: { label: 'Show GC Content', notForProtein: true },
  settingsField: {
    label: 'Window size',
    description:
      'Each position is colored by the GC fraction of the surrounding window (blue = 0%, white = 50%, red = 100%)',
    min: 1,
    max: 999,
    unit: 'bp',
  },
  track: { useLane: useGcLane, render: renderGcTrack },
};

export default gcContentPlugin;
