import { describe, expect, it } from 'vitest';
import { createEditHistory } from '../editHistory';
import {
  createProjectSnapshot,
  projectSnapshotsEqual,
  snapshotIsDirty,
} from '../workspace/projectSnapshot';

const baseline = createProjectSnapshot({
  sequence: 'AAAA',
  features: [{ id: 'f1', start: 0, end: 3 }],
  primers: [{ id: 'p1' }],
  textColorsTop: [{ start: 0, end: 1, color: '#111111' }],
  textColorsBottom: [],
});

describe('project snapshots and dirty state', () => {
  it.each(['features', 'primers', 'textColorsTop', 'textColorsBottom'])(
    'detects %s changes even when sequence is unchanged',
    (field) => {
      const changed = { ...baseline, [field]: [...baseline[field], { id: 'changed' }] };
      expect(projectSnapshotsEqual(changed, baseline)).toBe(false);
      expect(snapshotIsDirty(changed, baseline, '/tmp/a.gbk')).toBe(true);
    },
  );

  it('keeps feature-only changes dirty after sequence edit, undo and redo', () => {
    const history = createEditHistory();
    history.reset(baseline);
    const featureEdit = {
      ...baseline,
      features: [{ id: 'f1', start: 1, end: 3 }],
    };
    const sequenceEdit = { ...featureEdit, sequence: 'AAAAT' };
    history.push(featureEdit);
    history.push(sequenceEdit);

    const undoneSequence = history.undo();
    expect(undoneSequence.sequence).toBe('AAAA');
    expect(snapshotIsDirty(undoneSequence, baseline, '/tmp/a.gbk')).toBe(true);

    const undoneFeature = history.undo();
    expect(snapshotIsDirty(undoneFeature, baseline, '/tmp/a.gbk')).toBe(false);
    expect(snapshotIsDirty(history.redo(), baseline, '/tmp/a.gbk')).toBe(true);
    expect(snapshotIsDirty(history.redo(), baseline, '/tmp/a.gbk')).toBe(true);
  });

  it('keeps a returned-to-baseline state clean despite mutation dirty markers', () => {
    expect(snapshotIsDirty(baseline, baseline, '/tmp/a.gbk', true)).toBe(false);
    expect(snapshotIsDirty(baseline, baseline, '/tmp/a.gbk', false)).toBe(false);
    expect(snapshotIsDirty(baseline, baseline, 'untitled-1')).toBe(true);
  });

  it('uses backend dirty truth for non-baseline states when available', () => {
    const changed = { ...baseline, sequence: 'AAAT' };
    expect(snapshotIsDirty(changed, baseline, '/tmp/a.gbk', false)).toBe(false);
    expect(snapshotIsDirty(changed, baseline, '/tmp/a.gbk', true)).toBe(true);
  });
});
