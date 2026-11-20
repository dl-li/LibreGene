// Local (pre-recompute) mapping of enzyme sites and primer binding sites onto
// an edited sequence, mirroring adjustAnnotations/adjustAlignmentsForEdit.
import { describe, it, expect } from 'vitest';
import { adjustEnzymesForEdit, adjustPrimersForEdit } from '../derivedEdit';

const enzyme = (name, recStart, cutIndex, extra = {}) => ({
  id: `${name}_${recStart}_0`,
  name,
  recStart,
  recEnd: recStart + 5,
  cutIndex,
  botCutIndex: cutIndex + 1,
  displayStart: recStart - 1,
  displayEnd: recStart + 6,
  cutPairs: [{ topCutIndex: cutIndex, botCutIndex: cutIndex + 1 }],
  ...extra,
});

describe('adjustEnzymesForEdit', () => {
  it('keeps sites before the edit, shifts sites after it, drops the ones it touches', () => {
    const before = enzyme('EcoRI', 10, 11);
    const after = enzyme('BamHI', 100, 101);
    const spanning = enzyme('HindIII', 40, 41);

    const out = adjustEnzymesForEdit([before, after, spanning], 45, 54, 10, 0);
    expect(out.map((e) => e.name)).toEqual(['EcoRI', 'BamHI']);

    const shifted = out[1];
    expect(shifted.recStart).toBe(90);
    expect(shifted.recEnd).toBe(95);
    expect(shifted.cutIndex).toBe(91);
    expect(shifted.botCutIndex).toBe(92);
    expect(shifted.displayStart).toBe(89);
    expect(shifted.displayEnd).toBe(96);
    expect(shifted.cutPairs).toEqual([{ topCutIndex: 91, botCutIndex: 92 }]);
    // The untouched site keeps its identity.
    expect(out[0]).toBe(before);
    // Cut-pairs are cloned rather than mutated in place.
    expect(after.cutPairs).toEqual([{ topCutIndex: 101, botCutIndex: 102 }]);
  });

  it('drops a site whose recognition is before the edit but whose cut is after it', () => {
    // IIS-style layout: recognition at 100..105, cut 20 bp downstream.
    const iis = enzyme('BsaI', 100, 125);
    expect(adjustEnzymesForEdit([iis], 110, 119, 10, 0)).toEqual([]);
  });

  it('keeps an insertion-boundary site that ends right before the insertion', () => {
    const endsAtBoundary = { ...enzyme('XbaI', 94, 95), recEnd: 99 };
    const startsAtBoundary = enzyme('SpeI', 100, 101);
    const out = adjustEnzymesForEdit([endsAtBoundary, startsAtBoundary], 100, 99, 0, 4);
    expect(out.map((e) => e.recStart)).toEqual([94, 104]);
  });

  it('leaves equal-length replacements in place and drops only the covered site', () => {
    const before = enzyme('EcoRI', 10, 11);
    const covered = enzyme('BamHI', 40, 41);
    const after = enzyme('HindIII', 100, 101);
    const out = adjustEnzymesForEdit([before, covered, after], 45, 54, 10, 10);
    expect(out.map((e) => e.name)).toEqual(['EcoRI', 'HindIII']);
    expect(out[1].recStart).toBe(100);
  });

  it('returns the same array when nothing can change', () => {
    const list = [enzyme('EcoRI', 10, 11)];
    expect(adjustEnzymesForEdit(list, 500, 509, 10, 0)).toBe(list);
    expect(adjustEnzymesForEdit([], 0, 0, 0, 1)).toEqual([]);
  });
});

describe('adjustPrimersForEdit', () => {
  const site = (templateStart, templateEnd) => ({ templateStart, templateEnd, strand: 1 });

  it('shifts later binding sites and drops the ones the edit touches', () => {
    const primer = {
      id: 'p1',
      name: 'P1',
      bindingSites: [site(10, 30), site(40, 60), site(100, 120)],
    };
    const [out] = adjustPrimersForEdit([primer], 45, 54, 10, 0);
    expect(out.bindingSites.map((b) => [b.templateStart, b.templateEnd])).toEqual([
      [10, 30],
      [90, 110],
    ]);
    // The source list stays untouched.
    expect(primer.bindingSites[2].templateStart).toBe(100);
  });

  it('keeps a primer whose sites were all covered, with an empty site list', () => {
    const primer = { id: 'p1', name: 'P1', bindingSites: [site(45, 60)] };
    const [out] = adjustPrimersForEdit([primer], 45, 54, 10, 0);
    expect(out.bindingSites).toEqual([]);
  });

  it('keeps primers without binding sites and returns the same array when idle', () => {
    const idle = { id: 'p2', name: 'P2', bindingSites: [] };
    const list = [idle];
    expect(adjustPrimersForEdit(list, 0, 0, 0, 1)).toBe(list);
    expect(adjustPrimersForEdit([], 0, 0, 0, 1)).toEqual([]);
  });
});
