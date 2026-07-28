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
  addFeature,
  deleteFeature,
  deletePrimer,
  rekeyProject,
  addAlignment,
  addAlignmentSeq,
  removeAlignment,
  openAlignmentFileDialog,
  listenProjectUpdates,
  isTauri,
} from './tauriApi';
import { plugins } from './plugins';
import AddAlignmentTextDialog from './plugins/alignment/AddAlignmentTextDialog';
import { createEditHistory } from './editHistory';
import SequenceEditDialog from './SequenceEditDialog';
import FeatureScrollbar from './FeatureScrollbar';
import MapView from './MapView';
import PrimerOverviewDialog from './components/PrimerOverviewDialog';
import { findOrfs } from './plugins/orf';

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
}) {
  const [sequence, setSequence] = useState(initialData?.sequence ?? null);
  const [features, setFeatures] = useState(initialData?.features || EMPTY_ARRAY);
  const [enzymes, setEnzymes] = useState(initialData?.enzymes || EMPTY_ARRAY);
  const [primers, setPrimers] = useState(initialData?.primers || EMPTY_ARRAY);
  const [alignments, setAlignments] = useState(initialData?.alignments || EMPTY_ARRAY);
  const [showAlignments, setShowAlignments] = useState(true);
  const [hiddenAlignIds, setHiddenAlignIds] = useState(EMPTY_ARRAY);
  const [alignTextOpen, setAlignTextOpen] = useState(false);
  const alignmentEnabled = !disabledPlugins.includes('alignment');

  const [primerOverviewOpen, setPrimerOverviewOpen] = useState(false);
  const [pluginDialogs, setPluginDialogs] = useState({});
  const openPrimerEditorRef = useRef(null);
  const openFeatureEditorRef = useRef(null);
  const [mapViewOpen, setMapViewOpen] = useState(false);
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
          editHistoryRef.current.reset({
            sequence: data.sequence,
            features: data.features || EMPTY_ARRAY,
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
        editHistoryRef.current.reset({
          sequence: data.sequence,
          features: data.features || EMPTY_ARRAY,
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
    if (hidden) return;
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
  }, [methKey, hidden, backendStatus, sequence, methylationSystems, methylationOverlap]);

  // Report dirty state up to App (sidebar dots + title bar)
  useEffect(() => {
    onDirtyChange(projectId, isDirty);
  }, [projectId, isDirty, onDirtyChange]);

  const [showOrfs, setShowOrfs] = useState(true);
  const orfEnabled = !disabledPlugins.includes('orf') && showOrfs;
  const orfFeatures = useMemo(
    () => (orfEnabled && sequence ? findOrfs(sequence, topology) : EMPTY_ARRAY),
    [orfEnabled, sequence, topology],
  );
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
    () => (showPrimers ? primers : EMPTY_ARRAY),
    [showPrimers, primers],
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
    if (!showEnzymes) return EMPTY_ARRAY;
    const all = enzymes || [];
    if (enzymeFilter === 'all') return all;
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
  }, [enzymes, enzymeFilter, showEnzymes, totalNamePairCounts]);

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
    setEditDialog({
      open: true,
      mode: request.type,
      cursorIndex: request.cursorIndex ?? null,
      selStart: request.selStart ?? null,
      selEnd: request.selEnd ?? null,
      selectedText: request.selectedText ?? '',
      initialText: request.clipboardText ?? '',
    });
  }, []);

  const handleFeatureFtypeChange = useCallback(
    async (featureId, newFtype) => {
      const gen = operationGenRef.current;
      try {
        editHistoryRef.current.push({
          sequence,
          features: features || EMPTY_ARRAY,
          cursorIndex: null,
          selStart: null,
          selEnd: null,
        });
        const data = await updateFeatureFtype(featureId, newFtype);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          setFeatures(data.features);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('update feature ftype error:', e);
      }
    },
    [sequence, features, onProjectsSync],
  );

  const handleFeatureColorChange = useCallback(
    async (featureId, newColor) => {
      const gen = operationGenRef.current;
      try {
        editHistoryRef.current.push({
          sequence,
          features: features || EMPTY_ARRAY,
          cursorIndex: null,
          selStart: null,
          selEnd: null,
        });
        const data = await updateFeatureColor(featureId, newColor);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          setFeatures(data.features);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('update feature color error:', e);
      }
    },
    [sequence, features, onProjectsSync],
  );

  const handleFeatureNameChange = useCallback(
    async (featureId, newName) => {
      const gen = operationGenRef.current;
      try {
        editHistoryRef.current.push({
          sequence,
          features: features || EMPTY_ARRAY,
          cursorIndex: null,
          selStart: null,
          selEnd: null,
        });
        const data = await updateFeatureName(featureId, newName);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          setFeatures(data.features);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('update feature name error:', e);
      }
    },
    [sequence, features, onProjectsSync],
  );

  const handlePrimerChange = useCallback(
    async (primerData) => {
      const gen = operationGenRef.current;
      try {
        // Push current state to undo history before mutating
        editHistoryRef.current.push({
          sequence,
          features: features || EMPTY_ARRAY,
          primers: primers || EMPTY_ARRAY,
          cursorIndex: null,
          selStart: null,
          selEnd: null,
        });
        const data = await addPrimer(primerData);
        if (operationGenRef.current !== gen) return;
        if (data && data.primers) {
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
    [sequence, features, primers, onProjectsSync],
  );

  const handleFeatureAdd = useCallback(
    async (featureData) => {
      const gen = operationGenRef.current;
      const { locationStr, ...feature } = featureData;
      // errors propagate so the dialog can display them
      editHistoryRef.current.push({
        sequence,
        features: features || EMPTY_ARRAY,
        primers: primers || EMPTY_ARRAY,
        cursorIndex: null,
        selStart: null,
        selEnd: null,
      });
      const data = await addFeature(feature, locationStr);
      if (operationGenRef.current !== gen) return;
      if (data && data.features) {
        setFeatures(data.features);
        if (data.enzymes) setEnzymes(data.enzymes);
        setIsDirty(true);
        if (data.projects) onProjectsSync(data.projects);
      }
    },
    [sequence, features, primers, onProjectsSync],
  );

  const handleFeatureStrandChange = useCallback(
    async (featureId, strand) => {
      const gen = operationGenRef.current;
      try {
        editHistoryRef.current.push({
          sequence,
          features: features || EMPTY_ARRAY,
          cursorIndex: null,
          selStart: null,
          selEnd: null,
        });
        const data = await updateFeatureStrand(featureId, strand);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          setFeatures(data.features);
          if (data.enzymes) setEnzymes(data.enzymes);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('update feature strand error:', e);
      }
    },
    [sequence, features],
  );

  const handleFeatureLocationChange = useCallback(
    async (featureId, locationStr) => {
      const gen = operationGenRef.current;
      // errors propagate so the dialog can display them
      editHistoryRef.current.push({
        sequence,
        features: features || EMPTY_ARRAY,
        cursorIndex: null,
        selStart: null,
        selEnd: null,
      });
      const data = await updateFeatureLocation(featureId, locationStr);
      if (operationGenRef.current !== gen) return;
      if (data && data.features) {
        setFeatures(data.features);
        if (data.projects) onProjectsSync(data.projects);
        setIsDirty(true);
      }
    },
    [sequence, features, onProjectsSync],
  );

  const handleDeleteFeature = useCallback(
    async (featureId) => {
      const gen = operationGenRef.current;
      try {
        editHistoryRef.current.push({
          sequence,
          features: features || EMPTY_ARRAY,
          cursorIndex: null,
          selStart: null,
          selEnd: null,
        });
        const data = await deleteFeature(featureId);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          setFeatures(data.features);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('delete feature error:', e);
      }
    },
    [sequence, features, onProjectsSync],
  );

  const handleDeletePrimer = useCallback(
    async (primerId) => {
      const gen = operationGenRef.current;
      try {
        editHistoryRef.current.push({
          sequence,
          features: features || EMPTY_ARRAY,
          primers: primers || EMPTY_ARRAY,
          cursorIndex: null,
          selStart: null,
          selEnd: null,
        });
        const data = await deletePrimer(primerId);
        if (operationGenRef.current !== gen) return;
        if (data && data.primers) {
          setPrimers(data.primers);
          if (data.alignments) setAlignments(data.alignments);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('delete primer error:', e);
      }
    },
    [sequence, features, primers, onProjectsSync],
  );

  const handleAddAlignment = useCallback(async () => {
    const gen = operationGenRef.current;
    const path = await openAlignmentFileDialog();
    if (!path) return;
    const data = await addAlignment(path);
    if (operationGenRef.current !== gen) return;
    if (data && data.error) throw new Error(data.error);
    if (data) {
      if (data.alignments) setAlignments(data.alignments);
      if (data.projects) onProjectsSync(data.projects);
      setIsDirty(true);
    }
  }, [onProjectsSync]);

  const handleAddAlignmentText = useCallback(
    async (name, seq) => {
      const gen = operationGenRef.current;
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
      const gen = operationGenRef.current;
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
   */
  const adjustAnnotations = useCallback((anns, editStart, editEnd, oldLen, newLen) => {
    const delta = newLen - oldLen;
    if (delta === 0 && oldLen === 0) return anns; // no-op

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
      const gen = operationGenRef.current;
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
      const adjustedFeatures = adjustAnnotations(
        features || EMPTY_ARRAY,
        editStart,
        editEnd,
        oldLen,
        newLen,
      );

      // Push new state to undo history (includes adjusted features for correct undo)
      editHistoryRef.current.push({
        sequence: newSeq,
        features: adjustedFeatures,
        cursorIndex,
        selStart: mode === 'insert' ? null : selStart,
        selEnd: mode === 'insert' ? null : selEnd,
      });

      // Optimistic UI update
      setSequence(newSeq);
      setFeatures(adjustedFeatures);
      setIsDirty(true);

      // Send to backend for recomputation (enzymes, primer binding sites)
      try {
        const data = await updateSequence(newSeq);
        if (operationGenRef.current !== gen) return;
        if (data && !data.error) {
          setSequence(data.sequence);
          setFeatures(adjustedFeatures); // use our adjusted features (backend doesn't recalculate feature positions)
          setEnzymes(data.enzymes || EMPTY_ARRAY);
          setPrimers(data.primers || EMPTY_ARRAY);
          setAlignments(data.alignments || EMPTY_ARRAY);
          if (data.projects) onProjectsSync(data.projects);
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
    [sequence, editDialog, adjustAnnotations, features, onProjectsSync],
  );

  // --- Edit dialog cancelled ---
  const handleEditCancel = useCallback(() => {
    setEditDialog((prev) => ({ ...prev, open: false }));
  }, []);

  // --- Undo ---
  const handleUndo = useCallback(async () => {
    const gen = operationGenRef.current;
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
      const data = await updateSequence(snapshot.sequence);
      if (operationGenRef.current !== gen) return;
      if (data && !data.error) {
        setSequence(data.sequence);
        setFeatures(snapshot.features || EMPTY_ARRAY);
        setEnzymes(data.enzymes || EMPTY_ARRAY);
        setPrimers(snapshot.primers || data.primers || EMPTY_ARRAY);
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
    const gen = operationGenRef.current;
    const snapshot = editHistoryRef.current.redo();
    if (!snapshot) return;

    setRestoreState({
      version: ++undoVersionRef.current,
      cursorIndex: snapshot.cursorIndex,
      selStart: snapshot.selStart,
      selEnd: snapshot.selEnd,
    });

    try {
      const data = await updateSequence(snapshot.sequence);
      if (operationGenRef.current !== gen) return;
      if (data && !data.error) {
        setSequence(data.sequence);
        setFeatures(snapshot.features || EMPTY_ARRAY);
        setEnzymes(data.enzymes || EMPTY_ARRAY);
        setPrimers(snapshot.primers || data.primers || EMPTY_ARRAY);
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

    const rawName = projectIdRef.current ? projectIdRef.current.split('/').pop() : 'sequence.gbk';
    const defaultName = rawName.replace(/\.[^.]+$/, '') + '.gbk';
    const path = await saveFileDialog(defaultName);
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
  }, [onRekey, sequence]);

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

    // Non-GenBank sources (.dna, .fasta, .ab1, ...) must not be overwritten
    // with GenBank text — force Save As with a .gbk target.
    const ext = filePath.split('.').pop()?.toLowerCase();
    if (ext !== 'gbk' && ext !== 'gb') {
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
  }, [handleSaveAs, sequence]);

  // Expose imperative handle for App (sidebar buttons, close-with-save flow)
  useEffect(() => {
    registerHandle(projectId, {
      save: handleSave,
      saveAs: handleSaveAs,
      isDirty: () => isDirtyRef.current,
      openPluginDialog: (key) => {
        if (key === 'orf') {
          setShowOrfs((v) => !v);
          return;
        }
        setPluginDialogs((prev) => ({ ...prev, [key]: true }));
      },
      openMapView: () => setMapViewOpen(true),
      openPrimerOverview: () => setPrimerOverviewOpen(true),
    });
    return () => registerHandle(projectId, null);
  }, [projectId, registerHandle, handleSave, handleSaveAs]);

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
              layoutParams={editorLayoutParams}
              onEditRequest={handleEditRequest}
              restoreState={restoreState}
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
              showAlignments={showAlignments}
              onToggleAlignments={() => setShowAlignments((v) => !v)}
              hiddenAlignIds={hiddenAlignIds}
              onToggleAlignmentVisible={handleToggleAlignmentVisible}
              onAddAlignmentFile={handleAddAlignment}
              onAddAlignmentText={() => setAlignTextOpen(true)}
              onManageAlignments={() => setPluginDialogs((prev) => ({ ...prev, alignment: true }))}
              onEnzymeHoverChange={setEnzymeHoverCuts}
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
          />
        </>
      ) : null}

      <PrimerOverviewDialog
        open={primerOverviewOpen}
        onOpenChange={setPrimerOverviewOpen}
        primers={primers}
        alignmentCacheRef={alignmentCacheRef}
        onEditPrimer={(p) => {
          openPrimerEditorRef.current?.(p);
        }}
      />

      {plugins
        .filter((plugin) => !disabledPlugins.includes(plugin.id))
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
              alignments={alignments}
              onAddAlignment={handleAddAlignment}
              onRemoveAlignment={handleRemoveAlignment}
            />
          );
        })}

      <AddAlignmentTextDialog
        open={alignTextOpen}
        onOpenChange={setAlignTextOpen}
        onSubmit={handleAddAlignmentText}
      />

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
      />
    </div>
  );
}
