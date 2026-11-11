import { renderToString } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { AlignmentView, OtherBindingSites } from '../dialogs/PrimerAlignmentDialog';

const alignment = [
  '      TGCAAA',
  "5' > ACGTTT > 3'",
  '     ||||||',
  '     ACGTTT',
  '      ACGTTT',
].join('\n');

const sites = [
  { start: 4, end: 10, strand: 1, alignment },
  { start: 19, end: 25, strand: -1, alignment: alignment.replaceAll('ACGTTT', 'GGGCCC') },
];

describe('PrimerAlignmentDialog other binding sites', () => {
  it('collapses every alignment by default', () => {
    const html = renderToString(<OtherBindingSites sites={sites} />);

    const text = html.replaceAll('<!-- -->', '');
    expect(html.match(/aria-expanded="false"/g)).toHaveLength(2);
    expect(text).toContain('5..10');
    expect(text).toContain('20..25');
    expect(html).toContain('border-orange-200/80');
    expect(html).not.toContain('ACGTTT');
    expect(html).not.toContain('GGGCCC');
  });

  it('uses the full alignment renderer with the multi-site theme color', () => {
    const html = renderToString(<AlignmentView data={sites[0]} primerColor="#C2410C" />);

    expect(html).toContain('ACGTTT');
    expect(html).toContain('||||||');
    expect(html).toContain('font-weight:bold');
    expect(html).toContain('background-color:#C2410C');
  });

  it('shows the selected Tm mode for alternative sites', () => {
    const withTm = [
      {
        ...sites[0],
        tm: 64.8,
        tmOptions: { alignment: 64.8, annealCore: 63.9, fullPrimer: 67.3 },
      },
    ];
    const coreHtml = renderToString(<OtherBindingSites sites={withTm} tmMode="annealCore" />);
    const fullHtml = renderToString(<OtherBindingSites sites={withTm} tmMode="fullPrimer" />);
    expect(coreHtml.replaceAll('<!-- -->', '')).toContain('63.9°C');
    expect(fullHtml.replaceAll('<!-- -->', '')).toContain('67.3°C');
  });

  it('falls back to the annealing core for gap-containing alternative sites', () => {
    const withGap = [
      {
        ...sites[0],
        tm: 59.6,
        alignmentTmAvailable: false,
        alignmentHasGaps: true,
        tmOptions: { alignment: 59.6, annealCore: 59.6, fullPrimer: 63.9 },
      },
    ];
    const html = renderToString(<OtherBindingSites sites={withGap} tmMode="alignment" />);
    expect(html.replaceAll('<!-- -->', '')).toContain('59.6°C');
    expect(html).not.toContain('63.9°C');
  });
});
