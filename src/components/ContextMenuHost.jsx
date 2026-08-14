import { useState, useEffect, useLayoutEffect, useRef, useCallback } from 'react';
import { RotateCw } from 'lucide-react';
import { CONTEXT_MENU_EVENT } from '@/contextMenu';
import { cn } from '@/lib/utils';

// 开发调试用：每个菜单末尾固定附带 Reload
const RELOAD_ITEM = { icon: RotateCw, label: 'Reload', onSelect: () => window.location.reload() };

export default function ContextMenuHost() {
  const [menu, setMenu] = useState(null); // { x, y, items }
  const [pos, setPos] = useState(null);
  const menuRef = useRef(null);

  const close = useCallback(() => setMenu(null), []);

  useEffect(() => {
    const onCustom = (e) => {
      const { x, y, items } = e.detail || {};
      setPos(null);
      setMenu({ x, y, items: items || [] });
    };
    const onNativeContextMenu = (e) => {
      // 输入框/文本域/可编辑区域保留原生菜单
      if (e.target.closest?.('input, textarea, [contenteditable="true"]')) return;
      // 具体组件已通过 showContextMenu 处理（preventDefault + stopPropagation）
      if (e.defaultPrevented) return;
      e.preventDefault();
      // 其余区域只给 Reload（开发用）
      setPos(null);
      setMenu({ x: e.clientX, y: e.clientY, items: [] });
    };
    window.addEventListener(CONTEXT_MENU_EVENT, onCustom);
    window.addEventListener('contextmenu', onNativeContextMenu);
    return () => {
      window.removeEventListener(CONTEXT_MENU_EVENT, onCustom);
      window.removeEventListener('contextmenu', onNativeContextMenu);
    };
  }, []);

  useEffect(() => {
    if (!menu) return;
    const onDown = (e) => {
      if (menuRef.current?.contains(e.target)) return;
      close();
    };
    const onKey = (e) => {
      if (e.key === 'Escape') close();
    };
    window.addEventListener('mousedown', onDown, true);
    window.addEventListener('keydown', onKey);
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    window.addEventListener('blur', close);
    return () => {
      window.removeEventListener('mousedown', onDown, true);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
      window.removeEventListener('blur', close);
    };
  }, [menu, close]);

  // 打开后按实际尺寸钳位到视口内
  useLayoutEffect(() => {
    if (!menu || !menuRef.current) return;
    const rect = menuRef.current.getBoundingClientRect();
    setPos({
      x: Math.max(4, Math.min(menu.x, window.innerWidth - rect.width - 4)),
      y: Math.max(4, Math.min(menu.y, window.innerHeight - rect.height - 4)),
    });
  }, [menu]);

  if (!menu) return null;

  const items = [...menu.items];
  if (items.length) items.push({ type: 'separator' });
  items.push(RELOAD_ITEM);

  return (
    <div
      ref={menuRef}
      className="nav-menu-content fixed z-[100] min-w-[11rem] rounded-lg border border-border/60 bg-popover p-1 text-popover-foreground shadow-lg"
      style={{
        left: pos?.x ?? menu.x,
        top: pos?.y ?? menu.y,
        visibility: pos ? 'visible' : 'hidden',
      }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item, i) =>
        item.type === 'separator' ? (
          <div key={i} className="-mx-1 my-1 h-px bg-border/60" />
        ) : (
          <button
            key={i}
            type="button"
            disabled={item.disabled}
            className={cn(
              'flex w-full cursor-default select-none items-center gap-2 rounded-md px-2 py-1.5 text-sm outline-none transition-colors hover:bg-accent hover:text-accent-foreground disabled:pointer-events-none disabled:opacity-30 [&_svg]:size-4 [&_svg]:shrink-0',
              item.bold && 'font-bold',
              item.danger && 'text-destructive',
            )}
            onClick={() => {
              close();
              item.onSelect?.();
            }}
          >
            {item.icon && <item.icon />}
            <span>{item.label}</span>
          </button>
        ),
      )}
    </div>
  );
}
