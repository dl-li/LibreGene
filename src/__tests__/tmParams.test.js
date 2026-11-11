import { describe, expect, it } from 'vitest';
import {
  isDefaultTmParams,
  normalizePrimerTmMode,
  normalizeTmParams,
  primerTmForMode,
  primerTmModeAvailable,
  primerTmModeForSite,
  readStoredPrimerTmMode,
  readStoredTmParams,
  TM_PARAM_DEFAULTS,
  writeStoredPrimerTmMode,
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

describe('Primer Tm display mode', () => {
  it('defaults to the annealing core and rejects unknown modes', () => {
    expect(normalizePrimerTmMode(undefined)).toBe('annealCore');
    expect(normalizePrimerTmMode('invalid')).toBe('annealCore');
    expect(normalizePrimerTmMode('annealCore')).toBe('annealCore');
    expect(normalizePrimerTmMode('fullPrimer')).toBe('fullPrimer');
  });

  it('persists the selected mode', () => {
    const values = new Map();
    const storage = {
      getItem: (key) => values.get(key),
      setItem: (key, value) => values.set(key, value),
    };
    expect(writeStoredPrimerTmMode('fullPrimer', storage)).toBe('fullPrimer');
    expect(readStoredPrimerTmMode(storage)).toBe('fullPrimer');
    writeStoredPrimerTmMode('invalid', storage);
    expect(readStoredPrimerTmMode(storage)).toBe('annealCore');
  });

  it('selects the requested Tm and supports legacy results', () => {
    const site = {
      tm: 64.8,
      tmOptions: { alignment: 64.8, annealCore: 63.9, fullPrimer: 67.3 },
    };
    expect(primerTmForMode(site, 'alignment')).toBe(64.8);
    expect(primerTmForMode(site, 'annealCore')).toBe(63.9);
    expect(primerTmForMode(site, 'fullPrimer')).toBe(67.3);
    expect(primerTmForMode({ tm: 60 }, 'fullPrimer')).toBe(60);
  });

  it('disables alignment Tm per site without changing the preferred mode', () => {
    const site = {
      tm: 59.6,
      alignmentTmAvailable: false,
      alignmentHasGaps: true,
      tmOptions: { alignment: 59.6, annealCore: 59.6, fullPrimer: 63.9 },
    };
    expect(primerTmModeAvailable(site, 'alignment')).toBe(false);
    expect(primerTmModeAvailable(site, 'fullPrimer')).toBe(true);
    expect(primerTmModeForSite(site, 'alignment')).toBe('annealCore');
    expect(primerTmModeForSite(site, 'fullPrimer')).toBe('fullPrimer');
    expect(primerTmForMode(site, 'alignment')).toBe(59.6);
    expect(primerTmForMode(site, 'fullPrimer')).toBe(63.9);
    expect(normalizePrimerTmMode('alignment')).toBe('alignment');
  });
});
