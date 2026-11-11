import { describe, expect, it } from 'vitest';
import React from 'react';
import { renderToString } from 'react-dom/server';
import WorkspaceError from '../components/WorkspaceError';

describe('WorkspaceError', () => {
  it('renders operation failures as an accessible alert with a dismiss action', () => {
    const html = renderToString(
      React.createElement(WorkspaceError, {
        error: 'Save failed: permission denied',
        onDismiss: () => {},
      }),
    );
    expect(html).toContain('role="alert"');
    expect(html).toContain('Save failed: permission denied');
    expect(html).toContain('aria-label="Dismiss error"');
  });

  it('renders nothing when there is no error', () => {
    expect(renderToString(React.createElement(WorkspaceError, { error: '' }))).toBe('');
  });
});
