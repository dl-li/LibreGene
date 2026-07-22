import React, { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Repeat, Trash2, AlertTriangle, RotateCcw, Copy, Check } from 'lucide-react';
import { monoFont } from './editorConstants';
import { computePrimerAlignment } from './tauriApi';
const COLORS = { bg: '#faf9f7', fwd: '#166534', rev: '#4A148C' };

/* Reverse complement (preserves case, supports IUPAC degenerate bases) */
const COMP_MAP = {
  A: 'T',
  a: 't',
  T: 'A',
  t: 'a',
  C: 'G',
  c: 'g',
  G: 'C',
  g: 'c',
  U: 'A',
  u: 'a',
  R: 'Y',
  r: 'y',
  Y: 'R',
  y: 'r',
  S: 'S',
  s: 's',
  W: 'W',
  w: 'w',
  K: 'M',
  k: 'm',
  M: 'K',
  m: 'k',
  B: 'V',
  b: 'v',
  V: 'B',
  v: 'b',
  D: 'H',
  d: 'h',
  H: 'D',
  h: 'd',
  N: 'N',
  n: 'n',
  '.': '.',
};
function reverseComplement(seq) {
  return [...seq]
    .reverse()
    .map((ch) => COMP_MAP[ch] || ch)
    .join('');
}

function AlignmentView({ data }) {
  if (!data?.alignment) return null;
  const lines = data.alignment.split('\n');
  // Line 4 (index 3) has the arrow indicators → detect rev from "3' <".
  const primerArrowLine = lines[3] || '';
  const isRev = primerArrowLine.includes("3' <");
  const primerColor = isRev ? COLORS.rev : COLORS.fwd;

  // Forward: show primer on top, template on bottom (swap lines 1↔5, 2↔4)
  const displayLines = isRev ? lines : [lines[4], lines[3], lines[2], lines[1], lines[0]];

  return (
    <div
      style={{
        fontFamily: monoFont,
        fontSize: '13px',
        lineHeight: '1.6',
        display: 'inline-block',
        textAlign: 'left',
      }}
    >
      {displayLines.map((line, i) => {
        let color;
        if (i === 2) {
          // Match line: gray
          color = '#888';
        } else if (isRev ? i >= 3 : i <= 1) {
          // Primer lines: top two in forward mode, bottom two in reverse
          color = primerColor;
        }
        return (
          <div key={i} style={{ whiteSpace: 'pre', color, fontWeight: 'bold' }}>
            {line}
          </div>
        );
      })}
    </div>
  );
}

function CopyBtn({ text }) {
  const [copied, setCopied] = useState(false);
  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    });
  }, [text]);
  return (
    <button
      type="button"
      onClick={(e) => { e.stopPropagation(); handleCopy(); }}
      className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
      title="Copy"
    >
      {copied ? <Check className="size-3.5 text-emerald-500" /> : <Copy className="size-3.5" />}
    </button>
  );
}

const LABEL_CLS =
  'w-[76px] shrink-0 text-xs font-medium uppercase tracking-wide text-muted-foreground';

