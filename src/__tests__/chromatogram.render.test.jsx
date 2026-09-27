// SSR regression tests for the chromatogram bands (rendered by the real
// SequenceEditor component). Trace rendering ported from GenePad
// (https://github.com/GenePad), provided by the GenePad team /
// https://github.com/Masterchiefm.
import { describe, it, expect } from 'vitest';
import React from 'react';
import { renderToString } from 'react-dom/server';
import SequenceEditor from '../SequenceEditor';

// Synthetic chromatogram: peak every 12 samples, four channels with
// distinguishable sine patterns and a spike at each base's peak.
// Peak deltas are deliberately non-uniform (like real Sanger files) so
// interpolated sample indices land on fractions — indexing with them must
// not produce NaN path coordinates.
function makeChrom(nBases) {
  const samples = nBases * 12 + 20;
  const peaks = Array.from({ length: nBases }, (_, i) => Math.round(i * 12 + 6 + (i % 3)));
  const wave = (phase) =>
    Array.from({ length: samples }, (_, i) => Math.round(120 + 100 * Math.sin(i / 5 + phase)));
  const chrom = {
    traceA: wave(0),
    traceC: wave(1.5),
    traceG: wave(3),
    traceT: wave(4.5),
    peakLocations: peaks,
  };
  for (let b = 0; b < nBases; b++) {
    const ch = ['traceA', 'traceC', 'traceG', 'traceT'][b % 4];
    chrom[ch][peaks[b]] = 1500 + (b % 7) * 50;
  }
  return chrom;
}

// Extract x-extent of every `M/L` coordinate pair in an SVG path d string.
function pathXExtent(d) {
  const nums = d.match(/-?\d+(?:\.\d+)?/g).map(Number);
  let minX = Infinity;
  let maxX = -Infinity;
  for (let i = 0; i + 1 < nums.length; i += 2) {
    minX = Math.min(minX, nums[i]);
    maxX = Math.max(maxX, nums[i]);
  }
  return { minX, maxX };
}

// All G-channel (stroke #808080) path data, either attribute order.
function extractChannelPaths(html) {
  return [
    ...[...html.matchAll(/<path[^>]*stroke="#808080"[^>]*d="([^"]+)"/g)].map((m) => m[1]),
    ...[...html.matchAll(/<path[^>]*d="([^"]+)"[^>]*stroke="#808080"/g)].map((m) => m[1]),
  ];
}

function renderEditor(props) {
  return renderToString(
    React.createElement(SequenceEditor, {
      sequence: '',
      features: [],
      enzymes: [],
      primers: [],
      ...props,
    }),
  );
}

describe('SequenceEditor chromatogram bands (SSR)', () => {
  const seq = 'ACGT'.repeat(75); // 300 bases, 5 rows at cpl=60
  const chrom = makeChrom(300);

  it('renders a full-width main band for every row', () => {
    const html = renderEditor({ sequence: seq, chromatogram: chrom });
    const all = extractChannelPaths(html);
    expect(all.length).toBeGreaterThanOrEqual(5);
    for (const d of all) {
      // A NaN coordinate makes the whole path invalid — the browser stops
      // drawing at the first one (the "only a dot renders" bug).
      expect(d).not.toContain('NaN');
      const { minX, maxX } = pathXExtent(d);
      // startX=220, cw=12 → full 60-col row spans 226..934
      expect(minX).toBeLessThanOrEqual(230);
      expect(maxX).toBeGreaterThanOrEqual(925);
    }
  });

  it('renders full-width alignment bands across the aligned span', () => {
    const aln = {
      id: 'aln-1',
      name: 'read1',
      length: 300,
      strand: '+',
      identity: 1,
      seq,
      segments: [{ start: 70, end: 369, chars: seq }],
      insertions: [],
    };
    const html = renderEditor({
      sequence: 'ACGT'.repeat(100), // 400-base template
      alignmentTracks: [aln],
      alignmentChromatograms: { 'aln-1': chrom },
    });
    const paths = extractChannelPaths(html);
    // 300 aligned columns → rows 1..6 of the 400-base template, ≥ 5 band rows
    expect(paths.length).toBeGreaterThanOrEqual(5);
    for (const d of paths) {
      expect(d).not.toContain('NaN');
    }
    const widest = paths
      .map(pathXExtent)
      .reduce((a, b) => (b.maxX - b.minX > a.maxX - a.minX ? b : a));
    expect(widest.maxX - widest.minX).toBeGreaterThanOrEqual(650);
  });

  it('expands insertions into slot columns with bases and trace anchors', () => {
    // 1bp insertion at column 10, 5bp at column 100 (row 1 at cpl 60), and a
    // 21bp junk tail at the alignment start. The lane layout must shrink the
    // template grid by the slot count and render every inserted base as a
    // full-width character; the trace must pass through the slot columns.
    const template = 'ACGT'.repeat(100); // 400 bases
    const aligned = template.substring(0, 300);
    const chars =
      aligned.substring(0, 10) + '-' + aligned.substring(11, 100) + '-----' + aligned.substring(100);
    const aln = {
      id: 'aln-2',
      name: 'read-ins',
      length: 321,
      strand: '+',
      identity: 1,
      seq: aligned.substring(0, 10) + 'A' + aligned.substring(11, 100) + 'CCCCC' + aligned.substring(100),
      segments: [{ start: 0, end: 299, chars }],
      insertions: [
        { pos: 0, bases: 'GGGGGGGGGGGGGGGGGGGGG' },
        { pos: 10, bases: 'A' },
        { pos: 100, bases: 'CCCCC' },
        // Trailing tail anchored past the last aligned column — rendered by
        // the dedicated tail path, not while walking segment chars.
        { pos: 300, bases: 'TTCCAAATTCAGAT' },
      ],
    };
    const html = renderEditor({
      sequence: template,
      alignmentTracks: [aln],
      alignmentChromatograms: { 'aln-2': chrom },
    });
    // Inserted bases render as red mono tspan characters in the slot columns
    // (tspan, not nested <text> — text cannot nest in SVG and browsers drop
    // the inner element entirely).
    const insChars = [
      ...[...html.matchAll(/<tspan[^>]*fill="#b91c1c"[^>]*>([^<]+)/g)].map((m) => m[1]),
    ];
    expect(insChars.join('')).toContain('CCCCC');
    expect(insChars).toContain('A');
    expect(insChars.join('')).toContain('GGGGGGGGGGGGGGGGGGGGG');
    // The template row mirrors each slot with red '-' placeholders so the
    // rows stay column-aligned (21 + 1 + 5 + 14 = 41 dashes across rows).
    const dashes = insChars.filter((c) => c === '-').length;
    expect(dashes).toBe(41);
    // The tail bases appear exactly in order.
    const joined = insChars.join('');
    expect(joined).toContain('TTCCAAATTCAGAT');
    // Trace bands still render and stay NaN-free through the slots.
    const paths = extractChannelPaths(html);
    expect(paths.length).toBeGreaterThanOrEqual(4);
    for (const d of paths) {
      expect(d).not.toContain('NaN');
    }
  });
});
