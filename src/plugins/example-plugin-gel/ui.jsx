// Self-contained UI primitives so this plugin directory can be dropped into
// any React host without shadcn/Radix. Styling relies on Tailwind theme
// tokens (bg-card, text-muted-foreground, ...) provided by the host app;
// see manifest.json for the host contract.
import { createContext, useContext, useEffect, useRef, useSyncExternalStore } from 'react';

const ZH = {
  Close: '关闭',
  'Digest and Run Gel': '酶切与凝胶电泳',
  Enzymes: '酶',
  'Enzyme group': '酶分组',
  'Single cutters': '单切酶',
  'Double cutters': '双切酶',
  Ladder: 'Marker',
  Gel: '凝胶',
  'Simulated gel': '模拟凝胶',
  Agarose: '琼脂糖',
  'None for this plasmid': '此质粒中无匹配项',
  '{count} cut(s)': '{count} 个切点',
  ' (methylation blocked)': '（甲基化阻断）',
  '{site} — click to remove': '{site} — 点击移除',
  'Click to remove': '点击移除',
  'No enzymes selected — pick from the badges below': '未选择酶，请从下方标签中选择',
  'Cutting efficiency': '酶切效率',
  '{selected}/{max} selected': '已选 {selected}/{max}',
  'Sizes in bp; wrap a size in *asterisks* for the brightest band.':
    '单位为 bp；用 *星号* 包裹数值可将该条带设为最亮。',
  Concentration: '浓度',
  'Run time': '电泳时间',
  'No sequence loaded.': '未加载序列。',
  uncut: '未酶切',
  ' (supercoiled)': '（超螺旋）',
  ' (nicked circle)': '（切口环状）',
  'All Enzymes': '全部酶',
  'Unique + Twice-cutter': '单切 + 双切酶',
  'Unique Cutters': '单切酶',
  'Unique 6 bp': '6 bp 单切酶',
  'Twice-cutter': '双切酶',
  'My Enzymes': '我的酶',
  Blunt: '平末端',
  "5' Overhang": "5' 突出端",
  "3' Overhang": "3' 突出端",
  '4 bp Recognition': '4 bp 识别序列',
  '5 bp Recognition': '5 bp 识别序列',
  '6 bp Recognition': '6 bp 识别序列',
  '≥8 bp Recognition': '≥8 bp 识别序列',
};

function subscribeLanguage(callback) {
  const observer = new MutationObserver(callback);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ['lang'] });
  return () => observer.disconnect();
}

export function useTranslation() {
  const lang = useSyncExternalStore(
    subscribeLanguage,
    () => document.documentElement.lang,
    () => 'en',
  );
  const t = (text, values) => {
    let translated = lang.startsWith('zh') ? ZH[text] || text : text;
    if (values) {
      translated = translated.replace(/\{(\w+)\}/g, (match, key) =>
        Object.hasOwn(values, key) ? String(values[key]) : match,
      );
    }
    return translated;
  };
  return { t };
}

export function T({ children }) {
  const { t } = useTranslation();
  return typeof children === 'string' ? t(children) : children;
}

function cx(...parts) {
  return parts.filter(Boolean).join(' ');
}

const DialogCtx = createContext(null);

export function Dialog({ open, onOpenChange, children }) {
  useEffect(() => {
    if (!open) return undefined;
    const onKey = (e) => {
      if (e.key === 'Escape') onOpenChange?.(false);
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open, onOpenChange]);
  if (!open) return null;
  return <DialogCtx.Provider value={onOpenChange}>{children}</DialogCtx.Provider>;
}

// Contain wheel scrolling inside the dialog: inner scrollable areas scroll
// natively, but wheel over blank/non-scrollable parts (or a scrollable
// already at its edge) must not leak to the page behind the dialog. Needs a
// native listener — React attaches wheel as passive, so preventDefault via
// onWheel is ignored.
function useWheelContainment() {
  const ref = useRef(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return undefined;
    const onWheel = (e) => {
      let node = e.target;
      while (node && node !== el) {
        if (node instanceof HTMLElement) {
          const oy = getComputedStyle(node).overflowY;
          if ((oy === 'auto' || oy === 'scroll') && node.scrollHeight > node.clientHeight) {
            const atTop = node.scrollTop <= 0;
            const atBottom = node.scrollTop + node.clientHeight >= node.scrollHeight - 1;
            if ((e.deltaY < 0 && !atTop) || (e.deltaY > 0 && !atBottom)) return;
          }
        }
        node = node.parentNode;
      }
      e.preventDefault();
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, []);
  return ref;
}

export function DialogContent({ className, style, children }) {
  const { t } = useTranslation();
  const onOpenChange = useContext(DialogCtx);
  const close = () => onOpenChange?.(false);
  const rootRef = useWheelContainment();
  return (
    <div className="fixed inset-0 z-50" ref={rootRef}>
      <div
        className="absolute inset-0 bg-foreground/35 backdrop-blur-[3px]"
        onClick={close}
        aria-hidden="true"
      />
      <div
        role="dialog"
        aria-modal="true"
        style={style}
        className={cx(
          'fixed left-[50%] top-[50%] z-50 grid w-full translate-x-[-50%] translate-y-[-50%] gap-4 rounded-xl border border-border/60 bg-card p-6 shadow-2xl ring-1 ring-black/[0.04] outline-none',
          className,
        )}
      >
        {children}
        <button
          type="button"
          onClick={close}
          aria-label={t('Close')}
          className="absolute right-4 top-4 flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            className="size-4"
          >
            <path d="M18 6 6 18" />
            <path d="m6 6 12 12" />
          </svg>
        </button>
      </div>
    </div>
  );
}

export function DialogHeader({ className, ...props }) {
  return <div className={cx('flex flex-col space-y-1.5 text-left', className)} {...props} />;
}

export function DialogTitle({ className, ...props }) {
  return (
    <h2
      className={cx('text-base font-semibold leading-none tracking-tight', className)}
      {...props}
    />
  );
}

export function Label({ className, ...props }) {
  return <label className={cx('text-sm font-medium leading-none', className)} {...props} />;
}

const NOTICE_TONES = {
  error: 'border-red-200 bg-red-50 text-red-800',
  warning: 'border-amber-200 bg-amber-50 text-amber-800',
  info: 'border-border bg-muted/60 text-muted-foreground',
};

export function InlineNotice({ tone = 'info', className, children }) {
  return (
    <div
      className={cx(
        'flex items-center gap-2 rounded-lg border px-3 py-2 text-xs leading-relaxed',
        NOTICE_TONES[tone] || NOTICE_TONES.info,
        className,
      )}
    >
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        className="size-3.5 shrink-0 opacity-80"
      >
        <circle cx="12" cy="12" r="10" />
        <line x1="12" x2="12" y1="8" y2="12" />
        <line x1="12" x2="12.01" y1="16" y2="16" />
      </svg>
      <div className="min-w-0">{children}</div>
    </div>
  );
}
