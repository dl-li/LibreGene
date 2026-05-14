import React from 'react';

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
        <div style={{ padding: 40, fontFamily: 'system-ui, sans-serif' }}>
          <h2>Something went wrong</h2>
          <pre style={{ color: '#ef4444', whiteSpace: 'pre-wrap' }}>
            {this.state.error?.message}
          </pre>
          <div style={{ display: 'flex', gap: 8 }}>
            <button onClick={() => this.setState({ hasError: false, error: null })}>
              Try again
            </button>
            <button onClick={() => window.location.reload()}>
              Reload page
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
