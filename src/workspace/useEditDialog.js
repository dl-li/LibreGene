import { useState, useCallback } from 'react';
import { addPrimers, getProject, updateSequence } from '../tauriApi';
import { adjustAlignmentsForEdit } from '../alignmentEdit';
import { EMPTY_ARRAY } from './constants';

export default function useEditDialog({
  agentLockedRef,
  operationGenRef,
  editHistoryRef,
  setRestoreState,
  undoVersionRef,
  sequence,
  features,
  primers,
  setSequence,
  setFeatures,
  setEnzymes,
  setPrimers,
  setAlignments,
  setIsDirty,
  onProjectsSync,
  isDna,
}) {
  const [editDialog, setEditDialog] = useState({
    open: false,
    mode: 'insert',
    cursorIndex: null,
    selStart: null,
    selEnd: null,
    selectedText: '',
    initialText: '',
    clipboardMeta: null,
  });

  /**
   * 调整特征/注释放置位置以适配编辑后的序列。
   * 编辑会删除 [editStart, editEnd] 区间（oldLen 个碱基），
   * 然后插入 newLen 个碱基。
   * 编辑区之外的特征位置保持与原序列的相对偏移不变。
   * 等长替换（delta === 0）不改变任何坐标，特征原样保留。
   */
  const adjustAnnotations = useCallback((anns, editStart, editEnd, oldLen, newLen) => {
    const delta = newLen - oldLen;
    if (delta === 0) return anns; // no-op / equal-length replace keeps all coordinates

    return anns
      .map((ann) => {
        const adjustSegments = (segments) => {
          if (!segments || !segments.length) return segments;
          return segments
            .map((seg) => {
              let { start: s, end: e } = seg;
              if (e < editStart) return seg;
              if (s > editEnd) return { ...seg, start: s + delta, end: e + delta };
              // spans
              const ns = s < editStart ? s : editStart + newLen;
              const ne = e > editEnd ? e + delta : editStart + newLen - 1;
              if (ns > ne) return null;
              return { ...seg, start: ns, end: ne };
            })
            .filter(Boolean);
        };

        if (ann.segments && ann.segments.length) {
          // Derive start/end from adjusted segments so wrap-origin features
          // (start > end, segments in join order) shift correctly.
          const newSegments = adjustSegments(ann.segments);
          if (!newSegments.length) return null;
          return {
            ...ann,
            start: newSegments[0].start,
            end: newSegments[newSegments.length - 1].end,
            segments: newSegments,
          };
        }

        let newStart, newEnd;
        if (ann.end < editStart) {
          // entirely before – unchanged
          return ann;
        }
        if (ann.start > editEnd) {
          // entirely after – shift
          newStart = ann.start + delta;
          newEnd = ann.end + delta;
        } else {
          // spans the edit
          newStart = ann.start < editStart ? ann.start : editStart + newLen;
          newEnd = ann.end > editEnd ? ann.end + delta : editStart + newLen - 1;
        }

        if (newStart > newEnd) return null;

        return { ...ann, start: newStart, end: newEnd };
      })
      .filter(Boolean);
  }, []);

  // --- Edit dialog confirmed (insert/delete/replace) ---
  const handleEditConfirm = useCallback(
    async (result) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      const { mode, cursorIndex, selStart, selEnd } = editDialog;
      let newSeq;
      const currentSeq = sequence || '';
      let editStart, editEnd, oldLen, newLen;

      // Compute the new sequence and save edit parameters for feature adjustment
      if (mode === 'insert') {
        const cleaned = (result.sequence || '').replace(/\s/g, '');
        if (!cleaned) {
          setEditDialog((prev) => ({ ...prev, open: false }));
          return;
        }
        editStart = cursorIndex;
        editEnd = cursorIndex - 1; // no deletion range
        oldLen = 0;
        newLen = cleaned.length;
        newSeq = currentSeq.slice(0, cursorIndex) + cleaned + currentSeq.slice(cursorIndex);
      } else if (mode === 'delete') {
        if (selStart === null || selEnd === null) {
          setEditDialog((prev) => ({ ...prev, open: false }));
          return;
        }
        editStart = selStart;
        editEnd = selEnd;
        oldLen = selEnd - selStart + 1;
        newLen = 0;
        newSeq = currentSeq.slice(0, selStart) + currentSeq.slice(selEnd + 1);
      } else if (mode === 'replace') {
        const cleaned = (result.sequence || '').replace(/\s/g, '');
        if (selStart === null || selEnd === null) {
          setEditDialog((prev) => ({ ...prev, open: false }));
          return;
        }
        editStart = selStart;
        editEnd = selEnd;
        oldLen = selEnd - selStart + 1;
        newLen = cleaned.length;
        newSeq = currentSeq.slice(0, selStart) + cleaned + currentSeq.slice(selEnd + 1);
      } else {
        return;
      }

      // Close dialog
      setEditDialog((prev) => ({ ...prev, open: false }));

      // Compute adjusted features BEFORE backend call (for history and optimistic update)
      let adjustedFeatures = adjustAnnotations(
        features || EMPTY_ARRAY,
        editStart,
        editEnd,
        oldLen,
        newLen,
      );

      // Merge annotation features from clipboard
      const annotations = result.annotations;
      let mergedFeatures = adjustedFeatures;
      if (annotations && annotations.features && annotations.features.length > 0) {
        const insertAnchor = mode === 'insert' ? cursorIndex : selStart;
        // Clamp the offset segments to the pasted span — mirrors the Rust
        // transfer_features_for_insert clamping, so a forged clipboard meta
        // can't push coordinates outside the insertion window.
        const newLen = (result.sequence || '').replace(/\s/g, '').length;
        const existingNames = new Set(adjustedFeatures.map((f) => f.name));
        const newFeats = [];
        annotations.features.forEach((af, i) => {
          const segsIn = Array.isArray(af.segments) ? af.segments : [];
          const segs = [];
          for (const s of segsIn) {
            const cs = Math.max(0, Math.min(s.start ?? 0, newLen - 1));
            const ce = Math.max(0, Math.min(s.end ?? 0, newLen - 1));
            if (cs <= ce) segs.push({ start: insertAnchor + cs, end: insertAnchor + ce });
          }
          if (segs.length === 0) return;
          let name = af.name;
          if (existingNames.has(name)) {
            let n = 2;
            while (existingNames.has(`${name} (${n})`)) n++;
            name = `${name} (${n})`;
          }
          existingNames.add(name);
          newFeats.push({
            id: `feature_${Date.now()}_${i}_${Math.random().toString(36).slice(2, 8)}`,
            name,
            ftype: af.ftype,
            color: af.color,
            strand: af.strand,
            notes: af.notes,
            qualifiers: af.qualifiers,
            start: segs[0].start,
            end: segs[segs.length - 1].end,
            segments: segs,
          });
        });
        mergedFeatures = [...adjustedFeatures, ...newFeats];
      }

      // Post-edit selection: a deletion clears the (now stale) selection but
      // parks the cursor at the deletion point; an insertion leaves the
      // freshly inserted bases selected.
      const postCursor =
        mode === 'insert' ? cursorIndex + newLen : mode === 'delete' ? selStart : cursorIndex;
      const postSelStart = mode === 'insert' ? cursorIndex : mode === 'delete' ? null : selStart;
      const postSelEnd =
        mode === 'insert' ? cursorIndex + newLen - 1 : mode === 'delete' ? null : selEnd;

      // Push new state to undo history (includes adjusted features for correct undo)
      editHistoryRef.current.push({
        sequence: newSeq,
        features: mergedFeatures,
        primers: primers || EMPTY_ARRAY,
        cursorIndex: postCursor,
        selStart: postSelStart,
        selEnd: postSelEnd,
      });

      // Apply the post-edit selection in the editor (replace keeps its range).
      if (mode !== 'replace') {
        setRestoreState({
          version: ++undoVersionRef.current,
          cursorIndex: postCursor,
          selStart: postSelStart,
          selEnd: postSelEnd,
          selectionMode: postSelStart === null ? 'none' : 'text',
          selectedPrimerIds: [],
          isEnzymeSelection: false,
          selectedEnzymeIds: [],
          translationSel: null,
        });
      }

      // Optimistic UI update
      setSequence(newSeq);
      setFeatures(mergedFeatures);
      // Alignment models are anchored to template columns, so they have to be
      // carried over with the edit too — otherwise the lanes (and the
      // chromatogram riding on them) are drawn from the old model against the
      // new sequence until the backend's recomputed models land.
      setAlignments((prev) => adjustAlignmentsForEdit(prev, editStart, editEnd, oldLen, newLen));
      setIsDirty(true);

      // Send to backend for recomputation (enzymes, primer binding sites)
      try {
        const data = await updateSequence(newSeq, mergedFeatures);
        if (operationGenRef.current !== gen) return;
        if (data && !data.error) {
          setSequence(data.sequence);
          setFeatures(data.features || mergedFeatures); // backend recomputed CDS/mRNA translations
          setEnzymes(data.enzymes || EMPTY_ARRAY);
          setPrimers(data.primers || EMPTY_ARRAY);
          setAlignments(data.alignments || EMPTY_ARRAY);
          if (data.projects) onProjectsSync(data.projects);

          // Import primers from clipboard annotations (DNA only)
          if (annotations && annotations.primers && annotations.primers.length > 0 && isDna) {
            const allNames = new Set([
              ...(data.primers || []).map((p) => p.name),
              ...(data.features || mergedFeatures).map((f) => f.name),
            ]);
            const primerImports = annotations.primers.map((ap) => {
              let name = ap.name;
              if (allNames.has(name)) {
                let n = 2;
                while (allNames.has(`${name} (${n})`)) n++;
                name = `${name} (${n})`;
              }
              allNames.add(name);
              return { name, type: ap.type, primerSeq: ap.primerSeq };
            });
            try {
              const pData = await addPrimers(primerImports);
              if (operationGenRef.current !== gen) return;
              if (pData && pData.primers) {
                setPrimers(pData.primers);
                setIsDirty(true);
              }
            } catch (pe) {
              console.error('import primers error:', pe);
            }
          }
        } else {
          console.error('update_sequence error:', data?.error || 'unknown');
          // Re-fetch to recover from optimistic update
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
        console.error('update_sequence exception:', e);
      }
    },
    [sequence, editDialog, adjustAnnotations, features, primers, onProjectsSync, isDna],
  );

  return { editDialog, setEditDialog, handleEditConfirm };
}
