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

  it('previews undo/redo without moving the cursor until commit', () => {
    const h = createEditHistory();
    h.reset({ sequence: 'A' });
    h.push({ sequence: 'AB' });

    expect(h.peekUndo().sequence).toBe('A');
    expect(h.getCursor()).toBe(1);
    expect(h.canRedo()).toBe(false);
    expect(h.commitUndo().sequence).toBe('A');
    expect(h.getCursor()).toBe(0);

    expect(h.peekRedo().sequence).toBe('AB');
    expect(h.getCursor()).toBe(0);
    expect(h.canUndo()).toBe(false);
    expect(h.commitRedo().sequence).toBe('AB');
    expect(h.getCursor()).toBe(1);
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

  it('round-trips long sequences through patches, history eviction, and redo', () => {
    const h = createEditHistory();
    const states = ['ACGT'.repeat(25000)];
    for (let i = 1; i <= 60; i++) {
      const previous = states[i - 1];
      const at = 100 + i * 17;
      states.push(previous.slice(0, at) + `N${i % 10}` + previous.slice(at + 1));
    }
    h.reset({ sequence: states[0], features: [] });
    for (let i = 1; i < states.length; i++) h.push({ sequence: states[i], features: [] });
    expect(h.getLength()).toBe(50);
    for (let i = 60; i > 11; i--) expect(h.undo().sequence).toBe(states[i - 1]);
    expect(h.canUndo()).toBe(false);
    for (let i = 12; i <= 60; i++) expect(h.redo().sequence).toBe(states[i]);
    expect(h.canRedo()).toBe(false);
  });
});
