import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import SequenceEditor from './SequenceEditor';
import {
  getProject,
  getProjectById,
  updateSequence,
  saveFile,
  saveFileDialog,
  setMethylation,
  activateProject,
  updateFeatureFtype,
  updateFeatureColor,
  updateFeatureName,
  updateFeatureLocation,
  updateFeatureStrand,
  addPrimer,
  addPrimers,
  checkPrimersBinding,
  addFeature,
  deleteFeature,
  deletePrimer,
  rekeyProject,
  addAlignment,
  addAlignmentSeq,
  removeAlignment,
  openAlignmentFileDialog,
  listenProjectUpdates,
  setAgentTabLocked,
  emitMapWatermark,
  listenMapWatermark,
  isTauri,
} from './tauriApi';
import { plugins } from './plugins';
import AddAlignmentTextDialog from './plugins/alignment/AddAlignmentTextDialog';
import { createEditHistory } from './editHistory';
import SequenceEditDialog from './SequenceEditDialog';
import FeatureScrollbar from './FeatureScrollbar';
import MapView from './MapView';
import PrimerOverviewDialog from './components/PrimerOverviewDialog';
import DetectFeaturesDialog from './DetectFeaturesDialog';
import MyPrimersDialog from './MyPrimersDialog';
import MyEnzymesDialog from './MyEnzymesDialog';
import EnzymeDatabaseDialog from './EnzymeDatabaseDialog';
import { findOrfs } from './plugins/orf';
import { addMyPrimers, removeMyPrimer, libraryToPrimers } from './myPrimers';
import { setMyEnzymes } from './myEnzymes';

const EMPTY_ARRAY = [];

