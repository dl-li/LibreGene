import { describe, it, expect } from 'vitest';
import { createEditHistory } from '../editHistory';

describe('createEditHistory (undo/redo stack)', () => {
  it('starts with nothing to undo/redo', () => {
    const h = createEditHistory();
    expect(h.canUndo()).toBe(false);
    expect(h.canRedo()).toBe(false);
    expect(h.undo()).toBeNull();
    expect(h.redo()).toBeNull();
  });

  it('reset seeds the baseline', () => {
    const h = createEditHistory();
    h.reset({ sequence: 'ATGC', features: [] });
    expect(h.canUndo()).toBe(false);
    expect(h.getLength()).toBe(1);
  });

  it('push / undo / redo round-trip', () => {
    const h = createEditHistory();
    h.reset({ sequence: 'AAAA' });
    h.push({ sequence: 'AAAT' });
    h.push({ sequence: 'AAAT' }); // same value, distinct stack entries
    expect(h.canUndo()).toBe(true);

    const s1 = h.undo();
    expect(s1.sequence).toBe('AAAT', 'first undo steps back one entry');
    const s0 = h.undo();
    expect(s0.sequence).toBe('AAAA');
    expect(h.canUndo()).toBe(false, 'back to baseline');
    expect(h.canRedo()).toBe(true);

    const s2 = h.redo();
    expect(s2.sequence).toBe('AAAT');
    expect(h.canRedo()).toBe(true);
    h.redo();
    expect(h.canRedo()).toBe(false);
  });

  it('push truncates the redo branch', () => {
    const h = createEditHistory();
    h.reset({ sequence: 'A' });
    h.push({ sequence: 'AB' });
    h.undo(); // now redo-able
    h.push({ sequence: 'AC' }); // branch replaced
    expect(h.canRedo()).toBe(false, 'redo branch truncated by new push');
    expect(h.undo().sequence).toBe('A');
    expect(h.redo().sequence).toBe('AC', 'AB is gone');
  });

  it('caps history at 50 entries', () => {
    const h = createEditHistory();
    h.reset({ n: 0 });
    for (let i = 1; i <= 60; i++) h.push({ n: i });
    expect(h.getLength()).toBeLessThanOrEqual(50);
    // After the shift the baseline {n:0} is gone: oldest undoable is n>=11
    let steps = 0;
    while (h.canUndo()) {
      h.undo();
      steps++;
    }
    expect(steps).toBeLessThanOrEqual(49);
  });

  it('returns a fresh top-level object per snapshot (shallow copy)', () => {
    const h = createEditHistory();
    const baseline = { sequence: 'AT' };
    h.reset(baseline);
    h.push({ sequence: 'ATG' });
    const undone = h.undo();
    expect(undone).not.toBe(baseline, 'snapshot is a spread copy, not the caller object');
    expect(undone.sequence).toBe('AT');
    // Note: copies are shallow — nested arrays are shared with the pushed
    // snapshot. Callers push freshly built objects, so this is fine in
    // practice; this test pins the actual contract.
  });
});
