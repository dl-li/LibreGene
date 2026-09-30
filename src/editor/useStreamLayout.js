import { useCallback, useMemo } from 'react';
import { cw, startX } from '../editorConstants';
import { alignmentInsertUnion, buildStreamLayout } from './alignmentLayout';

// Shared visual-stream layout for the sequence editor: insertion slot
// reservation, the per-row cell stream, and the column mapping helpers every
// lane (template chars, features, enzymes, primers, translation,
// chromatogram, selection, cursor) goes through.
export default function useStreamLayout({ isDna, alignmentTracks, seqLen, baseCpl }) {
  // Insertion reserve: every inserted read base (flank junk included) gets a
  // full cell in the shared visual stream (GenePad's merged gap columns), one
  // cell per base at its own anchor — the read's bases must stay in read order
  // or its chromatogram trace would have to jump around them.
  const insReserve = useMemo(
    () => (isDna ? alignmentInsertUnion(alignmentTracks, seqLen) : new Map()),
    [isDna, alignmentTracks, seqLen],
  );
  // Visual stream: template columns + slot cells, exactly baseCpl cells per
  // row — the row width is constant and never overflows the viewport.
  const stream = useMemo(
    () => buildStreamLayout(insReserve, seqLen, baseCpl),
    [insReserve, seqLen, baseCpl],
  );
  const { rowStarts, rowCounts, insTotal, streamOf, rowOf, colOfAbs, visCpl, absFromStream } =
    stream;
  const charsPerLine = baseCpl;
  const numRows = stream.numRows;
  const svgWidth = startX + charsPerLine * cw + startX;

  // --- stream column mapping (shared by every lane) ---
  // Visual column of the row-local template column `col` in row `row`: its
  // stream index modulo the row width. Slot cells sit between the template
  // columns of a row (or fill pure-slot rows of a wide block). Every lane
  // (template chars, features, enzymes, primers, translation, chromatogram,
  // selection, cursor) goes through these so all tracks stay column-aligned.
  const colVis = useCallback((col, row) => stream.colVis(col, row), [stream]);

  // Inverse of colVis: template column whose stream cell reaches `vis`.
  // Clicks on a slot cell resolve to the slot's anchor column (the cell
  // renders left of it).
  const colFromVis = useCallback((vis, row) => stream.colFromVis(vis, row), [stream]);

  // Split the template range [c0, c1] (row-local, inclusive) into visual
  // runs [[visStart, len], ...] separated at slot cells.
  const colRuns = useCallback(
    (c0, c1, row) => {
      if (c1 < c0) return [];
      if (insTotal === 0) return [[c0, c1 - c0 + 1]];
      const runs = [];
      let runStart = c0;
      for (let c = c0; c <= c1; c++) {
        const vHere = colVis(c, row);
        if (c === c1 || colVis(c + 1, row) !== vHere + 1) {
          runs.push([colVis(runStart, row), c - runStart + 1]);
          runStart = c + 1;
        }
      }
      return runs;
    },
    [colVis, insTotal],
  );

  const sp = useCallback((s, e) => stream.sp(s, e), [stream]);

  return {
    insReserve,
    stream,
    rowStarts,
    rowCounts,
    insTotal,
    streamOf,
    rowOf,
    colOfAbs,
    visCpl,
    absFromStream,
    charsPerLine,
    numRows,
    svgWidth,
    colVis,
    colFromVis,
    colRuns,
    sp,
  };
}
