const DIAGONAL_DEADZONE = 1.2;

export function continuousWheelDelta({
  deltaX,
  deltaY,
  shiftKey = false,
  scrollLeft,
  maxScrollLeft,
}) {
  if (shiftKey || Math.abs(deltaY) <= Math.abs(deltaX) * DIAGONAL_DEADZONE) {
    return null;
  }
  const delta = deltaY + deltaX;
  if (delta === 0 || (delta < 0 && scrollLeft <= 0) || (delta > 0 && scrollLeft >= maxScrollLeft)) {
    return null;
  }
  return delta;
}

export function createScrollAxisScheduler(requestFrame) {
  let ticking = false;
  return (callback) => {
    if (ticking) return;
    ticking = true;
    requestFrame(() => {
      ticking = false;
      callback();
    });
  };
}
