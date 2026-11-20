import { describe, expect, it } from 'vitest';
import { isEditTriggerKey } from '../editor/editKeys';

describe('isEditTriggerKey', () => {
  it('accepts letters and shortcut brackets', () => {
    for (const key of ['a', 'Z', 'n', '[', ']']) expect(isEditTriggerKey(key)).toBe(true);
  });

  it('rejects other keys and non-single characters', () => {
    for (const key of ['1', '5', ',', ' ', 'Enter', 'ArrowLeft', '', null, undefined]) {
      expect(isEditTriggerKey(key)).toBe(false);
    }
  });
});
