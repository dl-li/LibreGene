import { describe, expect, it } from 'vitest';
import React from 'react';
import { renderToString } from 'react-dom/server';
import MsaWorkspace from '../msa/MsaWorkspace';

describe('MsaWorkspace layout', () => {
  it('reserves space for the collapsed sidebar rail', () => {
    const html = renderToString(
      React.createElement(MsaWorkspace, {
        projectId: '/tmp/test.msa.fa',
        projectName: 'test.msa.fa',
        initialData: {
          msaRows: [{ id: 'a', name: 'alpha', sequence: 'ACGT', alignedSequence: 'ACGT' }],
          moleculeType: 'dna',
          msaAligned: true,
        },
        hidden: false,
        leftViewportInset: 48,
        onDirtyChange: () => {},
        registerHandle: () => {},
        onProjectsSync: () => {},
        onRekey: () => {},
      }),
    );
    expect(html).toContain('padding-left:48px');
    expect(html).toContain('title="Save options"');
  });
});
