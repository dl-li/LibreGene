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

export function findSeqMatches(seq, query) {
  const pattern = normalizeSeqQuery(query);
  if (!pattern || !seq) return [];
  const out = [];
  scanStrand(seq, pattern, '+', out);
  const rc = reverseComplementIupac(pattern);
  if (rc && rc !== pattern) scanStrand(seq, rc, '-', out);
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
        const segs = f.segments && f.segments.length ? f.segments : [{ start: f.start, end: f.end }];
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
