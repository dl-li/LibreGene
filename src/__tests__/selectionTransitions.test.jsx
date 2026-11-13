import { describe, expect, it, vi } from 'vitest';
import { renderFeatures, renderFeatureLabels } from '../editor/layers/features';
import { renderEnzymeLabels } from '../editor/layers/enzymes';

const feature = {
  id: 'f1',
  name: 'Feature 1',
  start: 2,
  end: 5,
  strand: '+',
  color: '#60A5FA',
  segments: [{ start: 2, end: 5 }],
};

const mouseEvent = {
  button: 0,
  clientX: 0,
  clientY: 0,
  stopPropagation: vi.fn(),
  preventDefault: vi.fn(),
};

function mouseDownHandlers(node, handlers = []) {
  if (Array.isArray(node)) {
    node.forEach((child) => mouseDownHandlers(child, handlers));
    return handlers;
  }
  if (!node || typeof node !== 'object') return handlers;
  if (node.props?.onMouseDown) handlers.push(node.props.onMouseDown);
  mouseDownHandlers(node.props?.children, handlers);
  return handlers;
}

const geometry = {
  visibleFeatures: [feature],
  hoveredFeature: null,
  alwaysExpandFeatures: false,
  featureLabelsBelow: false,
  labelViewport: null,
  sp: (start, end) => [{ row: 0, colStart: start, colEnd: end }],
  abutColors: { feat: {}, seg: {} },
  rowStarts: [0],
  rowCounts: [10],
  getSeqY: () => 0,
  alignLaneInfo: { chromBelow: [0], counts: [0] },
  featureRowTracks: {},
  lp: { featTrackHeight: 10, featBaseOffset: 20 },
  ALIGN_FEAT_GAP: 10,
  colRuns: (start, end) => [[start, end - start + 1]],
  colFromVis: (col) => col,
  colVis: (col) => col,
  solidLineCols: new Map(),
  isDraggingRef: { current: false },
  featureLeaveRef: { current: null },
  setHoveredFeature: vi.fn(),
  openFeatureMenu: vi.fn(),
  featureSelRef: { current: null },
  setCreateFeatureLoc: vi.fn(),
  setFeatureInfoFeature: vi.fn(),
};

describe('selection type transitions', () => {
  it('uses the shared text-range transition when a feature bar is clicked', () => {
    const selectTextRange = vi.fn();
    const tree = renderFeatures({
      ...geometry,
      translationDragRef: { current: null },
      cdsFeatureData: {},
      svgRef: { current: null },
      setHoveredCodon: vi.fn(),
      selectTextRange,
      rowOf: () => 0,
      hoveredCodon: null,
      startTranslationSelection: vi.fn(),
    });
    const handlers = mouseDownHandlers(tree);
    expect(handlers).toHaveLength(1);
    handlers[0](mouseEvent);
    expect(selectTextRange).toHaveBeenCalledWith(2, 5);
  });

  it.each(['+', '-'])('uses the shared transition for %s feature labels', (strand) => {
    const selectTextRange = vi.fn();
    const tree = renderFeatureLabels({
      ...geometry,
      visibleFeatures: [{ ...feature, strand }],
      selectTextRange,
    });
    const handlers = mouseDownHandlers(tree);
    expect(handlers).toHaveLength(1);
    handlers[0](mouseEvent);
    expect(selectTextRange).toHaveBeenCalledWith(2, 5);
  });

  it('prepares a text-compatible selection before selecting an enzyme label', () => {
    const prepareEnzymeSelection = vi.fn();
    const setIsEnzymeSelection = vi.fn();
    const tree = renderEnzymeLabels({
      hoveredEnzyme: null,
      enzymeLayout: [
        {
          id: 'e1',
          groupId: 'e1',
          name: 'EcoRI',
          cutX: 0,
          yTop: 0,
          sy: 20,
          enzW: 50,
          isUnique: true,
          pairIndex: 0,
          topCutIndex: 3,
        },
      ],
      enzymes: [
        {
          id: 'e1',
          name: 'EcoRI',
          cutIndex: 3,
          botCutIndex: 4,
          recStart: 1,
          recEnd: 6,
          displayStart: 1,
          displayEnd: 6,
        },
      ],
      selectedEnzymeIds: [],
      totalNameCounts: new Map(),
      openEnzymeMenu: vi.fn(),
      openEnzymeDetail: vi.fn(),
      enzymeDragRef: { current: null },
      isDraggingRef: { current: false },
      setHoveredEnzyme: vi.fn(),
      setSelStart: vi.fn(),
      setSelEnd: vi.fn(),
      setCursorIndex: vi.fn(),
      setSelectedEnzymeIds: vi.fn(),
      prepareEnzymeSelection,
      lastEnzymeSelRef: { current: null },
      clearCursorTimer: vi.fn(),
      setIsEnzymeSelection,
      cleanSeq: 'ACGTACGTACGT',
      topology: 'linear',
      setIsDragging: vi.fn(),
      setIsEnzymeDragging: vi.fn(),
    });
    const handlers = mouseDownHandlers(tree);
    expect(handlers).toHaveLength(1);
    handlers[0](mouseEvent);
    expect(prepareEnzymeSelection).toHaveBeenCalledOnce();
    expect(setIsEnzymeSelection).toHaveBeenCalledWith(true);
    expect(prepareEnzymeSelection.mock.invocationCallOrder[0]).toBeLessThan(
      setIsEnzymeSelection.mock.invocationCallOrder[0],
    );
  });
});
