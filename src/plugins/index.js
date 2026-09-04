import alignmentPlugin from './alignment';
import orfPlugin from './orf';
import codonOptimizationPlugin from './codonOptimization';
import rnaFoldPlugin from './rnaFold';
import primerDesignPlugin from './primerDesign';
import blastPlugin from './blast';
import mapPlugin from './map';

export const plugins = [
  mapPlugin,
  alignmentPlugin,
  orfPlugin,
  codonOptimizationPlugin,
  rnaFoldPlugin,
  primerDesignPlugin,
  blastPlugin,
];
