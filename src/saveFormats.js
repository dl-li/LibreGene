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

export function saveFormatLabel(ext) {
  if (ext === 'fa' || ext === 'fasta') return 'FASTA';
  if (ext === 'libregene.gbk') return 'GenBank (LibreGene)';
  if (ext === 'libregene.gpt') return 'GenPept (LibreGene)';
  return ext === 'gpt' ? 'Protein GenBank' : 'GenBank';
}
