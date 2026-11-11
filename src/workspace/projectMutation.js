const FIELD_SETTERS = {
  sequence: 'setSequence',
  features: 'setFeatures',
  enzymes: 'setEnzymes',
  primers: 'setPrimers',
  alignments: 'setAlignments',
  textColorsTop: 'setTextColorsTop',
  textColorsBottom: 'setTextColorsBottom',
  moleculeType: 'setMoleculeType',
  topology: 'setTopologyLive',
  tracePath: 'setTracePath',
  msaRows: 'setRows',
  msaAligned: 'setAligned',
  referenceRowId: 'setReferenceRowId',
};

export function applyProjectMutation(data, setters, { defaultDirty, onError } = {}) {
  if (!data || data.error) {
    onError?.('Project update failed', data?.error || 'Invalid mutation response');
    return false;
  }
  const revision = data.revision;
  const latestRevision = setters.latestRevisionRef?.current;
  if (typeof revision === 'number' && typeof latestRevision === 'number') {
    if (revision < latestRevision) return false;
  }
  const changedFields = Array.isArray(data.changedFields) ? new Set(data.changedFields) : null;
  for (const [field, setterName] of Object.entries(FIELD_SETTERS)) {
    if (data[field] !== undefined && (!changedFields || changedFields.has(field))) {
      setters[setterName]?.(data[field]);
    }
  }
  if (typeof revision === 'number' && setters.latestRevisionRef) {
    setters.latestRevisionRef.current = revision;
  }
  if (data.projects) setters.onProjectsSync?.(data.projects);
  if (typeof data.dirty === 'boolean') setters.setIsDirty?.(data.dirty);
  else if (typeof defaultDirty === 'boolean') setters.setIsDirty?.(defaultDirty);
  return true;
}