export default function ProjectWorkspace({
  projectId,
  hidden,
  initialData,
  topology = 'circular',
  backendStatus,
  methylationSystems,
  methylationOverlap,
  primerSeedLength,
  tmParams,
  layoutParams,
  showFeatures,
  onToggleFeatures,
  alwaysExpandFeatures,
  onToggleAlwaysExpandFeatures,
  showPrimers,
  onTogglePrimers,
  showEnzymes,
  onToggleEnzymes,
  enzymeFilter,
  onEnzymeFilterChange,
  disabledPlugins,
  onDirtyChange,
  registerHandle,
  onProjectsSync,
  onRekey,
  myPrimers = [],
  onMyPrimersChange,
  myEnzymes = [],
  onMyEnzymesChange,
  autoAddPrimers = false,
  onToggleAutoAddPrimers,
  // True while this project is bound to an MCP agent tab and locked: only
  // dirty-producing edits are refused; viewing/scrolling/selection stay live.
  agentLocked = false,
}) {
  const [sequence, setSequence] = useState(initialData?.sequence ?? null);
  const [features, setFeatures] = useState(initialData?.features || EMPTY_ARRAY);
  const [enzymes, setEnzymes] = useState(initialData?.enzymes || EMPTY_ARRAY);
  const [primers, setPrimers] = useState(initialData?.primers || EMPTY_ARRAY);
  const [alignments, setAlignments] = useState(initialData?.alignments || EMPTY_ARRAY);
  const [moleculeType, setMoleculeType] = useState(initialData?.moleculeType || 'dna');
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
  const [hiddenAlignIds, setHiddenAlignIds] = useState(EMPTY_ARRAY);
  const [alignTextOpen, setAlignTextOpen] = useState(false);
  const alignmentEnabled = isDna && !disabledPlugins.includes('alignment');
  const primerDesignEnabled = isDna && !disabledPlugins.includes('primerDesign');

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
  // Global persistent toggle (shared across projects, survives restart)
  const [mapWatermark, setMapWatermark] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('mapWatermark')) || false;
    } catch {
      return false;
    }
  });
  const toggleMapWatermark = useCallback(() => {
    setMapWatermark((v) => {
      const next = !v;
      try {
        localStorage.setItem('mapWatermark', JSON.stringify(next));
      } catch {
        // storage may be unavailable; toggle still applies in-memory
      }
      emitMapWatermark(next);
      return next;
    });
  }, []);
  // storage events don't propagate between Tauri webview windows, so sync
  // via a Tauri broadcast; keep the storage listener as a browser fallback.
  // Both fire only in *other* documents — no feedback loop.
  useEffect(() => {
    const onStorage = (e) => {
      if (e.key !== 'mapWatermark' || e.newValue == null) return;
      try {
        setMapWatermark(JSON.parse(e.newValue));
      } catch {
        // ignore malformed values
      }
    };
    window.addEventListener('storage', onStorage);
    const listener = listenMapWatermark((v) => setMapWatermark(!!v));
    return () => {
      window.removeEventListener('storage', onStorage);
      listener.close();
    };
  }, []);
  const [enzymeHoverCuts, setEnzymeHoverCuts] = useState(null);
  const [liveSelection, setLiveSelection] = useState(null);
  const alignmentCacheRef = useRef({});
  const sequenceRef = useRef(sequence);
  const projectIdRef = useRef(projectId);
  const operationGenRef = useRef(0);
  const mainScrollRef = useRef(null);
  const lastSelectionRef = useRef(null);

  // --- Sequence editing state ---
  const editHistoryRef = useRef(createEditHistory());
  const [historyVersion, setHistoryVersion] = useState(0);
  useEffect(() => editHistoryRef.current.subscribe(() => setHistoryVersion((v) => v + 1)), []);
  const canUndo = historyVersion >= 0 && editHistoryRef.current.canUndo();
  const canRedo = historyVersion >= 0 && editHistoryRef.current.canRedo();
  const [editDialog, setEditDialog] = useState({
    open: false,
    mode: 'insert',
    cursorIndex: null,
    selStart: null,
    selEnd: null,
    selectedText: '',
    initialText: '',
    clipboardMeta: null,
  });
  const [restoreState, setRestoreState] = useState({
    version: 0,
    cursorIndex: null,
    selStart: null,
    selEnd: null,
    translationSel: null,
  });
  const undoVersionRef = useRef(0);
  const [isDirty, setIsDirty] = useState(false);
  const isDirtyRef = useRef(false);
  const baselineSequenceRef = useRef(initialData?.sequence ?? '');

  // Direct Save is only allowed for text GenBank formats; binary sources
  // (.dna, .rna, .prot, .fasta, .ab1, ...) must be re-exported via Save As.
  // Protein projects may be re-saved in place as .gpt (protein GenBank).
  const canDirectSave = useMemo(() => {
    const ext = (projectId || '').split('.').pop()?.toLowerCase();
    return ext === 'gbk' || ext === 'gb' || (moleculeType === 'protein' && ext === 'gpt');
  }, [projectId, moleculeType]);

  useEffect(() => {
    isDirtyRef.current = isDirty;
  }, [isDirty]);
  useEffect(() => {
    sequenceRef.current = sequence;
  }, [sequence]);
  useEffect(() => {
    projectIdRef.current = projectId;
  }, [projectId]);

  // Initialize undo history from initialData, or fetch the project when absent
  useEffect(() => {
    if (initialData && initialData.sequence) {
      editHistoryRef.current.reset({
        sequence: initialData.sequence,
        features: initialData.features || EMPTY_ARRAY,
        primers: initialData.primers || EMPTY_ARRAY,
        cursorIndex: null,
        selStart: null,
        selEnd: null,
      });
      baselineSequenceRef.current = initialData.sequence;
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const data = await getProjectById(projectIdRef.current, 'all');
        if (cancelled) return;
        if (data && !data.error && data.sequence) {
          setSequence(data.sequence);
          setFeatures(data.features || EMPTY_ARRAY);
          setEnzymes(data.enzymes || EMPTY_ARRAY);
          setPrimers(data.primers || EMPTY_ARRAY);
          setAlignments(data.alignments || EMPTY_ARRAY);
          setMoleculeType(data.moleculeType || 'dna');
          editHistoryRef.current.reset({
            sequence: data.sequence,
            features: data.features || EMPTY_ARRAY,
            primers: data.primers || EMPTY_ARRAY,
            cursorIndex: null,
            selStart: null,
            selEnd: null,
          });
          baselineSequenceRef.current = data.sequence;
          setIsDirty(false);
        }
      } catch {
        // initial load failed; workspace stays on welcome/empty data
      }
    })();
    return () => {
      cancelled = true;
    };
    // mount-only load; projectId prop changes come from Save As rekey (state preserved)
  }, []);

  // Listen for external mutations on this project (other windows)
  useEffect(() => {
    let listener = null;
    let cancelled = false;
    (async () => {
      let ownLabel = null;
      try {
        if (isTauri) {
          ownLabel = (await import('@tauri-apps/api/window')).getCurrentWindow().label;
        }
      } catch {
        // non-Tauri or window API unavailable; listener just won't filter by label
      }
      if (cancelled) return;
      listener = listenProjectUpdates((msg) => {
        if (cancelled) return;
        // Skip messages from this window (applied by command response)
        if (msg.source && msg.source === ownLabel) return;
        const id = projectIdRef.current;
        let data = null;
        if (msg.activeId === id && msg.data && msg.data.sequence) {
          data = msg.data;
        } else if (msg.projectData && msg.projectData[id] && msg.projectData[id].sequence) {
          data = msg.projectData[id];
        }
        if (!data) return;
        setSequence(data.sequence);
        setFeatures(data.features || EMPTY_ARRAY);
        setEnzymes(data.enzymes || EMPTY_ARRAY);
        setPrimers(data.primers || EMPTY_ARRAY);
        setAlignments(data.alignments || EMPTY_ARRAY);
        setMoleculeType(data.moleculeType || 'dna');
        editHistoryRef.current.reset({
          sequence: data.sequence,
          features: data.features || EMPTY_ARRAY,
          primers: data.primers || EMPTY_ARRAY,
          cursorIndex: null,
          selStart: null,
          selEnd: null,
        });
        baselineSequenceRef.current = data.sequence;
        setIsDirty(data.dirty === true);
      });
    })();
    return () => {
      cancelled = true;
      if (listener) listener.close();
    };
  }, []);

  // Sync methylation settings with the backend when they change.
  // Deferred while hidden: the backend command applies to the active project.
  const methKey = useMemo(
    () => methylationSystems.join(',') + '|' + methylationOverlap,
    [methylationSystems, methylationOverlap],
  );
  const syncedMethKeyRef = useRef(initialData ? '' : methKey);
  useEffect(() => {
    // Methylation/Dam/Dcm only applies to DNA; skip for rna/protein projects.
    if (hidden || !isDna) return;
    if (agentLockedRef.current) return;
    if (syncedMethKeyRef.current === methKey) return;
    if (backendStatus !== 'online' || !sequence) return;
    let cancelled = false;
    (async () => {
      try {
        await activateProject(projectIdRef.current);
        const data = await setMethylation(methylationSystems, methylationOverlap);
        if (cancelled) return;
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
  }, [methKey, hidden, backendStatus, sequence, methylationSystems, methylationOverlap, isDna]);

  // Report dirty state up to App (sidebar dots + title bar)
  useEffect(() => {
    onDirtyChange(projectId, isDirty);
  }, [projectId, isDirty, onDirtyChange]);

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
  }, [orfEnabled, backendStatus, sequence, topology, hidden]);
  const editorFeatures = useMemo(
    () => [...(showFeatures ? features : EMPTY_ARRAY), ...orfFeatures],
    [showFeatures, features, orfFeatures],
  );
  const displayFeatures = useMemo(
    () => (showFeatures ? features : EMPTY_ARRAY),
    [showFeatures, features],
  );
  const mapName = useMemo(() => {
    return projectId
      .split('/')
      .pop()
      .split('\\')
      .pop()
      .replace(/\.[^.]+$/, '');
  }, [projectId]);
  const editorPrimers = useMemo(
    () => (isDna && showPrimers ? primers : EMPTY_ARRAY),
    [isDna, showPrimers, primers],
  );
  const editorLayoutParams = useMemo(() => layoutParams, [layoutParams]);

  const hasProject = sequence !== null && sequence.length > 0;

  const totalNamePairCounts = useMemo(() => {
    const m = new Map();
    for (const e of enzymes || []) {
      const nPairs = (e.cutPairs && e.cutPairs.length) || 1;
      m.set(e.name, (m.get(e.name) || 0) + nPairs);
    }
    return m;
  }, [enzymes]);

  const displayEnzymes = useMemo(() => {
    // Enzymes only exist for DNA; rna/protein render sequence + features only.
    if (!isDna) return EMPTY_ARRAY;
    if (!showEnzymes) return EMPTY_ARRAY;
    const all = enzymes || [];
    if (enzymeFilter === 'all') return all;
    if (enzymeFilter === 'myEnzymes') {
      const set = new Set(myEnzymes);
      return all.filter((e) => set.has(e.name));
    }
    if (enzymeFilter === 'unique') return all.filter((e) => e.isUnique);
    if (enzymeFilter === 'unique6') return all.filter((e) => e.isUnique && e.recSeq?.length === 6);
    if (enzymeFilter === 'twice') return all.filter((e) => totalNamePairCounts.get(e.name) === 2);
    if (enzymeFilter === 'unique+twice')
      return all.filter((e) => e.isUnique || totalNamePairCounts.get(e.name) === 2);
    const cutType = (e) =>
      e.cutType ||
      (e.botCutIndex - e.cutIndex === 0
        ? 'blunt'
        : e.botCutIndex - e.cutIndex > 0
          ? '5overhang'
          : '3overhang');
    if (enzymeFilter === 'blunt') return all.filter((e) => cutType(e) === 'blunt');
    if (enzymeFilter === 'overhang5') return all.filter((e) => cutType(e) === '5overhang');
    if (enzymeFilter === 'overhang3') return all.filter((e) => cutType(e) === '3overhang');
    if (enzymeFilter === 'iis')
      return all.filter((e) => {
        const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
        return pairs.some((cp) => {
          const d = [
            cp.topCutIndex - e.recEnd,
            e.recStart - cp.topCutIndex,
            cp.botCutIndex - e.recEnd,
            e.recStart - cp.botCutIndex,
          ];
          return d.some((v) => v >= 2);
        });
      });
    if (enzymeFilter === 'rec4') return all.filter((e) => e.recSeq?.length === 4);
    if (enzymeFilter === 'rec5') return all.filter((e) => e.recSeq?.length === 5);
    if (enzymeFilter === 'rec6') return all.filter((e) => e.recSeq?.length === 6);
    if (enzymeFilter === 'rec8p') return all.filter((e) => (e.recSeq?.length || 0) >= 8);
    return all.filter((e) => e.isUnique);
  }, [enzymes, enzymeFilter, showEnzymes, totalNamePairCounts, myEnzymes, isDna]);

  const handleSelectionChange = useCallback(
    (sel) => {
      lastSelectionRef.current = sel;
      if (mapViewOpen) setLiveSelection(sel);
    },
    [mapViewOpen],
  );

  // Seed the map's selection highlight from the editor's current selection when opening
  useEffect(() => {
    if (mapViewOpen) {
      setLiveSelection(lastSelectionRef.current ?? null);
    }
  }, [mapViewOpen]);

  // Map view: clicking/dragging on the map restores the corresponding selection in the editor
  const handleMapSelect = useCallback((selStart, selEnd) => {
    setRestoreState({
      version: ++undoVersionRef.current,
      cursorIndex: selEnd + 1,
      selStart,
      selEnd,
      selectionMode: 'text',
      selectedPrimerIds: [],
      isEnzymeSelection: false,
      selectedEnzymeIds: [],
      translationSel: null,
      scrollToIndex: selStart,
    });
    setLiveSelection({ selStart, selEnd });
  }, []);

  const handleMapClear = useCallback(() => {
    setRestoreState({
      version: ++undoVersionRef.current,
      cursorIndex: null,
      selStart: null,
      selEnd: null,
      selectionMode: 'none',
      selectedPrimerIds: [],
      isEnzymeSelection: false,
      selectedEnzymeIds: [],
      translationSel: null,
    });
    setLiveSelection(null);
  }, []);

  // --- Edit request from SequenceEditor: open the confirmation dialog ---
  const handleEditRequest = useCallback((request) => {
    if (agentLockedRef.current) return;
    setEditDialog({
      open: true,
      mode: request.type,
      cursorIndex: request.cursorIndex ?? null,
      selStart: request.selStart ?? null,
      selEnd: request.selEnd ?? null,
      selectedText: request.selectedText ?? '',
      initialText: request.clipboardText ?? '',
      clipboardMeta: request.clipboardMeta ?? null,
    });
  }, []);

  // Record a post-mutation snapshot in undo history. Entries are always the
  // state AFTER a mutation (sequence edits follow the same convention), so
  // undo returns the previous entry — the pre-mutation state — and redo
  // restores the mutation. Snapshots always carry primers so undo/redo can
  // restore them on both sides (UI state + backend via update_sequence).
  const pushHistory = useCallback(
    (overrides = {}) => {
      editHistoryRef.current.push({
        sequence,
        features: features || EMPTY_ARRAY,
        primers: primers || EMPTY_ARRAY,
        cursorIndex: null,
        selStart: null,
        selEnd: null,
        ...overrides,
      });
    },
    [sequence, features, primers],
  );

  const handleFeatureFtypeChange = useCallback(
    async (featureId, newFtype) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await updateFeatureFtype(featureId, newFtype);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          pushHistory({ features: data.features });
          setFeatures(data.features);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('update feature ftype error:', e);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const handleFeatureColorChange = useCallback(
    async (featureId, newColor) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await updateFeatureColor(featureId, newColor);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          pushHistory({ features: data.features });
          setFeatures(data.features);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('update feature color error:', e);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const handleFeatureNameChange = useCallback(
    async (featureId, newName) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await updateFeatureName(featureId, newName);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          pushHistory({ features: data.features });
          setFeatures(data.features);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('update feature name error:', e);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const handlePrimerChange = useCallback(
    async (primerData, { recordHistory = true } = {}) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await addPrimer(primerData);
        if (operationGenRef.current !== gen) return;
        if (data && data.primers) {
          // Batch callers record history once via recordHistory on their last
          // call only — the response carries the full post-batch list.
          if (recordHistory) pushHistory({ primers: data.primers });
          setPrimers(data.primers);
          if (data.alignments) setAlignments(data.alignments);
          if (data.enzymes) setEnzymes(data.enzymes);
          setIsDirty(true);
          if (data.projects) onProjectsSync(data.projects);
        }
      } catch (e) {
        console.error('add primer error:', e);
      }
    },
    [pushHistory, onProjectsSync],
  );

  // --- My Primers library actions ---
  const handleAddPrimerToMyPrimers = useCallback(
    (primer) => {
      if (!primer || !primer.primerSeq) return;
      onMyPrimersChange?.(addMyPrimers([primer]));
    },
    [onMyPrimersChange],
  );

  const handleAddAllPrimersToMyPrimers = useCallback(() => {
    if (!primers.length) return;
    onMyPrimersChange?.(addMyPrimers(primers));
  }, [primers, onMyPrimersChange]);

  const handleDeleteMyPrimer = useCallback(
    (id) => {
      onMyPrimersChange?.(removeMyPrimer(id));
    },
    [onMyPrimersChange],
  );

  // Auto-add primers from opened files to My Primers.
  useEffect(() => {
    if (!autoAddPrimers || !primers.length) return;
    const cur = new Set(myPrimers.map((p) => String(p.seq || p.primerSeq || '').toUpperCase()));
    const missing = primers.some((p) => p.primerSeq && !cur.has(String(p.primerSeq).toUpperCase()));
    if (missing) onMyPrimersChange?.(addMyPrimers(primers));
  }, [autoAddPrimers, primers, myPrimers, onMyPrimersChange]);

  // Check binding of My Primers against the current sequence when the dialog opens.
  useEffect(() => {
    if (!isDna || !myPrimersOpen || !sequence || !myPrimers.length) {
      setMyPrimerBinding({ loading: false, results: [] });
      return;
    }
    let cancelled = false;
    setMyPrimerBinding({ loading: true, results: [] });
    checkPrimersBinding(libraryToPrimers(myPrimers))
      .then((data) => {
        if (cancelled) return;
        setMyPrimerBinding({ loading: false, results: (data && data.results) || [] });
      })
      .catch(() => {
        if (!cancelled) setMyPrimerBinding({ loading: false, results: [] });
      });
    return () => {
      cancelled = true;
    };
  }, [myPrimersOpen, myPrimers, sequence, isDna]);

  const applyAddedPrimers = useCallback(
    (data, gen) => {
      if (operationGenRef.current !== gen) return;
      if (data && data.primers) {
        setPrimers(data.primers);
        if (data.alignments) setAlignments(data.alignments);
        if (data.enzymes) setEnzymes(data.enzymes);
        setIsDirty(true);
        if (data.projects) onProjectsSync(data.projects);
      }
    },
    [onProjectsSync],
  );

  const handleAddMyPrimerToFile = useCallback(
    async (entry) => {
      if (agentLockedRef.current) return;
      if (!entry) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await addPrimers(libraryToPrimers([entry]));
        if (data && data.primers && operationGenRef.current === gen) {
          pushHistory({ primers: data.primers });
        }
        applyAddedPrimers(data, gen);
      } catch (e) {
        console.error('add primer from My Primers error:', e);
      }
    },
    [pushHistory, applyAddedPrimers],
  );

  const handleAddAllBindingPrimers = useCallback(async () => {
    if (agentLockedRef.current) return;
    const bindingIds = new Set(
      (myPrimerBinding.results || []).filter((r) => r.binds).map((r) => r.id),
    );
    const inFile = new Set(primers.map((p) => String(p.primerSeq || '').toUpperCase()));
    const toAdd = myPrimers.filter(
      (p) => bindingIds.has(p.id) && !inFile.has(String(p.seq || p.primerSeq || '').toUpperCase()),
    );
    if (!toAdd.length) return;
    const gen = ++operationGenRef.current;
    try {
      const data = await addPrimers(libraryToPrimers(toAdd));
      if (data && data.primers && operationGenRef.current === gen) {
        pushHistory({ primers: data.primers });
      }
      applyAddedPrimers(data, gen);
    } catch (e) {
      console.error('add binding primers error:', e);
    }
  }, [myPrimerBinding.results, myPrimers, primers, pushHistory, applyAddedPrimers]);

  const handleMyEnzymesChange = useCallback(
    (list) => {
      onMyEnzymesChange?.(setMyEnzymes(list));
    },
    [onMyEnzymesChange],
  );

  const handleFeatureAdd = useCallback(
    async (featureData, { recordHistory = true } = {}) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      const { locationStr, ...feature } = featureData;
      // errors propagate so the dialog can display them; batch callers record
      // history once via recordHistory on their last call only — the response
      // carries the full post-batch list
      const data = await addFeature(feature, locationStr);
      if (operationGenRef.current !== gen) return;
      if (data && data.features) {
        if (recordHistory) pushHistory({ features: data.features });
        setFeatures(data.features);
        if (data.enzymes) setEnzymes(data.enzymes);
        setIsDirty(true);
        if (data.projects) onProjectsSync(data.projects);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const handleFeatureStrandChange = useCallback(
    async (featureId, strand) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await updateFeatureStrand(featureId, strand);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          pushHistory({ features: data.features });
          setFeatures(data.features);
          if (data.enzymes) setEnzymes(data.enzymes);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('update feature strand error:', e);
      }
    },
    [pushHistory],
  );

  const handleFeatureLocationChange = useCallback(
    async (featureId, locationStr) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      // errors propagate so the dialog can display them
      const data = await updateFeatureLocation(featureId, locationStr);
      if (operationGenRef.current !== gen) return;
      if (data && data.features) {
        pushHistory({ features: data.features });
        setFeatures(data.features);
        if (data.projects) onProjectsSync(data.projects);
        setIsDirty(true);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const handleDeleteFeature = useCallback(
    async (featureId) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await deleteFeature(featureId);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          pushHistory({ features: data.features });
          setFeatures(data.features);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('delete feature error:', e);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const handleDeletePrimer = useCallback(
    async (primerId) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await deletePrimer(primerId);
        if (operationGenRef.current !== gen) return;
        if (data && data.primers) {
          pushHistory({ primers: data.primers });
          setPrimers(data.primers);
          if (data.alignments) setAlignments(data.alignments);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('delete primer error:', e);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const addAlignmentFiles = useCallback(
    async (paths) => {
      const gen = ++operationGenRef.current;
      const failed = [];
      let added = 0;
      let lastData = null;
      for (const path of paths) {
        try {
          const data = await addAlignment(path);
          if (operationGenRef.current !== gen) return null;
          if (data && data.error) {
            failed.push({ path, error: data.error });
          } else if (data) {
            added += 1;
            lastData = data;
          }
        } catch (e) {
          failed.push({ path, error: String(e?.message || e) });
        }
      }
      if (lastData) {
        if (lastData.alignments) setAlignments(lastData.alignments);
        if (lastData.projects) onProjectsSync(lastData.projects);
        setIsDirty(true);
      }
      return { added, failed };
    },
    [onProjectsSync],
  );

  const handleAddAlignment = useCallback(async () => {
    if (agentLockedRef.current) return;
    const paths = await openAlignmentFileDialog();
    if (!paths || paths.length === 0) return;
    const result = await addAlignmentFiles(paths);
    if (result && result.failed.length > 0) {
      const lines = result.failed.map((f) => {
        const name = f.path.replace(/\\/g, '/').split('/').pop();
        return `${name}: ${f.error}`;
      });
      throw new Error(lines.join('\n'));
    }
  }, [addAlignmentFiles]);

  const handleAddAlignmentText = useCallback(
    async (name, seq) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      const data = await addAlignmentSeq(name, seq);
      if (operationGenRef.current !== gen) return;
      if (data && data.error) throw new Error(data.error);
      if (data) {
        if (data.alignments) setAlignments(data.alignments);
        if (data.projects) onProjectsSync(data.projects);
        setIsDirty(true);
      }
    },
    [onProjectsSync],
  );

  const handleToggleAlignmentVisible = useCallback((alignmentId) => {
    setHiddenAlignIds((prev) =>
      prev.includes(alignmentId) ? prev.filter((x) => x !== alignmentId) : [...prev, alignmentId],
    );
  }, []);

  const visibleAlignments = useMemo(
    () =>
      alignmentEnabled && showAlignments
        ? alignments.filter((a) => !hiddenAlignIds.includes(a.id))
        : EMPTY_ARRAY,
    [alignmentEnabled, showAlignments, alignments, hiddenAlignIds],
  );

  const handleRemoveAlignment = useCallback(
    async (alignmentId) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await removeAlignment(alignmentId);
        if (operationGenRef.current !== gen) return;
        if (data && data.alignments) {
          setAlignments(data.alignments);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('remove alignment error:', e);
      }
    },
    [onProjectsSync],
  );

  /**
   * 调整特征/注释放置位置以适配编辑后的序列。
   * 编辑会删除 [editStart, editEnd] 区间（oldLen 个碱基），
   * 然后插入 newLen 个碱基。
   * 编辑区之外的特征位置保持与原序列的相对偏移不变。
   * 等长替换（delta === 0）不改变任何坐标，特征原样保留。
   */
  const adjustAnnotations = useCallback((anns, editStart, editEnd, oldLen, newLen) => {
    const delta = newLen - oldLen;
    if (delta === 0) return anns; // no-op / equal-length replace keeps all coordinates

    return anns
      .map((ann) => {
        const adjustSegments = (segments) => {
          if (!segments || !segments.length) return segments;
          return segments
            .map((seg) => {
              let { start: s, end: e } = seg;
              if (e < editStart) return seg;
              if (s > editEnd) return { ...seg, start: s + delta, end: e + delta };
              // spans
              const ns = s < editStart ? s : editStart + newLen;
              const ne = e > editEnd ? e + delta : editStart + newLen - 1;
              if (ns > ne) return null;
              return { ...seg, start: ns, end: ne };
            })
            .filter(Boolean);
        };

        let newStart, newEnd;
        if (ann.end < editStart) {
          // entirely before – unchanged
          return ann;
        }
        if (ann.start > editEnd) {
          // entirely after – shift
          newStart = ann.start + delta;
          newEnd = ann.end + delta;
        } else {
          // spans the edit
          newStart = ann.start < editStart ? ann.start : editStart + newLen;
          newEnd = ann.end > editEnd ? ann.end + delta : editStart + newLen - 1;
        }

        if (newStart > newEnd) return null;

        const result = { ...ann, start: newStart, end: newEnd };
        if (ann.segments) result.segments = adjustSegments(ann.segments);
        return result;
      })
      .filter(Boolean);
  }, []);

  // --- Edit dialog confirmed (insert/delete/replace) ---
  const handleEditConfirm = useCallback(
    async (result) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      const { mode, cursorIndex, selStart, selEnd } = editDialog;
      let newSeq;
      const currentSeq = sequence || '';
      let editStart, editEnd, oldLen, newLen;

      // Compute the new sequence and save edit parameters for feature adjustment
      if (mode === 'insert') {
        const cleaned = (result.sequence || '').replace(/\s/g, '');
        if (!cleaned) {
          setEditDialog((prev) => ({ ...prev, open: false }));
          return;
        }
        editStart = cursorIndex;
        editEnd = cursorIndex - 1; // no deletion range
        oldLen = 0;
        newLen = cleaned.length;
        newSeq = currentSeq.slice(0, cursorIndex) + cleaned + currentSeq.slice(cursorIndex);
      } else if (mode === 'delete') {
        if (selStart === null || selEnd === null) {
          setEditDialog((prev) => ({ ...prev, open: false }));
          return;
        }
        editStart = selStart;
        editEnd = selEnd;
        oldLen = selEnd - selStart + 1;
        newLen = 0;
        newSeq = currentSeq.slice(0, selStart) + currentSeq.slice(selEnd + 1);
      } else if (mode === 'replace') {
        const cleaned = (result.sequence || '').replace(/\s/g, '');
        if (selStart === null || selEnd === null) {
          setEditDialog((prev) => ({ ...prev, open: false }));
          return;
        }
        editStart = selStart;
        editEnd = selEnd;
        oldLen = selEnd - selStart + 1;
        newLen = cleaned.length;
        newSeq = currentSeq.slice(0, selStart) + cleaned + currentSeq.slice(selEnd + 1);
      } else {
        return;
      }

      // Close dialog
      setEditDialog((prev) => ({ ...prev, open: false }));

      // Compute adjusted features BEFORE backend call (for history and optimistic update)
      let adjustedFeatures = adjustAnnotations(
        features || EMPTY_ARRAY,
        editStart,
        editEnd,
        oldLen,
        newLen,
      );

      // Merge annotation features from clipboard
      const annotations = result.annotations;
      let mergedFeatures = adjustedFeatures;
      if (annotations && annotations.features && annotations.features.length > 0) {
        const insertAnchor = mode === 'insert' ? cursorIndex : selStart;
        const existingNames = new Set(adjustedFeatures.map((f) => f.name));
        const newFeats = annotations.features.map((af, i) => {
          let name = af.name;
          if (existingNames.has(name)) {
            let n = 2;
            while (existingNames.has(`${name} (${n})`)) n++;
            name = `${name} (${n})`;
          }
          existingNames.add(name);
          const segs = af.segments.map((s) => ({
            start: insertAnchor + s.start,
            end: insertAnchor + s.end,
          }));
          return {
            id: `feature_${Date.now()}_${i}_${Math.random().toString(36).slice(2, 8)}`,
            name,
            ftype: af.ftype,
            color: af.color,
            strand: af.strand,
            notes: af.notes,
            qualifiers: af.qualifiers,
            start: segs[0].start,
            end: segs[segs.length - 1].end,
            segments: segs,
          };
        });
        mergedFeatures = [...adjustedFeatures, ...newFeats];
      }

      // Push new state to undo history (includes adjusted features for correct undo)
      editHistoryRef.current.push({
        sequence: newSeq,
        features: mergedFeatures,
        primers: primers || EMPTY_ARRAY,
        cursorIndex,
        selStart: mode === 'insert' ? null : selStart,
        selEnd: mode === 'insert' ? null : selEnd,
      });

      // Optimistic UI update
      setSequence(newSeq);
      setFeatures(mergedFeatures);
      setIsDirty(true);

      // Send to backend for recomputation (enzymes, primer binding sites)
      try {
        const data = await updateSequence(newSeq, mergedFeatures);
        if (operationGenRef.current !== gen) return;
        if (data && !data.error) {
          setSequence(data.sequence);
          setFeatures(data.features || mergedFeatures); // backend recomputed CDS/mRNA translations
          setEnzymes(data.enzymes || EMPTY_ARRAY);
          setPrimers(data.primers || EMPTY_ARRAY);
          setAlignments(data.alignments || EMPTY_ARRAY);
          if (data.projects) onProjectsSync(data.projects);

          // Import primers from clipboard annotations (DNA only)
          if (annotations && annotations.primers && annotations.primers.length > 0 && isDna) {
            const allNames = new Set([
              ...(data.primers || []).map((p) => p.name),
              ...(data.features || mergedFeatures).map((f) => f.name),
            ]);
            const primerImports = annotations.primers.map((ap) => {
              let name = ap.name;
              if (allNames.has(name)) {
                let n = 2;
                while (allNames.has(`${name} (${n})`)) n++;
                name = `${name} (${n})`;
              }
              allNames.add(name);
              return { name, type: ap.type, primerSeq: ap.primerSeq };
            });
            try {
              const pData = await addPrimers(primerImports);
              if (operationGenRef.current !== gen) return;
              if (pData && pData.primers) {
                setPrimers(pData.primers);
                setIsDirty(true);
              }
            } catch (pe) {
              console.error('import primers error:', pe);
            }
          }
        } else {
          console.error('update_sequence error:', data?.error || 'unknown');
          // Re-fetch to recover from optimistic update
          try {
            const refresh = await getProject('all');
            if (refresh && !refresh.error && refresh.sequence) {
              setSequence(refresh.sequence);
              setFeatures(refresh.features || EMPTY_ARRAY);
              setEnzymes(refresh.enzymes || EMPTY_ARRAY);
              setPrimers(refresh.primers || EMPTY_ARRAY);
              setAlignments(refresh.alignments || EMPTY_ARRAY);
            }
          } catch {
            // recovery fetch failed; state left as-is
          }
        }
      } catch (e) {
        console.error('update_sequence exception:', e);
      }
    },
    [sequence, editDialog, adjustAnnotations, features, primers, onProjectsSync, isDna],
  );

  // --- Edit dialog cancelled ---
  const handleEditCancel = useCallback(() => {
    setEditDialog((prev) => ({ ...prev, open: false }));
  }, []);

  // --- Undo ---
  const handleUndo = useCallback(async () => {
    if (agentLockedRef.current) return;
    const gen = ++operationGenRef.current;
    const snapshot = editHistoryRef.current.undo();
    if (!snapshot) return;

    // Restore cursor/selection in SequenceEditor (use ref for atomic version)
    setRestoreState({
      version: ++undoVersionRef.current,
      cursorIndex: snapshot.cursorIndex,
      selStart: snapshot.selStart,
      selEnd: snapshot.selEnd,
    });

    // Send to backend for recomputation
    try {
      const data = await updateSequence(snapshot.sequence, snapshot.features, snapshot.primers);
      if (operationGenRef.current !== gen) return;
      if (data && !data.error) {
        setSequence(data.sequence);
        setFeatures(data.features || snapshot.features || EMPTY_ARRAY);
        setEnzymes(data.enzymes || EMPTY_ARRAY);
        // Backend response carries freshly recomputed binding sites
        setPrimers(data.primers || snapshot.primers || EMPTY_ARRAY);
        setAlignments(data.alignments || EMPTY_ARRAY);
        if (data.projects) onProjectsSync(data.projects);
        // Accurate dirty check: undo to saved state = not dirty
        setIsDirty(snapshot.sequence !== baselineSequenceRef.current);
      } else {
        // Re-fetch to recover
        try {
          const refresh = await getProject('all');
          if (refresh && !refresh.error && refresh.sequence) {
            setSequence(refresh.sequence);
            setFeatures(refresh.features || EMPTY_ARRAY);
            setEnzymes(refresh.enzymes || EMPTY_ARRAY);
            setPrimers(refresh.primers || EMPTY_ARRAY);
            setAlignments(refresh.alignments || EMPTY_ARRAY);
          }
        } catch {
          // recovery fetch failed; state left as-is
        }
      }
    } catch (e) {
      console.error('undo error:', e);
    }
  }, [onProjectsSync]);

  // --- Redo ---
  const handleRedo = useCallback(async () => {
    if (agentLockedRef.current) return;
    const gen = ++operationGenRef.current;
    const snapshot = editHistoryRef.current.redo();
    if (!snapshot) return;

    setRestoreState({
      version: ++undoVersionRef.current,
      cursorIndex: snapshot.cursorIndex,
      selStart: snapshot.selStart,
      selEnd: snapshot.selEnd,
    });

    try {
      const data = await updateSequence(snapshot.sequence, snapshot.features, snapshot.primers);
      if (operationGenRef.current !== gen) return;
      if (data && !data.error) {
        setSequence(data.sequence);
        setFeatures(data.features || snapshot.features || EMPTY_ARRAY);
        setEnzymes(data.enzymes || EMPTY_ARRAY);
        setPrimers(data.primers || snapshot.primers || EMPTY_ARRAY);
        setAlignments(data.alignments || EMPTY_ARRAY);
        if (data.projects) onProjectsSync(data.projects);
        // Accurate dirty check: redo back to saved state = not dirty
        setIsDirty(snapshot.sequence !== baselineSequenceRef.current);
      } else {
        try {
          const refresh = await getProject('all');
          if (refresh && !refresh.error && refresh.sequence) {
            setSequence(refresh.sequence);
            setFeatures(refresh.features || EMPTY_ARRAY);
            setEnzymes(refresh.enzymes || EMPTY_ARRAY);
            setPrimers(refresh.primers || EMPTY_ARRAY);
            setAlignments(refresh.alignments || EMPTY_ARRAY);
          }
        } catch {
          // recovery fetch failed; state left as-is
        }
      }
    } catch (e) {
      console.error('redo error:', e);
    }
  }, [onProjectsSync]);

  // --- Save As (defined before Save because Save may reference it) ---
  const handleSaveAs = useCallback(async () => {
    if (!isTauri) return;

    // Protein projects export as protein GenBank (.gpt); DNA/RNA as .gbk.
    const defaultExt = moleculeType === 'protein' ? 'gpt' : 'gbk';
    const rawName = projectIdRef.current
      ? projectIdRef.current.split('/').pop().split('\\').pop()
      : 'sequence.gbk';
    const defaultName = rawName.replace(/\.[^.]+$/, '') + '.' + defaultExt;
    const path = await saveFileDialog(defaultName, defaultExt);
    if (!path) return; // User cancelled

    try {
      const result = await saveFile(path);
      if (result && !result.error) {
        const oldId = projectIdRef.current;
        if (oldId) {
          const res = await rekeyProject(oldId, path);
          if (res && !res.error) {
            projectIdRef.current = path;
            onRekey(oldId, path);
          }
        }
        baselineSequenceRef.current = sequenceRef.current || sequence;
        setIsDirty(false);
      }
    } catch (e) {
      console.error('save as error:', e);
    }
  }, [onRekey, sequence, moleculeType]);

  // --- Save ---
  const handleSave = useCallback(async () => {
    if (!isTauri) {
      console.warn('Save is only available in desktop mode');
      return;
    }

    const filePath = projectIdRef.current;
    if (!filePath) {
      // No path known — fall back to Save As
      await handleSaveAs();
      return;
    }

    // Non-GenBank sources (.dna, .rna, .prot, .fasta, .ab1, ...) must not be
    // overwritten with text formats — force Save As with a .gbk/.gpt target.
    const ext = filePath.split('.').pop()?.toLowerCase();
    if (ext !== 'gbk' && ext !== 'gb' && !(moleculeType === 'protein' && ext === 'gpt')) {
      await handleSaveAs();
      return;
    }

    try {
      const result = await saveFile(filePath);
      if (result && !result.error) {
        baselineSequenceRef.current = sequenceRef.current || sequence;
        setIsDirty(false);
      } else {
        console.error('save error:', result?.error);
      }
    } catch (e) {
      console.error('save exception:', e);
    }
  }, [handleSaveAs, sequence, moleculeType]);

  // Refetch the project's data after a mutation whose command response does not
  // carry the updated sequence/features (e.g. apply_codon_optimization); record
  // the new state in undo history and mark the project dirty so Ctrl+Z can
  // revert the optimization.
  const refreshProject = useCallback(async () => {
    try {
      const data = await getProjectById(projectIdRef.current, 'all');
      if (data && !data.error && data.sequence) {
        editHistoryRef.current.push({
          sequence: data.sequence,
          features: data.features || EMPTY_ARRAY,
          primers: data.primers || EMPTY_ARRAY,
          cursorIndex: null,
          selStart: null,
          selEnd: null,
        });
        setSequence(data.sequence);
        setFeatures(data.features || EMPTY_ARRAY);
        setEnzymes(data.enzymes || EMPTY_ARRAY);
        setPrimers(data.primers || EMPTY_ARRAY);
        setAlignments(data.alignments || EMPTY_ARRAY);
        setIsDirty(true);
      }
    } catch (e) {
      console.error('project refresh error:', e);
    }
  }, []);

  // Expose imperative handle for App (sidebar buttons, close-with-save flow,
  // drag-drop alignment routing)
  useEffect(() => {
    registerHandle(projectId, {
      save: handleSave,
      saveAs: handleSaveAs,
      isDirty: () => isDirtyRef.current,
      openPluginDialog: (key) => {
        setPluginDialogs((prev) => ({ ...prev, [key]: true }));
      },
      openMapView: () => setMapViewOpen(true),
      openPrimerOverview: () => setPrimerOverviewOpen(true),
      moleculeType,
      alignmentEnabled,
      addAlignmentFiles,
    });
    return () => registerHandle(projectId, null);
  }, [
    projectId,
    registerHandle,
    handleSave,
    handleSaveAs,
    moleculeType,
    alignmentEnabled,
    addAlignmentFiles,
  ]);

  // --- Keyboard shortcuts for the visible workspace: Ctrl+Z/Y/S/Shift+S ---
  useEffect(() => {
    if (hidden) return undefined;
    const handleKeyDown = (e) => {
      const isCtrl = e.ctrlKey || e.metaKey;
      if (!isCtrl) return;

      // Ignore when focus is in input/textarea (e.g. edit dialog)
      const tag = e.target?.tagName?.toLowerCase();
      if (tag === 'input' || tag === 'textarea' || e.target?.isContentEditable) return;

      // Ctrl+Z: Undo (no shift)
      if (e.key === 'z' && !e.shiftKey) {
        e.preventDefault();
        handleUndo();
        return;
      }
      // Ctrl+Shift+Z or Ctrl+Y: Redo
      if ((e.key === 'z' && e.shiftKey) || e.key === 'y') {
        e.preventDefault();
        handleRedo();
        return;
      }
      // Ctrl+S: Save (no shift)
      if (e.key === 's' && !e.shiftKey) {
        e.preventDefault();
        handleSave();
        return;
      }
      // Ctrl+Shift+S: Save As
      if (e.key === 's' && e.shiftKey) {
        e.preventDefault();
        handleSaveAs();
        return;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [hidden, handleUndo, handleRedo, handleSave, handleSaveAs]);

  // Hidden workspaces stay mounted (CSS-only) so editor state/scroll/undo survive switching
  return (
    <div className={hidden ? 'hidden' : 'relative flex flex-1 min-h-0 min-w-0'}>
      {hasProject ? (
        <>
          <main
            ref={mainScrollRef}
            className="w-full flex-1 overflow-y-auto overscroll-contain hide-scrollbar transition-[padding] duration-300 ease-out"
          >
            <SequenceEditor
              sequence={sequence}
              features={editorFeatures}
              enzymes={displayEnzymes}
              allEnzymes={enzymes}
              primers={editorPrimers}
              charsPerLine={60}
              layoutKey={hidden ? undefined : 'visible'}
              hidden={hidden}
              layoutParams={editorLayoutParams}
              onEditRequest={handleEditRequest}
              restoreState={restoreState}
              onSave={handleSave}
              onSaveAs={handleSaveAs}
              canDirectSave={canDirectSave}
              onFeatureFtypeChange={handleFeatureFtypeChange}
              onFeatureColorChange={handleFeatureColorChange}
              onFeatureLocationChange={handleFeatureLocationChange}
              onFeatureNameChange={handleFeatureNameChange}
              onFeatureStrandChange={handleFeatureStrandChange}
              onFeatureAdd={handleFeatureAdd}
              onFeatureDelete={handleDeleteFeature}
              onPrimerChange={handlePrimerChange}
              onPrimerDelete={handleDeletePrimer}
              primerSeedLength={primerSeedLength}
              tmParams={tmParams}
              onSelectionChange={handleSelectionChange}
              scrollContainerRef={mainScrollRef}
              onUndo={handleUndo}
              onRedo={handleRedo}
              canUndo={canUndo}
              canRedo={canRedo}
              showFeatures={showFeatures}
              onToggleFeatures={onToggleFeatures}
              alwaysExpandFeatures={alwaysExpandFeatures}
              onToggleAlwaysExpandFeatures={onToggleAlwaysExpandFeatures}
              showOrfs={isDna && !disabledPlugins.includes('orf') ? showOrfs : undefined}
              onToggleOrfs={
                isDna && !disabledPlugins.includes('orf') ? () => setShowOrfs((v) => !v) : undefined
              }
              showPrimers={showPrimers}
              onTogglePrimers={onTogglePrimers}
              showEnzymes={showEnzymes}
              onToggleEnzymes={onToggleEnzymes}
              enzymeFilter={enzymeFilter}
              onEnzymeFilterChange={onEnzymeFilterChange}
              openPrimerEditorRef={openPrimerEditorRef}
              openFeatureEditorRef={openFeatureEditorRef}
              alignmentCacheRef={alignmentCacheRef}
              alignmentTracks={visibleAlignments}
              alignments={alignments}
              alignmentEnabled={alignmentEnabled}
              primerDesignEnabled={primerDesignEnabled}
              mapWatermark={mapWatermark}
              mapName={mapName}
              showAlignments={showAlignments}
              onToggleAlignments={() => setShowAlignments((v) => !v)}
              hiddenAlignIds={hiddenAlignIds}
              onToggleAlignmentVisible={handleToggleAlignmentVisible}
              onAddAlignmentFile={handleAddAlignment}
              onAddAlignmentText={() => setAlignTextOpen(true)}
              onManageAlignments={() => setPluginDialogs((prev) => ({ ...prev, alignment: true }))}
              onOpenRnaFold={
                disabledPlugins.includes('rnaFold')
                  ? undefined
                  : () => setPluginDialogs((prev) => ({ ...prev, rnaFold: true }))
              }
              onEnzymeHoverChange={setEnzymeHoverCuts}
              onOpenMyPrimers={() => setMyPrimersOpen(true)}
              onOpenPrimerOverview={() => setPrimerOverviewOpen(true)}
              onOpenDetectFeatures={
                isTauri && (isDna || isProtein) ? () => setDetectFeaturesOpen(true) : undefined
              }
              onOpenMyEnzymes={() => setMyEnzymesOpen(true)}
              onOpenEnzymeDatabase={() => setEnzymeDbOpen(true)}
              onAddPrimerToMyPrimers={handleAddPrimerToMyPrimers}
              onAddAllPrimersToMyPrimers={handleAddAllPrimersToMyPrimers}
              autoAddPrimers={autoAddPrimers}
              onToggleAutoAddPrimers={onToggleAutoAddPrimers}
              myEnzymes={myEnzymes}
              topology={topology}
              moleculeType={moleculeType}
              agentLocked={agentLocked}
              onUnlockAgent={handleUnlockAgent}
            />
          </main>
          {!hidden && (
            <FeatureScrollbar
              scrollContainerRef={mainScrollRef}
              features={displayFeatures}
              sequenceLength={sequence.length}
              highlightPositions={enzymeHoverCuts}
            />
          )}

          <MapView
            open={mapViewOpen}
            onOpenChange={setMapViewOpen}
            sequenceLength={sequence.length}
            features={displayFeatures}
            topology={topology}
            name={mapName}
            selection={liveSelection}
            onSelect={handleMapSelect}
            onClear={handleMapClear}
            onFeatureOpen={(f) => openFeatureEditorRef.current?.(f)}
            moleculeType={moleculeType}
            watermark={mapWatermark}
            onToggleWatermark={toggleMapWatermark}
          />
        </>
      ) : null}

      {isDna && (
        <PrimerOverviewDialog
          open={primerOverviewOpen}
          onOpenChange={setPrimerOverviewOpen}
          primers={primers}
          alignmentCacheRef={alignmentCacheRef}
          onEditPrimer={(p) => {
            openPrimerEditorRef.current?.(p);
          }}
        />
      )}

      <DetectFeaturesDialog
        open={detectFeaturesOpen}
        onOpenChange={setDetectFeaturesOpen}
        features={features}
        onAddFeature={handleFeatureAdd}
      />

      {isDna && (
        <MyPrimersDialog
          open={myPrimersOpen}
          onOpenChange={setMyPrimersOpen}
          myPrimers={myPrimers}
          currentPrimers={primers}
          binding={myPrimerBinding}
          onAddPrimer={handleAddMyPrimerToFile}
          onAddAllBinding={handleAddAllBindingPrimers}
          onDelete={handleDeleteMyPrimer}
        />
      )}

      {isDna && (
        <MyEnzymesDialog
          open={myEnzymesOpen}
          onOpenChange={setMyEnzymesOpen}
          enzymes={myEnzymes}
          onChange={handleMyEnzymesChange}
        />
      )}

      {isDna && <EnzymeDatabaseDialog open={enzymeDbOpen} onOpenChange={setEnzymeDbOpen} />}

      {plugins
        .filter(
          (plugin) =>
            !disabledPlugins.includes(plugin.id) &&
            (isDna || !plugin.dnaOnly) &&
            (moleculeType === 'rna' || !plugin.rnaOnly),
        )
        .map((plugin) => {
          const DialogComp = plugin.dialog;
          if (!DialogComp) return null;
          return (
            <DialogComp
              key={plugin.id}
              open={!!pluginDialogs[plugin.dialogKey]}
              onOpenChange={(open) =>
                setPluginDialogs((prev) => ({
                  ...prev,
                  [plugin.dialogKey]: open,
                }))
              }
              sequence={sequence}
              alignments={alignments}
              onAddAlignment={handleAddAlignment}
              onRemoveAlignment={handleRemoveAlignment}
              features={features}
              onProjectChanged={refreshProject}
            />
          );
        })}

      {isDna && (
        <AddAlignmentTextDialog
          open={alignTextOpen}
          onOpenChange={setAlignTextOpen}
          onSubmit={handleAddAlignmentText}
        />
      )}

      {/* --- Sequence Edit Dialog --- */}
      <SequenceEditDialog
        open={editDialog.open}
        mode={editDialog.mode}
        cursorIndex={editDialog.cursorIndex}
        selStart={editDialog.selStart}
        selEnd={editDialog.selEnd}
        selectedText={editDialog.selectedText}
        initialText={editDialog.initialText}
        onConfirm={handleEditConfirm}
        onCancel={handleEditCancel}
        moleculeType={moleculeType}
        clipboardMeta={editDialog.clipboardMeta}
      />
    </div>
  );
}
