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
// a dot at its top-left corner. Each window pair is tested by two independent
// conditions (a window can satisfy both at once): a direct match goes into
// `direct`, a match against the reverse complement of a's window goes into
// `revcomp` (inverted repeats). Both are flat [i, j, i, j, ...] arrays; the
// truncated flag fires once their combined size hits the render cap (caller
// should tell the user to raise the window size).
export function buildDotplot(a, b, w) {
  if (!a || !b || w < 1 || a.length < w || b.length < w)
    return { direct: [], revcomp: [], truncated: false };
  const index = new Map();
  for (let j = 0; j <= b.length - w; j++) {
    const kmer = b.substr(j, w);
    const arr = index.get(kmer);
    if (arr) arr.push(j);
    else index.set(kmer, [j]);
  }
  const direct = [];
  const revcomp = [];
  let truncated = false;
  scan: for (let i = 0; i <= a.length - w; i++) {
    const kmer = a.substr(i, w);
    const arr = index.get(kmer);
    if (arr) {
      for (let j = 0; j < arr.length; j++) {
        direct.push(i, arr[j]);
        if (direct.length + revcomp.length >= MAX_DOTS * 2) {
          truncated = true;
          break scan;
        }
      }
    }
    const rcArr = index.get(reverseComplement(kmer));
    if (rcArr) {
      for (let j = 0; j < rcArr.length; j++) {
        revcomp.push(i, rcArr[j]);
        if (direct.length + revcomp.length >= MAX_DOTS * 2) {
          truncated = true;
          break scan;
        }
      }
    }
  }
  return { direct, revcomp, truncated };
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
