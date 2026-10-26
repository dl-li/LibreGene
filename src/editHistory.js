const MAX_HISTORY = 50;

function sequencePatch(previous, next) {
  let start = 0;
  const limit = Math.min(previous.length, next.length);
  while (start < limit && previous[start] === next[start]) start++;
  let previousEnd = previous.length;
  let nextEnd = next.length;
  while (
    previousEnd > start &&
    nextEnd > start &&
    previous[previousEnd - 1] === next[nextEnd - 1]
  ) {
    previousEnd--;
    nextEnd--;
  }
  return {
    start,
    before: previous.slice(start, previousEnd),
    after: next.slice(start, nextEnd),
  };
}

function applySequencePatch(sequence, patch) {
  return (
    sequence.slice(0, patch.start) + patch.after + sequence.slice(patch.start + patch.before.length)
  );
}

function reverseSequencePatch(patch) {
  return { start: patch.start, before: patch.after, after: patch.before };
}

/**
 * Undo/redo history. Sequence states after the baseline are stored as
 * reversible patches; annotations remain immutable snapshot references.
 * Entries always represent the state AFTER a mutation.
 */
export function createEditHistory() {
  const stack = [];
  const listeners = new Set();
  let cursor = -1;
  let cache = { index: -1, snapshot: null };

  const notify = () => listeners.forEach((fn) => fn());

  function snapshotAt(target) {
    while (cache.index < target) {
      const entry = stack[cache.index + 1];
      const snapshot = { ...entry };
      if (entry.sequencePatch) {
        snapshot.sequence = applySequencePatch(cache.snapshot.sequence, entry.sequencePatch);
      }
      cache = { index: cache.index + 1, snapshot };
    }
    while (cache.index > target) {
      const entry = stack[cache.index];
      const snapshot = { ...stack[cache.index - 1] };
      if (entry.sequencePatch) {
        snapshot.sequence = applySequencePatch(
          cache.snapshot.sequence,
          reverseSequencePatch(entry.sequencePatch),
        );
      }
      cache = { index: cache.index - 1, snapshot };
    }
    return { ...cache.snapshot };
  }

  return {
    subscribe(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },

    reset(snapshot) {
      stack.length = 0;
      stack.push({ ...snapshot });
      cursor = 0;
      cache = { index: 0, snapshot: { ...snapshot } };
      notify();
    },

    push(snapshot) {
      const current = snapshotAt(cursor);
      stack.splice(cursor + 1);
      const entry = { ...snapshot };
      if (typeof current.sequence === 'string' && typeof snapshot.sequence === 'string') {
        entry.sequencePatch = sequencePatch(current.sequence, snapshot.sequence);
        delete entry.sequence;
      }
      stack.push(entry);
      if (stack.length > MAX_HISTORY) {
        const oldest = snapshotAt(1);
        stack[1] = { ...stack[1], sequence: oldest.sequence };
        delete stack[1].sequencePatch;
        stack.shift();
        cache = { index: 0, snapshot: { ...stack[0] } };
      }
      cursor = stack.length - 1;
      cache = { index: cursor, snapshot: snapshotAt(cursor) };
      notify();
    },

    peekUndo() {
      return cursor > 0 ? snapshotAt(cursor - 1) : null;
    },

    peekRedo() {
      return cursor < stack.length - 1 ? snapshotAt(cursor + 1) : null;
    },

    commitUndo() {
      if (cursor <= 0) return null;
      cursor--;
      const snapshot = snapshotAt(cursor);
      notify();
      return snapshot;
    },

    commitRedo() {
      if (cursor >= stack.length - 1) return null;
      cursor++;
      const snapshot = snapshotAt(cursor);
      notify();
      return snapshot;
    },

    undo() {
      if (cursor <= 0) return null;
      cursor--;
      const snapshot = snapshotAt(cursor);
      notify();
      return snapshot;
    },

    redo() {
      if (cursor >= stack.length - 1) return null;
      cursor++;
      const snapshot = snapshotAt(cursor);
      notify();
      return snapshot;
    },

    canUndo() {
      return cursor > 0;
    },

    canRedo() {
      return cursor < stack.length - 1;
    },

    getCursor() {
      return cursor;
    },

    getLength() {
      return stack.length;
    },
  };
}
