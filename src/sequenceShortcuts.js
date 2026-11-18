import { GENETIC_CODE } from './editor/translation';

const DNA_BASES = 'ACGT';
const AMINO_ACIDS = new Set('ACDEFGHIKLMNPQRSTVWY*');
const MAX_RANDOM_BASES = 10000;

const CODONS_BY_AA = Object.entries(GENETIC_CODE).reduce((table, [codon, aa]) => {
  if (!table[aa]) table[aa] = [];
  table[aa].push(codon);
  return table;
}, {});

function randomIndex(length, random) {
  return Math.min(length - 1, Math.floor(random() * length));
}

function randomBases(count, random) {
  return Array.from({ length: count }, () => DNA_BASES[randomIndex(4, random)]).join('');
}

function aminoAcidReplacement(text, random) {
  if (!text || ![...text].every((ch) => AMINO_ACIDS.has(ch.toUpperCase()))) return null;
  return [...text]
    .map((ch) => {
      const codons = CODONS_BY_AA[ch.toUpperCase()];
      return codons[randomIndex(codons.length, random)];
    })
    .join('');
}

function tokenOptions(text, enzymeRecords, random) {
  const options = [];
  const randomMatch = text.match(/^([1-9]\d*)N$/i);
  if (randomMatch) {
    const count = Number(randomMatch[1]);
    if (count <= MAX_RANDOM_BASES) {
      options.push({ kind: 'random', replacement: randomBases(count, random) });
    }
  }

  const enzyme = (enzymeRecords || []).find(
    (record) => record.name?.toLowerCase() === text.toLowerCase(),
  );
  if (enzyme?.site) {
    options.push({ kind: 'enzyme', replacement: enzyme.site.toUpperCase() });
  }

  const codons = aminoAcidReplacement(text, random);
  if (codons) options.push({ kind: 'aminoAcids', replacement: codons });
  return options;
}

export function parseSequenceShortcuts(value, enzymeRecords = [], random = Math.random) {
  const groups = [];
  const groupPattern = /\[([^\]]*)\]/g;
  let match;
  while ((match = groupPattern.exec(value))) {
    const content = match[1];
    const contentStart = match.index + 1;
    const tokens = [];
    let offset = 0;
    for (const part of content.split(',')) {
      const text = part.trim();
      const leading = part.length - part.trimStart().length;
      const start = contentStart + offset + leading;
      tokens.push({
        id: `${groups.length}:${tokens.length}`,
        index: tokens.length,
        text,
        start,
        end: start + text.length,
        options: text ? tokenOptions(text, enzymeRecords, random) : [],
      });
      offset += part.length + 1;
    }
    groups.push({
      id: groups.length,
      start: match.index,
      end: match.index + match[0].length,
      tokens,
      valid: tokens.length > 0 && tokens.every((token) => token.options.length > 0),
    });
  }
  return {
    groups,
    tokens: groups.flatMap((group) =>
      group.tokens
        .filter((token) => token.options.length > 0)
        .map((token) => ({ ...token, group })),
    ),
  };
}

export function replaceSequenceShortcut(value, token, replacement) {
  const { group } = token;
  const before = group.tokens.slice(0, token.index);
  const after = group.tokens.slice(token.index + 1);
  const beforeText = before.length ? `[${before.map((item) => item.text).join(', ')}]` : '';
  const afterText = after.length ? `[${after.map((item) => item.text).join(', ')}]` : '';
  return (
    value.slice(0, group.start) + beforeText + replacement + afterText + value.slice(group.end)
  );
}

export function stripValidSequenceShortcuts(value, groups) {
  let result = '';
  let cursor = 0;
  for (const group of groups) {
    if (!group.valid) continue;
    result += value.slice(cursor, group.start);
    cursor = group.end;
  }
  return result + value.slice(cursor);
}

export function findInvalidSequenceCharacters(value, isValidCharacter, groups) {
  const masked = stripValidSequenceShortcuts(value, groups);
  const invalid = [];
  for (const ch of masked) {
    if (/\s/.test(ch) || isValidCharacter(ch)) continue;
    if (!invalid.includes(ch)) invalid.push(ch);
  }
  return invalid;
}
