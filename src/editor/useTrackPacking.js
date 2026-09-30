import { useMemo, useRef } from 'react';
import { baseSeqY, cw, enzLabelW, getX, primerLabelW } from '../editorConstants';
import { ensureReadableColor, shiftAbutLightness } from './colors';

// Track packing / row geometry: feature color normalization, feature+primer
// track assignment, per-row enzyme track lifts, adaptive row spacing, the
// visible row range, and per-row coverage maps.
export default function useTrackPacking({
  features,
  enrichedPrimers,
  enzymes,
  numRows,
  rowStarts,
  rowCounts,
  rowOf,
  sp,
  colVis,
  pp,
  lp,
  featureLabelsBelow,
  alignLaneInfo,
  scrollY,
  viewportH,
  ALIGN_FEAT_GAP,
  ROW_BUF,
}) {
  // --- collision avoidance: features + primers ---
  // Normalize features and pre-compute colors once
  const normFeatures = useMemo(
    () =>
      (features || []).map((f) => {
        const isRepeat = /repeat/i.test(f.ftype || '');
        const fixColor = (c) => (c && !isRepeat && !f.orf ? ensureReadableColor(c) : c);
        const fixedColor = fixColor(f.color);
        const segments = (
          f.segments && f.segments.length ? f.segments : [{ start: f.start, end: f.end }]
        ).map((seg) => ({
          ...seg,
          color: fixColor(seg.color),
        }));
        // Pre-compute dominant color (longest segment color)
        const colorLen = {};
        for (const seg of segments) {
          const c = seg.color || fixedColor || '#60A5FA';
          colorLen[c] = (colorLen[c] || 0) + (seg.end - seg.start + 1);
        }
        let dominantColor = fixedColor || '#60A5FA',
          bestLen = 0;
        for (const [c, len] of Object.entries(colorLen)) {
          if (len > bestLen) {
            dominantColor = c;
            bestLen = len;
          }
        }
        return { ...f, color: fixedColor, segments, dominantColor };
      }),
    [features],
  );

  // Lightness-nudge for abutting (touching, non-overlapping) same-colored bars
  // so they stay distinguishable. Chain rule: A-B-C-D same color → A base,
  // B shifted, C base (B already differs), D shifted. Segment level applies in
  // both modes (joined segments that touch); feature level only in
  // labels-below mode.
  const abutColors = useMemo(() => {
    const feat = {};
    if (featureLabelsBelow) {
      const ordered = [...normFeatures].sort(
        (a, b) =>
          Math.min(...a.segments.map((s) => s.start)) - Math.min(...b.segments.map((s) => s.start)),
      );
      const shownByEnd = new Map();
      for (const f of ordered) {
        const start = Math.min(...f.segments.map((s) => s.start));
        const end = Math.max(...f.segments.map((s) => s.end));
        const color = f.color || ensureReadableColor('#60A5FA');
        const abutters = shownByEnd.get(start - 1);
        const shown = abutters && abutters.includes(color) ? shiftAbutLightness(color) : color;
        if (shown !== color) feat[f.id] = shown;
        if (!shownByEnd.has(end)) shownByEnd.set(end, []);
        shownByEnd.get(end).push(shown);
      }
    }
    const seg = {};
    for (const f of normFeatures) {
      const order = f.segments
        .map((_, i) => i)
        .sort((a, b) => f.segments[a].start - f.segments[b].start);
      const cols = new Array(f.segments.length);
      let prevDi = -1;
      for (const di of order) {
        const s = f.segments[di];
        const base = s.color || feat[f.id] || f.color || ensureReadableColor('#60A5FA');
        cols[di] =
          prevDi >= 0 && s.start === f.segments[prevDi].end + 1 && base === cols[prevDi]
            ? shiftAbutLightness(base)
            : base;
        prevDi = di;
      }
      seg[f.id] = cols;
    }
    return { seg, feat };
  }, [normFeatures, featureLabelsBelow]);

  const { processedFeatures, primerTracks, featureRowTracks, revPrimerFeatOffsets } =
    useMemo(() => {
      const resultFeatures = [];
      const bottomTracks = [];

      if (normFeatures.length > 0) {
        const sorted = [...normFeatures].sort((a, b) => {
          // ORFs get lowest track priority (bottom-most)
          if (!!a.orf !== !!b.orf) return a.orf ? 1 : -1;
          const la = a.segments.reduce((s, seg) => s + seg.end - seg.start, 0);
          const lb = b.segments.reduce((s, seg) => s + seg.end - seg.start, 0);
          return lb - la || a.segments[0].start - b.segments[0].start;
        });
        for (const f of sorted) {
          const allStarts = f.segments.map((s) => s.start);
          const allEnds = f.segments.map((s) => s.end);
          const es = Math.min(...allStarts) - 0.5,
            ee = Math.max(...allEnds) + 0.5;
          let placed = false;
          for (let i = 0; i < bottomTracks.length; i++) {
            if (!bottomTracks[i].some((t) => !(ee < t.start || es > t.end))) {
              bottomTracks[i].push({ start: es, end: ee });
              resultFeatures.push({ ...f, trackIdx: i });
              placed = true;
              break;
            }
          }
          if (!placed) {
            bottomTracks.push([{ start: es, end: ee }]);
            resultFeatures.push({ ...f, trackIdx: bottomTracks.length - 1 });
          }
        }
      }

      // Per-row primer track assignment — primers on different rows can share tracks.
      const pTracks = {}; // { [primerId]: { [row]: trackIndex } }
      for (const isFwd of [true, false]) {
        const ofType = (enrichedPrimers || []).filter((p) => p.isFwd === isFwd);
        if (!ofType.length) continue;
        const matchLen = (p) =>
          (p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }]).reduce(
            (s, m) => s + m.end - m.start + 1,
            0,
          );
        const sorted = [...ofType].sort((a, b) => {
          const la = matchLen(a) + (a.mismatchStr?.length || 0);
          const lb = matchLen(b) + (b.mismatchStr?.length || 0);
          return lb - la || a.matchStart - b.matchStart;
        });
        for (let r = 0; r < numRows; r++) {
          const rs = rowStarts[r],
            re = rowStarts[r] + rowCounts[r] - 1;
          const rowTracks = [];
          for (const p of sorted) {
            const ml = p.mismatchStr?.length || 0;
            const segs = p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }];
            segs.forEach((m, mi) => {
              // 5' tail extends left of matchStart (fwd) / right of matchEnd (rev)
              const rawVs = isFwd && mi === 0 ? m.start - ml : m.start;
              const rawVe = !isFwd && mi === segs.length - 1 ? m.end + ml : m.end;
              if (rawVe < rs || rawVs > re) return;
              // 1nt judgment buffer on the 3' (arrow-tip) side so two abutting
              // primers don't share a track and bleed into each other
              const vs = !isFwd && mi === 0 ? rawVs - 1 : rawVs;
              const ve = isFwd && mi === segs.length - 1 ? rawVe + 1 : rawVe;
              if (!pTracks[p.id]) pTracks[p.id] = {};
              let placed = false;
              for (let i = 0; i < rowTracks.length; i++) {
                if (!rowTracks[i].some((t) => !(ve < t.start || vs > t.end))) {
                  rowTracks[i].push({ start: vs, end: ve });
                  if (pTracks[p.id][r] === undefined || i < pTracks[p.id][r]) {
                    pTracks[p.id][r] = i;
                  }
                  placed = true;
                  break;
                }
              }
              if (!placed) {
                rowTracks.push([{ start: vs, end: ve }]);
                if (pTracks[p.id][r] === undefined) pTracks[p.id][r] = rowTracks.length - 1;
              }
            });
          }
        }
      }

      // Gap connector pieces: a segmented feature's faint connector line
      // spans its gap columns in every row the gap passes through. Rows
      // where the feature also has a segment extend its own reservation to
      // cover the connector columns; rows without a segment default the
      // connector to track 0, which is seeded up front so other features
      // (e.g. long ORFs spanning the gap) drop to a lower track instead of
      // overlapping the connector line.
      const gapPiecesByFeatRow = {};
      const gapBlockersByRow = {};
      for (const f of resultFeatures) {
        const segs = f.segments;
        if (!segs || segs.length < 2) continue;
        const segRows = new Set();
        for (const seg of segs) {
          for (let r = rowOf(seg.start); r <= rowOf(seg.end); r++) {
            segRows.add(r);
          }
        }
        for (let di = 1; di < segs.length; di++) {
          const gStart = segs[di - 1].end + 1;
          const gEnd = segs[di].start - 1;
          if (gStart > gEnd) continue;
          for (let r = rowOf(gStart); r <= rowOf(gEnd); r++) {
            const pieceStart = Math.max(gStart, rowStarts[r]);
            const pieceEnd = Math.min(gEnd, rowStarts[r] + rowCounts[r] - 1);
            if (pieceStart > pieceEnd) continue;
            const entry = { start: pieceStart - 0.5, end: pieceEnd + 0.5 };
            ((gapPiecesByFeatRow[f.id] || (gapPiecesByFeatRow[f.id] = {}))[r] ||
              (gapPiecesByFeatRow[f.id][r] = [])).push(entry);
            if (!segRows.has(r)) (gapBlockersByRow[r] || (gapBlockersByRow[r] = [])).push(entry);
          }
        }
      }

      // Per-row feature track assignment — features only reserve space where they actually overlap
      const fRowTracks = {};
      for (let r = 0; r < numRows; r++) {
        const rs = rowStarts[r],
          re = rowStarts[r] + rowCounts[r] - 1;
        const rowFeats = resultFeatures.filter((f) =>
          f.segments.some((seg) => !(seg.end < rs || seg.start > re)),
        );
        rowFeats.sort((a, b) => {
          // ORFs get lowest track priority (bottom-most)
          if (!!a.orf !== !!b.orf) return a.orf ? 1 : -1;
          const la = a.segments.reduce((s, seg) => s + seg.end - seg.start, 0);
          const lb = b.segments.reduce((s, seg) => s + seg.end - seg.start, 0);
          return lb - la || a.segments[0].start - b.segments[0].start;
        });
        const rowTracks = [];
        for (const b of gapBlockersByRow[r] || []) {
          (rowTracks[0] || (rowTracks[0] = [])).push(b);
        }
        for (const f of rowFeats) {
          const rowSegs = f.segments.filter((seg) => !(seg.end < rs || seg.start > re));
          const segStart = Math.min(...rowSegs.map((s) => s.start));
          const segEnd = Math.max(...rowSegs.map((s) => s.end));
          const isFRev = f.strand === '-';
          // Below-line labels sit flush under the bar (fwd: left end aligned,
          // rev: right end aligned); the whole feature interval extends one
          // track down, so nothing in the next track sits under this feature.
          // Below mode uses no half-column margins: abutting (non-overlapping)
          // features may share a track.
          const hangsBelow = featureLabelsBelow;
          // Below-mode labels are never truncated: reserve the exact rendered
          // label width instead of the padded estimate.
          const labelCols = hangsBelow
            ? Math.ceil(
                primerLabelW(isFRev ? `< ${f.name}` : f.strand === '+' ? `${f.name} >` : f.name) /
                  cw,
              )
            : Math.ceil(primerLabelW(f.name) / cw) + 2 + (f.strand && f.strand !== '.' ? 2 : 0);
          const es0 = hangsBelow
            ? isFRev
              ? Math.min(segStart, Math.max(rs, segEnd - labelCols))
              : segStart
            : isFRev
              ? segStart - 0.5
              : Math.max(rs, segStart - labelCols) - 0.5;
          const ee0 = hangsBelow
            ? isFRev
              ? segEnd
              : Math.max(segEnd, segStart + labelCols)
            : isFRev
              ? segEnd + labelCols + 0.5
              : segEnd + 0.5;
          // Extend the reservation across this row's gap-connector columns.
          let es = es0;
          let ee = ee0;
          for (const gp of gapPiecesByFeatRow[f.id]?.[r] || []) {
            if (gp.start < es) es = gp.start;
            if (gp.end > ee) ee = gp.end;
          }
          const overlaps = (track, s, e) =>
            rowTracks[track] && rowTracks[track].some((t) => !(e < t.start || s > t.end));
          let placed = false;
          for (let i = 0; i < rowTracks.length; i++) {
            if (!overlaps(i, es, ee) && (!hangsBelow || !overlaps(i + 1, es, ee))) {
              rowTracks[i].push({ start: es, end: ee });
              if (hangsBelow) {
                if (!rowTracks[i + 1]) rowTracks[i + 1] = [];
                rowTracks[i + 1].push({ start: es, end: ee });
              }
              if (!fRowTracks[f.id]) fRowTracks[f.id] = {};
              fRowTracks[f.id][r] = i;
              placed = true;
              break;
            }
          }
          if (!placed) {
            rowTracks.push([{ start: es, end: ee }]);
            if (hangsBelow) rowTracks.push([{ start: es, end: ee }]);
            if (!fRowTracks[f.id]) fRowTracks[f.id] = {};
            fRowTracks[f.id][r] = rowTracks.length - (hangsBelow ? 2 : 1);
          }
        }
      }

      // Rev primer offset when overlapping with features on the same row
      const revFeatOff = {};
      for (const p of (enrichedPrimers || []).filter((p) => !p.isFwd)) {
        if (p.matchStart === undefined) continue;
        const ml = p.mismatchStr?.length || 0;
        const psegs = p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }];
        revFeatOff[p.id] = {};
        psegs.forEach((m, mi) => {
          // 1nt buffer on the 3' (arrow-tip) side: an abutting feature still
          // counts as overlapping, so the primer drops below its track
          const vs = mi === 0 ? m.start - 1 : m.start,
            ve = mi === psegs.length - 1 ? m.end + ml : m.end;
          for (const f of resultFeatures) {
            for (const fseg of f.segments) {
              const isFRev = f.strand === '-';
              const hangsBelow = featureLabelsBelow;
              const labelCols = hangsBelow
                ? Math.ceil(
                    primerLabelW(
                      isFRev ? `< ${f.name}` : f.strand === '+' ? `${f.name} >` : f.name,
                    ) / cw,
                  )
                : Math.ceil(primerLabelW(f.name) / cw) + 2 + (f.strand && f.strand !== '.' ? 2 : 0);
              const fvs = hangsBelow
                ? isFRev
                  ? Math.min(fseg.start, fseg.end - labelCols)
                  : fseg.start
                : isFRev
                  ? fseg.start
                  : fseg.start - labelCols;
              const fve = hangsBelow
                ? isFRev
                  ? fseg.end
                  : Math.max(fseg.end, fseg.start + labelCols)
                : isFRev
                  ? fseg.end + labelCols
                  : fseg.end;
              if (fve < vs || fvs > ve) continue;
              const sr = rowOf(fseg.start);
              const er = rowOf(fseg.end);
              for (let r = sr; r <= er; r++) {
                const ft = (fRowTracks[f.id] || {})[r] || 0;
                // below-line labels hang one extra track lower, clear them too
                const tracksToClear = ft + (featureLabelsBelow ? 2 : 1);
                revFeatOff[p.id][r] = Math.max(
                  revFeatOff[p.id][r] || 0,
                  tracksToClear * lp.featTrackHeight,
                );
              }
            }
          }
        });
      }

      return {
        processedFeatures: resultFeatures,
        primerTracks: pTracks,
        featureRowTracks: fRowTracks,
        revPrimerFeatOffsets: revFeatOff,
      };
    }, [features, enrichedPrimers, numRows, rowOf, rowStarts, rowCounts, lp, featureLabelsBelow]);

  // --- adaptive row spacing (memoized with pre-indexed lookups) ---
  const enzymesByRow = useMemo(() => {
    const map = {};
    for (const e of enzymes) {
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      const rows = new Set();
      for (const cp of pairs) {
        rows.add(rowOf(cp.topCutIndex));
      }
      for (const r of rows) {
        (map[r] || (map[r] = [])).push(e);
      }
    }
    return map;
  }, [enzymes, rowOf]);

  const primersByRow = useMemo(() => {
    const map = {};
    for (const p of enrichedPrimers || []) {
      if (p.matchStart === undefined || p.matchEnd === undefined) continue;
      // Only include rows that actually render primer segments (the match range).
      // Tail characters beyond the match segment's row are truncated by rendering.
      for (const m of p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }]) {
        const sr = rowOf(m.start);
        const er = rowOf(m.end);
        for (let r = sr; r <= er; r++) {
          if (rowCounts[r] === 0) continue;
          if (!map[r]) map[r] = [];
          if (!map[r].includes(p)) map[r].push(p);
        }
      }
    }
    return map;
  }, [enrichedPrimers, rowOf, rowCounts]);

  const featuresByRow = useMemo(() => {
    const map = {};
    for (const f of processedFeatures) {
      for (const seg of f.segments) {
        const sr = rowOf(seg.start);
        const er = rowOf(seg.end);
        for (let r = sr; r <= er; r++) {
          if (rowCounts[r] === 0) continue;
          (map[r] || (map[r] = [])).push({ feature: f, seg });
        }
      }
    }
    return map;
  }, [processedFeatures, rowOf, rowCounts]);

  // Pre-compute fwd primer occupied x-ranges per row so enzyme track assignment
  // can lift labels clear of a primer. Two tiers, both reserved up-front so
  // expanding a primer never shifts other elements:
  //  - label zone (label + 5' tail): a selected primer lifts its label ~16px,
  //    top ≈ 69px above the sequence.
  //  - body zone: only the expanded block reaches here, top ≈ 56px.
  //  The lift formula adds the clearance gap on top of these. Expanded content
  //  also paints above enzyme labels as a backstop. The label renders once per
  //  segment (multi-row primers repeat it), so occupancy covers every segment.
  const primerLabelOcc = useMemo(() => {
    const occ = {}; // { [row]: [{x1, x2, topOffset}] }
    for (const [rowStr, primers] of Object.entries(primersByRow)) {
      const row = parseInt(rowStr, 10);
      const entries = [];
      for (const p of primers) {
        if (!p.isFwd) continue;
        const segs = (p.matchSegs || [{ start: p.matchStart, end: p.matchEnd }]).flatMap((m) =>
          sp(m.start, m.end),
        );
        const ml = p.mismatchStr?.length || 0;
        const pt = (primerTracks[p.id] || {})[row] || 0;
        const nameW = primerLabelW(p.name);
        for (const seg of segs) {
          if (seg.row !== row) continue;
          const drawMisLen = seg === segs[0] ? Math.min(ml, seg.colStart + 5) : 0;
          const bodyX1 = getX(colVis(seg.colStart, row));
          const bodyX2 = getX(colVis(seg.colEnd, row)) + cw;
          const labelX = bodyX1 - drawMisLen * cw;
          const off = pt * pp.trackGap;
          entries.push({
            x1: labelX,
            x2: Math.max(labelX + nameW, bodyX1),
            topOffset: 69 + off,
          });
          entries.push({
            x1: bodyX1,
            x2: bodyX2 + pp.arrowHeadLen,
            topOffset: 56 + off,
          });
        }
      }
      if (entries.length) occ[row] = entries;
    }
    return occ;
  }, [primersByRow, primerTracks, sp, pp.trackGap, pp.arrowHeadLen, colVis]);

  const { rowAbove, rowBelow, enzymeRowTracks } = useMemo(() => {
    // Per-row enzyme track assignment — cut-twice enzymes are expanded per pair
    const eTracks = {};
    for (let r = 0; r < numRows; r++) {
      const rEnz = enzymesByRow[r];
      if (!rEnz || !rEnz.length) continue;
      // Expand cut-twice enzymes into per-pair entries for independent track assignment
      const expanded = [];
      for (const e of rEnz) {
        const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
        pairs.forEach((cp, pi) => {
          if (rowOf(cp.topCutIndex) === r) {
            expanded.push({
              key: pairs.length > 1 ? `${e.id}_p${pi}` : e.id,
              cutIndex: cp.topCutIndex,
              name: e.name,
              isUnique: e.isUnique,
            });
          }
        });
      }
      const sorted = expanded.sort(
        (a, b) => b.cutIndex - a.cutIndex || b.name.length - a.name.length,
      );
      const occupied = [];
      for (const item of sorted) {
        const cs = item.cutIndex - rowStarts[r];
        const ce = cs + Math.ceil(enzLabelW(item.name, item.isUnique) / cw);
        // Lift labels whose x-range overlaps a fwd primer label/body. The
        // enzyme text baseline sits (enzLabelBase-5)+lift above the sequence;
        // lift is continuous — just enough to clear the occupancy top by 6px.
        let avoidOff = 0;
        const occ = primerLabelOcc[r];
        if (occ) {
          const cutX = getX(colVis(cs, r));
          const enzW = enzLabelW(item.name, item.isUnique);
          for (const o of occ) {
            if (cutX < o.x2 + 4 && cutX + enzW > o.x1) {
              avoidOff = Math.max(avoidOff, o.topOffset);
            }
          }
        }
        let lift = avoidOff > 0 ? avoidOff + 6 - (lp.enzLabelBase - 5) : 0;
        // Enzyme-vs-enzyme: keep a full track of vertical separation between
        // x-overlapping labels, stacked on actual lifts.
        for (;;) {
          let bump = 0;
          for (const o of occupied) {
            if (Math.abs(o.lift - lift) < lp.enzTrackHeight && !(ce < o.cs || cs > o.ce)) {
              bump = Math.max(bump, o.lift + lp.enzTrackHeight);
            }
          }
          if (!bump) break;
          lift = Math.max(lift, bump);
        }
        occupied.push({ lift, cs, ce });
        if (!eTracks[item.key]) eTracks[item.key] = {};
        eTracks[item.key][r] = lift;
      }
    }

    const above = new Array(numRows).fill(0);
    const below = new Array(numRows).fill(0);

    for (let r = 0; r < numRows; r++) {
      let ae = lp.minAboveSpace,
        be = lp.minBelowSpace;

      const rowPrimers = primersByRow[r];
      let maxFwdPrimerH = 0;

      if (rowPrimers) {
        for (const p of rowPrimers) {
          const t = (primerTracks[p.id] || {})[r] || 0;
          if (p.isFwd) {
            const hasTail = r === rowOf(p.matchStart);
            const extra = hasTail ? pp.fwdAboveExtra : pp.fwdAboveNonTailExtra;
            const h = pp.fwdAboveBase + t * pp.trackGap + extra;
            maxFwdPrimerH = Math.max(maxFwdPrimerH, h);
            ae = Math.max(ae, h);
          } else {
            const hasTail = r === rowOf(p.matchEnd);
            const extra = hasTail ? pp.revBelowExtra : pp.revBelowNonTailExtra;
            const featOff = (revPrimerFeatOffsets[p.id] || {})[r] || 0;
            be = Math.max(
              be,
              pp.revBelowBase +
                t * pp.trackGap +
                extra +
                featOff +
                alignLaneInfo.chromBelow[r] +
                alignLaneInfo.counts[r] * lp.featTrackHeight +
                (alignLaneInfo.counts[r] > 0 ? ALIGN_FEAT_GAP : 0),
            );
          }
        }
      }

      // Enzymes: above fwd primers; per-row lifts from eTracks
      const rowEnz = enzymesByRow[r];
      if (rowEnz && rowEnz.length > 0) {
        let maxEnzLift = 0;
        for (const e of rowEnz) {
          const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
          pairs.forEach((cp, pi) => {
            if (rowOf(cp.topCutIndex) !== r) return;
            const key = pairs.length > 1 ? `${e.id}_p${pi}` : e.id;
            const lift = (eTracks[key] || {})[r] || 0;
            maxEnzLift = Math.max(maxEnzLift, lift);
          });
        }
        const enzDefaultAbove = lp.enzLabelBase + lp.enzAbovePad;
        const enzBase =
          maxFwdPrimerH > 0 ? Math.max(enzDefaultAbove, maxFwdPrimerH + 38) : enzDefaultAbove;
        ae = Math.max(ae, enzBase + maxEnzLift);
      }

      // Features: below sequence (shifted down by alignment lanes and any
      // chromatogram bands)
      const nAlign = alignLaneInfo.counts[r];
      const chromBelow = alignLaneInfo.chromBelow[r];
      if (nAlign > 0 || chromBelow > 0) {
        be = Math.max(
          be,
          lp.featBaseOffset +
            chromBelow +
            nAlign * lp.featTrackHeight +
            (nAlign > 0 ? ALIGN_FEAT_GAP : 0) +
            lp.featLabelPad,
        );
      }
      const rowFeats = featuresByRow[r];
      if (rowFeats) {
        for (const { feature: f } of rowFeats) {
          const t = (featureRowTracks[f.id] || {})[r] || 0;
          be = Math.max(
            be,
            lp.featBaseOffset +
              chromBelow +
              (t + nAlign) * lp.featTrackHeight +
              (nAlign > 0 ? ALIGN_FEAT_GAP : 0) +
              lp.featLabelPad +
              (featureLabelsBelow ? lp.featLabelBelowExtra : 0),
          );
        }
      }

      above[r] = ae;
      below[r] = be;
    }

    return { rowAbove: above, rowBelow: below, enzymeRowTracks: eTracks };
  }, [
    numRows,
    enzymesByRow,
    primersByRow,
    featuresByRow,
    primerTracks,
    featureRowTracks,
    revPrimerFeatOffsets,
    primerLabelOcc,
    rowOf,
    rowStarts,
    pp,
    lp,
    alignLaneInfo,
    featureLabelsBelow,
  ]);

  const rowY = useMemo(() => {
    const y = [Math.max(baseSeqY, rowAbove[0] + lp.minRowGap)];
    for (let r = 0; r < numRows - 1; r++) {
      y.push(y[r] + Math.max(lp.minRowGap, rowBelow[r] + rowAbove[r + 1] + lp.rowContentGap));
    }
    return y;
  }, [numRows, rowAbove, rowBelow, lp.minRowGap, lp.rowContentGap]);

  // Visible row range for enzyme virtualization.
  // Hold a stable object identity while start/end are unchanged: downstream
  // memos (visibleEnzymes/visibleFeatures/renderedSeqBg/...) key on this
  // object, and scrollY churns every frame — a fresh object would recompute
  // all of them even when the visible range didn't actually move.
  const visibleRowsRef = useRef({ start: 0, end: 0 });
  const visibleRows = useMemo(() => {
    let start = 0,
      end = numRows - 1;
    if (rowY.length) {
      const vh = viewportH || 900;
      const top = scrollY;
      const bot = top + vh;
      for (let r = 0; r < rowY.length; r++) {
        if (rowY[r] + (rowBelow[r] || 0) > top) {
          start = Math.max(0, r);
          break;
        }
      }
      for (let r = rowY.length - 1; r >= 0; r--) {
        if (rowY[r] - (rowAbove[r] || 0) < bot) {
          end = Math.min(numRows - 1, r + 1);
          break;
        }
      }
    }
    const prev = visibleRowsRef.current;
    if (prev.start === start && prev.end === end) return prev;
    visibleRowsRef.current = { start, end };
    return visibleRowsRef.current;
  }, [rowY, rowAbove, rowBelow, scrollY, viewportH, numRows]);

  // Virtualize features: only render those overlapping visible rows
  const visibleFeatures = useMemo(() => {
    if (!processedFeatures.length) return [];
    const vs = Math.max(0, visibleRows.start - ROW_BUF);
    const ve = Math.min(numRows - 1, visibleRows.end + ROW_BUF);
    return processedFeatures.filter((f) =>
      f.segments.some((seg) => {
        const sr = rowOf(seg.start);
        const er = rowOf(seg.end);
        return !(er < vs || sr > ve);
      }),
    );
  }, [processedFeatures, visibleRows, numRows, rowOf]);

  // Solid segment coverage per `${row}:${track}` across all features. A
  // segmented feature's gap draws a faint connector line at its track,
  // defaulting to track 0 in rows where it has no track reservation — there
  // the phantom line must give way wherever another feature solidly occupies
  // the same track, instead of crossing its bar and translation glyphs.
  const solidLineCols = useMemo(() => {
    const m = new Map();
    for (const g of visibleFeatures) {
      for (const seg of g.segments) {
        for (const vs of sp(seg.start, seg.end)) {
          const track = (featureRowTracks[g.id] || {})[vs.row] || 0;
          const key = `${vs.row}:${track}`;
          if (!m.has(key)) m.set(key, []);
          m.get(key).push({ colStart: vs.colStart, colEnd: vs.colEnd });
        }
      }
    }
    return m;
  }, [visibleFeatures, featureRowTracks, sp]);

  return {
    normFeatures,
    abutColors,
    processedFeatures,
    primerTracks,
    featureRowTracks,
    revPrimerFeatOffsets,
    enzymesByRow,
    primersByRow,
    featuresByRow,
    primerLabelOcc,
    rowAbove,
    rowBelow,
    enzymeRowTracks,
    rowY,
    visibleRows,
    visibleFeatures,
    solidLineCols,
  };
}
