import React from 'react';
import {
  cw,
  bgColor,
  monoFont,
  complement,
  enzymeActiveBlue,
} from '../../editorConstants';
import { isIISEnzyme, splitEnzName } from '../seqUtils';

export function renderEnzymeLines({
  enzymeLayout,
  enzymeLinesPath,
  lp,
}) {
  if (!enzymeLayout.length) return null;
  return (
    <g>
      <path
        d={enzymeLinesPath}
        fill="none"
        stroke="#333"
        strokeWidth="0.8"
        style={{ pointerEvents: 'none' }}
      />
      {enzymeLayout
        .filter((l) => l.isUnique)
        .map((l) => (
          <line
            key={`u-${l.id}`}
            x1={l.cutX}
            x2={l.cutX}
            y1={l.yTop}
            y2={l.sy - lp.enzLineGap}
            stroke="#333"
            strokeWidth="1"
            style={{ pointerEvents: 'none' }}
          />
        ))}
    </g>
  );
}

export function renderEnzymeLabels({
  hoveredEnzyme,
  enzymeLayout,
  enzymes,
  selectedEnzymeIds,
  totalNameCounts,
  openEnzymeMenu,
  openEnzymeDetail,
  enzymeDragRef,
  isDraggingRef,
  setHoveredEnzyme,
  setSelStart,
  setSelEnd,
  setCursorIndex,
  setSelectedEnzymeIds,
  isPrimerDraggingRef,
  primerDragRef,
  primerDimTimerRef,
  setPrimerDimActive,
  setTranslationSel,
  translationDragRef,
  setIsTranslationDragging,
  lastEnzymeSelRef,
  clearCursorTimer,
  setIsEnzymeSelection,
  setSelectedPrimerIds,
  cleanSeq,
  topology,
  setIsDragging,
  setIsEnzymeDragging,
}) {
  const hoveredName = hoveredEnzyme
    ? enzymeLayout.find((l) => l.id === hoveredEnzyme)?.name
    : null;
  return enzymeLayout.map((l) => {
    const e = enzymes.find((x) => x.id === l.groupId);
    const isGray =
      e && (e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length));
    const isHoveredGroup = hoveredName != null && l.name === hoveredName;
    const isSelected = selectedEnzymeIds.includes(l.id);
    const isBlunt = e && e.cutType === 'blunt';
    const isIIS = isIISEnzyme(e);
    const showTwo = totalNameCounts.get(l.name) === 2;

    // Color: selected (dark blue) > hovered (blue) > type-specific > default
    let labelColor = '#333';
    if (isGray) labelColor = '#9CA3AF';
    else if (isSelected) labelColor = enzymeActiveBlue;
    else if (isHoveredGroup) labelColor = '#2563EB';
    else if (isBlunt) labelColor = '#6B3A2A';
    else if (isIIS) labelColor = '#0D6B6B';

    return (
      <g
        key={l.id}
        onContextMenu={(e) => openEnzymeMenu(e, l)}
        onDoubleClick={(e) => openEnzymeDetail(e, l)}
        onMouseEnter={() => {
          if (enzymeDragRef.current?.active) {
            // Regular enzyme can't drag to cut-twice enzyme
            const srcEnz = enzymes.find((x) => x.id === enzymeDragRef.current.startEnzymeId);
            if (srcEnz && !(srcEnz.cutPairs?.length > 1) && e?.cutPairs?.length > 1) return;
            // During drag: update selection between start and target
            enzymeDragRef.current.hoveredId = l.id;
            enzymeDragRef.current.didDrag = true;
            setHoveredEnzyme(l.id);
            const startIdx = enzymeDragRef.current.startCutIdx;
            const targetIdx = l.topCutIndex;
            if (targetIdx !== startIdx) {
              const s = Math.min(startIdx, targetIdx);
              const e = Math.max(startIdx, targetIdx) - 1;
              if (s <= e) {
                setSelStart(s);
                setSelEnd(e);
                setCursorIndex(null);
              }
              setSelectedEnzymeIds([enzymeDragRef.current.entryId, l.id]);
              enzymeDragRef.current.backToStart = false;
            } else {
              const rs = enzymeDragRef.current.recStart;
              const re = enzymeDragRef.current.recEnd;
              if (rs != null && re != null) {
                setSelStart(rs);
                setSelEnd(re);
              }
              setSelectedEnzymeIds([l.id]);
              enzymeDragRef.current.backToStart = true;
            }
          } else if (!isDraggingRef.current) {
            setHoveredEnzyme(l.id);
          }
        }}
        onMouseLeave={() => {
          if (enzymeDragRef.current?.active && enzymeDragRef.current.hoveredId === l.id) {
            enzymeDragRef.current.hoveredId = null;
          }
          setHoveredEnzyme(null);
        }}
        onMouseDown={(e) => {
          if (e.button !== 0) return;
          e.stopPropagation();
          e.preventDefault();
          // Clear primer selection
          setSelectedPrimerIds([]);
          isPrimerDraggingRef.current = false;
          primerDragRef.current = null;
          if (primerDimTimerRef.current) {
            clearTimeout(primerDimTimerRef.current);
            primerDimTimerRef.current = null;
          }
          setPrimerDimActive(false);
          // Clear translation selection
          setTranslationSel(null);
          translationDragRef.current = null;
          setIsTranslationDragging(false);
          const enzyme = enzymes.find((x) => x.id === l.groupId);
          if (!enzyme) return;
          const pairs = enzyme.cutPairs || [
            { topCutIndex: enzyme.cutIndex, botCutIndex: enzyme.botCutIndex },
          ];
          const isCutTwice = pairs.length > 1;
          const cutIdx = l.topCutIndex;

          // Shift+click: extend from previous enzyme selection
          if (
            e.shiftKey &&
            lastEnzymeSelRef.current &&
            lastEnzymeSelRef.current.cutIdx !== cutIdx
          ) {
            const prevCutIdx = lastEnzymeSelRef.current.cutIdx;
            const prevEntryId = lastEnzymeSelRef.current.entryId;
            const s = Math.min(prevCutIdx, cutIdx);
            const ed = Math.max(prevCutIdx, cutIdx) - 1;
            if (s <= ed) {
              setSelStart(s);
              setSelEnd(ed);
              setCursorIndex(null);
              setIsEnzymeSelection(true);
              setSelectedEnzymeIds(prevEntryId ? [prevEntryId, l.id] : [l.id]);
              setHoveredEnzyme(null);
              clearCursorTimer();
              lastEnzymeSelRef.current = {
                enzymeId: l.groupId,
                cutIdx,
                name: l.name,
                entryId: l.id,
              };
            }
            return;
          }

          // Cut-twice enzyme: directly select between two cut positions, close tooltip
          if (isCutTwice) {
            const otherPair = pairs[l.pairIndex === 0 ? 1 : 0];
            const cut1 = cutIdx;
            const cut2 = otherPair.topCutIndex;
            const s = Math.min(cut1, cut2);
            const ed = Math.max(cut1, cut2) - 1;
            const otherEntryId = `${l.groupId}_p${l.pairIndex === 0 ? 1 : 0}`;
            setSelStart(s);
            setSelEnd(ed);
            setCursorIndex(null);
            setIsEnzymeSelection(true);
            setSelectedEnzymeIds([l.id, otherEntryId]);
            setHoveredEnzyme(null);
            clearCursorTimer();
            lastEnzymeSelRef.current = {
              enzymeId: l.groupId,
              cutIdx,
              name: l.name,
              entryId: l.id,
            };
            return;
          }

          // Start enzyme drag — immediately select the recognition site.
          // Sites wrapping the origin of a circular sequence (recEnd beyond
          // the sequence length) fall back to the display window. For
          // circular molecules wrap the coords into [0, tlen) so
          // selStart > selEnd expresses the cross-origin selection
          // (mirrors the tooltip path).
          const tlen = cleanSeq.length;
          const recWraps = enzyme.recStart == null || enzyme.recEnd >= tlen;
          let recSelStart = recWraps ? enzyme.displayStart : enzyme.recStart;
          let recSelEnd = recWraps ? enzyme.displayEnd : enzyme.recEnd;
          if (recWraps && topology === 'circular' && tlen > 0) {
            recSelStart = ((recSelStart % tlen) + tlen) % tlen;
            recSelEnd = ((recSelEnd % tlen) + tlen) % tlen;
          }
          setSelStart(recSelStart);
          setSelEnd(recSelEnd);
          setCursorIndex(null);
          enzymeDragRef.current = {
            active: true,
            startEnzymeId: l.groupId,
            startName: l.name,
            startCutIdx: cutIdx,
            recStart: recSelStart,
            recEnd: recSelEnd,
            didDrag: false,
            backToStart: false,
            hoveredId: l.id,
            entryId: l.id,
          };
          isDraggingRef.current = true;
          setIsDragging(true);
          setIsEnzymeDragging(true);
          setIsEnzymeSelection(true);
          setSelectedEnzymeIds([l.id]);
          setHoveredEnzyme(l.id);
          clearCursorTimer();
        }}
        style={{ cursor: 'pointer' }}
      >
        <rect
          x={l.cutX + 3}
          y={l.yTop - 10}
          width={l.enzW + (showTwo ? 10 : 2)}
          height={18}
          fill="transparent"
        />
        {(() => {
          const enzText = {
            x: l.cutX + 6,
            y: l.yTop + 5,
            fontSize: '14px',
            fontFamily: monoFont,
            fontWeight: l.isUnique ? '700' : '350',
            style: { pointerEvents: 'none' },
          };
          const nameContent = (() => {
            const s = splitEnzName(l.name);
            return s.normal
              ? [
                  <tspan key="i" fontStyle="italic">
                    {s.italic}
                  </tspan>,
                  <tspan key="n">{s.normal}</tspan>,
                ]
              : l.name;
          })();
          const content = showTwo
            ? [
                ...(Array.isArray(nameContent) ? nameContent : [nameContent]),
                <tspan key="two" fontSize="12" dy="-2">
                  ²
                </tspan>,
              ]
            : nameContent;
          return (
            <>
              <text
                {...enzText}
                fill="none"
                stroke={bgColor}
                strokeWidth="5"
                strokeLinejoin="round"
                strokeLinecap="round"
              >
                {content}
              </text>
              <text {...enzText} fill={labelColor} stroke="none">
                {content}
              </text>
            </>
          );
        })()}
      </g>
    );
  });
}

