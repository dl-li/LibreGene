import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import {
  openFile,
  peekFastaRecords,
  createProject,
  isTauri,
  openFileDialog,
  listenProjectUpdates,
  listenFileOpened,
  takePendingOpens,
  getProjects,
  activateProject,
  getWindowProjectId,
  getAgentTabState,
  setAgentTabLocked,
  setAgentEditLock,
  listenAgentTabLock,
  listenQuitRequested,
  forceQuit,
  openInNewWindow,
  deleteProject,
  setWindowTitle,
  setMcpConfig,
  getMcpConfig,
  listenDragDrop,
  isSequenceFilePath,
} from './tauriApi';
import { plugins } from './plugins';
import ProjectWorkspace from './ProjectWorkspace';
import SettingsPage from './components/SettingsPage';
import McpGuideDialog from './components/McpGuideDialog';
import NewSequenceDialog from './NewSequenceDialog';
import FastaSplitDialog from './FastaSplitDialog';
import TitleBar from './components/TitleBar';
import ContextMenuHost from './components/ContextMenuHost';
import {
  SidebarProvider,
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Button } from '@/components/ui/button';
import { TooltipProvider } from '@/components/ui/tooltip';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
  DialogClose,
} from '@/components/ui/dialog';
import {
  LoaderCircle,
  FolderOpen,
  FilePlus2,
  ChevronDown,
  AlertTriangle,
  X,
  ExternalLink,
  Settings,
  Puzzle,
  Bot,
  Lock,
  Map as MapIcon,
  Clock,
} from 'lucide-react';
import { getFileIcon } from './fileIcons';
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

