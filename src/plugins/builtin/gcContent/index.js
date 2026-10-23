import { useGcLane, renderGcTrack, gcContentColor } from './track';

const gcContentPlugin = {
  id: 'gcContent',
  name: 'GC Content',
  description: 'Show a per-base GC content gradient track below the sequence',
  version: '1.0.0',
  dialogKey: null,
  // Per-base GC fraction is meaningless for protein sequences.
  notForProtein: true,
  // Entry points are the View nav menu toggle and the settings field
  // wired via the featuresMenuItem/settingsField hooks below.
  sidebarItems: [],
  featuresMenuItem: {
    label: 'Show GC Content',
    // Gradient endpoints, sampled from the same color function as the track.
    legend: [
      { color: gcContentColor(0), label: '0% GC' },
      { color: gcContentColor(0.5), label: '50% GC' },
      { color: gcContentColor(1), label: '100% GC' },
    ],
  },
  settingsField: {
    label: 'Window size',
    description:
      'Each position is colored by the GC fraction of the surrounding window (blue = 0%, white = 50%, red = 100%)',
    min: 1,
    max: 999,
    unit: 'bp',
    defaultValue: 11,
  },
  track: { useLane: useGcLane, render: renderGcTrack },
};

export default gcContentPlugin;
