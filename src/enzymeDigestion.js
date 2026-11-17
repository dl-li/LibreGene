import { isIISEnzyme } from './editor/seqUtils';

export const ENZYME_FILTER_OPTIONS = [
  { value: 'all', label: 'All Enzymes' },
  { value: 'unique+twice', label: 'Highlighted Cutters' },
  { value: 'unique', label: 'Unique Cutters' },
  { value: 'unique6', label: 'Unique 6 bp' },
  { value: 'twice', label: 'Twice-cutter' },
  { value: 'blunt', label: 'Blunt', className: 'bg-amber-800/10' },
  { value: 'overhang5', label: "5' Overhang" },
  { value: 'overhang3', label: "3' Overhang" },
  { value: 'iis', label: 'Type IIS', className: 'bg-teal-700/10' },
  { value: 'rec4', label: '4 bp Recognition' },
  { value: 'rec5', label: '5 bp Recognition' },
  { value: 'rec6', label: '6 bp Recognition' },
  { value: 'rec8p', label: '≥8 bp Recognition' },
];

const nameKey = (name) => (name || '').toLowerCase();

function cutTypeOf(enzyme) {
  return (
    enzyme.cutType ||
    (enzyme.botCutIndex === enzyme.cutIndex
      ? 'blunt'
      : enzyme.botCutIndex > enzyme.cutIndex
        ? '5overhang'
        : '3overhang')
  );
}

function recordIsIIS(record) {
  const recLen = (record.site || '').length;
  const offsets = [record.fst5, recLen + (record.fst3 || 0)];
  if (record.scd5 != null && record.scd3 != null) {
    offsets.push(record.scd5, recLen + record.scd3);
  }
  return offsets.some((offset) => offset <= -2 || offset >= recLen + 1);
}

function groupProjectEnzymes(enzymes) {
  const groups = new Map();
  for (const enzyme of enzymes || []) {
    const key = nameKey(enzyme.name);
    let group = groups.get(key);
    if (!group) {
      group = {
        name: enzyme.name,
        recSeq: enzyme.recSeq || '',
        cutType: cutTypeOf(enzyme),
        cuts: [],
        pairCount: 0,
        iis: false,
        gray: false,
      };
      groups.set(key, group);
    }
    if (
      enzyme.methylationBlocked ||
      (enzyme.methylationRequired && enzyme.methylRequiredSources?.length)
    ) {
      group.gray = true;
    }
    const pairs = enzyme.cutPairs?.length
      ? enzyme.cutPairs
      : [{ topCutIndex: enzyme.cutIndex, botCutIndex: enzyme.botCutIndex }];
    group.pairCount += pairs.length;
    for (const pair of pairs) {
      if (Number.isFinite(pair.topCutIndex) && !group.cuts.includes(pair.topCutIndex)) {
        group.cuts.push(pair.topCutIndex);
      }
    }
    if (isIISEnzyme(enzyme)) group.iis = true;
  }
  for (const group of groups.values()) group.cuts.sort((a, b) => a - b);
  return groups;
}

export function enzymeMatchesDigestionGroup(group, filter, myEnzymeNames) {
  const recLen = group.recSeq.length;
  switch (filter) {
    case 'myEnzymes':
      return myEnzymeNames.has(nameKey(group.name));
    case 'unique':
      return group.pairCount === 1;
    case 'unique6':
      return group.pairCount === 1 && recLen === 6;
    case 'twice':
      return group.pairCount === 2;
    case 'unique+twice':
      return group.pairCount === 1 || group.pairCount === 2;
    case 'blunt':
      return group.cutType === 'blunt';
    case 'overhang5':
      return group.cutType === '5overhang';
    case 'overhang3':
      return group.cutType === '3overhang';
    case 'iis':
      return group.iis;
    case 'rec4':
      return recLen === 4;
    case 'rec5':
      return recLen === 5;
    case 'rec6':
      return recLen === 6;
    case 'rec8p':
      return recLen >= 8;
    default:
      return true;
  }
}

export function buildDigestionRows(records, enzymes, filter, myEnzymes = []) {
  const projectGroups = groupProjectEnzymes(enzymes);
  const rows = new Map();
  for (const record of records || []) {
    const key = nameKey(record.name);
    const project = projectGroups.get(key);
    rows.set(key, {
      name: record.name,
      recSeq: record.site || project?.recSeq || '',
      cutType: record.cutType || project?.cutType || '',
      cuts: project?.cuts || [],
      pairCount: project?.pairCount || 0,
      iis: recordIsIIS(record) || !!project?.iis,
      gray: !!project?.gray,
    });
    if (project) projectGroups.delete(key);
  }
  for (const [key, project] of projectGroups) rows.set(key, project);

  const myEnzymeNames = new Set((myEnzymes || []).map(nameKey));
  return [...rows.values()].filter((row) =>
    enzymeMatchesDigestionGroup(row, filter, myEnzymeNames),
  );
}

export function sortDigestionRows(rows, sortBy) {
  const sorted = [...rows];
  sorted.sort((a, b) => {
    const byName = a.name.localeCompare(b.name);
    return sortBy === 'cuts' ? a.cuts.length - b.cuts.length || byName : byName;
  });
  return sorted;
}

export function formatDigestionCutPosition(cutIndex, sequenceLength) {
  if (cutIndex === 0 && sequenceLength > 0) return `${sequenceLength}^1`;
  return `${cutIndex}^${cutIndex + 1}`;
}
