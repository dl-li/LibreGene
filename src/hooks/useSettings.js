import { useState, useCallback, useMemo } from 'react';
import { plugins } from '../plugins';
import { ENZYME_PROVIDER_VALUES } from '../enzymeProviders';
import { isTauri, setMcpConfig, getMcpConfig } from '../tauriApi';

// Generic per-plugin UI state derived from the registry: featuresMenuItem
// declares a persisted boolean toggle, settingsField a persisted number.
const menuPlugins = plugins.filter((p) => p.featuresMenuItem);
const fieldPlugins = plugins.filter((p) => p.settingsField);
const toggleKey = (p) => `pluginToggle:${p.id}`;
const fieldKey = (p) => `pluginSetting:${p.id}`;

const readStoredBool = (key, fallback) => {
  try {
    const v = JSON.parse(localStorage.getItem(key));
    return typeof v === 'boolean' ? v : fallback;
  } catch {
    return fallback;
  }
};

const clampField = (field, v) => {
  let n = Math.round(v);
  if (field.min != null) n = Math.max(field.min, n);
  if (field.max != null) n = Math.min(field.max, n);
  return n;
};

const readStoredNumber = (field, key) => {
  const fallback = field.defaultValue ?? field.min ?? 0;
  try {
    const v = JSON.parse(localStorage.getItem(key));
    if (Number.isFinite(v)) return clampField(field, v);
  } catch {
    // fall through to the declared default
  }
  return fallback;
};

// Valid values for the persisted enzyme filter (ENZYME_FILTER_OPTIONS in
// EditorNavMenu.jsx plus the dynamic 'myEnzymes' entry).
const ENZYME_FILTER_VALUES = new Set([
  'all',
  'unique+twice',
  'unique',
  'unique6',
  'twice',
  'blunt',
  'overhang5',
  'overhang3',
  'iis',
  'rec4',
  'rec5',
  'rec6',
  'rec8p',
  'myEnzymes',
]);

// MCP server config defaults; legacy localStorage entries lack requireAuth
// and must default to verifying the access token.
const MCP_CONFIG_DEFAULTS = { enabled: true, port: 8766, requireAuth: true };

// Per-molecule-type sequence case display: 'asis' keeps the stored case,
// 'upper'/'lower' force a case at render time. Defaults match the historical
// behavior (protein always uppercase).
const CASE_DISPLAY_DEFAULTS = { dna: 'asis', rna: 'asis', protein: 'upper' };
const CASE_DISPLAY_VALUES = new Set(['asis', 'upper', 'lower']);

