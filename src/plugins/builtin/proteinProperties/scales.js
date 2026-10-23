// Per-residue color tables for the protein property tracks. Each scale maps
// an uppercase one-letter code to a fill color; non-standard residues fall
// back to a uniform neutral gray.
export const AA_UNKNOWN_COLOR = '#F0F0F0';

// Kyte-Doolittle hydropathy in three tiers: hydrophobic KD ≥ 1.0 (orange,
// "oil"), intermediate |KD| < 1.0 (gray), hydrophilic KD ≤ -1.0 (blue,
// "water").
export const HYDROPHOBICITY_COLORS = {
  I: '#E8833A',
  V: '#E8833A',
  L: '#E8833A',
  F: '#E8833A',
  C: '#E8833A',
  M: '#E8833A',
  A: '#E8833A',
  G: '#E8E8E8',
  T: '#E8E8E8',
  S: '#E8E8E8',
  W: '#E8E8E8',
  Y: '#3A7BD5',
  P: '#3A7BD5',
  H: '#3A7BD5',
  E: '#3A7BD5',
  Q: '#3A7BD5',
  D: '#3A7BD5',
  N: '#3A7BD5',
  K: '#3A7BD5',
  R: '#3A7BD5',
};

// Charge at physiological pH: positive (blue, basic) / negative (red,
// acidic) / neutral (gray).
export const CHARGE_COLORS = {
  K: '#3B6FD4',
  R: '#3B6FD4',
  H: '#3B6FD4',
  D: '#D43B3B',
  E: '#D43B3B',
  A: '#E8E8E8',
  V: '#E8E8E8',
  I: '#E8E8E8',
  L: '#E8E8E8',
  M: '#E8E8E8',
  F: '#E8E8E8',
  W: '#E8E8E8',
  P: '#E8E8E8',
  G: '#E8E8E8',
  S: '#E8E8E8',
  T: '#E8E8E8',
  N: '#E8E8E8',
  Q: '#E8E8E8',
  Y: '#E8E8E8',
  C: '#E8E8E8',
};

// Polarity: polar (green, includes the charged residues) / nonpolar (sand).
export const POLARITY_COLORS = {
  S: '#3FA34D',
  T: '#3FA34D',
  N: '#3FA34D',
  Q: '#3FA34D',
  Y: '#3FA34D',
  C: '#3FA34D',
  K: '#3FA34D',
  R: '#3FA34D',
  H: '#3FA34D',
  D: '#3FA34D',
  E: '#3FA34D',
  A: '#C9B458',
  V: '#C9B458',
  I: '#C9B458',
  L: '#C9B458',
  M: '#C9B458',
  F: '#C9B458',
  W: '#C9B458',
  P: '#C9B458',
  G: '#C9B458',
};

// Residue bulk by molecular weight, four tiers light → dark purple:
// tiny GAS / small CDNPT / medium VEQHILKM / bulky FRYW.
export const SIZE_COLORS = {
  G: '#DCD0F0',
  A: '#DCD0F0',
  S: '#DCD0F0',
  C: '#B39DDB',
  D: '#B39DDB',
  N: '#B39DDB',
  P: '#B39DDB',
  T: '#B39DDB',
  V: '#7E57C2',
  E: '#7E57C2',
  Q: '#7E57C2',
  H: '#7E57C2',
  I: '#7E57C2',
  L: '#7E57C2',
  K: '#7E57C2',
  M: '#7E57C2',
  F: '#4A2F8F',
  R: '#4A2F8F',
  Y: '#4A2F8F',
  W: '#4A2F8F',
};

// Chou–Fasman-style secondary-structure propensity: helix-favoring (magenta),
// sheet-favoring (yellow), breakers/coil (gray).
export const SECONDARY_STRUCTURE_COLORS = {
  E: '#D43B6E',
  A: '#D43B6E',
  L: '#D43B6E',
  M: '#D43B6E',
  Q: '#D43B6E',
  K: '#D43B6E',
  H: '#D43B6E',
  R: '#D43B6E',
  V: '#E8C53A',
  I: '#E8C53A',
  Y: '#E8C53A',
  F: '#E8C53A',
  W: '#E8C53A',
  T: '#E8C53A',
  C: '#E8C53A',
  G: '#9E9E9E',
  P: '#9E9E9E',
  N: '#9E9E9E',
  D: '#9E9E9E',
  S: '#9E9E9E',
};

export const aaColor = (scale, ch) => scale[ch.toUpperCase()] || AA_UNKNOWN_COLOR;
