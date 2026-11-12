import { renderToString } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { cw, getX } from '../editorConstants';
import SearchHighlightLayer, {
  SEARCH_HIGHLIGHT_COLOR,
} from '../editor/layers/SearchHighlightLayer';

const wrappedSp = (start, end) => {
  const segments = [];
  const firstRow = Math.floor(start / 6);
  const lastRow = Math.floor(end / 6);
  for (let row = firstRow; row <= lastRow; row++) {
    segments.push({
      row,
      colStart: row === firstRow ? start % 6 : 0,
      colEnd: row === lastRow ? end % 6 : 5,
    });
  }
  return segments;
};

function renderLayer(matches, overrides = {}) {
  return renderToString(
    <svg>
      <SearchHighlightLayer
        matches={matches}
        visibleRows={{ start: 0, end: 1 }}
        rowBuf={0}
        numRows={2}
        getSeqY={(row) => 40 + row * 60}
        sp={wrappedSp}
        colRuns={(start, end) => [[start, end - start + 1]]}
        {...overrides}
      />
    </svg>,
  );
}

describe('SearchHighlightLayer', () => {
  it('highlights every visible match across rows and both strands', () => {
    const html = renderLayer(
      [
        { start: 0, end: 2 },
        { start: 4, end: 7 },
        { start: 12, end: 14 },
      ],
      { dsBottomY: 28 },
    );

    expect(html.match(new RegExp(`fill="${SEARCH_HIGHLIGHT_COLOR}"`, 'g'))).toHaveLength(3);
    expect(html).toContain('height="56.2"');
    expect(html).not.toContain('y="121"');
  });

  it('clips matches to the horizontal viewport in continuous mode', () => {
    const html = renderLayer([{ start: 0, end: 5 }], {
      numRows: 1,
      sp: (start, end) => [{ row: 0, colStart: start, colEnd: end }],
      colFromVis: (vis) => vis,
      visibleCols: { start: 2, end: 4 },
    });

    expect(html).toContain(`x="${getX(2)}"`);
    expect(html).toContain(`width="${3 * cw}"`);
  });

  it('renders nothing without matches', () => {
    expect(renderLayer([])).toBe('<svg></svg>');
  });
});
