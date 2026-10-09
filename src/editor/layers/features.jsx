import {
  cw,
  startX,
  bgColor,
  monoFont,
  springAnim,
  getX,
  featureSelRange,
} from '../../editorConstants';
import { ensureReadableColor } from '../colors';
import { truncatedLabel } from '../seqUtils';
import { isTranslatable } from '../translation';

export function renderFeatures({
  visibleFeatures,
  hoveredFeature,
  alwaysExpandFeatures,
  sp,
  abutColors,
  rowStarts,
  getSeqY,
  alignLaneInfo,
  featureRowTracks,
  lp,
  ALIGN_FEAT_GAP,
  colRuns,
  colFromVis,
  colVis,
  solidLineCols,
  isDraggingRef,
  featureLeaveRef,
  setHoveredFeature,
  translationDragRef,
  cdsFeatureData,
  svgRef,
  setHoveredCodon,
  openFeatureMenu,
  featureSelRef,
  setSelStart,
  setSelEnd,
  setCursorIndex,
  clearCursorTimer,
  setSelectionMode,
  setSelectedPrimerIds,
  setTranslationSel,
  setIsTranslationDragging,
  isPrimerDraggingRef,
  primerDragRef,
  primerDimTimerRef,
  setPrimerDimActive,
  setCreateFeatureLoc,
  setFeatureInfoFeature,
  rowOf,
  hoveredCodon,
  startTranslationSelection,
}) {
  if (!visibleFeatures.length) return null;
  return visibleFeatures.map((f) => {
    const isHovered = hoveredFeature === f.id;
    const isExpanded = alwaysExpandFeatures || isHovered;
    const dataSegs = f.segments;

    const visuals = [];
    const seenRows = new Set();
    for (let di = 0; di < dataSegs.length; di++) {
      const ds = dataSegs[di];
      if (di > 0) {
        const prevEnd = dataSegs[di - 1].end;
        if (ds.start > prevEnd + 1) {
          for (const vs of sp(prevEnd + 1, ds.start - 1)) {
            const showL = !seenRows.has(vs.row);
            seenRows.add(vs.row);
            visuals.push({
              type: 'gap',
              row: vs.row,
              colStart: vs.colStart,
              colEnd: vs.colEnd,
              showLabel: showL,
              color: abutColors.feat[f.id] || f.color || ensureReadableColor('#60A5FA'),
            });
          }
        }
      }
      for (const vs of sp(ds.start, ds.end)) {
        const showLabel = !seenRows.has(vs.row);
        seenRows.add(vs.row);
        const segColor =
          abutColors.seg[f.id]?.[di] ||
          dataSegs[di].color ||
          f.color ||
          ensureReadableColor('#60A5FA');
        visuals.push({
          type: 'solid',
          row: vs.row,
          colStart: vs.colStart,
          colEnd: vs.colEnd,
          showLabel,
          color: segColor,
        });
      }
    }

    visuals.sort((a, b) => rowStarts[a.row] + a.colStart - (rowStarts[b.row] + b.colStart));
    if (!visuals.length) return null;

    return (
      <g key={f.id}>
        {visuals.map((v) => {
          // Insertion slots split the visual range into runs; each run
          // draws its own bars so features stay aligned with the shifted
          // template characters. A faint connector (same style as a
          // segmented feature's gap line) bridges the slot cells between
          // consecutive runs.
          const runs = colRuns(v.colStart, v.colEnd, v.row);
          return runs.map(([visStart, len], ri) => {
            const x = getX(visStart);
            const w = len * cw;
            const sy = getSeqY(v.row);
            const rowTo =
              alignLaneInfo.chromBelow[v.row] +
              (((featureRowTracks[f.id] || {})[v.row] || 0) + alignLaneInfo.counts[v.row]) *
                lp.featTrackHeight +
              (alignLaneInfo.counts[v.row] > 0 ? ALIGN_FEAT_GAP : 0);
            const y = sy + lp.featBaseOffset + rowTo;
            const isGap = v.type === 'gap';
            // Cut gap-connector portions where another feature solidly
            // occupies this row+track, so the faint line doesn't cross it.
            // This run's span in template columns (colVis is strictly
            // increasing, so colFromVis of the run start is exact).
            const runTStart = colFromVis(visStart, v.row);
            let colParts = [[runTStart, runTStart + len - 1]];
            if (isGap) {
              const solids = solidLineCols.get(
                `${v.row}:${(featureRowTracks[f.id] || {})[v.row] || 0}`,
              );
              if (solids) {
                for (const s of solids) {
                  const next = [];
                  for (const [a, b] of colParts) {
                    if (s.colEnd < a || s.colStart > b) {
                      next.push([a, b]);
                      continue;
                    }
                    if (s.colStart > a) next.push([a, s.colStart - 1]);
                    if (s.colEnd < b) next.push([s.colEnd + 1, b]);
                  }
                  colParts = next;
                  if (!colParts.length) break;
                }
              }
            }

            return (
              <g
                key={`${v.type}-${v.row}-${v.colStart}-${visStart}`}
                onMouseEnter={() => {
                  if (isDraggingRef.current) return;
                  clearTimeout(featureLeaveRef.current);
                  setHoveredFeature(f.id);
                }}
                onMouseMove={(e) => {
                  if (isDraggingRef.current || translationDragRef.current) return;
                  const cds = cdsFeatureData[f.id];
                  if (!cds || !svgRef.current) return;
                  const pt = svgRef.current.createSVGPoint();
                  pt.x = e.clientX;
                  pt.y = e.clientY;
                  const ctm = svgRef.current.getScreenCTM();
                  if (!ctm) return;
                  const svgPt = pt.matrixTransform(ctm.inverse());
                  let col = Math.floor((svgPt.x - startX) / cw);
                  col = colFromVis(col, v.row);
                  col = Math.max(v.colStart, Math.min(v.colEnd, col));
                  const idx = rowStarts[v.row] + col;
                  const map = {};
                  for (const [featureId, cds] of Object.entries(cdsFeatureData)) {
                    const codon = cds.codonMap.get(idx);
                    if (codon !== undefined) map[featureId] = codon;
                  }
                  const next = Object.keys(map).length === 0 ? null : { key: `${idx}`, map };
                  setHoveredCodon((prev) => (prev?.key === next?.key ? prev : next));
                }}
                onMouseLeave={() => {
                  setHoveredCodon(null);
                  featureLeaveRef.current = setTimeout(() => setHoveredFeature(null), 250);
                }}
                onContextMenu={(e) => openFeatureMenu(e, f)}
                onMouseDown={(e) => {
                  if (e.button !== 0) return;
                  e.stopPropagation();
                  e.preventDefault();

                  // Translatable features: start codon-unit selection
                  const cds = cdsFeatureData[f.id];
                  if (cds) {
                    // Compute clicked template index from the known visual row/column
                    // instead of re-detecting the row, which can be unreliable over the
                    // feature bar that sits below the sequence text.
                    const pt = svgRef.current.createSVGPoint();
                    pt.x = e.clientX;
                    pt.y = e.clientY;
                    const ctm = svgRef.current.getScreenCTM();
                    if (ctm) {
                      const svgPt = pt.matrixTransform(ctm.inverse());
                      let col = Math.floor((svgPt.x - startX) / cw);
                      col = colFromVis(col, v.row);
                      col = Math.max(v.colStart, Math.min(v.colEnd, col));
                      const idx = rowStarts[v.row] + col;
                      const codon = cds.codonMap.get(idx);
                      if (codon !== undefined && codon !== null) {
                        startTranslationSelection(f.id, codon);
                        return;
                      }
                    }
                  }

                  const [fStart, fEnd] = featureSelRange(f);
                  setSelStart(fStart);
                  setSelEnd(fEnd);
                  setCursorIndex(fEnd + 1);
                  featureSelRef.current = f.orf
                    ? null
                    : { id: f.id, selStart: fStart, selEnd: fEnd };
                  clearCursorTimer();
                  // Clear primer / translation selection
                  setSelectionMode('text');
                  setSelectedPrimerIds([]);
                  setTranslationSel(null);
                  translationDragRef.current = null;
                  setIsTranslationDragging(false);
                  isPrimerDraggingRef.current = false;
                  primerDragRef.current = null;
                  if (primerDimTimerRef.current) {
                    clearTimeout(primerDimTimerRef.current);
                    primerDimTimerRef.current = null;
                  }
                  setPrimerDimActive(false);
                }}
                onDoubleClick={(e) => {
                  e.stopPropagation();
                  if (f.orf) return;
                  setCreateFeatureLoc(null);
                  setFeatureInfoFeature(f);
                }}
                className="cursor-pointer"
              >
                <rect
                  x={x}
                  y={isExpanded && !isGap && !f.orf ? sy - 18 : y}
                  width={w}
                  height={isExpanded && !isGap && !f.orf ? y - (sy - 18) : 0}
                  fill={v.color}
                  fillOpacity={isExpanded && !f.orf ? (isGap ? 0 : 0.15) : 0}
                  style={{ transition: springAnim, pointerEvents: 'none' }}
                />
                {alwaysExpandFeatures && isHovered && !isGap && !f.orf && (
                  <rect
                    x={x + 0.75}
                    y={sy - 18 + 0.75}
                    width={w - 1.5}
                    height={y - (sy - 18) - 1.5}
                    fill="none"
                    stroke={v.color}
                    strokeWidth={1.5}
                    style={{ pointerEvents: 'none' }}
                  />
                )}
                {/* Slot bridge: faint continuation across the insertion cells
                  that split this visual, styled like a segment gap line. */}
                {ri > 0 &&
                  (f.orf ? (
                    <line
                      x1={getX(runs[ri - 1][0] + runs[ri - 1][1])}
                      x2={x}
                      y1={y}
                      y2={y}
                      stroke={v.color}
                      strokeWidth="13"
                      opacity={0.25}
                    />
                  ) : (
                    <>
                      <line
                        x1={getX(runs[ri - 1][0] + runs[ri - 1][1])}
                        x2={x}
                        y1={y}
                        y2={y}
                        stroke={isExpanded ? 'transparent' : bgColor}
                        strokeWidth="7"
                      />
                      <line
                        x1={getX(runs[ri - 1][0] + runs[ri - 1][1])}
                        x2={x}
                        y1={y}
                        y2={y}
                        stroke={v.color}
                        strokeWidth="5"
                        opacity={0.25}
                      />
                    </>
                  ))}
                {f.orf ? (
                  <>
                    {colParts.map(([a, b]) => (
                      <line
                        key={`gl-${a}`}
                        x1={getX(colVis(a, v.row))}
                        x2={getX(colVis(b, v.row)) + cw}
                        y1={y}
                        y2={y}
                        stroke={v.color}
                        strokeWidth="13"
                        opacity={isGap ? 0.25 : 1}
                      />
                    ))}
                    <line x1={x} x2={x + w} y1={y} y2={y} stroke="transparent" strokeWidth="13" />
                  </>
                ) : (
                  <>
                    {colParts.map(([a, b]) => (
                      <line
                        key={`bl-${a}`}
                        x1={getX(colVis(a, v.row))}
                        x2={getX(colVis(b, v.row)) + cw}
                        y1={y}
                        y2={y}
                        stroke={isExpanded ? 'transparent' : bgColor}
                        strokeWidth="7"
                      />
                    ))}
                    {colParts.map(([a, b]) => (
                      <line
                        key={`cl-${a}`}
                        x1={getX(colVis(a, v.row))}
                        x2={getX(colVis(b, v.row)) + cw}
                        y1={y}
                        y2={y}
                        stroke={v.color}
                        strokeWidth="5"
                        opacity={isGap ? 0.25 : 1}
                      />
                    ))}
                    <line x1={x} x2={x + w} y1={y} y2={y} stroke="transparent" strokeWidth="10" />
                  </>
                )}
              </g>
            );
          });
        })}

        {/* Feature translation — 1-letter AA centered on middle base of each codon */}
        {isTranslatable(f) &&
          (cdsFeatureData[f.id]?.trans || []).flatMap((t) => {
            const r = rowOf(t.templatePos2);
            const c = t.templatePos2 - rowStarts[r];
            const cov = visuals.find(
              (v) => v.row === r && c >= v.colStart && c <= v.colEnd && v.type !== 'gap',
            );
            if (!cov) return [];
            const sy = getSeqY(r);
            const rowTo =
              alignLaneInfo.chromBelow[r] +
              (((featureRowTracks[f.id] || {})[r] || 0) + alignLaneInfo.counts[r]) *
                lp.featTrackHeight +
              (alignLaneInfo.counts[r] > 0 ? ALIGN_FEAT_GAP : 0);
            const y = sy + lp.featBaseOffset + rowTo;
            const hoveredIdx = hoveredCodon?.map?.[f.id];
            const isCodonHovered = hoveredIdx === t.codonIndex;
            return (
              <text
                key={`tr-${t.templatePos2}`}
                x={getX(colVis(c, r)) + cw / 2}
                y={y}
                fontSize={10}
                fontWeight="900"
                fontFamily={monoFont}
                fill={f.orf ? bgColor : cov.color}
                stroke={f.orf ? 'none' : bgColor}
                strokeWidth={f.orf ? 0 : 3}
                paintOrder="stroke"
                textAnchor="middle"
                dominantBaseline="central"
                style={{ pointerEvents: 'none' }}
              >
                {isCodonHovered ? t.codonIndex + 1 : t.aa}
              </text>
            );
          })}
      </g>
    );
  });
}

