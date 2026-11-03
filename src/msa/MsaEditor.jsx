import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { computeConsensus, rowDisplaySequence } from './msaAnalysis';

const CW = 10;
const ROW_H = 24;
const LABEL_W = 176;
const RULER_H = 24;

const DNA_COLORS = {
  A: '#15803d',
  C: '#2563eb',
  G: '#b45309',
  T: '#dc2626',
  U: '#dc2626',
};

const PROTEIN_GROUPS = [
  ['#2563eb', 'KRH'],
  ['#15803d', 'STNQ'],
  ['#7c3aed', 'DE'],
  ['#b45309', 'AVLIM'],
  ['#db2777', 'FWY'],
  ['#0891b2', 'G'],
  ['#ea580c', 'P'],
  ['#65a30d', 'C'],
];

function baseColor(c, moleculeType) {
  const symbol = c.toUpperCase();
  if (moleculeType === 'protein') {
    return PROTEIN_GROUPS.find(([, group]) => group.includes(symbol))?.[0] || '#64748b';
  }
  return DNA_COLORS[symbol] || '#64748b';
}

function buildRuns(sequence, startCol, endCol, getStyle) {
  const runs = [];
  for (let col = startCol; col <= endCol && col < sequence.length; col++) {
    const style = getStyle(sequence[col] || '-', col);
    const previous = runs[runs.length - 1];
    if (previous && previous.color === style.color && previous.background === style.background) {
      previous.text += sequence[col];
      previous.endCol = col;
    } else {
      runs.push({
        text: sequence[col],
        startCol: col,
        endCol: col,
        color: style.color,
        background: style.background,
      });
    }
  }
  return runs;
}

const SequenceLine = React.memo(function SequenceLine({
  sequence,
  startCol = 0,
  endCol = sequence.length - 1,
  label,
  rowIndex,
  selection,
  onSelect,
  styleFor,
  stickyLabel = false,
  reference = false,
}) {
  const runs = useMemo(
    () => buildRuns(sequence, startCol, endCol, styleFor),
    [sequence, startCol, endCol, styleFor],
  );
  const width = Math.max(0, endCol - startCol + 1) * CW;
  const selected =
    selection &&
    rowIndex >= Math.min(selection.startRow, selection.endRow) &&
    rowIndex <= Math.max(selection.startRow, selection.endRow)
      ? selection
      : null;
  const selectionStart = selected
    ? Math.max(startCol, Math.min(selected.startCol, selected.endCol))
    : 0;
  const selectionEnd = selected
    ? Math.min(endCol, Math.max(selected.startCol, selected.endCol))
    : -1;

  const columnAt = (event) => {
    const rect = event.currentTarget.getBoundingClientRect();
    return Math.max(
      startCol,
      Math.min(endCol, startCol + Math.floor((event.clientX - rect.left) / CW)),
    );
  };

  return (
    <div className="relative h-6" style={{ width: stickyLabel ? '100%' : undefined }}>
      <div
        className={`top-0 z-10 flex h-6 items-center gap-1.5 border-r border-border/60 bg-background px-2 text-xs ${
          stickyLabel ? 'sticky left-0 float-left' : 'absolute left-0'
        }`}
        style={{ width: LABEL_W }}
        title={label}
      >
        <span className="truncate font-medium text-foreground/80">{label}</span>
        {reference && (
          <span className="shrink-0 rounded bg-teal-50 px-1 text-[9px] font-semibold text-teal-700">
            REF
          </span>
        )}
      </div>
      <svg
        className="absolute top-0 h-6 select-none overflow-visible font-mono text-[12px]"
        style={{ left: LABEL_W + (stickyLabel ? startCol * CW : 0), width }}
        onMouseDown={(event) => {
          if (!onSelect || event.button !== 0) return;
          event.preventDefault();
          onSelect(rowIndex, columnAt(event), false);
        }}
        onMouseEnter={(event) => {
          if (onSelect && (event.buttons & 1) === 1) onSelect(rowIndex, columnAt(event), true);
        }}
      >
        {runs.map((run) => (
          <g key={`${run.startCol}-${run.endCol}`}>
            {run.background && (
              <rect
                x={(run.startCol - startCol) * CW}
                y={2}
                width={(run.endCol - run.startCol + 1) * CW}
                height={19}
                fill={run.background}
              />
            )}
            <text
              x={(run.startCol - startCol) * CW + 1}
              y={16}
              fill={run.color}
              style={{ letterSpacing: `${CW - 7.2}px` }}
            >
              {run.text}
            </text>
          </g>
        ))}
        {selected && selectionEnd >= selectionStart && (
          <rect
            x={(selectionStart - startCol) * CW}
            y={1}
            width={(selectionEnd - selectionStart + 1) * CW}
            height={21}
            fill="#3b82f6"
            fillOpacity={0.22}
            stroke="#2563eb"
            strokeOpacity={0.45}
            pointerEvents="none"
          />
        )}
      </svg>
    </div>
  );
});

