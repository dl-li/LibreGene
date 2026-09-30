import { useCallback, useMemo } from 'react';
import {
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
  setTopology,
  addAlignment,
  addAlignmentSeq,
  removeAlignment,
  openAlignmentFileDialog,
} from '../tauriApi';
import { addMyPrimers, removeMyPrimer, libraryToPrimers } from '../myPrimers';
import { setMyEnzymes } from '../myEnzymes';
import { writeHiddenAlnNames } from './hiddenAlignments';
import { EMPTY_ARRAY } from './constants';

export default function useProjectMutations({
  agentLockedRef,
  operationGenRef,
  projectIdRef,
  pushHistory,
  setIsDirty,
  setFeatures,
  setPrimers,
  setEnzymes,
  setAlignments,
  setTopologyLive,
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
  alignmentAlgorithm,
  alignmentEnabled,
  showAlignments,
  hiddenAlignIds,
}) {
  const handleFeatureFtypeChange = useCallback(
    async (featureId, newFtype) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await updateFeatureFtype(featureId, newFtype);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          pushHistory({ features: data.features });
          setFeatures(data.features);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('update feature ftype error:', e);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const handleFeatureColorChange = useCallback(
    async (featureId, newColor) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await updateFeatureColor(featureId, newColor);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          pushHistory({ features: data.features });
          setFeatures(data.features);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('update feature color error:', e);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const handleFeatureNameChange = useCallback(
    async (featureId, newName) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await updateFeatureName(featureId, newName);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          pushHistory({ features: data.features });
          setFeatures(data.features);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('update feature name error:', e);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const handlePrimerChange = useCallback(
    async (primerData, { recordHistory = true } = {}) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await addPrimer(primerData);
        if (operationGenRef.current !== gen) return;
        if (data && data.primers) {
          // Batch callers record history once via recordHistory on their last
          // call only — the response carries the full post-batch list.
          if (recordHistory) pushHistory({ primers: data.primers });
          setPrimers(data.primers);
          if (data.alignments) setAlignments(data.alignments);
          if (data.enzymes) setEnzymes(data.enzymes);
          setIsDirty(true);
          if (data.projects) onProjectsSync(data.projects);
        }
      } catch (e) {
        console.error('add primer error:', e);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const handleToggleTopology = useCallback(async () => {
    if (agentLockedRef.current) return;
    const next = topologyLive === 'circular' ? 'linear' : 'circular';
    const gen = ++operationGenRef.current;
    try {
      const data = await setTopology(next, projectIdRef.current);
      if (operationGenRef.current !== gen) return;
      if (data && !data.error) {
        // Enzymes/primer binding sites/feature translations are
        // topology-dependent; the response carries the recomputed values.
        if (data.features) setFeatures(data.features);
        if (data.primers) setPrimers(data.primers);
        if (data.enzymes) setEnzymes(data.enzymes);
        setTopologyLive(data.topology || next);
        setIsDirty(true);
        // Topology itself flows down from App's project list.
        if (data.projects) onProjectsSync(data.projects);
      }
    } catch (e) {
      console.error('topology toggle error:', e);
    }
  }, [topologyLive, onProjectsSync]);

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
      if (data && data.primers) {
        setPrimers(data.primers);
        if (data.alignments) setAlignments(data.alignments);
        if (data.enzymes) setEnzymes(data.enzymes);
        setIsDirty(true);
        if (data.projects) onProjectsSync(data.projects);
      }
    },
    [onProjectsSync],
  );

  const handleAddMyPrimerToFile = useCallback(
    async (entry) => {
      if (agentLockedRef.current) return;
      if (!entry) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await addPrimers(libraryToPrimers([entry]));
        if (data && data.primers && operationGenRef.current === gen) {
          pushHistory({ primers: data.primers });
        }
        applyAddedPrimers(data, gen);
      } catch (e) {
        console.error('add primer from My Primers error:', e);
      }
    },
    [pushHistory, applyAddedPrimers],
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
      if (data && data.primers && operationGenRef.current === gen) {
        pushHistory({ primers: data.primers });
      }
      applyAddedPrimers(data, gen);
    } catch (e) {
      console.error('add binding primers error:', e);
    }
  }, [myPrimerBinding.results, myPrimers, primers, pushHistory, applyAddedPrimers]);

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
      const data = await addFeature(feature, locationStr);
      if (operationGenRef.current !== gen) return;
      if (data && data.features) {
        if (recordHistory) pushHistory({ features: data.features });
        setFeatures(data.features);
        if (data.enzymes) setEnzymes(data.enzymes);
        setIsDirty(true);
        if (data.projects) onProjectsSync(data.projects);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const handleFeatureStrandChange = useCallback(
    async (featureId, strand) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await updateFeatureStrand(featureId, strand);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          pushHistory({ features: data.features });
          setFeatures(data.features);
          if (data.enzymes) setEnzymes(data.enzymes);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('update feature strand error:', e);
      }
    },
    [pushHistory],
  );

  const handleFeatureLocationChange = useCallback(
    async (featureId, locationStr) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      // errors propagate so the dialog can display them
      const data = await updateFeatureLocation(featureId, locationStr);
      if (operationGenRef.current !== gen) return;
      if (data && data.features) {
        pushHistory({ features: data.features });
        setFeatures(data.features);
        if (data.projects) onProjectsSync(data.projects);
        setIsDirty(true);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const handleDeleteFeature = useCallback(
    async (featureId) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await deleteFeature(featureId);
        if (operationGenRef.current !== gen) return;
        if (data && data.features) {
          pushHistory({ features: data.features });
          setFeatures(data.features);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('delete feature error:', e);
      }
    },
    [pushHistory, onProjectsSync],
  );

  const handleDeletePrimer = useCallback(
    async (primerId) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      try {
        const data = await deletePrimer(primerId);
        if (operationGenRef.current !== gen) return;
        if (data && data.primers) {
          pushHistory({ primers: data.primers });
          setPrimers(data.primers);
          if (data.alignments) setAlignments(data.alignments);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
        }
      } catch (e) {
        console.error('delete primer error:', e);
      }
    },
    [pushHistory, onProjectsSync],
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
      if (lastData) {
        if (lastData.alignments) setAlignments(lastData.alignments);
        if (lastData.projects) onProjectsSync(lastData.projects);
        setIsDirty(true);
      }
      return { added, failed };
    },
    [onProjectsSync, alignmentAlgorithm],
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
  }, [addAlignmentFiles]);

  const handleAddAlignmentText = useCallback(
    async (name, seq) => {
      if (agentLockedRef.current) return;
      const gen = ++operationGenRef.current;
      const data = await addAlignmentSeq(name, seq, alignmentAlgorithm);
      if (operationGenRef.current !== gen) return;
      if (data && data.error) throw new Error(data.error);
      if (data) {
        if (data.alignments) setAlignments(data.alignments);
        if (data.projects) onProjectsSync(data.projects);
        setIsDirty(true);
      }
    },
    [onProjectsSync, alignmentAlgorithm],
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
    [alignments],
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
        if (data && data.alignments) {
          setAlignments(data.alignments);
          if (data.projects) onProjectsSync(data.projects);
          setIsDirty(true);
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
        console.error('remove alignment error:', e);
      }
    },
    [onProjectsSync, alignments],
  );

  return {
    handleFeatureFtypeChange,
    handleFeatureColorChange,
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
