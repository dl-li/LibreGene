import { renderToString } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import NavStateNotice from '../components/NavStateNotice';

describe('NavStateNotice', () => {
  it('renders a detached deep-teal status capsule', () => {
    const html = renderToString(
      <NavStateNotice notice={{ id: 1, text: '显示引物：开', leaving: false }} />,
    );
    expect(html).toContain('role="status"');
    expect(html).toContain('显示引物：开');
    expect(html).toContain('bottom-[calc(100%+0.5rem)]');
    expect(html).toContain('rounded-full');
    expect(html).toContain('bg-teal-800');
    expect(html).toContain('text-white');
    expect(html).toContain('data-leaving="false"');
  });

  it('marks a departing notice for the retract animation', () => {
    const html = renderToString(
      <NavStateNotice notice={{ id: 2, text: 'Show Primers: Off', leaving: true }} />,
    );
    expect(html).toContain('data-leaving="true"');
  });
});
