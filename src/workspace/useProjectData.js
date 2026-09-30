import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import {
  activateProject,
  setMethylation,
  setAgentTabLocked,
  emitEditorBackground,
  listenEditorBackground,
} from '../tauriApi';
import { findOrfs } from '../plugins/orf';
import { EMPTY_ARRAY } from './constants';
import { readHiddenAlnNames } from './hiddenAlignments';

export default function useProjectData({
  projectId,
  initialData,
  topology,
  agentLocked,
  disabledPlugins,
  hidden,
  backendStatus,
  methylationSystems,
  methylationOverlap,
}) {
  const [sequence, setSequence] = useState(initialData?.sequence ?? null);
  const [features, setFeatures] = useState(initialData?.features || EMPTY_ARRAY);
  const [enzymes, setEnzymes] = useState(initialData?.enzymes || EMPTY_ARRAY);
  const [primers, setPrimers] = useState(initialData?.primers || EMPTY_ARRAY);
  const [alignments, setAlignments] = useState(initialData?.alignments || EMPTY_ARRAY);
  const [moleculeType, setMoleculeType] = useState(initialData?.moleculeType || 'dna');
  // Source .ab1 path when the project itself was opened from a trace file.
  const [tracePath, setTracePath] = useState(initialData?.tracePath || null);
  // Alignment track whose chromatogram band is expanded (one at a time).
  const [expandedChromAlnId, setExpandedChromAlnId] = useState(null);
  // Live topology: project windows receive a constant prop, so mirror it into
  // state and refresh from broadcasts / the toggle command response.
  const [topologyLive, setTopologyLive] = useState(initialData?.topology || topology);
  useEffect(() => {
    setTopologyLive((cur) => (cur === topology ? cur : topology));
  }, [topology]);
  // Primers/enzymes/ORFs/alignments are DNA-only features; rna/protein projects
  // render single-strand sequence + features only.
  const isDna = moleculeType === 'dna';
  const isProtein = moleculeType === 'protein';
  // Ref mirror so the guard below never disturbs useCallback dep arrays.
  const agentLockedRef = useRef(agentLocked);
  agentLockedRef.current = agentLocked;
  const handleUnlockAgent = useCallback(() => {
    setAgentTabLocked(projectId, false).catch(() => {});
  }, [projectId]);
  const [showAlignments, setShowAlignments] = useState(true);
  const [hiddenAlignIds, setHiddenAlignIds] = useState(() => {
    const names = readHiddenAlnNames(projectId);
    return (initialData?.alignments || EMPTY_ARRAY)
      .filter((a) => names.includes(a.name))
      .map((a) => a.id);
  });
  const [alignTextOpen, setAlignTextOpen] = useState(false);
  const alignmentEnabled = isDna && !disabledPlugins.includes('alignment');
  const primerDesignEnabled = isDna && !disabledPlugins.includes('primerDesign');
  const mapEnabled = !disabledPlugins.includes('map');
  // Background (watermark) choices offered in the editor context menu and the
  // Diagram nav menu for the current molecule type; a single choice means
  // nothing to switch.
  const backgroundOptions = useMemo(() => {
    const opts = [];
    if (mapEnabled) opts.push({ value: 'map', label: 'Map' });
    if (moleculeType === 'rna' && !disabledPlugins.includes('rnaFold'))
      opts.push({ value: 'folding', label: 'Folding' });
    return opts.length ? [{ value: 'none', label: 'None' }, ...opts] : [];
  }, [moleculeType, mapEnabled, disabledPlugins]);

  const [primerOverviewOpen, setPrimerOverviewOpen] = useState(false);
  const [detectFeaturesOpen, setDetectFeaturesOpen] = useState(false);
  const [myPrimersOpen, setMyPrimersOpen] = useState(false);
  const [myEnzymesOpen, setMyEnzymesOpen] = useState(false);
  const [enzymeDbOpen, setEnzymeDbOpen] = useState(false);
  const [myPrimerBinding, setMyPrimerBinding] = useState({ loading: false, results: [] });
  const [pluginDialogs, setPluginDialogs] = useState({});
  const openPrimerEditorRef = useRef(null);
  const openFeatureEditorRef = useRef(null);
  const [mapViewOpen, setMapViewOpen] = useState(false);
  // Editor background (watermark) per molecule type, persisted globally and
  // shared across projects of the same type: dna → none|map, rna →
  // none|map|folding, protein → none|map.
  const [backgrounds, setBackgrounds] = useState(() => {
    const fallback = { dna: 'none', rna: 'none', protein: 'none' };
    try {
      const stored = JSON.parse(localStorage.getItem('editorBackground'));
      if (stored && typeof stored === 'object') return { ...fallback, ...stored };
      // Migrate the legacy global watermark toggles.
      const migrated = { ...fallback };
      if (JSON.parse(localStorage.getItem('mapWatermark'))) migrated.dna = 'map';
      if (JSON.parse(localStorage.getItem('foldWatermark'))) migrated.rna = 'folding';
      localStorage.setItem('editorBackground', JSON.stringify(migrated));
      localStorage.removeItem('mapWatermark');
      localStorage.removeItem('foldWatermark');
      return migrated;
    } catch {
      return fallback;
    }
  });
  const setEditorBackground = useCallback((type, value) => {
    setBackgrounds((prev) => {
      const next = { ...prev, [type]: value };
      try {
        localStorage.setItem('editorBackground', JSON.stringify(next));
      } catch {
        // storage may be unavailable; toggle still applies in-memory
      }
      emitEditorBackground(next);
      return next;
    });
  }, []);
  const background = backgrounds[moleculeType] || 'none';
  // storage events don't propagate between Tauri webview windows, so sync
  // via a Tauri broadcast; keep the storage listener as a browser fallback.
  // Both fire only in *other* documents — no feedback loop.
  useEffect(() => {
    const onStorage = (e) => {
      if (e.key !== 'editorBackground' || e.newValue == null) return;
      try {
        setBackgrounds((prev) => ({ ...prev, ...JSON.parse(e.newValue) }));
      } catch {
        // ignore malformed values
      }
    };
    window.addEventListener('storage', onStorage);
    const listener = listenEditorBackground((v) => {
      if (v && typeof v === 'object') setBackgrounds((prev) => ({ ...prev, ...v }));
    });
    return () => {
      window.removeEventListener('storage', onStorage);
      listener.close();
    };
  }, []);

  // Shared with the workspace component (Save As rekeys it in place) and with
  // the backend-facing effects below; created here because the methylation
  // sync needs it.
  const projectIdRef = useRef(projectId);

  // Sync methylation settings with the backend when they change.
  // Deferred while hidden: the backend command applies to the active project.
  const methKey = useMemo(
    () => methylationSystems.join(',') + '|' + methylationOverlap,
    [methylationSystems, methylationOverlap],
  );
  const syncedMethKeyRef = useRef('');
  useEffect(() => {
    // Methylation/Dam/Dcm only applies to DNA; skip for rna/protein projects.
    if (hidden || !isDna) return;
    if (agentLocked) return;
    if (syncedMethKeyRef.current === methKey) return;
    if (backendStatus !== 'online' || !sequence) return;
    let cancelled = false;
    (async () => {
      try {
        const pid = projectIdRef.current;
        await activateProject(pid);
        const data = await setMethylation(methylationSystems, methylationOverlap, pid);
        if (cancelled || projectIdRef.current !== pid) return;
        if (data && !data.error && data.enzymes) {
          setEnzymes(data.enzymes);
        }
        syncedMethKeyRef.current = methKey;
      } catch (e) {
        console.error('methylation sync error:', e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [
    methKey,
    hidden,
    backendStatus,
    sequence,
    methylationSystems,
    methylationOverlap,
    isDna,
    agentLocked,
  ]);

  const [showOrfs, setShowOrfs] = useState(false);
  const orfEnabled = isDna && !disabledPlugins.includes('orf') && showOrfs;
  const [orfFeatures, setOrfFeatures] = useState(EMPTY_ARRAY);
  // ORFs are computed by the backend on the active project's sequence; refetch
  // whenever the toggle, sequence, topology, or backend availability changes.
  // Old ORFs stay visible during the refetch to avoid flicker.
  useEffect(() => {
    if (hidden || !orfEnabled || backendStatus !== 'online' || !sequence) {
      setOrfFeatures(EMPTY_ARRAY);
      return undefined;
    }
    let cancelled = false;
    findOrfs()
      .then((feats) => {
        if (!cancelled) setOrfFeatures(feats);
      })
      .catch((e) => {
        console.error('ORF search error:', e);
        if (!cancelled) setOrfFeatures(EMPTY_ARRAY);
      });
    return () => {
      cancelled = true;
    };
  }, [orfEnabled, backendStatus, sequence, topologyLive, hidden]);

  return {
    sequence,
    setSequence,
    features,
    setFeatures,
    enzymes,
    setEnzymes,
    primers,
    setPrimers,
    alignments,
    setAlignments,
    moleculeType,
    setMoleculeType,
    tracePath,
    setTracePath,
    expandedChromAlnId,
    setExpandedChromAlnId,
    topologyLive,
    setTopologyLive,
    isDna,
    isProtein,
    agentLockedRef,
    handleUnlockAgent,
    showAlignments,
    setShowAlignments,
    hiddenAlignIds,
    setHiddenAlignIds,
    alignTextOpen,
    setAlignTextOpen,
    alignmentEnabled,
    primerDesignEnabled,
    mapEnabled,
    backgroundOptions,
    primerOverviewOpen,
    setPrimerOverviewOpen,
    detectFeaturesOpen,
    setDetectFeaturesOpen,
    myPrimersOpen,
    setMyPrimersOpen,
    myEnzymesOpen,
    setMyEnzymesOpen,
    enzymeDbOpen,
    setEnzymeDbOpen,
    myPrimerBinding,
    setMyPrimerBinding,
    pluginDialogs,
    setPluginDialogs,
    openPrimerEditorRef,
    openFeatureEditorRef,
    mapViewOpen,
    setMapViewOpen,
    background,
    setEditorBackground,
    showOrfs,
    setShowOrfs,
    orfFeatures,
    projectIdRef,
  };
}
