import alignmentPlugin from './alignment';
import orfPlugin from './orf';
import codonOptimizationPlugin from './codonOptimization';
import rnaFoldPlugin from './rnaFold';
import dotplotPlugin from './dotplot';
import gelPlugin from './gel';
import primerDesignPlugin from './primerDesign';
import blastPlugin from './blast';
import mapPlugin from './map';
import gcContentPlugin from './gcContent';
import snapgeneHistoryPlugin from './snapgeneHistory';

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
