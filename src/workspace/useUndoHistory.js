import { useState, useRef, useCallback } from 'react';
import { redoProjectEdit, undoProjectEdit } from '../tauriApi';
import { createProjectSnapshot } from './projectSnapshot';
import { applyProjectMutation } from './projectMutation';

export default function useUndoHistory({
  initialData,
  agentLockedRef,
  operationGenRef,
  projectId,
  latestRevisionRef,
  backendCanUndo,
  backendCanRedo,
  onError,
  setSequence,
  setFeatures,
  setEnzymes,
  setPrimers,
  setAlignments,
  setTextColorsTop,
  setTextColorsBottom,
  onProjectsSync,
}) {
  const [restoreState, setRestoreState] = useState({
    version: 0,
    cursorIndex: null,
    selStart: null,
    selEnd: null,
    translationSel: null,
  });
  const undoVersionRef = useRef(0);
  const [isDirty, setIsDirty] = useState(initialData?.dirty === true);
  const isDirtyRef = useRef(false);
  const baselineSnapshotRef = useRef(createProjectSnapshot(initialData));

  const restoreHistory = useCallback(
    async (direction) => {
      if (agentLockedRef.current) return;
      if (direction === 'undo' ? !backendCanUndo : !backendCanRedo) return;
      const gen = ++operationGenRef.current;
      try {
        const command = direction === 'undo' ? undoProjectEdit : redoProjectEdit;
        const data = await command(projectId);
        if (operationGenRef.current !== gen) return;
        if (!data || data.error) {
          onError(`${direction} failed`, data?.error || 'Unknown error');
          return;
        }
        applyProjectMutation(data, {
          latestRevisionRef,
          setSequence,
          setFeatures,
          setEnzymes,
          setPrimers,
          setAlignments,
          setTextColorsTop,
          setTextColorsBottom,
          setIsDirty,
          onProjectsSync,
        });
        setRestoreState({
          version: ++undoVersionRef.current,
          cursorIndex: null,
          selStart: null,
          selEnd: null,
          translationSel: null,
        });
      } catch (error) {
        onError(`${direction} failed`, error);
      }
    },
    [
      agentLockedRef,
      backendCanUndo,
      backendCanRedo,
      operationGenRef,
      projectId,
      onError,
      latestRevisionRef,
      setSequence,
      setFeatures,
      setEnzymes,
      setPrimers,
      setAlignments,
      setTextColorsTop,
      setTextColorsBottom,
      onProjectsSync,
    ],
  );

  const handleUndo = useCallback(() => restoreHistory('undo'), [restoreHistory]);
  const handleRedo = useCallback(() => restoreHistory('redo'), [restoreHistory]);

  return {
    canUndo: backendCanUndo,
    canRedo: backendCanRedo,
    restoreState,
    setRestoreState,
    undoVersionRef,
    isDirty,
    setIsDirty,
    isDirtyRef,
    baselineSnapshotRef,
    handleUndo,
    handleRedo,
  };
}
