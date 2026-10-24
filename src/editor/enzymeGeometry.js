export const MAX_ENZ_LABEL_COLS = 128;

export function enzymeLabelIntersectsWindow(cutIndex, visibleCols, streamOf) {
  if (!visibleCols) return false;
  const start = streamOf(cutIndex);
  return start <= visibleCols.end && start + MAX_ENZ_LABEL_COLS >= visibleCols.start;
}
