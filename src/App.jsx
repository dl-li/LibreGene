import React, { useState, useEffect, useRef, useCallback, useMemo, startTransition } from 'react';
import SequenceEditor from './SequenceEditor';
import { getProject, getProjectById, openFile, setMethylation, isTauri, openFileDialog, listenProjectUpdates, getProjects, activateProject, getWindowProjectId, openInNewWindow, updateSequence, saveFile, saveFileDialog, updateFeatureFtype, updateFeatureColor, updateFeatureName, updateFeatureLocation, updateFeatureStrand, addPrimer, addFeature, deleteFeature, deletePrimer, deleteProject, setWindowTitle } from './tauriApi';
import { createEditHistory } from './editHistory';
import SequenceEditDialog from './SequenceEditDialog';
import DebugPanel from './components/DebugPanel';
import { SidebarProvider, Sidebar, SidebarContent, SidebarGroup, SidebarGroupContent, SidebarGroupLabel, SidebarHeader, SidebarMenu, SidebarMenuButton, SidebarMenuItem } from '@/components/ui/sidebar';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Empty, EmptyContent, EmptyDescription, EmptyMedia, EmptyTitle } from '@/components/ui/empty';
import { Button } from '@/components/ui/button';
import { TooltipProvider } from '@/components/ui/tooltip';
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription,
  DialogFooter, DialogClose,
} from '@/components/ui/dialog';
import { Dna, FolderOpen, ChevronDown, ExternalLink, AlertTriangle, X } from 'lucide-react';
import { getFileIcon } from './fileIcons';

const EMPTY_ARRAY = [];

