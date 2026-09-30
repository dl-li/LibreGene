import React from 'react';
import { plugins } from '../../plugins';

// Registry-order list of plugins contributing an in-editor track lane
// (src/plugins/*/index.js `track` hook, e.g. the GC-content plugin). Module
// constant so the lane hooks below always run in a stable order.
export const trackPlugins = plugins.filter((p) => p.track);

// Track-plugin lanes (e.g. the GC-content gradient band): each active
// plugin renders all visible rows below the sequence. `ctx` is the shared
// layout context built by SequenceEditor, `lanes` the per-plugin useLane
// results in the same registry order.
export function renderTrackLanes(ctx, lanes) {
  return trackPlugins.map((plugin, i) =>
    lanes[i] && plugin.track.render
      ? React.createElement(React.Fragment, { key: plugin.id }, plugin.track.render(ctx, lanes[i]))
      : null,
  );
}
