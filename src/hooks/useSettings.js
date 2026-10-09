import { useState, useCallback, useMemo } from 'react';
import { SHOW_GC_CONTENT_KEY, GC_WINDOW_SIZE_KEY } from '../plugins/gcContent';
import { ENZYME_PROVIDER_VALUES } from '../enzymeProviders';
import { isTauri, setMcpConfig, getMcpConfig } from '../tauriApi';

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
  const [showGcContent, setShowGcContent] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem(SHOW_GC_CONTENT_KEY)) || false;
    } catch {
      return false;
    }
  });
  const [gcWindowSize, setGcWindowSize] = useState(() => {
    try {
      const v = JSON.parse(localStorage.getItem(GC_WINDOW_SIZE_KEY));
      return Number.isFinite(v) && v >= 1 ? Math.round(v) : 11;
    } catch {
      return 11;
    }
  });
  const [alwaysExpandFeatures, setAlwaysExpandFeatures] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('alwaysExpandFeatures')) || false;
    } catch {
      return false;
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
  const [methylationOverlap, setMethylationOverlap] = useState(2);
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

  const onToggleFeatures = useCallback(() => setShowFeatures((v) => !v), []);
  const onToggleGcContent = useCallback(
    () =>
      setShowGcContent((v) => {
        const next = !v;
        try {
          localStorage.setItem(SHOW_GC_CONTENT_KEY, JSON.stringify(next));
        } catch {
          // storage may be unavailable; toggle still applies in-memory
        }
        return next;
      }),
    [],
  );
  const onGcWindowSizeChange = useCallback((next) => {
    if (!Number.isFinite(next)) return;
    const v = Math.min(999, Math.max(1, Math.round(next)));
    setGcWindowSize(v);
    try {
      localStorage.setItem(GC_WINDOW_SIZE_KEY, JSON.stringify(v));
    } catch {
      // storage may be unavailable; selection still applies in-memory
    }
  }, []);
  // Generic per-plugin UI state handed to the nav menu (featuresMenuItem
  // toggles), the editor (track lanes) and the settings page (settingsField).
  const pluginToggles = useMemo(
    () => ({
      gcContent: { checked: showGcContent, onToggle: onToggleGcContent },
    }),
    [showGcContent, onToggleGcContent],
  );
  const pluginSettings = useMemo(
    () => ({
      gcContent: { value: gcWindowSize, onChange: onGcWindowSizeChange },
    }),
    [gcWindowSize, onGcWindowSizeChange],
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

  return {
    disabledPlugins,
    handleTogglePlugin,
    autoAddPrimers,
    onToggleAutoAddPrimers,
    showFeatures,
    onToggleFeatures,
    showGcContent,
    onToggleGcContent,
    gcWindowSize,
    onGcWindowSizeChange,
    alwaysExpandFeatures,
    onToggleAlwaysExpandFeatures,
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
    methylationSystems,
    setMethylationSystems,
    methylationOverlap,
    setMethylationOverlap,
    primerSeedLength,
    setPrimerSeedLength,
    alignmentAlgorithm,
    onAlignmentAlgorithmChange,
    tmParams,
    setTmParams,
    mcpConfig,
    setMcpConfigState,
    handleMcpConfigChange,
    pluginToggles,
    pluginSettings,
  };
}
