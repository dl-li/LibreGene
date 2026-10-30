import { useCallback, useEffect, useState } from 'react';
import { getSessionEditLog, listenSessionEditLog, rollbackSessionEdit } from '../tauriApi';

export default function useSessionEditLog(projectId) {
  const [entries, setEntries] = useState([]);

  useEffect(() => {
    let cancelled = false;
    setEntries([]);
    const listener = listenSessionEditLog((message) => {
      if (!cancelled && message.projectId === projectId) {
        setEntries(message.entries || []);
      }
    });
    getSessionEditLog(projectId)
      .then((result) => {
        if (!cancelled && result && !result.error) setEntries(result.entries || []);
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
      return result;
    },
    [projectId],
  );

  return { entries, rollback };
}
