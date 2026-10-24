export function createCircularDrag(anchor) {
  return { anchor, offset: 0, moved: false };
}

export function advanceCircularDrag(drag, position, length) {
  let delta = position - ((((drag.anchor + drag.offset) % length) + length) % length);
  if (delta > length / 2) delta -= length;
  if (delta < -length / 2) delta += length;
  return {
    anchor: drag.anchor,
    offset: drag.offset + delta,
    moved: drag.moved || delta !== 0,
  };
}

export function circularDragSelection(drag, length) {
  if (!drag?.moved || drag.offset === 0 || Math.abs(drag.offset) >= length) return null;
  const position = (((drag.anchor + drag.offset) % length) + length) % length;
  return drag.offset > 0
    ? { start: drag.anchor, end: position }
    : { start: position, end: drag.anchor };
}
