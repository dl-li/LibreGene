import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { getProjects, activateProject, deleteProject } from '../tauriApi';

export default function useProjects() {
  // Multi-project state
  const [projects, setProjects] = useState([]);
  const [activeId, setActiveId] = useState(null);
  const [projectOrder, setProjectOrder] = useState(() => {
    try {
      return JSON.parse(localStorage.getItem('projectOrder')) || [];
    } catch {
      return [];
    }
  });
  const orderedProjects = useMemo(() => {
    if (projectOrder.length === 0) return projects;
    const byId = new Map(projects.map((p) => [p.id, p]));
    const ordered = [];
    for (const id of projectOrder) {
      const p = byId.get(id);
      if (p) {
        ordered.push(p);
        byId.delete(id);
      }
    }
    for (const p of projects) {
      if (byId.has(p.id)) ordered.push(p);
    }
    return ordered;
  }, [projects, projectOrder]);
  const orderedProjectsRef = useRef(orderedProjects);
  useEffect(() => {
    orderedProjectsRef.current = orderedProjects;
  }, [orderedProjects]);
  // Agent tabs live in the main window; their lock state lives in agentTabs.
  const [agentTabs, setAgentTabs] = useState({}); // { [projectId]: locked }
  const activeIdRef = useRef(null);
  // Per-project dirty state reported by workspaces: { [projectId]: bool }
  const [dirtyById, setDirtyById] = useState({});
  const handlesRef = useRef({}); // { [projectId]: workspace imperative handle }
  const initialDataRef = useRef({}); // { [projectId]: openFile response } — consumed on workspace mount
  const keyMapRef = useRef({}); // { [projectId]: stable React key } — survives Save As rekey

  const keyFor = useCallback((id) => {
    if (!keyMapRef.current[id]) keyMapRef.current[id] = id;
    return keyMapRef.current[id];
  }, []);

  useEffect(() => {
    activeIdRef.current = activeId;
  }, [activeId]);

  const projectsRef = useRef([]);
  useEffect(() => {
    projectsRef.current = projects;
  }, [projects]);

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

  return {
    projects,
    setProjects,
    activeId,
    setActiveId,
    projectOrder,
    setProjectOrder,
    orderedProjects,
    orderedProjectsRef,
    agentTabs,
    setAgentTabs,
    dirtyById,
    handlesRef,
    initialDataRef,
    keyMapRef,
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
  };
}
