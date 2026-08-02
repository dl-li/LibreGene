import { useState, useEffect, useRef, useCallback } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { LoaderCircle, Check } from 'lucide-react';
import { computeTm } from '../../tauriApi';
import { buildAmplifyGroups, buildOepcrGroups, buildMutagenesisGroups } from './candidates';

// Wallace-rule fallback when the backend Tm model is unavailable (HTTP mode)
const wallaceTm = (s) => {
  const at = (s.match(/[AT]/gi) || []).length;
  const gc = (s.match(/[GC]/gi) || []).length;
  return 2 * at + 4 * gc;
};

const DEFAULTS = {
  amplify: { targetTm: 60, name: 'Amplicon' },
  oepcr: { targetTm: 60, overlapLen: 20, name1: 'Fragment 1', name2: 'Fragment 2' },
  mutagenesis: { targetTm: 60, armLen: 20, mutSeq: '', siteName: 'Mutation' },
};

const FWD_COLOR = '#166534';
const REV_COLOR = '#4A148C';
const BAND_H = 3.5;
const HEAD_H = 6.5;
const HEAD_LEN = 9;

// Block arrow like the editor's primers: filled band + arrowhead at the 3' end;
// x5 = 5' end, x3 = 3' tip; tailLen portion from the 5' end drawn lighter
function PrimerArrow({ x5, x3, y, color, tailLen = 0 }) {
  const d = x3 > x5 ? 1 : -1;
  const xb = x3 - HEAD_LEN * d;
  const te = tailLen > 0 ? x5 + tailLen * d : x5;
  return (
    <g>
      {tailLen > 0 && (
        <rect
          x={Math.min(x5, te)}
          y={y - BAND_H}
          width={Math.abs(te - x5)}
          height={BAND_H * 2}
          fill={color}
          fillOpacity={0.3}
        />
      )}
      <path
        d={`M ${te} ${y - BAND_H} L ${xb} ${y - BAND_H} L ${xb} ${y - HEAD_H} L ${x3} ${y} L ${xb} ${y + HEAD_H} L ${xb} ${y + BAND_H} L ${te} ${y + BAND_H} Z`}
        fill={color}
      />
    </g>
  );
}

function PrimerLabel({ x, y, anchor, color, children }) {
  return (
    <text x={x} y={y} fontSize="9" fontWeight="600" fill={color} textAnchor={anchor}>
      {children}
    </text>
  );
}

function TemplateLines({ x1, x2, y }) {
  return (
    <g stroke="currentColor" strokeWidth="2">
      <line x1={x1} y1={y} x2={x2} y2={y} />
      <line x1={x1} y1={y + 8} x2={x2} y2={y + 8} />
    </g>
  );
}

function AmplifySchematic({ name }) {
  return (
    <svg viewBox="0 0 260 80" className="w-full">
      <PrimerLabel x={20} y={10} anchor="start" color={FWD_COLOR}>{`${name}-Fwd`}</PrimerLabel>
      <PrimerArrow x5={20} x3={96} y={21} color={FWD_COLOR} />
      <TemplateLines x1={20} x2={240} y={40} />
      <PrimerArrow x5={240} x3={164} y={60} color={REV_COLOR} />
      <PrimerLabel x={240} y={74} anchor="end" color={REV_COLOR}>{`${name}-Rev`}</PrimerLabel>
    </svg>
  );
}

function OepcrSchematic({ name1, name2 }) {
  return (
    <svg viewBox="0 0 260 84" className="w-full">
      <PrimerLabel x={24} y={10} anchor="start" color={FWD_COLOR}>{`${name1}-Fwd`}</PrimerLabel>
      <PrimerArrow x5={24} x3={78} y={21} color={FWD_COLOR} />
      <PrimerLabel x={104} y={10} anchor="start" color={FWD_COLOR}>{`${name2}-Fwd`}</PrimerLabel>
      <PrimerArrow x5={104} x3={170} y={21} color={FWD_COLOR} tailLen={22} />
      <TemplateLines x1={20} x2={240} y={40} />
      <line x1={127} y1={50} x2={133} y2={38} stroke="currentColor" strokeWidth="2" />
      <PrimerArrow x5={156} x3={96} y={62} color={REV_COLOR} tailLen={22} />
      <PrimerLabel x={96} y={76} anchor="start" color={REV_COLOR}>{`${name1}-Rev`}</PrimerLabel>
      <PrimerArrow x5={240} x3={184} y={62} color={REV_COLOR} />
      <PrimerLabel x={240} y={76} anchor="end" color={REV_COLOR}>{`${name2}-Rev`}</PrimerLabel>
    </svg>
  );
}

