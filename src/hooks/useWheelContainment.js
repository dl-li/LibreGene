import { useCallback } from 'react';

// Contain wheel scrolling within the attached subtree: inner scrollable
// areas scroll natively while they can, everything else (blank areas, or a
// scrollable already at its edge) is prevented from leaking the gesture to
// outer scroll containers — which shows up as macOS rubber-banding of the
// whole window. Native listener because React attaches wheel as passive, so
// preventDefault via onWheel is ignored.
//
// Callback ref (not ref + mount effect): the target may render conditionally
// AFTER the host component mounts (e.g. the sidebar only appears once a
// project exists), and an effect with [] would have captured a null node.
export function useWheelContainment() {
  return useCallback((el) => {
    if (!el) return undefined;
    const onWheel = (e) => {
      let node = e.target;
      while (node && node !== el) {
        if (node instanceof HTMLElement) {
          const oy = getComputedStyle(node).overflowY;
          if ((oy === 'auto' || oy === 'scroll') && node.scrollHeight > node.clientHeight) {
            const atTop = node.scrollTop <= 0;
            const atBottom = node.scrollTop + node.clientHeight >= node.scrollHeight - 1;
            if ((e.deltaY < 0 && !atTop) || (e.deltaY > 0 && !atBottom)) return;
          }
        }
        node = node.parentNode;
      }
      e.preventDefault();
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, []);
}
