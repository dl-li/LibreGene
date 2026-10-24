import { useState, useEffect, useRef, useCallback } from 'react';
import { createEditHistory } from '../editHistory';
import { updateSequence } from '../tauriApi';
import { EMPTY_ARRAY } from './constants';
import { createProjectSnapshot, snapshotIsDirty } from './projectSnapshot';

export default function useUndoHistory({
  initialData,
  agentLockedRef,
  operationGenRef,
  projectId,
  onProjectsSync,
  sequence,
  features,
  primers,
  textColorsTop,
  textColorsBottom,
  setSequence,
  setFeatures,
  setEnzymes,
  setPrimers,
  setAlignments,
  setTextColorsTop,
  setTextColorsBottom,
}) {
  // --- Sequence editing state ---
  const editHistoryRef = useRef(createEditHistory());
  const [historyVersion, setHistoryVersion] = useState(0);
  useEffect(() => editHistoryRef.current.subscribe(() => setHistoryVersion((v) => v + 1)), []);
  const canUndo = historyVersion >= 0 && editHistoryRef.current.canUndo();
  const canRedo = historyVersion >= 0 && editHistoryRef.current.canRedo();
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

  // Record a post-mutation snapshot in undo history. Entries are always the
  // state AFTER a mutation (sequence edits follow the same convention), so
  // undo returns the previous entry — the pre-mutation state — and redo
  // restores the mutation. Snapshots always carry primers so undo/redo can
  // restore them on both sides (UI state + backend via update_sequence).
  const pushHistory = useCallback(
    (overrides = {}) => {
      editHistoryRef.current.push({
        sequence,
        features: features || EMPTY_ARRAY,
        primers: primers || EMPTY_ARRAY,
        textColorsTop: textColorsTop || EMPTY_ARRAY,
        textColorsBottom: textColorsBottom || EMPTY_ARRAY,
        cursorIndex: null,
        selStart: null,
        selEnd: null,
        ...overrides,
      });
    },
    [sequence, features, primers, textColorsTop, textColorsBottom],
  );

  const restoreHistorySnapshot = useCallback(
    async (snapshot, gen, commit, operation) => {
      try {
        const data = await updateSequence(
          snapshot.sequence,
          snapshot.features,
          snapshot.primers,
          snapshot.textColorsTop,
          snapshot.textColorsBottom,
        );
        if (operationGenRef.current !== gen) return;
        if (!data || data.error) {
          console.error(`${operation} error:`, data?.error || 'unknown');
          return;
        }
        if (!commit()) return;
        setSequence(data.sequence);
        setFeatures(data.features || snapshot.features || EMPTY_ARRAY);
        setEnzymes(data.enzymes || EMPTY_ARRAY);
        setPrimers(data.primers || snapshot.primers || EMPTY_ARRAY);
        setAlignments(data.alignments || EMPTY_ARRAY);
        setTextColorsTop(data.textColorsTop || snapshot.textColorsTop || EMPTY_ARRAY);
        setTextColorsBottom(data.textColorsBottom || snapshot.textColorsBottom || EMPTY_ARRAY);
        setRestoreState({
          version: ++undoVersionRef.current,
          cursorIndex: snapshot.cursorIndex,
          selStart: snapshot.selStart,
          selEnd: snapshot.selEnd,
        });
        if (data.projects) onProjectsSync(data.projects);
        setIsDirty(snapshotIsDirty(snapshot, baselineSnapshotRef.current, projectId, data.dirty));
      } catch (error) {
        console.error(`${operation} error:`, error);
      }
    },
    [onProjectsSync, projectId],
  );

  const handleUndo = useCallback(async () => {
    if (agentLockedRef.current) return;
    const snapshot = editHistoryRef.current.peekUndo();
    if (!snapshot) return;
    const gen = ++operationGenRef.current;
    await restoreHistorySnapshot(snapshot, gen, () => editHistoryRef.current.commitUndo(), 'undo');
  }, [restoreHistorySnapshot]);

  const handleRedo = useCallback(async () => {
    if (agentLockedRef.current) return;
    const snapshot = editHistoryRef.current.peekRedo();
    if (!snapshot) return;
    const gen = ++operationGenRef.current;
    await restoreHistorySnapshot(snapshot, gen, () => editHistoryRef.current.commitRedo(), 'redo');
  }, [restoreHistorySnapshot]);

  return {
    editHistoryRef,
    canUndo,
    canRedo,
    restoreState,
    setRestoreState,
    undoVersionRef,
    isDirty,
    setIsDirty,
    isDirtyRef,
    baselineSnapshotRef,
    pushHistory,
    handleUndo,
    handleRedo,
  };
}
