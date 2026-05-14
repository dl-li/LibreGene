import React, { useState, useCallback, useRef } from 'react';
import { getX, cw, bgColor, springAnim, monoFont } from './editorConstants';
import { computePrimerSegments, mismatchOffset, buildSegmentPath, buildSegmentHoverPath } from './primerRenderer';

const STYLES = {
  default: {
    bg: () => ({ fill: 'none' }),
    overlay: () => null,
    text: () => null,
    labelColor: (pColor) => pColor,
    labelStroke: () => bgColor,
    showLabelCollapsed: true, showLabelExpanded: false,
    expandedLabelColor: () => bgColor, expandedLabelStroke: (pColor) => pColor,
  },
  selected: {
    bg: (pColor) => ({ fill: pColor }),
    overlay: () => null,
    text: () => bgColor,
    labelColor: (pColor) => pColor,
    labelStroke: () => bgColor,
    showLabelCollapsed: false, showLabelExpanded: true,
    expandedLabelColor: (pColor) => pColor, expandedLabelStroke: () => bgColor,
  },
  hover: {
    bg: () => ({ fill: bgColor }),
    overlay: (pColor) => ({ fill: pColor, fillOpacity: 0.1 }),
    text: (pColor) => pColor,
    labelColor: () => bgColor,
    labelStroke: (pColor) => pColor,
    showLabelCollapsed: false, showLabelExpanded: true,
    expandedLabelColor: () => bgColor, expandedLabelStroke: (pColor) => pColor,
  },
};
export { STYLES };

