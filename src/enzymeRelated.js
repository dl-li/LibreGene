// Related-enzyme lookup (same recognition site / compatible overhang) against
// the enzyme database returned by get_enzyme_database.

function cutInfo(rec) {
  if (!rec || !rec.site || rec.isCutTwice) return null;
  const site = String(rec.site).toUpperCase();
  const oStart = Math.max(0, rec.fst5 ?? 0);
  const oEnd = Math.min(site.length, site.length + (rec.fst3 ?? 0));
  const overhang = site.slice(oStart, oEnd);
  // Degenerate bases in the overhang make the resulting end sequence
  // indeterminate; blunt enzymes have an empty overhang and stay determinate.
  if (/[^ACGT]/.test(overhang)) return null;
  const key = rec.cutType === 'blunt' ? 'blunt' : `${rec.cutType}:${overhang}`;
  return { site, key };
}

export function getRelatedEnzymes(targetName, currentEnzymeNames, dbRecords) {
  const target = (dbRecords || []).find((r) => r.name === targetName);
  const tInfo = cutInfo(target);
  if (!tInfo) return null;
  const inSet = new Set(currentEnzymeNames);
  const isocaudomers = new Set();
  const isoschizomers = new Set();
  for (const r of dbRecords || []) {
    if (r.name === targetName || !inSet.has(r.name)) continue;
    const info = cutInfo(r);
    if (!info) continue;
    if (info.site === tInfo.site) {
      isoschizomers.add(r.name);
    } else if (info.key === tInfo.key) {
      isocaudomers.add(r.name);
    }
  }
  return {
    isocaudomers: [...isocaudomers].sort(),
    isoschizomers: [...isoschizomers].sort(),
  };
}
