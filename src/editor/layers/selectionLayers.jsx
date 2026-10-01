import React from 'react';
import { cw, bgColor, monoFont, getX, measureWidth, amplimerGreen } from '../../editorConstants';

// Read-only selection-state render layers. Each is React.memo'd and takes
// only the state/derived values its body reads.

export const CursorLayer = React.memo(function CursorLayer({
  cursorIndex,
  visible = true,
  hasSelection,
  isDragging,
  selectionMode,
  currentSelColor,
  rowOf,
  colOfAbs,
  numRows,
  getSeqY,
  rowAbove,
  rowBelow,
}) {
  if (cursorIndex === null || !visible) return null;
  if (hasSelection && !isDragging) return null;
  if (selectionMode !== 'text' && selectionMode !== 'none') return null;
  const row = rowOf(cursorIndex);
  const x = getX(colOfAbs(cursorIndex));
  const sy = getSeqY(row);
  const topY = sy - rowAbove[row];
  const botY = row === numRows - 1 ? sy + rowBelow[row] + 24 : getSeqY(row + 1) - rowAbove[row + 1];
  return (
    <g style={{ pointerEvents: 'none' }}>
      <line x1={x} x2={x} y1={topY} y2={botY} stroke={bgColor} strokeWidth="3" />
      <line x1={x} x2={x} y1={topY} y2={botY} stroke={currentSelColor} strokeWidth="1.5" />
    </g>
  );
});

export const DesignPickedLayer = React.memo(function DesignPickedLayer({
  designPick,
  getSeqY,
  sp,
  colRuns,
}) {
  if (!designPick || designPick.segments.length === 0) return null;
  return (
    <g style={{ pointerEvents: 'none' }}>
      {designPick.segments.flatMap((picked, i) =>
        sp(picked.start, picked.end).flatMap((seg) =>
          colRuns(seg.colStart, seg.colEnd, seg.row).map(([visStart, len]) => (
            <rect
              key={`pickedbg-${i}-${seg.row}-${seg.colStart}-${visStart}`}
              x={getX(visStart)}
              y={getSeqY(seg.row) - 19}
              width={len * cw}
              height={28}
              fill="#0f766e"
              fillOpacity={0.25}
              rx="1"
            />
          )),
        ),
      )}
    </g>
  );
});

export const SelectionLayer = React.memo(function SelectionLayer({
  hasSelection,
  selectionMode,
  isEnzymeSelection,
  selStart,
  selEnd,
  getSeqY,
  sp,
  currentSelColor,
  colRuns,
}) {
  if (!hasSelection || (selectionMode !== 'text' && !isEnzymeSelection)) return null;
  const segs = sp(selStart, selEnd);
  return (
    <g style={{ pointerEvents: 'none' }}>
      {segs.flatMap((seg) =>
        colRuns(seg.colStart, seg.colEnd, seg.row).map(([visStart, len]) => (
          <rect
            key={`selbg-${seg.row}-${seg.colStart}-${visStart}`}
            x={getX(visStart)}
            y={getSeqY(seg.row) - 19}
            width={len * cw}
            height={28}
            fill={currentSelColor}
            rx="1"
          />
        )),
      )}
    </g>
  );
});

// Selection overlay: only renders selected characters in white (grouped by row)
export const SeqSelLayer = React.memo(function SeqSelLayer({
  hasSelection,
  selectionMode,
  isEnzymeSelection,
  selStart,
  selEnd,
  cleanSeq,
  getSeqY,
  sp,
  rowStarts,
  colVis,
}) {
  if (!hasSelection || (selectionMode !== 'text' && !isEnzymeSelection)) return null;
  const segs = sp(selStart, selEnd);
  // Group segments by row
  const byRow = {};
  for (const seg of segs) {
    (byRow[seg.row] || (byRow[seg.row] = [])).push(seg);
  }
  return Object.entries(byRow).map(([rowStr, rowSegs]) => {
    const row = parseInt(rowStr, 10);
    const sy = getSeqY(row);
    const rowStart = rowStarts[row];
    return (
      <text
        key={`sel-${row}`}
        y={sy}
        fontFamily={monoFont}
        fontSize="14px"
        fontWeight="bold"
        style={{ userSelect: 'none', pointerEvents: 'none' }}
      >
        {rowSegs
          .map((seg) => {
            const chars = cleanSeq
              .substring(rowStart + seg.colStart, rowStart + seg.colEnd + 1)
              .split('');
            return chars.map((c, i) => (
              <tspan
                key={`${seg.colStart + i}`}
                x={getX(colVis(seg.colStart + i, row)) + cw / 2}
                textAnchor="middle"
                fill={bgColor}
              >
                {c}
              </tspan>
            ));
          })
          .flat()}
      </text>
    );
  });
});

