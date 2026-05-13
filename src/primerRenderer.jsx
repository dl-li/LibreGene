import { getX, cw } from './editorConstants';

// Shared primer segment geometry computation.

export const mismatchOffset = 4; // px offset away from main chain for mismatches/gaps/tails

// Normalize alignment column to new format (supports both old {col,op,disp,tmpl} and new {templateCol,kind,primerBase,templateBase}).
export const normCol = (ac) => ({
  templateCol: ac.templateCol ?? ac.col ?? 0,
  kind: ac.kind || (ac.op === 'M' ? 'match' : ac.op === 'X' ? 'mismatch' : ac.op === 'D' || ac.op === 'Del' ? 'gap' : 'mismatch'),
  primerBase: ac.primerBase ?? ac.disp ?? '',
  templateBase: ac.templateBase ?? ac.tmpl ?? '',
  insertionAfter: ac.insertionAfter ?? null,
});

/**
 * Compute rendering segments for a primer's best binding site.
 *
 * New data model (from backend):
 *   primer.bindingSites[0] = {
 *     matchStart, matchEnd, tm,
 *     fivePrimeTail, threePrimeTail,
 *     alignment: [{ templateCol, kind: "match"|"mismatch"|"gap",
 *                   primerBase, templateBase, insertionAfter }]
 *   }
 */
export function computePrimerSegments(primer, charsPerLine) {
  const isFwd = primer.type === 'fwd';
  const bs = primer.bindingSites?.[0];
  if (!bs || !bs.alignment?.length) return null;

  const { fivePrimeTail, threePrimeTail, alignment } = bs;

  // Normalize all columns to new format (supports old-format data).
  const normAlign = alignment.map(normCol);

  // Group alignment columns by row.
  const segs = [];
  let i = 0;
  while (i < normAlign.length) {
    const col = normAlign[i].templateCol;
    const row = Math.floor(col / charsPerLine);
    const rowEndCol = (row + 1) * charsPerLine - 1;
    const segCols = [];
    while (i < normAlign.length && normAlign[i].templateCol <= rowEndCol) {
      segCols.push(normAlign[i]);
      i++;
    }
    if (!segCols.length) { i++; continue; }
    segs.push({
      row,
      colStart: segCols[0].templateCol % charsPerLine,
      colEnd: segCols[segCols.length - 1].templateCol % charsPerLine,
      alignmentCols: segCols,
    });
  }

  if (!segs.length) return null;

  const hasFivePrimeTail = (fivePrimeTail || '').length > 0;
  const hasThreePrimeTail = (threePrimeTail || '').length > 0;

  // 3' end: fwd=right(last seg), rev=left(first seg)
  const threePrimeSeg = isFwd ? segs[segs.length - 1] : segs[0];
  // 5' end: fwd=left(first seg), rev=right(last seg)
  const fivePrimeSeg = isFwd ? segs[0] : segs[segs.length - 1];

  return {
    segs,
    isFwd,
    fivePrimeTail: fivePrimeTail || '',
    threePrimeTail: threePrimeTail || '',
    hasFivePrimeTail,
    hasThreePrimeTail,
    threePrimeSeg,
    fivePrimeSeg,
  };
}

/**
 * Build the SVG path for a single primer segment.
 *
 * Path order:
 *   5'edge → column centers (matchY for matches, mismatchY for mismatches/gaps) →
 *   3'edge → [arrow tip] → [3' tail]
 *
 * For rev primers, columns are traversed right-to-left.
 */
export function buildSegmentPath(
  seg, matchY, mismatchY, misY, isFwd, charsPerLine,
  hasArrow, hasThreePrimeTail, threePrimeTail, arrowWidth, arrowHeight
) {
  const { alignmentCols } = seg;
  if (!alignmentCols?.length) return null;

  const N = alignmentCols.length;

  // For rev: traverse right→left (reverse order)
  const order = isFwd
    ? alignmentCols.map((_, k) => k)
    : alignmentCols.map((_, k) => N - 1 - k);

  const cx = order.map(k => getX(alignmentCols[k].templateCol % charsPerLine) + cw / 2);
  const cy = order.map(k => {
    const kind = alignmentCols[k].kind;
    return (kind === 'mismatch' || kind === 'gap') ? mismatchY : matchY;
  });

  const firstK = order[0];
  const lastK = order[N - 1];

  // 5' and 3' edges in display order
  const edge5x = isFwd
    ? getX(alignmentCols[firstK].templateCol % charsPerLine)
    : getX(alignmentCols[firstK].templateCol % charsPerLine) + cw;
  const edge3x = isFwd
    ? getX(alignmentCols[lastK].templateCol % charsPerLine) + cw
    : getX(alignmentCols[lastK].templateCol % charsPerLine);

  // Arrow at 3' end
  let tipX = null, tipY = null;
  if (hasArrow) {
    tipX = isFwd ? edge3x - arrowWidth : edge3x + arrowWidth;
    tipY = isFwd ? cy[cy.length - 1] - arrowHeight : cy[cy.length - 1] + arrowHeight;
  }

  // Build path: 5' edge → centers → 3' edge → arrow → 3' tail
  const pts = [];
  pts.push([edge5x, cy[0]]);
  for (let k = 0; k < N; k++) pts.push([cx[k], cy[k]]);
  pts.push([edge3x, cy[cy.length - 1]]);

  if (tipX !== null) pts.push([tipX, tipY]);

  // 3' tail
  if (hasThreePrimeTail && threePrimeTail) {
    const tailLen = threePrimeTail.length;
    pts.push([edge3x, misY]);
    const tailEndX = isFwd
      ? getX(alignmentCols[lastK].templateCol + tailLen + 1) + cw / 2
      : getX(alignmentCols[lastK].templateCol - tailLen) - cw / 2;
    pts.push([tailEndX, misY]);
  }

  const d = `M ${pts.map(p => `${p[0]} ${p[1]}`).join(' L ')}`;
  return { d, edge5x, edge3x, pts, cy, order, alignmentCols, firstK, lastK };
}

