import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import {
  openFile,
  openMsaFasta,
  peekFastaRecords,
  createProject,
  createMsaProject,
  openSnapgeneSnapshot,
  isTauri,
  openFileDialog,
  setAgentTabLocked,
  setAgentEditLock,
  setWindowTitle,
  setMcpConfig,
  getMcpConfig,
  isSequenceFilePath,
} from './tauriApi';
import { plugins, pluginAvailable } from './plugins';
import ProjectWorkspace from './ProjectWorkspace';
import MsaWorkspace from './msa/MsaWorkspace';
import ProjectSidebar from './components/ProjectSidebar';
import SettingsPage from './components/SettingsPage';
import TitleBar from './components/TitleBar';
import ContextMenuHost from './components/ContextMenuHost';
import AppDialogs from './components/AppDialogs';
import AboutDialog from './dialogs/AboutDialog';
import useSettings from './hooks/useSettings';
import useProjects from './hooks/useProjects';
import useTabDrag from './hooks/useTabDrag';
import useTauriEvents from './hooks/useTauriEvents';
import { useWheelContainment } from './hooks/useWheelContainment';
import { SidebarProvider } from '@/components/ui/sidebar';
import { Button } from '@/components/ui/button';
import { TooltipProvider } from '@/components/ui/tooltip';
import { FolderOpen, FilePlus2, Bot, Lock, Languages } from 'lucide-react';
import { useTranslation } from './i18n';
import {
  getRecentFiles,
  addRecentFile,
  removeRecentFile,
  getLastOpenedFile,
  fileNameOf,
  dirOf,
} from './recentFiles';
import { getMyPrimers } from './myPrimers';
import { getMyEnzymes } from './myEnzymes';
import { T } from '@/i18n';

function isOpenedProjectData(data) {
  return !!data && (data.projectKind === 'msa' ? data.msaRows?.length > 0 : !!data.sequence);
}

function isMsaFastaPath(path) {
  return /\.msa\.fasta?$/i.test(path || '');
}