function Ruler({ startCol, endCol, stickyLabel = false }) {
  const ticks = [];
  for (let col = startCol; col <= endCol; col++) {
    if ((col + 1) % 10 === 0 || col === startCol) ticks.push(col);
  }
  return (
    <div className="relative h-6 border-b border-border/50 text-[10px] text-muted-foreground">
      <div
        className={`top-0 z-20 flex h-6 items-center border-r border-border/60 bg-background px-2 ${
          stickyLabel ? 'sticky left-0 float-left' : 'absolute left-0'
        }`}
        style={{ width: LABEL_W }}
      >
        Alignment column
      </div>
      <svg
        className="absolute top-0 h-6"
        style={{
          left: LABEL_W + (stickyLabel ? startCol * CW : 0),
          width: (endCol - startCol + 1) * CW,
        }}
      >
        {ticks.map((col) => (
          <text key={col} x={(col - startCol) * CW + 1} y={16}>
            {col + 1}
          </text>
        ))}
      </svg>
    </div>
  );
}

function ConservationLine({ values, startCol, endCol, stickyLabel = false }) {
  return (
    <div className="relative h-6">
      <div
        className={`top-0 z-10 flex h-6 items-center border-r border-border/60 bg-background px-2 text-xs text-muted-foreground ${
          stickyLabel ? 'sticky left-0 float-left' : 'absolute left-0'
        }`}
        style={{ width: LABEL_W }}
      >
        Conservation
      </div>
      <svg
        className="absolute top-0 h-6"
        style={{
          left: LABEL_W + (stickyLabel ? startCol * CW : 0),
          width: (endCol - startCol + 1) * CW,
        }}
      >
        {values.slice(startCol, endCol + 1).map((value, index) => (
          <rect
            key={startCol + index}
            x={index * CW + 2}
            y={20 - value * 16}
            width={Math.max(2, CW - 4)}
            height={Math.max(1, value * 16)}
            rx={1}
            fill="#0f766e"
            fillOpacity={0.25 + value * 0.65}
          />
        ))}
      </svg>
    </div>
  );
}

