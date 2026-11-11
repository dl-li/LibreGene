import { describe, expect, it, vi } from 'vitest';
import { requirePrimerMutationData } from '../workspace/useProjectMutations';
import { applyProjectMutation } from '../workspace/projectMutation';

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

describe('unified project mutation application', () => {
  it('applies only declared fields and backend dirty/revision metadata', () => {
    const calls = [];
    const latestRevisionRef = { current: 4 };
    const applied = applyProjectMutation(
      {
        revision: 5,
        dirty: false,
        changedFields: ['primers'],
        primers: [{ id: 'p' }],
        enzymes: [{ id: 'ignored' }],
        projects: [{ id: 'project' }],
      },
      {
        latestRevisionRef,
        setPrimers: (value) => calls.push(['primers', value]),
        setEnzymes: (value) => calls.push(['enzymes', value]),
        setIsDirty: (value) => calls.push(['dirty', value]),
        onProjectsSync: (value) => calls.push(['projects', value]),
      },
    );
    expect(applied).toBe(true);
    expect(calls).toEqual([
      ['primers', [{ id: 'p' }]],
      ['projects', [{ id: 'project' }]],
      ['dirty', false],
    ]);
    expect(latestRevisionRef.current).toBe(5);
  });

  it('rejects stale revisions and reports malformed responses', () => {
    const setSequence = vi.fn();
    const onError = vi.fn();
    expect(
      applyProjectMutation(
        { sequence: 'AAAA', revision: 3 },
        { latestRevisionRef: { current: 4 }, setSequence },
      ),
    ).toBe(false);
    expect(setSequence).not.toHaveBeenCalled();
    expect(applyProjectMutation({ error: 'denied' }, {}, { onError })).toBe(false);
    expect(onError).toHaveBeenCalledWith('Project update failed', 'denied');
  });
});
