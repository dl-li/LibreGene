export function nativeSaveExtension(moleculeType) {
  return moleculeType === 'protein' ? 'libregene.gpt' : 'libregene.gbk';
}

export function nativeSaveFileName(rawName, moleculeType) {
  const suffix = `.${nativeSaveExtension(moleculeType)}`;
  const base = rawName.toLowerCase().endsWith(suffix)
    ? rawName.slice(0, -suffix.length)
    : rawName.replace(/\.[^.]+$/, '');
  return `${base}${suffix}`;
}

const LEGACY_GENBANK_SUFFIX = /(?:\.libregene)?\.(?:gbk|gb|genbank|gbf|gbff|gpt|gp|gpe|gpff)$/i;

export function ensureNativeSaveExtension(path, ext = 'libregene.gbk') {
  if (!path) return path;
  const suffix = `.${ext}`;
  if (path.toLowerCase().endsWith(suffix)) return path;
  const base = ext.startsWith('libregene.') ? path.replace(LEGACY_GENBANK_SUFFIX, '') : path;
  return `${base}${suffix}`;
}

export function isNativeSavePath(path, moleculeType) {
  if (!path) return false;
  return path.toLowerCase().endsWith(`.${nativeSaveExtension(moleculeType)}`);
}

export const MSA_SAVE_EXTENSION = 'msa.fa';

export function isMsaSavePath(path) {
  return !!path && path.toLowerCase().endsWith(`.${MSA_SAVE_EXTENSION}`);
}

export function ensureMsaSaveExtension(path) {
  if (!path) return path;
  if (isMsaSavePath(path)) return path;
  return `${path.replace(/(?:\.msa)?\.fa(?:sta)?$/i, '')}.${MSA_SAVE_EXTENSION}`;
}

export function msaSaveBaseName(rawName) {
  return (rawName || 'alignment')
    .replace(/\.msa\.fasta$/i, '')
    .replace(/\.msa\.fa$/i, '')
    .replace(/\.[^.]+$/, '');
}

export function saveFormatLabel(ext) {
  if (ext === 'fa' || ext === 'fasta') return 'FASTA';
  if (ext === 'msa.fa' || ext === 'msa.fasta') return 'Aligned FASTA (LibreGene)';
  if (ext === 'libregene.gbk') return 'GenBank (LibreGene)';
  if (ext === 'libregene.gpt') return 'GenPept (LibreGene)';
  return ext === 'gpt' ? 'Protein GenBank' : 'GenBank';
}
