import { useState, useRef, useCallback, useEffect } from 'react';

export default function useTabDrag({ windowInfo, orderedProjectsRef, setProjectOrder }) {
  const [dragTabId, setDragTabId] = useState(null);
  const tabDragRef = useRef(null); // { id, dragging }
  const tabDragTimerRef = useRef(null);
  const tabItemRefs = useRef(new Map());
  const suppressTabClickRef = useRef(false);

  const handleTabPointerDown = useCallback(
    (e, id) => {
      if (e.button !== 0 || windowInfo?.type === 'project') return;
      tabDragRef.current = { id, dragging: false };
      clearTimeout(tabDragTimerRef.current);
      tabDragTimerRef.current = setTimeout(() => {
        if (!tabDragRef.current || tabDragRef.current.id !== id) return;
        tabDragRef.current.dragging = true;
        setDragTabId(id);
      }, 300);
    },
    [windowInfo],
  );

  useEffect(() => {
    const moveDragTo = (targetIndex) => {
      const drag = tabDragRef.current;
      if (!drag) return;
      const ids = orderedProjectsRef.current.map((p) => p.id);
      const from = ids.indexOf(drag.id);
      if (from < 0 || from === targetIndex) return;
      const next = [...ids];
      next.splice(from, 1);
      next.splice(targetIndex, 0, drag.id);
      setProjectOrder(next);
      try {
        localStorage.setItem('projectOrder', JSON.stringify(next));
      } catch {
        /* storage full */
      }
    };
    const onPointerMove = (e) => {
      const drag = tabDragRef.current;
      if (!drag || !drag.dragging) return;
      e.preventDefault();
      const ids = orderedProjectsRef.current.map((p) => p.id);
      let target = ids.length - 1;
      for (let i = 0; i < ids.length; i++) {
        const el = tabItemRefs.current.get(ids[i]);
        if (!el) continue;
        const r = el.getBoundingClientRect();
        if (e.clientY < r.top + r.height / 2) {
          target = i;
          break;
        }
      }
      moveDragTo(target);
    };
    const endDrag = () => {
      clearTimeout(tabDragTimerRef.current);
      if (tabDragRef.current?.dragging) {
        suppressTabClickRef.current = true;
        setDragTabId(null);
      }
      tabDragRef.current = null;
    };
    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', endDrag);
    window.addEventListener('pointercancel', endDrag);
    return () => {
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', endDrag);
      window.removeEventListener('pointercancel', endDrag);
    };
  }, []);

  return { dragTabId, tabItemRefs, suppressTabClickRef, handleTabPointerDown };
}