export default function App() {
  const [sequence, setSequence] = useState(null);
  const [features, setFeatures] = useState(EMPTY_ARRAY);
  const [enzymes, setEnzymes] = useState(EMPTY_ARRAY);
  const [primers, setPrimers] = useState(EMPTY_ARRAY);
  const [backendStatus, setBackendStatus] = useState(isTauri ? 'online' : 'offline');

  // Multi-project state
  const [projects, setProjects] = useState([]);
  const [activeId, setActiveId] = useState(null);

  // Multi-window: tracks whether this window is "main" or a "project" window
  const [windowInfo, setWindowInfo] = useState(null);
  // { type: 'main' } or { type: 'project', projectId: '...' }

  // Debug toggles
  const [debugOpen, setDebugOpen] = useState(false);
  const [showFeatures, setShowFeatures] = useState(true);
  const [showPrimers, setShowPrimers] = useState(true);
  const [enzymeFilter, setEnzymeFilter] = useState('unique');
  const [methylationSystems, setMethylationSystems] = useState(['dam', 'dcm', 'ecoki']);
  const [methylationOverlap, setMethylationOverlap] = useState(2);
  const [primerSeedLength, setPrimerSeedLength] = useState(10);
  const [openPath, setOpenPath] = useState('/Users/lidonglin/Documents/LibreGene/test/pUC-GW-Amp.gb');
  const [fileStatus, setFileStatus] = useState('');
  const sequenceRef = useRef(sequence);
  const projectCacheRef = useRef({}); // { [id]: { sequence, features, enzymes, primers, methKey } }
  const perProjectSelectionRef = useRef({}); // { [id]: { cursorIndex, selStart, selEnd, selectionMode, selectedPrimerIds, isEnzymeSelection, selectedEnzymeIds } }
  const switchGenRef = useRef(0);      // generation counter to cancel stale async responses
  const operationGenRef = useRef(0);   // generation counter for all mutations (prevents cross-contamination)

  // --- Sequence editing state ---
  const editHistoryRef = useRef(createEditHistory());
  const [editDialog, setEditDialog] = useState({ open: false, mode: 'insert', cursorIndex: null, selStart: null, selEnd: null, selectedText: '', initialText: '' });
  const [restoreState, setRestoreState] = useState({ version: 0, cursorIndex: null, selStart: null, selEnd: null });
  const undoVersionRef = useRef(0);
  const [isDirty, setIsDirty] = useState(false);
  const isDirtyRef = useRef(false);
  const activeIdRef = useRef(null);
  const dirtyStateRef = useRef({}); // per-project dirty state
  const [unsavedDialog, setUnsavedDialog] = useState({ open: false, pendingAction: null });
  const unsavedPendingRef = useRef(null); // ref mirror of pendingAction for stale-closure-safe access
  const lastSavePathRef = useRef(null); // 最后保存/打开的路径
  const baselineSequenceRef = useRef(''); // 文件打开/保存时的基线序列（用于 undo/redo 后准确判断 dirty）
  const baselinePerProjectRef = useRef({}); // { [projectId]: baselineSequence } — per-project baseline tracking

  // Cache methylation settings key — used to detect stale cache entries
  const methKey = useMemo(() => methylationSystems.join(',') + '|' + methylationOverlap, [methylationSystems, methylationOverlap]);
  const methKeyRef = useRef(methKey);
  useEffect(() => { methKeyRef.current = methKey; }, [methKey]);
  // Sync refs for stale-closure-safe access in event listeners
  useEffect(() => { isDirtyRef.current = isDirty; }, [isDirty]);
  useEffect(() => { activeIdRef.current = activeId; }, [activeId]);

  // Keep cache in sync with latest state (captures post-methylation enzymes)
  useEffect(() => {
    if (activeId && sequence) {
      projectCacheRef.current[activeId] = { sequence, features, enzymes, primers, methKey };
    }
  }, [activeId, sequence, features, enzymes, primers, methKey]);

  // Layout parameters (行距 / 特征 / 引物排版)
  const [layoutParams, setLayoutParams] = useState({
    // 引物排版
    fwdMatchY: 25, revMatchY: 15, misYDelta: 4,
    fwdBaseTextY: 8, revBaseTextY: 18,
    fwdLabelY: 8, revLabelY: 20,
    trackGap: 36,
    fwdAboveBase: 30, fwdAboveExtra: 23, fwdAboveNonTailExtra: 5,
    revBelowBase: 26, revBelowExtra: 25, revBelowNonTailExtra: 5,
    hoverExpand: 26, arrowHeadLen: 7, arrowHeadHeight: 5,
    // 行距
    minRowGap: 27,
    rowContentGap: 25,
    minAboveSpace: 36,
    minBelowSpace: 0,
    // 特征排版
    featTrackHeight: 18,
    featBaseOffset: 14,
    featLabelPad: 12,
    // 酶切位点排版
    enzTrackHeight: 18,
    enzLineGap: 18,
    enzLabelBase: 51,
    enzAbovePad: 10,
  });
  const setLP = (key, value) => setLayoutParams(prev => ({ ...prev, [key]: value }));

  const editorFeatures = useMemo(() => showFeatures ? features : EMPTY_ARRAY, [showFeatures, features]);
  const editorPrimers = useMemo(() => showPrimers ? primers : EMPTY_ARRAY, [showPrimers, primers]);
  const editorLayoutParams = useMemo(() => layoutParams, [layoutParams]);

  const hasProject = sequence !== null && sequence.length > 0;

  // Sidebar hover state
  const [sidebarHover, setSidebarHover] = useState(false);
  const sidebarLeaveRef = useRef(null);

  const handleSidebarEnter = useCallback(() => {
    clearTimeout(sidebarLeaveRef.current);
    setSidebarHover(true);
  }, []);

  const handleSidebarLeave = useCallback(() => {
    sidebarLeaveRef.current = setTimeout(() => setSidebarHover(false), 250);
  }, []);

  // Sync project list from backend
  const refreshProjects = useCallback(async () => {
    try {
      const data = await getProjects();
      if (data && !data.error) {
        setProjects(data.projects || []);
        setActiveId(data.activeId || null);
      }
    } catch {}
  }, []);

  // Detect window type on mount
  useEffect(() => {
    if (!isTauri) {
      setWindowInfo({ type: 'main' });
      return;
    }
    getWindowProjectId().then(pid => {
      setWindowInfo(pid ? { type: 'project', projectId: pid } : { type: 'main' });
    });
  }, []);

  // Main window: load active project + listen for project list updates
  // Project window: load its bound project by ID
  useEffect(() => {
    if (!windowInfo) return;
    let listener = null;
    let cancelled = false;

    async function loadData() {
      setBackendStatus(isTauri ? 'online' : 'connecting');
      try {
        let data;
        if (windowInfo.type === 'project') {
          data = await getProjectById(windowInfo.projectId, 'all');
        } else {
          data = await getProject('all');
        }
        if (cancelled) return;
        if (data && !data.error && data.sequence) {
          setSequence(data.sequence);
          setFeatures(data.features || []);
          setEnzymes(data.enzymes || []);
          setPrimers(data.primers || []);
          setBackendStatus('online');
          // Initialize undo history
          const pid = data.activeId || activeId;
          if (pid) {
            editHistoryRef.current.reset({ sequence: data.sequence, features: data.features || EMPTY_ARRAY, cursorIndex: null, selStart: null, selEnd: null });
            baselineSequenceRef.current = data.sequence;
            baselinePerProjectRef.current[pid] = data.sequence;
            lastSavePathRef.current = pid;
            dirtyStateRef.current[pid] = false;
            setIsDirty(false);
          }
        }
        if (data && data.projects) setProjects(data.projects);
        if (data && data.activeId !== undefined) {
          setActiveId(data.activeId);
          const pid = windowInfo.type === 'project' ? windowInfo.projectId : data.activeId;
          if (pid && pid !== 'all') {
            const fn = pid.split('/').pop().split('\\').pop();
            setWindowTitle(fn);
          }
        }
        await refreshProjects();
      } catch {
        if (!cancelled) setBackendStatus(isTauri ? 'online' : 'offline');
      }
    }

    loadData();

    // Event listener: main window syncs project list; project windows ignore
    if (windowInfo.type === 'main') {
      listener = listenProjectUpdates((msg) => {
        if (cancelled) return;
        // Update the project list only — dirtyStateRef is managed by mutation handlers
        if (msg.projects) {
          setProjects(msg.projects);
        }
        if (msg.activeId !== undefined) {
          setActiveId(msg.activeId);
        }
        // Only apply full data update if it matches the currently active project
        // to prevent cross-contamination from other windows' modifications
        if (msg.data && msg.data.sequence && msg.activeId !== undefined) {
          // Update cache for the affected project regardless
          projectCacheRef.current[msg.activeId] = {
            sequence: msg.data.sequence,
            features: msg.data.features || EMPTY_ARRAY,
            enzymes: msg.data.enzymes || EMPTY_ARRAY,
            primers: msg.data.primers || EMPTY_ARRAY,
            methKey: methKeyRef.current,
          };
          baselinePerProjectRef.current[msg.activeId] = msg.data.sequence;
          // Only update UI if this is the currently active project
          // (use ref to avoid stale closure on activeId)
          if (msg.activeId === activeIdRef.current) {
            setSequence(msg.data.sequence);
            setFeatures(msg.data.features || []);
            setEnzymes(msg.data.enzymes || []);
            setPrimers(msg.data.primers || []);
            editHistoryRef.current.reset({
              sequence: msg.data.sequence,
              features: msg.data.features || EMPTY_ARRAY,
              cursorIndex: null, selStart: null, selEnd: null,
            });
            baselineSequenceRef.current = msg.data.sequence;
            setIsDirty(msg.data.dirty === true);
          }
        }
      });
    }

    return () => { cancelled = true; if (listener) listener.close(); };
  }, [windowInfo]);

  // Sync methylation systems with backend, then fetch updated enzymes.
  // Only fires when methylation settings change (NOT on every project load).
  const syncMethylation = useCallback(async () => {
    if (backendStatus !== 'online') return;
    try {
      const data = await setMethylation(methylationSystems, methylationOverlap);
      if (data && !data.error && data.enzymes) {
        setEnzymes(data.enzymes);
      }
    } catch (e) {
      console.error('methylation sync error:', e);
    }
  }, [methylationSystems, methylationOverlap, backendStatus]);

  // Re-sync enzymes when methylation settings change
  useEffect(() => {
    if (backendStatus !== 'online' || !sequence) return;
    syncMethylation();
  }, [methylationSystems, methylationOverlap]);

  useEffect(() => { sequenceRef.current = sequence; }, [sequence]);

  // --- beforeunload: warn on close with unsaved changes ---
  useEffect(() => {
    const handler = (e) => {
      if (isDirty) {
        e.preventDefault();
        e.returnValue = '';
      }
    };
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  }, [isDirty]);

  const displayEnzymes = useMemo(() => {
    const all = enzymes || [];
    if (enzymeFilter === 'all') return all;
    if (enzymeFilter === 'unique') return all.filter(e => e.isUnique);
    if (enzymeFilter === 'unique6') return all.filter(e => e.isUnique && e.recSeq?.length === 6);
    const cutType = e => e.cutType || (e.botCutIndex - e.cutIndex === 0 ? 'blunt' : e.botCutIndex - e.cutIndex > 0 ? '5overhang' : '3overhang');
    if (enzymeFilter === 'blunt') return all.filter(e => cutType(e) === 'blunt');
    if (enzymeFilter === 'overhang5') return all.filter(e => cutType(e) === '5overhang');
    if (enzymeFilter === 'overhang3') return all.filter(e => cutType(e) === '3overhang');
    if (enzymeFilter === 'iis') return all.filter(e => {
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      return pairs.some(cp => {
        const d = [cp.topCutIndex - e.recEnd, e.recStart - cp.topCutIndex, cp.botCutIndex - e.recEnd, e.recStart - cp.botCutIndex];
        return d.some(v => v >= 2);
      });
    });
    if (enzymeFilter === 'rec4') return all.filter(e => e.recSeq?.length === 4);
    if (enzymeFilter === 'rec5') return all.filter(e => e.recSeq?.length === 5);
    if (enzymeFilter === 'rec6') return all.filter(e => e.recSeq?.length === 6);
    if (enzymeFilter === 'rec8p') return all.filter(e => (e.recSeq?.length || 0) >= 8);
    return all.filter(e => e.isUnique);
  }, [enzymes, enzymeFilter]);

  // Sync unsavedPendingRef alongside setUnsavedDialog for stale-closure-safe access
  const openUnsavedDialog = useCallback((pendingAction) => {
    setUnsavedDialog({ open: true, pendingAction });
    unsavedPendingRef.current = pendingAction;
  }, []);

  const handleOpenFile = useCallback(async () => {
    const gen = ++operationGenRef.current; // new files = new generation
    // Save current project's dirty state before opening new files
    if (activeId) {
      dirtyStateRef.current[activeId] = isDirty;
    }
    let paths;
    if (isTauri) {
      paths = await openFileDialog();
      if (!paths || !paths.length) return;
    } else {
      paths = [openPath];
    }
    setFileStatus('loading...');
    try {
      let lastData = null;
      for (const p of paths) {
        const data = await openFile(p);
        if (data && data.sequence) {
          // Pre-cache every opened file immediately
          projectCacheRef.current[p] = {
            sequence: data.sequence,
            features: data.features || EMPTY_ARRAY,
            enzymes: data.enzymes || EMPTY_ARRAY,
            primers: data.primers || EMPTY_ARRAY,
            methKey,
          };
          lastData = data;
        }
      }
      await refreshProjects();

      if (lastData && lastData.sequence) {
        setSequence(lastData.sequence);
        setFeatures(lastData.features || EMPTY_ARRAY);
        setEnzymes(lastData.enzymes || EMPTY_ARRAY);
        setPrimers(lastData.primers || EMPTY_ARRAY);
        // Apply methylation to newly opened file
        syncMethylation();
        // Initialize undo history and dirty state
        const pid = paths.length > 0 ? paths[paths.length - 1] : null;
        if (pid) {
          editHistoryRef.current.reset({ sequence: lastData.sequence, features: lastData.features || EMPTY_ARRAY, cursorIndex: null, selStart: null, selEnd: null });
          baselineSequenceRef.current = lastData.sequence;
          baselinePerProjectRef.current[pid] = lastData.sequence;
          lastSavePathRef.current = pid;
          dirtyStateRef.current[pid] = false;
          setIsDirty(false);
        }
      }
      const fn = paths[paths.length - 1].split('/').pop().split('\\').pop();
      setWindowTitle(fn);
      setFileStatus('ok');
    } catch (e) {
      setFileStatus('error: ' + e.message);
    }
  }, [isTauri, openPath, methKey, refreshProjects, syncMethylation, activeId, isDirty]);

  // Track selection state per project so switching files preserves it
  const handleSelectionChange = useCallback((sel) => {
    if (activeId) {
      perProjectSelectionRef.current[activeId] = sel;
    }
  }, [activeId]);

  const handleActivateProject = useCallback(async (id) => {
    // Save current project's dirty state, baseline, selection, and scroll position before switching away
    if (activeId && activeId !== id) {
      dirtyStateRef.current[activeId] = isDirty;
      baselinePerProjectRef.current[activeId] = baselineSequenceRef.current;
      // Save scroll position so we can restore it when switching back
      if (!perProjectSelectionRef.current[activeId]) {
        perProjectSelectionRef.current[activeId] = {};
      }
      perProjectSelectionRef.current[activeId].scrollY = window.scrollY;
    }
    // Retrieve the target project's saved selection (or empty defaults)
    const targetSel = id ? perProjectSelectionRef.current[id] : null;
    const gen = ++switchGenRef.current;
    ++operationGenRef.current; // invalidate in-flight mutations from previous project

    // Instant switch from cache for perceived speed
    const cached = projectCacheRef.current[id];
    if (cached) {
      const methFresh = cached.methKey === methKey;
      setActiveId(id);
      setRestoreState({
        version: ++undoVersionRef.current,
        cursorIndex: targetSel?.cursorIndex ?? null,
        selStart: targetSel?.selStart ?? null,
        selEnd: targetSel?.selEnd ?? null,
        selectionMode: targetSel?.selectionMode ?? 'text',
        selectedPrimerIds: targetSel?.selectedPrimerIds ?? [],
        isEnzymeSelection: targetSel?.isEnzymeSelection ?? false,
        selectedEnzymeIds: targetSel?.selectedEnzymeIds ?? [],
      });
      setSequence(cached.sequence);
      setFeatures(cached.features);
      setEnzymes(cached.enzymes);
      setPrimers(cached.primers);
      // Reset undo history for the switched-to project
      editHistoryRef.current.reset({ sequence: cached.sequence, features: cached.features, cursorIndex: null, selStart: null, selEnd: null });
      baselineSequenceRef.current = baselinePerProjectRef.current[id] ?? cached.sequence;
      lastSavePathRef.current = id;
      // Restore per-project dirty state and keep dirty indicator consistent
      setIsDirty(dirtyStateRef.current[id] === true);
      if (!methFresh) {
        syncMethylation();
      }
      const fn = id.split('/').pop().split('\\').pop();
      setWindowTitle(fn);

      // Restore scroll position for this project (or scroll to top for new projects)
      requestAnimationFrame(() => {
        window.scrollTo(0, targetSel?.scrollY ?? 0);
      });
    }

    try {
      const data = await activateProject(id);
      if (switchGenRef.current !== gen) return;

      if (data && !data.error && data.sequence) {
        if (!cached) {
          projectCacheRef.current[id] = {
            sequence: data.sequence,
            features: data.features || EMPTY_ARRAY,
            enzymes: data.enzymes || EMPTY_ARRAY,
            primers: data.primers || EMPTY_ARRAY,
            methKey,
          };
          setActiveId(id);
          setRestoreState({
            version: ++undoVersionRef.current,
            cursorIndex: targetSel?.cursorIndex ?? null,
            selStart: targetSel?.selStart ?? null,
            selEnd: targetSel?.selEnd ?? null,
            selectionMode: targetSel?.selectionMode ?? 'text',
            selectedPrimerIds: targetSel?.selectedPrimerIds ?? [],
            isEnzymeSelection: targetSel?.isEnzymeSelection ?? false,
            selectedEnzymeIds: targetSel?.selectedEnzymeIds ?? [],
          });
          setSequence(data.sequence);
          setFeatures(data.features || EMPTY_ARRAY);
          setEnzymes(data.enzymes || EMPTY_ARRAY);
          setPrimers(data.primers || EMPTY_ARRAY);
          syncMethylation();
          // Reset undo history
          editHistoryRef.current.reset({ sequence: data.sequence, features: data.features || EMPTY_ARRAY, cursorIndex: null, selStart: null, selEnd: null });
          baselineSequenceRef.current = data.sequence;
          baselinePerProjectRef.current[id] = data.sequence;
          lastSavePathRef.current = id;
          // Restore per-project dirty state instead of always setting clean
          setIsDirty(dirtyStateRef.current[id] === true);
          const fn = id.split('/').pop().split('\\').pop();
          setWindowTitle(fn);

          // Restore scroll position for this project
          requestAnimationFrame(() => {
            window.scrollTo(0, targetSel?.scrollY ?? 0);
          });
        }

        if (data.projects) setProjects(data.projects);
      }
    } catch (e) {
      console.error('activate project error:', e);
    }
  }, [methKey, syncMethylation, activeId, isDirty]);

  // Switch projects freely — dirty state is tracked per-project and shown in sidebar (*)
  const handleSwitchProject = useCallback((targetId) => {
    handleActivateProject(targetId);
  }, [handleActivateProject]);

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

  const handleFeatureFtypeChange = useCallback(async (featureId, newFtype) => {
    const gen = operationGenRef.current;
    try {
      editHistoryRef.current.push({
        sequence,
        features: features || EMPTY_ARRAY,
        cursorIndex: null, selStart: null, selEnd: null,
      });
      const data = await updateFeatureFtype(featureId, newFtype);
      if (operationGenRef.current !== gen) return;
      if (data && data.features) {
        setFeatures(data.features);
        if (data.projects) setProjects(data.projects);
        if (data.activeId !== undefined) setActiveId(data.activeId);
        setIsDirty(true);
        if (activeId) dirtyStateRef.current[activeId] = true;
      }
    } catch (e) {
      console.error('update feature ftype error:', e);
    }
  }, [activeId, sequence, features]);

  const handleFeatureColorChange = useCallback(async (featureId, newColor) => {
    const gen = operationGenRef.current;
    try {
      editHistoryRef.current.push({
        sequence,
        features: features || EMPTY_ARRAY,
        cursorIndex: null, selStart: null, selEnd: null,
      });
      const data = await updateFeatureColor(featureId, newColor);
      if (operationGenRef.current !== gen) return;
      if (data && data.features) {
        setFeatures(data.features);
        if (data.projects) setProjects(data.projects);
        if (data.activeId !== undefined) setActiveId(data.activeId);
        setIsDirty(true);
        if (activeId) dirtyStateRef.current[activeId] = true;
      }
    } catch (e) {
      console.error('update feature color error:', e);
    }
  }, [activeId, sequence, features]);

  const handleFeatureNameChange = useCallback(async (featureId, newName) => {
    const gen = operationGenRef.current;
    try {
      editHistoryRef.current.push({
        sequence,
        features: features || EMPTY_ARRAY,
        cursorIndex: null, selStart: null, selEnd: null,
      });
      const data = await updateFeatureName(featureId, newName);
      if (operationGenRef.current !== gen) return;
      if (data && data.features) {
        setFeatures(data.features);
        if (data.projects) setProjects(data.projects);
        if (data.activeId !== undefined) setActiveId(data.activeId);
        setIsDirty(true);
        if (activeId) dirtyStateRef.current[activeId] = true;
      }
    } catch (e) {
      console.error('update feature name error:', e);
    }
  }, [activeId, sequence, features]);

  const handlePrimerChange = useCallback(async (primerData) => {
    const gen = operationGenRef.current;
    try {
      // Push current state to undo history before mutating
      editHistoryRef.current.push({
        sequence,
        features: features || EMPTY_ARRAY,
        primers: primers || EMPTY_ARRAY,
        cursorIndex: null, selStart: null, selEnd: null,
      });
      const data = await addPrimer(primerData);
      if (operationGenRef.current !== gen) return;
      if (data && data.primers) {
        setPrimers(data.primers);
        if (data.enzymes) setEnzymes(data.enzymes);
        setIsDirty(true);
        if (activeId) dirtyStateRef.current[activeId] = true;
        if (data.projects) setProjects(data.projects);
        if (data.activeId !== undefined) setActiveId(data.activeId);
      }
    } catch (e) {
      console.error('add primer error:', e);
    }
  }, [activeId, sequence, features, primers]);

  const handleFeatureAdd = useCallback(async (featureData) => {
    const gen = operationGenRef.current;
    const { locationStr, ...feature } = featureData;
    try {
      editHistoryRef.current.push({
        sequence,
        features: features || EMPTY_ARRAY,
        primers: primers || EMPTY_ARRAY,
        cursorIndex: null, selStart: null, selEnd: null,
      });
      const data = await addFeature(feature, locationStr);
      if (operationGenRef.current !== gen) return;
      if (data && data.features) {
        setFeatures(data.features);
        if (data.enzymes) setEnzymes(data.enzymes);
        setIsDirty(true);
        if (activeId) dirtyStateRef.current[activeId] = true;
        if (data.projects) setProjects(data.projects);
        if (data.activeId !== undefined) setActiveId(data.activeId);
      }
    } catch (e) {
      throw e; // re-throw so dialog can display error
    }
  }, [activeId, sequence, features, primers]);

  const handleFeatureStrandChange = useCallback(async (featureId, strand) => {
    const gen = operationGenRef.current;
    try {
      editHistoryRef.current.push({
        sequence,
        features: features || EMPTY_ARRAY,
        cursorIndex: null, selStart: null, selEnd: null,
      });
      const data = await updateFeatureStrand(featureId, strand);
      if (operationGenRef.current !== gen) return;
      if (data && data.features) {
        setFeatures(data.features);
        if (data.enzymes) setEnzymes(data.enzymes);
        setIsDirty(true);
        if (activeId) dirtyStateRef.current[activeId] = true;
      }
    } catch (e) {
      console.error('update feature strand error:', e);
    }
  }, [activeId, sequence, features]);

  const handleFeatureLocationChange = useCallback(async (featureId, locationStr) => {
    const gen = operationGenRef.current;
    try {
      editHistoryRef.current.push({
        sequence,
        features: features || EMPTY_ARRAY,
        cursorIndex: null, selStart: null, selEnd: null,
      });
      const data = await updateFeatureLocation(featureId, locationStr);
      if (operationGenRef.current !== gen) return;
      if (data && data.features) {
        setFeatures(data.features);
        if (data.projects) setProjects(data.projects);
        if (data.activeId !== undefined) setActiveId(data.activeId);
        setIsDirty(true);
        if (activeId) dirtyStateRef.current[activeId] = true;
      }
    } catch (e) {
      throw e;
    }
  }, [activeId, sequence, features]);

  const handleDeleteFeature = useCallback(async (featureId) => {
    const gen = operationGenRef.current;
    try {
      editHistoryRef.current.push({
        sequence,
        features: features || EMPTY_ARRAY,
        cursorIndex: null, selStart: null, selEnd: null,
      });
      const data = await deleteFeature(featureId);
      if (operationGenRef.current !== gen) return;
      if (data && data.features) {
        setFeatures(data.features);
        if (data.projects) setProjects(data.projects);
        if (data.activeId !== undefined) setActiveId(data.activeId);
        setIsDirty(true);
        if (activeId) dirtyStateRef.current[activeId] = true;
      }
    } catch (e) {
      console.error('delete feature error:', e);
    }
  }, [activeId, sequence, features]);

  const handleDeletePrimer = useCallback(async (primerId) => {
    const gen = operationGenRef.current;
    try {
      editHistoryRef.current.push({
        sequence,
        features: features || EMPTY_ARRAY,
        primers: primers || EMPTY_ARRAY,
        cursorIndex: null, selStart: null, selEnd: null,
      });
      const data = await deletePrimer(primerId);
      if (operationGenRef.current !== gen) return;
      if (data && data.primers) {
        setPrimers(data.primers);
        if (data.projects) setProjects(data.projects);
        if (data.activeId !== undefined) setActiveId(data.activeId);
        setIsDirty(true);
        if (activeId) dirtyStateRef.current[activeId] = true;
      }
    } catch (e) {
      console.error('delete primer error:', e);
    }
  }, [activeId, sequence, features, primers]);

  /**
   * 调整特征/注释放置位置以适配编辑后的序列。
   * 编辑会删除 [editStart, editEnd] 区间（oldLen 个碱基），
   * 然后插入 newLen 个碱基。
   * 编辑区之外的特征位置保持与原序列的相对偏移不变。
   */
  const adjustAnnotations = useCallback((anns, editStart, editEnd, oldLen, newLen) => {
    const delta = newLen - oldLen;
    if (delta === 0 && oldLen === 0) return anns; // no-op

    return anns.map(ann => {
      const adjustSegments = (segments) => {
        if (!segments || !segments.length) return segments;
        return segments.map(seg => {
          let { start: s, end: e } = seg;
          if (e < editStart) return seg;
          if (s > editEnd) return { ...seg, start: s + delta, end: e + delta };
          // spans
          const ns = s < editStart ? s : editStart + newLen;
          const ne = e > editEnd ? e + delta : editStart + newLen - 1;
          if (ns > ne) return null;
          return { ...seg, start: ns, end: ne };
        }).filter(Boolean);
      };

      let newStart = ann.start, newEnd = ann.end;
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
    }).filter(Boolean);
  }, []);

  // --- Edit dialog confirmed (insert/delete/replace) ---
  const handleEditConfirm = useCallback(async (result) => {
    const gen = operationGenRef.current;
    const { mode, cursorIndex, selStart, selEnd } = editDialog;
    let newSeq;
    const currentSeq = sequence || '';
    let editStart, editEnd, oldLen, newLen;

    // Compute the new sequence and save edit parameters for feature adjustment
    if (mode === 'insert') {
      const cleaned = (result.sequence || '').replace(/\s/g, '');
      if (!cleaned) { setEditDialog(prev => ({ ...prev, open: false })); return; }
      editStart = cursorIndex;
      editEnd = cursorIndex - 1; // no deletion range
      oldLen = 0;
      newLen = cleaned.length;
      newSeq = currentSeq.slice(0, cursorIndex) + cleaned + currentSeq.slice(cursorIndex);
    } else if (mode === 'delete') {
      if (selStart === null || selEnd === null) { setEditDialog(prev => ({ ...prev, open: false })); return; }
      editStart = selStart;
      editEnd = selEnd;
      oldLen = selEnd - selStart + 1;
      newLen = 0;
      newSeq = currentSeq.slice(0, selStart) + currentSeq.slice(selEnd + 1);
    } else if (mode === 'replace') {
      const cleaned = (result.sequence || '').replace(/\s/g, '');
      if (selStart === null || selEnd === null) { setEditDialog(prev => ({ ...prev, open: false })); return; }
      editStart = selStart;
      editEnd = selEnd;
      oldLen = selEnd - selStart + 1;
      newLen = cleaned.length;
      newSeq = currentSeq.slice(0, selStart) + cleaned + currentSeq.slice(selEnd + 1);
    } else {
      return;
    }

    // Close dialog
    setEditDialog(prev => ({ ...prev, open: false }));

    // Optimistic UI update
    setSequence(newSeq);
    setIsDirty(true);
    if (activeId) dirtyStateRef.current[activeId] = true;

    // Compute adjusted features BEFORE backend call (for history and optimistic update)
    const adjustedFeatures = adjustAnnotations(features || EMPTY_ARRAY, editStart, editEnd, oldLen, newLen);

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
    if (activeId) dirtyStateRef.current[activeId] = true;

    // Send to backend for recomputation (enzymes, primer binding sites)
    try {
      const data = await updateSequence(newSeq);
      if (operationGenRef.current !== gen) return;
      if (data && !data.error) {
        setSequence(data.sequence);
        setFeatures(adjustedFeatures); // use our adjusted features (backend doesn't recalculate feature positions)
        setEnzymes(data.enzymes || EMPTY_ARRAY);
        setPrimers(data.primers || EMPTY_ARRAY);
        if (activeId) {
          projectCacheRef.current[activeId] = {
            sequence: data.sequence,
            features: data.features || EMPTY_ARRAY,
            enzymes: data.enzymes || EMPTY_ARRAY,
            primers: data.primers || EMPTY_ARRAY,
            methKey,
          };
        }
        if (data.projects) setProjects(data.projects);
        if (data.activeId !== undefined) setActiveId(data.activeId);
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
          }
        } catch {}
      }
    } catch (e) {
      console.error('update_sequence exception:', e);
    }
  }, [sequence, editDialog, activeId, methKey]);

  // --- Edit dialog cancelled ---
  const handleEditCancel = useCallback(() => {
    setEditDialog(prev => ({ ...prev, open: false }));
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
        setPrimers(data.primers || EMPTY_ARRAY);
        if (activeId) {
          projectCacheRef.current[activeId] = {
            sequence: data.sequence,
            features: snapshot.features || EMPTY_ARRAY,
            enzymes: data.enzymes || EMPTY_ARRAY,
            primers: data.primers || EMPTY_ARRAY,
            methKey,
          };
        }
        if (data.projects) setProjects(data.projects);
        if (data.activeId !== undefined) setActiveId(data.activeId);
        // Accurate dirty check: undo to saved state = not dirty
        if (snapshot.sequence === baselineSequenceRef.current) {
          setIsDirty(false);
          if (activeId) dirtyStateRef.current[activeId] = false;
        } else {
          setIsDirty(true);
          if (activeId) dirtyStateRef.current[activeId] = true;
        }
      } else {
        // Re-fetch to recover
        try {
          const refresh = await getProject('all');
          if (refresh && !refresh.error && refresh.sequence) {
            setSequence(refresh.sequence);
            setFeatures(refresh.features || EMPTY_ARRAY);
            setEnzymes(refresh.enzymes || EMPTY_ARRAY);
            setPrimers(refresh.primers || EMPTY_ARRAY);
          }
        } catch {}
      }
    } catch (e) {
      console.error('undo error:', e);
    }
  }, [activeId, methKey]);

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
        setPrimers(data.primers || EMPTY_ARRAY);
        if (activeId) {
          projectCacheRef.current[activeId] = {
            sequence: data.sequence,
            features: snapshot.features || EMPTY_ARRAY,
            enzymes: data.enzymes || EMPTY_ARRAY,
            primers: data.primers || EMPTY_ARRAY,
            methKey,
          };
        }
        if (data.projects) setProjects(data.projects);
        if (data.activeId !== undefined) setActiveId(data.activeId);
        // Accurate dirty check: redo back to saved state = not dirty
        if (snapshot.sequence === baselineSequenceRef.current) {
          setIsDirty(false);
          if (activeId) dirtyStateRef.current[activeId] = false;
        } else {
          setIsDirty(true);
          if (activeId) dirtyStateRef.current[activeId] = true;
        }
      } else {
        try {
          const refresh = await getProject('all');
          if (refresh && !refresh.error && refresh.sequence) {
            setSequence(refresh.sequence);
            setFeatures(refresh.features || EMPTY_ARRAY);
            setEnzymes(refresh.enzymes || EMPTY_ARRAY);
            setPrimers(refresh.primers || EMPTY_ARRAY);
          }
        } catch {}
      }
    } catch (e) {
      console.error('redo error:', e);
    }
  }, [activeId, methKey]);

  // --- Save As (defined before Save because Save may reference it) ---
  const handleSaveAs = useCallback(async () => {
    if (!isTauri) return;

    const defaultName = activeId ? activeId.split('/').pop() : 'sequence.gbk';
    const path = await saveFileDialog(defaultName);
    if (!path) return; // User cancelled

    try {
      const result = await saveFile({ path });
      if (result && !result.error) {
        lastSavePathRef.current = path;
        baselineSequenceRef.current = sequenceRef.current || sequence;
        baselinePerProjectRef.current[path] = baselineSequenceRef.current;
        dirtyStateRef.current[path] = false;
        setIsDirty(false);
      }
    } catch (e) {
      console.error('save as error:', e);
    }
  }, [activeId]);

  // --- Save ---
  const handleSave = useCallback(async () => {
    if (!isTauri) {
      console.warn('Save is only available in desktop mode');
      return;
    }

    const filePath = lastSavePathRef.current || activeId;
    if (!filePath) {
      // No path known — fall back to Save As
      await handleSaveAs();
      return;
    }

    try {
      const result = await saveFile({ path: filePath });
      if (result && !result.error) {
        baselineSequenceRef.current = sequenceRef.current || sequence;
        baselinePerProjectRef.current[filePath] = baselineSequenceRef.current;
        setIsDirty(false);
      } else {
        console.error('save error:', result?.error);
      }
    } catch (e) {
      console.error('save exception:', e);
    }
  }, [activeId]);

  // Internal: actually perform the close (no dirty check)
  const doCloseProject = useCallback(async (id) => {
    try {
      const data = await deleteProject(id);
      if (data && data.error) return;

      // Clean up per-project state
      delete dirtyStateRef.current[id];
      delete baselinePerProjectRef.current[id];
      delete projectCacheRef.current[id];

      // If the closed project was active, load the new active project's data
      if (id === activeId) {
        const newData = await getProject('all');
        if (newData && !newData.error && newData.sequence) {
          setSequence(newData.sequence);
          setFeatures(newData.features || EMPTY_ARRAY);
          setEnzymes(newData.enzymes || EMPTY_ARRAY);
          setPrimers(newData.primers || EMPTY_ARRAY);
          editHistoryRef.current.reset({ sequence: newData.sequence, features: newData.features || EMPTY_ARRAY, cursorIndex: null, selStart: null, selEnd: null });
          baselineSequenceRef.current = newData.sequence;
          setIsDirty(false);
        }
        if (newData && newData.projects) setProjects(newData.projects);
        if (newData && newData.activeId !== undefined) {
          setActiveId(newData.activeId);
          if (newData.activeId && newData.activeId !== 'all') {
            const fn = newData.activeId.split('/').pop().split('\\').pop();
            setWindowTitle(fn);
          } else {
            setWindowTitle('LibreGene');
          }
        }
      }
    } catch (e) {
      console.error('close project error:', e);
    }
  }, [activeId]);

  // Public close handler with dirty check
  const handleCloseProject = useCallback(async (id) => {
    const isProjectDirty = id === activeId ? isDirty : dirtyStateRef.current[id] === true;
    if (isProjectDirty) {
      openUnsavedDialog({ type: 'close', targetId: id });
      return;
    }
    doCloseProject(id);
  }, [activeId, isDirty, openUnsavedDialog, doCloseProject]);

  // --- Unsaved changes dialog handlers ---
  const handleUnsavedSave = useCallback(async () => {
    const pending = unsavedPendingRef.current;
    setUnsavedDialog({ open: false, pendingAction: null });
    unsavedPendingRef.current = null;
    await handleSave();
    if (pending?.type === 'open') {
      handleOpenFile();
    } else if (pending?.type === 'close' && pending.targetId) {
      doCloseProject(pending.targetId);
    }
  }, [handleSave, handleActivateProject, handleOpenFile, doCloseProject]);

  const handleUnsavedDiscard = useCallback(() => {
    const pending = unsavedPendingRef.current;
    setUnsavedDialog({ open: false, pendingAction: null });
    unsavedPendingRef.current = null;
    if (pending?.type === 'open') {
      handleOpenFile();
    } else if (pending?.type === 'close' && pending.targetId) {
      doCloseProject(pending.targetId);
    }
  }, [handleActivateProject, handleOpenFile, doCloseProject]);

  const handleUnsavedCancel = useCallback(() => {
    setUnsavedDialog({ open: false, pendingAction: null });
    unsavedPendingRef.current = null;
  }, []);

  const handleOpenInNewWindow = useCallback(async (id) => {
    try {
      await openInNewWindow(id);
    } catch (e) {
      console.error('open in new window error:', e);
    }
  }, []);

  // --- Global keyboard shortcuts: Ctrl+Z, Ctrl+Y, Ctrl+S, Ctrl+Shift+S ---
  // (must be placed AFTER all handler definitions to avoid TDZ)
  useEffect(() => {
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
  }, [handleUndo, handleRedo, handleSave, handleSaveAs]);

  // Extract filename from path
  const fileName = (p) => {
    const s = (p.name || p.id || '').replace(/\\/g, '/');
    return s.split('/').pop() || s;
  };

  // Collapsible "Open Files" group
  const [filesOpen, setFilesOpen] = useState(true);

  const sidebarContent = (
    <Sidebar collapsible="icon" variant="sidebar" className="transition-[width] duration-300 ease-out">
      <SidebarHeader className="flex flex-row items-center gap-2 px-3 py-2">
        <Dna className="size-5 shrink-0 text-primary" />
        <span className="font-semibold text-sm group-data-[state=collapsed]:hidden">LibreGene</span>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton onClick={handleOpenFile}>
                  <FolderOpen className="size-4" />
                  <span>Open File</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
        {projects.length > 0 && (
          <Collapsible open={filesOpen} onOpenChange={setFilesOpen}>
            <SidebarGroup>
              <CollapsibleTrigger asChild>
                <SidebarGroupLabel className="cursor-pointer">
                  Opened Files
                  <ChevronDown className={`ml-auto size-4 shrink-0 transition-transform ${filesOpen ? 'rotate-0' : '-rotate-90'}`} />
                </SidebarGroupLabel>
              </CollapsibleTrigger>
              <CollapsibleContent>
                <SidebarGroupContent>
                  <SidebarMenu>
                    {projects.map(p => (
                      <SidebarMenuItem key={p.id} className="flex items-center">
                        <SidebarMenuButton
                          onClick={() => handleSwitchProject(p.id)}
                          isActive={p.id === activeId}
                          className="flex-1"
                        >
                          {(() => { const Icon = getFileIcon(fileName(p)); return <Icon className="size-4 shrink-0" />; })()}
                          <span className="truncate hover:overflow-x-auto hover:[text-overflow:clip] [scrollbar-width:none] [&::-webkit-scrollbar]:[display:none]">{fileName(p)}{(p.id === activeId && isDirty) || dirtyStateRef.current[p.id] ? ' *' : ''}</span>
                        </SidebarMenuButton>
                        <div className="flex items-center gap-0.5 shrink-0 group-data-[state=collapsed]:hidden">
                          {/* "Open in new window" button removed — multi-window sync is incomplete */}
                          <button
                            className="size-4 shrink-0 opacity-40 hover:opacity-100 transition-opacity cursor-pointer"
                            onClick={(e) => { e.stopPropagation(); handleCloseProject(p.id); }}
                            title="Close"
                          >
                            <X className="size-3.5" />
                          </button>
                        </div>
                      </SidebarMenuItem>
                    ))}
                  </SidebarMenu>
                </SidebarGroupContent>
              </CollapsibleContent>
            </SidebarGroup>
          </Collapsible>
        )}
      </SidebarContent>
    </Sidebar>
  );

  return (
    <TooltipProvider>
      <SidebarProvider
        open={sidebarHover}
        style={{ "--sidebar-width": "14rem" }}
      >
        <div className="relative min-h-screen w-full bg-[#fdfbf7]">
          {/* Only show sidebar in main window */}
          {hasProject && (!windowInfo || windowInfo.type !== 'project') && (
            <div
              className="absolute left-0 top-0 bottom-0 z-40"
              onMouseEnter={handleSidebarEnter}
              onMouseLeave={handleSidebarLeave}
            >
              {sidebarContent}
            </div>
          )}
          <main className="w-full transition-[padding] duration-300 ease-out">
            {hasProject ? (
              <SequenceEditor
                sequence={sequence}
                features={editorFeatures}
                enzymes={displayEnzymes}
                primers={editorPrimers}
                charsPerLine={60}
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
                onSelectionChange={handleSelectionChange}
              />
            ) : (
              <Empty className="min-h-screen">
                <EmptyMedia variant="icon">
                  <Dna className="size-6" />
                </EmptyMedia>
                <EmptyTitle>No file opened</EmptyTitle>
                <EmptyDescription>
                  Open a GenBank (.gbk), SnapGene (.dna), or FASTA (.fasta) file to get started.
                </EmptyDescription>
                <EmptyContent>
                  <Button onClick={handleOpenFile}>
                    <FolderOpen className="size-4 mr-2" />
                    Open File
                  </Button>
                </EmptyContent>
              </Empty>
            )}
          </main>
        </div>

        <DebugPanel
          open={debugOpen}
          onOpenChange={setDebugOpen}
          showFeatures={showFeatures} setShowFeatures={setShowFeatures}
          features={features}
          showPrimers={showPrimers} setShowPrimers={setShowPrimers}
          primers={primers}
          enzymeFilter={enzymeFilter} setEnzymeFilter={setEnzymeFilter}
          enzymes={enzymes} displayEnzymes={displayEnzymes}
          methylationSystems={methylationSystems} setMethylationSystems={setMethylationSystems}
          methylationOverlap={methylationOverlap} setMethylationOverlap={setMethylationOverlap}
          primerSeedLength={primerSeedLength} setPrimerSeedLength={setPrimerSeedLength}
          isTauri={isTauri} openPath={openPath} setOpenPath={setOpenPath}
          fileStatus={fileStatus}
          layoutParams={layoutParams} setLP={setLP}
          onOpenFile={handleOpenFile}
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

        {/* --- Unsaved Changes Dialog --- */}
        <Dialog open={unsavedDialog.open} onOpenChange={(open) => { if (!open) handleUnsavedCancel(); }}>
          <DialogContent onInteractOutside={(e) => e.preventDefault()}>
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2">
                <AlertTriangle className="size-4 text-amber-500" />
                Unsaved Changes
              </DialogTitle>
              <DialogDescription>
                This project has unsaved changes. Save before continuing?
              </DialogDescription>
            </DialogHeader>
            <DialogFooter className="gap-2 sm:gap-0">
              <DialogClose asChild>
                <Button variant="outline" onClick={handleUnsavedCancel}>Cancel</Button>
              </DialogClose>
              <Button variant="outline" onClick={handleUnsavedDiscard}>Don't Save</Button>
              <Button onClick={handleUnsavedSave}>Save</Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <button
          className="fixed bottom-3 right-3 w-3.5 h-3.5 rounded-full z-50 opacity-60 hover:opacity-100 hover:scale-125 transition-all cursor-pointer border-0"
          style={{ background: backendStatus === 'online' || isTauri ? '#22c55e' : backendStatus === 'connecting' ? '#f59e0b' : '#9ca3af' }}
          onClick={() => setDebugOpen(true)}
          title={isTauri ? 'Desktop mode — Click for debug' : backendStatus === 'online' ? 'Backend connected — Click for debug' : backendStatus === 'connecting' ? 'Connecting...' : 'Offline — Click for debug'}
        />
      </SidebarProvider>
    </TooltipProvider>
  );
}
