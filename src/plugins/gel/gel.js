// Core in-silico digest + gel electrophoresis simulation. Zero dependencies;
// usable standalone in any JS project. All coordinates here are plain numbers:
// cut positions are 0-based "cut falls between position-1 and position".

export const LADDER_PRESETS = [
  {
    id: 'ladder100',
    label: 'DNA Ladder 100',
    text: '100, 200, 300, 400, *500*, 600, 700, 800, 900, 1000',
  },
  {
    id: 'ladder2000',
    label: 'DNA Ladder 2000',
    text: '100, 250, 500, *750*, 1000, 1500, 2000',
  },
  {
    id: 'ladder5000',
    label: 'DNA Ladder 5000',
    text: '100, 250, 500, 750, *1000*, 1500, 2000, 3000, 5000',
  },
  {
    id: 'ladder15000',
    label: 'DNA Ladder 15000',
    text: '250, 1000, 2500, 5000, 7500, 10000, 15000',
  },
];

export const DEFAULT_LADDER_ID = 'ladder5000';

// Parse "100, 250, *500*, ..." into [{ size, bright }]. Returns
// { bands, error } — error is a string when nothing valid was entered.
export function parseLadderText(text) {
  const bands = [];
  for (const raw of String(text || '').split(/[,;、\s]+/)) {
    if (!raw) continue;
    const bright = raw.startsWith('*') && raw.endsWith('*') && raw.length > 2;
    const num = Number(bright ? raw.slice(1, -1) : raw);
    if (!Number.isFinite(num) || num <= 0) {
      return { bands: [], error: `Invalid band size: "${raw}"` };
    }
    bands.push({ size: Math.round(num), bright });
  }
  bands.sort((a, b) => b.size - a.size);
  if (!bands.length) return { bands: [], error: 'Enter at least one band size' };
  return { bands, error: null };
}

// Group the per-site enzyme list (project data `enzymes`) into per-enzyme
// entries: { name, recSeq, cutType, cuts[] }.
export function groupEnzymeSites(enzymes) {
  const byName = new Map();
  for (const e of enzymes || []) {
    let g = byName.get(e.name);
    if (!g) {
      const cutType =
        e.cutType ||
        (e.botCutIndex === e.cutIndex
          ? 'blunt'
          : e.botCutIndex > e.cutIndex
            ? '5overhang'
            : '3overhang');
      g = { name: e.name, recSeq: e.recSeq || '', cutType, cuts: [], blocked: false };
      byName.set(e.name, g);
    }
    if (!g.cuts.includes(e.cutIndex)) g.cuts.push(e.cutIndex);
    if (e.methylationBlocked) g.blocked = true;
  }
  const groups = [...byName.values()];
  for (const g of groups) g.cuts.sort((a, b) => a - b);
  groups.sort((a, b) => a.name.localeCompare(b.name));
  return groups;
}

// Editor enzyme-filter semantics, mirrored locally so the plugin stays
// self-contained. Values match the host's localStorage `enzymeFilter`.
export const ENZYME_GROUP_OPTIONS = [
  { value: 'all', label: 'All Enzymes' },
  { value: 'unique+twice', label: 'Unique + Twice-cutter' },
  { value: 'unique', label: 'Unique Cutters' },
  { value: 'unique6', label: 'Unique 6 bp' },
  { value: 'twice', label: 'Twice-cutter' },
  { value: 'blunt', label: 'Blunt' },
  { value: 'overhang5', label: "5' Overhang" },
  { value: 'overhang3', label: "3' Overhang" },
  { value: 'rec4', label: '4 bp Recognition' },
  { value: 'rec5', label: '5 bp Recognition' },
  { value: 'rec6', label: '6 bp Recognition' },
  { value: 'rec8p', label: '≥8 bp Recognition' },
];

// `myEnzymes` is a list of enzyme names; when present, a "My Enzymes" group
// is offered first, mirroring the editor's Enzymes menu.
export function enzymeGroupOptions(myEnzymes) {
  const base = [...ENZYME_GROUP_OPTIONS];
  if (myEnzymes?.length) base.splice(1, 0, { value: 'myEnzymes', label: 'My Enzymes' });
  return base;
}