function MutagenesisSchematic({ name }) {
  return (
    <svg viewBox="0 0 260 84" className="w-full">
      <PrimerLabel x={64} y={10} anchor="start" color={FWD_COLOR}>{`${name}-Fwd`}</PrimerLabel>
      <PrimerArrow x5={64} x3={190} y={21} color={FWD_COLOR} tailLen={56} />
      <g stroke="currentColor" strokeWidth="2">
        <line x1={20} y1={40} x2={116} y2={40} />
        <line x1={20} y1={48} x2={116} y2={48} />
        <line x1={132} y1={40} x2={240} y2={40} />
        <line x1={132} y1={48} x2={240} y2={48} />
      </g>
      <g stroke="#b91c1c" strokeWidth="2">
        <line x1={119} y1={38} x2={129} y2={50} />
        <line x1={129} y1={38} x2={119} y2={50} />
      </g>
      <PrimerArrow x5={196} x3={70} y={62} color={REV_COLOR} tailLen={56} />
      <PrimerLabel x={196} y={76} anchor="end" color={REV_COLOR}>{`${name}-Rev`}</PrimerLabel>
    </svg>
  );
}

const inputCls =
  'h-8 w-full rounded-md border border-border bg-background px-2.5 text-sm outline-none transition-colors focus:border-teal-700/50 focus:ring-2 focus:ring-teal-700/15';

const numInputCls =
  'h-8 w-16 shrink-0 rounded-md border border-border bg-background px-2.5 text-sm outline-none transition-colors focus:border-teal-700/50 focus:ring-2 focus:ring-teal-700/15';

function Field({ label, className = '', children }) {
  return (
    <label className={`flex flex-col gap-1 ${className}`}>
      <span className="text-[11px] font-medium text-muted-foreground">{label}</span>
      {children}
    </label>
  );
}

