import { describe, it, expect, vi } from 'vitest';
import {
  buildPrimerImports,
  dropCoveredFeatures,
  restoreEditSnapshot,
} from '../workspace/useEditDialog';

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

describe('restoreEditSnapshot', () => {
  it('restores every optimistic UI field and the previous dirty value', () => {
    const setters = {
      setSequence: vi.fn(),
      setFeatures: vi.fn(),
      setTextColorsTop: vi.fn(),
      setTextColorsBottom: vi.fn(),
      setAlignments: vi.fn(),
      setIsDirty: vi.fn(),
    };
    const snapshot = {
      sequence: 'AAAA',
      features: [{ id: 'f' }],
      textColorsTop: [{ start: 0, end: 1 }],
      textColorsBottom: [{ start: 2, end: 3 }],
      alignments: [{ id: 'a' }],
      isDirty: false,
    };

    restoreEditSnapshot(snapshot, setters);
    expect(setters.setSequence).toHaveBeenCalledWith('AAAA');
    expect(setters.setFeatures).toHaveBeenCalledWith(snapshot.features);
    expect(setters.setTextColorsTop).toHaveBeenCalledWith(snapshot.textColorsTop);
    expect(setters.setTextColorsBottom).toHaveBeenCalledWith(snapshot.textColorsBottom);
    expect(setters.setAlignments).toHaveBeenCalledWith(snapshot.alignments);
    expect(setters.setIsDirty).toHaveBeenCalledWith(false);
  });
});

describe('buildPrimerImports', () => {
  it('creates complete unique primer inputs and preserves name deduplication', () => {
    const annotations = [
      { name: 'Forward', type: 'fwd', primerSeq: 'AACCGG' },
      { name: 'Forward', type: 'rev', primerSeq: 'TTGGCC' },
    ];
    const existing = [{ name: 'Forward' }, { name: 'Forward (2)' }];

    const imports = buildPrimerImports(annotations, existing);

    expect(imports).toMatchObject([
      { name: 'Forward (3)', type: 'fwd', primerSeq: 'AACCGG' },
      { name: 'Forward (4)', type: 'rev', primerSeq: 'TTGGCC' },
    ]);
    expect(imports.every((primer) => typeof primer.id === 'string' && primer.id.length > 0)).toBe(
      true,
    );
    expect(new Set(imports.map((primer) => primer.id)).size).toBe(2);
  });
});
