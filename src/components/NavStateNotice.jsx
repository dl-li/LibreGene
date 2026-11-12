import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from '@/i18n';

const HOLD_MS = 1600;
const EXIT_MS = 220;

export function useNavStateNotice() {
  const { t } = useTranslation();
  const [notice, setNotice] = useState(null);
  const nextIdRef = useRef(0);
  const holdTimerRef = useRef(null);
  const exitTimerRef = useRef(null);

  const showNotice = useCallback(
    (label, value, labelValues) => {
      clearTimeout(holdTimerRef.current);
      clearTimeout(exitTimerRef.current);
      const id = ++nextIdRef.current;
      const state = typeof value === 'boolean' ? t(value ? 'On' : 'Off') : t(value);
      setNotice({
        id,
        text: t('{label}: {value}', { label: t(label, labelValues), value: state }),
        leaving: false,
      });
      holdTimerRef.current = setTimeout(() => {
        setNotice((current) => (current?.id === id ? { ...current, leaving: true } : current));
        exitTimerRef.current = setTimeout(() => {
          setNotice((current) => (current?.id === id ? null : current));
        }, EXIT_MS);
      }, HOLD_MS);
    },
    [t],
  );

  useEffect(
    () => () => {
      clearTimeout(holdTimerRef.current);
      clearTimeout(exitTimerRef.current);
    },
    [],
  );

  return { notice, showNotice };
}

export default function NavStateNotice({ notice }) {
  if (!notice) return null;
  return (
    <div className="pointer-events-none absolute bottom-[calc(100%+0.5rem)] left-1/2 z-20 -translate-x-1/2">
      <div
        key={notice.id}
        role="status"
        aria-live="polite"
        aria-atomic="true"
        data-leaving={notice.leaving}
        className="nav-state-notice flex h-7 w-max min-w-32 max-w-[min(80vw,28rem)] items-center justify-center whitespace-nowrap rounded-full border border-white/15 bg-gradient-to-b from-teal-700/95 to-teal-800/95 px-4 text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.14),0_10px_26px_-10px_rgba(15,118,110,0.38),0_4px_12px_rgba(15,23,42,0.16)] backdrop-blur-md"
      >
        <span className="overflow-hidden text-ellipsis text-center text-[11px] font-medium leading-4 tracking-[0.01em]">
          {notice.text}
        </span>
      </div>
    </div>
  );
}
