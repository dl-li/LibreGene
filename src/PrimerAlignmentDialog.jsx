import React, { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Repeat } from 'lucide-react';
import { computePrimerAlignment } from './tauriApi';

const MONO = '"Cascadia Code", ui-monospace, monospace';
const COLORS = { bg: '#faf9f7', fwd: '#166534', rev: '#4A148C' };

/* Reverse complement (preserves case, supports IUPAC degenerate bases) */
const COMP_MAP = {
  'A': 'T', 'a': 't', 'T': 'A', 't': 'a',
  'C': 'G', 'c': 'g', 'G': 'C', 'g': 'c',
  'U': 'A', 'u': 'a',
  'R': 'Y', 'r': 'y', 'Y': 'R', 'y': 'r',
  'S': 'S', 's': 's',
  'W': 'W', 'w': 'w',
  'K': 'M', 'k': 'm', 'M': 'K', 'm': 'k',
  'B': 'V', 'b': 'v', 'V': 'B', 'v': 'b',
  'D': 'H', 'd': 'h', 'H': 'D', 'h': 'd',
  'N': 'N', 'n': 'n',
  '.': '.',
};
function reverseComplement(seq) {
  return [...seq].reverse().map(ch => COMP_MAP[ch] || ch).join('');
}

function AlignmentView({ data }) {
  if (!data?.alignment) return null;
  const lines = data.alignment.split('\n');
  // Line 4 (index 3) has the arrow indicators → detect rev from "3' <".
  const primerArrowLine = lines[3] || '';
  const isRev = primerArrowLine.includes("3' <");
  const primerColor = isRev ? COLORS.rev : COLORS.fwd;
  return (
    <div style={{ fontFamily: MONO, fontSize: '13px', lineHeight: '1.6', display: 'inline-block', textAlign: 'left' }}>
      {lines.map((line, i) => {
        let color;
        if (i === 3 || i === 4) {
          // Primer arrow line + name: primer colour
          color = primerColor;
        } else if (i === 2) {
          // Match line: gray
          color = '#888';
        }
        // All lines use the same font weight for visual alignment.
        return (
          <div key={i} style={{ whiteSpace: 'pre', color, fontWeight: 'bold' }}>{line}</div>
        );
      })}
    </div>
  );
}

