import { useCallback, useEffect, useState } from 'react';
import { getSessionEditLog, listenSessionEditLog, rollbackSessionEdit } from '../tauriApi';

export default function useSessionEditLog(projectId) {
  const [entries, setEntries] = useState([]);
  const [canUndo, setCanUndo] = useState(false);
  const [canRedo, setCanRedo] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setEntries([]);
    setCanUndo(false);
    setCanRedo(false);
    const applyHistory = (message) => {
      setEntries(message.entries || []);
      setCanUndo(message.canUndo === true);
      setCanRedo(message.canRedo === true);
    };
    const listener = listenSessionEditLog((message) => {
      if (!cancelled && message.projectId === projectId) applyHistory(message);
    });
    getSessionEditLog(projectId)
      .then((result) => {
        if (!cancelled && result && !result.error) applyHistory(result);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      listener.close();
    };
  }, [projectId]);

  const rollback = useCallback(
    async (entryId) => {
      const result = await rollbackSessionEdit(projectId, entryId);
      if (!result || result.error) throw new Error(result?.error || 'Rollback failed');
      setEntries(result.entries || []);
      setCanUndo((result.entries || []).length > 0);
      setCanRedo(false);
      return result;
    },
    [projectId],
  );

  return { entries, canUndo, canRedo, rollback };
}
