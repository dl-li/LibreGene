import { describe, expect, it, vi } from 'vitest';
import { nativeSaveExtension, nativeSaveFileName, saveFormatLabel } from '../saveFormats';

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

  it('labels native formats explicitly in the save dialog', () => {
    expect(saveFormatLabel('libregene.gbk')).toBe('GenBank (LibreGene)');
    expect(saveFormatLabel('libregene.gpt')).toBe('GenPept (LibreGene)');
    expect(saveFormatLabel('fa')).toBe('FASTA');
  });

  it.each([
    ['libregene.gbk', 'GenBank (LibreGene)'],
    ['libregene.gpt', 'GenPept (LibreGene)'],
  ])('uses an extension-free macOS default name with the %s native filter', async (ext, label) => {
    vi.resetModules();
    const save = vi.fn().mockResolvedValue(`/tmp/sequence.${ext}`);
    vi.doMock('@tauri-apps/plugin-dialog', () => ({ save }));
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    vi.stubGlobal('navigator', { platform: 'MacIntel' });
    try {
      const { saveFileDialog } = await import('../tauriApi');
      const defaultName = `sequence.${ext}`;
      await expect(saveFileDialog(defaultName, ext)).resolves.toBe(`/tmp/sequence.${ext}`);
      expect(save).toHaveBeenCalledWith({
        title: `Save ${label} file`,
        defaultPath: 'sequence',
        filters: [{ name: label, extensions: [ext] }],
      });
    } finally {
      vi.doUnmock('@tauri-apps/plugin-dialog');
      vi.unstubAllGlobals();
    }
  });

  it('keeps the full default filename on non-macOS platforms', async () => {
    vi.resetModules();
    const save = vi.fn().mockResolvedValue('/tmp/sequence.libregene.gbk');
    vi.doMock('@tauri-apps/plugin-dialog', () => ({ save }));
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    vi.stubGlobal('navigator', { platform: 'Win32' });
    try {
      const { saveFileDialog } = await import('../tauriApi');
      await saveFileDialog('sequence.libregene.gbk', 'libregene.gbk');
      expect(save).toHaveBeenCalledWith(
        expect.objectContaining({ defaultPath: 'sequence.libregene.gbk' }),
      );
    } finally {
      vi.doUnmock('@tauri-apps/plugin-dialog');
      vi.unstubAllGlobals();
    }
  });
});
