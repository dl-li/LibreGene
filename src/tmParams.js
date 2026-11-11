export const TM_PARAM_DEFAULTS = Object.freeze({
  naConc: 0.05,
  mgConc: 0,
  dntpConc: 0,
  trisConc: 0,
  primerConc: 2.5e-7,
});

const TM_PARAM_KEYS = Object.keys(TM_PARAM_DEFAULTS);

export function normalizeTmParams(params = {}) {
  return Object.fromEntries(
    TM_PARAM_KEYS.map((key) => {
      const value = params?.[key];
      return [
        key,
        typeof value === 'number' && Number.isFinite(value) && value >= 0
          ? value
          : TM_PARAM_DEFAULTS[key],
      ];
    }),
  );
}

export function isDefaultTmParams(params) {
  const normalized = normalizeTmParams(params);
  return TM_PARAM_KEYS.every((key) => normalized[key] === TM_PARAM_DEFAULTS[key]);
}

export function readStoredTmParams(storage = globalThis.localStorage) {
  try {
    return normalizeTmParams(JSON.parse(storage?.getItem('tmParams') || '{}'));
  } catch {
    return { ...TM_PARAM_DEFAULTS };
  }
}

export function writeStoredTmParams(params, storage = globalThis.localStorage) {
  const normalized = normalizeTmParams(params);
  try {
    storage?.setItem('tmParams', JSON.stringify(normalized));
  } catch {
    // Keep the in-memory setting when storage is unavailable.
  }
  return normalized;
}

export const PRIMER_TM_MODE_DEFAULT = 'alignment';
export const PRIMER_TM_MODE_OPTIONS = Object.freeze([
  { value: 'annealCore', label: 'Based on 3′ continuous annealing region' },
  { value: 'fullPrimer', label: 'Based on full duplex' },
  { value: 'alignment', label: 'Based on alignment' },
]);

export function normalizePrimerTmMode(mode) {
  return PRIMER_TM_MODE_OPTIONS.some((option) => option.value === mode)
    ? mode
    : PRIMER_TM_MODE_DEFAULT;
}

export function readStoredPrimerTmMode(storage = globalThis.localStorage) {
  try {
    return normalizePrimerTmMode(storage?.getItem('primerTmMode'));
  } catch {
    return PRIMER_TM_MODE_DEFAULT;
  }
}

export function writeStoredPrimerTmMode(mode, storage = globalThis.localStorage) {
  const normalized = normalizePrimerTmMode(mode);
  try {
    storage?.setItem('primerTmMode', normalized);
  } catch {
    // Keep the in-memory setting when storage is unavailable.
  }
  return normalized;
}

export function primerTmModeAvailable(site, mode) {
  return normalizePrimerTmMode(mode) !== 'alignment' || site?.alignmentTmAvailable !== false;
}

export function primerTmModeForSite(site, mode) {
  const normalized = normalizePrimerTmMode(mode);
  return primerTmModeAvailable(site, normalized) ? normalized : 'annealCore';
}

export function primerTmForMode(site, mode) {
  return site?.tmOptions?.[primerTmModeForSite(site, mode)] ?? site?.tm;
}
