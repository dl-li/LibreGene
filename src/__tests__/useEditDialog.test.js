import { describe, it, expect } from 'vitest';
import { dropCoveredFeatures } from '../workspace/useEditDialog';

describe('dropCoveredFeatures (annotated paste supersedes covered features)', () => {
  const editStart = 100;
  const editEnd = 199;

  it('drops features fully inside the replaced span, keeps the rest', () => {
    const features = [
      { id: 'inside', name: 'inside', start: 120, end: 150 },
      { id: 'exact', name: 'exact', start: 100, end: 199 },
      { id: 'left', name: 'left', start: 50, end: 120 },
      { id: 'right', name: 'right', start: 150, end: 250 },
      { id: 'spanning', name: 'spanning', start: 50, end: 250 },
      { id: 'before', name: 'before', start: 10, end: 20 },
      { id: 'after', name: 'after', start: 300, end: 320 },
    ];
    const kept = dropCoveredFeatures(features, editStart, editEnd).map((f) => f.id);
    expect(kept).toEqual(['left', 'right', 'spanning', 'before', 'after']);
  });

  it('drops a multi-segment feature only when every segment is covered', () => {
    const features = [
      {
        id: 'allIn',
        name: 'allIn',
        start: 110,
        end: 180,
        segments: [
          { start: 110, end: 130 },
          { start: 160, end: 180 },
        ],
      },
      {
        id: 'oneOut',
        name: 'oneOut',
        start: 110,
        end: 300,
        segments: [
          { start: 110, end: 130 },
          { start: 250, end: 300 },
        ],
      },
    ];
    const kept = dropCoveredFeatures(features, editStart, editEnd).map((f) => f.id);
    expect(kept).toEqual(['oneOut']);
  });

  it('judges cross-origin features (segments in join order) per segment', () => {
    const wrap = {
      id: 'wrap',
      name: 'wrap',
      start: 190,
      end: 9,
      segments: [
        { start: 190, end: 199 },
        { start: 0, end: 9 },
      ],
    };
    // A range covering only one segment keeps the feature.
    expect(dropCoveredFeatures([wrap], 150, 199)).toEqual([wrap]);
    // Replacing the whole circle covers both segments → dropped.
    expect(dropCoveredFeatures([wrap], 0, 199)).toEqual([]);
  });
});