// --- translation (codon) selection render ---
export const TranslationSelectionLayer = React.memo(function TranslationSelectionLayer({
  selectionMode,
  translationSel,
  cdsFeatureData,
  cleanSeq,
  currentSelColor,
  rowOf,
  rowStarts,
  getSeqY,
  colRuns,
  colVis,
}) {
  if (
    selectionMode !== 'translation' ||
    !translationSel ||
    !cdsFeatureData[translationSel.featureId]
  ) {
    return null;
  }
  const cds = cdsFeatureData[translationSel.featureId];
  const start = Math.min(translationSel.startCodon, translationSel.endCodon);
  const end = Math.max(translationSel.startCodon, translationSel.endCodon);
  const selectedBases = new Set();
  for (let i = start; i <= end; i++) {
    const t = cds.trans[i];
    if (!t) continue;
    for (const b of t.bases) selectedBases.add(b);
  }
  if (!selectedBases.size) return null;

  const byRow = {};
  for (const pos of selectedBases) {
    const row = rowOf(pos);
    const col = pos - rowStarts[row];
    (byRow[row] || (byRow[row] = [])).push(col);
  }

  const rects = [];
  const texts = [];
  for (const [rowStr, cols] of Object.entries(byRow)) {
    const row = parseInt(rowStr, 10);
    const sy = getSeqY(row);
    const rowStart = rowStarts[row];
    cols.sort((a, b) => a - b);

    const flush = (cs, ce) => {
      for (const [visStart, len] of colRuns(cs, ce, row)) {
        rects.push(
          <rect
            key={`trselbg-${row}-${cs}-${visStart}`}
            x={getX(visStart)}
            y={sy - 19}
            width={len * cw}
            height={28}
            fill={currentSelColor}
            rx="1"
          />,
        );
      }
      const chars = cleanSeq.substring(rowStart + cs, rowStart + ce + 1).split('');
      texts.push(
        <text
          key={`trseltxt-${row}-${cs}`}
          y={sy}
          fontFamily={monoFont}
          fontSize="14px"
          fontWeight="bold"
          style={{ userSelect: 'none', pointerEvents: 'none' }}
        >
          {chars.map((c, i) => (
            <tspan
              key={i}
              x={getX(colVis(cs + i, row)) + cw / 2}
              textAnchor="middle"
              fill={bgColor}
            >
              {c}
            </tspan>
          ))}
        </text>,
      );
    };

    let segStart = cols[0];
    let prev = cols[0];
    for (let i = 1; i < cols.length; i++) {
      if (cols[i] === prev + 1) {
        prev = cols[i];
      } else {
        flush(segStart, prev);
        segStart = prev = cols[i];
      }
    }
    flush(segStart, prev);
  }

  return (
    <g style={{ pointerEvents: 'none' }}>
      {rects}
      {texts}
    </g>
  );
});

