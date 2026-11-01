import { useEffect } from 'react';
import {
  isTauri,
  listenProjectUpdates,
  listenQuitRequested,
  listenFileOpened,
  takePendingOpens,
  listenAgentTabLock,
  listenDragDrop,
} from '../tauriApi';

export default function useTauriEvents({
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
    let cancelled = false;

    setBackendStatus(isTauri ? 'online' : 'connecting');
    (async () => {
      try {
        await refreshProjects();
        if (!cancelled) setBackendStatus('online');
      } catch {
        if (!cancelled) setBackendStatus(isTauri ? 'online' : 'offline');
      }
    })();
    const listener = listenProjectUpdates((msg) => {
      if (cancelled) return;
      if (msg.projects) {
        setProjects(msg.projects);
        mergeAgentTabs(msg.projects);
      }
      if (msg.activeId !== undefined) {
        setActiveId(msg.activeId);
      }
    });

    return () => {
      cancelled = true;
      listener.close();
    };
  }, [refreshProjects, mergeAgentTabs]);

  // Tray Quit with unsaved changes: backend shows this window and emits
  // quit-requested (payload = dirty project ids); confirm here before force-quit.
  useEffect(() => {
    if (!isTauri) return;
    const listener = listenQuitRequested((dirtyIds) => {
      setQuitRequest(dirtyIds);
    });
    return () => listener.close();
  }, []);

  // Main window only: open files handed over by the OS. Listen for runtime
  // "file-opened" events and drain the backend queue once on mount (cold
  // start via Open With fires before the webview is ready).
  useEffect(() => {
    if (!isTauri) return;
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
  }, [openExternalPath]);

  // Agent-tab lock state is pushed from the backend (auto-relock on every MCP
  // tool call targeting the bound project).
  useEffect(() => {
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
  }, []);

  useEffect(() => {
    if (!isTauri) return;
    const listener = listenDragDrop(handleDroppedPaths);
    return () => listener.close();
  }, [handleDroppedPaths]);
}
