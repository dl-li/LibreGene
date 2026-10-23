import { makeProteinTrackPlugin } from './track';
import {
  CHARGE_COLORS,
  HYDROPHOBICITY_COLORS,
  POLARITY_COLORS,
  SECONDARY_STRUCTURE_COLORS,
  SIZE_COLORS,
} from './scales';

// Protein property tracks: five independently toggleable per-residue color
// bands (View menu), each encoding a single physicochemical property. Legend
// colors are sampled from the scales so the menu can never drift from the
// track.
export const proteinPropertyPlugins = [
  makeProteinTrackPlugin({
    id: 'proteinHydrophobicity',
    name: 'Protein Hydrophobicity',
    description:
      'Per-residue hydrophobicity track (Kyte-Doolittle): orange = hydrophobic, gray = neutral, blue = hydrophilic',
    menuLabel: 'Show Hydrophobicity',
    scale: HYDROPHOBICITY_COLORS,
    legend: [
      { color: HYDROPHOBICITY_COLORS.I, label: 'Hydrophobic' },
      { color: HYDROPHOBICITY_COLORS.G, label: 'Intermediate' },
      { color: HYDROPHOBICITY_COLORS.R, label: 'Hydrophilic' },
    ],
  }),
  makeProteinTrackPlugin({
    id: 'proteinCharge',
    name: 'Protein Charge',
    description:
      'Per-residue charge track: blue = positive (K/R/H), red = negative (D/E), gray = neutral',
    menuLabel: 'Show Charge',
    scale: CHARGE_COLORS,
    legend: [
      { color: CHARGE_COLORS.K, label: 'Positive (K R H)' },
      { color: CHARGE_COLORS.D, label: 'Negative (D E)' },
      { color: CHARGE_COLORS.A, label: 'Neutral' },
    ],
  }),
  makeProteinTrackPlugin({
    id: 'proteinPolarity',
    name: 'Protein Polarity',
    description: 'Per-residue polarity track: green = polar, sand = nonpolar',
    menuLabel: 'Show Polarity',
    scale: POLARITY_COLORS,
    legend: [
      { color: POLARITY_COLORS.S, label: 'Polar' },
      { color: POLARITY_COLORS.A, label: 'Nonpolar' },
    ],
  }),
  makeProteinTrackPlugin({
    id: 'proteinSize',
    name: 'Protein Residue Size',
    description:
      'Per-residue size track, light → dark purple: tiny (G/A/S), small (C/D/N/P/T), medium (V/E/Q/H/I/L/K/M), bulky (F/R/Y/W)',
    menuLabel: 'Show Residue Size',
    scale: SIZE_COLORS,
    legend: [
      { color: SIZE_COLORS.G, label: 'Tiny (G A S)' },
      { color: SIZE_COLORS.C, label: 'Small (C D N P T)' },
      { color: SIZE_COLORS.V, label: 'Medium' },
      { color: SIZE_COLORS.W, label: 'Bulky (F R Y W)' },
    ],
  }),
  makeProteinTrackPlugin({
    id: 'proteinSecondaryStructure',
    name: 'Protein Secondary Structure',
    description:
      'Per-residue secondary-structure propensity track (Chou-Fasman): magenta = helix-favoring, yellow = sheet-favoring, gray = breaker/coil',
    menuLabel: 'Show Secondary Structure Propensity',
    scale: SECONDARY_STRUCTURE_COLORS,
    legend: [
      { color: SECONDARY_STRUCTURE_COLORS.A, label: 'α-helix favoring' },
      { color: SECONDARY_STRUCTURE_COLORS.V, label: 'β-sheet favoring' },
      { color: SECONDARY_STRUCTURE_COLORS.G, label: 'Breaker / coil' },
    ],
  }),
];
