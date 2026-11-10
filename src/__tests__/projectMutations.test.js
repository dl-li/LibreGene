import { describe, expect, it } from 'vitest';
import { requirePrimerMutationData } from '../workspace/useProjectMutations';

describe('primer mutation responses', () => {
  it('accepts a response containing the updated primers', () => {
    const data = { primers: [] };
    expect(requirePrimerMutationData(data)).toBe(data);
  });

  it('surfaces backend business errors to the dialog', () => {
    expect(() => requirePrimerMutationData({ error: 'Duplicate primer name' })).toThrow(
      'Duplicate primer name',
    );
  });

  it('rejects missing or malformed responses instead of reporting success', () => {
    expect(() => requirePrimerMutationData(null)).toThrow('Primer update failed');
    expect(() => requirePrimerMutationData({})).toThrow('Primer update failed');
  });
});
