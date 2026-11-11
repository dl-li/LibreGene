import { useCallback, useMemo, useRef } from 'react';
import {
  updateFeatureFtype as updateFeatureFtypeCommand,
  updateFeatureColor as updateFeatureColorCommand,
  updateFeatureName as updateFeatureNameCommand,
  updateFeatureStrand as updateFeatureStrandCommand,
  updateFeatureLocation as updateFeatureLocationCommand,
  addFeature as addFeatureCommand,
  deleteFeature as deleteFeatureCommand,
  addPrimer as addPrimerCommand,
  addPrimers as addPrimersCommand,
  deletePrimer as deletePrimerCommand,
  setTopology,
  setTextColors as setTextColorsCommand,
  addAlignment as addAlignmentCommand,
  addAlignmentSeq as addAlignmentSeqCommand,
  removeAlignment as removeAlignmentCommand,
  openAlignmentFileDialog,
} from '../tauriApi';
import { addMyPrimers, removeMyPrimer, libraryToPrimers } from '../myPrimers';
import { setMyEnzymes } from '../myEnzymes';
import { writeHiddenAlnNames } from './hiddenAlignments';
import { EMPTY_ARRAY } from './constants';
import { applyProjectMutation } from './projectMutation';

export function requirePrimerMutationData(data) {
  if (data?.error) throw new Error(data.error);
  if (!data?.primers) throw new Error('Primer update failed');
  return data;
}

