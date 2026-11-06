import { describe, expect, it } from 'vitest';
import { resolvePrimerOverviewSites } from '../dialogs/PrimerOverviewDialog';

const storedSite = { templateStart: 4, templateEnd: 10, strand: 1, tm: 50 };
const storedAlternative = { templateStart: 20, templateEnd: 26, strand: -1, tm: 45 };
const currentSite = { start: 4, end: 10, strand: 1, tm: 60 };
const primer = { id: 'p1', bindingSites: [storedSite, storedAlternative] };

describe('PrimerOverviewDialog Tm conditions', () => {
  it('uses the cache computed with the current conditions', () => {
    const result = resolvePrimerOverviewSites(
      primer,
      { current: currentSite, alternatives: [] },
      false,
    );
    expect(result).toEqual({ primary: currentSite, alternatives: [] });
  });

  it('uses stored binding sites only for compatibility defaults', () => {
    expect(resolvePrimerOverviewSites(primer, undefined, true)).toEqual({
      primary: storedSite,
      alternatives: [storedAlternative],
    });
    expect(resolvePrimerOverviewSites(primer, undefined, false)).toEqual({
      primary: null,
      alternatives: [],
    });
  });
});