export function renderEnzymeOverlay({
  hoveredEnzyme,
  enzymeLayout,
  selectedEnzymeIds,
  enzymes,
  totalNameCounts,
  isEnzymeSelection,
}) {
  // Collect enzyme names to render lines for (from hover or selected ids)
  const namesToRender = new Set();
  if (hoveredEnzyme) {
    const entry = enzymeLayout.find((l) => l.id === hoveredEnzyme);
    if (entry) namesToRender.add(entry.name);
  }
  for (const id of selectedEnzymeIds) {
    const entry = enzymeLayout.find((l) => l.id === id);
    if (entry) namesToRender.add(entry.name);
  }
  if (namesToRender.size === 0) return null;

  // Compute hover text content (only for hovered enzyme)
  let hoverTextContent = null;
  if (hoveredEnzyme) {
    const hoveredEntry = enzymeLayout.find((l) => l.id === hoveredEnzyme);
    if (hoveredEntry) {
      const e = enzymes.find((x) => x.name === hoveredEntry.name);
      if (e) {
        const isGray =
          e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length);
        const isSelOv = selectedEnzymeIds.includes(hoveredEntry.id);
        const ovColor = isGray ? '#9CA3AF' : isSelOv ? enzymeActiveBlue : '#2563EB';
        const showTwoOv = totalNameCounts.get(hoveredEntry.name) === 2;
        const ovNameContent = (() => {
          const s = splitEnzName(e.name);
          const parts = s.normal
            ? [
                <tspan key="i" fontStyle="italic">
                  {s.italic}
                </tspan>,
                <tspan key="n">{s.normal}</tspan>,
              ]
            : [e.name];
          if (showTwoOv)
            parts.push(
              <tspan key="two" fontSize="12" dy="-2">
                ²
              </tspan>,
            );
          return parts;
        })();
        const methParts = [];
        if (e.methylationBlocked && e.methylationSources?.length) {
          methParts.push('[' + e.methylationSources.join('/') + ' Blocked]');
        }
        if (e.methylationRequired && e.methylRequiredSources?.length) {
          methParts.push('[' + e.methylRequiredSources.join('/') + ' Required]');
        }
        const methText = methParts.length ? '  ' + methParts.join(' ') : '';
        hoverTextContent = { hoveredEntry, ovColor, ovNameContent, methText };
      }
    }
  }

  return (
    <g style={{ pointerEvents: 'none' }}>
      {/* Render lines for each enzyme name */}
      {[...namesToRender].map((name) => {
        const e = enzymes.find((x) => x.name === name);
        if (!e) return null;
        const isGray =
          e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length);
        const hoveredName = hoveredEnzyme
          ? enzymeLayout.find((l) => l.id === hoveredEnzyme)?.name
          : null;
        let nameEntries = enzymeLayout.filter((l) => l.name === name);
        // In selection mode, only filter non-hovered entries (selected + hovered lines both show)
        if (isEnzymeSelection && name !== hoveredName) {
          nameEntries = nameEntries.filter((l) => selectedEnzymeIds.includes(l.id));
        }
        return nameEntries.map((l) => {
          const isSel = selectedEnzymeIds.includes(l.id);
          const lineColor = isGray ? '#9CA3AF' : isSel ? enzymeActiveBlue : '#2563EB';
          return (
            <React.Fragment key={`ov-${l.id}`}>
              <line
                x1={l.cutX}
                x2={l.cutX}
                y1={l.yTop}
                y2={l.sy + 5}
                stroke={bgColor}
                strokeWidth="4"
                strokeLinecap="square"
              />
              <line
                x1={l.cutX}
                x2={l.cutX}
                y1={l.yTop}
                y2={l.sy + 5}
                stroke={lineColor}
                strokeWidth={e.isUnique ? '2' : '1'}
              />
            </React.Fragment>
          );
        });
      })}
      {/* Hover text */}
      {hoverTextContent && (
        <React.Fragment>
          <text
            x={hoverTextContent.hoveredEntry.cutX + 6}
            y={hoverTextContent.hoveredEntry.yTop + 5}
            fill="none"
            stroke={bgColor}
            strokeWidth="5"
            strokeLinejoin="round"
            strokeLinecap="round"
            fontSize="14px"
            fontFamily={monoFont}
            fontWeight={hoverTextContent.hoveredEntry.isUnique ? '700' : '350'}
          >
            {hoverTextContent.ovNameContent}
            {hoverTextContent.methText}
          </text>
          <text
            x={hoverTextContent.hoveredEntry.cutX + 6}
            y={hoverTextContent.hoveredEntry.yTop + 5}
            fill={hoverTextContent.ovColor}
            stroke="none"
            fontSize="14px"
            fontFamily={monoFont}
            fontWeight={hoverTextContent.hoveredEntry.isUnique ? '700' : '350'}
          >
            {hoverTextContent.ovNameContent}
            {hoverTextContent.methText}
          </text>
        </React.Fragment>
      )}
    </g>
  );
}

