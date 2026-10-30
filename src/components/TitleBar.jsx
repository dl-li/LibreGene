import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { isTauri } from '@/tauriApi';

// Linux uses the desktop environment's native window frame and controls.
// macOS/Windows keep the custom overlay; a spurious activation error (or a
// remount re-invoke) must not hide it there.
const isLinux = typeof navigator !== 'undefined' && /Linux/i.test(navigator.platform);

// macOS never activates the decoration plugin (it refuses dev-URL content
// anyway, and when it does activate in release builds its inset semantics
// fight tao's trafficLightPosition and AppKit relayouts — the lights drift
// or jump). tao's native trafficLightPosition already places the lights
// correctly; the window just needs to be shown.
const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);

export default function TitleBar({ title, dirty = false, onFallback }) {
  const [hidden, setHidden] = useState(isLinux && isTauri);

  useEffect(() => {
    if (!isTauri) return undefined;
    if (isLinux) {
      setHidden(true);
      onFallback?.();
      invoke('restore_native_titlebar').catch(() => {
        invoke('show_window').catch(() => {});
      });
      return undefined;
    }
    if (isMac) {
      invoke('show_window').catch(() => {});
      return undefined;
    }
    const fallback = () => {
      invoke('restore_native_titlebar').catch(() => {});
    };
    invoke('activate_custom_titlebar').catch(fallback);

    // If activation hangs or the plugin never marks its root element, fall back
    const timer = setTimeout(() => {
      const active = document.querySelector(
        '[data-tauri-plugin-decoration-root][data-tauri-plugin-decoration-active]',
      );
      if (!active) fallback();
    }, 5000);

    return () => clearTimeout(timer);
  }, [onFallback]);

  if (hidden) return null;

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
