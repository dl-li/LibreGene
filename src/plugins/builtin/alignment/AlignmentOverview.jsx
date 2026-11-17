import { useEffect, useRef, useState } from 'react';
import { OverviewMap } from '@/components/ContinuousOverview';
import { alignmentGapSegments } from '@/editor/alignmentLayout';

const ROW_H = 18;
const MAP_H = 40;
const PAD_X = 14;
const ARROW_COLOR = '#c81e1e';
const HOVER_COLOR = '#2684e8';

export function alignmentMatchRuns(alignment, sequence) {
  const sequenceLength = sequence?.length || 0;
  if (!sequenceLength) return [];
  const segments = [
    ...(alignment.segments || []),
    ...alignmentGapSegments(alignment, sequenceLength),
  ].sort((a, b) => a.start - b.start);
  const runs = [];

  for (const segment of segments) {
    for (let i = 0; i <= segment.end - segment.start; i++) {
      const column = segment.start + i;
      const char = segment.chars?.[i] || '-';
      const matched =
        !segment.gap &&
        char !== '-' &&
        char.toUpperCase() === (sequence[column] || '').toUpperCase();
      const last = runs[runs.length - 1];
      if (last && last.matched === matched && last.end + 1 === column) {
        last.end = column;
      } else {
        runs.push({ start: column, end: column, matched });
      }
    }
  }
  return runs;
}

export function alignmentOverviewPieces(alignment, sequenceLength) {
  const segments = alignment.segments || [];
  if (!segments.length || !sequenceLength) return [];
  const start = segments[0].start;
  const end = segments[segments.length - 1].end;
  if (end < start) {
    return [
      { start, end: sequenceLength - 1, tip: alignment.strand !== '-' },
      { start: 0, end, tip: alignment.strand === '-' },
    ];
  }
  return [{ start, end, tip: true }];
}

function arrowPoints(x0, x1, y, strand, tip) {
  const width = Math.max(0, x1 - x0);
  if (!tip || width < 1) {
    return `${x0},${y - 3} ${x1},${y - 3} ${x1},${y + 3} ${x0},${y + 3}`;
  }
  const head = Math.min(8, width / 2);
  if (strand === '-') {
    return `${x1},${y - 3} ${x0 + head},${y - 3} ${x0 + head},${y - 5} ${x0},${y} ${x0 + head},${y + 5} ${x0 + head},${y + 3} ${x1},${y + 3}`;
  }
  return `${x0},${y - 3} ${x1 - head},${y - 3} ${x1 - head},${y - 5} ${x1},${y} ${x1 - head},${y + 5} ${x1 - head},${y + 3} ${x0},${y + 3}`;
}

export function AlignmentArrow({ alignment, sequence, width, y, hovered }) {
  const sequenceLength = sequence?.length || 0;
  const mapLength = Math.max(0, width - PAD_X * 2);
  const px = (position) => PAD_X + (position / sequenceLength) * mapLength;
  const color = hovered ? HOVER_COLOR : ARROW_COLOR;
  const pieces = alignmentOverviewPieces(alignment, sequenceLength);
  const runs = alignmentMatchRuns(alignment, sequence);

  return (
    <g style={{ transition: 'fill 120ms, stroke 120ms' }}>
      {pieces.map((piece) => (
        <g key={`${piece.start}-${piece.end}`}>
          <polygon
            points={arrowPoints(px(piece.start), px(piece.end + 1), y, alignment.strand, piece.tip)}
            fill={color}
          />
          {runs
            .filter((run) => !run.matched)
            .map((run, runIndex) => {
              const start = Math.max(piece.start, run.start);
              const end = Math.min(piece.end, run.end);
              if (end < start) return null;
              const x0 = px(start);
              const x1 = px(end + 1);
              return (
                <rect
                  key={`${run.start}-${runIndex}`}
                  x={x0}
                  y={y - 2}
                  width={Math.max(0.5, x1 - x0)}
                  height={4}
                  fill="var(--card)"
                  stroke={color}
                  strokeWidth={0.8}
                />
              );
            })}
          {(alignment.insertions || []).map((insertion, insertionIndex) => {
            if (insertion.pos < piece.start || insertion.pos > piece.end + 1) return null;
            const x = px(insertion.pos);
            return (
              <line
                key={`${insertion.pos}-${insertionIndex}`}
                x1={x}
                y1={y - 5}
                x2={x}
                y2={y + 5}
                stroke={color}
                strokeWidth={1.5}
              />
            );
          })}
        </g>
      ))}
    </g>
  );
}

export default function AlignmentOverview({ alignments, sequence, features, hoveredId }) {
  const rootRef = useRef(null);
  const [width, setWidth] = useState(0);
  const sequenceLength = sequence?.length || 0;

  useEffect(() => {
    const element = rootRef.current;
    if (!element) return;
    const sync = () => setWidth(element.clientWidth);
    sync();
    const observer = new ResizeObserver(sync);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  if (!sequenceLength) return null;

  return (
    <div className="shrink-0 overflow-hidden rounded-lg border border-border/60 bg-background/60 px-3 py-2">
      <div ref={rootRef}>
        {width > 0 && (
          <>
            <div className="max-h-40 overflow-y-auto">
              <svg width={width} height={alignments.length * ROW_H} className="block">
                {alignments.map((alignment, index) => (
                  <AlignmentArrow
                    key={alignment.id}
                    alignment={alignment}
                    sequence={sequence}
                    width={width}
                    y={index * ROW_H + ROW_H / 2}
                    hovered={hoveredId === alignment.id}
                  />
                ))}
              </svg>
            </div>
            <OverviewMap
              width={width}
              height={MAP_H}
              features={features}
              sequenceLength={sequenceLength}
              padX={PAD_X}
            />
          </>
        )}
      </div>
    </div>
  );
}
