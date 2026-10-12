// Self-contained UI primitives so this plugin directory can be dropped into
// any React host without shadcn/Radix. Styling relies on Tailwind theme
// tokens (bg-card, text-muted-foreground, ...) provided by the host app;
// see manifest.json for the host contract.
import { createContext, useContext, useEffect } from 'react';

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

export function DialogContent({ className, children }) {
  const onOpenChange = useContext(DialogCtx);
  const close = () => onOpenChange?.(false);
  return (
    <div className="fixed inset-0 z-50">
      <div
        className="absolute inset-0 bg-foreground/35 backdrop-blur-[3px]"
        onClick={close}
        aria-hidden="true"
      />
      <div
        role="dialog"
        aria-modal="true"
        className={cx(
          'fixed left-[50%] top-[50%] z-50 grid w-full max-w-lg translate-x-[-50%] translate-y-[-50%] gap-4 rounded-xl border border-border/60 bg-card p-6 shadow-2xl ring-1 ring-black/[0.04] outline-none',
          className,
        )}
      >
        {children}
        <button
          type="button"
          onClick={close}
          aria-label="Close"
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
