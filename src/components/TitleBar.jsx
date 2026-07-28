import { useCallback, useEffect, useState } from 'react';
import { X, Minus, Plus, Square } from 'lucide-react';
import { isTauri } from '@/tauriApi';
import { cn } from '@/lib/utils';

let windowPromise = null;
function getAppWindow() {
  if (!windowPromise) {
    windowPromise = import('@tauri-apps/api/window').then((m) => m.getCurrentWindow());
  }
  return windowPromise;
}

const isMac = /mac os x/i.test(navigator.userAgent);

// macOS-style traffic light button (12px circle)
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

export default function TitleBar({ title, dirty = false }) {
  const [focused, setFocused] = useState(true);
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!isTauri) return undefined;
    let unlisten;
    let cancelled = false;
    getAppWindow()
      .then((w) => {
        // Focus tracking (macOS dims traffic lights on unfocused windows)
        if (isMac) {
          return w.onFocusChanged((e) => {
            if (!cancelled) setFocused(e.payload);
          });
        }
        // Maximized tracking (Windows shows restore icon)
        w.isMaximized()
          .then(setMaximized)
          .catch(() => {});
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

  return (
    <header
      data-tauri-drag-region="deep"
      className="relative z-50 flex h-10 shrink-0 select-none items-center border-b border-border/70 bg-background"
    >
      {/* macOS: custom traffic lights (perfectly positioned via flex centering) */}
      {isTauri && isMac && (
        <div className="group/traffic flex items-center gap-[6px] pl-3.5">
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

      {/* Centered title */}
      <div className="pointer-events-none absolute inset-x-0 flex justify-center">
        <span className="max-w-[45%] truncate text-[13px] font-medium leading-10 text-foreground/75">
          {dirty ? '\u2022 ' : ''}
          {title || 'LibreGene'}
        </span>
      </div>

      <div className="ml-auto flex items-center pr-2.5">
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
