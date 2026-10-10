import { useEffect, useMemo, useRef, useState } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle, InlineNotice } from './ui';
import {
  LADDER_PRESETS,
  DEFAULT_LADDER_ID,
  ENZYME_GROUP_OPTIONS,
  parseLadderText,
  groupEnzymeSites,
  enzymeGroupOptions,
  enzymeMatchesGroup,
  categorizeEnzymes,
  gelRange,
  mobility,
  defaultTimeFactor,
  bandSpread,
  simulateLane,
} from './gel';

const MAX_ENZYMES = 4;
const SAMPLE_LANES = 7; // uncut + up to 4 single digests + mix + empties
const GEL_TOP = 78;
const GEL_H = 560; // default gel height in viewBox units before measuring
const LANE_W = 72;
const LABEL_W = 64;
const PAD_R = 18;
const GEL_W = LABEL_W + (SAMPLE_LANES + 1) * LANE_W + PAD_R;

function readEditorEnzymeFilter() {
  try {
    const v = JSON.parse(localStorage.getItem('enzymeFilter'));
    if (v === 'myEnzymes' || ENZYME_GROUP_OPTIONS.some((o) => o.value === v)) return v;
  } catch {
    /* fall through */
  }
  return 'unique+twice';
}

function Section({ title, children }) {
  return (
    <section className="rounded-xl border border-border/60 bg-muted/30 p-3">
      <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        {title}
      </h3>
      {children}
    </section>
  );
}

const selectClass =
  'w-full rounded-md border border-border bg-card px-2 py-1.5 text-sm outline-none focus:ring-2 focus:ring-ring';

function EnzymeBadge({ g, active, disabled, onClick }) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      title={`${g.recSeq} — ${g.cuts.length} cut${g.cuts.length === 1 ? '' : 's'}${g.blocked ? ' (methylation blocked)' : ''}`}
      className={`inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs transition-colors ${
        active
          ? 'border-primary bg-primary text-primary-foreground'
          : disabled
            ? 'cursor-not-allowed border-border/50 text-muted-foreground/40'
            : 'border-border bg-card text-foreground/80 hover:bg-accent hover:text-foreground'
      }`}
    >
      {g.name}
      {g.blocked && <span className="size-1.5 rounded-full bg-amber-500" />}
    </button>
  );
}

