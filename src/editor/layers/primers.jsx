import { cw, bgColor, monoFont, springAnim, getX } from '../../editorConstants';
import { safePrimerColor } from '../seqUtils';

export function renderPrimers({
  visiblePrimers,
  hoveredPrimer,
  sp,
  rowStarts,
  rowCounts,
  openPrimerMenu,
  getSeqY,
  revPrimerFeatOffsets,
  alignLaneInfo,
  ALIGN_FEAT_GAP,
  lp,
  primerTracks,
  pp,
  colVis,
  selectedPrimerIds,
  selectionMode,
  isPrimerDragging,
  primerDimActive,
  primerDragRef,
  setSelStart,
  setSelEnd,
  setCursorIndex,
  setIsEnzymeSelection,
  setSelectedEnzymeIds,
  lastEnzymeSelRef,
  setTranslationSel,
  translationDragRef,
  setIsTranslationDragging,
  setSelectionMode,
  setSelectedPrimerIds,
  setIsPrimerDragging,
  isPrimerDraggingRef,
  setHoveredPrimer,
  primerDimTimerRef,
  clearCursorTimer,
  setPrimerDimActive,
  setCreatePrimerSeq,
  setPrimerAlignmentPrimer,
  enrichedPrimers,
  isDraggingRef,
}) {
  if (!visiblePrimers.length) return null;
  return visiblePrimers.map((p) => {
    const isFwd = p.isFwd;
    const isHovered = hoveredPrimer === p.id;
    const misLen = p.mismatchStr?.length || 0;
    const hasMis = misLen > 0;
    const pColor = safePrimerColor(p.color);
    const segs = (p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }]).flatMap((m) =>
      sp(m.start, m.end),
    );
    if (p.renderCols) {
      for (const seg of segs) {
        const lo = rowStarts[seg.row] + seg.colStart;
        const hi = rowStarts[seg.row] + seg.colEnd;
        seg.renderCols = p.renderCols.filter((rc) => rc.templateCol >= lo && rc.templateCol <= hi);
      }
    }
    const tailSeg = isFwd ? segs[0] : segs[segs.length - 1];
    const arrowSeg = isFwd ? segs[segs.length - 1] : segs[0];
    // Same defence as the alignment lane: overlapping match segments in a
    // stored model must not repeat a React key.
    let segIdx = -1;

    let drawMisLen = misLen,
      showMisDots = false;
    if (hasMis) {
      if (isFwd) {
        const max = tailSeg.colStart + 5;
        if (misLen > max) {
          drawMisLen = max;
          showMisDots = true;
        }
      } else {
        const max = rowCounts[tailSeg.row] - 1 - tailSeg.colEnd + 5;
        if (misLen > max) {
          drawMisLen = max;
          showMisDots = true;
        }
      }
    }

    return (
      <g key={p.id} onContextMenu={(e) => openPrimerMenu(e, p)}>
        {segs.map((seg) => {
          const isTail = seg === tailSeg,
            isArrow = seg === arrowSeg;
          const sy = getSeqY(seg.row);
          const featOff =
            (isFwd ? 0 : (revPrimerFeatOffsets[p.id] || {})[seg.row] || 0) +
            (isFwd
              ? 0
              : alignLaneInfo.chromBelow[seg.row] +
                (alignLaneInfo.counts[seg.row] > 0
                  ? alignLaneInfo.counts[seg.row] * lp.featTrackHeight + ALIGN_FEAT_GAP
                  : 0));
          const trackOff = ((primerTracks[p.id] || {})[seg.row] || 0) * pp.trackGap + featOff;
          const matchY =
            (isFwd ? sy - pp.fwdMatchY : sy + pp.revMatchY) + (isFwd ? -trackOff : trackOff);
          const misY = matchY + (isFwd ? -pp.misYDelta : pp.misYDelta);
          const x1 = getX(colVis(seg.colStart, seg.row)),
            x2 = getX(colVis(seg.colEnd, seg.row));

          // Build per-column path points from renderCols
          let pts = [];
          let edge3x, edge5x;
          const hasRenderCols = seg.renderCols && seg.renderCols.length > 0;
          if (hasRenderCols) {
            // Per-column zigzag path
            const cols = isFwd ? seg.renderCols : [...seg.renderCols].reverse();
            const firstCol = cols[0],
              lastCol = cols[cols.length - 1];
            edge5x = isFwd
              ? getX(colVis(firstCol.templateCol - rowStarts[seg.row], seg.row)) // fwd: left edge of leftmost
              : getX(colVis(firstCol.templateCol - rowStarts[seg.row], seg.row)) + cw; // rev: right edge
            edge3x = isFwd
              ? getX(colVis(lastCol.templateCol - rowStarts[seg.row], seg.row)) + cw // fwd: right edge
              : getX(colVis(lastCol.templateCol - rowStarts[seg.row], seg.row)); // rev: left edge
            // 5' tail
            if (isTail && hasMis && drawMisLen > 0) {
              if (isFwd) pts.push([x1 - drawMisLen * cw, misY], [x1 - cw / 2, misY]);
              else pts.push([x2 + (drawMisLen + 1) * cw, misY], [x2 + cw * 1.5, misY]);
            }
            pts.push([edge5x, cols[0].kind === 'match' ? matchY : misY]);
            for (const rc of cols) {
              const cx = getX(colVis(rc.templateCol - rowStarts[seg.row], seg.row)) + cw / 2;
              const cy = rc.kind === 'match' ? matchY : misY;
              pts.push([cx, cy]);
            }
            pts.push([edge3x, cols[cols.length - 1].kind === 'match' ? matchY : misY]);
          } else {
            // Fallback: straight line
            if (isTail && hasMis && drawMisLen > 0) {
              if (isFwd) pts.push([x1 - drawMisLen * cw, misY], [x1 - cw / 2, misY]);
              else pts.push([x2 + (drawMisLen + 1) * cw, misY], [x2 + cw * 1.5, misY]);
            }
            if (isFwd) pts.push([x1 + cw / 2, matchY], [x2 + cw, matchY]);
            else pts.push([x2 + cw, matchY], [x1, matchY]);
          }
          if (pts.length < 2) return null;

          const isSelectedPrimer =
            selectedPrimerIds.includes(p.id) &&
            (selectionMode === 'primer' || selectionMode === 'amplimer');
          const isDimDuringDrag =
            isPrimerDragging &&
            primerDimActive &&
            !isSelectedPrimer &&
            p.isFwd === primerDragRef.current?.startFwd;

          const pathStr = `M ${pts.map((p) => `${p[0]} ${p[1]}`).join(' L ')}`;
          const expD = isFwd ? -1 : 1;
          const curExp = isHovered || isSelectedPrimer ? pp.hoverExpand : 0;
          const last = pts[pts.length - 1];
          const hoverPath =
            pathStr +
            ` L ${last[0]} ${last[1] + expD * curExp} ` +
            [...pts]
              .reverse()
              .map((p) => `L ${p[0]} ${p[1] + expD * curExp}`)
              .join(' ') +
            ' Z';

          const arrowTipY =
            hasRenderCols && seg.renderCols.length > 0
              ? seg.renderCols[seg.renderCols.length - 1].kind === 'match'
                ? matchY
                : misY
              : matchY;
          const arrowBaseX = hasRenderCols ? edge3x : isFwd ? x2 + cw : x1;
          const arrowPath = isArrow
            ? `M ${arrowBaseX} ${arrowTipY} L ${isFwd ? arrowBaseX - pp.arrowHeadLen : arrowBaseX + pp.arrowHeadLen} ${arrowTipY + expD * pp.arrowHeadHeight}`
            : '';

          const visMis = hasMis && drawMisLen > 0 ? p.mismatchStr.slice(misLen - drawMisLen) : '';
          const labelOff = isSelectedPrimer ? (isFwd ? -16 : 13) : 0;

          return (
            <g
              key={`${seg.row}-${seg.colStart}-${++segIdx}`}
              opacity={isDimDuringDrag ? 0.2 : undefined}
              style={isDimDuringDrag ? { pointerEvents: 'none' } : undefined}
            >
              <path
                d={hoverPath}
                fill={isSelectedPrimer ? pColor : bgColor}
                style={{ transition: springAnim }}
              />
              {!isSelectedPrimer && (
                <path
                  d={hoverPath}
                  fill={pColor}
                  fillOpacity={0.1}
                  style={{ transition: springAnim }}
                />
              )}

              <text
                fill={isSelectedPrimer ? bgColor : pColor}
                fontSize="14px"
                fontFamily={monoFont}
                fontWeight="bold"
                style={{
                  opacity: isHovered || isSelectedPrimer ? 1 : 0,
                  transition: 'opacity 0.2s ease-in-out',
                  pointerEvents: 'none',
                }}
              >
                {/* 5' tail */}
                {isTail && hasMis && drawMisLen > 0 && (
                  <>
                    {showMisDots && (
                      <tspan
                        x={isFwd ? x1 - (drawMisLen + 1.5) * cw : x2 + (drawMisLen + 2.5) * cw}
                        y={misY + (isFwd ? -pp.fwdBaseTextY : pp.revBaseTextY)}
                        textAnchor="middle"
                      >
                        ···
                      </tspan>
                    )}
                    {visMis.split('').map((c, k) => (
                      <tspan
                        key={`mis-${k}`}
                        x={
                          (isFwd ? x1 - (drawMisLen - k) * cw : x2 + (drawMisLen - k) * cw) + cw / 2
                        }
                        y={misY + (isFwd ? -pp.fwdBaseTextY : pp.revBaseTextY)}
                        textAnchor="middle"
                      >
                        {c}
                      </tspan>
                    ))}
                  </>
                )}
                {/* Per-column alignment rendering */}
                {seg.renderCols &&
                  seg.renderCols.map((rc) => {
                    const isOffset =
                      rc.kind === 'mismatch' || rc.kind === 'gap' || rc.kind === 'insertion';
                    const y =
                      (isOffset ? misY : matchY) + (isFwd ? -pp.fwdBaseTextY : pp.revBaseTextY);
                    const x = getX(colVis(rc.templateCol - rowStarts[seg.row], seg.row)) + cw / 2;
                    const isGap = rc.kind === 'gap';
                    const isIns = rc.kind === 'insertion';
                    return (
                      <tspan
                        key={`aln-${rc.templateCol}`}
                        x={x}
                        y={y}
                        textAnchor="middle"
                        fill={isGap ? '#9ca3af' : undefined}
                        fontWeight={isGap ? '200' : undefined}
                        fontSize={isIns ? '10px' : undefined}
                      >
                        {isIns ? rc.insDetail?.insertedBases || rc.primerBase : rc.primerBase}
                      </tspan>
                    );
                  })}
                {/* 3' tail */}
                {isArrow &&
                  p.threePrimeTail &&
                  (() => {
                    const tail3 = p.threePrimeTail;
                    const tailLen = tail3.length;
                    return tail3.split('').map((c, k) => (
                      <tspan
                        key={`3t-${k}`}
                        x={(isFwd ? x2 + (k + 1) * cw : x1 - (tailLen - k) * cw) + cw / 2}
                        y={misY + (isFwd ? -pp.fwdBaseTextY : pp.revBaseTextY)}
                        textAnchor="middle"
                      >
                        {c}
                      </tspan>
                    ));
                  })()}
              </text>

              <path
                d={pathStr}
                fill="none"
                stroke={bgColor}
                strokeWidth="6"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
              {isArrow && (
                <path
                  d={arrowPath}
                  fill="none"
                  stroke={bgColor}
                  strokeWidth="6"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              )}
              <path d={pathStr} fill="none" stroke={pColor} strokeWidth="2.5" />
              {isArrow && (
                <path
                  d={arrowPath}
                  fill="none"
                  stroke={pColor}
                  strokeWidth="2.5"
                  strokeLinecap="round"
                />
              )}

              <text
                x={pts[0][0]}
                y={pts[0][1] + (isFwd ? -pp.fwdLabelY : pp.revLabelY) + labelOff}
                fontSize="12px"
                fontFamily="TeX Gyre Heros"
                fontWeight="600"
                fontStyle="italic"
                textAnchor={isFwd ? 'start' : 'end'}
                fill="none"
                stroke={bgColor}
                strokeWidth="5"
                strokeLinejoin="round"
                strokeLinecap="round"
                style={{
                  opacity: isHovered && !isSelectedPrimer ? 0 : 1,
                  transition: springAnim,
                  pointerEvents: 'none',
                }}
              >
                {p.name}
              </text>
              <text
                x={pts[0][0]}
                y={pts[0][1] + (isFwd ? -pp.fwdLabelY : pp.revLabelY) + labelOff}
                fontSize="12px"
                fontFamily="TeX Gyre Heros"
                fontWeight="600"
                fontStyle="italic"
                textAnchor={isFwd ? 'start' : 'end'}
                fill={pColor}
                stroke="none"
                style={{
                  opacity: isHovered && !isSelectedPrimer ? 0 : 1,
                  transition: springAnim,
                  pointerEvents: 'none',
                }}
              >
                {p.name}
              </text>

              <path d={pathStr} fill="none" stroke="transparent" strokeWidth="20" />
              <rect
                x={Math.min(...pts.map((p) => p[0])) - 4}
                y={Math.min(...pts.map((p) => p[1])) - 20}
                width={Math.max(...pts.map((p) => p[0])) - Math.min(...pts.map((p) => p[0])) + 8}
                height={Math.max(...pts.map((p) => p[1])) - Math.min(...pts.map((p) => p[1])) + 40}
                fill="transparent"
                onMouseDown={(e) => {
                  if (e.button !== 0) return;
                  e.stopPropagation();
                  e.preventDefault();
                  // Clear all other selections
                  setSelStart(null);
                  setSelEnd(null);
                  setCursorIndex(null);
                  setIsEnzymeSelection(false);
                  setSelectedEnzymeIds([]);
                  lastEnzymeSelRef.current = null;
                  setTranslationSel(null);
                  translationDragRef.current = null;
                  setIsTranslationDragging(false);
                  // Start primer drag
                  setSelectionMode('primer');
                  setSelectedPrimerIds([p.id]);
                  setIsPrimerDragging(true);
                  isPrimerDraggingRef.current = true;
                  setHoveredPrimer(p.id);
                  primerDragRef.current = {
                    startPrimerId: p.id,
                    startFwd: p.isFwd,
                    didDrag: false,
                    hoveredPrimerId: p.id,
                  };
                  // Delay dimming other primers by 500ms to prevent flash on click
                  if (primerDimTimerRef.current) clearTimeout(primerDimTimerRef.current);
                  primerDimTimerRef.current = setTimeout(() => setPrimerDimActive(true), 500);
                  clearCursorTimer();
                }}
                onMouseEnter={() => {
                  if (isPrimerDraggingRef.current) {
                    if (primerDragRef.current) {
                      primerDragRef.current.hoveredPrimerId = p.id;
                    }
                    setHoveredPrimer(p.id);
                    return;
                  }
                  if (isDraggingRef.current) return;
                  setHoveredPrimer(p.id);
                }}
                onMouseLeave={() => setHoveredPrimer(null)}
                onDoubleClick={(e) => {
                  e.stopPropagation();
                  // Prevent drag activation on double-click
                  setPrimerDimActive(false);
                  if (primerDimTimerRef.current) clearTimeout(primerDimTimerRef.current);
                  setCreatePrimerSeq(null); // clear create mode
                  setPrimerAlignmentPrimer(enrichedPrimers.find((ep) => ep.id === p.id) || null);
                }}
                className="cursor-pointer"
              />
            </g>
          );
        })}
      </g>
    );
  });
}
