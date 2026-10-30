import { Bot, History, LockOpen } from 'lucide-react';
import { DESIGN_MODES } from '../plugins/builtin/primerDesign';

export default function EditorStatusBar({
  designPick,
  designPickError,
  cancelDesignPick,
  agentLocked,
  onUnlockAgent,
  onOpenSessionEditLog,
}) {
  if (!designPick && !agentLocked) return null;
  return (
    <div className="fixed bottom-4 left-1/2 z-40 -translate-x-1/2">
      <div className="nav-bar-enter flex min-w-max items-center gap-3 rounded-full border-2 border-teal-700 bg-background/80 px-4 py-2 text-sm text-foreground shadow-[0_10px_30px_rgba(0,0,0,0.15)] backdrop-blur-md dark:shadow-[0_10px_30px_rgba(0,0,0,0.6)]">
        {designPick ? (
          <>
            <span className="font-semibold text-teal-800">
              {DESIGN_MODES[designPick.mode].prompts[designPick.segments.length]}
            </span>
            {designPickError && <span className="text-xs text-destructive">{designPickError}</span>}
            <button
              type="button"
              onClick={cancelDesignPick}
              className="rounded-md bg-muted/60 px-2.5 py-1 text-xs font-medium text-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
            >
              Cancel
            </button>
          </>
        ) : (
          <>
            <Bot className="size-4 shrink-0 text-teal-600" />
            <span className="font-semibold text-teal-800">Controlled by an MCP agent</span>
            <span className="text-xs text-muted-foreground">
              Viewing and selecting work as usual; editing is disabled
            </span>
            <button
              type="button"
              onClick={onOpenSessionEditLog}
              className="inline-flex items-center gap-1.5 rounded-md bg-muted/60 px-2.5 py-1 text-xs font-medium text-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
            >
              <History className="size-3.5" />
              Edit Log
            </button>
            <button
              type="button"
              onClick={onUnlockAgent}
              className="rounded-md bg-muted/60 px-2.5 py-1 text-xs font-medium text-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
            >
              <span className="inline-flex items-center gap-1.5">
                <LockOpen className="size-3.5" />
                Unlock
              </span>
            </button>
          </>
        )}
      </div>
    </div>
  );
}
