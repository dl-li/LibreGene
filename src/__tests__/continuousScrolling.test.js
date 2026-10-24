import { describe, expect, it, vi } from 'vitest';
import { continuousWheelDelta, createScrollAxisScheduler } from '../editor/continuousScrolling';

const wheel = (overrides = {}) => ({
  deltaX: 0,
  deltaY: 40,
  scrollLeft: 100,
  maxScrollLeft: 500,
  canScrollVertically: false,
  ...overrides,
});

describe('continuous wheel routing', () => {
  it('leaves vertical wheel native while vertical content can scroll', () => {
    expect(continuousWheelDelta(wheel({ canScrollVertically: true }))).toBeNull();
  });

  it('releases wheel at both horizontal endpoints', () => {
    expect(continuousWheelDelta(wheel({ deltaY: -20, scrollLeft: 0 }))).toBeNull();
    expect(continuousWheelDelta(wheel({ deltaY: 20, scrollLeft: 500 }))).toBeNull();
    expect(continuousWheelDelta(wheel({ deltaY: 20, scrollLeft: 499 }))).toBe(20);
  });

  it('uses a diagonal deadzone and preserves native horizontal and Shift gestures', () => {
    expect(continuousWheelDelta(wheel({ deltaX: 30, deltaY: 30 }))).toBeNull();
    expect(continuousWheelDelta(wheel({ deltaX: 30, deltaY: 60 }))).toBe(90);
    expect(continuousWheelDelta(wheel({ deltaX: 30, deltaY: 0 }))).toBeNull();
    expect(continuousWheelDelta(wheel({ shiftKey: true }))).toBeNull();
  });
});

describe('independent scroll axis schedulers', () => {
  it('throttles each axis without dropping the other axis update', () => {
    const frames = [];
    const vertical = createScrollAxisScheduler((cb) => frames.push(cb));
    const horizontal = createScrollAxisScheduler((cb) => frames.push(cb));
    const verticalUpdate = vi.fn();
    const horizontalUpdate = vi.fn();

    vertical(verticalUpdate);
    vertical(verticalUpdate);
    horizontal(horizontalUpdate);
    expect(frames).toHaveLength(2);
    expect(verticalUpdate).not.toHaveBeenCalled();
    expect(horizontalUpdate).not.toHaveBeenCalled();

    frames.forEach((cb) => cb());
    expect(verticalUpdate).toHaveBeenCalledTimes(1);
    expect(horizontalUpdate).toHaveBeenCalledTimes(1);
  });
});
