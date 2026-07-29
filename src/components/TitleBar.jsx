import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { isTauri } from '@/tauriApi';

export default function TitleBar({ title, dirty = false }) {
  useEffect(() => {
    if (!isTauri) return undefined;
    invoke('activate_custom_titlebar').catch(() => {
      invoke('restore_native_titlebar').catch(() => {});
    });

    // If activation hangs or the plugin never marks its root element, fall back
    const timer = setTimeout(() => {
      const active = document.querySelector(
        '[data-tauri-plugin-decoration-root][data-tauri-plugin-decoration-active]'
      );
      if (!active) {
        invoke('restore_native_titlebar').catch(() => {});
      }
    }, 5000);

    return () => clearTimeout(timer);
  }, []);

  return (
    <header
      data-tauri-drag-region="deep"
      className="relative z-50 flex h-10 shrink-0 select-none items-center border-b border-border/70 bg-background"
      style={{
        paddingLeft: 'max(8px, var(--tauri-plugin-decoration-left-clearance, 0px))',
        paddingRight: 'max(8px, var(--tauri-plugin-decoration-right-clearance, 0px))',
      }}
    >
      <div className="pointer-events-none absolute inset-x-0 flex justify-center">
        <span className="max-w-[45%] truncate text-[13px] font-medium leading-10 text-foreground/75">
          {dirty ? '\u2022 ' : ''}
          {title || 'LibreGene'}
        </span>
      </div>
    </header>
  );
}