export default function App() {
  const { t, locale, setLocale } = useTranslation();
  const [recentFiles, setRecentFiles] = useState(() => getRecentFiles());
  const [myPrimers, setMyPrimers] = useState(() => getMyPrimers());
  const [myEnzymes, setMyEnzymes] = useState(() => getMyEnzymes());
  const {
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
    thickFeatureLines,
    onToggleThickFeatureLines,
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
    showTextColor,
    onToggleTextColor,
    highContrastTextColors,
    onToggleHighContrastTextColors,
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
    primerTmMode,
    onPrimerTmModeChange,
    mcpConfig,
    setMcpConfigState,
    handleMcpConfigChange,
    caseDisplay,
    onCaseDisplayChange,
    pluginToggles,
    pluginSettings,
  } = useSettings();
  const [backendStatus, setBackendStatus] = useState(isTauri ? 'online' : 'offline');

  // Multi-project state
  const {
    projects,
    setProjects,
    activeId,
    setActiveId,
    orderedProjects,
    orderedProjectsRef,
    setProjectOrder,
    agentTabs,
    setAgentTabs,
    dirtyById,
    handlesRef,
    initialDataRef,
    keyFor,
    projectsRef,
    mergeAgentTabs,
    refreshProjects,
    onDirtyChange,
    registerHandle,
    onProjectsSync,
    onRekey,
    handleSwitchProject,
    doCloseProject,
  } = useProjects();

  const { dragTabId, tabItemRefs, suppressTabClickRef, handleTabPointerDown } = useTabDrag({
    orderedProjectsRef,
    setProjectOrder,
  });

  const [settingsOpen, setSettingsOpen] = useState(false);
  // 'plugins' → open the dialog as the plugin management view (plugins only)
  const [settingsFocus, setSettingsFocus] = useState(null);
  const openSettings = (focus = null) => {
    setSettingsFocus(focus);
    setSettingsOpen(true);
  };
  const [mcpGuideOpen, setMcpGuideOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [newSeqOpen, setNewSeqOpen] = useState(false);
  const [unsavedDialog, setUnsavedDialog] = useState({ open: false, pendingAction: null });
  const unsavedPendingRef = useRef(null);
  // Drag-drop: { paths } when the user must pick alignment-vs-open, and
  // { added, failed } for per-file failure feedback after alignment import.
  const [dropConfirm, setDropConfirm] = useState(null);
  const [dropResult, setDropResult] = useState(null);
  // Per-file failures from the Open File dialog: [{ path, error }]
  const [openError, setOpenError] = useState(null);
  // Multi-record FASTA pending split: { path, records: [{ name, length }] }
  const [fastaSplit, setFastaSplit] = useState(null);
  // Tray Quit blocked by unsaved changes: array of dirty project ids
  const [quitRequest, setQuitRequest] = useState(null);

  // Layout parameters (行距 / 特征 / 引物排版)
  const [layoutParams] = useState({
    // 引物排版
    fwdMatchY: 25,
    revMatchY: 15,
    misYDelta: 4,
    fwdBaseTextY: 8,
    revBaseTextY: 18,
    fwdLabelY: 8,
    revLabelY: 20,
    trackGap: 36,
    fwdAboveBase: 30,
    fwdAboveExtra: 23,
    fwdAboveNonTailExtra: 5,
    revBelowBase: 26,
    revBelowExtra: 25,
    revBelowNonTailExtra: 5,
    hoverExpand: 26,
    arrowHeadLen: 7,
    arrowHeadHeight: 5,
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

  // Scroll-edge fades for the sidebar list: show a fade-out at the top/bottom
  // when there are more items hidden in that direction.
  const sidebarContentRef = useRef(null);
  const [sidebarScroll, setSidebarScroll] = useState({ up: false, down: false });
  useEffect(() => {
    const el = sidebarContentRef.current;
    if (!el) return;
    const update = () => {
      setSidebarScroll({
        up: el.scrollTop > 1,
        down: el.scrollTop + el.clientHeight < el.scrollHeight - 1,
      });
    };
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    for (const child of el.children) ro.observe(child);
    return () => ro.disconnect();
  }, [projects.length]);

  // Sidebar hover state
  const [sidebarHover, setSidebarHover] = useState(false);
  const sidebarLeaveRef = useRef(null);
  // Wheel gestures that can't scroll the sidebar list must not leak to the
  // window behind (rubber-banding); the list itself still scrolls normally.
  const sidebarWheelRef = useWheelContainment();

  const handleSidebarEnter = useCallback(() => {
    clearTimeout(sidebarLeaveRef.current);
    setSidebarHover(true);
  }, []);

  const handleSidebarLeave = useCallback(() => {
    sidebarLeaveRef.current = setTimeout(() => setSidebarHover(false), 250);
  }, []);

  // Apply the persisted MCP config once on startup so the server reflects the
  // saved enable/port (the Rust side already started with the default config).
  useEffect(() => {
    if (!isTauri) return;
    let cfg = { enabled: true, port: 8766, requireAuth: true };
    try {
      cfg = { ...cfg, ...(JSON.parse(localStorage.getItem('mcpConfig')) || {}) };
    } catch {
      /* ignore malformed stored config */
    }
    setMcpConfig(Boolean(cfg.enabled), Number(cfg.port), cfg.requireAuth !== false).catch(() => {});
  }, []);

  // The MCP guide reflects the backend's real state: a failed bind flips
  // enabled off server-side while localStorage still claims it is running.
  useEffect(() => {
    if (!mcpGuideOpen || !isTauri) return undefined;
    let cancelled = false;
    getMcpConfig()
      .then((cfg) => {
        if (cancelled || !cfg) return;
        const actual = {
          enabled: !!cfg.enabled,
          port: Number(cfg.port),
          requireAuth: cfg.requireAuth !== false,
        };
        setMcpConfigState((prev) => {
          if (
            prev.enabled === actual.enabled &&
            prev.port === actual.port &&
            (prev.requireAuth !== false) === actual.requireAuth
          )
            return prev;
          try {
            localStorage.setItem('mcpConfig', JSON.stringify(actual));
          } catch {
            // storage may be unavailable; config still applies in-memory
          }
          return actual;
        });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [mcpGuideOpen, setMcpConfigState]);

  // --- beforeunload: warn on close with unsaved changes in any workspace ---
  const anyDirty = Object.values(dirtyById).some(Boolean);
  useEffect(() => {
    const handler = (e) => {
      if (anyDirty) {
        e.preventDefault();
        e.returnValue = '';
      }
    };
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  }, [anyDirty]);

  // Extract filename from path
  const fileName = (p) => {
    const s = (p.name || p.id || '').replace(/\\/g, '/');
    return s.split('/').pop() || s;
  };

  // Title: display name of the active project (the list name carries the
  // entered name for unsaved `untitled-*` projects), or the app name
  const activeTitle = activeId
    ? fileName(projects.find((p) => p.id === activeId) || { id: activeId })
    : 'LibreGene';
  const activeDirty = activeId ? dirtyById[activeId] === true : false;
  useEffect(() => {
    setWindowTitle(activeTitle);
  }, [activeTitle]);

  // Sync unsavedPendingRef alongside setUnsavedDialog for stale-closure-safe access
  const openUnsavedDialog = useCallback((pendingAction) => {
    setUnsavedDialog({ open: true, pendingAction });
    unsavedPendingRef.current = pendingAction;
  }, []);

  const handleOpenFile = useCallback(async () => {
    if (!isTauri) return;
    // Start the native dialog in the directory of the last opened file,
    // so the user doesn't have to navigate from scratch each time.
    const lastDir = dirOf(getLastOpenedFile());
    const paths = await openFileDialog(lastDir || undefined);
    if (!paths || !paths.length) return;
    const failed = [];
    let splitPending = null;
    for (const p of paths) {
      // Reopening an already-open path would replace the project and lose
      // unsaved edits — just activate it instead
      if (projects.some((pr) => pr.id === p)) {
        handleSwitchProject(p);
        continue;
      }
      // Multi-record FASTA: ask whether to split into separate projects.
      // Only one such dialog per batch — later files fall back to the first
      // record (historical behavior).
      try {
        const peek = await peekFastaRecords(p);
        if (
          !isMsaFastaPath(p) &&
          peek &&
          peek.records &&
          peek.records.length > 1 &&
          !splitPending
        ) {
          splitPending = { path: p, records: peek.records };
          continue;
        }
      } catch {
        // Peek failure is not fatal — open normally below.
      }
      try {
        const data = await openFile(p);
        if (isOpenedProjectData(data)) {
          // Hand the response to the new workspace so it doesn't refetch
          initialDataRef.current[data.id || p] = data;
          setRecentFiles(addRecentFile(p));
        } else {
          failed.push({ path: p, error: data?.error || t('Not a readable sequence file') });
        }
      } catch (e) {
        failed.push({ path: p, error: e?.message || String(e) });
      }
    }
    await refreshProjects();
    if (failed.length > 0) setOpenError(failed);
    if (splitPending) setFastaSplit(splitPending);
  }, [projects, refreshProjects, handleSwitchProject, initialDataRef, t]);

  // FastaSplitDialog choice: 'split' opens every record as its own project
  // (project id `<path>#record-<i>`), 'first' opens record 0 only, null = cancel.
  const handleFastaSplitChoice = useCallback(
    async (choice) => {
      const target = fastaSplit;
      setFastaSplit(null);
      if (!target || !choice) return;
      if (choice === 'msa') {
        try {
          const data = await openMsaFasta(target.path);
          if (isOpenedProjectData(data)) {
            initialDataRef.current[data.id || target.path] = data;
            setRecentFiles(addRecentFile(target.path));
          } else {
            setOpenError([{ path: target.path, error: data?.error || t('Failed to open MSA') }]);
          }
        } catch (e) {
          setOpenError([{ path: target.path, error: e?.message || String(e) }]);
        }
        await refreshProjects();
        return;
      }
      const indices = choice === 'split' ? target.records.map((_, i) => i) : [0];
      const failed = [];
      setRecentFiles(addRecentFile(target.path));
      for (const i of indices) {
        try {
          const data = await openFile(target.path, i);
          if (isOpenedProjectData(data)) {
            initialDataRef.current[data.id || `${target.path}#record-${i}`] = data;
          } else {
            failed.push({
              path: `${fileNameOf(target.path)} (record ${i + 1})`,
              error: data?.error || t('Not a readable sequence file'),
            });
          }
        } catch (e) {
          failed.push({
            path: `${fileNameOf(target.path)} (record ${i + 1})`,
            error: e?.message || String(e),
          });
        }
      }
      await refreshProjects();
      if (failed.length > 0) setOpenError(failed);
    },
    [fastaSplit, refreshProjects, initialDataRef, t],
  );

  // Create an in-memory project from the New File dialog; hand the
  // response to the new workspace (same pattern as handleOpenFile).
  const handleCreateProject = useCallback(
    async (payload) => {
      try {
        const data = await createProject(payload);
        if (isOpenedProjectData(data) && data.id) {
          initialDataRef.current[data.id] = data;
          await refreshProjects();
          return { ok: true };
        }
        return { ok: false, error: data?.error || t('Failed to create project') };
      } catch (e) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    [refreshProjects, initialDataRef, t],
  );

  const handleCreateMsa = useCallback(
    async (payload) => {
      try {
        const data = await createMsaProject(payload);
        if (isOpenedProjectData(data) && data.id) {
          initialDataRef.current[data.id] = data;
          await refreshProjects();
          return { ok: true };
        }
        return { ok: false, error: data?.error || t('Failed to create MSA') };
      } catch (e) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    [refreshProjects, initialDataRef, t],
  );

  const handleCreateMsaFromActive = useCallback(() => {
    const source = projects.find((project) => project.id === activeId);
    const handle = activeId ? handlesRef.current[activeId] : null;
    const sequence = handle?.getSequence?.();
    if (!source || !sequence) return { ok: false, error: t('No active sequence') };
    const base = fileName(source).replace(/\.[^.]+$/, '');
    return handleCreateMsa({
      name: `${base} MSA`,
      sequence,
      moleculeType: source.moleculeType,
    });
  }, [activeId, projects, handlesRef, handleCreateMsa, t]);

  // Open a SnapGene history snapshot as a new in-memory project (Snapshots
  // dialog); the workspace mounts for the new id like handleCreateProject.
  const handleOpenSnapshot = useCallback(
    async (projectId, nodeId) => {
      try {
        const data = await openSnapgeneSnapshot(projectId, nodeId);
        if (isOpenedProjectData(data) && data.id) {
          initialDataRef.current[data.id] = data;
          await refreshProjects();
          return { ok: true };
        }
        return { ok: false, error: data?.error || t('Failed to open snapshot') };
      } catch (e) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    [refreshProjects, initialDataRef, t],
  );

  // Open a path directly from the recent-files list (no native dialog).
  const handleOpenRecent = useCallback(
    async (path) => {
      // Already open — just activate it instead of reloading over unsaved edits
      if (projects.some((p) => p.id === path)) {
        handleSwitchProject(path);
        return;
      }
      try {
        const data = await openFile(path);
        if (isOpenedProjectData(data)) {
          initialDataRef.current[data.id || path] = data;
          setRecentFiles(addRecentFile(path));
        }
        await refreshProjects();
      } catch (e) {
        // File may have been moved/deleted — drop it from the list.
        console.error('open recent error:', e);
        setRecentFiles(removeRecentFile(path));
      }
    },
    [projects, refreshProjects, handleSwitchProject, initialDataRef],
  );

  const handleRemoveRecent = useCallback((path) => {
    setRecentFiles(removeRecentFile(path));
  }, []);

  // Open a file handed over by the OS (Open With / double-click / dock drop /
  // second-instance forward). Dedups against both already-open projects and
  // in-flight opens (the backend queues AND emits, so a cold-start path can
  // arrive twice).
  const openingRef = useRef(new Set());
  const openExternalPath = useCallback(
    async (path) => {
      if (!path || openingRef.current.has(path)) return;
      openingRef.current.add(path);
      try {
        if (projectsRef.current.some((p) => p.id === path)) {
          handleSwitchProject(path);
          return;
        }
        const data = await openFile(path);
        if (isOpenedProjectData(data)) {
          initialDataRef.current[data.id || path] = data;
          setRecentFiles(addRecentFile(path));
        } else if (data && data.error) {
          console.error('open external file error:', data.error);
        }
        await refreshProjects();
      } catch (e) {
        console.error('open external file error:', e);
      } finally {
        openingRef.current.delete(path);
      }
    },
    [refreshProjects, handleSwitchProject, initialDataRef, projectsRef],
  );

  // Public close handler with dirty check
  const handleCloseProject = useCallback(
    async (id) => {
      if (dirtyById[id] === true) {
        openUnsavedDialog({ type: 'close', targetId: id });
        return;
      }
      doCloseProject(id);
    },
    [dirtyById, openUnsavedDialog, doCloseProject],
  );

  // --- Unsaved changes dialog handlers ---
  const handleUnsavedSave = useCallback(async () => {
    const pending = unsavedPendingRef.current;
    setUnsavedDialog({ open: false, pendingAction: null });
    unsavedPendingRef.current = null;
    if (pending?.type === 'close' && pending.targetId) {
      await handlesRef.current[pending.targetId]?.save();
      // Save may have been cancelled (Save As dialog) — don't close if still dirty
      if (handlesRef.current[pending.targetId]?.isDirty()) return;
      doCloseProject(pending.targetId);
    }
  }, [doCloseProject, handlesRef]);

  const handleUnsavedDiscard = useCallback(() => {
    const pending = unsavedPendingRef.current;
    setUnsavedDialog({ open: false, pendingAction: null });
    unsavedPendingRef.current = null;
    if (pending?.type === 'close' && pending.targetId) {
      doCloseProject(pending.targetId);
    }
  }, [doCloseProject]);

  const handleUnsavedCancel = useCallback(() => {
    setUnsavedDialog({ open: false, pendingAction: null });
    unsavedPendingRef.current = null;
  }, []);

  // Recent group is collapsible (default collapsed); already-open files are hidden from it
  const [recentOpen, setRecentOpen] = useState(false);
  useEffect(() => {
    if (!sidebarHover) setRecentOpen(false);
  }, [sidebarHover]);
  const visibleRecent = recentFiles.filter((p) => !projects.some((pr) => pr.id === p));

  const activeAgentLocked = !!activeId && agentTabs[activeId] === true;
  const activeAgentUnlocked = !!activeId && agentTabs[activeId] === false;

  // While the active project is a locked agent tab, the workspace stays
  // interactive (scroll/select/copy all work) and dirty-producing operations
  // are refused instead: ProjectWorkspace guards its own mutation handlers
  // before any optimistic update, and tauriApi rejects dirty-producing
  // commands for direct callers (dialogs).
  useEffect(() => {
    setAgentEditLock(activeAgentLocked);
  }, [activeAgentLocked]);

  const handleSetAgentLocked = useCallback((projectId, locked) => {
    setAgentTabLocked(projectId, locked).catch(() => {});
  }, []);

  const hasProject = projects.length > 0;
  const sidebarTargetId = activeId;
  // rna/protein projects hide DNA-only sidebar entries (ORFs, codon
  // optimization); DNA-only plugins are marked dnaOnly in the registry,
  // RNA-only plugins (RNA folding) are marked rnaOnly.
  const activeProject = projects.find((p) => p.id === activeId);
  const activeMoleculeType = activeProject?.moleculeType || 'dna';
  // While the active project's moleculeType is unknown (project list still
  // loading), hide type-gated entries instead of flashing DNA-only items.
  const moleculeTypeKnown = !activeId || !!activeProject;
  const visiblePlugins =
    activeProject?.projectKind === 'msa'
      ? []
      : plugins.filter((plugin) =>
          pluginAvailable(plugin, {
            moleculeType: moleculeTypeKnown ? activeMoleculeType : null,
            disabledPlugins,
            isTauri,
          }),
        );

  // Files dragged onto the window: valid sequence files either attach to the
  // active DNA project as alignments (after confirmation) or open as new
  // projects. Invalid extensions are ignored silently.
  const handleDroppedPaths = useCallback(
    (paths) => {
      if (activeAgentLocked) return;
      const valid = (paths || []).filter(isSequenceFilePath);
      if (valid.length === 0) return;
      const handle = sidebarTargetId ? handlesRef.current[sidebarTargetId] : null;
      if (handle?.alignmentEnabled && handle?.addAlignmentFiles) {
        setDropConfirm({ paths: valid });
      } else {
        valid.forEach((p) => openExternalPath(p));
      }
    },
    [sidebarTargetId, openExternalPath, activeAgentLocked, handlesRef],
  );

  useTauriEvents({
    setBackendStatus,
    setProjects,
    setActiveId,
    mergeAgentTabs,
    refreshProjects,
    setQuitRequest,
    openExternalPath,
    setAgentTabs,
    handleDroppedPaths,
  });

  const handleDropAddAsAlignment = useCallback(async () => {
    const paths = dropConfirm?.paths || [];
    const targetId = sidebarTargetId;
    setDropConfirm(null);
    const handle = targetId ? handlesRef.current[targetId] : null;
    if (!handle?.addAlignmentFiles || paths.length === 0) return;
    const result = await handle.addAlignmentFiles(paths);
    if (result && result.failed.length > 0) {
      setDropResult({ added: result.added, failed: result.failed });
    }
  }, [dropConfirm, sidebarTargetId, handlesRef]);

  const handleDropOpenAsNew = useCallback(() => {
    const paths = dropConfirm?.paths || [];
    setDropConfirm(null);
    paths.forEach((p) => openExternalPath(p));
  }, [dropConfirm, openExternalPath]);

  const workspaceProps = useMemo(
    () => ({
      backendStatus,
      methylationSystems,
      methylationOverlap,
      primerSeedLength,
      alignmentAlgorithm,
      tmParams,
      primerTmMode,
      onPrimerTmModeChange,
      caseDisplay,
      layoutParams,
      showFeatures,
      onToggleFeatures,
      pluginToggles,
      pluginSettings,
      alwaysExpandFeatures,
      onToggleAlwaysExpandFeatures,
      alwaysExpandPrimers,
      onToggleAlwaysExpandPrimers,
      showPrimers,
      onTogglePrimers,
      featureLabelsBelow,
      onFeatureLabelsBelowChange,
      thickFeatureLines,
      onToggleThickFeatureLines,
      showEnzymes,
      onToggleEnzymes,
      enzymeFilter,
      onEnzymeFilterChange,
      enzymeProvider,
      onEnzymeProviderChange,
      maxEnzymeSites,
      viewMode,
      onViewModeChange,
      showDoubleStrand,
      onToggleDoubleStrand,
      showTextColor,
      onToggleTextColor,
      highContrastTextColors,
      disabledPlugins,
      onDirtyChange,
      registerHandle,
      onProjectsSync,
      onRekey,
      onCreateMsa: handleCreateMsaFromActive,
      myPrimers,
      onMyPrimersChange: setMyPrimers,
      myEnzymes,
      onMyEnzymesChange: setMyEnzymes,
      autoAddPrimers,
      onToggleAutoAddPrimers,
      onOpenSnapshot: handleOpenSnapshot,
    }),
    [
      backendStatus,
      methylationSystems,
      methylationOverlap,
      primerSeedLength,
      alignmentAlgorithm,
      tmParams,
      primerTmMode,
      onPrimerTmModeChange,
      caseDisplay,
      layoutParams,
      showFeatures,
      onToggleFeatures,
      pluginToggles,
      pluginSettings,
      alwaysExpandFeatures,
      onToggleAlwaysExpandFeatures,
      alwaysExpandPrimers,
      onToggleAlwaysExpandPrimers,
      showPrimers,
      onTogglePrimers,
      featureLabelsBelow,
      onFeatureLabelsBelowChange,
      thickFeatureLines,
      onToggleThickFeatureLines,
      showEnzymes,
      onToggleEnzymes,
      enzymeFilter,
      onEnzymeFilterChange,
      enzymeProvider,
      onEnzymeProviderChange,
      maxEnzymeSites,
      viewMode,
      onViewModeChange,
      showDoubleStrand,
      onToggleDoubleStrand,
      showTextColor,
      onToggleTextColor,
      highContrastTextColors,
      disabledPlugins,
      onDirtyChange,
      registerHandle,
      onProjectsSync,
      onRekey,
      handleCreateMsaFromActive,
      myPrimers,
      myEnzymes,
      autoAddPrimers,
      onToggleAutoAddPrimers,
      handleOpenSnapshot,
    ],
  );

  const sidebarContent = (
    <ProjectSidebar
      sidebarWheelRef={sidebarWheelRef}
      nativeTitleBar={isTauri && !/Mac/i.test(navigator.platform)}
      handleOpenFile={handleOpenFile}
      setNewSeqOpen={setNewSeqOpen}
      sidebarScroll={sidebarScroll}
      setSidebarScroll={setSidebarScroll}
      sidebarContentRef={sidebarContentRef}
      visibleRecent={visibleRecent}
      recentOpen={recentOpen}
      setRecentOpen={setRecentOpen}
      handleOpenRecent={handleOpenRecent}
      handleRemoveRecent={handleRemoveRecent}
      projects={projects}
      orderedProjects={orderedProjects}
      activeId={activeId}
      dirtyById={dirtyById}
      fileName={fileName}
      dragTabId={dragTabId}
      tabItemRefs={tabItemRefs}
      handleTabPointerDown={handleTabPointerDown}
      suppressTabClickRef={suppressTabClickRef}
      handleSwitchProject={handleSwitchProject}
      handleCloseProject={handleCloseProject}
      visiblePlugins={visiblePlugins}
      handlesRef={handlesRef}
      sidebarTargetId={sidebarTargetId}
      setMcpGuideOpen={setMcpGuideOpen}
      openSettings={openSettings}
      setAboutOpen={setAboutOpen}
    />
  );

  return (
    <TooltipProvider>
      <SidebarProvider open={sidebarHover} style={{ '--sidebar-width': '14rem' }}>
        <div className="relative flex h-screen w-full flex-col overflow-hidden bg-background">
          <TitleBar title={activeTitle} dirty={activeDirty} />
          {/* Locked agent tab: the bottom nav menu is replaced by a
              teal-outlined control pill (SequenceEditor), which also blocks
              nav-level misoperation. The workspace itself stays interactive —
              scrolling, selecting and copying work; only dirty-producing
              edits are refused (ProjectWorkspace handler guard + tauriApi
              command guard). */}
          {/* Unlocked agent tab: floating badge with a manual re-lock */}
          {activeAgentUnlocked && (
            <div className="fixed bottom-4 right-4 z-[60] flex items-center gap-2 rounded-md border border-border bg-background/90 px-3 py-1.5 text-xs text-muted-foreground shadow-sm">
              <Bot className="size-3.5 text-teal-500" />
              <span>
                <T>{'Agent tab — unlocked'}</T>
              </span>
              <Button
                variant="ghost"
                size="sm"
                className="h-6 gap-1 px-2 text-xs"
                onClick={() => handleSetAgentLocked(activeId, true)}
              >
                <Lock className="size-3" />
                <T>{'Lock'}</T>
              </Button>
            </div>
          )}
          {hasProject && (
            <div
              className="absolute left-0 top-0 bottom-0 z-40"
              onMouseEnter={handleSidebarEnter}
              onMouseLeave={handleSidebarLeave}
            >
              {sidebarContent}
            </div>
          )}
          <div className="relative flex flex-1 min-h-0">
            {projects.length > 0 ? (
              orderedProjects.map((p) =>
                p.projectKind === 'msa' ? (
                  <MsaWorkspace
                    key={keyFor(p.id)}
                    projectId={p.id}
                    projectName={p.name}
                    hidden={p.id !== activeId}
                    initialData={initialDataRef.current[p.id]}
                    agentLocked={agentTabs[p.id] === true}
                    leftViewportInset={48}
                    {...workspaceProps}
                  />
                ) : (
                  <ProjectWorkspace
                    key={keyFor(p.id)}
                    projectId={p.id}
                    projectName={p.name}
                    hidden={p.id !== activeId}
                    initialData={initialDataRef.current[p.id]}
                    topology={p.topology || 'circular'}
                    moleculeType={p.moleculeType || 'dna'}
                    agentLocked={agentTabs[p.id] === true}
                    leftViewportInset={48}
                    {...workspaceProps}
                  />
                ),
              )
            ) : (
              <main className="w-full flex-1 overflow-y-auto overscroll-contain hide-scrollbar">
                <div className="flex min-h-full flex-col items-center justify-center gap-5 p-6 text-center">
                  <div className="flex size-16 items-center justify-center">
                    <img src="/icon.png" alt="LibreGene" className="size-14" />
                  </div>
                  <div className="space-y-1.5">
                    <h1 className="text-lg font-semibold tracking-tight">
                      <T>{'LibreGene'}</T>
                    </h1>
                    <p className="max-w-sm text-sm leading-relaxed text-muted-foreground">
                      {t('Open a sequence file to start viewing and editing your plasmid.')}
                    </p>
                  </div>
                  <div className="flex max-w-md flex-wrap items-center justify-center gap-1.5">
                    {[
                      '.gbk',
                      '.gbff',
                      '.dna',
                      '.fasta',
                      '.msa.fa',
                      '.faa',
                      '.ab1',
                      '.rna',
                      '.prot',
                      '.gpt',
                      '.gp',
                      '.seq',
                    ].map((ext) => (
                      <span
                        key={ext}
                        className="rounded-md border border-border bg-muted px-2 py-0.5 font-mono text-[11px] text-muted-foreground"
                      >
                        {ext}
                      </span>
                    ))}
                  </div>
                  <div className="mt-1 flex items-center gap-2">
                    <Button onClick={handleOpenFile} size="lg">
                      <FolderOpen className="size-4" />
                      {t('Open File')}
                    </Button>
                    <Button variant="outline" size="lg" onClick={() => setNewSeqOpen(true)}>
                      <FilePlus2 className="size-4" />
                      {t('New File')}
                    </Button>
                  </div>
                  <button
                    type="button"
                    onClick={() => setLocale(locale === 'zh' ? 'en' : 'zh')}
                    aria-label={t('Switch language')}
                    title={t('Switch language')}
                    className="flex items-center gap-1.5 text-xs text-muted-foreground transition-colors hover:text-foreground"
                  >
                    <Languages className="size-3.5" />
                    {locale === 'zh' ? '中文 / English' : 'English / 中文'}
                  </button>
                  <button
                    type="button"
                    onClick={() => setMcpGuideOpen(true)}
                    className="flex items-center gap-1.5 text-xs text-muted-foreground transition-colors hover:text-foreground"
                  >
                    <Bot className="size-3.5" />
                    {t('Connect an LLM agent via MCP')}
                  </button>
                </div>
              </main>
            )}
          </div>
        </div>

        <AboutDialog open={aboutOpen} onOpenChange={setAboutOpen} />

        <SettingsPage
          open={settingsOpen}
          onOpenChange={setSettingsOpen}
          focusSection={settingsFocus}
          pluginSettings={pluginSettings}
          methylationSystems={methylationSystems}
          setMethylationSystems={setMethylationSystems}
          methylationOverlap={methylationOverlap}
          setMethylationOverlap={setMethylationOverlap}
          maxEnzymeSites={maxEnzymeSites}
          onMaxEnzymeSitesChange={onMaxEnzymeSitesChange}
          primerSeedLength={primerSeedLength}
          setPrimerSeedLength={setPrimerSeedLength}
          alignmentAlgorithm={alignmentAlgorithm}
          onAlignmentAlgorithmChange={onAlignmentAlgorithmChange}
          tmParams={tmParams}
          setTmParams={setTmParams}
          primerTmMode={primerTmMode}
          onPrimerTmModeChange={onPrimerTmModeChange}
          caseDisplay={caseDisplay}
          onCaseDisplayChange={onCaseDisplayChange}
          highContrastTextColors={highContrastTextColors}
          onToggleHighContrastTextColors={onToggleHighContrastTextColors}
          showTextColor={showTextColor}
          plugins={plugins}
          disabledPlugins={disabledPlugins}
          onTogglePlugin={handleTogglePlugin}
        />

        <AppDialogs
          mcpGuideOpen={mcpGuideOpen}
          setMcpGuideOpen={setMcpGuideOpen}
          mcpConfig={mcpConfig}
          onMcpConfigChange={handleMcpConfigChange}
          newSeqOpen={newSeqOpen}
          setNewSeqOpen={setNewSeqOpen}
          onCreateProject={handleCreateProject}
          onCreateMsa={handleCreateMsa}
          unsavedDialog={unsavedDialog}
          onUnsavedCancel={handleUnsavedCancel}
          onUnsavedDiscard={handleUnsavedDiscard}
          onUnsavedSave={handleUnsavedSave}
          dropConfirm={dropConfirm}
          setDropConfirm={setDropConfirm}
          onDropOpenAsNew={handleDropOpenAsNew}
          onDropAddAsAlignment={handleDropAddAsAlignment}
          dropResult={dropResult}
          setDropResult={setDropResult}
          openError={openError}
          setOpenError={setOpenError}
          fastaSplit={fastaSplit}
          onFastaSplitChoice={handleFastaSplitChoice}
          quitRequest={quitRequest}
          setQuitRequest={setQuitRequest}
        />
      </SidebarProvider>
      <ContextMenuHost />
    </TooltipProvider>
  );
}
