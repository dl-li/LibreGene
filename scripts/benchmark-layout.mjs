import { performance } from 'node:perf_hooks';
import { alignmentInsertUnion, buildStreamLayout } from '../src/editor/alignmentLayout.js';
import { buildSegmentRowIndex, entitiesInRowRange, visibleRowRange } from '../src/editor/rowIndex.js';

const sequenceLength = 100_000;
const features = Array.from({ length: 1000 }, (_, i) => ({
  id: `f${i}`,
  segments: [{ start: (i * 97) % sequenceLength, end: Math.min(sequenceLength - 1, (i * 97) % sequenceLength + 300) }],
}));
const primers = Array.from({ length: 200 }, (_, i) => ({
  id: `p${i}`,
  matchSegs: [{ start: (i * 431) % sequenceLength, end: Math.min(sequenceLength - 1, (i * 431) % sequenceLength + 24) }],
}));
const alignments = Array.from({ length: 50 }, (_, i) => ({
  insertions: [{ pos: (i * 1999) % sequenceLength, bases: 'A'.repeat((i % 5) + 1) }],
}));
const rowOf = (position) => Math.floor(position / 60);

function benchmark(name, iterations, run) {
  run();
  const started = performance.now();
  for (let i = 0; i < iterations; i++) run(i);
  const elapsed = performance.now() - started;
  return { name, iterations, totalMs: Number(elapsed.toFixed(2)), meanMs: Number((elapsed / iterations).toFixed(3)) };
}

const results = [];
results.push(benchmark('stream layout: 100 kb + 50 alignments', 10, () => {
  buildStreamLayout(alignmentInsertUnion(alignments, sequenceLength), sequenceLength, 60);
}));
const featureIndex = buildSegmentRowIndex(features, rowOf);
const primerIndex = buildSegmentRowIndex(primers, rowOf, (p) => p.matchSegs);
results.push(benchmark('feature row index: 1000 features', 20, () => {
  buildSegmentRowIndex(features, rowOf);
}));
results.push(benchmark('primer row index: 200 primers', 20, () => {
  buildSegmentRowIndex(primers, rowOf, (p) => p.matchSegs);
}));
results.push(benchmark('visible feature lookup: all rows', 20, () => {
  for (let row = 0; row < Math.ceil(sequenceLength / 60); row++) {
    entitiesInRowRange(featureIndex, row * 60, row * 60 + 59, rowOf);
  }
}));
const rowY = Array.from({ length: Math.ceil(sequenceLength / 60) }, (_, i) => 60 + i * 80);
const above = rowY.map(() => 30);
const below = rowY.map(() => 30);
results.push(benchmark('binary visible row lookup: 10000 positions', 20, () => {
  for (let i = 0; i < 10000; i++) visibleRowRange(rowY, above, below, i * 13, 900);
}));
console.log(JSON.stringify({ sequenceLength, features: features.length, primers: primers.length, alignments: alignments.length, results }, null, 2));
