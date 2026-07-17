import React, { useCallback, useEffect, useState } from 'react';
import { X, Minus, Plus, SlidersHorizontal } from 'lucide-react';
import { isTauri } from '@/tauriApi';
import { cn } from '@/lib/utils';

let windowPromise = null;
function getAppWindow() {
  if (!windowPromise) {
    windowPromise = import('@tauri-apps/api/window').then((m) => m.getCurrentWindow());
  }
  return windowPromise;
}

function TrafficLight({ label, onClick, focused, activeClass, dirty, icon }) {
  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      className={cn(
        'relative flex size-3 shrink-0 items-center justify-center rounded-full border',
        focused ? activeClass : 'border-black/[0.08] bg-[#dcdcdc]',
      )}
    >
      {dirty ? (
        <>
          <span className="flex items-center justify-center text-black/60 opacity-0 group-hover/traffic:opacity-100">
            {icon}
          </span>
          <span className="absolute size-1 rounded-full bg-black/60 group-hover/traffic:hidden" />
        </>
      ) : (
        <span className="flex items-center justify-center text-black/60 opacity-0 transition-opacity group-hover/traffic:opacity-100">
          {icon}
        </span>
      )}
    </button>
  );
}

export default function TitleBar({ title, dirty = false, backendStatus, onOpenDebug }) {
  const [focused, setFocused] = useState(true);

  useEffect(() => {
    if (!isTauri) return undefined;
    let unlisten;
    let cancelled = false;
    getAppWindow()
      .then((w) => w.onFocusChanged((e) => setFocused(e.payload)))
      .then((fn) => { if (cancelled) fn(); else unlisten = fn; })
      .catch(() => {});
    return () => { cancelled = true; if (unlisten) unlisten(); };
  }, []);

  const doMinimize = useCallback(() => { getAppWindow().then((w) => w.minimize()).catch(() => {}); }, []);
  const doZoom = useCallback(() => { getAppWindow().then((w) => w.toggleMaximize()).catch(() => {}); }, []);
  const doClose = useCallback(() => { getAppWindow().then((w) => w.close()).catch(() => {}); }, []);

  const statusColor = backendStatus === 'online' || isTauri ? '#22c55e' : backendStatus === 'connecting' ? '#f59e0b' : '#9ca3af';
  const statusTip = isTauri ? 'Desktop mode' : backendStatus === 'online' ? 'Backend connected' : backendStatus === 'connecting' ? 'Connecting...' : 'Offline';

  return (
    <header
      data-tauri-drag-region="deep"
      className="fixed inset-x-0 top-0 z-50 flex h-10 select-none items-center border-b border-border/70 bg-background/85 backdrop-blur-md"
    >
      {isTauri && (
        <div className="group/traffic flex items-center gap-2 pl-3.5">
          <TrafficLight
            label="Close"
            onClick={doClose}
            focused={focused}
            dirty={dirty}
            activeClass="border-[#e0443e] bg-[#ff5f57]"
            icon={<X className="size-2" strokeWidth={3.5} />}
          />
          <TrafficLight
            label="Minimize"
            onClick={doMinimize}
            focused={focused}
            activeClass="border-[#d89e24] bg-[#febc2e]"
            icon={<Minus className="size-2" strokeWidth={3.5} />}
          />
          <TrafficLight
            label="Zoom"
            onClick={doZoom}
            focused={focused}
            activeClass="border-[#1dad2b] bg-[#28c840]"
            icon={<Plus className="size-2" strokeWidth={3.5} />}
          />
        </div>
      )}

      <div className="pointer-events-none absolute inset-x-0 flex justify-center">
        <span className="max-w-[45%] truncate text-[13px] font-medium leading-10 text-foreground/75">
          {title || 'LibreGene'}
        </span>
      </div>

      <div className="ml-auto flex items-center gap-1.5 pr-2.5">
        {onOpenDebug && (
          <button
            type="button"
            onClick={onOpenDebug}
            title={`Debug 面板 · ${statusTip}`}
            className="relative flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <SlidersHorizontal className="size-3.5" />
            <span
              className="absolute right-1 top-1 size-1.5 rounded-full"
              style={{ background: statusColor }}
            />
          </button>
        )}
      </div>
    </header>
  );
}
