import { useEffect } from 'react';
import {
  isTauri,
  listenProjectUpdates,
  listenQuitRequested,
  listenFileOpened,
  takePendingOpens,
  getAgentTabState,
  listenAgentTabLock,
  listenDragDrop,
} from '../tauriApi';

export default function useTauriEvents({
  windowInfo,
  setBackendStatus,
  setProjects,
  setActiveId,
  mergeAgentTabs,
  refreshProjects,
  setQuitRequest,
  openExternalPath,
  setAgentTabs,
  handleDroppedPaths,
}) {
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

  // Tray Quit with unsaved changes: backend shows this window and emits
  // quit-requested (payload = dirty project ids); confirm here before force-quit.
  useEffect(() => {
    if (!isTauri || !windowInfo || windowInfo.type !== 'main') return;
    const listener = listenQuitRequested((dirtyIds) => {
      setQuitRequest(dirtyIds);
    });
    return () => listener.close();
  }, [windowInfo]);

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

  useEffect(() => {
    if (!isTauri) return;
    const listener = listenDragDrop(handleDroppedPaths);
    return () => listener.close();
  }, [handleDroppedPaths]);
}
