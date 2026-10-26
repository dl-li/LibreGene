import { buildDotplot } from './dotplot';

self.onmessage = (event) => {
  const { id, sequence, windowSize } = event.data;
  const result = buildDotplot(sequence, sequence, windowSize);
  self.postMessage({ id, result });
};