export default function useProjectMutations({
  agentLockedRef,
  operationGenRef,
  projectIdRef,
  latestRevisionRef,
  setIsDirty,
  setSequence,
  setFeatures,
  setPrimers,
  setEnzymes,
  setAlignments,
  setTextColorsTop,
  setTextColorsBottom,
  setTopologyLive,
  setMoleculeType,
  setTracePath,
  topologyLive,
  setExpandedChromAlnId,
  setHiddenAlignIds,
  alignments,
  primers,
  myPrimerBinding,
  myPrimers,
  onMyPrimersChange,
  onMyEnzymesChange,
  onProjectsSync,
  onError,
  alignmentAlgorithm,
  alignmentEnabled,
  showAlignments,
  hiddenAlignIds,
}) {
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;
  const {
    updateFeatureFtype,
    updateFeatureColor,
    updateFeatureName,
    updateFeatureStrand,
    updateFeatureLocation,
    addFeature,
    deleteFeature,
    addPrimer,
    addPrimers,
    deletePrimer,
    setTextColors,
    addAlignment,
    addAlignmentSeq,
    removeAlignment,
  } = useMemo(() => {
    const forProject =
      (command) =>
      (...args) =>
        command(projectIdRef.current, ...args);
    return {
      updateFeatureFtype: forProject(updateFeatureFtypeCommand),
      updateFeatureColor: forProject(updateFeatureColorCommand),
      updateFeatureName: forProject(updateFeatureNameCommand),
      updateFeatureStrand: forProject(updateFeatureStrandCommand),
      updateFeatureLocation: forProject(updateFeatureLocationCommand),
      addFeature: forProject(addFeatureCommand),
      deleteFeature: forProject(deleteFeatureCommand),
      addPrimer: forProject(addPrimerCommand),
      addPrimers: forProject(addPrimersCommand),
      deletePrimer: forProject(deletePrimerCommand),
      setTextColors: forProject(setTextColorsCommand),
      addAlignment: forProject(addAlignmentCommand),
      addAlignmentSeq: forProject(addAlignmentSeqCommand),
      removeAlignment: forProject(removeAlignmentCommand),
    };
  }, [projectIdRef]);

  const applyMutation = useCallback(
    (data) =>
      applyProjectMutation(
        data,
        {
          latestRevisionRef,
          setSequence,
          setFeatures,
          setPrimers,
          setEnzymes,
          setAlignments,
          setTextColorsTop,
          setTextColorsBottom,
          setTopologyLive,
          setMoleculeType,
          setTracePath,
          setIsDirty,
          onProjectsSync,
        },
        { defaultDirty: true, onError: onErrorRef.current },
      ),
    [
      latestRevisionRef,
      setSequence,
      setFeatures,
      setPrimers,
      setEnzymes,
      setAlignments,
      setTextColorsTop,
      setTextColorsBottom,
      setTopologyLive,
      setMoleculeType,
      setTracePath,
      setIsDirty,
      onProjectsSync,
    ],
  );

  const handleFeatureFtypeChange = useCallback(
    async (featureId, newFtype) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await updateFeatureFtype(featureId, newFtype);
        if (operationGenRef.current !== gen) return;
        applyMutation(data);
      } catch (e) {
        onErrorRef.current('update feature ftype error:', e);
      }
    },
    [applyMutation, agentLockedRef, operationGenRef, updateFeatureFtype],
  );

  const handleFeatureColorChange = useCallback(
    async (featureId, newColor) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await updateFeatureColor(featureId, newColor);
        if (operationGenRef.current !== gen) return;
        applyMutation(data);
      } catch (e) {
        onErrorRef.current('update feature color error:', e);
      }
    },
    [applyMutation, agentLockedRef, operationGenRef, updateFeatureColor],
  );

  // Sequence text color on [start, end] (0-based inclusive); '' clears.
  const handleSetTextColor = useCallback(
    async (strand, start, end, color) => {
      if (agentLockedRef.current) return false;
      const gen = ++operationGenRef.current;
      try {
        const data = await setTextColors(strand, start, end, color);
        if (operationGenRef.current !== gen) return false;
        if (!data || !data.textColorsTop) return false;
        return applyMutation(data);
      } catch (e) {
        onErrorRef.current('set text colors error:', e);
        return false;
      }
    },
    [applyMutation, agentLockedRef, operationGenRef, setTextColors],
  );

  const handleFeatureNameChange = useCallback(
    async (featureId, newName) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await updateFeatureName(featureId, newName);
        if (operationGenRef.current !== gen) return;
        applyMutation(data);
      } catch (e) {
        onErrorRef.current('update feature name error:', e);
      }
    },
    [applyMutation, agentLockedRef, operationGenRef, updateFeatureName],
  );

  const handlePrimerChange = useCallback(
    async (primerData, { recordHistory = true } = {}) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = requirePrimerMutationData(await addPrimer(primerData, recordHistory));
        if (operationGenRef.current !== gen) return;
        applyMutation(data);
      } catch (e) {
        onErrorRef.current('add primer error:', e);
        throw e;
      }
    },
    [addPrimer, applyMutation, agentLockedRef, operationGenRef],
  );

  const handleToggleTopology = useCallback(async () => {
    if (agentLockedRef.current) return;
    const next = topologyLive === 'circular' ? 'linear' : 'circular';
    const gen = ++operationGenRef.current;
    try {
      const data = await setTopology(next, projectIdRef.current);
      if (operationGenRef.current !== gen) return;
      applyMutation(data);
    } catch (e) {
      onErrorRef.current('topology toggle error:', e);
    }
  }, [topologyLive, applyMutation, agentLockedRef, operationGenRef, projectIdRef]);

  // --- My Primers library actions ---
  const handleAddPrimerToMyPrimers = useCallback(
    (primer) => {
      if (!primer || !primer.primerSeq) return;
      onMyPrimersChange?.(addMyPrimers([primer]));
    },
    [onMyPrimersChange],
  );

  const handleAddAllPrimersToMyPrimers = useCallback(() => {
    if (!primers.length) return;
    onMyPrimersChange?.(addMyPrimers(primers));
  }, [primers, onMyPrimersChange]);

  const handleDeleteMyPrimer = useCallback(
    (id) => {
      onMyPrimersChange?.(removeMyPrimer(id));
    },
    [onMyPrimersChange],
  );

  const applyAddedPrimers = useCallback(
    (data, gen) => {
      if (operationGenRef.current !== gen) return;
      applyMutation(data);
    },
    [operationGenRef, applyMutation],
  );

  const handleAddMyPrimerToFile = useCallback(
    async (entry) => {
      if (agentLockedRef.current) return;
      if (!entry) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await addPrimers(libraryToPrimers([entry]));
        applyAddedPrimers(data, gen);
      } catch (e) {
        onErrorRef.current('add primer from My Primers error:', e);
      }
    },
    [applyAddedPrimers, addPrimers, agentLockedRef, operationGenRef],
  );

  const handleAddAllBindingPrimers = useCallback(async () => {
    if (agentLockedRef.current) return;
    const bindingIds = new Set(
      (myPrimerBinding.results || []).filter((r) => r.binds).map((r) => r.id),
    );
    const inFile = new Set(primers.map((p) => String(p.primerSeq || '').toUpperCase()));
    const toAdd = myPrimers.filter(
      (p) => bindingIds.has(p.id) && !inFile.has(String(p.seq || p.primerSeq || '').toUpperCase()),
    );
    if (!toAdd.length) return;
    const gen = ++operationGenRef.current;
    try {
      const data = await addPrimers(libraryToPrimers(toAdd));
      applyAddedPrimers(data, gen);
    } catch (e) {
      onErrorRef.current('add binding primers error:', e);
    }
  }, [
    myPrimerBinding.results,
    myPrimers,
    primers,
    applyAddedPrimers,
    addPrimers,
    agentLockedRef,
    operationGenRef,
  ]);

  const handleMyEnzymesChange = useCallback(
    (list) => {
      onMyEnzymesChange?.(setMyEnzymes(list));
    },
    [onMyEnzymesChange],
  );

  const handleFeatureAdd = useCallback(
    async (featureData, { recordHistory = true } = {}) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      const { locationStr, ...feature } = featureData;
      // errors propagate so the dialog can display them; batch callers record
      // history once via recordHistory on their last call only — the response
      // carries the full post-batch list
      const data = await addFeature(feature, locationStr, recordHistory);
      if (operationGenRef.current !== gen) return;
      applyMutation(data);
    },
    [addFeature, applyMutation, agentLockedRef, operationGenRef],
  );

  const handleFeatureStrandChange = useCallback(
    async (featureId, strand) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await updateFeatureStrand(featureId, strand);
        if (operationGenRef.current !== gen) return;
        applyMutation(data);
      } catch (e) {
        onErrorRef.current('update feature strand error:', e);
      }
    },
    [applyMutation, agentLockedRef, operationGenRef, updateFeatureStrand],
  );

  const handleFeatureLocationChange = useCallback(
    async (featureId, locationStr) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      // errors propagate so the dialog can display them
      const data = await updateFeatureLocation(featureId, locationStr);
      if (operationGenRef.current !== gen) return;
      applyMutation(data);
    },
    [applyMutation, agentLockedRef, operationGenRef, updateFeatureLocation],
  );

  const handleDeleteFeature = useCallback(
    async (featureId) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await deleteFeature(featureId);
        if (operationGenRef.current !== gen) return;
        applyMutation(data);
      } catch (e) {
        onErrorRef.current('delete feature error:', e);
      }
    },
    [applyMutation, agentLockedRef, deleteFeature, operationGenRef],
  );

  const handleDeletePrimer = useCallback(
    async (primerId) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await deletePrimer(primerId);
        if (operationGenRef.current !== gen) return;
        applyMutation(data);
      } catch (e) {
        onErrorRef.current('delete primer error:', e);
      }
    },
    [applyMutation, agentLockedRef, deletePrimer, operationGenRef],
  );

  const addAlignmentFiles = useCallback(
    async (paths) => {
      if (agentLockedRef.current) return null;
      const gen = ++operationGenRef.current;
      const failed = [];
      let added = 0;
      let lastData = null;
      for (const path of paths) {
        try {
          const data = await addAlignment(path, alignmentAlgorithm);
          if (operationGenRef.current !== gen) return null;
          if (data && data.error) {
            failed.push({ path, error: data.error });
          } else if (data) {
            added += 1;
            lastData = data;
          }
        } catch (e) {
          failed.push({ path, error: String(e?.message || e) });
        }
      }
      if (lastData) applyMutation(lastData);
      return { added, failed };
    },
    [alignmentAlgorithm, addAlignment, applyMutation, agentLockedRef, operationGenRef],
  );

  const handleAddAlignment = useCallback(async () => {
    if (agentLockedRef.current) return;
    const paths = await openAlignmentFileDialog();
    if (!paths || paths.length === 0) return;
    const result = await addAlignmentFiles(paths);
    if (result && result.failed.length > 0) {
      const lines = result.failed.map((f) => {
        const name = f.path.replace(/\\/g, '/').split('/').pop();
        return `${name}: ${f.error}`;
      });
      throw new Error(lines.join('\n'));
    }
  }, [addAlignmentFiles, agentLockedRef]);

  const handleAddAlignmentText = useCallback(
    async (name, seq) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      const data = await addAlignmentSeq(name, seq, alignmentAlgorithm);
      if (operationGenRef.current !== gen) return;
      if (data && data.error) throw new Error(data.error);
      applyMutation(data);
    },
    [alignmentAlgorithm, addAlignmentSeq, applyMutation, agentLockedRef, operationGenRef],
  );

  const handleToggleAlignmentVisible = useCallback(
    (alignmentId) => {
      setHiddenAlignIds((prev) => {
        const next = prev.includes(alignmentId)
          ? prev.filter((x) => x !== alignmentId)
          : [...prev, alignmentId];
        writeHiddenAlnNames(
          projectIdRef.current,
          next.map((id) => alignments.find((a) => a.id === id)?.name).filter(Boolean),
        );
        return next;
      });
      // Hiding a track collapses its chromatogram band if it was expanded.
      setExpandedChromAlnId((cur) => (cur === alignmentId ? null : cur));
    },
    [alignments, projectIdRef, setExpandedChromAlnId, setHiddenAlignIds],
  );

  const visibleAlignments = useMemo(
    () =>
      alignmentEnabled && showAlignments
        ? alignments.filter((a) => !hiddenAlignIds.includes(a.id))
        : EMPTY_ARRAY,
    [alignmentEnabled, showAlignments, alignments, hiddenAlignIds],
  );

  const handleRemoveAlignment = useCallback(
    async (alignmentId) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await removeAlignment(alignmentId);
        if (operationGenRef.current !== gen) return;
        if (applyMutation(data)) {
          const removedName = alignments.find((a) => a.id === alignmentId)?.name;
          setHiddenAlignIds((prev) => {
            const next = prev.filter((x) => x !== alignmentId);
            writeHiddenAlnNames(
              projectIdRef.current,
              next
                .map((id) => alignments.find((a) => a.id === id)?.name)
                .filter((n) => n && n !== removedName),
            );
            return next;
          });
        }
      } catch (e) {
        onErrorRef.current('remove alignment error:', e);
      }
    },
    [
      alignments,
      applyMutation,
      agentLockedRef,
      operationGenRef,
      projectIdRef,
      removeAlignment,
      setHiddenAlignIds,
    ],
  );

  return {
    handleFeatureFtypeChange,
    handleFeatureColorChange,
    handleSetTextColor,
    handleFeatureNameChange,
    handlePrimerChange,
    handleToggleTopology,
    handleAddPrimerToMyPrimers,
    handleAddAllPrimersToMyPrimers,
    handleDeleteMyPrimer,
    handleAddMyPrimerToFile,
    handleAddAllBindingPrimers,
    handleMyEnzymesChange,
    handleFeatureAdd,
    handleFeatureStrandChange,
    handleFeatureLocationChange,
    handleDeleteFeature,
    handleDeletePrimer,
    addAlignmentFiles,
    handleAddAlignment,
    handleAddAlignmentText,
    handleToggleAlignmentVisible,
    visibleAlignments,
    handleRemoveAlignment,
  };
}
