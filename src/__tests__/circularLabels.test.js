import { describe, expect, it } from 'vitest';
import { layoutCircularLabels } from '../MapView';

const LENGTH = 360;
const R = 150;
const LABEL_R = R + 40;

function featuresInArc(count, startAngle, endAngle) {
  return Array.from({ length: count }, (_, i) => {
    const angle = startAngle + ((endAngle - startAngle) * i) / Math.max(1, count - 1);
    const position = Math.floor(angle) % LENGTH;
    return {
      id: `${startAngle}-${i}`,
      name: `Feature ${startAngle}-${i}`,
      start: position,
      end: position,
      segments: [],
    };
  });
}

function orientation(a, b, c) {
  return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
}

function intersects(a, b, c, d) {
  return (
    orientation(a, b, c) * orientation(a, b, d) < -1e-8 &&
    orientation(c, d, a) * orientation(c, d, b) < -1e-8
  );
}

function leaderPairs(labels) {
  const leaders = labels.map((label) => ({
    start: [(LABEL_R - 12) * Math.sin(label.th), -(LABEL_R - 12) * Math.cos(label.th)],
    end: [label.lx + label.side * 3, label.ly],
    side: label.side,
  }));
  const crossings = [];
  for (let i = 0; i < leaders.length; i++) {
    for (let j = i + 1; j < leaders.length; j++) {
      if (
        leaders[i].side === leaders[j].side &&
        intersects(leaders[i].start, leaders[i].end, leaders[j].start, leaders[j].end)
      ) {
        crossings.push([i, j]);
      }
    }
  }
  return crossings;
}

describe('circular map feature labels', () => {
  it('keeps sparse labels on the original label circle', () => {
    const { labels } = layoutCircularLabels(featuresInArc(1, 35, 35), LENGTH, R, 9);
    expect(labels).toHaveLength(1);
    expect(Math.hypot(labels[0].lx, labels[0].ly)).toBeCloseTo(LABEL_R, 6);
  });

  it('spreads dense quadrants outwards without label or leader collisions', () => {
    const features = [
      ...featuresInArc(24, 2, 80),
      ...featuresInArc(24, 100, 178),
      ...featuresInArc(24, 182, 260),
      ...featuresInArc(24, 280, 358),
    ];
    const { labels } = layoutCircularLabels(features, LENGTH, R, 9);
    for (const side of [1, -1]) {
      const column = labels.filter((label) => label.side === side).sort((a, b) => a.ly - b.ly);
      for (let i = 1; i < column.length; i++) {
        expect(column[i].ly - column[i - 1].ly).toBeGreaterThanOrEqual(17 - 1e-8);
      }
    }
    const topRight = labels.filter((label) => label.side === 1 && label.ly < 0);
    expect(Math.min(...topRight.map((label) => label.ly))).toBeLessThan(-LABEL_R);
    for (const label of labels) {
      expect(Math.hypot(label.lx, label.ly)).toBeGreaterThanOrEqual(LABEL_R - 1e-8);
      expect(Math.abs(label.lx)).toBeLessThanOrEqual(LABEL_R + 1e-8);
    }
    expect(leaderPairs(labels)).toEqual([]);
  });
});
