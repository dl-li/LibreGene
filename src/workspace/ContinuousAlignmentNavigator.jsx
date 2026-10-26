import { ChevronLeft, ChevronRight } from 'lucide-react';
import { BASE_HILITE_BG } from '../editor/alignmentLayout';

export default function ContinuousAlignmentNavigator({ show, shift, alignNavRef }) {
  if (!show) return null;
  return (
    <div
      style={{
        transform: `translateX(calc(-50% + ${shift}px))`,
        '--align-hilite': BASE_HILITE_BG,
        '--align-hilite-soft': `${BASE_HILITE_BG}99`,
      }}
      className="nav-bar-enter pointer-events-auto absolute left-1/2 top-full z-40 mt-1.5 flex flex-row items-stretch overflow-hidden rounded-full border border-border/60 bg-background/70 shadow-md backdrop-blur-md transition-shadow duration-200 hover:shadow-lg"
    >
      <button
        type="button"
        title="Previous alignment marker"
        onClick={() => alignNavRef.current?.(-1)}
        className="rounded-l-full py-2 pl-2.5 pr-1.5 text-muted-foreground transition-colors duration-150 hover:bg-[var(--align-hilite-soft)] hover:text-foreground active:bg-[var(--align-hilite)]"
      >
        <ChevronLeft className="mx-auto size-4" strokeWidth={2.25} />
      </button>
      <div className="my-auto h-3.5 w-px bg-border/70" />
      <button
        type="button"
        title="Next alignment marker"
        onClick={() => alignNavRef.current?.(1)}
        className="rounded-r-full py-2 pl-1.5 pr-2.5 text-muted-foreground transition-colors duration-150 hover:bg-[var(--align-hilite-soft)] hover:text-foreground active:bg-[var(--align-hilite)]"
      >
        <ChevronRight className="mx-auto size-4" strokeWidth={2.25} />
      </button>
    </div>
  );
}