export default function PrimerSegmentRenderer({
  primer, mode = 'default', charsPerLine, rowYGetter,
  primerTracks, primerFeatOffsets, primerConfig,
}) {
  const {
    fPrimerBase = 30, rPrimerBase = 30, primerGap = 36,
    primerExpand = 26, tailYOffset = 4,
    arrowWidth = 7, arrowHeight = 5,
    matchTextYFwd = -8, matchTextYRev = 18,
    tailTextYFwd = -8, tailTextYRev = 18,
    labelTextYFwd = -8, labelTextYRev = 20,
  } = primerConfig || {};

  const [hoveredInsertion, setHoveredInsertion] = useState(null);
  const style = STYLES[mode];
  const info = computePrimerSegments(primer, charsPerLine);
  if (!info) return null;

  const { segs, isFwd, fivePrimeTail, threePrimeTail, hasFivePrimeTail, hasThreePrimeTail, threePrimeSeg, fivePrimeSeg } = info;

  // Color: orange if multi-site with secondary Tm > 45
  const bs = primer.bindingSites || [];
  const multiSite = bs.length > 1 && (bs[1]?.tm || 0) > 45;
  const baseColor = isFwd ? (primer.color || '#166534') : '#5b21b6';
  const pColor = multiSite ? '#c2410c' : baseColor;

  const showText = mode !== 'default';

  const handleInsEnter = useCallback((k) => setHoveredInsertion(k), []);
  const handleInsLeave = useCallback(() => setHoveredInsertion(null), []);

  const colX = (col) => getX(col % charsPerLine) + cw / 2;
  const textYMatch = isFwd ? matchTextYFwd : matchTextYRev;
  const textYMismatch = isFwd ? matchTextYFwd - mismatchOffset : matchTextYRev + mismatchOffset;
  const textYTail = isFwd ? tailTextYFwd : tailTextYRev;
  const textYLabel = isFwd ? labelTextYFwd : labelTextYRev;

  // Row Y cache (persisted via useRef across renders)
  const rowCacheRef = useRef({});
  const getRowYs = (row) => {
    const rowCache = rowCacheRef.current;
    if (rowCache[row]) return rowCache[row];
    const sy = rowYGetter(row);
    const trackOff = ((primerTracks?.[primer.id] || {})[row] || 0) * primerGap;
    const extraR = !isFwd ? ((primerFeatOffsets?.[primer.id] || {})[row] || 0) : 0;
    const base = isFwd ? sy - fPrimerBase : sy + rPrimerBase;
    return rowCache[row] = {
      matchY: base + (isFwd ? -trackOff : trackOff) + extraR,
      mismatchY: base + (isFwd ? -trackOff : trackOff) + extraR + (isFwd ? -mismatchOffset : mismatchOffset),
      misY: base + (isFwd ? -trackOff : trackOff) + extraR + (isFwd ? -tailYOffset : tailYOffset),
    };
  };

  // Collect insertion positions from all segments
  const allInsertions = [];
  for (const seg of segs) {
    const { alignmentCols } = seg;
    for (const ac of alignmentCols) {
      if (ac.insertionAfter) {
        allInsertions.push({
          templateCol: ac.templateCol,
          bases: ac.insertionAfter,
          seg,
        });
      }
    }
  }

  // 5' label: per-segment (for multi-line, each row gets label at 5' end)
  const getLabelX = (seg) => {
    if (isFwd) {
      // 5' = left edge of first column in seg
      return getX(seg.colStart);
    } else {
      // 5' = right edge of last column in seg
      return getX(seg.colEnd) + cw;
    }
  };

  return (
    <g>
      {/* 5' tail — rendered at first segment's mismatchY */}
      {hasFivePrimeTail && (() => {
        const rowYs = getRowYs(fivePrimeSeg.row);
        const tailLen = fivePrimeTail.length;
        const edge5x = isFwd
          ? getX(fivePrimeSeg.colStart)
          : getX(fivePrimeSeg.colEnd) + cw;
        const tailStartX = edge5x;
        const tailEndX = isFwd
          ? getX(fivePrimeSeg.colStart - tailLen)
          : getX(fivePrimeSeg.colEnd + tailLen + 1) + cw;

        return (
          <g key="5tail">
            {/* Tail line */}
            <path d={`M ${tailStartX} ${rowYs.misY} L ${tailEndX} ${rowYs.misY}`}
              fill="none" stroke={bgColor} strokeWidth="5" strokeLinecap="round" />
            <path d={`M ${tailStartX} ${rowYs.misY} L ${tailEndX} ${rowYs.misY}`}
              fill="none" stroke={pColor} strokeWidth="2.5" strokeLinecap="round" />
            {/* Tail text on hover/select */}
            {showText && (
              <text fill={style.text(pColor)} fontSize="14px" fontFamily={monoFont} fontWeight="bold"
                style={{ pointerEvents: 'none' }}>
                {fivePrimeTail.split('').map((c, k) => (
                  <tspan key={`5t-${k}`}
                    x={isFwd
                      ? getX(fivePrimeSeg.colStart - tailLen + k) + cw / 2
                      : getX(fivePrimeSeg.colEnd - k) + cw / 2}
                    y={rowYs.misY + textYTail} textAnchor="middle">{c}</tspan>
                ))}
              </text>
            )}
            {/* Invisible hit area */}
            <line x1={Math.min(tailStartX, tailEndX)} y1={rowYs.misY}
              x2={Math.max(tailStartX, tailEndX)} y2={rowYs.misY}
              stroke="transparent" strokeWidth="20" />
          </g>
        );
      })()}

      {/* Per-segment rendering */}
      {segs.map((seg) => {
        const { alignmentCols } = seg;
        const rowYs = getRowYs(seg.row);
        const { matchY, mismatchY, misY } = rowYs;
        const isThreePrime = seg === threePrimeSeg;

        const pathData = buildSegmentPath(
          seg, matchY, mismatchY, misY, isFwd, charsPerLine,
          isThreePrime, isThreePrime && hasThreePrimeTail, threePrimeTail,
          arrowWidth, arrowHeight
        );

        const hoverPath = buildSegmentHoverPath(
          seg, matchY, mismatchY, misY, isFwd, charsPerLine,
          primerExpand,
          isThreePrime && hasThreePrimeTail, threePrimeTail,
          seg === fivePrimeSeg && hasFivePrimeTail, fivePrimeTail
        );

        return (
          <g key={`${seg.row}-${seg.colStart}`}>
            {/* Background */}
            {hoverPath && <path d={hoverPath} {...style.bg(pColor)} />}
            {style.overlay(pColor) && hoverPath && <path d={hoverPath} {...style.overlay(pColor)} />}

            {/* Primer path */}
            {pathData && (
              <>
                <path d={pathData.d} fill="none" stroke={bgColor} strokeWidth="5"
                  strokeLinejoin="round" strokeLinecap="round" />
                <path d={pathData.d} fill="none" stroke={pColor} strokeWidth="2.5"
                  strokeLinejoin="round" strokeLinecap="round" />
              </>
            )}

            {/* Alignment text */}
            {showText && alignmentCols && (
              <text fill={style.text(pColor)} fontSize="14px" fontFamily={monoFont} fontWeight="bold"
                style={{ pointerEvents: 'none' }}>
                {alignmentCols.map((ac) => {
                  const isOffset = ac.kind === 'mismatch' || ac.kind === 'gap';
                  const y = isOffset ? mismatchY + textYMismatch : matchY + textYMatch;
                  const isGap = ac.kind === 'gap';
                  return (
                    <tspan key={`aln-${ac.templateCol}`} x={colX(ac.templateCol)} y={y} textAnchor="middle"
                      fill={isGap ? '#9ca3af' : undefined} fontWeight={isGap ? '200' : undefined}>
                      {ac.primerBase}
                    </tspan>
                  );
                })}
              </text>
            )}

            {/* 3' tail text (per segment when it's the three-prime segment) */}
            {showText && isThreePrime && hasThreePrimeTail && (
              <text fill={style.text(pColor)} fontSize="14px" fontFamily={monoFont} fontWeight="bold"
                style={{ pointerEvents: 'none' }}>
                {threePrimeTail.split('').map((c, k) => (
                  <tspan key={`3t-${k}`}
                    x={isFwd
                      ? getX(seg.colEnd + k + 1) + cw / 2
                      : getX(seg.colStart - threePrimeTail.length + k) + cw / 2}
                    y={misY + textYTail} textAnchor="middle">{c}</tspan>
                ))}
              </text>
            )}

            {/* Invisible hit area */}
            {pathData && <path d={pathData.d} fill="none" stroke="transparent" strokeWidth="20" />}

            {/* Insertion markers for this segment */}
            {allInsertions.filter(ins => ins.seg === seg).map((ins) => {
              const insX = getX((ins.templateCol + 0.5) % charsPerLine);
              const insTop = matchY + (isFwd ? -8 : 0);
              const insBot = matchY + (isFwd ? 0 : 8);
              const insKey = `${seg.row}-I-${ins.templateCol}`;
              // R primer insertion: display 3'→5' (reverse)
              const dispBases = isFwd ? ins.bases : [...ins.bases].reverse().join('');
              return (
                <g key={insKey}>
                  <line x1={insX} y1={insTop} x2={insX} y2={insBot}
                    stroke={pColor} strokeWidth="2" strokeLinecap="round" />
                  <line x1={insX} y1={insTop - 4} x2={insX} y2={insBot + 4}
                    stroke="transparent" strokeWidth="12"
                    onMouseEnter={() => handleInsEnter(insKey)} onMouseLeave={handleInsLeave} />
                  {hoveredInsertion === insKey && showText && (
                    <g style={{ pointerEvents: 'none' }}>
                      <rect x={insX - dispBases.length * cw / 2 - 4}
                        y={isFwd ? insTop - 22 : insBot + 2}
                        width={dispBases.length * cw + 8} height={20} rx={4}
                        fill={bgColor} stroke={pColor} strokeWidth="1" />
                      <text x={insX} y={isFwd ? insTop - 7 : insBot + 16}
                        fill={pColor} fontSize="13px" fontFamily={monoFont}
                        fontWeight="bold" textAnchor="middle">{dispBases}</text>
                    </g>
                  )}
                </g>
              );
            })}

            {/* Per-line label at 5' end */}
            {style.showLabelCollapsed && seg === fivePrimeSeg && (
              <text x={getLabelX(seg)} y={getRowYs(seg.row).matchY + textYLabel}
                fill={style.labelColor(pColor)} fontSize="12px" fontFamily="TeX Gyre Heros"
                fontWeight="600" fontStyle="italic"
                textAnchor={isFwd ? 'start' : 'end'}
                stroke={style.labelStroke(pColor)} strokeWidth="4" paintOrder="stroke fill"
                style={{ opacity: 1, transition: springAnim, pointerEvents: 'none' }}>
                {primer.name}</text>
            )}
            {style.showLabelExpanded && (
              <text x={getLabelX(seg)} y={getRowYs(seg.row).matchY + (isFwd ? textYLabel - 15 : textYLabel + 12)}
                fill={style.expandedLabelColor(pColor)} fontSize="12px" fontFamily="TeX Gyre Heros"
                fontWeight="600" fontStyle="italic"
                textAnchor={isFwd ? 'start' : 'end'}
                stroke={style.expandedLabelStroke(pColor)} strokeWidth="4" paintOrder="stroke fill"
                style={{ opacity: 1, transition: springAnim, pointerEvents: 'none' }}>
                {primer.name}</text>
            )}
          </g>
        );
      })}
    </g>
  );
}
