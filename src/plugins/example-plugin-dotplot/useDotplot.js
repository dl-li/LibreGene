import { useEffect, useRef, useState } from 'react';

const EMPTY = { direct: [], revcomp: [], truncated: false };

export default function useDotplot(sequence, windowSize) {
  const workerRef = useRef(null);
  const requestRef = useRef(0);
  const [result, setResult] = useState(EMPTY);
  const [pending, setPending] = useState(false);

  useEffect(() => {
    const worker = new Worker(new URL('./dotplot.worker.js', import.meta.url), { type: 'module' });
    workerRef.current = worker;
    worker.onmessage = (event) => {
      if (event.data.id !== requestRef.current) return;
      setResult(event.data.result);
      setPending(false);
    };
    worker.onerror = () => {
      setResult(EMPTY);
      setPending(false);
    };
    return () => {
      requestRef.current++;
      worker.terminate();
      workerRef.current = null;
    };
  }, []);

  useEffect(() => {
    const worker = workerRef.current;
    if (!worker || !sequence) {
      setResult(EMPTY);
      setPending(false);
      return;
    }
    const id = ++requestRef.current;
    setPending(true);
    worker.postMessage({ id, sequence, windowSize });
  }, [sequence, windowSize]);

  return { result, pending };
}
