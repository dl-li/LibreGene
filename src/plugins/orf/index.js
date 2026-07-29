import { BookA } from 'lucide-react';

// ORF Search plugin — scans the sequence itself (ignoring feature annotations)
// for start→stop in-frame ORFs on both strands and displays translations of
// ORFs ≥ MIN_AA as virtual, display-only CDS features (never persisted).

const STOPS = new Set(['TAA', 'TAG', 'TGA']);
const MIN_AA = 75;

// Lighter variants of the primer F/R theme colors (#166534 / #4A148C)
export const ORF_COLORS = { fwd: '#8BB29A', rev: '#A58AC6' };

const COMP = { A: 'T', T: 'A', G: 'C', C: 'G' };

function reverseComplement(seq) {
  let out = '';
  for (let i = seq.length - 1; i >= 0; i--) out += COMP[seq[i]] || 'N';
  return out;
}

// Scan one strand (5'→3' string) for ORFs. For circular sequences the strand is
// scanned twice so ORFs wrapping the origin are found; duplicates from the
// second lap are removed by the caller. Longest ORF per stop codon: the first
// in-frame ATG after the previous stop is used as the start.
function scanStrand(seq, circular) {
  const tlen = seq.length;
  const limit = circular ? tlen * 2 : tlen;
  const orfs = [];
  for (let frame = 0; frame < 3; frame++) {
    let orfStart = -1;
    for (let p = frame; p + 2 < limit; p += 3) {
      const codon = seq[p % tlen] + seq[(p + 1) % tlen] + seq[(p + 2) % tlen];
      if (codon === 'ATG') {
        if (orfStart < 0) orfStart = p;
      } else if (STOPS.has(codon)) {
        if (orfStart >= 0) {
          // orfStart >= tlen means this ORF was already found in the first lap
          // (its lap-2 twin is only re-detected because tlen % 3 shifts frames)
          if ((p - orfStart) / 3 >= MIN_AA && orfStart < tlen) {
            orfs.push({ start: orfStart, end: p + 2 });
          }
          orfStart = -1;
        }
      }
    }
  }
  return orfs;
}

export function findOrfs(sequence, topology = 'circular') {
  if (!sequence || sequence.length < (MIN_AA + 1) * 3) return [];
  const seq = sequence.toUpperCase();
  const tlen = seq.length;
  const circular = topology !== 'linear';
  const seen = new Set();
  const out = [];

  const push = (strand, lo, hi, segments) => {
    const key = `${strand}${lo}:${hi}`;
    if (seen.has(key)) return;
    seen.add(key);
    const color = strand === '+' ? ORF_COLORS.fwd : ORF_COLORS.rev;
    out.push({
      id: `orf-${key}`,
      name: `ORF ${lo + 1}..${hi + 1}`,
      ftype: 'CDS',
      strand,
      start: lo,
      end: hi,
      color,
      segments: segments.map((s) => ({ ...s, color })),
      orf: true,
    });
  };

  for (const o of scanStrand(seq, circular)) {
    const s = o.start % tlen;
    const e = o.end % tlen;
    const segments =
      o.end < tlen
        ? [{ start: s, end: e }]
        : [
            { start: s, end: tlen - 1 },
            { start: 0, end: e },
          ];
    push('+', s, e, segments);
  }

  const rc = reverseComplement(seq);
  for (const o of scanStrand(rc, circular)) {
    // rc index i maps to template index tlen-1-i
    const ts = tlen - 1 - (o.start % tlen); // template position of the 5' end
    const te = tlen - 1 - (o.end % tlen); // template position of the 3' end
    // Segments are ordered so that buildCDSData's minus-strand iteration
    // (last segment first, each end→start) yields 5'→3' coding order.
    const segments =
      o.end < tlen
        ? [{ start: te, end: ts }]
        : [
            { start: te, end: tlen - 1 },
            { start: 0, end: ts },
          ];
    push('-', te, ts, segments);
  }

  return out;
}

export default {
  id: 'orf',
  name: 'ORF Search',
  dialogKey: null,
  // Sidebar entry toggles ORF visibility (handled in ProjectWorkspace, no dialog)
  sidebarItems: [
    { dialogKey: 'orf', label: 'ORF Search', tooltip: 'Toggle ORF display', icon: BookA },
  ],
  dialog: null,
};
