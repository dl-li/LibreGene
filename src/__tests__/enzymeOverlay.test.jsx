// Regression test: methylation state is per-site. The hover overlay must look
// up the hovered site by entry id — not the first entry sharing the enzyme
// name — or a blocked site shows a gray label with no "[Dam Blocked]" hint
// (and an unblocked site could show a bogus one).
import { describe, it, expect } from 'vitest';
import React from 'react';
import { renderToString } from 'react-dom/server';
import { renderEnzymeOverlay } from '../editor/layers/enzymes';

// Two sites of the same enzyme: the first (leftmost) site is clean, the
// second is Dam-blocked. Entry order mirrors the backend's rec_start sort.
const enzymes = [
  {
    id: 'XxxI_10_0',
    name: 'XxxI',
    isUnique: false,
    methylationBlocked: false,
    methylationSources: [],
    methylationRequired: false,
    methylRequiredSources: [],
  },
  {
    id: 'XxxI_50_0',
    name: 'XxxI',
    isUnique: false,
    methylationBlocked: true,
    methylationSources: ['Dam'],
    methylationRequired: false,
    methylRequiredSources: [],
  },
];

const enzymeLayout = [
  {
    id: 'XxxI_10_0_p0',
    groupId: 'XxxI_10_0',
    pairIndex: 0,
    name: 'XxxI',
    cutX: 100,
    yTop: 20,
    sy: 40,
    isUnique: false,
  },
  {
    id: 'XxxI_50_0_p0',
    groupId: 'XxxI_50_0',
    pairIndex: 0,
    name: 'XxxI',
    cutX: 300,
    yTop: 20,
    sy: 40,
    isUnique: false,
  },
];

function renderOverlay(hoveredEnzyme) {
  return renderToString(
    React.createElement(
      'svg',
      null,
      renderEnzymeOverlay({
        hoveredEnzyme,
        enzymeLayout,
        selectedEnzymeIds: [],
        enzymes,
        totalNameCounts: new Map([['XxxI', 2]]),
        isEnzymeSelection: false,
      }),
    ),
  );
}

describe('renderEnzymeOverlay per-site methylation state', () => {
  it('shows the blocked reason when hovering the blocked site', () => {
    const html = renderOverlay('XxxI_50_0_p0');
    expect(html).toContain('[Dam Blocked]');
    // Each site's cut line takes its own methylation state.
    expect(html).toContain('stroke="#9CA3AF"');
    expect(html).toContain('stroke="#2563EB"');
  });

  it('shows no blocked reason when hovering the clean site', () => {
    const html = renderOverlay('XxxI_10_0_p0');
    expect(html).not.toContain('[Dam Blocked]');
  });
});
