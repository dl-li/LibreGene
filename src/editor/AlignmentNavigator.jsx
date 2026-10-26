import { ChevronDown, ChevronUp } from 'lucide-react';
import { BASE_HILITE_BG } from './alignmentLayout';

export default function AlignmentNavigator({ show, continuous, onJump }) {
  if (!show || continuous) return null;
  return (
    <div
      className="nav-bar-enter fixed right-7 top-1/2 z-40 flex -translate-y-1/2 flex-col items-stretch overflow-hidden rounded-full border border-border/60 bg-background/70 shadow-md backdrop-blur-md transition-shadow duration-200 hover:shadow-lg"
      style={{
        '--align-hilite': BASE_HILITE_BG,
        '--align-hilite-soft': `${BASE_HILITE_BG}99`,
      }}
    >
      <button
        type="button"
        title="Previous alignment marker"
        onClick={() => onJump(-1)}
        className="rounded-t-full py-2 pl-2.5 pr-1.5 text-muted-foreground transition-colors duration-150 hover:bg-[var(--align-hilite-soft)] hover:text-foreground active:bg-[var(--align-hilite)]"
      >
        <ChevronUp className="mx-auto size-4" strokeWidth={2.25} />
      </button>
      <div className="mx-auto h-px w-3.5 bg-border/70" />
      <button
        type="button"
        title="Next alignment marker"
        onClick={() => onJump(1)}
        className="rounded-b-full py-2 pl-1.5 pr-2.5 text-muted-foreground transition-colors duration-150 hover:bg-[var(--align-hilite-soft)] hover:text-foreground active:bg-[var(--align-hilite)]"
      >
        <ChevronDown className="mx-auto size-4" strokeWidth={2.25} />
      </button>
    </div>
  );
}
