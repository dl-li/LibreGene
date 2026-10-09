import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import SequenceEditor from './SequenceEditor';
import {
  getProjectById,
  saveFile,
  saveFileDialog,
  checkPrimersBinding,
  rekeyProject,
  getChromatogram,
  listenProjectUpdates,
  isTauri,
} from './tauriApi';
import { orientChromatogram } from './chromatogram';
import FeatureScrollbar from './components/FeatureScrollbar';
import MapView from './MapView';
import {
  loadProviderData,
  buildProviderIndex,
  findProviderEntry,
  hasProvider,
} from './enzymeProviders';
import { addMyPrimers, libraryToPrimers } from './myPrimers';
import useProjectData from './workspace/useProjectData';
import useUndoHistory from './workspace/useUndoHistory';
import useEditDialog from './workspace/useEditDialog';
import useProjectMutations from './workspace/useProjectMutations';
import WorkspaceDialogs from './workspace/WorkspaceDialogs';
import { EMPTY_ARRAY } from './workspace/constants';
import { readHiddenAlnNames } from './workspace/hiddenAlignments';

export default function ProjectWorkspace({
  projectId,
  projectName,
  hidden,
  initialData,
  topology = 'circular',
  backendStatus,
  methylationSystems,
  methylationOverlap,
  primerSeedLength,
  alignmentAlgorithm,
  tmParams,
  layoutParams,
  showFeatures,
  onToggleFeatures,
  pluginToggles,
  pluginSettings,
  alwaysExpandFeatures,
  showPrimers,
  onTogglePrimers,
  featureLabelsBelow,
  showEnzymes,
  onToggleEnzymes,
  enzymeFilter,
  onEnzymeFilterChange,
  enzymeProvider = 'all',
  onEnzymeProviderChange,
  viewMode = 'wrap',
  onViewModeChange,
  // Width of the collapsed sidebar icon rail overlaying the content's left
  // edge (main window); forwarded to SequenceEditor for label clamping.
  leftViewportInset = 0,
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
  // Open a SnapGene history snapshot as a new project (App owns project
  // switching; returns {ok} or {ok: false, error}).
  onOpenSnapshot,
}) {
  const {
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
  } = useProjectData({
    projectId,
    initialData,
    topology,
    agentLocked,
    disabledPlugins,
    hidden,
    backendStatus,
    methylationSystems,
    methylationOverlap,
  });
  const [enzymeHoverCuts, setEnzymeHoverCuts] = useState(null);
  const [liveSelection, setLiveSelection] = useState(null);
  const alignmentCacheRef = useRef({});
  const sequenceRef = useRef(sequence);
  const operationGenRef = useRef(0);
  const mainScrollRef = useRef(null);
  // The editor's inner container div (the horizontal scroll host in
  // continuous mode); SequenceEditor merges it with its own containerRef.
  const editorScrollRef = useRef(null);
  const lastSelectionRef = useRef(null);

  const {
    editHistoryRef,
    canUndo,
    canRedo,
    restoreState,
    setRestoreState,
    undoVersionRef,
    isDirty,
    setIsDirty,
    isDirtyRef,
    baselineSequenceRef,
    pushHistory,
    handleUndo,
    handleRedo,
  } = useUndoHistory({
    initialData,
    agentLockedRef,
    operationGenRef,
    projectId,
    onProjectsSync,
    sequence,
    features,
    primers,
    setSequence,
    setFeatures,
    setEnzymes,
    setPrimers,
    setAlignments,
  });

  const { editDialog, setEditDialog, handleEditConfirm } = useEditDialog({
    agentLockedRef,
    operationGenRef,
    editHistoryRef,
    setRestoreState,
    undoVersionRef,
    sequence,
    features,
    primers,
    setSequence,
    setFeatures,
    setEnzymes,
    setPrimers,
    setAlignments,
    setIsDirty,
    onProjectsSync,
    isDna,
  });

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
      setIsDirty(initialData.dirty === true);
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
          const hiddenNames = readHiddenAlnNames(projectIdRef.current);
          setHiddenAlignIds(
            (data.alignments || EMPTY_ARRAY)
              .filter((a) => hiddenNames.includes(a.name))
              .map((a) => a.id),
          );
          setMoleculeType(data.moleculeType || 'dna');
          setTopologyLive(data.topology || 'circular');
          setTracePath(data.tracePath || null);
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
        setTopologyLive(data.topology || 'circular');
        setTracePath(data.tracePath || null);
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

  // Report dirty state up to App (sidebar dots + title bar)
  useEffect(() => {
    onDirtyChange(projectId, isDirty);
  }, [projectId, isDirty, onDirtyChange]);

  const editorFeatures = useMemo(
    () => [...(showFeatures ? features : EMPTY_ARRAY), ...orfFeatures],
    [showFeatures, features, orfFeatures],
  );
  const displayFeatures = useMemo(
    () => (showFeatures ? features : EMPTY_ARRAY),
    [showFeatures, features],
  );
  const mapName = useMemo(() => {
    // Unsaved in-memory projects carry the user-entered name in the project
    // list; file-backed projects derive the name from the file stem.
    if (projectName && projectName !== projectId) return projectName;
    return projectId
      .split('/')
      .pop()
      .split('\\')
      .pop()
      .replace(/\.[^.]+$/, '');
  }, [projectId, projectName]);
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

  // Chromatogram traces are loaded lazily per source .ab1 path and cached in
  // a ref (raw payloads are big; they must not re-render on every fetch).
  // chromVersion bumps when a new trace lands so the derived maps recompute.
  // Lazy-load scheme adapted from GenePad (https://github.com/GenePad),
  // provided by the GenePad team / https://github.com/Masterchiefm.
  const chromRawRef = useRef(new Map());
  const [chromVersion, setChromVersion] = useState(0);
  const neededTracePaths = useMemo(() => {
    const paths = new Set();
    if (tracePath) paths.add(tracePath);
    for (const al of alignments) {
      if (al.tracePath) paths.add(al.tracePath);
    }
    return [...paths];
  }, [tracePath, alignments]);
  useEffect(() => {
    // StrictMode double-runs effects in dev: the in-flight marker must not
    // gate the result away, and resolved data is written unconditionally
    // (the ref survives effect re-runs, and a state bump after unmount is a
    // harmless no-op), otherwise the trace would never land.
    for (const path of neededTracePaths) {
      if (chromRawRef.current.has(path)) continue;
      chromRawRef.current.set(path, null); // in-flight marker
      getChromatogram(path)
        .then((chrom) => {
          if (!chrom) return;
          chromRawRef.current.set(path, chrom);
          setChromVersion((v) => v + 1);
        })
        .catch((e) => {
          console.error('chromatogram load error:', e);
          chromRawRef.current.delete(path);
        });
    }
  }, [neededTracePaths]);
  const mainChromatogram = useMemo(() => {
    if (!tracePath) return null;
    return chromRawRef.current.get(tracePath) || null;
  }, [tracePath, chromVersion]);
  const alignmentChromatograms = useMemo(() => {
    // Alignment trace bands are opt-in (visual tidiness): only the track the
    // user expanded via its label carries a chromatogram, one at a time.
    const out = {};
    if (!expandedChromAlnId) return out;
    const al = alignments.find((a) => a.id === expandedChromAlnId);
    if (!al?.tracePath) return out;
    const raw = chromRawRef.current.get(al.tracePath);
    if (raw) out[al.id] = orientChromatogram(raw, al.strand);
    return out;
  }, [alignments, expandedChromAlnId, chromVersion]);
  const alignmentTraceAvailable = useMemo(() => {
    const s = new Set();
    for (const al of alignments) {
      if (al.tracePath && chromRawRef.current.get(al.tracePath)) s.add(al.id);
    }
    return s;
  }, [alignments, chromVersion]);
  const toggleAlignmentChrom = useCallback((id) => {
    setExpandedChromAlnId((cur) => (cur === id ? null : id));
  }, []);

  const [providerIndex, setProviderIndex] = useState(null);
  useEffect(() => {
    if (enzymeProvider === 'all' || providerIndex) return;
    let cancelled = false;
    loadProviderData().then((data) => {
      if (!cancelled) setProviderIndex(buildProviderIndex(data));
    });
    return () => {
      cancelled = true;
    };
  }, [enzymeProvider, providerIndex]);

  const setFilteredEnzymes = useMemo(() => {
    // Enzymes only exist for DNA; rna/protein render sequence + features only.
    if (!isDna) return EMPTY_ARRAY;
    if (!showEnzymes) return EMPTY_ARRAY;
    // Trace view (project opened from .ab1): cut-site markers and labels
    // clutter the chromatogram bands — hide them for the trace's project.
    if (mainChromatogram) return EMPTY_ARRAY;
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
  }, [enzymes, enzymeFilter, showEnzymes, totalNamePairCounts, myEnzymes, isDna, mainChromatogram]);

  const displayEnzymes = useMemo(() => {
    if (enzymeProvider === 'all' || !providerIndex) return setFilteredEnzymes;
    return setFilteredEnzymes.filter((e) =>
      hasProvider(findProviderEntry(providerIndex, e.name), enzymeProvider),
    );
  }, [setFilteredEnzymes, enzymeProvider, providerIndex]);

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

  const {
    handleFeatureFtypeChange,
    handleFeatureColorChange,
    handleFeatureNameChange,
    handlePrimerChange,
    handleToggleTopology,
    handleAddPrimerToMyPrimers,
    handleAddAllPrimersToMyPrimers,
    handleDeleteMyPrimer,
    handleAddMyPrimerToFile,
    handleAddAllBindingPrimers,
    handleMyEnzymesChange,
    handleFeatureAdd,
    handleFeatureStrandChange,
    handleFeatureLocationChange,
    handleDeleteFeature,
    handleDeletePrimer,
    addAlignmentFiles,
    handleAddAlignment,
    handleAddAlignmentText,
    handleToggleAlignmentVisible,
    visibleAlignments,
    handleRemoveAlignment,
  } = useProjectMutations({
    agentLockedRef,
    operationGenRef,
    projectIdRef,
    pushHistory,
    setIsDirty,
    setFeatures,
    setPrimers,
    setEnzymes,
    setAlignments,
    setTopologyLive,
    topologyLive,
    setExpandedChromAlnId,
    setHiddenAlignIds,
    alignments,
    primers,
    myPrimerBinding,
    myPrimers,
    onMyPrimersChange,
    onMyEnzymesChange,
    onProjectsSync,
    alignmentAlgorithm,
    alignmentEnabled,
    showAlignments,
    hiddenAlignIds,
  });

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

  // --- Edit dialog cancelled ---
  const handleEditCancel = useCallback(() => {
    setEditDialog((prev) => ({ ...prev, open: false }));
  }, []);

  // --- Save As (defined before Save because Save may reference it) ---
  const handleSaveAs = useCallback(async () => {
    if (!isTauri) return;

    // Protein projects export as protein GenBank (.gpt); DNA/RNA as .gbk.
    const defaultExt = moleculeType === 'protein' ? 'gpt' : 'gbk';
    const pid = projectIdRef.current || '';
    // Unsaved in-memory projects (ids `untitled-*` / `snapshot-*`) have no
    // path — prefill the save dialog with the project name (for snapshots,
    // the snapshot's original node name).
    const rawName =
      pid.startsWith('untitled-') || pid.startsWith('snapshot-') || !pid
        ? projectName || 'sequence'
        : pid.split('/').pop().split('\\').pop();
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
  }, [onRekey, sequence, moleculeType, projectName]);

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
              pluginToggles={pluginToggles}
              pluginSettings={pluginSettings}
              disabledPlugins={disabledPlugins}
              alwaysExpandFeatures={alwaysExpandFeatures}
              featureLabelsBelow={featureLabelsBelow}
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
              enzymeProvider={enzymeProvider}
              onEnzymeProviderChange={onEnzymeProviderChange}
              openPrimerEditorRef={openPrimerEditorRef}
              openFeatureEditorRef={openFeatureEditorRef}
              alignmentCacheRef={alignmentCacheRef}
              alignmentTracks={visibleAlignments}
              alignments={alignments}
              chromatogram={isDna ? mainChromatogram : null}
              alignmentChromatograms={alignmentChromatograms}
              alignmentTraceAvailable={alignmentTraceAvailable}
              expandedChromAlnId={expandedChromAlnId}
              onToggleAlignmentChrom={toggleAlignmentChrom}
              onHideAlignment={handleToggleAlignmentVisible}
              alignmentEnabled={alignmentEnabled}
              primerDesignEnabled={primerDesignEnabled}
              mapWatermark={mapEnabled && background === 'map'}
              background={background}
              backgroundOptions={backgroundOptions}
              onBackgroundChange={(v) => setEditorBackground(moleculeType, v)}
              foldWatermark={
                // Hidden workspaces stay mounted (display:none) — don't fold
                // or render a watermark for them; a forna layout created
                // while hidden gets a degenerate 0-size canvas.
                !hidden &&
                moleculeType === 'rna' &&
                !disabledPlugins.includes('rnaFold') &&
                background === 'folding'
              }
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
              onOpenCodonOptimization={
                isDna && !disabledPlugins.includes('codonOptimization')
                  ? () => setPluginDialogs((prev) => ({ ...prev, codonOptimization: true }))
                  : undefined
              }
              onOpenDotplot={
                !isProtein && !disabledPlugins.includes('dotplot')
                  ? () => setPluginDialogs((prev) => ({ ...prev, dotplot: true }))
                  : undefined
              }
              onOpenMapView={mapEnabled ? () => setMapViewOpen(true) : undefined}
              onOpenSnapshots={
                !disabledPlugins.includes('snapgeneHistory') &&
                isTauri &&
                isDna &&
                (/\.dna$/i.test(projectId || '') || /^snapshot-/.test(projectId || ''))
                  ? () => setPluginDialogs((prev) => ({ ...prev, snapshots: true }))
                  : undefined
              }
              blastEnabled={isTauri && (isDna || isProtein) && !disabledPlugins.includes('blast')}
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
              topology={topologyLive}
              onToggleTopology={handleToggleTopology}
              moleculeType={moleculeType}
              viewMode={viewMode}
              onViewModeChange={onViewModeChange}
              editorScrollRef={editorScrollRef}
              leftViewportInset={leftViewportInset}
              agentLocked={agentLocked}
              onUnlockAgent={handleUnlockAgent}
            />
          </main>
          {!hidden && viewMode !== 'continuous' && (
            <FeatureScrollbar
              scrollContainerRef={mainScrollRef}
              features={displayFeatures}
              sequenceLength={sequence.length}
              highlightPositions={enzymeHoverCuts}
            />
          )}
          {!hidden && viewMode === 'continuous' && (
            <div className="absolute top-0 left-0 right-0">
              <FeatureScrollbar
                orientation="horizontal"
                scrollContainerRef={editorScrollRef}
                features={displayFeatures}
                sequenceLength={sequence.length}
                highlightPositions={enzymeHoverCuts}
              />
            </div>
          )}

          <MapView
            open={mapEnabled && mapViewOpen}
            onOpenChange={setMapViewOpen}
            sequenceLength={sequence.length}
            features={displayFeatures}
            topology={topologyLive}
            name={mapName}
            selection={liveSelection}
            onSelect={handleMapSelect}
            onClear={handleMapClear}
            onFeatureOpen={(f) => openFeatureEditorRef.current?.(f)}
            moleculeType={moleculeType}
            watermark={background === 'map'}
            onToggleWatermark={() =>
              setEditorBackground(moleculeType, background === 'map' ? 'none' : 'map')
            }
          />
        </>
      ) : null}

      <WorkspaceDialogs
        projectId={projectId}
        moleculeType={moleculeType}
        isDna={isDna}
        sequence={sequence}
        features={features}
        enzymes={enzymes}
        alignments={alignments}
        primers={primers}
        myPrimers={myPrimers}
        myEnzymes={myEnzymes}
        enzymeProvider={enzymeProvider}
        onEnzymeProviderChange={onEnzymeProviderChange}
        disabledPlugins={disabledPlugins}
        onOpenSnapshot={onOpenSnapshot}
        mapName={mapName}
        background={background}
        setEditorBackground={setEditorBackground}
        alignmentCacheRef={alignmentCacheRef}
        openPrimerEditorRef={openPrimerEditorRef}
        primerOverviewOpen={primerOverviewOpen}
        setPrimerOverviewOpen={setPrimerOverviewOpen}
        detectFeaturesOpen={detectFeaturesOpen}
        setDetectFeaturesOpen={setDetectFeaturesOpen}
        myPrimersOpen={myPrimersOpen}
        setMyPrimersOpen={setMyPrimersOpen}
        myEnzymesOpen={myEnzymesOpen}
        setMyEnzymesOpen={setMyEnzymesOpen}
        enzymeDbOpen={enzymeDbOpen}
        setEnzymeDbOpen={setEnzymeDbOpen}
        myPrimerBinding={myPrimerBinding}
        pluginDialogs={pluginDialogs}
        setPluginDialogs={setPluginDialogs}
        alignTextOpen={alignTextOpen}
        setAlignTextOpen={setAlignTextOpen}
        editDialog={editDialog}
        handleEditConfirm={handleEditConfirm}
        handleEditCancel={handleEditCancel}
        handleFeatureAdd={handleFeatureAdd}
        handleAddMyPrimerToFile={handleAddMyPrimerToFile}
        handleAddAllBindingPrimers={handleAddAllBindingPrimers}
        handleDeleteMyPrimer={handleDeleteMyPrimer}
        handleMyEnzymesChange={handleMyEnzymesChange}
        handleAddAlignment={handleAddAlignment}
        handleAddAlignmentText={handleAddAlignmentText}
        handleRemoveAlignment={handleRemoveAlignment}
        refreshProject={refreshProject}
      />
    </div>
  );
}
