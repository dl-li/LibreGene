import { renderToString } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import {
  AlignmentArrow,
  alignmentMatchRuns,
  alignmentOverviewPieces,
} from '../plugins/builtin/alignment/AlignmentOverview';

describe('Alignment Overview match runs', () => {
  it('merges matches, mismatches, read gaps, and template gaps into visual runs', () => {
    const alignment = {
      segments: [
        { start: 0, end: 3, chars: 'AAT-' },
        { start: 6, end: 7, chars: 'TT' },
      ],
    };

    expect(alignmentMatchRuns(alignment, 'AACCGGTT')).toEqual([
      { start: 0, end: 1, matched: true },
      { start: 2, end: 5, matched: false },
      { start: 6, end: 7, matched: true },
    ]);
  });

  it('splits a read crossing the origin without filling the rest of the template', () => {
    const alignment = {
      strand: '+',
      segments: [
        { start: 6, end: 7, chars: 'GG' },
        { start: 0, end: 1, chars: 'TT' },
      ],
    };

    expect(alignmentOverviewPieces(alignment, 8)).toEqual([
      { start: 6, end: 7, tip: true },
      { start: 0, end: 1, tip: false },
    ]);
    expect(alignmentOverviewPieces({ ...alignment, strand: '-' }, 8)).toEqual([
      { start: 6, end: 7, tip: false },
      { start: 0, end: 1, tip: true },
    ]);
  });
});

describe('Alignment Overview arrow', () => {
  const alignment = {
    strand: '-',
    segments: [{ start: 1, end: 4, chars: 'AT-G' }],
    insertions: [{ pos: 3, bases: 'A' }],
  };

  it('shows discrepancies as hollow regions and changes color on hover', () => {
    const normal = renderToString(
      <AlignmentArrow alignment={alignment} sequence="AACCGG" width={600} y={9} />,
    );
    const hovered = renderToString(
      <AlignmentArrow alignment={alignment} sequence="AACCGG" width={600} y={9} hovered />,
    );

    expect(normal).toContain('fill="#c81e1e"');
    expect(normal).toContain('fill="var(--card)"');
    expect(normal).toContain('stroke="#c81e1e"');
    expect(hovered).toContain('fill="#2684e8"');
    expect(hovered).toContain('stroke="#2684e8"');
  });
});
