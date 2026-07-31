import { useState, useEffect, useRef, useCallback } from 'react';
import {
  openFile,
  isTauri,
  openFileDialog,
  listenProjectUpdates,
  getProjects,
  activateProject,
  getWindowProjectId,
  openInNewWindow,
  deleteProject,
  setWindowTitle,
} from './tauriApi';
import { plugins } from './plugins';
import ProjectWorkspace from './ProjectWorkspace';
import SettingsPage from './components/SettingsPage';
import TitleBar from './components/TitleBar';
import {
  SidebarProvider,
  Sidebar,
  SidebarContent,
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
  ChevronDown,
  AlertTriangle,
  X,
  ExternalLink,
  Settings,
  ArrowDownWideNarrow,
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

export default function App() {
  const [disabledPlugins, setDisabledPlugins] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('disabledPlugins')) || [];
    } catch {
      return [];
    }
  });
  const [recentFiles, setRecentFiles] = useState(() => getRecentFiles());
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
  const [backendStatus, setBackendStatus] = useState(isTauri ? 'online' : 'offline');

  // Multi-project state
  const [projects, setProjects] = useState([]);
  const [activeId, setActiveId] = useState(null);

  // Multi-window: tracks whether this window is "main" or a "project" window
  const [windowInfo, setWindowInfo] = useState(null);
  // { type: 'main' } or { type: 'project', projectId: '...' }

  const [settingsOpen, setSettingsOpen] = useState(false);
  const [showFeatures, setShowFeatures] = useState(true);
  const [alwaysExpandFeatures, setAlwaysExpandFeatures] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('alwaysExpandFeatures')) || false;
    } catch {
      return false;
    }
  });
  const [showPrimers, setShowPrimers] = useState(true);
  const [showEnzymes, setShowEnzymes] = useState(true);
  const [enzymeFilter, setEnzymeFilter] = useState('unique+twice');
  const [methylationSystems, setMethylationSystems] = useState(['dam', 'dcm', 'ecoki']);
  const [methylationOverlap, setMethylationOverlap] = useState(2);
  const [primerSeedLength, setPrimerSeedLength] = useState(10);
  const [tmParams, setTmParams] = useState({
    naConc: 0.05,
    mgConc: 0.0015,
    dntpConc: 0.0008,
    trisConc: 0.01,
    primerConc: 2e-7,
  });
  const [openPath] = useState('');
  const activeIdRef = useRef(null);
  // Per-project dirty state reported by workspaces: { [projectId]: bool }
  const [dirtyById, setDirtyById] = useState({});
  const [unsavedDialog, setUnsavedDialog] = useState({ open: false, pendingAction: null });
  const unsavedPendingRef = useRef(null);
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
    } catch {
      // backend unreachable; keep current project list
    }
  }, []);

  // Detect window type on mount
  useEffect(() => {
    if (!isTauri) {
      setWindowInfo({ type: 'main' });
      return;
    }
    getWindowProjectId().then((pid) => {
      setWindowInfo(pid ? { type: 'project', projectId: pid } : { type: 'main' });
    });
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
  }, [windowInfo, refreshProjects]);

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

  // Title: filename of the active project, or the app name
  const activeTitle = activeId
    ? activeId.split('/').pop().split('\\').pop()
    : windowInfo?.type === 'project'
      ? windowInfo.projectId.split('/').pop().split('\\').pop()
      : 'LibreGene';
  const titleId = windowInfo?.type === 'project' ? windowInfo.projectId : activeId;
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

  const handleOpenFile = useCallback(async () => {
    let paths;
    if (isTauri) {
      // Start the native dialog in the directory of the last opened file,
      // so the user doesn't have to navigate from scratch each time.
      const lastDir = dirOf(getLastOpenedFile());
      paths = await openFileDialog(lastDir || undefined);
      if (!paths || !paths.length) return;
    } else {
      paths = [openPath];
    }
    try {
      for (const p of paths) {
        // Reopening an already-open path would replace the project and lose unsaved edits
        if (projects.some((pr) => pr.id === p)) continue;
        const data = await openFile(p);
        if (data && data.sequence) {
          // Hand the response to the new workspace so it doesn't refetch
          initialDataRef.current[p] = data;
          setRecentFiles(addRecentFile(p));
        }
      }
      await refreshProjects();
    } catch (e) {
      console.error('open file error:', e);
    }
  }, [openPath, projects, refreshProjects]);

  // --- Global shortcut: Cmd/Ctrl+O opens a file ---
  useEffect(() => {
    const handler = (e) => {
      const isCtrl = e.ctrlKey || e.metaKey;
      if (!isCtrl) return;
      const tag = e.target?.tagName?.toLowerCase();
      if (tag === 'input' || tag === 'textarea' || e.target?.isContentEditable) return;
      if (e.key === 'o') {
        e.preventDefault();
        handleOpenFile();
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [handleOpenFile]);

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

  // Extract filename from path
  const fileName = (p) => {
    const s = (p.name || p.id || '').replace(/\\/g, '/');
    return s.split('/').pop() || s;
  };

  // Recent group is collapsible (default collapsed); already-open files are hidden from it
  const [recentOpen, setRecentOpen] = useState(false);
  const visibleRecent = recentFiles.filter((p) => !projects.some((pr) => pr.id === p));

  const isProjectWindow = windowInfo?.type === 'project';
  const hasProject = isProjectWindow || projects.length > 0;
  // Sidebar buttons target the visible workspace: projectId in project windows,
  // activeId in the main window
  const sidebarTargetId = isProjectWindow ? windowInfo.projectId : activeId;

  const workspaceProps = {
    onOpenFile: handleOpenFile,
    backendStatus,
    methylationSystems,
    methylationOverlap,
    primerSeedLength,
    tmParams,
    layoutParams,
    showFeatures,
    onToggleFeatures: () => setShowFeatures((v) => !v),
    alwaysExpandFeatures,
    onToggleAlwaysExpandFeatures: () =>
      setAlwaysExpandFeatures((v) => {
        const next = !v;
        try {
          localStorage.setItem('alwaysExpandFeatures', JSON.stringify(next));
        } catch {
          // storage may be unavailable; toggle still applies in-memory
        }
        return next;
      }),
    showPrimers,
    onTogglePrimers: () => setShowPrimers((v) => !v),
    showEnzymes,
    onToggleEnzymes: () => setShowEnzymes((v) => !v),
    enzymeFilter,
    onEnzymeFilterChange: setEnzymeFilter,
    disabledPlugins,
    onDirtyChange,
    registerHandle,
    onProjectsSync,
    onRekey,
  };

  const sidebarContent = (
    <Sidebar
      collapsible="icon"
      variant="sidebar"
      className="pt-10 transition-[width] duration-300 ease-out"
    >
      <SidebarHeader className="flex flex-row items-center gap-2.5 px-3 pb-2 pt-1.5 group-data-[state=collapsed]:justify-center group-data-[state=collapsed]:px-0">
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
        <div className="flex min-w-0 flex-col group-data-[state=collapsed]:hidden">
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
                <SidebarMenuButton
                  onClick={handleOpenFile}
                  tooltip="Open File"
                  className="border border-dashed border-sidebar-border text-muted-foreground hover:border-primary/40 hover:text-primary"
                >
                  <FolderOpen className="size-4" />
                  <span>Open File…</span>
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
              {plugins
                .filter((plugin) => !disabledPlugins.includes(plugin.id))
                .flatMap((plugin) =>
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
                  <span>Plasmid Map</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton
                  onClick={() => handlesRef.current[sidebarTargetId]?.openPrimerOverview()}
                  tooltip="Primer Overview"
                  className="text-muted-foreground hover:text-foreground"
                >
                  <ArrowDownWideNarrow className="size-4" />
                  <span>Primer Overview</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton
                  onClick={() => setSettingsOpen(true)}
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
          <TitleBar title={activeTitle} dirty={activeDirty} />
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
                  <div className="flex items-center gap-1.5">
                    {['.gbk', '.dna', '.fasta', '.ab1'].map((ext) => (
                      <span
                        key={ext}
                        className="rounded-md border border-border bg-muted px-2 py-0.5 font-mono text-[11px] text-muted-foreground"
                      >
                        {ext}
                      </span>
                    ))}
                  </div>
                  <Button onClick={handleOpenFile} size="lg" className="mt-1">
                    <FolderOpen className="size-4" />
                    Open File
                  </Button>
                </div>
              </main>
            )}
          </div>
        </div>

        <SettingsPage
          open={settingsOpen}
          onOpenChange={setSettingsOpen}
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
      </SidebarProvider>
    </TooltipProvider>
  );
}