export default function useSettings() {
  const [disabledPlugins, setDisabledPlugins] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('disabledPlugins')) || [];
    } catch {
      return [];
    }
  });
  const [autoAddPrimers, setAutoAddPrimers] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('autoAddPrimers')) || false;
    } catch {
      return false;
    }
  });
  const handleTogglePlugin = useCallback((pluginId) => {
    setDisabledPlugins((prev) => {
      const next = prev.includes(pluginId)
        ? prev.filter((x) => x !== pluginId)
        : [...prev, pluginId];
      try {
        localStorage.setItem('disabledPlugins', JSON.stringify(next));
      } catch {
        // storage may be unavailable; plugin toggle still applies in-memory
      }
      return next;
    });
  }, []);

  // MCP server config: persisted in localStorage, pushed to the Rust side on
  // every change so the loopback server starts/stops/restarts without an app
  // restart.
  const handleMcpConfigChange = useCallback((next) => {
    setMcpConfigState(next);
    try {
      localStorage.setItem('mcpConfig', JSON.stringify(next));
    } catch {
      // storage may be unavailable; config still applies in-memory
    }
    if (isTauri) {
      setMcpConfig(Boolean(next.enabled), Number(next.port), next.requireAuth !== false)
        .then(() => getMcpConfig())
        .then((cfg) => {
          // A failed bind flips enabled off server-side; adopt that truth.
          if (!cfg) return;
          const actual = {
            enabled: !!cfg.enabled,
            port: Number(cfg.port),
            requireAuth: cfg.requireAuth !== false,
          };
          if (
            actual.enabled === Boolean(next.enabled) &&
            actual.port === Number(next.port) &&
            actual.requireAuth === (next.requireAuth !== false)
          ) {
            return;
          }
          setMcpConfigState(actual);
          try {
            localStorage.setItem('mcpConfig', JSON.stringify(actual));
          } catch {
            // storage may be unavailable; config still applies in-memory
          }
        })
        .catch(() => {});
    }
  }, []);
  const [showFeatures, setShowFeatures] = useState(true);
  const [pluginToggleValues, setPluginToggleValues] = useState(() =>
    Object.fromEntries(
      menuPlugins.map((p) => [
        p.id,
        readStoredBool(toggleKey(p), !!p.featuresMenuItem.defaultChecked),
      ]),
    ),
  );
  const [pluginFieldValues, setPluginFieldValues] = useState(() =>
    Object.fromEntries(
      fieldPlugins.map((p) => [p.id, readStoredNumber(p.settingsField, fieldKey(p))]),
    ),
  );
  const [alwaysExpandFeatures, setAlwaysExpandFeatures] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('alwaysExpandFeatures')) || false;
    } catch {
      return false;
    }
  });
  const [alwaysExpandPrimers, setAlwaysExpandPrimers] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('alwaysExpandPrimers')) ?? true;
    } catch {
      return true;
    }
  });
  const [showPrimers, setShowPrimers] = useState(true);
  const [featureLabelsBelow, setFeatureLabelsBelow] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('featureLabelsBelow')) || false;
    } catch {
      return false;
    }
  });
  const [showEnzymes, setShowEnzymes] = useState(true);
  const [enzymeFilter, setEnzymeFilter] = useState(() => {
    try {
      const v = JSON.parse(localStorage.getItem('enzymeFilter'));
      return v && ENZYME_FILTER_VALUES.has(v) ? v : 'unique+twice';
    } catch {
      return 'unique+twice';
    }
  });
  const [methylationSystems, setMethylationSystems] = useState(['dam', 'dcm', 'ecoki']);
  const [enzymeProvider, setEnzymeProvider] = useState(() => {
    try {
      const v = JSON.parse(localStorage.getItem('enzymeProvider'));
      return v && ENZYME_PROVIDER_VALUES.has(v) ? v : 'all';
    } catch {
      return 'all';
    }
  });
  const [viewMode, setViewMode] = useState(() => {
    try {
      const v = JSON.parse(localStorage.getItem('viewMode'));
      return v === 'continuous' ? 'continuous' : 'wrap';
    } catch {
      return 'wrap';
    }
  });
  const [showDoubleStrand, setShowDoubleStrand] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('showDoubleStrand')) || false;
    } catch {
      return false;
    }
  });
  const [methylationOverlap, setMethylationOverlap] = useState(2);
  // Hard cap on displayed cut sites per enzyme, applied to every enzyme filter
  // including "All". Guards the map against 4-cutters exploding on long sequences.
  const [maxEnzymeSites, setMaxEnzymeSites] = useState(() => {
    try {
      const v = JSON.parse(localStorage.getItem('maxEnzymeSites'));
      return Number.isFinite(v) && v >= 1 ? Math.round(v) : 50;
    } catch {
      return 50;
    }
  });
  const onMaxEnzymeSitesChange = useCallback((next) => {
    if (!Number.isFinite(next) || next < 1) return;
    const v = Math.round(next);
    setMaxEnzymeSites(v);
    try {
      localStorage.setItem('maxEnzymeSites', JSON.stringify(v));
    } catch {
      // storage may be unavailable; selection still applies in-memory
    }
  }, []);
  const [primerSeedLength, setPrimerSeedLength] = useState(10);
  const [alignmentAlgorithm, setAlignmentAlgorithm] = useState(() => {
    const v = localStorage.getItem('alignmentAlgorithm');
    return v === 'smith-waterman' ? 'smith-waterman' : 'blast';
  });
  const onAlignmentAlgorithmChange = useCallback((next) => {
    if (next !== 'blast' && next !== 'smith-waterman') return;
    setAlignmentAlgorithm(next);
    try {
      localStorage.setItem('alignmentAlgorithm', next);
    } catch {
      // storage may be unavailable; selection still applies in-memory
    }
  }, []);
  const [tmParams, setTmParams] = useState({
    naConc: 0.05,
    mgConc: 0,
    dntpConc: 0,
    trisConc: 0,
    primerConc: 2.5e-7,
  });
  const [mcpConfig, setMcpConfigState] = useState(() => {
    try {
      return { ...MCP_CONFIG_DEFAULTS, ...(JSON.parse(localStorage.getItem('mcpConfig')) || {}) };
    } catch {
      return MCP_CONFIG_DEFAULTS;
    }
  });
  const [caseDisplay, setCaseDisplay] = useState(() => {
    const out = { ...CASE_DISPLAY_DEFAULTS };
    try {
      const raw = JSON.parse(localStorage.getItem('caseDisplay')) || {};
      for (const k of Object.keys(out)) {
        if (CASE_DISPLAY_VALUES.has(raw[k])) out[k] = raw[k];
      }
    } catch {
      // fall through to defaults
    }
    return out;
  });
  const onCaseDisplayChange = useCallback((moleculeType, value) => {
    if (!(moleculeType in CASE_DISPLAY_DEFAULTS) || !CASE_DISPLAY_VALUES.has(value)) return;
    setCaseDisplay((prev) => {
      const next = { ...prev, [moleculeType]: value };
      try {
        localStorage.setItem('caseDisplay', JSON.stringify(next));
      } catch {
        // storage may be unavailable; selection still applies in-memory
      }
      return next;
    });
  }, []);

  const onToggleFeatures = useCallback(() => setShowFeatures((v) => !v), []);
  const handlePluginToggle = useCallback((id) => {
    setPluginToggleValues((prev) => {
      const next = { ...prev, [id]: !prev[id] };
      const p = menuPlugins.find((pl) => pl.id === id);
      if (p) {
        try {
          localStorage.setItem(toggleKey(p), JSON.stringify(next[id]));
        } catch {
          // storage may be unavailable; toggle still applies in-memory
        }
      }
      return next;
    });
  }, []);
  const handlePluginFieldChange = useCallback((id, next) => {
    if (!Number.isFinite(next)) return;
    const p = fieldPlugins.find((pl) => pl.id === id);
    if (!p) return;
    const v = clampField(p.settingsField, next);
    setPluginFieldValues((prev) => ({ ...prev, [id]: v }));
    try {
      localStorage.setItem(fieldKey(p), JSON.stringify(v));
    } catch {
      // storage may be unavailable; selection still applies in-memory
    }
  }, []);
  // Generic per-plugin UI state handed to the nav menu (featuresMenuItem
  // toggles), the editor (track lanes) and the settings page (settingsField).
  const pluginToggles = useMemo(
    () =>
      Object.fromEntries(
        menuPlugins.map((p) => [
          p.id,
          { checked: !!pluginToggleValues[p.id], onToggle: () => handlePluginToggle(p.id) },
        ]),
      ),
    [pluginToggleValues, handlePluginToggle],
  );
  const pluginSettings = useMemo(
    () =>
      Object.fromEntries(
        fieldPlugins.map((p) => [
          p.id,
          { value: pluginFieldValues[p.id], onChange: (v) => handlePluginFieldChange(p.id, v) },
        ]),
      ),
    [pluginFieldValues, handlePluginFieldChange],
  );
  const onTogglePrimers = useCallback(() => setShowPrimers((v) => !v), []);
  const onToggleEnzymes = useCallback(() => setShowEnzymes((v) => !v), []);
  const onToggleAlwaysExpandFeatures = useCallback(
    () =>
      setAlwaysExpandFeatures((v) => {
        const next = !v;
        try {
          localStorage.setItem('alwaysExpandFeatures', JSON.stringify(next));
        } catch {
          // storage may be unavailable; toggle still applies in-memory
        }
        return next;
      }),
    [],
  );
  const onToggleAlwaysExpandPrimers = useCallback(
    () =>
      setAlwaysExpandPrimers((v) => {
        const next = !v;
        try {
          localStorage.setItem('alwaysExpandPrimers', JSON.stringify(next));
        } catch {
          // storage may be unavailable; toggle still applies in-memory
        }
        return next;
      }),
    [],
  );
  const onFeatureLabelsBelowChange = useCallback(
    (next) =>
      setFeatureLabelsBelow(() => {
        try {
          localStorage.setItem('featureLabelsBelow', JSON.stringify(next));
        } catch {
          // storage may be unavailable; toggle still applies in-memory
        }
        return next;
      }),
    [],
  );
  const onToggleAutoAddPrimers = useCallback(
    () =>
      setAutoAddPrimers((v) => {
        const next = !v;
        try {
          localStorage.setItem('autoAddPrimers', JSON.stringify(next));
        } catch {
          // storage may be unavailable; toggle still applies in-memory
        }
        return next;
      }),
    [],
  );

  const onEnzymeFilterChange = useCallback((next) => {
    if (!ENZYME_FILTER_VALUES.has(next)) return;
    setEnzymeFilter(next);
    try {
      localStorage.setItem('enzymeFilter', JSON.stringify(next));
    } catch {
      // storage may be unavailable; selection still applies in-memory
    }
  }, []);

  const onEnzymeProviderChange = useCallback((next) => {
    if (!ENZYME_PROVIDER_VALUES.has(next)) return;
    setEnzymeProvider(next);
    try {
      localStorage.setItem('enzymeProvider', JSON.stringify(next));
    } catch {
      // storage may be unavailable; selection still applies in-memory
    }
  }, []);

  const onViewModeChange = useCallback((next) => {
    if (next !== 'wrap' && next !== 'continuous') return;
    setViewMode(next);
    try {
      localStorage.setItem('viewMode', JSON.stringify(next));
    } catch {
      // storage may be unavailable; selection still applies in-memory
    }
  }, []);

  const onToggleDoubleStrand = useCallback(
    () =>
      setShowDoubleStrand((v) => {
        const next = !v;
        try {
          localStorage.setItem('showDoubleStrand', JSON.stringify(next));
        } catch {
          // storage may be unavailable; toggle still applies in-memory
        }
        return next;
      }),
    [],
  );

  return {
    disabledPlugins,
    handleTogglePlugin,
    autoAddPrimers,
    onToggleAutoAddPrimers,
    showFeatures,
    onToggleFeatures,
    alwaysExpandFeatures,
    onToggleAlwaysExpandFeatures,
    alwaysExpandPrimers,
    onToggleAlwaysExpandPrimers,
    showPrimers,
    onTogglePrimers,
    featureLabelsBelow,
    onFeatureLabelsBelowChange,
    showEnzymes,
    onToggleEnzymes,
    enzymeFilter,
    onEnzymeFilterChange,
    enzymeProvider,
    onEnzymeProviderChange,
    viewMode,
    onViewModeChange,
    showDoubleStrand,
    onToggleDoubleStrand,
    methylationSystems,
    setMethylationSystems,
    methylationOverlap,
    setMethylationOverlap,
    maxEnzymeSites,
    onMaxEnzymeSitesChange,
    primerSeedLength,
    setPrimerSeedLength,
    alignmentAlgorithm,
    onAlignmentAlgorithmChange,
    tmParams,
    setTmParams,
    mcpConfig,
    setMcpConfigState,
    handleMcpConfigChange,
    caseDisplay,
    onCaseDisplayChange,
    pluginToggles,
    pluginSettings,
  };
}
