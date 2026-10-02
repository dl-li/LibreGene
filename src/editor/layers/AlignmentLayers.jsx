import React, { useState } from 'react';
import { cw, bgColor, featLabelW, getX, monoFont } from '../../editorConstants';
import { BASE_HILITE_BG, alignmentGapSegments, insertionBases } from '../alignmentLayout';
import { EyeOff } from 'lucide-react';
import MonoRun from './MonoRun';

// Italic read-lane font, as a canvas-measurable shorthand for MonoRun's dx.
const READ_FONT = `italic 350 13px ${monoFont}`;

// Read text lanes of the stored alignments (mismatch plates + italic read
// bases + insertion slot bases). Read-only — no handlers.
const AlignmentTextLanes = React.memo(function AlignmentTextLanes({
  alignmentTracks,
  visibleRows,
  rowBuf,
  numRows,
  sp,
  getSeqY,
  lp,
  rowStarts,
  visCpl,
  streamOf,
  colRuns,
  insReserve,
  alignLaneInfo,
  sequence,
  cleanSeq,
}) {
  if (!alignmentTracks.length) return null;
  const vs = Math.max(0, visibleRows.start - rowBuf);
  const ve = Math.min(numRows - 1, visibleRows.end + rowBuf);
  return alignmentTracks.map((al, ti) => {
    const laneY = (row) =>
      getSeqY(row) +
      lp.featBaseOffset +
      alignLaneInfo.trackH +
      alignLaneInfo.mainChromH +
      (alignLaneInfo.perRow[row]?.get(ti) ?? 0) * lp.featTrackHeight +
      8;
    const rows = [];
    const segs = [...(al.segments || []), ...alignmentGapSegments(al, cleanSeq.length)];
    // Segment pieces are keyed by position plus a running index: a model
    // whose segments overlap (older engine output) would otherwise repeat a
    // key, and React would drop or duplicate the lane content.
    let piece = 0;
    for (const seg of segs) {
      for (const v of sp(seg.start, seg.end)) {
        if (v.row < vs || v.row > ve) continue;
        const pieceKey = `${v.row}-${v.colStart}-${piece++}`;
        const y = laneY(v.row);
        const chars = (seg.chars || '').slice(v.strOffset, v.strOffset + v.len);
        // Visually contiguous runs (split at insertion slots): each run renders
        // as ONE tspan pinned to len*cw by textLength, so a lane row costs a
        // handful of nodes instead of one per base. Mismatch/gap plates merge
        // the same way. Insertion-adjacent columns are not flagged: inserted
        // cells mark themselves, and a red wash behind the flanking matches
        // just muddies the lane.
        const plates = [];
        const tspans = [];
        let cc = v.colStart;
        for (const [visStart, runLen] of colRuns(v.colStart, v.colEnd, v.row)) {
          const runChars = chars.slice(cc - v.colStart, cc - v.colStart + runLen);
          let mStart = -1;
          for (let i = 0; i < runLen; i++) {
            const c = runChars[i];
            const bad =
              c === '-' ||
              c.toUpperCase() !== (sequence[rowStarts[v.row] + cc + i] || '').toUpperCase();
            if (bad) {
              if (mStart < 0) mStart = i;
            } else if (mStart >= 0) {
              plates.push([visStart + mStart, i - mStart]);
              mStart = -1;
            }
          }
          if (mStart >= 0) plates.push([visStart + mStart, runLen - mStart]);
          tspans.push(
            <MonoRun
              key={visStart}
              visStart={visStart}
              text={runChars}
              font={READ_FONT}
              fill="#1f2937"
              fillOpacity={0.55}
            />,
          );
          cc += runLen;
        }
        rows.push(
          <g key={pieceKey}>
            {plates.map(([vis, n]) => (
              <rect
                key={vis}
                x={getX(vis)}
                y={y - 11}
                width={n * cw}
                height={14}
                fill={BASE_HILITE_BG}
                fillOpacity={0.6}
                style={{ pointerEvents: 'none' }}
              />
            ))}
            <text
              y={y}
              fontFamily="Cascadia Code"
              fontSize="13px"
              fontStyle="italic"
              fontWeight="350"
              style={{ userSelect: 'none' }}
            >
              {tspans}
            </text>
          </g>,
        );
      }
    }
    // Inserted read bases (internal junctions and unalignable flank junk
    // alike) expand into their block's reserved cells: a merged block of
    // width w anchored at `a` occupies stream [S(a)-w, S(a)-1] left of the
    // anchor column, and each insertion renders in its own sub-slot at
    // `offset`. A wide block spans rows, so every base lands on its own row.
    // Each base sits on a pink plate — the template row shows '-' there, so
    // the plate marks the read bases that have no template column.
    const insByRow = new Map();
    for (const [pos, insBases] of insertionBases(al)) {
      const slotN = insReserve.get(pos) || 0;
      const cell0 = streamOf(pos) - slotN;
      for (let k = 0; k < Math.min(slotN, insBases.length); k++) {
        const si = cell0 + k;
        const row = Math.floor(si / visCpl);
        if (row < vs || row > ve) continue;
        if (!insByRow.has(row)) insByRow.set(row, []);
        insByRow.get(row).push({ vis: si % visCpl, char: insBases[k] });
      }
    }
    for (const [row, cells] of insByRow) {
      // Merge adjacent slot cells into runs (one plate + one tspan each);
      // cells of different anchors stay separate unless truly adjacent.
      cells.sort((a, b) => a.vis - b.vis);
      const runs = [];
      for (const cell of cells) {
        const last = runs[runs.length - 1];
        if (last && cell.vis === last.visStart + last.text.length) last.text += cell.char;
        else runs.push({ visStart: cell.vis, text: cell.char });
      }
      rows.push(
        <g key={`ins-${row}`}>
          {runs.map((g) => (
            <rect
              key={g.visStart}
              x={getX(g.visStart)}
              y={laneY(row) - 11}
              width={g.text.length * cw}
              height={14}
              fill={BASE_HILITE_BG}
              fillOpacity={0.6}
              style={{ pointerEvents: 'none' }}
            />
          ))}
          <text
            y={laneY(row)}
            fontFamily="Cascadia Code"
            fontSize="13px"
            fontStyle="italic"
            fontWeight="350"
            style={{ userSelect: 'none' }}
          >
            {runs.map((g) => (
              <MonoRun
                key={g.visStart}
                visStart={g.visStart}
                text={g.text}
                font={READ_FONT}
                className="ins-base"
                fill="#1f2937"
                fillOpacity={0.55}
                style={{ userSelect: 'none', pointerEvents: 'none' }}
              />
            ))}
          </text>
        </g>,
      );
    }
    return <g key={al.id}>{rows}</g>;
  });
});

