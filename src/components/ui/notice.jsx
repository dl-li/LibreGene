import { AlertTriangle, CircleAlert, Info } from 'lucide-react';

import { cn } from '@/lib/utils';

const TONES = {
  error: {
    cls: 'border-red-200 bg-red-50 text-red-800',
    Icon: CircleAlert,
  },
  warning: {
    cls: 'border-amber-200 bg-amber-50 text-amber-800',
    Icon: AlertTriangle,
  },
  info: {
    cls: 'border-border bg-muted/60 text-muted-foreground',
    Icon: Info,
  },
};

function InlineNotice({ tone = 'info', icon = true, className, children }) {
  const { cls, Icon } = TONES[tone] || TONES.info;
  return (
    <div
      className={cn(
        'flex items-center gap-2 rounded-lg border px-3 py-2 text-xs leading-relaxed',
        cls,
        className,
      )}
    >
      {icon && <Icon className="size-3.5 shrink-0 opacity-80" />}
      <div className="min-w-0">{children}</div>
    </div>
  );
}

export { InlineNotice };
