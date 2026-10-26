import React from 'react';
import { renderToString } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { renderGcTrack } from '../plugins/builtin/gcContent/track';

function ctx(visibleCols) {
  return {
    visibleRows: { start: 0, end: 0 },
    rowBuf: 0,
    numRows: 1,
    rowStarts: [0],
    rowCounts: [10000],
    getSeqY: () => 60,
    lp: { featBaseOffset: 14 },
    idPrefix: 'test',
    colVis: (col) => col,
    colFromVis: (vis) => Math.max(0, Math.min(10000, Math.round(vis))),
    colRuns: (start, end) => (start <= end ? [[start, end - start + 1]] : []),
    visibleCols,
  };
}

describe('GC track continuous window', () => {
  it('only materializes gradient stops near the visible columns', () => {
    const lane = { fracs: new Float64Array(10000).fill(0.5), height: 14 };
    const html = renderToString(<>{renderGcTrack(ctx({ start: 500, end: 600 }), lane)}</>);
    expect(html.match(/<stop/g)).toHaveLength(512);
    expect(html).toContain('test-gc-0-1');
    expect(html).toContain('test-gc-0-2');
    expect(html).not.toContain('test-gc-0-0"');
    expect(html).not.toContain('test-gc-0-3"');
  });
});