// Right-aligned per-row alignment labels: marquee on hover for truncated
// names, eye-off hide button, and a clickable underline for alignments whose
// trace .ab1 resolved (toggles the chromatogram band).
const AlignmentLabels = React.memo(function AlignmentLabels({
  alignmentTracks,
  visibleRows,
  rowBuf,
  numRows,
  sp,
  getSeqY,
  lp,
  alignLaneInfo,
  cleanSeq,
  colVis,
  insReserve,
  visCpl,
  streamOf,
  alignmentTraceAvailable,
  expandedChromAlnId,
  onToggleAlignmentChrom,
  onHideAlignment,
}) {
  const [hoverAlignLabel, setHoverAlignLabel] = useState(null); // `${alignmentId}:${row}`
  if (!alignmentTracks.length) return null;
  const vs = Math.max(0, visibleRows.start - rowBuf);
  const ve = Math.min(numRows - 1, visibleRows.end + rowBuf);
  const textProps = {
    fontSize: '12px',
    fontFamily: 'TeX Gyre Heros',
    fontWeight: '600',
    fontStyle: 'italic',
    fill: '#78716C',
  };
  // Long names are middle-truncated to LABEL_MAX_W; hovering scrolls the
  // full name via a CSS marquee (distance = overflow width).
  const LABEL_MAX_W = 140;
  const middleTruncate = (name) => {
    if (featLabelW(name) <= LABEL_MAX_W) return name;
    let keep = name.length - 1;
    let s = name;
    while (keep > 4) {
      const l = Math.ceil(keep / 2);
      const r = Math.floor(keep / 2);
      s = `${name.slice(0, l)}…${name.slice(name.length - r)}`;
      if (featLabelW(s) <= LABEL_MAX_W) return s;
      keep--;
    }
    return s;
  };
  return alignmentTracks.map((al, ti) => {
    const rowLabels = {};
    // Real segments and gap-dash placeholder runs both mark a row as part
    // of this track, so gap-only rows get the right-margin label too.
    for (const seg of [...(al.segments || []), ...alignmentGapSegments(al, cleanSeq.length)]) {
      for (const v of sp(seg.start, seg.end)) {
        if (v.row < vs || v.row > ve) continue;
        if (!rowLabels[v.row] || v.colEnd > rowLabels[v.row].colEnd) rowLabels[v.row] = v;
      }
    }
    const short = middleTruncate(al.name);
    const truncated = short !== al.name;
    // Right edge (exclusive visual column) of this read's content per row:
    // the last aligned column plus every insertion cell rendered in that
    // row — including the cells of an insertion anchored at the first
    // column of the NEXT row, which occupy the tail of this one.
    const rowRight = {};
    for (const v of Object.values(rowLabels)) {
      rowRight[v.row] = colVis(v.colEnd, v.row) + 1;
    }
    for (const [pos, insBases] of insertionBases(al)) {
      const slotN = insReserve.get(pos) || 0;
      const cell0 = streamOf(pos) - slotN;
      for (let k = 0; k < Math.min(slotN, insBases.length); k++) {
        const si = cell0 + k;
        const row = Math.floor(si / visCpl);
        if (rowRight[row] === undefined) continue;
        rowRight[row] = Math.max(rowRight[row], (si % visCpl) + 1);
      }
    }
    return (
      <g key={al.id}>
        {Object.values(rowLabels).map((v) => {
          const sy = getSeqY(v.row);
          const lane = alignLaneInfo.perRow[v.row]?.get(ti) ?? 0;
          // Labels sit a few px above the lane baseline to visually align
          // with the alignment text track.
          const y =
            sy +
            lp.featBaseOffset +
            alignLaneInfo.trackH +
            alignLaneInfo.mainChromH +
            lane * lp.featTrackHeight +
            6.5;
          const hKey = `${al.id}:${v.row}`;
          const labelHover = hoverAlignLabel === hKey;
          const hovered = truncated && labelHover;
          // Hug the right edge of THIS row's read content — rows with fewer
          // slots have nearer labels; they don't share a common column.
          const labelX = getX(rowRight[v.row]) + 8;
          const clipId = `align-label-clip-${al.id}-${v.row}`;
          const scrollW = hovered ? featLabelW(al.name) - featLabelW(short) + 4 : 0;
          // Trace toggle: labels of alignments whose .ab1 resolved are
          // underlined and clickable; the expanded track's label is inverted
          // (rect in label color, text in bgColor).
          const traceable = !!alignmentTraceAvailable?.has(al.id);
          const expanded = al.id === expandedChromAlnId;
          const labelText = hovered ? al.name : short;
          const labelW = Math.min(featLabelW(labelText), LABEL_MAX_W);
          const textEl = (
            <text
              x={labelX}
              y={y}
              textAnchor="start"
              {...textProps}
              fill={expanded ? bgColor : textProps.fill}
              style={{
                userSelect: 'none',
                pointerEvents: 'auto',
                textDecoration: traceable ? 'underline' : undefined,
                ...(hovered
                  ? {
                      '--align-label-scroll': `-${scrollW}px`,
                      animation: 'alignLabelScroll 2.5s linear infinite alternate',
                    }
                  : {}),
              }}
              onClick={
                traceable && onToggleAlignmentChrom
                  ? () => onToggleAlignmentChrom(al.id)
                  : undefined
              }
            >
              {labelText}
            </text>
          );
          return (
            <g
              key={v.row}
              style={{ cursor: traceable ? 'pointer' : 'default' }}
              onMouseEnter={() => setHoverAlignLabel(hKey)}
              onMouseLeave={() => setHoverAlignLabel(null)}
            >
              {truncated && (
                <defs>
                  <clipPath id={clipId}>
                    <rect x={labelX} y={y - 12} width={LABEL_MAX_W} height={16} />
                  </clipPath>
                </defs>
              )}
              {expanded && (
                <rect
                  x={labelX - 4}
                  y={y - 12}
                  width={labelW + 16}
                  height={16}
                  fill={textProps.fill}
                />
              )}
              {/* Invisible hit area bridging label and eye-off icon, so the
                  icon stays reachable while the pointer moves toward it. */}
              <rect
                x={labelX - 4}
                y={y - 12}
                width={labelW + (expanded ? 44 : 34)}
                height={16}
                fill="transparent"
              />
              {truncated ? <g clipPath={`url(#${clipId})`}>{textEl}</g> : textEl}
              {labelHover && onHideAlignment && (
                <EyeOff
                  x={labelX + labelW + (expanded ? 20 : 10)}
                  y={y - 11}
                  width={13}
                  height={13}
                  color={textProps.fill}
                  style={{ cursor: 'pointer' }}
                  onClick={(e) => {
                    e.stopPropagation();
                    onHideAlignment(al.id);
                  }}
                />
              )}
            </g>
          );
        })}
      </g>
    );
  });
});

