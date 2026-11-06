import { describe, expect, it } from 'vitest';
import { isDefaultTmParams, normalizeTmParams, TM_PARAM_DEFAULTS } from '../tmParams';

describe('Tm parameter conventions', () => {
  it('fills missing and invalid values with compatibility defaults', () => {
    expect(normalizeTmParams({ mgConc: 0.0015, naConc: Number.NaN })).toEqual({
      ...TM_PARAM_DEFAULTS,
      mgConc: 0.0015,
    });
  });

  it('detects the compatibility condition set', () => {
    expect(isDefaultTmParams(TM_PARAM_DEFAULTS)).toBe(true);
    expect(isDefaultTmParams({})).toBe(true);
    expect(isDefaultTmParams({ ...TM_PARAM_DEFAULTS, mgConc: 0.0015 })).toBe(false);
  });
});
