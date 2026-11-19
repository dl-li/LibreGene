import { renderToString } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import SelectionLengthBadge from '../editor/SelectionLengthBadge';

const textSelection = {
  selectionMode: 'text',
  isEnzymeSelection: false,
  selStart: 0,
  selEnd: 9,
  cleanSeq: 'ACGTACGTAA',
  selectedPrimerIds: [],
  enrichedPrimers: [],
};

describe('SelectionLengthBadge', () => {
  it('uses the navigation capsule style and preserves selection colors', () => {
    const html = renderToString(<SelectionLengthBadge {...textSelection} />);
    expect(html).toContain('10 bp · 40% GC');
    expect(html).toContain('rounded-full');
    expect(html).toContain('text-[13px]');
    expect(html).toContain('font-bold');
    expect(html).toContain('selection-presence-notice');
    expect(html).toContain('selection-coverage');
    expect(html).toContain('background-color:#3E2723');
    expect(html).toContain('background-image:none');
    expect(html).toContain('color:#fdfbf7');
    expect(html).toContain('box-shadow:0 0 3px 3px #fdfbf7');
    expect(html).not.toContain('outline:');
    expect(html).toContain('data-leaving="false"');
    expect(html).toContain('fixed bottom-[4.375rem] left-1/2');
    expect(html).toContain('-translate-x-1/2');
  });

  it('fades out while covered by a navigation notice', () => {
    const html = renderToString(<SelectionLengthBadge {...textSelection} coveredByNavNotice />);
    expect(html).toContain('data-leaving="true"');
    expect(html).toContain('aria-hidden="true"');
    expect(html).toContain('z-30');
  });

  it.each([
    [true, '#166534'],
    [false, '#4A148C'],
  ])('keeps the primer color rule for isFwd=%s', (isFwd, color) => {
    const html = renderToString(
      <SelectionLengthBadge
        {...textSelection}
        selectionMode="primer"
        selectedPrimerIds={['p1']}
        enrichedPrimers={[{ id: 'p1', primerSeq: 'ACGT', isFwd }]}
      />,
    );
    expect(html).toContain('4 bp · 50% GC');
    expect(html).toContain(`background-color:${color}`);
  });

  it('shows the selected primer Tm rounded to an integer', () => {
    const html = renderToString(
      <SelectionLengthBadge
        {...textSelection}
        selectionMode="primer"
        selectedPrimerIds={['p1']}
        enrichedPrimers={[
          {
            id: 'p1',
            primerSeq: 'ACGT',
            isFwd: true,
            bindingSites: [{ tmOptions: { annealCore: 52.6 } }],
          },
        ]}
      />,
    );
    expect(html).toContain('4 bp · 50% GC · 53℃');
    expect(html).not.toContain('Tm');
  });

  it('shows forward and reverse primer Tms for a selected primer pair', () => {
    const html = renderToString(
      <SelectionLengthBadge
        {...textSelection}
        selectionMode="amplimer"
        selectedPrimerIds={['fwd', 'rev']}
        cleanSeq="ACGTAAAACCCGGG"
        enrichedPrimers={[
          {
            id: 'fwd',
            primerSeq: 'ACGT',
            isFwd: true,
            matchStart: 0,
            matchEnd: 3,
            bindingSites: [{ tmOptions: { annealCore: 52.4 } }],
          },
          {
            id: 'rev',
            primerSeq: 'CCCG',
            isFwd: false,
            matchStart: 10,
            matchEnd: 13,
            bindingSites: [{ tmOptions: { annealCore: 53.4 } }],
          },
        ]}
      />,
    );
    expect(html).toContain('F 52℃ · R 53℃');
  });
});
