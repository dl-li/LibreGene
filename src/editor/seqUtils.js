import { complement } from '../editorConstants';

/** Template sequence covered by a primer match (origin-crossing aware). */
export function matchedSeqOf(p, cleanSeq) {
  return (p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }])
    .map((m) => cleanSeq.substring(m.start, m.end + 1))
    .join('');
}

export const complementStr = (s) =>
  s
    .split('')
    .map((c) => complement(c))
    .join('');
export const reverseComplement = (s) => complementStr(s).split('').reverse().join('');

export const isIISEnzyme = (e) => {
  if (!e) return false;
  const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
  return pairs.some((cp) => {
    const dTopRight = Math.max(0, cp.topCutIndex - e.recEnd);
    const dTopLeft = Math.max(0, e.recStart - cp.topCutIndex);
    const dBotRight = Math.max(0, cp.botCutIndex - e.recEnd);
    const dBotLeft = Math.max(0, e.recStart - cp.botCutIndex);
    return Math.max(dTopRight, dTopLeft, dBotRight, dBotLeft) >= 2;
  });
};

export const splitEnzName = (name) => {
  // Italic: everything before the first digit or uppercase letter (beyond position 0)
  let at = name.length;
  for (let i = 1; i < name.length; i++) {
    const c = name[i];
    if ((c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9')) {
      at = i;
      break;
    }
  }
  return { italic: name.slice(0, at), normal: name.slice(at) };
};

// Ensure primer color is a valid non-black hex, falling back to default green
export const safePrimerColor = (c) => {
  if (!c || c === '#000000' || c === '#000' || c === 'black') return '#166534';
  if (/^#[0-9a-f]{6}$/i.test(c)) return c;
  return '#166534';
};

export function truncatedLabel(name, isRev, isFwd, maxLen = 12) {
  const full = isRev ? `< ${name}` : isFwd ? `${name} >` : name;
  if (name.length <= maxLen) return { full, short: full };
  const short = isRev
    ? `< ${name.slice(0, maxLen)}··`
    : isFwd
      ? `${name.slice(0, maxLen)}·· >`
      : `${name.slice(0, maxLen)}··`;
  return { full, short };
}