export function renderTooltips({
  hoveredEnzyme,
  isEnzymeDragging,
  enzymeLayout,
  enzymes,
  cleanSeq,
}) {
  if (!hoveredEnzyme || isEnzymeDragging) return null;
  const hoveredEntry = enzymeLayout.find((l) => l.id === hoveredEnzyme);
  if (!hoveredEntry) return null;
  const e = enzymes.find((x) => x.id === hoveredEntry.groupId);
  if (!e || e.displayStart === undefined) return null;
  const isGray =
    e.methylationBlocked || (e.methylationRequired && e.methylRequiredSources?.length);
  const ttColor = isGray ? '#9CA3AF' : isEnzymeDragging ? enzymeActiveBlue : '#2563EB';

  const tlen = cleanSeq.length;
  const dispLen = e.displayEnd - e.displayStart + 1;
  const sw = e.isUnique ? '2' : '1';
  const pad = 6;
  const ttH = 44;
  const cutPairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
  // Circular display window may wrap the origin (displayEnd >= tlen).
  const sub =
    e.displayEnd < tlen
      ? cleanSeq.substring(e.displayStart, e.displayEnd + 1)
      : Array.from({ length: dispLen }, (_, i) => cleanSeq[(e.displayStart + i) % tlen]).join('');
  const comp = sub.split('').map(complement).join('');
  const pattern = e.recSeqPattern || '';
  const recOffset = e.recStart - e.displayStart;
  const recLen = e.recEnd - e.recStart + 1;
  // Position relative to displayStart, in window coordinates (handles wrap).
  const relPos = (idx) => (((idx - e.displayStart) % tlen) + tlen) % tlen;
  const isRecBold = (i) => {
    if (i < recOffset || i >= recOffset + recLen) return false;
    const pi = i - recOffset;
    return pi < pattern.length && pattern[pi] !== 'N' && pattern[pi] !== 'n';
  };

  const groupEntries = enzymeLayout.filter((l) => l.groupId === hoveredEntry.groupId);

  return (
    <g style={{ pointerEvents: 'none' }}>
      {groupEntries.map((entry) => {
        const sy = entry.sy;
        const ttY = sy - 19;
        const hp = cutPairs[entry.pairIndex] || cutPairs[0];
        const charsBeforeCut = relPos(hp.topCutIndex);
        const baseX = entry.cutX - charsBeforeCut * cw;
        const leftX = baseX - pad;
        const ttW = dispLen * cw + pad * 2;

        const polyEntries = cutPairs.map((cp, i) => {
          const tGapX = baseX + relPos(cp.topCutIndex) * cw;
          const bGapX = baseX + relPos(cp.botCutIndex) * cw;
          const isLocal = i === entry.pairIndex;
          return {
            tGapX,
            bGapX,
            isLocal,
            path: [
              `M ${tGapX} ${ttY - 2}`,
              `L ${tGapX} ${sy + 3}`,
              `L ${bGapX} ${sy + 3}`,
              `L ${bGapX} ${sy + 19}`,
            ].join(' '),
          };
        });

        const uniqueGapXs = [...new Set(polyEntries.map((pe) => pe.tGapX))].sort((a, b) => a - b);
        const gapHalfW = 4;
        const r = 8;
        let borderD = `M ${leftX + r} ${ttY}`;
        let curX = leftX + r;
        for (const gx of uniqueGapXs) {
          if (gx - gapHalfW > curX) {
            borderD += ` L ${gx - gapHalfW} ${ttY}`;
          }
          borderD += ` M ${gx + gapHalfW} ${ttY}`;
          curX = gx + gapHalfW;
        }
        if (curX < leftX + ttW - r) {
          borderD += ` L ${leftX + ttW - r} ${ttY}`;
        }
        borderD += ` A ${r} ${r} 0 0 1 ${leftX + ttW} ${ttY + r}`;
        borderD += ` L ${leftX + ttW} ${ttY + ttH - r}`;
        borderD += ` A ${r} ${r} 0 0 1 ${leftX + ttW - r} ${ttY + ttH}`;
        borderD += ` L ${leftX + r} ${ttY + ttH}`;
        borderD += ` A ${r} ${r} 0 0 1 ${leftX} ${ttY + ttH - r}`;
        borderD += ` L ${leftX} ${ttY + r}`;
        borderD += ` A ${r} ${r} 0 0 1 ${leftX + r} ${ttY}`;

        return (
          <g key={`tt-${entry.id}`}>
            <rect
              x={leftX}
              y={ttY}
              width={ttW}
              height={ttH}
              rx={8}
              fill="#FFFFFF"
              stroke="none"
            />
            <path
              d={borderD}
              fill="none"
              stroke={ttColor}
              strokeWidth={sw}
              strokeLinejoin="round"
            />
            {polyEntries.map((pe, i) => (
              <path
                key={`poly-${i}`}
                d={pe.path}
                fill="none"
                stroke={ttColor}
                strokeWidth={sw}
                strokeLinejoin="round"
                strokeLinecap="round"
              />
            ))}
            <text y={sy} fontFamily={monoFont} fontSize="14px">
              {sub.split('').map((c, i) => {
                const bold = isRecBold(i);
                return (
                  <tspan
                    key={i}
                    x={baseX + i * cw + cw / 2}
                    textAnchor="middle"
                    fontWeight={bold ? '700' : '200'}
                    fill={bold ? '#1f2937' : '#BFBFBF'}
                  >
                    {c}
                  </tspan>
                );
              })}
            </text>
            <text y={sy + 16} fontFamily={monoFont} fontSize="14px">
              {comp.split('').map((c, i) => {
                const bold = isRecBold(i);
                return (
                  <tspan
                    key={i}
                    x={baseX + i * cw + cw / 2}
                    textAnchor="middle"
                    fontWeight={bold ? '700' : '200'}
                    fill={bold ? '#1f2937' : '#BFBFBF'}
                  >
                    {c}
                  </tspan>
                );
              })}
            </text>
          </g>
        );
      })}
    </g>
  );
}
