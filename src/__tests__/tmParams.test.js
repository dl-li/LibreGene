import { describe, expect, it } from 'vitest';
import {
  isDefaultTmParams,
  normalizeTmParams,
  readStoredTmParams,
  TM_PARAM_DEFAULTS,
  writeStoredTmParams,
} from '../tmParams';

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

  it('persists and restores validated conditions', () => {
    const values = new Map();
    const storage = {
      getItem: (key) => values.get(key),
      setItem: (key, value) => values.set(key, value),
    };
    writeStoredTmParams({ ...TM_PARAM_DEFAULTS, mgConc: 0.0015 }, storage);
    expect(readStoredTmParams(storage)).toEqual({ ...TM_PARAM_DEFAULTS, mgConc: 0.0015 });
  });

  it('falls back to defaults for malformed stored data', () => {
    const storage = { getItem: () => '{not json' };
    expect(readStoredTmParams(storage)).toEqual(TM_PARAM_DEFAULTS);
  });
});
