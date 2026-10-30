import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { isTauri } from '@/tauriApi';

const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);

export default function TitleBar({ title, dirty = false }) {
  useEffect(() => {
    if (isTauri) invoke('show_window').catch(() => {});
  }, []);

  if (isTauri && !isMac) return null;

  return (
    <header
      data-tauri-drag-region="deep"
      className="relative z-50 flex h-10 shrink-0 select-none items-center border-b border-border/70 bg-background px-2"
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
