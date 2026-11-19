import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from '@/i18n';
import { bgColor } from '../editorConstants';

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

export function NavStateCapsule({
  children,
  leaving = false,
  motionClassName = 'nav-state-notice',
  style,
  ...props
}) {
  return (
    <div
      {...props}
      data-leaving={leaving}
      className={`${motionClassName} flex h-7 w-max min-w-32 max-w-[min(80vw,28rem)] items-center justify-center whitespace-nowrap rounded-full bg-gradient-to-b from-teal-700/95 to-teal-800/95 px-4 text-white backdrop-blur-md`}
      style={{ boxShadow: `0 0 3px 3px ${bgColor}`, ...style }}
    >
      <span className="w-full overflow-hidden text-ellipsis text-center text-[13px] font-bold leading-none">
        {children}
      </span>
    </div>
  );
}

export default function NavStateNotice({ notice }) {
  if (!notice) return null;
  return (
    <div className="pointer-events-none absolute bottom-[calc(100%+0.5rem)] left-1/2 z-20 -translate-x-1/2">
      <NavStateCapsule
        key={notice.id}
        role="status"
        aria-live="polite"
        aria-atomic="true"
        leaving={notice.leaving}
      >
        {notice.text}
      </NavStateCapsule>
    </div>
  );
}
