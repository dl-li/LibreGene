import { describe, expect, it, vi } from 'vitest';
import {
  ensureMsaSaveExtension,
  ensureNativeSaveExtension,
  isMsaSavePath,
  msaSaveBaseName,
  isNativeSavePath,
  nativeSaveExtension,
  nativeSaveFileName,
  saveFormatLabel,
} from '../saveFormats';

describe('native sequence save formats', () => {
  it('uses LibreGene GenBank and GenPept suffixes by molecule type', () => {
    expect(nativeSaveExtension('dna')).toBe('libregene.gbk');
    expect(nativeSaveExtension('rna')).toBe('libregene.gbk');
    expect(nativeSaveExtension('protein')).toBe('libregene.gpt');
  });

  it('replaces legacy suffixes without duplicating native suffixes', () => {
    expect(nativeSaveFileName('pUC19.gbk', 'dna')).toBe('pUC19.libregene.gbk');
    expect(nativeSaveFileName('pUC19.libregene.gbk', 'dna')).toBe('pUC19.libregene.gbk');
    expect(nativeSaveFileName('pUC19.LIBREGENE.GBK', 'dna')).toBe('pUC19.libregene.gbk');
    expect(nativeSaveFileName('mCherry.gpt', 'protein')).toBe('mCherry.libregene.gpt');
    expect(nativeSaveFileName('mCherry.libregene.gpt', 'protein')).toBe('mCherry.libregene.gpt');
    expect(nativeSaveFileName('Untitled', 'dna')).toBe('Untitled.libregene.gbk');
  });

  it('detects paths that already carry the native suffix', () => {
    expect(isNativeSavePath('/tmp/pUC19.libregene.gbk', 'dna')).toBe(true);
    expect(isNativeSavePath('/tmp/pUC19.LIBREGENE.GBK', 'rna')).toBe(true);
    expect(isNativeSavePath('/tmp/mCherry.libregene.gpt', 'protein')).toBe(true);
    expect(isNativeSavePath('/tmp/pUC19.gbk', 'dna')).toBe(false);
    expect(isNativeSavePath('/tmp/mCherry.gpt', 'protein')).toBe(false);
    expect(isNativeSavePath('/tmp/mCherry.libregene.gpt', 'dna')).toBe(false);
    expect(isNativeSavePath('', 'dna')).toBe(false);
  });

  it('forces the native suffix onto a chosen filename', () => {
    expect(ensureNativeSaveExtension('/tmp/pUC19', 'libregene.gbk')).toBe(
      '/tmp/pUC19.libregene.gbk',
    );
    expect(ensureNativeSaveExtension('/tmp/pUC19.gbk', 'libregene.gbk')).toBe(
      '/tmp/pUC19.libregene.gbk',
    );
    expect(ensureNativeSaveExtension('/tmp/pUC19.gb', 'libregene.gbk')).toBe(
      '/tmp/pUC19.libregene.gbk',
    );
    expect(ensureNativeSaveExtension('/tmp/pUC19.genbank', 'libregene.gbk')).toBe(
      '/tmp/pUC19.libregene.gbk',
    );
    expect(ensureNativeSaveExtension('/tmp/pUC19.LIBREGENE.GBK', 'libregene.gbk')).toBe(
      '/tmp/pUC19.LIBREGENE.GBK',
    );
    expect(ensureNativeSaveExtension('/tmp/mCherry.gpt', 'libregene.gpt')).toBe(
      '/tmp/mCherry.libregene.gpt',
    );
  });

  it('detects canonical MSA save paths', () => {
    expect(isMsaSavePath('/tmp/alignment.msa.fa')).toBe(true);
    expect(isMsaSavePath('/tmp/alignment.MSA.FA')).toBe(true);
    expect(isMsaSavePath('/tmp/alignment.msa.fasta')).toBe(false);
    expect(isMsaSavePath('/tmp/alignment.fa')).toBe(false);
    expect(isMsaSavePath('')).toBe(false);
  });

  it('derives a clean MSA base name from an existing project name', () => {
    expect(msaSaveBaseName('test.msa.fa')).toBe('test');
    expect(msaSaveBaseName('test.msa.fasta')).toBe('test');
    expect(msaSaveBaseName('test.fa')).toBe('test');
    expect(msaSaveBaseName('test')).toBe('test');
    expect(msaSaveBaseName('')).toBe('alignment');
  });

  it('forces the MSA native suffix onto a chosen filename', () => {
    expect(ensureMsaSaveExtension('/tmp/alignment')).toBe('/tmp/alignment.msa.fa');
    expect(ensureMsaSaveExtension('/tmp/alignment.fa')).toBe('/tmp/alignment.msa.fa');
    expect(ensureMsaSaveExtension('/tmp/alignment.fasta')).toBe('/tmp/alignment.msa.fa');
    expect(ensureMsaSaveExtension('/tmp/alignment.msa.fa')).toBe('/tmp/alignment.msa.fa');
    expect(ensureMsaSaveExtension('/tmp/alignment.msa.fasta')).toBe('/tmp/alignment.msa.fa');
    expect(ensureMsaSaveExtension('/tmp/alignment.MSA.FA')).toBe('/tmp/alignment.MSA.FA');
  });

  it('labels native formats explicitly in the save dialog', () => {
    expect(saveFormatLabel('libregene.gbk')).toBe('GenBank (LibreGene)');
    expect(saveFormatLabel('libregene.gpt')).toBe('GenPept (LibreGene)');
    expect(saveFormatLabel('msa.fa')).toBe('Aligned FASTA (LibreGene)');
    expect(saveFormatLabel('fa')).toBe('FASTA');
  });

  it.each([
    ['libregene.gbk', 'GenBank (LibreGene)'],
    ['libregene.gpt', 'GenPept (LibreGene)'],
  ])('prefills the native %s suffix and forces it onto the chosen path', async (ext, label) => {
    vi.resetModules();
    const save = vi.fn().mockResolvedValue('/tmp/sequence');
    vi.doMock('@tauri-apps/plugin-dialog', () => ({ save }));
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    vi.stubGlobal('navigator', { platform: 'MacIntel' });
    try {
      const { saveFileDialog } = await import('../tauriApi');
      await expect(saveFileDialog(`sequence.${ext}`, ext)).resolves.toBe(`/tmp/sequence.${ext}`);
      expect(save).toHaveBeenCalledWith({
        title: `Save ${label} file`,
        defaultPath: `sequence.${ext}`,
        filters: [{ name: label, extensions: [ext] }],
      });
    } finally {
      vi.doUnmock('@tauri-apps/plugin-dialog');
      vi.unstubAllGlobals();
    }
  });

  it('keeps an already-native chosen filename intact on every platform', async () => {
    vi.resetModules();
    const save = vi.fn().mockResolvedValue('/tmp/sequence.libregene.gbk');
    vi.doMock('@tauri-apps/plugin-dialog', () => ({ save }));
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    vi.stubGlobal('navigator', { platform: 'Win32' });
    try {
      const { saveFileDialog } = await import('../tauriApi');
      await expect(saveFileDialog('sequence.libregene.gbk', 'libregene.gbk')).resolves.toBe(
        '/tmp/sequence.libregene.gbk',
      );
      expect(save).toHaveBeenCalledWith(
        expect.objectContaining({ defaultPath: 'sequence.libregene.gbk' }),
      );
    } finally {
      vi.doUnmock('@tauri-apps/plugin-dialog');
      vi.unstubAllGlobals();
    }
  });

  it('prefills and forces the MSA native suffix', async () => {
    vi.resetModules();
    const save = vi.fn().mockResolvedValue('/tmp/alignment');
    vi.doMock('@tauri-apps/plugin-dialog', () => ({ save }));
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    vi.stubGlobal('navigator', { platform: 'MacIntel' });
    try {
      const { saveFileDialog } = await import('../tauriApi');
      await expect(saveFileDialog('alignment.msa.fa', 'msa.fa')).resolves.toBe(
        '/tmp/alignment.msa.fa',
      );
      expect(save).toHaveBeenCalledWith({
        title: 'Save Aligned FASTA (LibreGene) file',
        defaultPath: 'alignment.msa.fa',
        filters: [{ name: 'Aligned FASTA (LibreGene)', extensions: ['msa.fa'] }],
      });
    } finally {
      vi.doUnmock('@tauri-apps/plugin-dialog');
      vi.unstubAllGlobals();
    }
  });

  it('normalizes an MSA filename typed with a plain FASTA suffix', async () => {
    vi.resetModules();
    const save = vi.fn().mockResolvedValue('/tmp/alignment.msa.fasta');
    vi.doMock('@tauri-apps/plugin-dialog', () => ({ save }));
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    vi.stubGlobal('navigator', { platform: 'MacIntel' });
    try {
      const { saveFileDialog } = await import('../tauriApi');
      await expect(saveFileDialog('alignment.msa.fa', 'msa.fa')).resolves.toBe(
        '/tmp/alignment.msa.fa',
      );
    } finally {
      vi.doUnmock('@tauri-apps/plugin-dialog');
      vi.unstubAllGlobals();
    }
  });

  it('leaves plain FASTA exports on the ext-less dialog path', async () => {
    vi.resetModules();
    const save = vi.fn().mockResolvedValue('/tmp/alignment-unaligned.fa');
    vi.doMock('@tauri-apps/plugin-dialog', () => ({ save }));
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    vi.stubGlobal('navigator', { platform: 'MacIntel' });
    try {
      const { saveFileDialog } = await import('../tauriApi');
      await expect(saveFileDialog('alignment-unaligned.fa', 'fa')).resolves.toBe(
        '/tmp/alignment-unaligned.fa',
      );
      expect(save).toHaveBeenCalledWith(
        expect.objectContaining({ defaultPath: 'alignment-unaligned.fa' }),
      );
    } finally {
      vi.doUnmock('@tauri-apps/plugin-dialog');
      vi.unstubAllGlobals();
    }
  });
});
