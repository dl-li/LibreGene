import { describe, it, expect, beforeEach } from 'vitest';
import {
  getRecentFiles,
  addRecentFile,
  removeRecentFile,
  clearRecentFiles,
  getLastOpenedFile,
  fileNameOf,
  dirOf,
} from '../recentFiles';

function stubStorage() {
  let store = new Map();
  globalThis.localStorage = {
    getItem: (k) => (store.has(k) ? store.get(k) : null),
    setItem: (k, v) => store.set(k, String(v)),
    removeItem: (k) => store.delete(k),
    clear: () => store.clear(),
  };
  return store;
}

describe('recentFiles (localStorage persistence)', () => {
  beforeEach(() => {
    stubStorage();
    clearRecentFiles();
  });

  it('starts empty', () => {
    expect(getRecentFiles()).toEqual([]);
    expect(getLastOpenedFile()).toBeNull();
  });

  it('adds newest first and dedupes', () => {
    addRecentFile('/a/a.gbk');
    addRecentFile('/b/b.dna');
    expect(getRecentFiles()).toEqual(['/b/b.dna', '/a/a.gbk']);
    expect(getLastOpenedFile()).toBe('/b/b.dna');

    addRecentFile('/a/a.gbk');
    expect(getRecentFiles()).toEqual(['/a/a.gbk', '/b/b.dna'], 're-opening moves it to front, no dupes');
  });

  it('caps the list at 10 entries', () => {
    for (let i = 0; i < 15; i++) addRecentFile(`/f/file${i}.gbk`);
    const list = getRecentFiles();
    expect(list).toHaveLength(10);
    expect(list[0]).toBe('/f/file14.gbk', 'newest kept');
    expect(list).not.toContain('/f/file0.gbk', 'oldest evicted');
  });

  it('ignores empty path', () => {
    addRecentFile('');
    expect(getRecentFiles()).toEqual([]);
  });

  it('removes and clears', () => {
    addRecentFile('/x.gbk');
    addRecentFile('/y.gbk');
    removeRecentFile('/x.gbk');
    expect(getRecentFiles()).toEqual(['/y.gbk']);
    expect(clearRecentFiles()).toEqual([]);
    expect(getRecentFiles()).toEqual([]);
  });

  it('survives corrupted storage gracefully', () => {
    localStorage.setItem('recentFiles', '{not json');
    expect(getRecentFiles()).toEqual([]);
    addRecentFile('/ok.gbk');
    expect(getRecentFiles()).toEqual(['/ok.gbk']);
  });
});

describe('fileNameOf / dirOf', () => {
  it('extracts file names across separators', () => {
    expect(fileNameOf('C:\\Users\\liu\\pUC.gbk')).toBe('pUC.gbk');
    expect(fileNameOf('/home/liu/pCas9.dna')).toBe('pCas9.dna');
    expect(fileNameOf('plain.fa')).toBe('plain.fa');
    expect(fileNameOf('')).toBe('');
  });

  it('extracts directories', () => {
    expect(dirOf('C:\\Users\\liu\\pUC.gbk')).toBe('C:/Users/liu');
    expect(dirOf('/home/liu/pCas9.dna')).toBe('/home/liu');
    expect(dirOf('plain.fa')).toBe('', 'no directory part');
  });
});
