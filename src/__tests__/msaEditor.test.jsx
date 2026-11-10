import { describe, expect, it } from 'vitest';
import React from 'react';
import { renderToString } from 'react-dom/server';
import MsaEditor from '../msa/MsaEditor';

const rows = [
  { id: 'a', name: 'alpha', description: '', sequence: 'ACGT', alignedSequence: 'AC-GT' },
  { id: 'b', name: 'beta', description: '', sequence: 'AGT', alignedSequence: 'A--GT' },
];

describe('MsaEditor', () => {
  it('renders rows, reference, consensus and conservation in continuous mode', () => {
    const html = renderToString(
      React.createElement(MsaEditor, {
        rows,
        aligned: true,
        moleculeType: 'dna',
        referenceRowId: 'b',
        colorMode: 'consensus',
        selection: { startRow: 0, endRow: 1, startCol: 1, endCol: 2 },
        onSelectionChange: () => {},
      }),
    );
    expect(html).toContain('alpha');
    expect(html).toContain('beta');
    expect(html).toContain('REF');
    expect(html).toContain('Consensus');
    expect(html).toContain('Conservation');
    expect(html).not.toContain('Alignment column');
    expect(html).toContain('text-[14px] font-semibold');
    expect(html).toContain('aria-label="Resize sequence names column"');
    expect(html).toContain('>-</tspan>');
    expect(html).toContain('text-anchor="middle"');
    expect(html).toContain('<tspan x="5">A</tspan>');
    expect(html).toContain('<tspan x="15">C</tspan>');
    expect(html).not.toContain('textLength');
    expect(html).not.toContain('lengthAdjust');
    expect(html).not.toContain('letterSpacing');
    expect(html.match(/<text/g)?.length).toBeGreaterThan(10);
  });

  it('supports black residue coloring', () => {
    const html = renderToString(
      React.createElement(MsaEditor, {
        rows,
        aligned: true,
        moleculeType: 'dna',
        colorMode: 'black',
      }),
    );
    expect(html).toContain('fill="#111827"');
    expect(html).not.toContain('fill="#15803d"');
  });
});
