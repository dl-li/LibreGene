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
});
