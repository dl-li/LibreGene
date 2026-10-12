import alignmentPlugin from './builtin/alignment';
import orfPlugin from './builtin/orf';
import codonOptimizationPlugin from './builtin/codonOptimization';
import rnaFoldPlugin from './builtin/rnaFold';
import dotplotPlugin from './example-plugin-dotplot';
import gelPlugin from './example-plugin-gel';
import primerDesignPlugin from './builtin/primerDesign';
import blastPlugin from './builtin/blast';
import mapPlugin from './builtin/map';
import gcContentPlugin from './builtin/gcContent';
import snapgeneHistoryPlugin from './builtin/snapgeneHistory';

export const plugins = [
  mapPlugin,
  alignmentPlugin,
  orfPlugin,
  codonOptimizationPlugin,
  rnaFoldPlugin,
  dotplotPlugin,
  gelPlugin,
  primerDesignPlugin,
  blastPlugin,
  gcContentPlugin,
  snapgeneHistoryPlugin,
];

// Single source of truth for whether a plugin's entries are available:
// disabled state, molecule-type gating (dnaOnly/rnaOnly/notForProtein) and
// the optional dialogVisible predicate. moleculeType may be null while the
// project list is still loading — gated entries stay hidden then.
export function pluginAvailable(
  plugin,
  { moleculeType = null, disabledPlugins = [], projectId = null, isTauri = false } = {},
) {
  if (!plugin) return false;
  if (disabledPlugins.includes(plugin.id)) return false;
  if (plugin.dnaOnly && moleculeType !== 'dna') return false;
  if (plugin.rnaOnly && moleculeType !== 'rna') return false;
  if (plugin.notForProtein && moleculeType === 'protein') return false;
  if (plugin.dialogVisible && !plugin.dialogVisible({ projectId, moleculeType, isTauri }))
    return false;
  return true;
}
