import { X } from 'lucide-react';
import { InlineNotice } from './ui/notice';

export default function WorkspaceError({ error, onDismiss }) {
  if (!error) return null;
  return (
    <div role="alert" className="relative z-40 px-3 pt-2">
      <InlineNotice tone="error" className="pr-9">
        {error}
      </InlineNotice>
      <button
        type="button"
        aria-label="Dismiss error"
        title="Dismiss"
        onClick={onDismiss}
        className="absolute right-5 top-1/2 -translate-y-1/2 rounded p-1 text-red-700 hover:bg-red-100"
      >
        <X className="size-3.5" />
      </button>
    </div>
  );
}
