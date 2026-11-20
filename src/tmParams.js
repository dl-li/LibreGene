export const TM_PARAM_DEFAULTS = Object.freeze({
  naConc: 0.05,
  mgConc: 0,
  dntpConc: 0,
  trisConc: 0,
  primerConc: 2.5e-7,
});

const TM_PARAM_KEYS = Object.keys(TM_PARAM_DEFAULTS);

export const PRIMER_TM_ALGORITHM_DEFAULT = 'standard';
export const PRIMER_TM_ALGORITHM_OPTIONS = Object.freeze([
  {
    value: 'standard',
    label: 'SantaLucia nearest-neighbor (standard)',
    description: 'SantaLucia 2004 thermodynamics with configurable salt conditions',
  },
  {
    value: 'snapgeneCompatible',
    label: 'SnapGene compatible (perfect-match core)',
    description:
      'SnapGene 7.0.2-compatible core and template dangling ends; unsupported cases use SantaLucia',
  },
]);

export function normalizePrimerTmAlgorithm(algorithm) {
  return PRIMER_TM_ALGORITHM_OPTIONS.some((option) => option.value === algorithm)
    ? algorithm
    : PRIMER_TM_ALGORITHM_DEFAULT;
}

export function normalizeTmParams(params = {}) {
  return {
    ...Object.fromEntries(
      TM_PARAM_KEYS.map((key) => {
        const value = params?.[key];
        return [
          key,
          typeof value === 'number' && Number.isFinite(value) && value >= 0
            ? value
            : TM_PARAM_DEFAULTS[key],
        ];
      }),
    ),
    algorithm: normalizePrimerTmAlgorithm(params?.algorithm),
  };
}

export function isDefaultTmParams(params) {
  const normalized = normalizeTmParams(params);
  return (
    TM_PARAM_KEYS.every((key) => normalized[key] === TM_PARAM_DEFAULTS[key]) &&
    normalized.algorithm === PRIMER_TM_ALGORITHM_DEFAULT
  );
}

export function readStoredTmParams(storage = globalThis.localStorage) {
  try {
    return normalizeTmParams(JSON.parse(storage?.getItem('tmParams') || '{}'));
  } catch {
    return normalizeTmParams();
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

export const PRIMER_TM_MODE_DEFAULT = 'annealCore';
export const PRIMER_TM_MODE_OPTIONS = Object.freeze([
  {
    value: 'annealCore',
    label: 'Based on 3′ continuous annealing region',
    description: 'Recommended for initial-cycle PCR annealing',
  },
  {
    value: 'fullPrimer',
    label: 'Based on full duplex',
    description: 'Entire primer treated as a perfect duplex',
  },
  {
    value: 'alignment',
    label: 'Based on alignment',
    description: 'Local duplex with mismatch and dangling-end corrections',
  },
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