// Both former alignment render memos, adjacent in the SVG just as before.
function AlignmentLayers(props) {
  const {
    alignmentTracks,
    visibleRows,
    rowBuf,
    numRows,
    sp,
    getSeqY,
    lp,
    rowStarts,
    visCpl,
    streamOf,
    colVis,
    colRuns,
    insReserve,
    alignLaneInfo,
    sequence,
    cleanSeq,
    alignmentTraceAvailable,
    expandedChromAlnId,
    onToggleAlignmentChrom,
    onHideAlignment,
  } = props;
  const laneProps = {
    alignmentTracks,
    visibleRows,
    rowBuf,
    numRows,
    sp,
    getSeqY,
    lp,
    rowStarts,
    visCpl,
    streamOf,
    colRuns,
    insReserve,
    alignLaneInfo,
    sequence,
    cleanSeq,
  };
  const labelProps = {
    alignmentTracks,
    visibleRows,
    rowBuf,
    numRows,
    sp,
    getSeqY,
    lp,
    alignLaneInfo,
    cleanSeq,
    colVis,
    insReserve,
    visCpl,
    streamOf,
    alignmentTraceAvailable,
    expandedChromAlnId,
    onToggleAlignmentChrom,
    onHideAlignment,
  };
  return (
    <>
      <AlignmentTextLanes {...laneProps} />
      <AlignmentLabels {...labelProps} />
    </>
  );
}

export default AlignmentLayers;