// --- amplimer intervening region (deep green) ---
// Rendered after SeqBg + SeqSel so white text overrides dark text
export const AmplimerRegionLayer = React.memo(function AmplimerRegionLayer({
  selectionMode,
  selectedPrimerIds,
  enrichedPrimers,
  cleanSeq,
  topology,
  getSeqY,
  sp,
  rowStarts,
  colVis,
  colRuns,
}) {
  if (selectionMode !== 'amplimer' || selectedPrimerIds.length !== 2) return null;
  const fp = enrichedPrimers.find((p) => p.id === selectedPrimerIds[0]);
  const rp = enrichedPrimers.find((p) => p.id === selectedPrimerIds[1]);
  const fwdPrimer = fp && fp.isFwd ? fp : rp;
  const revPrimer = fp && !fp.isFwd ? fp : rp;
  if (!fwdPrimer || !revPrimer) return null;

  const segsByRange = (s, e) => {
    if (s > e || s >= cleanSeq.length || e < 0) return [];
    return sp(s, e);
  };

  let ranges;
  if (topology === 'circular' && fwdPrimer.matchEnd >= revPrimer.matchStart) {
    // Circular: wrap from fwd end+1 to end of seq, then 0 to rev start-1
    ranges = [
      segsByRange(fwdPrimer.matchEnd + 1, cleanSeq.length - 1),
      revPrimer.matchStart > 0 ? segsByRange(0, revPrimer.matchStart - 1) : [],
    ];
  } else {
    const s = fwdPrimer.matchEnd + 1;
    const e = revPrimer.matchStart - 1;
    ranges = [s <= e ? segsByRange(s, e) : []];
  }

  const allSegs = ranges.flat();
  if (!allSegs.length) return null;

  return (
    <g style={{ pointerEvents: 'none' }}>
      {allSegs.map((seg) =>
        colRuns(seg.colStart, seg.colEnd, seg.row).map(([visStart, len]) => (
          <rect
            key={`amp-${seg.row}-${seg.colStart}-${visStart}`}
            x={getX(visStart)}
            y={getSeqY(seg.row) - 19}
            width={len * cw}
            height={28}
            fill={amplimerGreen}
            rx="1"
          />
        )),
      )}
      {allSegs.map((seg) => {
        const rowStart = rowStarts[seg.row];
        const chars = cleanSeq
          .substring(rowStart + seg.colStart, rowStart + seg.colEnd + 1)
          .split('');
        return (
          <text
            key={`amp-txt-${seg.row}-${seg.colStart}`}
            y={getSeqY(seg.row)}
            fontFamily={monoFont}
            fontSize="14px"
            fontWeight="bold"
            style={{ userSelect: 'none', pointerEvents: 'none' }}
          >
            {chars.map((c, i) => (
              <tspan
                key={i}
                x={getX(colVis(seg.colStart + i, seg.row)) + cw / 2}
                textAnchor="middle"
                fill={bgColor}
              >
                {c}
              </tspan>
            ))}
          </text>
        );
      })}
    </g>
  );
});

// Selection length (+ Tm while dragging) badge following the cursor column
export const SelectionInfoLayer = React.memo(function SelectionInfoLayer({
  isDragging,
  hasSelection,
  cursorIndex,
  selectionMode,
  selStart,
  selEnd,
  currentSelColor,
  selectionTm,
  seqUnit,
  isDna,
  rowOf,
  colOfAbs,
  numRows,
  getSeqY,
  rowAbove,
  rowBelow,
}) {
  if (!isDragging || !hasSelection || cursorIndex === null) return null;
  if (selectionMode !== 'text') return null;
  const len = selEnd - selStart + 1;
  const tm = selectionTm;
  const showTm = tm !== null && tm >= 40 && tm <= 75;
  const row = rowOf(cursorIndex);
  const sy = getSeqY(row);
  const botY = row === numRows - 1 ? sy + rowBelow[row] + 24 : getSeqY(row + 1) - rowAbove[row + 1];
  const x = getX(colOfAbs(cursorIndex));
  const fontSize = '11px';
  const fontStr = `600 ${fontSize} ${monoFont}`;
  let label = `${len} ${seqUnit}`;
  if (isDna && showTm) label += `, ${tm}°C`;
  const tw = measureWidth(label, fontStr);
  return (
    <g style={{ pointerEvents: 'none' }}>
      <text
        dominantBaseline="text-after-edge"
        x={x - 8 - tw}
        y={botY}
        fontFamily={monoFont}
        fontSize={fontSize}
        fontWeight="600"
        fill={currentSelColor}
        stroke={bgColor}
        strokeWidth="2.5"
        strokeLinejoin="round"
        paintOrder="stroke"
        style={{ userSelect: 'none', whiteSpace: 'nowrap' }}
      >
        {label}
      </text>
    </g>
  );
});

export const HoverIndexLayer = React.memo(function HoverIndexLayer({
  hoveredIndex,
  isDragging,
  isTranslationDragging,
  rowOf,
  colOfAbs,
  getSeqY,
}) {
  if (hoveredIndex === null || isDragging || isTranslationDragging) return null;
  const row = rowOf(hoveredIndex);
  const sy = getSeqY(row);
  return (
    <g style={{ pointerEvents: 'none' }}>
      <text
        x={getX(colOfAbs(hoveredIndex)) + cw / 2}
        y={sy - 22}
        fontFamily={monoFont}
        fontSize="9px"
        fontWeight="600"
        fill="#A8A29E"
        stroke={bgColor}
        strokeWidth="2"
        strokeLinejoin="round"
        paintOrder="stroke"
        textAnchor="middle"
        style={{ pointerEvents: 'none', userSelect: 'none' }}
      >
        {hoveredIndex + 1}
      </text>
    </g>
  );
});
