export function buildSegmentRowIndex(items, rowOf, segmentsOf = (item) => item.segments) {
  const rows = {};
  for (const item of items || []) {
    const seen = new Set();
    for (const seg of segmentsOf(item) || []) {
      const start = rowOf(seg.start);
      const end = rowOf(seg.end);
      for (let row = start; row <= end; row++) {
        if (seen.has(row)) continue;
        seen.add(row);
        (rows[row] || (rows[row] = [])).push(item);
      }
    }
  }
  return rows;
}

export function entitiesInRowRange(index, start, end, rowOf) {
  const result = [];
  const seen = new Set();
  for (let row = rowOf(start); row <= rowOf(end); row++) {
    for (const item of index[row] || []) {
      if (seen.has(item)) continue;
      seen.add(item);
      result.push(item);
    }
  }
  return result;
}

export function visibleRowRange(rowY, rowAbove, rowBelow, scrollY, viewportH) {
  if (!rowY.length) return { start: 0, end: 0 };
  const bottom = scrollY + (viewportH || 900);
  let lo = 0;
  let hi = rowY.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (rowY[mid] + (rowBelow[mid] || 0) > scrollY) hi = mid;
    else lo = mid + 1;
  }
  const start = lo === rowY.length ? 0 : lo;
  lo = 0;
  hi = rowY.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (rowY[mid] - (rowAbove[mid] || 0) < bottom) lo = mid + 1;
    else hi = mid;
  }
  return { start, end: Math.min(rowY.length - 1, lo) };
}