export default function App() {
  const [disabledPlugins, setDisabledPlugins] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('disabledPlugins')) || [];
    } catch {
      return [];
    }
  });
  const [recentFiles, setRecentFiles] = useState(() => getRecentFiles());
  const [myPrimers, setMyPrimers] = useState(() => getMyPrimers());
  const [myEnzymes, setMyEnzymes] = useState(() => getMyEnzymes());
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
      setMcpConfig(Boolean(next.enabled), Number(next.port))
        .then(() => getMcpConfig())
        .then((cfg) => {
          // A failed bind flips enabled off server-side; adopt that truth.
          if (!cfg) return;
          const actual = { enabled: !!cfg.enabled, port: Number(cfg.port) };
          if (actual.enabled === Boolean(next.enabled) && actual.port === Number(next.port)) {
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
  const [backendStatus, setBackendStatus] = useState(isTauri ? 'online' : 'offline');
  // True when the decoration plugin can't take over the titlebar (Linux/X11)
  // and TitleBar hides itself; the sidebar must drop its reserved top padding.
  const [titleBarFallback, setTitleBarFallback] = useState(false);
  const handleTitleBarFallback = useCallback(() => setTitleBarFallback(true), []);

  // Multi-project state
  const [projects, setProjects] = useState([]);
  const [activeId, setActiveId] = useState(null);

  // Multi-window: tracks whether this window is "main" or a "project" window
  const [windowInfo, setWindowInfo] = useState(null);
  // { type: 'main' } | { type: 'project', projectId }
  // Agent tabs live in the main window; their lock state lives in agentTabs.
  const [agentTabs, setAgentTabs] = useState({}); // { [projectId]: locked }

  const [settingsOpen, setSettingsOpen] = useState(false);
  // 'plugins' → open the dialog as the plugin management view (plugins only)
  const [settingsFocus, setSettingsFocus] = useState(null);
  const openSettings = (focus = null) => {
    setSettingsFocus(focus);
    setSettingsOpen(true);
  };
  const [mcpGuideOpen, setMcpGuideOpen] = useState(false);
  const [newSeqOpen, setNewSeqOpen] = useState(false);
  const [showFeatures, setShowFeatures] = useState(true);
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
      const v = localStorage.getItem('enzymeFilter');
      return v && ENZYME_FILTER_VALUES.has(v) ? v : 'unique+twice';
    } catch {
      return 'unique+twice';
    }
  });
  const [methylationSystems, setMethylationSystems] = useState(['dam', 'dcm', 'ecoki']);
  const [methylationOverlap, setMethylationOverlap] = useState(2);
  const [primerSeedLength, setPrimerSeedLength] = useState(10);
  const [tmParams, setTmParams] = useState({
    naConc: 0.05,
    mgConc: 0,
    dntpConc: 0,
    trisConc: 0,
    primerConc: 2.5e-7,
  });
  const [mcpConfig, setMcpConfigState] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('mcpConfig')) || { enabled: true, port: 8766 };
    } catch {
      return { enabled: true, port: 8766 };
    }
  });
  const activeIdRef = useRef(null);
  // Per-project dirty state reported by workspaces: { [projectId]: bool }
  const [dirtyById, setDirtyById] = useState({});
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
  const handlesRef = useRef({}); // { [projectId]: workspace imperative handle }
  const initialDataRef = useRef({}); // { [projectId]: openFile response } — consumed on workspace mount
  const keyMapRef = useRef({}); // { [projectId]: stable React key } — survives Save As rekey

  const keyFor = useCallback((id) => {
    if (!keyMapRef.current[id]) keyMapRef.current[id] = id;
    return keyMapRef.current[id];
  }, []);

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

  useEffect(() => {
    activeIdRef.current = activeId;
  }, [activeId]);

  const projectsRef = useRef([]);
  useEffect(() => {
    projectsRef.current = projects;
  }, [projects]);

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

  // Merge the agentLocked field of a projects payload into the agentTabs map,
  // pruning entries for projects that are no longer in the list.
  const mergeAgentTabs = useCallback((projs) => {
    const next = {};
    for (const p of projs) {
      if (p.agentLocked != null) next[p.id] = !!p.agentLocked;
    }
    setAgentTabs((prev) => {
      if (Object.keys(prev).length === 0 && Object.keys(next).length === 0) return prev;
      let changed = false;
      for (const id of Object.keys(next)) {
        if (prev[id] !== next[id]) changed = true;
      }
      for (const id of Object.keys(prev)) {
        if (!(id in next)) changed = true;
      }
      return changed ? next : prev;
    });
  }, []);

  // Sync project list from backend
  const refreshProjects = useCallback(async () => {
    try {
      const data = await getProjects();
      if (data && !data.error) {
        setProjects(data.projects || []);
        setActiveId(data.activeId || null);
        mergeAgentTabs(data.projects || []);
      }
    } catch {
      // backend unreachable; keep current project list
    }
  }, [mergeAgentTabs]);

  // Apply the persisted MCP config once on startup so the server reflects the
  // saved enable/port (the Rust side already started with the default config).
  // Only the main window pushes config; project windows would just repeat it.
  useEffect(() => {
    if (!isTauri || windowInfo?.type !== 'main') return;
    let cfg = { enabled: true, port: 8766 };
    try {
      cfg = JSON.parse(localStorage.getItem('mcpConfig')) || cfg;
    } catch {
      /* ignore malformed stored config */
    }
    setMcpConfig(Boolean(cfg.enabled), Number(cfg.port)).catch(() => {});
  }, [windowInfo]);

  // The MCP guide reflects the backend's real state: a failed bind flips
  // enabled off server-side while localStorage still claims it is running.
  useEffect(() => {
    if (!mcpGuideOpen || !isTauri) return undefined;
    let cancelled = false;
    getMcpConfig()
      .then((cfg) => {
        if (cancelled || !cfg) return;
        const actual = { enabled: !!cfg.enabled, port: Number(cfg.port) };
        setMcpConfigState((prev) => {
          if (prev.enabled === actual.enabled && prev.port === actual.port) return prev;
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
  }, [mcpGuideOpen]);

  // Detect window type on mount
  useEffect(() => {
    if (!isTauri) {
      setWindowInfo({ type: 'main' });
      return;
    }
    getWindowProjectId()
      .then((pid) => {
        setWindowInfo(pid ? { type: 'project', projectId: pid } : { type: 'main' });
      })
      .catch(() => setWindowInfo({ type: 'main' }));
  }, []);

  // Main window: load project list + listen for project list updates.
  // Per-project data is loaded by each ProjectWorkspace itself.
  useEffect(() => {
    if (!windowInfo) return;
    let listener = null;
    let cancelled = false;

    if (windowInfo.type === 'main') {
      setBackendStatus(isTauri ? 'online' : 'connecting');
      (async () => {
        try {
          await refreshProjects();
          if (!cancelled) setBackendStatus('online');
        } catch {
          if (!cancelled) setBackendStatus(isTauri ? 'online' : 'offline');
        }
      })();
      listener = listenProjectUpdates((msg) => {
        if (cancelled) return;
        if (msg.projects) {
          setProjects(msg.projects);
          mergeAgentTabs(msg.projects);
        }
        if (msg.activeId !== undefined) {
          setActiveId(msg.activeId);
        }
      });
    }

    return () => {
      cancelled = true;
      if (listener) listener.close();
    };
  }, [windowInfo, refreshProjects, mergeAgentTabs]);

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

  // Tray Quit with unsaved changes: backend shows this window and emits
  // quit-requested (payload = dirty project ids); confirm here before force-quit.
  useEffect(() => {
    if (!isTauri || !windowInfo || windowInfo.type !== 'main') return;
    const listener = listenQuitRequested((dirtyIds) => {
      setQuitRequest(dirtyIds);
    });
    return () => listener.close();
  }, [windowInfo]);

  // Extract filename from path
  const fileName = (p) => {
    const s = (p.name || p.id || '').replace(/\\/g, '/');
    return s.split('/').pop() || s;
  };

  // Title: display name of the active project (the list name carries the
  // entered name for unsaved `untitled-*` projects), or the app name
  const titleSourceId =
    activeId || (windowInfo && windowInfo.type !== 'main' ? windowInfo.projectId : null);
  const activeTitle = titleSourceId
    ? fileName(projects.find((p) => p.id === titleSourceId) || { id: titleSourceId })
    : 'LibreGene';
  const titleId = windowInfo && windowInfo.type !== 'main' ? windowInfo.projectId : activeId;
  const activeDirty = titleId ? dirtyById[titleId] === true : false;
  useEffect(() => {
    setWindowTitle(activeTitle);
  }, [activeTitle]);

  const onDirtyChange = useCallback((id, dirty) => {
    setDirtyById((prev) => (prev[id] === dirty ? prev : { ...prev, [id]: dirty }));
  }, []);

  const registerHandle = useCallback((id, handle) => {
    if (handle) {
      handlesRef.current[id] = handle;
    } else {
      delete handlesRef.current[id];
    }
  }, []);

  const onProjectsSync = useCallback((projs) => {
    setProjects(projs);
  }, []);

  // Save As rekeys a project (old path → new path); keep the same mounted workspace
  const onRekey = useCallback(
    (oldId, newId) => {
      keyMapRef.current[newId] = keyMapRef.current[oldId] ?? oldId;
      delete keyMapRef.current[oldId];
      initialDataRef.current[newId] = initialDataRef.current[oldId];
      delete initialDataRef.current[oldId];
      const handle = handlesRef.current[oldId];
      if (handle) {
        handlesRef.current[newId] = handle;
        delete handlesRef.current[oldId];
      }
      setDirtyById((prev) => {
        const next = { ...prev };
        next[newId] = next[oldId];
        delete next[oldId];
        return next;
      });
      setActiveId((prev) => (prev === oldId ? newId : prev));
      refreshProjects();
    },
    [refreshProjects],
  );

  // Sync unsavedPendingRef alongside setUnsavedDialog for stale-closure-safe access
  const openUnsavedDialog = useCallback((pendingAction) => {
    setUnsavedDialog({ open: true, pendingAction });
    unsavedPendingRef.current = pendingAction;
  }, []);

  // Switching is lossless (workspaces stay mounted) — just update activeId
  const handleSwitchProject = useCallback((id) => {
    if (!id || id === activeIdRef.current) return;
    setActiveId(id);
    activateProject(id)
      .then((data) => {
        if (data && data.projects) setProjects(data.projects);
      })
      .catch((e) => console.error('activate project error:', e));
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
        if (peek && peek.records && peek.records.length > 1 && !splitPending) {
          splitPending = { path: p, records: peek.records };
          continue;
        }
      } catch {
        // Peek failure is not fatal — open normally below.
      }
      try {
        const data = await openFile(p);
        if (data && data.sequence) {
          // Hand the response to the new workspace so it doesn't refetch
          initialDataRef.current[p] = data;
          setRecentFiles(addRecentFile(p));
        } else {
          failed.push({ path: p, error: data?.error || 'Not a readable sequence file' });
        }
      } catch (e) {
        failed.push({ path: p, error: e?.message || String(e) });
      }
    }
    await refreshProjects();
    if (failed.length > 0) setOpenError(failed);
    if (splitPending) setFastaSplit(splitPending);
  }, [projects, refreshProjects, handleSwitchProject]);

  // FastaSplitDialog choice: 'split' opens every record as its own project
  // (project id `<path>#record-<i>`), 'first' opens record 0 only, null = cancel.
  const handleFastaSplitChoice = useCallback(
    async (choice) => {
      const target = fastaSplit;
      setFastaSplit(null);
      if (!target || !choice) return;
      const indices = choice === 'split' ? target.records.map((_, i) => i) : [0];
      const failed = [];
      setRecentFiles(addRecentFile(target.path));
      for (const i of indices) {
        try {
          const data = await openFile(target.path, i);
          if (data && data.sequence) {
            initialDataRef.current[`${target.path}#record-${i}`] = data;
          } else {
            failed.push({
              path: `${fileNameOf(target.path)} (record ${i + 1})`,
              error: data?.error || 'Not a readable sequence file',
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
    [fastaSplit, refreshProjects],
  );

  // Create an in-memory project from the New Sequence dialog; hand the
  // response to the new workspace (same pattern as handleOpenFile).
  const handleCreateProject = useCallback(
    async (payload) => {
      try {
        const data = await createProject(payload);
        if (data && data.sequence && data.id) {
          initialDataRef.current[data.id] = data;
          await refreshProjects();
          return { ok: true };
        }
        return { ok: false, error: data?.error || 'Failed to create project' };
      } catch (e) {
        return { ok: false, error: e?.message || String(e) };
      }
    },
    [refreshProjects],
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
        if (data && data.sequence) {
          initialDataRef.current[path] = data;
          setRecentFiles(addRecentFile(path));
        }
        await refreshProjects();
      } catch (e) {
        // File may have been moved/deleted — drop it from the list.
        console.error('open recent error:', e);
        setRecentFiles(removeRecentFile(path));
      }
    },
    [projects, refreshProjects, handleSwitchProject],
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
        if (data && data.sequence) {
          initialDataRef.current[path] = data;
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
    [refreshProjects, handleSwitchProject],
  );

  // Internal: actually perform the close (no dirty check)
  const doCloseProject = useCallback(
    async (id) => {
      try {
        const data = await deleteProject(id);
        if (data && data.error) return;

        delete initialDataRef.current[id];
        delete handlesRef.current[id];
        delete keyMapRef.current[id];
        setDirtyById((prev) => {
          const next = { ...prev };
          delete next[id];
          return next;
        });

        await refreshProjects();
      } catch (e) {
        console.error('close project error:', e);
      }
    },
    [refreshProjects],
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

  // Main window only: open files handed over by the OS. Listen for runtime
  // "file-opened" events and drain the backend queue once on mount (cold
  // start via Open With fires before the webview is ready).
  useEffect(() => {
    if (!isTauri || windowInfo?.type !== 'main') return;
    let cancelled = false;
    const listener = listenFileOpened((path) => {
      if (!cancelled) openExternalPath(path);
    });
    (async () => {
      try {
        const pending = await takePendingOpens();
        if (!cancelled && Array.isArray(pending)) {
          pending.forEach((p) => openExternalPath(p));
        }
      } catch {
        // backend unreachable; nothing to drain
      }
    })();
    return () => {
      cancelled = true;
      listener.close();
    };
  }, [windowInfo, openExternalPath]);

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
  }, [doCloseProject]);

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

  const handleOpenInNewWindow = useCallback(async (id) => {
    try {
      await openInNewWindow(id);
    } catch (e) {
      console.error('open in new window error:', e);
    }
  }, []);

  // Recent group is collapsible (default collapsed); already-open files are hidden from it
  const [recentOpen, setRecentOpen] = useState(false);
  useEffect(() => {
    if (!sidebarHover) setRecentOpen(false);
  }, [sidebarHover]);
  const visibleRecent = recentFiles.filter((p) => !projects.some((pr) => pr.id === p));

  // Agent tabs live in the main window; project windows render a single
  // workspace with no sidebar.
  const isProjectWindow = windowInfo?.type === 'project';
  // The active project is an agent tab, bound and locked by the MCP agent.
  // The project whose lock state drives this window: activeId in the main
  // window, the bound projectId in project windows.
  const lockTargetId = isProjectWindow ? windowInfo.projectId : activeId;
  const activeAgentLocked = !!lockTargetId && agentTabs[lockTargetId] === true;
  const activeAgentUnlocked = !!lockTargetId && agentTabs[lockTargetId] === false;

  // Agent-tab lock state is pushed from the backend (auto-relock on every MCP
  // tool call targeting the bound project).
  useEffect(() => {
    if (windowInfo?.type !== 'main') return undefined;
    const listener = listenAgentTabLock((payload) => {
      if (payload?.projectId) {
        // Ignore events for projects no longer in the list — writing them
        // would leave stale entries until the next project-list merge.
        setAgentTabs((prev) => {
          if (!(payload.projectId in prev)) return prev;
          if (prev[payload.projectId] === !!payload.locked) return prev;
          return { ...prev, [payload.projectId]: !!payload.locked };
        });
      }
    });
    return () => listener.close();
  }, [windowInfo?.type]);

  // Project windows get no project-list broadcasts, so resolve the bound
  // project's agent-tab state directly and keep it in sync via lock events.
  useEffect(() => {
    if (windowInfo?.type !== 'project') return undefined;
    const pid = windowInfo.projectId;
    let cancelled = false;
    getAgentTabState(pid)
      .then((st) => {
        if (!cancelled && st) setAgentTabs((prev) => ({ ...prev, [pid]: !!st.locked }));
      })
      .catch(() => {});
    const listener = listenAgentTabLock((payload) => {
      if (payload?.projectId === pid) {
        setAgentTabs((prev) => ({ ...prev, [pid]: !!payload.locked }));
      }
    });
    return () => {
      cancelled = true;
      listener.close();
    };
  }, [windowInfo]);

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

  const hasProject = isProjectWindow || projects.length > 0;
  // Sidebar buttons target the visible workspace: projectId in project windows,
  // activeId in the main window
  const sidebarTargetId = isProjectWindow ? windowInfo.projectId : activeId;
  // rna/protein projects hide DNA-only sidebar entries (ORFs, codon
  // optimization); DNA-only plugins are marked dnaOnly in the registry,
  // RNA-only plugins (RNA folding) are marked rnaOnly.
  const activeProject = projects.find((p) => p.id === activeId);
  const activeMoleculeType = activeProject?.moleculeType || 'dna';
  // While the active project's moleculeType is unknown (project list still
  // loading), hide type-gated entries instead of flashing DNA-only items.
  const moleculeTypeKnown = !activeId || !!activeProject;
  const isActiveDna = activeMoleculeType === 'dna';
  const isActiveRna = activeMoleculeType === 'rna';
  const visiblePlugins = plugins.filter(
    (plugin) =>
      !disabledPlugins.includes(plugin.id) &&
      (moleculeTypeKnown ? isActiveDna || !plugin.dnaOnly : !plugin.dnaOnly) &&
      (moleculeTypeKnown ? isActiveRna || !plugin.rnaOnly : !plugin.rnaOnly),
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
    [sidebarTargetId, openExternalPath, activeAgentLocked],
  );

  useEffect(() => {
    if (!isTauri) return;
    const listener = listenDragDrop(handleDroppedPaths);
    return () => listener.close();
  }, [handleDroppedPaths]);

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
  }, [dropConfirm, sidebarTargetId]);

  const handleDropOpenAsNew = useCallback(() => {
    const paths = dropConfirm?.paths || [];
    setDropConfirm(null);
    paths.forEach((p) => openExternalPath(p));
  }, [dropConfirm, openExternalPath]);

  const onToggleFeatures = useCallback(() => setShowFeatures((v) => !v), []);
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

  const workspaceProps = useMemo(
    () => ({
      backendStatus,
      methylationSystems,
      methylationOverlap,
      primerSeedLength,
      tmParams,
      layoutParams,
      showFeatures,
      onToggleFeatures,
      alwaysExpandFeatures,
      showPrimers,
      onTogglePrimers,
      featureLabelsBelow,
      showEnzymes,
      onToggleEnzymes,
      enzymeFilter,
      onEnzymeFilterChange,
      disabledPlugins,
      onDirtyChange,
      registerHandle,
      onProjectsSync,
      onRekey,
      myPrimers,
      onMyPrimersChange: setMyPrimers,
      myEnzymes,
      onMyEnzymesChange: setMyEnzymes,
      autoAddPrimers,
      onToggleAutoAddPrimers,
    }),
    [
      backendStatus,
      methylationSystems,
      methylationOverlap,
      primerSeedLength,
      tmParams,
      layoutParams,
      showFeatures,
      onToggleFeatures,
      alwaysExpandFeatures,
      showPrimers,
      onTogglePrimers,
      featureLabelsBelow,
      showEnzymes,
      onToggleEnzymes,
      enzymeFilter,
      onEnzymeFilterChange,
      disabledPlugins,
      onDirtyChange,
      registerHandle,
      onProjectsSync,
      onRekey,
      myPrimers,
      myEnzymes,
      autoAddPrimers,
      onToggleAutoAddPrimers,
    ],
  );

  const sidebarContent = (
    <Sidebar
      collapsible="icon"
      variant="sidebar"
      className={`${titleBarFallback ? '' : 'pt-10 '}transition-[width] duration-300 ease-out`}
    >
      <SidebarHeader className="flex flex-row items-center gap-2.5 overflow-hidden px-2.5 pb-2 pt-1.5">
        <div className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground shadow-sm">
          <div className="relative size-5">
            <svg viewBox="0 0 24 24" className="absolute inset-0 size-5">
              <circle
                cx="12"
                cy="12"
                r="9"
                fill="none"
                stroke="white"
                strokeOpacity="0.3"
                strokeWidth="2"
              />
            </svg>
            <LoaderCircle className="relative size-5 text-white" />
          </div>
        </div>
        <div className="flex min-w-0 flex-col whitespace-nowrap group-data-[state=collapsed]:hidden">
          <span className="text-[13px] font-semibold leading-tight tracking-tight text-foreground">
            LibreGene
          </span>
          <span className="text-[10px] leading-tight text-muted-foreground">Plasmid Editor</span>
        </div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton onClick={handleOpenFile} tooltip="Open File">
                  <FolderOpen className="size-4" />
                  <span>Open File…</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton onClick={() => setNewSeqOpen(true)} tooltip="New File">
                  <FilePlus2 className="size-4" />
                  <span>New File…</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
        {visibleRecent.length > 0 && (!windowInfo || windowInfo.type === 'main') && (
          <Collapsible open={recentOpen} onOpenChange={setRecentOpen}>
            <SidebarGroup className="pt-0">
              <CollapsibleTrigger asChild>
                <SidebarGroupLabel className="cursor-pointer select-none hover:text-sidebar-foreground">
                  <Clock className="mr-1.5 size-3" />
                  <span className="uppercase tracking-wider text-[10px] font-semibold">Recent</span>
                  <ChevronDown
                    className={`ml-auto size-3.5 shrink-0 transition-transform duration-200 ${recentOpen ? 'rotate-0' : '-rotate-90'}`}
                  />
                </SidebarGroupLabel>
              </CollapsibleTrigger>
              <CollapsibleContent>
                <SidebarGroupContent>
                  <SidebarMenu>
                    {visibleRecent.slice(0, 8).map((p) => {
                      const name = fileNameOf(p);
                      const Icon = getFileIcon(name);
                      return (
                        <SidebarMenuItem key={p}>
                          <SidebarMenuButton
                            onClick={() => handleOpenRecent(p)}
                            tooltip={p}
                            className="flex-1 min-w-0 pr-5"
                          >
                            <Icon className="size-4 shrink-0" />
                            <span className="truncate">{name}</span>
                          </SidebarMenuButton>
                          <button
                            type="button"
                            aria-label="Remove from recent"
                            onClick={(e) => {
                              e.stopPropagation();
                              handleRemoveRecent(p);
                            }}
                            title="Remove from recent"
                            className="absolute right-2 top-1/2 hidden -translate-y-1/2 size-4 items-center justify-center rounded text-muted-foreground hover:bg-sidebar-accent-foreground/10 hover:text-foreground group-hover/menu-item:flex"
                          >
                            <X className="size-3" />
                          </button>
                        </SidebarMenuItem>
                      );
                    })}
                  </SidebarMenu>
                </SidebarGroupContent>
              </CollapsibleContent>
            </SidebarGroup>
          </Collapsible>
        )}
        {projects.length > 0 && (
          <SidebarGroup className="pt-0">
            <SidebarGroupLabel>
              <span className="uppercase tracking-wider text-[10px] font-semibold">Opened</span>
              <span className="ml-1.5 rounded-full bg-sidebar-accent px-1.5 py-px text-[10px] font-medium tabular-nums text-sidebar-accent-foreground">
                {projects.length}
              </span>
            </SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {projects.map((p) => {
                  const isActiveProject = p.id === activeId;
                  const isProjectDirty = dirtyById[p.id] === true;
                  const name = fileName(p);
                  const Icon = getFileIcon(name);
                  return (
                    <SidebarMenuItem key={p.id}>
                      {isActiveProject && (
                        <span className="absolute left-0 top-1/2 h-4 w-0.5 -translate-y-1/2 rounded-full bg-primary" />
                      )}
                      <SidebarMenuButton
                        onClick={() => handleSwitchProject(p.id)}
                        isActive={isActiveProject}
                        tooltip={name}
                        className={`flex-1 min-w-0 ${
                          !windowInfo || windowInfo.type !== 'project' ? 'pr-12' : 'pr-6'
                        }`}
                      >
                        <Icon className="size-4 shrink-0" />
                        <span className="truncate">{name}</span>
                      </SidebarMenuButton>
                      {isProjectDirty && (
                        <span className="pointer-events-none absolute right-2.5 top-1/2 size-1.5 -translate-y-1/2 rounded-full bg-amber-500 group-hover/menu-item:hidden group-data-[state=collapsed]:hidden" />
                      )}
                      {!windowInfo || windowInfo.type !== 'project' ? (
                        <div className="absolute right-1 top-1/2 hidden -translate-y-1/2 items-center group-hover/menu-item:flex group-data-[state=collapsed]:hidden">
                          <button
                            className="flex size-5 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-sidebar-accent-foreground/10 hover:text-foreground"
                            onClick={(e) => {
                              e.stopPropagation();
                              handleOpenInNewWindow(p.id);
                            }}
                            title="Open in new window"
                          >
                            <ExternalLink className="size-3" />
                          </button>
                          <button
                            className="flex size-5 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-sidebar-accent-foreground/10 hover:text-foreground"
                            onClick={(e) => {
                              e.stopPropagation();
                              handleCloseProject(p.id);
                            }}
                            title="Close"
                          >
                            <X className="size-3" />
                          </button>
                        </div>
                      ) : (
                        <button
                          className="absolute right-1 top-1/2 hidden size-5 -translate-y-1/2 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-sidebar-accent-foreground/10 hover:text-foreground group-hover/menu-item:flex group-data-[state=collapsed]:hidden"
                          onClick={(e) => {
                            e.stopPropagation();
                            handleCloseProject(p.id);
                          }}
                          title="Close"
                        >
                          <X className="size-3" />
                        </button>
                      )}
                    </SidebarMenuItem>
                  );
                })}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        )}
        <SidebarGroup className="mt-auto">
          <SidebarGroupContent>
            <SidebarMenu>
              {visiblePlugins.flatMap((plugin) =>
                plugin.sidebarItems.map((item) => (
                  <SidebarMenuItem key={`${plugin.id}-${item.dialogKey}`}>
                    <SidebarMenuButton
                      onClick={() =>
                        handlesRef.current[sidebarTargetId]?.openPluginDialog(item.dialogKey)
                      }
                      tooltip={item.tooltip}
                      className="text-muted-foreground hover:text-foreground"
                    >
                      <item.icon className="size-4" />
                      <span>{item.label}</span>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                )),
              )}
              <SidebarMenuItem>
                <SidebarMenuButton
                  onClick={() => handlesRef.current[sidebarTargetId]?.openMapView()}
                  tooltip="Plasmid Map"
                  className="text-muted-foreground hover:text-foreground"
                >
                  <MapIcon className="size-4" />
                  <span>Map</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton
                  onClick={() => setMcpGuideOpen(true)}
                  tooltip="MCP Server (LLM agent)"
                  className="text-muted-foreground hover:text-foreground"
                >
                  <Bot className="size-4" />
                  <span>MCP Server</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton
                  onClick={() => openSettings('plugins')}
                  tooltip="Manage plugins"
                  className="text-muted-foreground hover:text-foreground"
                >
                  <Puzzle className="size-4" />
                  <span>Plugins</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton
                  onClick={() => openSettings()}
                  tooltip="Settings"
                  className="text-muted-foreground hover:text-foreground"
                >
                  <Settings className="size-4" />
                  <span>Settings</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
    </Sidebar>
  );

  return (
    <TooltipProvider>
      <SidebarProvider open={sidebarHover} style={{ '--sidebar-width': '14rem' }}>
        <div className="relative flex h-screen w-full flex-col overflow-hidden bg-background">
          <TitleBar title={activeTitle} dirty={activeDirty} onFallback={handleTitleBarFallback} />
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
              <span>Agent tab — unlocked</span>
              <Button
                variant="ghost"
                size="sm"
                className="h-6 gap-1 px-2 text-xs"
                onClick={() => handleSetAgentLocked(lockTargetId, true)}
              >
                <Lock className="size-3" />
                Lock
              </Button>
            </div>
          )}
          {/* Only show sidebar in main window */}
          {hasProject && !isProjectWindow && (
            <div
              className="absolute left-0 top-0 bottom-0 z-40"
              onMouseEnter={handleSidebarEnter}
              onMouseLeave={handleSidebarLeave}
            >
              {sidebarContent}
            </div>
          )}
          <div className="relative flex flex-1 min-h-0">
            {isProjectWindow ? (
              <ProjectWorkspace
                key={keyFor(windowInfo.projectId)}
                projectId={windowInfo.projectId}
                hidden={false}
                initialData={initialDataRef.current[windowInfo.projectId]}
                topology="circular"
                agentLocked={activeAgentLocked}
                {...workspaceProps}
              />
            ) : projects.length > 0 ? (
              projects.map((p) => (
                <ProjectWorkspace
                  key={keyFor(p.id)}
                  projectId={p.id}
                  hidden={p.id !== activeId}
                  initialData={initialDataRef.current[p.id]}
                  topology={p.topology || 'circular'}
                  moleculeType={p.moleculeType || 'dna'}
                  agentLocked={agentTabs[p.id] === true}
                  {...workspaceProps}
                />
              ))
            ) : (
              <main className="w-full flex-1 overflow-y-auto overscroll-contain hide-scrollbar">
                <div className="flex min-h-full flex-col items-center justify-center gap-5 p-6 text-center">
                  <div className="flex size-16 items-center justify-center">
                    <img src="/icon.png" alt="LibreGene" className="size-14" />
                  </div>
                  <div className="space-y-1.5">
                    <h1 className="text-lg font-semibold tracking-tight">LibreGene</h1>
                    <p className="max-w-sm text-sm leading-relaxed text-muted-foreground">
                      Open a sequence file to start viewing and editing your plasmid.
                    </p>
                  </div>
                  <div className="flex max-w-md flex-wrap items-center justify-center gap-1.5">
                    {[
                      '.gbk',
                      '.gbff',
                      '.dna',
                      '.fasta',
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
                      Open File
                    </Button>
                    <Button variant="outline" size="lg" onClick={() => setNewSeqOpen(true)}>
                      <FilePlus2 className="size-4" />
                      New Sequence
                    </Button>
                  </div>
                  <button
                    type="button"
                    onClick={() => setMcpGuideOpen(true)}
                    className="flex items-center gap-1.5 text-xs text-muted-foreground transition-colors hover:text-foreground"
                  >
                    <Bot className="size-3.5" />
                    Connect an LLM agent via MCP
                  </button>
                </div>
              </main>
            )}
          </div>
        </div>

        <SettingsPage
          open={settingsOpen}
          onOpenChange={setSettingsOpen}
          focusSection={settingsFocus}
          alwaysExpandFeatures={alwaysExpandFeatures}
          onToggleAlwaysExpandFeatures={onToggleAlwaysExpandFeatures}
          featureLabelsBelow={featureLabelsBelow}
          onFeatureLabelsBelowChange={onFeatureLabelsBelowChange}
          methylationSystems={methylationSystems}
          setMethylationSystems={setMethylationSystems}
          methylationOverlap={methylationOverlap}
          setMethylationOverlap={setMethylationOverlap}
          primerSeedLength={primerSeedLength}
          setPrimerSeedLength={setPrimerSeedLength}
          tmParams={tmParams}
          setTmParams={setTmParams}
          plugins={plugins}
          disabledPlugins={disabledPlugins}
          onTogglePlugin={handleTogglePlugin}
        />

        <McpGuideDialog
          open={mcpGuideOpen}
          onOpenChange={setMcpGuideOpen}
          mcpConfig={mcpConfig}
          onMcpConfigChange={handleMcpConfigChange}
        />

        <NewSequenceDialog
          open={newSeqOpen}
          onOpenChange={setNewSeqOpen}
          onConfirm={handleCreateProject}
        />

        {/* --- Unsaved Changes Dialog --- */}
        <Dialog
          open={unsavedDialog.open}
          onOpenChange={(open) => {
            if (!open) handleUnsavedCancel();
          }}
        >
          <DialogContent className="sm:max-w-md" onInteractOutside={(e) => e.preventDefault()}>
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2.5">
                <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-amber-100 text-amber-600">
                  <AlertTriangle className="size-4" />
                </span>
                Unsaved Changes
              </DialogTitle>
              <DialogDescription>
                This project has unsaved changes. Save before continuing?
              </DialogDescription>
            </DialogHeader>
            <DialogFooter className="gap-2 sm:gap-2">
              <DialogClose asChild>
                <Button variant="outline" onClick={handleUnsavedCancel}>
                  Cancel
                </Button>
              </DialogClose>
              <Button
                variant="outline"
                className="text-destructive hover:text-destructive"
                onClick={handleUnsavedDiscard}
              >
                Don't Save
              </Button>
              <Button onClick={handleUnsavedSave}>Save</Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        {/* --- Drag-drop: add as alignment or open as new file --- */}
        <Dialog
          open={!!dropConfirm}
          onOpenChange={(open) => {
            if (!open) setDropConfirm(null);
          }}
        >
          <DialogContent className="sm:max-w-md" onInteractOutside={(e) => e.preventDefault()}>
            <DialogHeader>
              <DialogTitle>Add as Alignment?</DialogTitle>
              <DialogDescription>
                Add {dropConfirm?.paths.length === 1 ? 'this file' : 'these files'} to the current
                project as {dropConfirm?.paths.length === 1 ? 'an alignment' : 'alignments'}, or
                open as new {dropConfirm?.paths.length === 1 ? 'project' : 'projects'}?
              </DialogDescription>
            </DialogHeader>
            <ul className="max-h-40 overflow-auto rounded-md border border-border/60 px-3 py-2 text-xs font-mono text-muted-foreground">
              {(dropConfirm?.paths || []).map((p) => (
                <li key={p} className="truncate py-0.5" title={p}>
                  {fileNameOf(p)}
                </li>
              ))}
            </ul>
            <DialogFooter className="gap-2 sm:gap-2">
              <DialogClose asChild>
                <Button variant="outline">Cancel</Button>
              </DialogClose>
              <Button variant="outline" onClick={handleDropOpenAsNew}>
                Open as New File
              </Button>
              <Button onClick={handleDropAddAsAlignment}>Add as Alignment</Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        {/* --- Drag-drop: per-file failure feedback --- */}
        <Dialog
          open={!!dropResult}
          onOpenChange={(open) => {
            if (!open) setDropResult(null);
          }}
        >
          <DialogContent className="sm:max-w-md">
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2.5">
                <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-amber-100 text-amber-600">
                  <AlertTriangle className="size-4" />
                </span>
                Some Alignments Failed
              </DialogTitle>
              <DialogDescription>
                {dropResult?.added > 0
                  ? `${dropResult.added} added, ${dropResult.failed.length} failed:`
                  : 'No alignments could be added:'}
              </DialogDescription>
            </DialogHeader>
            <ul className="max-h-48 overflow-auto rounded-md border border-border/60 px-3 py-2 text-xs text-muted-foreground">
              {(dropResult?.failed || []).map((f) => (
                <li key={f.path} className="py-1">
                  <span className="font-mono">{fileNameOf(f.path)}</span>
                  <span className="block text-destructive/80">{f.error}</span>
                </li>
              ))}
            </ul>
            <DialogFooter>
              <DialogClose asChild>
                <Button>OK</Button>
              </DialogClose>
            </DialogFooter>
          </DialogContent>
        </Dialog>
        {/* --- Open File: per-file failure feedback --- */}
        <Dialog
          open={!!openError}
          onOpenChange={(open) => {
            if (!open) setOpenError(null);
          }}
        >
          <DialogContent className="sm:max-w-md">
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2.5">
                <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-amber-100 text-amber-600">
                  <AlertTriangle className="size-4" />
                </span>
                {openError?.length > 1 ? 'Some Files Failed to Open' : 'Failed to Open File'}
              </DialogTitle>
            </DialogHeader>
            <ul className="max-h-48 overflow-auto rounded-md border border-border/60 px-3 py-2 text-xs text-muted-foreground">
              {(openError || []).map((f) => (
                <li key={f.path} className="py-1">
                  <span className="font-mono">{fileNameOf(f.path)}</span>
                  <span className="block text-destructive/80">{f.error}</span>
                </li>
              ))}
            </ul>
            <DialogFooter>
              <DialogClose asChild>
                <Button>OK</Button>
              </DialogClose>
            </DialogFooter>
          </DialogContent>
        </Dialog>
        {/* --- Multi-record FASTA: split into separate projects? --- */}
        <FastaSplitDialog target={fastaSplit} onChoice={handleFastaSplitChoice} />
        {/* --- Tray Quit: unsaved-changes confirmation --- */}
        <Dialog
          open={!!quitRequest}
          onOpenChange={(open) => {
            if (!open) setQuitRequest(null);
          }}
        >
          <DialogContent className="sm:max-w-md">
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2.5">
                <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-amber-100 text-amber-600">
                  <AlertTriangle className="size-4" />
                </span>
                Quit LibreGene?
              </DialogTitle>
              <DialogDescription>Unsaved changes will be lost:</DialogDescription>
            </DialogHeader>
            <ul className="max-h-48 overflow-auto rounded-md border border-border/60 px-3 py-2 text-xs text-muted-foreground">
              {(quitRequest || []).map((id) => (
                <li key={id} className="py-1 font-mono">
                  {fileNameOf(id)}
                </li>
              ))}
            </ul>
            <DialogFooter>
              <DialogClose asChild>
                <Button variant="outline">Cancel</Button>
              </DialogClose>
              <Button variant="destructive" onClick={() => forceQuit().catch(() => {})}>
                Quit Anyway
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </SidebarProvider>
      <ContextMenuHost />
    </TooltipProvider>
  );
}
