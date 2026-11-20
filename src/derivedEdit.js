/**
 * Map enzyme sites and primer binding sites onto an edited sequence without
 * re-running the backend searches.
 *
 * Same contract as `adjustAnnotations` / `adjustAlignmentsForEdit`: the edit
 * drops the inclusive range `[editStart, editEnd]` (`oldLen` bases) and inserts
 * `newLen` bases at `editStart` (an insertion is `editEnd = editStart - 1`).
 * Anything wholly before the edit is unchanged, anything wholly after shifts by
 * the length delta, and anything the edit touches is dropped — the backend's
 * recomputed copy replaces it when the enzyme refresh lands.
 *
 * Every absolute coordinate of an enzyme site is included in its span (cut
 * pairs too: IIS enzymes cut outside the recognition site), so a site whose
 * recognition is before the edit but whose cut falls after it is dropped
 * instead of being shifted rigidly.
 */

const ENZYME_COORDS = [
  'recStart',
  'recEnd',
  'displayStart',
  'displayEnd',
  'cutIndex',
  'botCutIndex',
];

// Before/after is decided by the recognition site and cuts only: the display
// window is just a tooltip frame around them, and a 1 bp margin overlapping an
// insertion must not drop an otherwise untouched site.
const ENZYME_SPAN_COORDS = ['recStart', 'recEnd', 'cutIndex', 'botCutIndex'];

function enzymeSpan(enzyme) {
  let lo = Infinity;
  let hi = -Infinity;
  const add = (value) => {
    if (typeof value === 'number' && Number.isFinite(value)) {
      if (value < lo) lo = value;
      if (value > hi) hi = value;
    }
  };
  for (const key of ENZYME_SPAN_COORDS) add(enzyme[key]);
  for (const pair of enzyme.cutPairs || []) {
    add(pair.topCutIndex);
    add(pair.botCutIndex);
  }
  return { lo, hi };
}

function shiftEnzyme(enzyme, delta) {
  const next = { ...enzyme };
  for (const key of ENZYME_COORDS) {
    if (typeof next[key] === 'number') next[key] += delta;
  }
  if (Array.isArray(enzyme.cutPairs)) {
    next.cutPairs = enzyme.cutPairs.map((pair) => ({
      ...pair,
      topCutIndex: pair.topCutIndex + delta,
      botCutIndex: pair.botCutIndex + delta,
    }));
  }
  return next;
}

export function adjustEnzymesForEdit(enzymes, editStart, editEnd, oldLen, newLen) {
  if (!Array.isArray(enzymes) || enzymes.length === 0) return enzymes;
  const delta = newLen - oldLen;
  let changed = false;
  const next = [];
  for (const enzyme of enzymes) {
    const { lo, hi } = enzymeSpan(enzyme);
    if (hi < editStart) {
      next.push(enzyme);
      continue;
    }
    changed = true;
    if (lo > editEnd) {
      next.push(delta === 0 ? enzyme : shiftEnzyme(enzyme, delta));
    }
  }
  return changed ? next : enzymes;
}

export function adjustPrimersForEdit(primers, editStart, editEnd, oldLen, newLen) {
  if (!Array.isArray(primers) || primers.length === 0) return primers;
  const delta = newLen - oldLen;
  let changed = false;
  const next = primers.map((primer) => {
    const sites = primer.bindingSites;
    if (!Array.isArray(sites) || sites.length === 0) return primer;
    let sitesChanged = false;
    const kept = [];
    for (const site of sites) {
      const start = site.templateStart;
      if (typeof start !== 'number' || !Number.isFinite(start)) {
        kept.push(site);
        continue;
      }
      // templateEnd is 0-based exclusive, so the last covered base is end - 1.
      const end = (site.templateEnd ?? start + 1) - 1;
      if (end < editStart) {
        kept.push(site);
        continue;
      }
      sitesChanged = true;
      if (start > editEnd) {
        kept.push(
          delta === 0
            ? site
            : {
                ...site,
                templateStart: start + delta,
                templateEnd: site.templateEnd + delta,
              },
        );
      }
    }
    if (!sitesChanged) return primer;
    changed = true;
    return { ...primer, bindingSites: kept };
  });
  return changed ? next : primers;
}
