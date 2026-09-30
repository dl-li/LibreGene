import { useState, useEffect, useRef, useCallback } from 'react';
import { createEditHistory } from '../editHistory';
import { getProject, updateSequence } from '../tauriApi';
import { EMPTY_ARRAY } from './constants';

export default function useUndoHistory({
  initialData,
  agentLockedRef,
  operationGenRef,
  projectId,
  onProjectsSync,
  sequence,
  features,
  primers,
  setSequence,
  setFeatures,
  setEnzymes,
  setPrimers,
  setAlignments,
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
  const baselineSequenceRef = useRef(initialData?.sequence ?? '');

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
        cursorIndex: null,
        selStart: null,
        selEnd: null,
        ...overrides,
      });
    },
    [sequence, features, primers],
  );

  // --- Undo ---
  const handleUndo = useCallback(async () => {
    if (agentLockedRef.current) return;
    const gen = ++operationGenRef.current;
    const snapshot = editHistoryRef.current.undo();
    if (!snapshot) return;

    // Restore cursor/selection in SequenceEditor (use ref for atomic version)
    setRestoreState({
      version: ++undoVersionRef.current,
      cursorIndex: snapshot.cursorIndex,
      selStart: snapshot.selStart,
      selEnd: snapshot.selEnd,
    });

    // Send to backend for recomputation
    try {
      const data = await updateSequence(snapshot.sequence, snapshot.features, snapshot.primers);
      if (operationGenRef.current !== gen) return;
      if (data && !data.error) {
        setSequence(data.sequence);
        setFeatures(data.features || snapshot.features || EMPTY_ARRAY);
        setEnzymes(data.enzymes || EMPTY_ARRAY);
        // Backend response carries freshly recomputed binding sites
        setPrimers(data.primers || snapshot.primers || EMPTY_ARRAY);
        setAlignments(data.alignments || EMPTY_ARRAY);
        if (data.projects) onProjectsSync(data.projects);
        // Accurate dirty check: undo to saved state = not dirty. A
        // never-saved `untitled-*` project has no on-disk state to undo back
        // to, so it stays dirty regardless.
        setIsDirty(
          projectId.startsWith('untitled-') || snapshot.sequence !== baselineSequenceRef.current,
        );
      } else {
        // Re-fetch to recover
        try {
          const refresh = await getProject('all');
          if (refresh && !refresh.error && refresh.sequence) {
            setSequence(refresh.sequence);
            setFeatures(refresh.features || EMPTY_ARRAY);
            setEnzymes(refresh.enzymes || EMPTY_ARRAY);
            setPrimers(refresh.primers || EMPTY_ARRAY);
            setAlignments(refresh.alignments || EMPTY_ARRAY);
          }
        } catch {
          // recovery fetch failed; state left as-is
        }
      }
    } catch (e) {
      console.error('undo error:', e);
    }
  }, [onProjectsSync, projectId]);

  // --- Redo ---
  const handleRedo = useCallback(async () => {
    if (agentLockedRef.current) return;
    const gen = ++operationGenRef.current;
    const snapshot = editHistoryRef.current.redo();
    if (!snapshot) return;

    setRestoreState({
      version: ++undoVersionRef.current,
      cursorIndex: snapshot.cursorIndex,
      selStart: snapshot.selStart,
      selEnd: snapshot.selEnd,
    });

    try {
      const data = await updateSequence(snapshot.sequence, snapshot.features, snapshot.primers);
      if (operationGenRef.current !== gen) return;
      if (data && !data.error) {
        setSequence(data.sequence);
        setFeatures(data.features || snapshot.features || EMPTY_ARRAY);
        setEnzymes(data.enzymes || EMPTY_ARRAY);
        setPrimers(data.primers || snapshot.primers || EMPTY_ARRAY);
        setAlignments(data.alignments || EMPTY_ARRAY);
        if (data.projects) onProjectsSync(data.projects);
        // Accurate dirty check: redo back to saved state = not dirty (a
        // never-saved `untitled-*` project stays dirty regardless)
        setIsDirty(
          projectId.startsWith('untitled-') || snapshot.sequence !== baselineSequenceRef.current,
        );
      } else {
        try {
          const refresh = await getProject('all');
          if (refresh && !refresh.error && refresh.sequence) {
            setSequence(refresh.sequence);
            setFeatures(refresh.features || EMPTY_ARRAY);
            setEnzymes(refresh.enzymes || EMPTY_ARRAY);
            setPrimers(refresh.primers || EMPTY_ARRAY);
            setAlignments(refresh.alignments || EMPTY_ARRAY);
          }
        } catch {
          // recovery fetch failed; state left as-is
        }
      }
    } catch (e) {
      console.error('redo error:', e);
    }
  }, [onProjectsSync, projectId]);

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
    baselineSequenceRef,
    pushHistory,
    handleUndo,
    handleRedo,
  };
}
