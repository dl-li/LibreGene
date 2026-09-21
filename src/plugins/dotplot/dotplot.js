export const MAX_DOTS = 200000;

export const MIN_WINDOW = 3;
export const MAX_WINDOW = 30;
export const DEFAULT_WINDOW = 9;

// Fold U into T so DNA (T) and RNA (U) bases compare equal; drop non-letters.
export function normalizeSequence(seq) {
  return seq
    .toUpperCase()
    .replace(/[^A-Z]/g, '')
    .replace(/U/g, 'T');
}

const COMPLEMENT = {
  A: 'T',
  T: 'A',
  U: 'A',
  G: 'C',
  C: 'G',
  R: 'Y',
  Y: 'R',
  M: 'K',
  K: 'M',
  B: 'V',
  V: 'B',
  D: 'H',
  H: 'D',
  S: 'S',
  W: 'W',
  N: 'N',
};

export function reverseComplement(seq) {
  let out = '';
  for (let i = seq.length - 1; i >= 0; i--) out += COMPLEMENT[seq[i]] || 'N';
  return out;
}

// Dot-plot via a k-mer index of b: every shared w-mer between a and b becomes
// a dot at its top-left corner. Returns a flat [i, j, i, j, ...] array plus a
// truncated flag once the render cap is hit (caller should tell the user to
// raise the window size).
export function buildDotplot(a, b, w) {
  if (!a || !b || w < 1 || a.length < w || b.length < w) return { dots: [], truncated: false };
  const index = new Map();
  for (let j = 0; j <= b.length - w; j++) {
    const kmer = b.substr(j, w);
    const arr = index.get(kmer);
    if (arr) arr.push(j);
    else index.set(kmer, [j]);
  }
  const dots = [];
  let truncated = false;
  scan: for (let i = 0; i <= a.length - w; i++) {
    const arr = index.get(a.substr(i, w));
    if (!arr) continue;
    for (let j = 0; j < arr.length; j++) {
      dots.push(i, arr[j]);
      if (dots.length >= MAX_DOTS * 2) {
        truncated = true;
        break scan;
      }
    }
  }
  return { dots, truncated };
}

// Round tick spacing (1/2/5 × 10^k) so an axis of length len gets ~8 ticks.
export function tickStep(len, target = 8) {
  if (len <= 0) return 1;
  const raw = len / target;
  const mag = 10 ** Math.floor(Math.log10(raw));
  for (const m of [1, 2, 5, 10]) {
    if (mag * m >= raw) return mag * m;
  }
  return mag * 10;
}
