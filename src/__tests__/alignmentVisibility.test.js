import { beforeEach, describe, expect, it } from 'vitest';
import {
  copyAlignmentVisibility,
  readHiddenAlnNames,
  readShowAlignments,
  shouldRevealAlignments,
  writeHiddenAlnNames,
  writeShowAlignments,
} from '../workspace/hiddenAlignments';

function stubStorage() {
  const store = new Map();
  globalThis.localStorage = {
    getItem: (key) => (store.has(key) ? store.get(key) : null),
    setItem: (key, value) => store.set(key, String(value)),
    removeItem: (key) => store.delete(key),
    clear: () => store.clear(),
  };
}

describe('alignment visibility persistence', () => {
  beforeEach(() => {
    stubStorage();
  });

  it('defaults to showing all alignments', () => {
    expect(readShowAlignments('/project.gbk')).toBe(true);
    expect(readHiddenAlnNames('/project.gbk')).toEqual([]);
  });

  it('persists overall and per-track visibility independently by project', () => {
    writeShowAlignments('/a.gbk', false);
    writeHiddenAlnNames('/a.gbk', ['read 1', 'read 2']);
    writeShowAlignments('/b.gbk', true);
    writeHiddenAlnNames('/b.gbk', ['other read']);

    expect(readShowAlignments('/a.gbk')).toBe(false);
    expect(readHiddenAlnNames('/a.gbk')).toEqual(['read 1', 'read 2']);
    expect(readShowAlignments('/b.gbk')).toBe(true);
    expect(readHiddenAlnNames('/b.gbk')).toEqual(['other read']);

    writeShowAlignments('/a.gbk', true);
    writeHiddenAlnNames('/a.gbk', []);
    expect(readShowAlignments('/a.gbk')).toBe(true);
    expect(readHiddenAlnNames('/a.gbk')).toEqual([]);
  });

  it('copies both kinds of visibility to a Save As path', () => {
    writeShowAlignments('untitled-1', false);
    writeHiddenAlnNames('untitled-1', ['hidden read']);
    writeShowAlignments('/saved.gbk', true);
    writeHiddenAlnNames('/saved.gbk', ['stale read']);

    copyAlignmentVisibility('untitled-1', '/saved.gbk');

    expect(readShowAlignments('/saved.gbk')).toBe(false);
    expect(readHiddenAlnNames('/saved.gbk')).toEqual(['hidden read']);
    expect(readShowAlignments('untitled-1')).toBe(false);
    expect(readHiddenAlnNames('untitled-1')).toEqual(['hidden read']);
  });

  it('recovers from malformed storage', () => {
    localStorage.setItem('showAlignments', '{bad json');
    localStorage.setItem('hiddenAlignments', 'null');

    expect(readShowAlignments('/a.gbk')).toBe(true);
    expect(readHiddenAlnNames('/a.gbk')).toEqual([]);
    writeShowAlignments('/a.gbk', false);
    writeHiddenAlnNames('/a.gbk', ['read 1']);
    expect(readShowAlignments('/a.gbk')).toBe(false);
    expect(readHiddenAlnNames('/a.gbk')).toEqual(['read 1']);
  });
});

describe('overall alignment visibility', () => {
  const alignments = [{ id: 1 }, { id: 2 }];

  it('reveals every track when showing alignments with all tracks hidden', () => {
    expect(shouldRevealAlignments(false, alignments, [1, 2])).toBe(true);
  });

  it('keeps per-track visibility independent when any track is visible', () => {
    expect(shouldRevealAlignments(false, alignments, [1])).toBe(false);
    expect(shouldRevealAlignments(false, alignments, [])).toBe(false);
  });

  it('does not change track visibility when hiding all alignments', () => {
    expect(shouldRevealAlignments(true, alignments, [1, 2])).toBe(false);
  });

  it('does not reveal tracks for a project without alignments', () => {
    expect(shouldRevealAlignments(false, [], [])).toBe(false);
  });
});
