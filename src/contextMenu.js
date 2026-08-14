// 全局自定义右键菜单：任意组件调用 showContextMenu，由 ContextMenuHost 统一渲染。
// items: [{ icon?, label, onSelect, disabled? } | { type: 'separator' }]
// Host 会在每个菜单末尾自动追加 Reload 项（开发调试用）。
export const CONTEXT_MENU_EVENT = 'app-context-menu';

export function showContextMenu(x, y, items) {
  window.dispatchEvent(new window.CustomEvent(CONTEXT_MENU_EVENT, { detail: { x, y, items } }));
}