export function enzymeMatchesGroup(g, filter, myEnzymeNames) {
  const n = g.cuts.length;
  const recLen = g.recSeq.length;
  switch (filter) {
    case 'myEnzymes':
      return myEnzymeNames ? myEnzymeNames.has(g.name) : false;
    case 'unique':
      return n === 1;
    case 'unique6':
      return n === 1 && recLen === 6;
    case 'twice':
      return n === 2;
    case 'unique+twice':
      return n === 1 || n === 2;
    case 'blunt':
      return g.cutType === 'blunt';
    case 'overhang5':
      return g.cutType === '5overhang';
    case 'overhang3':
      return g.cutType === '3overhang';
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

// Split enzyme groups into the dialog's three display categories.
export function categorizeEnzymes(groups) {
  const single = [];
  const double = [];
  const other = [];
  for (const g of groups) {
    if (g.cuts.length === 1) single.push(g);
    else if (g.cuts.length === 2) double.push(g);
    else other.push(g);
  }
  return { single, double, other };
}

// Fragments produced by cutting at exactly the given positions.
// cuts: sorted positions in (0, length]; circular wraps the last gap.
export function fragmentsForCuts(length, circular, cuts) {
  if (!cuts.length) return [{ length, topo: circular ? 'circular' : 'linear' }];
  const frags = [];
  for (let i = 0; i < cuts.length; i++) {
    const next = i + 1 < cuts.length ? cuts[i + 1] : circular ? cuts[0] + length : length;
    const len = next - cuts[i];
    if (len > 0) frags.push({ length: len, topo: 'linear' });
  }
  if (!circular && cuts[0] > 0) frags.push({ length: cuts[0], topo: 'linear' });
  return frags;
}

const MAX_ENUM_SITES = 16;

// Simulate a digest. Returns band species:
// [{ length, moles, topo }] where topo is 'supercoiled' | 'nicked' | 'linear'.
// efficiency is the per-site cutting probability; partial digests enumerate
// cut-site subsets (exact up to MAX_ENUM_SITES sites, approximated beyond).
export function digest({ length, circular = true, cuts = [], efficiency = 0.9 }) {
  const e = Math.min(1, Math.max(0, efficiency));
  const uncutSpecies = (moles) =>
    circular ? [{ length, moles, topo: 'supercoiled' }] : [{ length, moles, topo: 'linear' }];

  const uniqueCuts = [...new Set(cuts)].filter((c) => c > 0 && c <= length).sort((a, b) => a - b);
  const n = uniqueCuts.length;
  if (!n) return uncutSpecies(1);

  const acc = new Map(); // key `${length}|${topo}` -> species
  const add = (lengthV, moles, topo) => {
    if (moles <= 1e-9 || lengthV <= 0) return;
    const key = `${lengthV}|${topo}`;
    const cur = acc.get(key);
    if (cur) cur.moles += moles;
    else acc.set(key, { length: lengthV, moles, topo });
  };

  const emitSubset = (maskCount, chosen, prob) => {
    if (!chosen.length) {
      for (const s of uncutSpecies(prob)) add(s.length, s.moles, s.topo);
      return;
    }
    for (const f of fragmentsForCuts(length, circular, chosen)) add(f.length, prob, 'linear');
  };

  if (n <= MAX_ENUM_SITES) {
    const total = 1 << n;
    for (let mask = 0; mask < total; mask++) {
      const chosen = [];
      for (let i = 0; i < n; i++) if (mask & (1 << i)) chosen.push(uniqueCuts[i]);
      const k = chosen.length;
      const prob = Math.pow(e, k) * Math.pow(1 - e, n - k);
      emitSubset(mask, chosen, prob);
    }
  } else {
    // Too many sites to enumerate: full digest dominates; also show the
    // "one site missed" partials, which are the only ones bright enough to see.
    emitSubset(n, uniqueCuts, Math.pow(e, n));
    const pMissOne = Math.pow(e, n - 1) * (1 - e);
    for (let i = 0; i < n; i++) {
      emitSubset(
        n - 1,
        uniqueCuts.filter((_, j) => j !== i),
        pMissOne,
      );
    }
    emitSubset(0, [], Math.pow(1 - e, n));
  }
  return [...acc.values()];
}

// Gel separation range for a matrix: fragments below `lo` run with the front,
// fragments above `hi` barely enter the gel.
export function gelRange(gelType, concPct) {
  if (gelType === 'page') {
    const f = Math.pow(8 / Math.max(3, concPct), 1.5);
    return { hi: 2000 * f, lo: 25 * f };
  }
  const c = Math.max(0.3, concPct);
  return { hi: 20000 * Math.pow(1 / c, 1.1), lo: 150 * Math.pow(1 / c, 1.6) };
}

// Relative mobility 0..1 (1 = runs with the front).
export function mobility(size, range) {
  const s = Math.max(1, size);
  const { hi, lo } = range;
  const m = (Math.log(hi) - Math.log(s)) / (Math.log(hi) - Math.log(lo));
  return Math.min(1, Math.max(0.015, m));
}

// Apparent size of an uncut topoisomer, as a multiple of its true length.
export function topoEffectiveFactor(topo) {
  switch (topo) {
    case 'supercoiled':
      return 0.95;
    case 'nicked':
      return 1.04;
    default:
      return 1;
  }
}

// Default (conservative) run time factor: brightest ladder band reaches ~72%
// of the gel, and the smallest ladder band stays below 96%.
export function defaultTimeFactor(ladderBands, range) {
  const sizes = ladderBands.map((b) => b.size);
  if (!sizes.length) return 1;
  const bright =
    ladderBands.find((b) => b.bright) || ladderBands[Math.floor(ladderBands.length / 2)];
  let t = 0.72 / mobility(bright.size, range);
  const minSize = Math.min(...sizes);
  const tFront = 0.96 / mobility(minSize, range);
  if (t > tFront) t = tFront;
  return Math.max(0.1, t);
}

// Position (0 = wells, 1 = gel bottom) of a band after running at
// timeFactor. Bands past 1 have run off the gel.
export function bandPosition(size, topo, range, timeFactor) {
  const eff = size * topoEffectiveFactor(topo);
  return timeFactor * mobility(eff, range);
}

// Vertical half-width of a band as a fraction of gel height. Diffusion grows
// with run time and is worse for small fragments; topoisomers run wider.
export function bandSpread(size, topo, position, timeFactor) {
  const small = Math.pow(500 / Math.max(120, size), 0.25);
  let sigma = 0.0022 + 0.0045 * Math.sqrt(Math.max(0.1, timeFactor)) * small * (0.15 + position);
  if (topo === 'supercoiled' || topo === 'nicked') sigma *= 1.5;
  return sigma;
}

// Build every band to draw for one lane:
// [{ size, pos, spread, intensity (0..1), topo }]
export function simulateLane({ length, circular, cuts, efficiency, range, timeFactor }) {
  const species = digest({ length, circular, cuts, efficiency });
  let maxMass = 0;
  const bands = [];
  for (const s of species) {
    const pos = bandPosition(s.length, s.topo, range, timeFactor);
    if (pos > 1.02) continue; // ran off the gel
    const mass = s.moles * s.length;
    maxMass = Math.max(maxMass, mass);
    bands.push({
      size: s.length,
      topo: s.topo,
      pos: Math.min(pos, 1.02),
      spread: bandSpread(s.length, s.topo, pos, timeFactor),
      mass,
    });
  }
  for (const b of bands) {
    b.intensity = maxMass > 0 ? Math.max(0.05, b.mass / maxMass) : 0;
    delete b.mass;
  }
  bands.sort((a, b) => a.pos - b.pos);
  return bands;
}

export function formatSizeKb(size) {
  if (size < 1000) return `${size}`;
  const kb = size / 1000;
  return Number.isInteger(kb) ? `${kb}` : kb.toFixed(1);
}