export default function PrimerAlignmentDialog({ primer, alignmentData, open, onOpenChange, seedLength, newPrimerSeq, onPrimerChange, primers = [] }) {
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
        computePrimerAlignment(primer.id, seedLength).then(setData).catch(() => {});
      }
    }
  }, [open, primer?.id, isNewPrimer]); // eslint-disable-line react-hooks/exhaustive-deps

  const cur = preview?.data?.current || data?.current;
  const alts = preview?.data?.alternatives || data?.alternatives || [];

  const initialSeq = isNewPrimer ? (newPrimerSeq || '') : (primer?.primerSeq || '');
  const initialName = isNewPrimer ? 'New Primer' : (primer?.name || '');

  // For new primers: changes = has any input; for existing: changes = seq or name differs
  const hasChanges = isNewPrimer
    ? (editSeq !== '')
    : (editSeq !== initialSeq || editName !== initialName);

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
    return primers.some(p => {
      if (isNewPrimer) return p.name === trimmed;
      return p.id !== primer?.id && p.name === trimmed;
    });
  }, [editName, primers, isNewPrimer, primer?.id]);

  const stripIUPAC = useCallback((s) => {
    // Remove whitespace and keep only valid DNA/IUPAC chars (including degenerate)
    return [...s].filter(ch => ch !== ' ' && ch !== '\t' && IUPAC.has(ch)).join('');
  }, [IUPAC]);

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
    if (isInitialRender.current) { isInitialRender.current = false; return; }
    // Cancel previous debounce
    if (debounceRef.current) clearTimeout(debounceRef.current);
    const stripped = stripIUPAC(editSeq);
    if (stripped.length < 6) { setPreview({ data: null, loading: false, error: null }); return; }

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
    return () => { if (debounceRef.current) clearTimeout(debounceRef.current); };
  }, [editSeq, open, primer, seedLength, stripIUPAC, isNewPrimer, editName]);

  const handleApply = useCallback(async () => {
    if (!hasChanges) { onOpenChange(false); return; }
    if (!onPrimerChange) { onOpenChange(false); return; }
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
  }, [editSeq, editName, hasChanges, isNewPrimer, primer, onOpenChange, onPrimerChange, stripIUPAC, nameConflict, primerType]);

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
    <Dialog open={open} onOpenChange={(o) => { if (!o) handleClose(); }}>
      <DialogContent
        className="max-h-[80vh] flex flex-col"
        style={{ maxWidth: dialogWidth, width: dialogWidth }}
      >
        <DialogHeader>
          <DialogTitle className="text-base">Primer</DialogTitle>
        </DialogHeader>

        {/* Name row */}
        <div className="flex items-center gap-2 px-1 mb-3" style={{ minHeight: 28 }}>
          <span className="text-sm font-medium text-muted-foreground whitespace-nowrap">Name:</span>
          <input
            value={editName}
            onChange={(e) => setEditName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') setEditName(initialName);
            }}
            style={{
              fontWeight: 600, fontSize: 'inherit',
              border: 'none', borderBottom: '1px dashed #cbd5e1', outline: 'none',
              background: 'transparent', padding: '0 0 2px 0', minWidth: 80, flex: 1,
            }}
            onClick={(e) => e.stopPropagation()}
          />
        </div>

        {/* Sequence input */}
        <div className="px-1 mb-2">
          <div className="flex items-center justify-between mb-1">
            <span className="text-xs font-medium text-muted-foreground">Sequence</span>
          </div>
          <div className="flex items-center gap-0">
            <span className="text-xs font-bold text-muted-foreground mr-1.5">5'</span>
            <input
              value={editSeq}
              onChange={e => setEditSeq(e.target.value)}
              className="flex-1 h-8 px-2 py-1 text-xs font-mono border rounded"
              style={{ borderColor: hasChanges ? '#f59e0b' : '#d1d5db' }}
              spellCheck={false}
              placeholder="Enter primer sequence…"
            />
            <span className="text-xs font-bold text-muted-foreground ml-1.5">3'</span>
            <button
              type="button"
              onClick={() => {
                setEditSeq(reverseComplement(editSeq));
                setPrimerType(t => t === 'fwd' ? 'rev' : 'fwd');
              }}
              className="ml-2 p-1.5 rounded hover:bg-muted transition-colors"
              style={{ color: '#666', lineHeight: 0 }}
              title="Reverse complement"
            >
              <Repeat size={16} />
            </button>
          </div>
        </div>

        {/* Tm display */}
        <div className="px-1 mb-2 flex items-center gap-2 text-sm">
          {cur && (
            <span className="font-semibold mr-auto">
              Tm = <span style={{ color: COLORS.fwd }}>{cur.tm}°C</span>
              {preview?.loading && <span className="text-xs text-muted-foreground ml-2 italic">Computing…</span>}
            </span>
          )}
        </div>

        {/* Alignment box */}
        <div className="flex-1 overflow-auto rounded border p-4" style={{ backgroundColor: COLORS.bg, position: 'relative' }}>
          {preview?.error && <div className="text-sm text-red-600 font-mono">{preview.error}</div>}
          {cur && <div className="flex justify-center"><AlignmentView data={cur} /></div>}
          {!cur && preview?.loading && <div className="text-sm text-muted-foreground italic">Computing alignment…</div>}
          {!cur && !preview?.loading && preview?.data && !preview.data.current && (
            <div className="text-sm text-red-600 font-mono">No candidate binding sites found.</div>
          )}
          {!cur && !preview && !data && (
            <div className="text-sm text-muted-foreground italic">Type a DNA sequence to see alignment preview.</div>
          )}
          {preview?.loading && cur && (
            <div className="absolute inset-0 flex items-center justify-center rounded"
              style={{ backgroundColor: 'rgba(250,249,247,0.6)' }}>
              <div className="text-xs text-muted-foreground italic">Updating…</div>
            </div>
          )}
        </div>

        {/* Bottom banner: other binding sites */}
        {cur && alts.length > 0 && (
          <div className="px-3 py-1.5 mt-2 rounded text-xs flex items-center flex-wrap gap-x-3 gap-y-1"
            style={{ backgroundColor: '#fef3c7', color: '#92400e', border: '1px solid #fde68a' }}>
            <span className="font-medium opacity-80">Other binding sites:</span>
            {alts.map((alt, i) => {
              const dir = alt.strand === -1 ? 'R' : 'F';
              return (
                <span key={i} className="inline-flex items-center gap-1">
                  <span className="font-semibold">{dir}</span>
                  <span className="opacity-70">{alt.start}..{alt.end}</span>
                </span>
              );
            })}
          </div>
        )}

        {/* Invalid character warning */}
        {isInvalid && (
          <div className="mx-1 mt-2 px-2 py-1 rounded text-[11px] flex items-center gap-1"
            style={{ backgroundColor: '#fef9c3', color: '#854d0e', border: '1px solid #fde047' }}>
            <span>⚠ Invalid character(s):</span>
            <span className="font-mono">{invalidChars.join(', ')}</span>
          </div>
        )}

        {/* Bottom buttons */}
        <DialogFooter className="px-1 mt-3">
          <div className="flex justify-end gap-2">
            <Button variant="outline" size="sm" onClick={handleClose}>
              Cancel
            </Button>
            {hasChanges && (
              <Button variant="outline" size="sm" onClick={() => {
                setEditSeq(isNewPrimer ? (newPrimerSeq || '') : (primer?.primerSeq || ''));
                setPreview(null);
              }}>
                Reset
              </Button>
            )}
            {nameConflict && (
              <div className="text-xs text-red-600 mr-auto" style={{ fontFamily: MONO }}>
                Name "{editName}" is already used by another primer
              </div>
            )}
            {hasChanges && (
              <Button size="sm" onClick={handleApply} disabled={editSeq.length < 6 || isInvalid || nameConflict}>
                {isNewPrimer ? 'Create Primer' : 'Apply'}
              </Button>
            )}
          </div>
        </DialogFooter>

        {/* Confirm discard dialog */}
        {confirmClose && (
          <div className="absolute inset-0 flex items-center justify-center z-50 rounded"
            style={{ backgroundColor: 'rgba(0,0,0,0.35)' }}>
            <div className="bg-white rounded-lg shadow-xl p-5 mx-4 max-w-sm"
              onClick={e => e.stopPropagation()}>
              <div className="text-sm font-semibold mb-2">Discard changes?</div>
              <div className="text-xs text-muted-foreground mb-4">
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