function EnzymeCategory({ title, open, onToggle, list, selected, onToggleEnzyme }) {
  return (
    <div className="rounded-lg border border-border/50">
      <button
        type="button"
        onClick={onToggle}
        className="flex w-full items-center gap-1.5 px-2 py-1.5 text-left text-xs font-medium text-foreground/80 hover:bg-accent/60"
      >
        <span
          className={`inline-block transition-transform ${open ? 'rotate-90' : ''}`}
          aria-hidden="true"
        >
          ›
        </span>
        {title}
        <span className="ml-auto text-muted-foreground">{list.length}</span>
      </button>
      {open && (
        <div className="flex max-h-36 flex-wrap content-start gap-1 overflow-y-auto border-t border-border/40 p-1.5">
          {list.length === 0 && (
            <p className="px-1 py-0.5 text-xs text-muted-foreground">None for this plasmid</p>
          )}
          {list.map((g) => (
            <EnzymeBadge
              key={g.name}
              g={g}
              active={selected.includes(g.name)}
              disabled={!selected.includes(g.name) && selected.length >= MAX_ENZYMES}
              onClick={() => onToggleEnzyme(g.name)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export default function GelDialog({
  open,
  onOpenChange,
  sequence,
  fileName = '',
  enzymes,
  topology = 'circular',
  myEnzymes = [],
  dialogState,
}) {
  const [groupFilter, setGroupFilter] = useState(readEditorEnzymeFilter);
  const [selected, setSelected] = useState([]);
  const [openCats, setOpenCats] = useState({ single: true, double: false, other: false });
  const [efficiency, setEfficiency] = useState(90);
  const [ladderId, setLadderId] = useState(DEFAULT_LADDER_ID);
  const [ladderText, setLadderText] = useState(
    () => LADDER_PRESETS.find((l) => l.id === DEFAULT_LADDER_ID).text,
  );
  const [gelType, setGelType] = useState('agarose');
  const [agaroseConc, setAgaroseConc] = useState(1.0);
  const [pageConc, setPageConc] = useState(8);
  const [timeScale, setTimeScale] = useState(1);

  const gelBoxRef = useRef(null);
  const [gelBox, setGelBox] = useState(null);
  useEffect(() => {
    if (!open) return undefined;
    const el = gelBoxRef.current;
    if (!el) return undefined;
    const ro = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      if (width > 0 && height > 0) setGelBox({ width, height });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [open]);

  const length = sequence?.length ?? 0;
  const circular = topology === 'circular';

  const groups = useMemo(() => groupEnzymeSites(enzymes || []), [enzymes]);
  const byName = useMemo(() => new Map(groups.map((g) => [g.name, g])), [groups]);
  const groupOptions = useMemo(() => enzymeGroupOptions(myEnzymes), [myEnzymes]);
  const myEnzymeNames = useMemo(() => new Set(myEnzymes), [myEnzymes]);
  const cats = useMemo(
    () =>
      categorizeEnzymes(groups.filter((g) => enzymeMatchesGroup(g, groupFilter, myEnzymeNames))),
    [groups, groupFilter, myEnzymeNames],
  );

  // Preselect enzymes passed by the opener (editor selection / context menu).
  const presetAppliedRef = useRef(false);
  useEffect(() => {
    if (!open) {
      presetAppliedRef.current = false;
      return;
    }
    if (presetAppliedRef.current) return;
    presetAppliedRef.current = true;
    const names = dialogState?.enzymes;
    if (Array.isArray(names) && names.length)
      setSelected(names.filter((n) => byName.has(n)).slice(0, MAX_ENZYMES));
  }, [open, dialogState, byName]);

  useEffect(() => {
    setSelected((prev) => prev.filter((n) => byName.has(n)));
  }, [byName]);

  const ladder = useMemo(() => parseLadderText(ladderText), [ladderText]);
  const conc = gelType === 'page' ? pageConc : agaroseConc;
  const range = useMemo(() => gelRange(gelType, conc), [gelType, conc]);
  const t0 = useMemo(
    () => defaultTimeFactor(ladder.bands.length ? ladder.bands : [{ size: 1000 }], range),
    [ladder, range],
  );
  const t = t0 * timeScale;

  const lanes = useMemo(() => {
    const samples = [
      { key: 'uncut', label: 'uncut', kind: 'digest', names: [] },
      ...selected.map((n) => ({ key: `e:${n}`, label: n, kind: 'digest', names: [n] })),
    ];
    if (selected.length >= 2)
      samples.push({ key: 'mix', label: selected.join('+'), kind: 'digest', names: selected });
    while (samples.length < SAMPLE_LANES)
      samples.push({ key: `empty-${samples.length}`, label: '', kind: 'empty' });
    return [{ key: 'mw', label: 'MW', kind: 'ladder' }, ...samples.slice(0, SAMPLE_LANES)];
  }, [selected]);

  const laneBands = useMemo(() => {
    const map = new Map();
    for (const lane of lanes) {
      if (lane.kind === 'empty') {
        map.set(lane.key, []);
      } else if (lane.kind === 'ladder') {
        const bands = ladder.bands
          .map((b) => {
            const pos = t * mobility(b.size, range);
            if (pos > 1.02) return null;
            return {
              size: b.size,
              pos,
              spread: bandSpread(b.size, 'linear', pos, t),
              intensity: b.bright ? 1 : 0.55,
              topo: 'linear',
            };
          })
          .filter(Boolean);
        map.set(lane.key, bands);
      } else {
        const cuts = lane.names.flatMap((n) => byName.get(n)?.cuts || []);
        map.set(
          lane.key,
          simulateLane({
            length,
            circular,
            cuts,
            efficiency: efficiency / 100,
            range,
            timeFactor: t,
          }),
        );
      }
    }
    return map;
  }, [lanes, ladder, t, range, length, circular, byName, efficiency]);

  // Fill the container: keep the viewBox width fixed, stretch the gel
  // vertically so long dialogs give a long gel instead of dead space.
  const gelH = gelBox
    ? Math.max(240, gelBox.height / (gelBox.width / GEL_W) - GEL_TOP - 14)
    : GEL_H;
  const width = GEL_W;
  const height = GEL_TOP + gelH + 14;

  const ladderLabels = useMemo(() => {
    const out = [];
    let lastY = -Infinity;
    for (const b of ladder.bands) {
      const pos = t * mobility(b.size, range);
      if (pos > 1.0) continue;
      const y = GEL_TOP + pos * gelH;
      if (y - lastY < 16) continue;
      lastY = y;
      out.push({ size: b.size, y, bright: b.bright });
    }
    return out;
  }, [ladder, t, range, gelH]);

  const autoMinutes = gelType === 'page' ? 60 : 45;

  const toggleEnzyme = (name) =>
    setSelected((prev) =>
      prev.includes(name)
        ? prev.filter((n) => n !== name)
        : prev.length < MAX_ENZYMES
          ? [...prev, name]
          : prev,
    );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        style={{ width: 'min(85vw, 60rem)' }}
        className="flex h-[84vh] grid-cols-none gap-5 overflow-hidden"
      >
        <DialogHeader className="shrink-0">
          <DialogTitle>
            Digest and Run Gel
            {fileName ? (
              <span className="ml-2 font-normal text-muted-foreground">{fileName}</span>
            ) : null}
          </DialogTitle>
        </DialogHeader>
        <div className="flex min-h-0 flex-1 gap-4">
          <div className="min-w-0 flex-1 space-y-3 overflow-y-auto pr-1">
            <Section title="Enzymes">
              <div className="space-y-2">
                <select
                  className={selectClass}
                  value={groupFilter}
                  onChange={(e) => setGroupFilter(e.target.value)}
                  aria-label="Enzyme group"
                >
                  {groupOptions.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
                <div
                  className={`min-h-9 rounded-lg border p-1.5 ${
                    selected.length
                      ? 'flex flex-wrap gap-1 border-primary/40 bg-primary/5'
                      : 'flex items-center border-dashed border-border px-2 text-xs text-muted-foreground'
                  }`}
                >
                  {selected.length ? (
                    selected.map((name) => {
                      const g = byName.get(name);
                      return (
                        <button
                          key={name}
                          type="button"
                          onClick={() => toggleEnzyme(name)}
                          title={g ? `${g.recSeq} — click to remove` : 'Click to remove'}
                          className="inline-flex items-center gap-1 rounded-full bg-primary px-2 py-0.5 text-xs text-primary-foreground transition-colors hover:bg-primary/80"
                        >
                          {name}
                          <span aria-hidden="true">×</span>
                        </button>
                      );
                    })
                  ) : (
                    <span>No enzymes selected — pick from the badges below</span>
                  )}
                </div>
                <div className="space-y-1.5">
                  <EnzymeCategory
                    title="Single cutters"
                    open={openCats.single}
                    onToggle={() => setOpenCats((s) => ({ ...s, single: !s.single }))}
                    list={cats.single}
                    selected={selected}
                    onToggleEnzyme={toggleEnzyme}
                  />
                  <EnzymeCategory
                    title="Double cutters"
                    open={openCats.double}
                    onToggle={() => setOpenCats((s) => ({ ...s, double: !s.double }))}
                    list={cats.double}
                    selected={selected}
                    onToggleEnzyme={toggleEnzyme}
                  />
                  <EnzymeCategory
                    title="Other enzymes"
                    open={openCats.other}
                    onToggle={() => setOpenCats((s) => ({ ...s, other: !s.other }))}
                    list={cats.other}
                    selected={selected}
                    onToggleEnzyme={toggleEnzyme}
                  />
                </div>
                <div>
                  <div className="mb-1 flex justify-between text-xs text-muted-foreground">
                    <span>Cutting efficiency</span>
                    <span className="tabular-nums">{efficiency}%</span>
                  </div>
                  <input
                    type="range"
                    min={10}
                    max={100}
                    step={1}
                    value={efficiency}
                    onChange={(e) => setEfficiency(Number(e.target.value))}
                    className="w-full accent-primary"
                  />
                </div>
                <p className="text-xs text-muted-foreground">
                  {selected.length}/{MAX_ENZYMES} selected
                </p>
              </div>
            </Section>

            <Section title="Ladder">
              <div className="space-y-1.5">
                {LADDER_PRESETS.map((p) => (
                  <label
                    key={p.id}
                    className="flex cursor-pointer items-center gap-2 text-sm text-foreground/80"
                  >
                    <input
                      type="radio"
                      name="gel-ladder"
                      checked={ladderId === p.id}
                      onChange={() => {
                        setLadderId(p.id);
                        setLadderText(p.text);
                      }}
                      className="accent-primary"
                    />
                    {p.label}
                  </label>
                ))}
                <textarea
                  value={ladderText}
                  onChange={(e) => {
                    setLadderId('custom');
                    setLadderText(e.target.value);
                  }}
                  rows={3}
                  spellCheck={false}
                  className="mt-1 w-full resize-y rounded-md border border-border bg-card px-2 py-1.5 font-mono text-xs outline-none focus:ring-2 focus:ring-ring"
                  placeholder="100, 250, 500, *1000*, 2000"
                />
                {ladder.error && <InlineNotice tone="error">{ladder.error}</InlineNotice>}
                <p className="text-xs text-muted-foreground">
                  Sizes in bp; wrap a size in *asterisks* for the brightest band.
                </p>
              </div>
            </Section>

            <Section title="Gel">
              <div className="space-y-2.5">
                <div className="flex gap-1 rounded-lg bg-muted p-0.5">
                  {[
                    { value: 'agarose', label: 'Agarose' },
                    { value: 'page', label: 'PAGE' },
                  ].map((o) => (
                    <button
                      key={o.value}
                      type="button"
                      onClick={() => setGelType(o.value)}
                      className={`flex-1 rounded-md px-2 py-1 text-sm transition-colors ${
                        gelType === o.value
                          ? 'bg-card font-medium shadow-sm'
                          : 'text-muted-foreground hover:text-foreground'
                      }`}
                    >
                      {o.label}
                    </button>
                  ))}
                </div>
                <div>
                  <div className="mb-1 flex justify-between text-xs text-muted-foreground">
                    <span>Concentration</span>
                    <span className="tabular-nums">{conc.toFixed(1)}%</span>
                  </div>
                  {gelType === 'agarose' ? (
                    <input
                      type="range"
                      min={0.5}
                      max={3}
                      step={0.1}
                      value={agaroseConc}
                      onChange={(e) => setAgaroseConc(Number(e.target.value))}
                      className="w-full accent-primary"
                    />
                  ) : (
                    <input
                      type="range"
                      min={4}
                      max={20}
                      step={0.5}
                      value={pageConc}
                      onChange={(e) => setPageConc(Number(e.target.value))}
                      className="w-full accent-primary"
                    />
                  )}
                </div>
                <div>
                  <div className="mb-1 flex justify-between text-xs text-muted-foreground">
                    <span>Run time</span>
                    <span className="tabular-nums">~{Math.round(autoMinutes * timeScale)} min</span>
                  </div>
                  <input
                    type="range"
                    min={0.3}
                    max={3}
                    step={0.05}
                    value={timeScale}
                    onChange={(e) => setTimeScale(Number(e.target.value))}
                    className="w-full accent-primary"
                  />
                </div>
              </div>
            </Section>
          </div>

          <div
            ref={gelBoxRef}
            className="min-w-0 flex-1 overflow-hidden rounded-xl border border-border/60 bg-[#141414]"
          >
            {length === 0 ? (
              <p className="p-6 text-sm text-muted-foreground">No sequence loaded.</p>
            ) : (
              <svg
                viewBox={`0 0 ${width} ${height}`}
                preserveAspectRatio="xMidYMin meet"
                className="h-full w-full"
                role="img"
                aria-label="Simulated gel"
              >
                <defs>
                  {[
                    ['a', 0.5],
                    ['b', 1.4],
                    ['c', 2.6],
                    ['d', 4.2],
                  ].map(([k, v]) => (
                    <filter
                      key={k}
                      id={`gel-blur-${k}`}
                      x="-40%"
                      y="-200%"
                      width="180%"
                      height="500%"
                    >
                      <feGaussianBlur stdDeviation={`${(v * 0.55).toFixed(2)} ${v}`} />
                    </filter>
                  ))}
                </defs>
                {lanes.map((lane, i) => {
                  const cx = LABEL_W + i * LANE_W + LANE_W / 2;
                  const isEmpty = lane.kind === 'empty';
                  const nameLines =
                    lane.kind === 'digest' ? (lane.names.length ? lane.names : ['uncut']) : [];
                  return (
                    <g key={lane.key}>
                      {lane.kind === 'ladder' ? (
                        <text x={cx} y={22} fontSize={14} textAnchor="middle" fill="#a3a3a3">
                          Ladder
                        </text>
                      ) : (
                        <>
                          <text
                            x={cx}
                            y={16}
                            fontSize={14}
                            fontWeight={600}
                            textAnchor="middle"
                            fill={isEmpty ? '#525252' : '#e5e5e5'}
                          >
                            {i}
                          </text>
                          {nameLines.map((n, li) => (
                            <text
                              key={li}
                              x={cx}
                              y={34 + li * 12}
                              fontSize={11}
                              textAnchor="middle"
                              fill="#a3a3a3"
                            >
                              {n.length > 11 ? `${n.slice(0, 10)}…` : n}
                              <title>{n}</title>
                            </text>
                          ))}
                        </>
                      )}
                      <rect
                        x={cx - LANE_W * 0.32}
                        y={GEL_TOP - 9}
                        width={LANE_W * 0.64}
                        height={9}
                        rx={4.5}
                        fill="#0a0a0a"
                        stroke={isEmpty ? '#2a2a2a' : '#3f3f3f'}
                        strokeWidth={1}
                      />
                      {(laneBands.get(lane.key) || []).map((b, j) => {
                        const h = Math.min(9, Math.max(2, 2 * b.spread * gelH));
                        const y = GEL_TOP + b.pos * gelH - h / 2;
                        // Diffusion blur grows with migration distance:
                        // top-of-gel bands get a bare soft edge, bottom bands smear.
                        const s = b.spread * gelH;
                        const blurId =
                          s < 4
                            ? 'gel-blur-a'
                            : s < 7
                              ? 'gel-blur-b'
                              : s < 10
                                ? 'gel-blur-c'
                                : 'gel-blur-d';
                        return (
                          <rect
                            key={j}
                            x={cx - LANE_W * 0.3}
                            y={y}
                            width={LANE_W * 0.6}
                            height={h}
                            rx={Math.min(2.5, h / 2)}
                            fill="#f5f5f5"
                            opacity={b.intensity}
                            filter={`url(#${blurId})`}
                          >
                            <title>
                              {`${b.size} bp${b.topo === 'supercoiled' ? ' (supercoiled)' : b.topo === 'nicked' ? ' (nicked circle)' : ''}`}
                            </title>
                          </rect>
                        );
                      })}
                    </g>
                  );
                })}
                {ladderLabels.map((l) => (
                  <g key={l.size}>
                    <text
                      x={LABEL_W - 8}
                      y={l.y + 4}
                      fontSize={13}
                      textAnchor="end"
                      fill={l.bright ? '#f5f5f5' : '#a3a3a3'}
                      fontWeight={l.bright ? 700 : 400}
                      className="tabular-nums"
                    >
                      {l.size}
                    </text>
                    <line
                      x1={LABEL_W - 5}
                      y1={l.y}
                      x2={LABEL_W}
                      y2={l.y}
                      stroke="#525252"
                      strokeWidth={1}
                    />
                  </g>
                ))}
              </svg>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