/**
 * Build the expanded (hover/selected) background polygon.
 * Follows the line with offset for mismatches and gaps.
 */
export function buildSegmentHoverPath(
  seg, matchY, mismatchY, misY, isFwd, charsPerLine,
  primerExpand, hasThreePrimeTail, threePrimeTail, hasFivePrimeTail, fivePrimeTail
) {
  const { alignmentCols } = seg;
  if (!alignmentCols?.length) return null;

  const N = alignmentCols.length;
  const order = isFwd
    ? alignmentCols.map((_, k) => k)
    : alignmentCols.map((_, k) => N - 1 - k);

  const cx = order.map(k => getX(alignmentCols[k].templateCol % charsPerLine) + cw / 2);
  const cy = order.map(k => {
    const kind = alignmentCols[k].kind;
    return (kind === 'mismatch' || kind === 'gap') ? mismatchY : matchY;
  });

  const firstK = order[0];
  const lastK = order[N - 1];

  const edge5x = isFwd
    ? getX(alignmentCols[firstK].templateCol % charsPerLine)
    : getX(alignmentCols[firstK].templateCol % charsPerLine) + cw;
  const edge3x = isFwd
    ? getX(alignmentCols[lastK].templateCol % charsPerLine) + cw
    : getX(alignmentCols[lastK].templateCol % charsPerLine);

  // Expanded edge: away from main chain
  const expD = isFwd ? -primerExpand : primerExpand;
  const topCy = isFwd ? cy.map(y => y - primerExpand) : cy;
  const botCy = isFwd ? cy : cy.map(y => y + primerExpand);

  const parts = [];

  // Forward along expanded (top) edge
  parts.push([edge5x, topCy[0]]);
  for (let k = 0; k < N; k++) parts.push([cx[k], topCy[k]]);
  parts.push([edge3x, topCy[topCy.length - 1]]);

  // 5' tail on expanded side
  if (hasFivePrimeTail && fivePrimeTail) {
    const tailLen = fivePrimeTail.length;
    const tailEndX = isFwd
      ? getX(alignmentCols[firstK].templateCol - tailLen)
      : getX(alignmentCols[firstK].templateCol + tailLen + 1) + cw;
    parts.push([tailEndX, misY + (isFwd ? -primerExpand : 0)]);
  }

  // 3' tail on expanded side
  if (hasThreePrimeTail && threePrimeTail) {
    const tailLen = threePrimeTail.length;
    const tailEndX = isFwd
      ? getX(alignmentCols[lastK].templateCol + tailLen + 1) + cw
      : getX(alignmentCols[lastK].templateCol - tailLen) - cw;
    parts.push([tailEndX, misY + (isFwd ? -primerExpand : 0)]);
  }

  // Backward along bottom edge
  if (hasThreePrimeTail && threePrimeTail) {
    const tailLen = threePrimeTail.length;
    const tailEndX = isFwd
      ? getX(alignmentCols[lastK].templateCol + tailLen + 1) + cw
      : getX(alignmentCols[lastK].templateCol - tailLen) - cw;
    parts.push([tailEndX, misY + (isFwd ? 0 : primerExpand)]);
  }

  parts.push([edge3x, botCy[botCy.length - 1]]);
  for (let k = N - 1; k >= 0; k--) parts.push([cx[k], botCy[k]]);
  parts.push([edge5x, botCy[0]]);

  // 5' tail on bottom side
  if (hasFivePrimeTail && fivePrimeTail) {
    const tailLen = fivePrimeTail.length;
    const tailEndX = isFwd
      ? getX(alignmentCols[firstK].templateCol - tailLen)
      : getX(alignmentCols[firstK].templateCol + tailLen + 1) + cw;
    parts.push([tailEndX, misY + (isFwd ? 0 : primerExpand)]);
  }

  return `M ${parts.map(p => `${p[0]} ${p[1]}`).join(' L ')} Z`;
}