function PrimerDesignDialogInner({
  open,
  onOpenChange,
  mode,
  segments,
  sequence,
  topology,
  tmParams,
  onPrimerChange,
}) {
  const [params, setParams] = useState(() => {
    const d = { ...DEFAULTS[mode] };
    if (mode === 'amplify' && segments?.[0]?.name) d.name = segments[0].name;
    if (mode === 'oepcr') {
      if (segments?.[0]?.name) d.name1 = segments[0].name;
      if (segments?.[1]?.name) d.name2 = segments[1].name;
    }
    return d;
  });
  const [groups, setGroups] = useState([]);
  const [selections, setSelections] = useState({});
  const [expanded, setExpanded] = useState({});
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const genRef = useRef(0);

  const tmOf = useCallback(
    async (s) => {
      try {
        return await computeTm(s, tmParams);
      } catch {
        return wallaceTm(s);
      }
    },
    [tmParams],
  );

  const setParam = useCallback((key, value) => {
    setParams((p) => ({ ...p, [key]: value }));
  }, []);

  useEffect(() => {
    if (!open || !mode || !segments || !sequence) return;
    const timer = setTimeout(async () => {
      const gen = ++genRef.current;
      setLoading(true);
      setError('');
      try {
        const common = { seq: sequence, targetTm: Number(params.targetTm) || 60, topology, tmOf };
        let result;
        if (mode === 'amplify') {
          result = await buildAmplifyGroups({
            ...common,
            seg: segments[0],
            name: (params.name?.trim() ?? '') || 'Amplicon',
          });
        } else if (mode === 'oepcr') {
          result = await buildOepcrGroups({
            ...common,
            seg1: segments[0],
            seg2: segments[1],
            name1: (params.name1?.trim() ?? '') || 'Fragment 1',
            name2: (params.name2?.trim() ?? '') || 'Fragment 2',
            overlapLen: Math.max(8, Number(params.overlapLen) || 20),
          });
        } else {
          result = await buildMutagenesisGroups({
            ...common,
            seg: segments[0],
            siteName: (params.siteName?.trim() ?? '') || 'Mutation',
            mutSeq: params.mutSeq,
            armLen: Math.max(8, Number(params.armLen) || 20),
          });
        }
        if (genRef.current !== gen) return;
        setGroups(result);
        setSelections((prev) => {
          const next = {};
          result.forEach((g, i) => {
            if (g.candidates.some((c) => c.seq === prev[i])) next[i] = prev[i];
          });
          return next;
        });
      } catch (e) {
        console.error('primer design error:', e);
        if (genRef.current === gen) {
          setGroups([]);
          setError(`Failed to generate candidates: ${e?.message || e}`);
        }
      } finally {
        if (genRef.current === gen) setLoading(false);
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [open, mode, segments, sequence, topology, params, tmOf]);

  const allSelected = groups.length > 0 && groups.every((_, i) => selections[i]);

  const handleConfirm = useCallback(async () => {
    if (!allSelected || busy) return;
    setBusy(true);
    try {
      for (let i = 0; i < groups.length; i++) {
        const g = groups[i];
        const c = g.candidates.find((cand) => cand.seq === selections[i]);
        if (!c) return;
        await onPrimerChange?.({
          id: `primer_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`,
          name: g.name,
          type: g.type,
          primerSeq: c.seq,
        });
      }
      onOpenChange(false);
    } finally {
      setBusy(false);
    }
  }, [allSelected, busy, groups, selections, onPrimerChange, onOpenChange]);

  const segLabel = !segments?.length
    ? ''
    : mode === 'oepcr' && segments.length > 1
      ? `${segments[0].start + 1}..${segments[0].end + 1} + ${segments[1].start + 1}..${segments[1].end + 1}`
      : `${segments[0].start + 1}..${segments[0].end + 1}`;

  const targetTmNum = Number(params.targetTm) || 60;

  const modeTitle =
    mode === 'amplify' ? 'Amplify Fragment' : mode === 'oepcr' ? 'OE-PCR' : 'PCR Mutagenesis';
  const modeDesc =
    mode === 'amplify'
      ? 'Design a primer pair flanking the selected fragment'
      : mode === 'oepcr'
        ? 'Overlap-extension PCR joining two fragments'
        : 'Introduce a mutation at the selected site';

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[85vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-4xl">
        <DialogHeader className="px-6 pt-5">
          <DialogTitle>{modeTitle}</DialogTitle>
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <span>{modeDesc}</span>
            {segLabel && (
              <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] text-foreground/80">
                {segLabel}
              </span>
            )}
          </div>
        </DialogHeader>

        <div className="flex flex-1 flex-col gap-4 overflow-y-auto px-6 py-4">
          <div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_280px]">
            <div className="grid grid-cols-2 content-start gap-x-3 gap-y-3">
              <Field label="Target Tm (°C)" className="col-span-2">
                <div className="flex h-8 min-w-0 items-center gap-3">
                  <input
                    type="range"
                    min={40}
                    max={80}
                    step={1}
                    className="min-w-0 flex-1 accent-teal-700"
                    value={Math.min(80, Math.max(40, Number(params.targetTm) || 60))}
                    onChange={(e) => setParam('targetTm', e.target.value)}
                  />
                  <input
                    type="number"
                    min={40}
                    max={80}
                    step={1}
                    className={numInputCls}
                    value={params.targetTm}
                    onChange={(e) => setParam('targetTm', e.target.value)}
                  />
                </div>
              </Field>
              {mode === 'amplify' && (
                <Field label="Amplicon name" className="col-span-2">
                  <input
                    className={inputCls}
                    value={params.name}
                    onChange={(e) => setParam('name', e.target.value)}
                  />
                </Field>
              )}
              {mode === 'oepcr' && (
                <>
                  <Field label="Amplicon 1 name">
                    <input
                      className={inputCls}
                      value={params.name1}
                      onChange={(e) => setParam('name1', e.target.value)}
                    />
                  </Field>
                  <Field label="Amplicon 2 name">
                    <input
                      className={inputCls}
                      value={params.name2}
                      onChange={(e) => setParam('name2', e.target.value)}
                    />
                  </Field>
                  <Field label="Overlap length (nt)">
                    <input
                      type="number"
                      className={inputCls}
                      value={params.overlapLen}
                      onChange={(e) => setParam('overlapLen', e.target.value)}
                    />
                  </Field>
                </>
              )}
              {mode === 'mutagenesis' && (
                <>
                  <Field label="Mutation sequence (empty = deletion)" className="col-span-2">
                    <input
                      className={`${inputCls} font-mono uppercase`}
                      value={params.mutSeq}
                      onChange={(e) =>
                        setParam('mutSeq', e.target.value.toUpperCase().replace(/[^ACGT]/g, ''))
                      }
                    />
                  </Field>
                  <Field label="5' homology arm length (nt)">
                    <input
                      type="number"
                      className={inputCls}
                      value={params.armLen}
                      onChange={(e) => setParam('armLen', e.target.value)}
                    />
                  </Field>
                  <Field label="Mutation site name">
                    <input
                      className={inputCls}
                      value={params.siteName}
                      onChange={(e) => setParam('siteName', e.target.value)}
                    />
                  </Field>
                </>
              )}
            </div>
            <div className="flex min-h-[120px] items-center justify-center rounded-lg border border-border/60 bg-muted/30 px-4 py-3 text-foreground/70">
              {mode === 'amplify' && (
                <AmplifySchematic name={(params.name?.trim() ?? '') || 'Amplicon'} />
              )}
              {mode === 'oepcr' && (
                <OepcrSchematic
                  name1={(params.name1?.trim() ?? '') || 'Fragment 1'}
                  name2={(params.name2?.trim() ?? '') || 'Fragment 2'}
                />
              )}
              {mode === 'mutagenesis' && (
                <MutagenesisSchematic name={(params.siteName?.trim() ?? '') || 'Mutation'} />
              )}
            </div>
          </div>

          {loading && (
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <LoaderCircle className="size-3.5 animate-spin" /> Computing candidates…
            </div>
          )}
          {error && <div className="text-xs text-destructive">{error}</div>}

          <div className="flex flex-col gap-3">
            {groups.map((g, gi) => {
              const chosen = g.candidates.find((c) => c.seq === selections[gi]);
              const isCollapsed = !!chosen && !expanded[gi];
              const pick = (seq) => {
                setSelections((prev) => ({ ...prev, [gi]: seq }));
                setExpanded((prev) => ({ ...prev, [gi]: false }));
              };
              return (
                <div key={g.name} className="overflow-hidden rounded-lg border border-border">
                  <div className="flex items-center justify-between border-b border-border/60 bg-muted/40 px-3 py-2">
                    <div className="flex items-center gap-2">
                      <span
                        className="size-2 rounded-full"
                        style={{ background: g.type === 'fwd' ? FWD_COLOR : REV_COLOR }}
                      />
                      <span className="font-mono text-sm font-medium">{g.name}</span>
                      {chosen && <Check className="size-3.5 text-teal-700" />}
                    </div>
                    <span className="text-[11px] text-muted-foreground">
                      {g.type === 'fwd' ? 'Forward' : 'Reverse'}
                    </span>
                  </div>
                  {isCollapsed ? (
                    <div className="flex items-center gap-3 px-3 py-2">
                      <span className="font-mono text-xs whitespace-nowrap">
                        {chosen.tailLen > 0 && (
                          <span className="text-muted-foreground/60">
                            {chosen.seq.slice(0, chosen.tailLen)}
                          </span>
                        )}
                        <span>{chosen.seq.slice(chosen.tailLen)}</span>
                      </span>
                      <span className="text-xs tabular-nums whitespace-nowrap text-muted-foreground">
                        {chosen.seq.length} nt
                      </span>
                      <span
                        className={`text-xs tabular-nums whitespace-nowrap ${Math.abs(chosen.tm - targetTmNum) <= 1 ? 'font-semibold text-teal-700' : 'text-muted-foreground'}`}
                      >
                        {chosen.tm.toFixed(1)} °C
                      </span>
                      <span
                        className={`text-xs tabular-nums whitespace-nowrap ${chosen.gc >= 40 && chosen.gc <= 60 ? 'font-semibold text-teal-700' : 'text-muted-foreground'}`}
                      >
                        GC {chosen.gc.toFixed(1)}%
                      </span>
                      <button
                        className="ml-auto text-xs font-medium text-teal-700 hover:underline"
                        onClick={() => setExpanded((prev) => ({ ...prev, [gi]: true }))}
                      >
                        Change
                      </button>
                    </div>
                  ) : (
                    <div className="overflow-x-auto px-2 pt-1.5 pb-2">
                      <table className="w-full table-auto border-collapse">
                        <thead>
                          <tr className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground/70">
                            <th className="w-6 px-1 pb-1"></th>
                            <th className="px-1.5 pb-1 text-left">Sequence (5'→3')</th>
                            <th className="px-1.5 pb-1 text-right">Len</th>
                            <th className="px-1.5 pb-1 text-right">Tm °C</th>
                            <th className="px-1.5 pb-1 text-right">GC%</th>
                          </tr>
                        </thead>
                        <tbody>
                          {g.candidates.map((c) => {
                            const selected = selections[gi] === c.seq;
                            const tmClose = Math.abs(c.tm - targetTmNum) <= 1;
                            const gcGood = c.gc >= 40 && c.gc <= 60;
                            return (
                              <tr
                                key={c.seq}
                                className={`cursor-pointer hover:bg-accent/60 ${selected ? 'bg-teal-50' : ''}`}
                                onClick={() => pick(c.seq)}
                              >
                                <td
                                  className={`px-1 py-1 align-middle ${selected ? 'rounded-l-md shadow-[inset_2px_0_0_#0f766e]' : ''}`}
                                >
                                  <input
                                    type="radio"
                                    name={`pd-group-${gi}`}
                                    className="size-3.5 accent-teal-700"
                                    checked={selected}
                                    onChange={() => pick(c.seq)}
                                  />
                                </td>
                                <td className="px-1.5 py-1 font-mono text-xs leading-5 whitespace-nowrap">
                                  {c.tailLen > 0 && (
                                    <span className="text-muted-foreground/60">
                                      {c.seq.slice(0, c.tailLen)}
                                    </span>
                                  )}
                                  <span>{c.seq.slice(c.tailLen)}</span>
                                </td>
                                <td className="px-1.5 py-1 text-right text-xs tabular-nums whitespace-nowrap text-muted-foreground">
                                  {c.seq.length}
                                </td>
                                <td
                                  className={`px-1.5 py-1 text-right text-xs tabular-nums whitespace-nowrap ${tmClose ? 'font-semibold text-teal-700' : 'text-muted-foreground'}`}
                                >
                                  {c.tm.toFixed(1)}
                                </td>
                                <td
                                  className={`px-1.5 py-1 text-right text-xs tabular-nums whitespace-nowrap ${gcGood ? 'font-semibold text-teal-700' : 'text-muted-foreground'} ${selected ? 'rounded-r-md' : ''}`}
                                >
                                  {c.gc.toFixed(1)}
                                </td>
                              </tr>
                            );
                          })}
                        </tbody>
                      </table>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>

        <div className="flex items-center justify-between border-t border-border/60 bg-muted/30 px-6 py-3">
          <span className="text-[11px] text-muted-foreground/70">
            Tm is computed on the 3' annealing portion only
          </span>
          <Button size="sm" onClick={handleConfirm} disabled={!allSelected || busy || loading}>
            {busy && <LoaderCircle className="size-4 animate-spin" />}
            Add {groups.length > 0 ? groups.length : ''} Primers
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

// Remount per mode so params state always matches the active mode's shape
export default function PrimerDesignDialog({ open, mode, ...rest }) {
  if (!open || !mode) return null;
  return <PrimerDesignDialogInner key={mode} open={open} mode={mode} {...rest} />;
}
