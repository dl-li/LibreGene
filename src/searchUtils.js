const IUPAC_SETS = {
  A: new Set(['A']),
  C: new Set(['C']),
  G: new Set(['G']),
  T: new Set(['T']),
  U: new Set(['T']),
  R: new Set(['A', 'G']),
  Y: new Set(['C', 'T']),
  W: new Set(['A', 'T']),
  S: new Set(['C', 'G']),
  K: new Set(['G', 'T']),
  M: new Set(['A', 'C']),
  B: new Set(['C', 'G', 'T']),
  D: new Set(['A', 'G', 'T']),
  H: new Set(['A', 'C', 'T']),
  V: new Set(['A', 'C', 'G']),
  N: new Set(['A', 'C', 'G', 'T']),
};

const IUPAC_COMPLEMENT = {
  A: 'T',
  T: 'A',
  U: 'A',
  G: 'C',
  C: 'G',
  R: 'Y',
  Y: 'R',
  W: 'W',
  S: 'S',
  K: 'M',
  M: 'K',
  B: 'V',
  D: 'H',
  H: 'D',
  V: 'B',
  N: 'N',
};

export function reverseComplementIupac(seq) {
  let out = '';
  for (let i = seq.length - 1; i >= 0; i--) {
    const c = seq[i].toUpperCase();
    const comp = IUPAC_COMPLEMENT[c];
    if (!comp) return null;
    out += comp;
  }
  return out;
}

function basesOf(char) {
  return IUPAC_SETS[char.toUpperCase()] || null;
}

function setsIntersect(a, b) {
  for (const x of a) if (b.has(x)) return true;
  return false;
}

export function normalizeSeqQuery(query) {
  const q = (query || '').trim().toUpperCase();
  if (!q) return null;
  for (const c of q) if (!basesOf(c)) return null;
  return q;
}

const AA_CODON_PATTERNS = {
  A: 'GCN',
  R: 'MGN',
  N: 'AAY',
  D: 'GAY',
  C: 'TGY',
  Q: 'CAR',
  E: 'GAR',
  G: 'GGN',
  H: 'CAY',
  I: 'ATH',
  L: 'YTN',
  K: 'AAR',
  M: 'ATG',
  F: 'TTY',
  P: 'CCN',
  S: 'WSN',
  T: 'ACN',
  W: 'TGG',
  Y: 'TAY',
  V: 'GTN',
  B: 'RAY',
  Z: 'SAR',
  X: 'NNN',
  '*': 'TRR',
};

// A query that looks like a peptide (contains a letter outside the nucleotide
// IUPAC alphabet, e.g. E/F/I/L/P/Q/*) is treated as one: each residue expands
// to its degenerate IUPAC codon pattern, so the existing scanner matches any
// coding region that translates to it. Pure nucleotide-letter queries stay
// nucleotide searches. Null if the query is not a valid peptide.
export function normalizePeptideQuery(query) {
  const q = (query || '').trim().toUpperCase();
  if (!q) return null;
  let peptideOnly = false;
  let out = '';
  for (const c of q) {
    const p = AA_CODON_PATTERNS[c];
    if (!p) return null;
    if (!basesOf(c)) peptideOnly = true;
    out += p;
  }
  return peptideOnly ? out : null;
}

function scanStrand(seq, pattern, strand, out) {
  const n = seq.length;
  const m = pattern.length;
  if (m === 0 || m > n) return;
  for (let i = 0; i <= n - m; i++) {
    let ok = true;
    for (let j = 0; j < m; j++) {
      const t = basesOf(seq[i + j]);
      if (!t || !setsIntersect(basesOf(pattern[j]), t)) {
        ok = false;
        break;
      }
    }
    if (ok) out.push({ type: 'seq', start: i, end: i + m - 1, strand });
  }
}

const STOP_CODONS = new Set(['TAA', 'TAG', 'TGA']);

export function findSeqMatches(seq, query) {
  const pepPattern = normalizePeptideQuery(query);
  const pattern = pepPattern || normalizeSeqQuery(query);
  // A minimum length keeps single/double-base scans (thousands of hits) out.
  if (!pattern || !seq || pattern.length < 3) return [];
  const out = [];
  scanStrand(seq, pattern, '+', out);
  const rc = reverseComplementIupac(pattern);
  if (rc && rc !== pattern) scanStrand(seq, rc, '-', out);
  // '*' expands to TRR, which also matches TGG (Trp) — drop those false hits
  // by requiring an actual stop codon at every '*' position of the peptide.
  if (pepPattern && query.includes('*')) {
    const q = query.trim().toUpperCase();
    const starOffsets = [];
    for (let i = 0; i < q.length; i++) if (q[i] === '*') starOffsets.push(i * 3);
    if (starOffsets.length > 0) {
      return out.filter((h) =>
        starOffsets.every((off) => {
          const codon = seq.substr(h.start + off, 3).toUpperCase();
          return STOP_CODONS.has(h.strand === '+' ? codon : reverseComplementIupac(codon));
        }),
      );
    }
  }
  return out;
}

const TYPE_ORDER = { seq: 0, feature: 1, enzyme: 2, primer: 3 };

export function buildSearchResults(query, { seq, features, allEnzymes, primers }, scope = 'all') {
  const results = scope === 'all' || scope === 'seq' ? findSeqMatches(seq, query) : [];
  const q = (query || '').trim().toLowerCase();
  const minLen = scope === 'all' ? 3 : 1;
  if (q.length >= minLen) {
    if (scope === 'all' || scope === 'feature')
      for (const f of features || []) {
        if (!f.name || !f.name.toLowerCase().includes(q)) continue;
        const segs =
          f.segments && f.segments.length ? f.segments : [{ start: f.start, end: f.end }];
        let start = Infinity;
        let end = -Infinity;
        for (const s of segs) {
          if (s.start < start) start = s.start;
          if (s.end > end) end = s.end;
        }
        if (start <= end) results.push({ type: 'feature', start, end, ref: f });
      }
    if (scope === 'all' || scope === 'enzyme')
      for (const e of allEnzymes || []) {
        if (!e.name || !e.name.toLowerCase().includes(q)) continue;
        if (e.recStart == null || e.recEnd == null) continue;
        results.push({ type: 'enzyme', start: e.recStart, end: e.recEnd, ref: e });
      }
    if (scope === 'all' || scope === 'primer')
      for (const p of primers || []) {
        if (!p.name || !p.name.toLowerCase().includes(q)) continue;
        const bs = (p.bindingSites || [])[0];
        if (!bs) continue;
        results.push({
          type: 'primer',
          start: bs.templateStart,
          end: bs.templateEnd - 1,
          ref: p,
        });
      }
  }
  results.sort((a, b) => a.start - b.start || TYPE_ORDER[a.type] - TYPE_ORDER[b.type]);
  return results;
}
