import { describe, it, expect } from 'vitest';
import { collectAnnotations, parseMetaFromPasteEvent } from '../clipboardAnnotations';

function fakePasteEvent(payload) {
  return {
    clipboardData: {
      getData: (type) =>
        type === 'application/x-libregene-annotations' ? JSON.stringify(payload) : '',
    },
  };
}

function validMeta() {
  return collectAnnotations(
    {
      features: [
        {
          name: 'gene',
          ftype: 'CDS',
          strand: '+',
          segments: [{ start: 10, end: 29 }],
        },
      ],
      primers: [{ name: 'p1', type: 'fwd', primerSeq: 'ACGTACGTAC', bindingSites: [{ templateStart: 5, templateEnd: 15 }] }],
    },
    0,
    99,
  );
}

describe('parseMetaFromPasteEvent (forged-flavor hardening)', () => {
  it('accepts the shape collectAnnotations produces', () => {
    const meta = validMeta();
    expect(meta).not.toBeNull();
    const parsed = parseMetaFromPasteEvent(fakePasteEvent(meta));
    expect(parsed).not.toBeNull();
    expect(parsed.features).toHaveLength(1);
    expect(parsed.features[0].segments).toEqual([{ start: 10, end: 29 }]);
    expect(parsed.primers).toHaveLength(1);
  });

  it('rejects missing/wrong app marker and version', () => {
    expect(parseMetaFromPasteEvent(fakePasteEvent({ app: 'other', version: 1 }))).toBeNull();
    expect(parseMetaFromPasteEvent(fakePasteEvent({ app: 'libregene', version: 2 }))).toBeNull();
    expect(parseMetaFromPasteEvent(fakePasteEvent(null))).toBeNull();
  });

  it('rejects non-integer or absurd length', () => {
    const base = validMeta();
    expect(parseMetaFromPasteEvent(fakePasteEvent({ ...base, length: '100' }))).toBeNull();
    expect(parseMetaFromPasteEvent(fakePasteEvent({ ...base, length: 0 }))).toBeNull();
    expect(parseMetaFromPasteEvent(fakePasteEvent({ ...base, length: 1e12 }))).toBeNull();
  });

  it('rejects payload with no arrays', () => {
    expect(
      parseMetaFromPasteEvent(
        fakePasteEvent({ app: 'libregene', version: 1, length: 100 }),
      ),
    ).toBeNull();
  });

  it('drops features whose segments fall outside [0, length)', () => {
    const base = validMeta();
    base.features[0].segments = [{ start: -5, end: 10 }];
    const outOfRange = parseMetaFromPasteEvent(fakePasteEvent(base));
    expect(outOfRange?.features ?? []).toHaveLength(0);
    base.features[0].segments = [{ start: 90, end: 150 }];
    expect(parseMetaFromPasteEvent(fakePasteEvent(base))?.features ?? []).toHaveLength(0);
    base.features[0].segments = [{ start: 20, end: 10 }];
    expect(parseMetaFromPasteEvent(fakePasteEvent(base))?.features ?? []).toHaveLength(0);
  });

  it('drops features with non-integer segment coordinates', () => {
    const base = validMeta();
    base.features[0].segments = [{ start: 1.5, end: 10 }];
    expect(parseMetaFromPasteEvent(fakePasteEvent(base))?.features ?? []).toHaveLength(0);
  });

  it('drops features with empty or oversized segment lists', () => {
    const base = validMeta();
    base.features[0].segments = [];
    expect(parseMetaFromPasteEvent(fakePasteEvent(base))?.features ?? []).toHaveLength(0);
    base.features[0].segments = Array.from({ length: 100 }, (_, i) => ({ start: i, end: i }));
    expect(parseMetaFromPasteEvent(fakePasteEvent(base))?.features ?? []).toHaveLength(0);
  });

  it('caps feature and primer counts', () => {
    const spam = Array.from({ length: 500 }, (_, i) => ({
      name: `f${i}`,
      segments: [{ start: 0, end: 9 }],
    }));
    const base = validMeta();
    base.features = spam;
    expect(parseMetaFromPasteEvent(fakePasteEvent(base))).toBeNull();
  });

  it('sanitizes primer sequences to IUPAC and drops bad ones', () => {
    const base = validMeta();
    base.primers = [
      { name: 'ok', type: 'rev', primerSeq: 'ACGTRYSW' },
      { name: 'bad', type: 'fwd', primerSeq: 'ACGT1234!' },
    ];
    const parsed = parseMetaFromPasteEvent(fakePasteEvent(base));
    expect(parsed.primers).toHaveLength(1);
    expect(parsed.primers[0].name).toBe('ok');
  });

  it('drops malformed feature entries but keeps valid siblings', () => {
    const base = validMeta();
    base.features = [
      ...base.features,
      { name: 42, segments: [{ start: 0, end: 5 }] },
      { name: 'ok2', segments: [{ start: 0, end: 5 }] },
    ];
    const parsed = parseMetaFromPasteEvent(fakePasteEvent(base));
    expect(parsed.features.map((f) => f.name)).toEqual(['gene', 'ok2']);
  });

  it('returns null on invalid JSON', () => {
    const e = {
      clipboardData: { getData: () => '{not json' },
    };
    expect(parseMetaFromPasteEvent(e)).toBeNull();
  });

  it('returns null when flavor absent', () => {
    const e = { clipboardData: { getData: () => '' } };
    expect(parseMetaFromPasteEvent(e)).toBeNull();
  });
});
