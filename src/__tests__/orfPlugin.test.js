import { beforeEach, describe, expect, it, vi } from 'vitest';

const { findOrfsCommand } = vi.hoisted(() => ({ findOrfsCommand: vi.fn() }));
vi.mock('../tauriApi', () => ({ findOrfs: findOrfsCommand }));

import { findOrfs } from '../plugins/builtin/orf';

describe('ORF plugin', () => {
  beforeEach(() => {
    findOrfsCommand.mockReset();
  });

  it('queries the exact sequence and topology snapshot being rendered', async () => {
    findOrfsCommand.mockResolvedValue([]);

    await findOrfs('ATGGGGAAACCCTAA', 'linear');

    expect(findOrfsCommand).toHaveBeenCalledWith('ATGGGGAAACCCTAA', 'linear', 75);
  });

  it('maps returned CDS features to numbered virtual ORFs', async () => {
    findOrfsCommand.mockResolvedValue([
      {
        id: 'orf+0:14',
        name: 'ORF 1..15',
        start: 0,
        end: 14,
        strand: '+',
        qualifiers: [['orf', 'true']],
      },
    ]);

    const features = await findOrfs('ATGGGGAAACCCTAA', 'linear');

    expect(features[0]).toMatchObject({ name: 'ORF #1', orf: true });
  });
});
