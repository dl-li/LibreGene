import React from 'react';
import { Button } from '@/components/ui/button';
import { CircleAlert, RotateCcw, RefreshCw } from 'lucide-react';

export default class ErrorBoundary extends React.Component {
  constructor(props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error) {
    return { hasError: true, error };
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex h-screen w-full flex-col items-center justify-center gap-4 bg-background p-6 text-center">
          <div className="flex size-12 items-center justify-center rounded-full bg-red-50 text-red-500">
            <CircleAlert className="size-6" />
          </div>
          <div className="space-y-1.5">
            <h2 className="text-base font-semibold tracking-tight text-foreground">
              Something went wrong
            </h2>
            <p className="text-sm text-muted-foreground">
              The editor hit an unexpected error. Your files on disk are not affected.
            </p>
          </div>
          <pre className="max-w-lg whitespace-pre-wrap break-all rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-left font-mono text-xs text-red-700">
            {this.state.error?.message}
          </pre>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => this.setState({ hasError: false, error: null })}
            >
              <RotateCcw className="size-3.5" />
              Try again
            </Button>
            <Button size="sm" onClick={() => window.location.reload()}>
              <RefreshCw className="size-3.5" />
              Reload
            </Button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
