import { useRef } from 'react';

export default function useMemoCompare(factory, dependencies) {
  const ref = useRef(null);
  const previous = ref.current;
  const changed =
    !previous ||
    previous.dependencies.length !== dependencies.length ||
    dependencies.some((value, index) => !Object.is(value, previous.dependencies[index]));
  if (changed) {
    ref.current = { dependencies, value: factory() };
  }
  return ref.current.value;
}
