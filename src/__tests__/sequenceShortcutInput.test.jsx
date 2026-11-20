import { renderToString } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import SequenceShortcutInput, {
  isShortcutBadgeHighlighted,
} from '../components/SequenceShortcutInput';
import { parseSequenceShortcuts } from '../sequenceShortcuts';

describe('SequenceShortcutInput', () => {
  it('renders every replacement option and the usage examples', () => {
    const value = '[hhhhhh]';
    const shortcuts = {
      ...parseSequenceShortcuts(value, [{ name: 'HHHHHH', site: 'GGGCCC' }], () => 0),
      enabled: true,
    };
    const html = renderToString(
      <SequenceShortcutInput
        value={value}
        onChange={() => {}}
        shortcuts={shortcuts}
        inputClassName="bg-background text-xs"
      />,
    );

    const overlayClass = html.match(/<div aria-hidden="true" class="([^"]+)"/)?.[1];
    expect(overlayClass).toContain('bg-transparent');
    expect(overlayClass).not.toContain('bg-background');
    expect(overlayClass).toContain('text-transparent');
    expect(html.match(/aria-label="hhhhhh → /g)).toHaveLength(2);
    expect(html).toContain('hhhhhh → GGGCCC');
    expect(html).toContain(`hhhhhh → ${'CAC'.repeat(6)}`);
    expect(html).toContain('Sequence shortcut input');
    expect(html).toContain('data-slot="tooltip-trigger"');
  });

  it('highlights every badge of a shortcut unless a single badge is hovered', () => {
    expect(isShortcutBadgeHighlighted(null, '0:0', 'enzyme')).toBe(false);
    expect(isShortcutBadgeHighlighted({ tokenId: '0:0', kind: null }, '0:0', 'enzyme')).toBe(true);
    expect(isShortcutBadgeHighlighted({ tokenId: '0:0', kind: null }, '0:0', 'random')).toBe(true);
    expect(isShortcutBadgeHighlighted({ tokenId: '0:0', kind: 'enzyme' }, '0:0', 'enzyme')).toBe(
      true,
    );
    expect(isShortcutBadgeHighlighted({ tokenId: '0:0', kind: 'enzyme' }, '0:0', 'random')).toBe(
      false,
    );
    expect(isShortcutBadgeHighlighted({ tokenId: '0:1', kind: 'enzyme' }, '0:0', 'enzyme')).toBe(
      false,
    );
  });
});
