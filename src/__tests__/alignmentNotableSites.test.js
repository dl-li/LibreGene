// Unit tests for alignmentNotableSites (prev/next navigation anchors).
import { describe, it, expect } from 'vitest';
import { alignmentNotableSites } from '../editor/alignmentLayout';

// All-A template of length 100; matching read chars are 'A'.
const SEQ = 'A'.repeat(100);
const TLEN = SEQ.length;

describe('alignmentNotableSites', () => {
  it('returns no sites for a perfect full-circle match', () => {
    const al = { segments: [{ start: 0, end: 99, chars: SEQ }], insertions: [] };
    expect(alignmentNotableSites(al, SEQ, TLEN)).toEqual([]);
  });

  it('returns only start/end for a perfect partial match', () => {
    const al = { segments: [{ start: 10, end: 20, chars: 'A'.repeat(11) }], insertions: [] };
    expect(alignmentNotableSites(al, SEQ, TLEN)).toEqual([10, 20]);
  });

  it('collapses a consecutive mismatch run to one anchor at its start', () => {
    const chars = 'AA' + 'CC' + 'A'.repeat(7); // red at 12, 13
    const al = { segments: [{ start: 10, end: 20, chars }], insertions: [] };
    expect(alignmentNotableSites(al, SEQ, TLEN)).toEqual([10, 12, 20]);
  });

  it("treats '-' read gaps as plated columns", () => {
    const chars = 'AAAAA-A'; // red at 15
    const al = { segments: [{ start: 10, end: 16, chars }], insertions: [] };
    expect(alignmentNotableSites(al, SEQ, TLEN)).toEqual([10, 15, 16]);
  });

  it('anchors a template gap between segments once, at the gap start', () => {
    const al = {
      segments: [
        { start: 10, end: 14, chars: 'AAAAA' },
        { start: 20, end: 24, chars: 'AAAAA' },
      ],
      insertions: [],
    };
    expect(alignmentNotableSites(al, SEQ, TLEN)).toEqual([10, 15, 24]);
  });

  it('anchors a lone insertion at its template anchor column', () => {
    const al = {
      segments: [{ start: 10, end: 20, chars: 'A'.repeat(11) }],
      insertions: [{ pos: 15, bases: 'GG' }],
    };
    expect(alignmentNotableSites(al, SEQ, TLEN)).toEqual([10, 15, 20]);
  });

  it('merges an insertion touching a red run into that run', () => {
    const chars = 'AAAA' + 'C' + 'A'.repeat(6); // red at 14
    const al = {
      segments: [{ start: 10, end: 20, chars }],
      insertions: [{ pos: 15, bases: 'GG' }],
    };
    expect(alignmentNotableSites(al, SEQ, TLEN)).toEqual([10, 14, 20]);
  });

  it('merges a track end inside a trailing red run into the run', () => {
    const chars = 'A'.repeat(9) + 'CC'; // red run 19-20 covers the end
    const al = { segments: [{ start: 10, end: 20, chars }], insertions: [] };
    expect(alignmentNotableSites(al, SEQ, TLEN)).toEqual([10, 19]);
  });

  it('handles a run crossing the origin of a circular template', () => {
    // cols 95..99,0..4; red at 99 and 0 → one wrapped run anchored at 99
    const chars = 'AAAA' + 'CC' + 'AAAA';
    const al = { segments: [{ start: 95, end: 4, chars }], insertions: [] };
    expect(alignmentNotableSites(al, SEQ, TLEN)).toEqual([4, 95, 99]);
  });

  it('falls back to a single anchor when the whole circle is plated', () => {
    const al = { segments: [{ start: 0, end: 99, chars: 'C'.repeat(100) }], insertions: [] };
    expect(alignmentNotableSites(al, SEQ, TLEN)).toEqual([0]);
  });
});
