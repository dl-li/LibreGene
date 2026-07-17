import React, { useCallback, useEffect, useState } from 'react';
import { X, Minus, Square, SlidersHorizontal } from 'lucide-react';
import { isTauri } from '@/tauriApi';

let windowPromise = null;
function getAppWindow() {
  if (!windowPromise) {
    windowPromise = import('@tauri-apps/api/window').then((m) => m.getCurrentWindow());
  }
  return windowPromise;
}

const isMac = /mac os x/i.test(navigator.userAgent);

// Inline SVG for the Windows restore icon (two overlapping squares)
function RestoreIcon() {
  return (
    <svg
      width="10"
      height="10"
      viewBox="0 0 10 10"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <rect x="3" y="3" width="7" height="7" rx="0.5" />
      <path d="M1 7V1h6" />
    </svg>
  );
}

export default function TitleBar({ title, dirty = false, backendStatus, onOpenDebug }) {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!isTauri || isMac) return undefined;
    let unlisten;
    let cancelled = false;
    getAppWindow()
      .then(async (w) => {
        try {
          setMaximized(await w.isMaximized());
        } catch {}
        return w.onResized(() => {
          if (!cancelled)
            w.isMaximized()
              .then(setMaximized)
              .catch(() => {});
        });
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  const doMinimize = useCallback(() => {
    getAppWindow()
      .then((w) => w.minimize())
      .catch(() => {});
  }, []);
  const doZoom = useCallback(() => {
    getAppWindow()
      .then((w) => w.toggleMaximize())
      .catch(() => {});
  }, []);
  const doClose = useCallback(() => {
    getAppWindow()
      .then((w) => w.close())
      .catch(() => {});
  }, []);

  const statusColor =
    backendStatus === 'online' || isTauri
      ? '#22c55e'
      : backendStatus === 'connecting'
        ? '#f59e0b'
        : '#9ca3af';
  const statusTip = isTauri
    ? 'Desktop mode'
    : backendStatus === 'online'
      ? 'Backend connected'
      : backendStatus === 'connecting'
        ? 'Connecting...'
        : 'Offline';

  return (
    <header
      data-tauri-drag-region="deep"
      className="relative z-50 flex h-10 shrink-0 select-none items-center border-b border-border/70 bg-background"
    >
      {/* macOS: spacer for native traffic lights; non-Tauri: nothing */}
      {isTauri && isMac && <div className="w-[72px] shrink-0" />}

      <div className="pointer-events-none absolute inset-x-0 flex justify-center">
        <span className="max-w-[45%] truncate text-[13px] font-medium leading-10 text-foreground/75">
          {dirty ? '\u2022 ' : ''}
          {title || 'LibreGene'}
        </span>
      </div>

      <div className="ml-auto flex items-center pr-2.5">
        {onOpenDebug && (
          <button
            type="button"
            onClick={onOpenDebug}
            title={`Debug 面板 \u00b7 ${statusTip}`}
            className="relative flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <SlidersHorizontal className="size-3.5" />
            <span
              className="absolute right-1 top-1 size-1.5 rounded-full"
              style={{ background: statusColor }}
            />
          </button>
        )}

        {/* Windows/Linux caption buttons (right side) */}
        {isTauri && !isMac && (
          <div className="ml-1 flex items-center">
            <button
              type="button"
              aria-label="Minimize"
              onClick={doMinimize}
              className="flex h-10 w-[46px] items-center justify-center text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
            >
              <Minus className="size-3.5" />
            </button>
            <button
              type="button"
              aria-label={maximized ? 'Restore' : 'Maximize'}
              onClick={doZoom}
              className="flex h-10 w-[46px] items-center justify-center text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
            >
              {maximized ? <RestoreIcon /> : <Square className="size-3.5" />}
            </button>
            <button
              type="button"
              aria-label="Close"
              onClick={doClose}
              className="flex h-10 w-[46px] items-center justify-center text-muted-foreground transition-colors hover:bg-[#c42b1c] hover:text-white"
            >
              <X className="size-4" />
            </button>
          </div>
        )}
      </div>
    </header>
  );
}
