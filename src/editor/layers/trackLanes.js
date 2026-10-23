import React from 'react';
import { plugins } from '../../plugins';

// Registry-order list of plugins contributing an in-editor track lane
// (src/plugins/*/index.js `track` hook, e.g. the GC-content plugin). Module
// constant so the lane hooks below always run in a stable order.
export const trackPlugins = plugins.filter((p) => p.track);

// Track-plugin lanes (e.g. the GC-content gradient band, the protein
// property bands): each active plugin renders all visible rows below the
// sequence. `ctx` is the shared layout context built by SequenceEditor,
// `lanes` the per-plugin useLane results in the same registry order. Each
// plugin gets a per-lane `trackYOffset` — the cumulative height of the
// active lanes before it — so simultaneously enabled tracks stack instead
// of overlapping.
export function renderTrackLanes(ctx, lanes) {
  let offset = 0;
  return trackPlugins.map((plugin, i) => {
    const lane = lanes[i];
    const el =
      lane && plugin.track.render
        ? React.createElement(
            React.Fragment,
            { key: plugin.id },
            plugin.track.render({ ...ctx, trackYOffset: offset }, lane),
          )
        : null;
    if (lane?.height) offset += lane.height;
    return el;
  });
}
