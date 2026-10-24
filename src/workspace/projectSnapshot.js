import { EMPTY_ARRAY } from './constants';

export function createProjectSnapshot(state = {}) {
  return {
    sequence: state.sequence || '',
    features: state.features || EMPTY_ARRAY,
    primers: state.primers || EMPTY_ARRAY,
    textColorsTop: state.textColorsTop || EMPTY_ARRAY,
    textColorsBottom: state.textColorsBottom || EMPTY_ARRAY,
  };
}

export function projectSnapshotsEqual(a, b) {
  return JSON.stringify(createProjectSnapshot(a)) === JSON.stringify(createProjectSnapshot(b));
}

export function snapshotIsDirty(snapshot, baseline, projectId, backendDirty) {
  if (projectSnapshotsEqual(snapshot, baseline)) return projectId.startsWith('untitled-');
  if (typeof backendDirty === 'boolean') return backendDirty;
  return true;
}
