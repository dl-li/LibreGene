import { describe, expect, it } from 'vitest';
import {
  advanceCircularDrag,
  circularDragSelection,
  createCircularDrag,
} from '../circularSelection';

function drag(anchor, positions, length = 100) {
  let state = createCircularDrag(anchor);
  for (const position of positions) state = advanceCircularDrag(state, position, length);
  return circularDragSelection(state, length);
}

describe('circular map drag selection', () => {
  it('preserves a clockwise origin crossing as start > end', () => {
    expect(drag(90, [95, 0, 10])).toEqual({ start: 90, end: 10 });
  });

  it('preserves a reverse origin crossing instead of selecting the complement', () => {
    expect(drag(10, [5, 0, 95, 90])).toEqual({ start: 90, end: 10 });
  });

  it('supports ordinary forward and reverse selections', () => {
    expect(drag(20, [30, 40])).toEqual({ start: 20, end: 40 });
    expect(drag(40, [30, 20])).toEqual({ start: 20, end: 40 });
  });

  it('keeps clicks and ambiguous full turns selection-free', () => {
    expect(drag(20, [])).toBeNull();
    expect(drag(20, [40, 20])).toBeNull();
    expect(drag(0, [25, 50, 75, 0])).toBeNull();
  });
});