export function renderFeatureLabels({
  visibleFeatures,
  hoveredFeature,
  featureLabelsBelow,
  // Continuous mode: SVG-x window of the viewport; long-range labels clamp
  // into it (fwd to the left edge, rev to the right edge) so they stay readable.
  labelViewport,
  sp,
  abutColors,
  getSeqY,
  alignLaneInfo,
  featureRowTracks,
  lp,
  ALIGN_FEAT_GAP,
  rowCounts,
  colVis,
  isDraggingRef,
  featureLeaveRef,
  setHoveredFeature,
  translationDragRef,
  openFeatureMenu,
  featureSelRef,
  setSelStart,
  setSelEnd,
  setCursorIndex,
  clearCursorTimer,
  setSelectionMode,
  setSelectedPrimerIds,
  setTranslationSel,
  setIsTranslationDragging,
  isPrimerDraggingRef,
  primerDragRef,
  primerDimTimerRef,
  setPrimerDimActive,
  setCreateFeatureLoc,
  setFeatureInfoFeature,
}) {
  if (!visibleFeatures.length) return null;
  const seen = new Set();
  const labelsFor = (f) => {
    const isRev = f.strand === '-';
    const isFwd = f.strand === '+';
    const isHovered = hoveredFeature === f.id;
    const { full: fullText, short: shortText } = truncatedLabel(
      f.name,
      isRev,
      isFwd,
      featureLabelsBelow ? Infinity : 12,
    );
    const labelText = isHovered ? fullText : shortText;

    const rowLabels = {};
    const seenRows = new Set();
    const rowExtent = {};
    const trackExtent = (vs) => {
      const e = rowExtent[vs.row] || (rowExtent[vs.row] = { min: Infinity, max: -Infinity });
      if (vs.colStart < e.min) e.min = vs.colStart;
      if (vs.colEnd > e.max) e.max = vs.colEnd;
    };

    for (let di = 0; di < f.segments.length; di++) {
      const ds = f.segments[di];
      if (di > 0) {
        const prevEnd = f.segments[di - 1].end;
        if (ds.start > prevEnd + 1) {
          for (const vs of sp(prevEnd + 1, ds.start - 1)) {
            trackExtent(vs);
            if (!seenRows.has(vs.row) || isRev) {
              seenRows.add(vs.row);
              rowLabels[vs.row] = {
                ...vs,
                color: abutColors.feat[f.id] || f.color || ensureReadableColor('#60A5FA'),
              };
            }
          }
        }
      }
      for (const vs of sp(ds.start, ds.end)) {
        trackExtent(vs);
        if (!seenRows.has(vs.row) || isRev) {
          seenRows.add(vs.row);
          // label takes the color of the bar segment nearest to it
          rowLabels[vs.row] = {
            ...vs,
            color:
              abutColors.seg[f.id]?.[di] || ds.color || f.color || ensureReadableColor('#60A5FA'),
          };
        }
      }
    }

    return Object.values(rowLabels).map((vs) => {
      const key = `${f.id}-${vs.row}`;
      if (seen.has(key)) return null;
      seen.add(key);
      const labelColor = vs.color;
      const sy = getSeqY(vs.row);
      const rowTo =
        alignLaneInfo.chromBelow[vs.row] +
        (((featureRowTracks[f.id] || {})[vs.row] || 0) + alignLaneInfo.counts[vs.row]) *
          lp.featTrackHeight +
        (alignLaneInfo.counts[vs.row] > 0 ? ALIGN_FEAT_GAP : 0);
      const y = sy + lp.featBaseOffset + rowTo;
      // Continuous mode: anchor the label to the feature's whole extent (all
      // segments + connecting gaps), not just the piece that won rowLabels.
      const ext = labelViewport ? rowExtent[vs.row] : null;
      const effColStart = ext ? ext.min : vs.colStart;
      const effColEnd = ext ? ext.max : vs.colEnd;
      if (labelViewport) {
        // Drop the label once the feature's bar has scrolled fully out of the
        // viewport instead of leaving it pinned to an edge.
        const barX1 = getX(colVis(effColStart, vs.row));
        const barX2 = getX(colVis(effColEnd, vs.row)) + cw;
        if (barX2 < labelViewport.left || barX1 > labelViewport.right) return null;
      }
      const textProps = {
        y: featureLabelsBelow ? y + 19 : y + 4,
        fontSize: '12px',
        fontFamily: 'TeX Gyre Heros',
        fontWeight: '600',
      };
      if (isRev) {
        // colEnd + 1 can fall on the next row's first column, whose drift
        // counts slots that render in that row — clamp to this row's edge.
        const cols = rowCounts[vs.row];
        if (cols === 0) return null;
        const xr =
          effColEnd + 1 < cols
            ? getX(colVis(effColEnd + 1, vs.row))
            : getX(colVis(cols - 1, vs.row)) + cw;
        const lx = featureLabelsBelow
          ? labelViewport
            ? Math.min(xr, labelViewport.right)
            : xr
          : xr + 8;
        const lAnchor = featureLabelsBelow ? 'end' : 'start';
        const el = (
          <g
            key={key}
            onMouseEnter={() => {
              if (isDraggingRef.current) return;
              clearTimeout(featureLeaveRef.current);
              setHoveredFeature(f.id);
            }}
            onMouseLeave={() => {
              featureLeaveRef.current = setTimeout(() => setHoveredFeature(null), 250);
            }}
            onContextMenu={(e) => openFeatureMenu(e, f)}
            onMouseDown={(e) => {
              if (e.button !== 0) return;
              e.stopPropagation();
              e.preventDefault();

              const [fStart, fEnd] = featureSelRange(f);
              setSelStart(fStart);
              setSelEnd(fEnd);
              setCursorIndex(fEnd + 1);
              featureSelRef.current = { id: f.id, selStart: fStart, selEnd: fEnd };
              clearCursorTimer();
              setSelectionMode('text');
              setSelectedPrimerIds([]);
              setTranslationSel(null);
              translationDragRef.current = null;
              setIsTranslationDragging(false);
              isPrimerDraggingRef.current = false;
              primerDragRef.current = null;
              if (primerDimTimerRef.current) {
                clearTimeout(primerDimTimerRef.current);
                primerDimTimerRef.current = null;
              }
              setPrimerDimActive(false);
            }}
            onDoubleClick={(e) => {
              e.stopPropagation();
              setCreateFeatureLoc(null);
              setFeatureInfoFeature(f);
            }}
            className="cursor-pointer"
          >
            <text
              x={lx}
              {...textProps}
              textAnchor={lAnchor}
              fill="none"
              stroke={bgColor}
              strokeWidth="5"
              strokeLinejoin="round"
              strokeLinecap="round"
            >
              {labelText}
            </text>
            <text x={lx} {...textProps} textAnchor={lAnchor} fill={labelColor} stroke="none">
              {labelText}
            </text>
          </g>
        );
        return {
          el,
          clamp: labelViewport && featureLabelsBelow && lx !== xr ? 'right' : null,
          start: f.start,
        };
      }
      const x = getX(colVis(effColStart, vs.row));
      const lx = featureLabelsBelow ? (labelViewport ? Math.max(x, labelViewport.left) : x) : x - 8;
      const lAnchor = featureLabelsBelow ? 'start' : 'end';
      const el = (
        <g
          key={key}
          onMouseEnter={() => setHoveredFeature(f.id)}
          onMouseLeave={() => setHoveredFeature(null)}
          onContextMenu={(e) => openFeatureMenu(e, f)}
          onMouseDown={(e) => {
            if (e.button !== 0) return;
            e.stopPropagation();
            e.preventDefault();

            const [fStart, fEnd] = featureSelRange(f);
            setSelStart(fStart);
            setSelEnd(fEnd);
            setCursorIndex(fEnd + 1);
            featureSelRef.current = { id: f.id, selStart: fStart, selEnd: fEnd };
            clearCursorTimer();
            setSelectionMode('text');
            setSelectedPrimerIds([]);
            setTranslationSel(null);
            translationDragRef.current = null;
            setIsTranslationDragging(false);
            isPrimerDraggingRef.current = false;
            primerDragRef.current = null;
            if (primerDimTimerRef.current) {
              clearTimeout(primerDimTimerRef.current);
              primerDimTimerRef.current = null;
            }
            setPrimerDimActive(false);
          }}
          onDoubleClick={(e) => {
            e.stopPropagation();
            setCreateFeatureLoc(null);
            setFeatureInfoFeature(f);
          }}
          className="cursor-pointer"
        >
          <text
            x={lx}
            {...textProps}
            textAnchor={lAnchor}
            fill="none"
            stroke={bgColor}
            strokeWidth="5"
            strokeLinejoin="round"
            strokeLinecap="round"
          >
            {labelText}
          </text>
          <text x={lx} {...textProps} textAnchor={lAnchor} fill={labelColor} stroke="none">
            {labelText}
          </text>
        </g>
      );
      return {
        el,
        clamp: labelViewport && featureLabelsBelow && lx !== x ? 'left' : null,
        start: f.start,
      };
    });
  };
  // Paint order = z-order: labels of the hovered feature render last so
  // they are never occluded by other features' labels.
  const rest = [];
  const hovered = [];
  for (const f of visibleFeatures) {
    (hoveredFeature === f.id ? hovered : rest).push(...labelsFor(f));
  }
  if (labelViewport) {
    // Continuous mode: edge-clamped labels stack deterministically. At the
    // left edge the label whose feature starts further left paints on top;
    // at the right edge the one starting further right paints on top.
    const normal = [];
    const clampLeft = [];
    const clampRight = [];
    for (const entry of rest) {
      if (!entry) continue;
      if (entry.clamp === 'left') clampLeft.push(entry);
      else if (entry.clamp === 'right') clampRight.push(entry);
      else normal.push(entry);
    }
    clampLeft.sort((a, b) => a.start - b.start);
    clampRight.sort((a, b) => b.start - a.start);
    rest.length = 0;
    rest.push(...normal, ...clampLeft, ...clampRight);
  }
  return rest.concat(hovered).map((entry) => (entry ? entry.el : null));
}