export default function MsaEditor({
  rows,
  aligned,
  moleculeType,
  referenceRowId,
  colorMode,
  selection,
  onSelectionChange,
  hidden = false,
}) {
  const containerRef = useRef(null);
  const scrollRef = useRef(null);
  const [width, setWidth] = useState(1000);
  const [scroll, setScroll] = useState({ top: 0, left: 0 });
  const sequences = useMemo(
    () => rows.map((row) => rowDisplaySequence(row, aligned)),
    [rows, aligned],
  );
  const columnCount = sequences.reduce((max, sequence) => Math.max(max, sequence.length), 0);
  const consensus = useMemo(() => computeConsensus(rows, aligned), [rows, aligned]);
  const referenceIndex = rows.findIndex((row) => row.id === referenceRowId);
  const referenceSequence = referenceIndex >= 0 ? sequences[referenceIndex] : consensus.sequence;

  useEffect(() => {
    if (hidden || !containerRef.current) return undefined;
    const element = containerRef.current;
    const update = () => setWidth(element.clientWidth);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [hidden]);

  useEffect(() => {
    setScroll({ top: 0, left: 0 });
    scrollRef.current?.scrollTo({ top: 0, left: 0 });
  }, [rows.length, aligned]);

  const comparisonTarget =
    colorMode === 'reference' && referenceIndex >= 0
      ? referenceSequence
      : colorMode === 'consensus'
        ? consensus.sequence
        : null;

  const styleFor = useCallback(
    (c, col) => {
      if (comparisonTarget && c !== '-') {
        const matches = comparisonTarget[col] === c;
        return {
          color: matches ? baseColor(c, moleculeType) : '#b91c1c',
          background: matches ? null : '#fee2e2',
        };
      }
      return { color: baseColor(c, moleculeType), background: null };
    },
    [comparisonTarget, moleculeType],
  );

  const consensusStyle = useCallback(
    (c, col) => ({
      color: baseColor(c, moleculeType),
      background: `rgba(15, 118, 110, ${Math.min(0.2, (consensus.conservation[col] || 0) * 0.2)})`,
    }),
    [moleculeType, consensus.conservation],
  );

  const handleScroll = useCallback((event) => {
    const left = event.currentTarget.scrollLeft;
    const top = event.currentTarget.scrollTop;
    setScroll((previous) => {
      if (
        Math.floor(previous.left / CW) === Math.floor(left / CW) &&
        Math.floor(previous.top / ROW_H) === Math.floor(top / ROW_H)
      ) {
        return previous;
      }
      return { left, top };
    });
  }, []);

  const visibleStartCol = Math.max(0, Math.floor(scroll.left / CW) - 8);
  const visibleEndCol = Math.min(columnCount - 1, Math.ceil((scroll.left + width) / CW) + 8);
  const firstRow = Math.max(0, Math.floor((scroll.top - RULER_H) / ROW_H) - 4);
  const lastRow = Math.min(
    rows.length - 1,
    Math.ceil((scroll.top + (containerRef.current?.clientHeight || 700)) / ROW_H) + 4,
  );
  const totalWidth = LABEL_W + columnCount * CW + 40;
  const totalHeight = RULER_H + (rows.length + 2) * ROW_H + 20;

  return (
    <div ref={containerRef} className="relative min-h-0 flex-1 overflow-hidden">
      <div
        ref={scrollRef}
        className="h-full overflow-auto overscroll-contain"
        onScroll={handleScroll}
      >
        <div className="relative" style={{ width: totalWidth, height: totalHeight }}>
          <div className="sticky top-0 z-30 bg-background">
            <Ruler startCol={visibleStartCol} endCol={visibleEndCol} stickyLabel />
          </div>
          {rows.slice(firstRow, lastRow + 1).map((row, offset) => {
            const rowIndex = firstRow + offset;
            return (
              <div
                key={row.id}
                className="absolute left-0"
                style={{ top: RULER_H + rowIndex * ROW_H, width: totalWidth }}
              >
                <SequenceLine
                  sequence={sequences[rowIndex]}
                  startCol={visibleStartCol}
                  endCol={visibleEndCol}
                  label={row.name}
                  rowIndex={rowIndex}
                  selection={selection}
                  onSelect={onSelectionChange}
                  styleFor={styleFor}
                  stickyLabel
                  reference={row.id === referenceRowId}
                />
              </div>
            );
          })}
          <div
            className="absolute left-0"
            style={{ top: RULER_H + rows.length * ROW_H, width: totalWidth }}
          >
            <SequenceLine
              sequence={consensus.sequence}
              startCol={visibleStartCol}
              endCol={visibleEndCol}
              label="Consensus"
              rowIndex={-1}
              styleFor={consensusStyle}
              stickyLabel
            />
          </div>
          <div
            className="absolute left-0"
            style={{ top: RULER_H + (rows.length + 1) * ROW_H, width: totalWidth }}
          >
            <ConservationLine
              values={consensus.conservation}
              startCol={visibleStartCol}
              endCol={visibleEndCol}
              stickyLabel
            />
          </div>
        </div>
      </div>
    </div>
  );
}