export default function PrimerAlignmentDialog({
  primer,
  alignmentData,
  open,
  onOpenChange,
  seedLength,
  newPrimerSeq,
  onPrimerChange,
  onDeletePrimer,
  primers = [],
}) {
  const [editSeq, setEditSeq] = useState('');
  const [editName, setEditName] = useState('');
  const [data, setData] = useState(null);
  const [preview, setPreview] = useState(null); // { data, loading, error }
  const [confirmClose, setConfirmClose] = useState(false);
  const [primerType, setPrimerType] = useState('fwd');

  const isNewPrimer = !primer && newPrimerSeq !== undefined;

  // Initialize when dialog opens
  useEffect(() => {
    if (!open) return;
    if (isNewPrimer) {
      setEditSeq(newPrimerSeq || '');
      setEditName('New Primer');
      setPrimerType('fwd');
      setData(null);
      // No alignment computation yet — auto-preview will handle it on type
      setPreview(null);
    } else if (primer) {
      const seq = primer.primerSeq || '';
      setEditSeq(seq);
      setEditName(primer.name || '');
      setPrimerType(primer.type || 'fwd');
      setPreview(null);
      if (alignmentData) {
        setData(alignmentData);
      } else {
        setData(null);
        computePrimerAlignment(primer.id, seedLength)
          .then(setData)
          .catch(() => {});
      }
    }
  }, [open, primer?.id, isNewPrimer]); // eslint-disable-line react-hooks/exhaustive-deps

  const cur = preview?.data?.current || data?.current;
  const alts = preview?.data?.alternatives || data?.alternatives || [];

  const initialSeq = isNewPrimer ? newPrimerSeq || '' : primer?.primerSeq || '';
  const initialName = isNewPrimer ? 'New Primer' : primer?.name || '';

  // For new primers: changes = has any input; for existing: changes = seq or name differs
  const hasChanges = isNewPrimer
    ? editSeq !== ''
    : editSeq !== initialSeq || editName !== initialName;

  const IUPAC = useMemo(() => new Set('ACGTURYSWKMBDHVNacgturyswkmbdhvn'), []);
  const invalidChars = useMemo(() => {
    const bad = [];
    for (const ch of editSeq) {
      if (ch === ' ' || ch === '\t') continue; // whitespace is ignorable
      if (!IUPAC.has(ch)) bad.push(ch);
    }
    return [...new Set(bad)]; // unique list
  }, [editSeq, IUPAC]);
  const isInvalid = invalidChars.length > 0;

  // Check if the edited name conflicts with any other existing primer
  const nameConflict = useMemo(() => {
    const trimmed = editName.trim();
    if (!trimmed) return false;
    return primers.some((p) => {
      if (isNewPrimer) return p.name === trimmed;
      return p.id !== primer?.id && p.name === trimmed;
    });
  }, [editName, primers, isNewPrimer, primer?.id]);

  const stripIUPAC = useCallback(
    (s) => {
      // Remove whitespace and keep only valid DNA/IUPAC chars (including degenerate)
      return [...s].filter((ch) => ch !== ' ' && ch !== '\t' && IUPAC.has(ch)).join('');
    },
    [IUPAC],
  );

  // ── Auto-preview on sequence change (debounced) ──
  const debounceRef = useRef(null);
  const autoPreviewKey = useRef(0); // tracks latest request to avoid stale results
  const isInitialRender = useRef(true); // skip the very first render for edit mode (data already loaded)
  useEffect(() => {
    if (!open) return;
    // For create mode with pre-filled sequence, allow immediate preview
    if (isNewPrimer && newPrimerSeq && newPrimerSeq.length >= 6) {
      isInitialRender.current = false;
    }
    if (isInitialRender.current) {
      isInitialRender.current = false;
      return;
    }
    // Cancel previous debounce
    if (debounceRef.current) clearTimeout(debounceRef.current);
    const stripped = stripIUPAC(editSeq);
    if (stripped.length < 6) {
      setPreview({ data: null, loading: false, error: null });
      return;
    }

    const key = ++autoPreviewKey.current;
    debounceRef.current = setTimeout(async () => {
      setPreview({ data: null, loading: true, error: null });
      try {
        const result = isNewPrimer
          ? await computePrimerAlignment(null, seedLength, editSeq, editName)
          : await computePrimerAlignment(primer.id, seedLength, editSeq);
        if (key === autoPreviewKey.current) {
          setPreview({ data: result, loading: false, error: null });
        }
      } catch (e) {
        if (key === autoPreviewKey.current) {
          setPreview({ data: null, loading: false, error: String(e) });
        }
      }
    }, 350);
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [editSeq, open, primer, seedLength, stripIUPAC, isNewPrimer, editName]);

  const handleApply = useCallback(async () => {
    if (!hasChanges) {
      onOpenChange(false);
      return;
    }
    if (!onPrimerChange) {
      onOpenChange(false);
      return;
    }
    // Reject duplicate names
    if (nameConflict) return;
    try {
      const primerData = isNewPrimer
        ? {
            id: `primer_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`,
            name: editName || 'New Primer',
            type: 'fwd',
            primerSeq: stripIUPAC(editSeq),
            color: '#166534',
          }
        : {
            id: primer.id,
            name: editName || primer.name,
            type: primerType,
            primerSeq: stripIUPAC(editSeq),
            color: '#166534',
          };
      await onPrimerChange(primerData);
      onOpenChange(false);
    } catch (e) {
      setPreview({ data: null, loading: false, error: String(e) });
    }
  }, [
    editSeq,
    editName,
    hasChanges,
    isNewPrimer,
    primer,
    onOpenChange,
    onPrimerChange,
    stripIUPAC,
    nameConflict,
    primerType,
  ]);

  const handleClose = useCallback(() => {
    if (hasChanges) {
      setConfirmClose(true);
    } else {
      onOpenChange(false);
    }
  }, [hasChanges, onOpenChange]);

  const handleConfirmDiscard = useCallback(() => {
    setConfirmClose(false);
    onOpenChange(false);
  }, [onOpenChange]);

  const dialogWidth = useMemo(() => {
    const maxVw = (typeof window !== 'undefined' ? window.innerWidth : 1400) * 0.92;
    if (cur?.alignment) {
      const longest = cur.alignment.split('\n').reduce((max, l) => Math.max(max, l.length), 0);
      return Math.max(420, Math.min(longest * 7.8 + 96, maxVw));
    }
    const estChars = 30 + (editSeq?.length || 20);
    return Math.max(420, Math.min(estChars * 7.8 + 96, maxVw));
  }, [cur?.alignment, editSeq?.length]);

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) handleClose();
      }}
    >
      <DialogContent
        className="max-h-[80vh] flex flex-col"
        style={{ maxWidth: dialogWidth, width: dialogWidth }}
      >
        <DialogHeader>
          <DialogTitle>Primer</DialogTitle>
        </DialogHeader>

        <div className="grid grid-cols-[76px_1fr] items-center gap-x-3 gap-y-3">
          {/* Name */}
          <span className={LABEL_CLS}>Name</span>
          <div className="flex items-center gap-1 min-w-0">
            <input
              value={editName}
              onChange={(e) => setEditName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') setEditName(initialName);
              }}
              className="min-w-0 flex-1 border-b border-dashed border-input bg-transparent py-0.5 text-sm font-semibold outline-none transition-colors focus:border-primary"
              onClick={(e) => e.stopPropagation()}
            />
            <CopyBtn text={editName} />
          </div>

          {/* Sequence */}
          <span className={LABEL_CLS}>Sequence</span>
          <div className="flex items-center gap-1.5 min-w-0">
            <span className="shrink-0 font-mono text-xs font-bold text-muted-foreground">5'</span>
            <input
              value={editSeq}
              onChange={(e) => setEditSeq(e.target.value)}
              className={
                'h-8 min-w-0 flex-1 rounded-md border bg-background px-2 font-mono text-xs outline-none transition-shadow focus:ring-[3px] ' +
                (hasChanges
                  ? 'border-amber-400 focus:border-amber-400 focus:ring-amber-400/30'
                  : 'border-input focus:border-ring focus:ring-ring/40')
              }
              spellCheck={false}
              placeholder="Enter primer sequence…"
            />
            <span className="shrink-0 font-mono text-xs font-bold text-muted-foreground">3'</span>
            <CopyBtn text={editSeq} />
            <button
              type="button"
              onClick={() => {
                setEditSeq(reverseComplement(editSeq));
                setPrimerType((t) => (t === 'fwd' ? 'rev' : 'fwd'));
              }}
              className="flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              title="Reverse complement"
            >
              <Repeat className="size-4" />
            </button>
          </div>
        </div>

        {/* Tm display */}
        {cur && (
          <div className="flex items-center gap-2">
            <span className="inline-flex items-center gap-1.5 rounded-md border border-emerald-200 bg-emerald-50 px-2 py-0.5 text-xs text-emerald-700">
              Tm
              <span className="font-semibold tabular-nums">{cur.tm}°C</span>
            </span>
            {preview?.loading && (
              <span className="text-xs text-muted-foreground italic">Computing…</span>
            )}
          </div>
        )}

        {/* Alignment box */}
        <div className="relative flex-1 overflow-auto rounded-lg border border-border/70 bg-muted/40 p-4">
          {preview?.error && <div className="text-sm text-red-600 font-mono">{preview.error}</div>}
          {cur && (
            <div className="flex justify-center">
              <AlignmentView data={cur} />
            </div>
          )}
          {!cur && preview?.loading && (
            <div className="text-sm text-muted-foreground italic">Computing alignment…</div>
          )}
          {!cur && !preview?.loading && preview?.data && !preview.data.current && (
            <div className="text-sm text-red-600 font-mono">No candidate binding sites found.</div>
          )}
          {!cur && !preview && !data && (
            <div className="text-sm text-muted-foreground italic">
              Type a DNA sequence to see alignment preview.
            </div>
          )}
          {preview?.loading && cur && (
            <div className="absolute inset-0 flex items-center justify-center rounded-lg bg-background/60 backdrop-blur-[1px]">
              <div className="text-xs text-muted-foreground italic">Updating…</div>
            </div>
          )}
        </div>

        {/* Bottom banner: other binding sites */}
        {cur && alts.length > 0 && (
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg border border-amber-200 bg-amber-50 px-3 py-1.5 text-xs text-amber-800">
            <span className="font-medium opacity-80">Other binding sites:</span>
            {alts.map((alt, i) => {
              const dir = alt.strand === -1 ? 'R' : 'F';
              return (
                <span key={i} className="inline-flex items-center gap-1">
                  <span className="font-semibold">{dir}</span>
                  <span className="opacity-70 tabular-nums">
                    {alt.start}..{alt.end}
                  </span>
                </span>
              );
            })}
          </div>
        )}

        {/* Invalid character warning */}
        {isInvalid && (
          <div className="flex items-center gap-1.5 rounded-lg border border-amber-200 bg-amber-50 px-3 py-1.5 text-[11px] text-amber-800">
            <AlertTriangle className="size-3.5 shrink-0" />
            <span>Invalid character(s):</span>
            <span className="font-mono">{invalidChars.join(', ')}</span>
          </div>
        )}

        {/* Name conflict */}
        {nameConflict && (
          <div className="rounded-lg border border-red-200 bg-red-50 px-3 py-1.5 text-xs text-red-700">
            Name "{editName}" is already used by another primer
          </div>
        )}

        {/* Bottom buttons */}
        <DialogFooter className="mt-1">
          <div className="flex w-full items-center gap-2">
            {!isNewPrimer && onDeletePrimer && (
              <Button
                variant="ghost"
                size="sm"
                className="mr-auto text-destructive hover:bg-destructive/10 hover:text-destructive"
                onClick={async () => {
                  await onDeletePrimer(primer.id);
                  onOpenChange(false);
                }}
              >
                <Trash2 className="size-3.5" />
                Delete
              </Button>
            )}
            <Button variant="outline" size="sm" onClick={handleClose}>
              Cancel
            </Button>
            {hasChanges && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  setEditSeq(isNewPrimer ? newPrimerSeq || '' : primer?.primerSeq || '');
                  setPreview(null);
                }}
              >
                <RotateCcw className="size-3.5" />
                Reset
              </Button>
            )}
            {hasChanges && (
              <Button
                size="sm"
                onClick={handleApply}
                disabled={editSeq.length < 6 || isInvalid || nameConflict}
              >
                {isNewPrimer ? 'Create Primer' : 'Apply'}
              </Button>
            )}
          </div>
        </DialogFooter>

        {/* Confirm discard dialog */}
        {confirmClose && (
          <div className="absolute inset-0 z-50 flex items-center justify-center rounded-xl bg-black/35 backdrop-blur-[2px]">
            <div
              className="mx-4 max-w-sm rounded-xl border bg-card p-5 shadow-2xl"
              onClick={(e) => e.stopPropagation()}
            >
              <div className="mb-1.5 flex items-center gap-2 text-sm font-semibold">
                <AlertTriangle className="size-4 text-amber-500" />
                Discard changes?
              </div>
              <div className="mb-4 text-xs text-muted-foreground">
                Primer sequence has been modified but not applied. Discard changes?
              </div>
              <div className="flex justify-end gap-2">
                <Button variant="outline" size="sm" onClick={() => setConfirmClose(false)}>
                  Cancel
                </Button>
                <Button variant="destructive" size="sm" onClick={handleConfirmDiscard}>
                  Discard
                </Button>
              </div>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
