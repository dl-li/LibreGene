import React, { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { computePrimerAlignment, addPrimer } from './tauriApi';

const MONO = '"Cascadia Code", ui-monospace, monospace';
const COLORS = { bg: '#faf9f7', fwd: '#166534', rev: '#4A148C' };

function AlignmentView({ data }) {
  if (!data?.alignment) return null;
  const lines = data.alignment.split('\n');
  const primerLine = lines[2] || '';
  const isRev = primerLine.match(/\([53]'\)/)?.[0] === "(3')";
  const color = isRev ? COLORS.rev : COLORS.fwd;
  return (
    <div style={{ fontFamily: MONO, fontSize: '13px', lineHeight: '1.6' }}>
      {lines.map((line, i) => (
        <div key={i} style={{ whiteSpace: 'pre', color: i === 2 ? color : undefined }}>{line}</div>
      ))}
    </div>
  );
}

export default function PrimerAlignmentDialog({ primer, alignmentData, open, onOpenChange, seedLength }) {
  const [editSeq, setEditSeq] = useState('');
  const [data, setData] = useState(null);
  const [preview, setPreview] = useState(null); // { data, loading, error }
  const [confirmClose, setConfirmClose] = useState(false);

  // Initialize when dialog opens
  useEffect(() => {
    if (!open || !primer) return;
    const seq = primer.primerSeq || '';
    setEditSeq(seq);
    setPreview(null);
    if (alignmentData) {
      setData(alignmentData);
    } else {
      setData(null);
      computePrimerAlignment(primer.id, seedLength).then(setData).catch(() => {});
    }
  }, [open, primer?.id]);

  const cur = preview?.data?.current || data?.current;
  const alts = preview?.data?.alternatives || data?.alternatives || [];
  const hasChanges = editSeq !== (primer?.primerSeq || '');
  const isPreviewStale = hasChanges && !preview;

  const handlePreview = useCallback(async () => {
    if (!editSeq || editSeq.length < 6) return;
    setPreview({ data: null, loading: true, error: null });
    try {
      const result = await computePrimerAlignment(primer.id, seedLength, editSeq);
      setPreview({ data: result, loading: false, error: null });
    } catch (e) {
      setPreview({ data: null, loading: false, error: String(e) });
    }
  }, [editSeq, primer?.id, seedLength]);

  const handleApply = useCallback(async () => {
    if (!hasChanges) { onOpenChange(false); return; }
    try {
      await addPrimer({
        id: primer.id,
        name: primer.name,
        type: primer.type,
        primerSeq: editSeq,
        color: primer.color || '#166534',
      });
      onOpenChange(false);
    } catch (e) {
      setPreview({ data: null, loading: false, error: String(e) });
    }
  }, [editSeq, hasChanges, primer, onOpenChange]);

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
          <DialogTitle className="text-base">
            Primer: <span className="font-mono">{primer?.name || primer?.id || ''}</span>
          </DialogTitle>
        </DialogHeader>

        {/* Sequence input */}
        <div className="px-1 mb-2">
          <div className="flex items-center gap-0">
            <span className="text-xs font-bold text-muted-foreground mr-1.5">5'</span>
            <input
              value={editSeq}
              onChange={e => setEditSeq(e.target.value.toUpperCase().replace(/[^ATCG]/g, ''))}
              className="flex-1 h-8 px-2 py-1 text-xs font-mono border rounded"
              style={{ borderColor: hasChanges ? '#f59e0b' : '#d1d5db' }}
              spellCheck={false}
              placeholder="Enter primer sequence…"
            />
            <span className="text-xs font-bold text-muted-foreground ml-1.5">3'</span>
          </div>
          {isPreviewStale && (
            <div className="text-[11px] text-amber-600 mt-0.5">Modified — preview may be stale</div>
          )}
        </div>

        {/* Tm + Preview button */}
        <div className="px-1 mb-2 flex items-center gap-2 text-sm">
          {cur && (
            <span className="font-semibold mr-auto">
              Tm = <span style={{ color: COLORS.fwd }}>{cur.tm}°C</span>
              {preview && <span className="text-xs text-muted-foreground ml-1">(preview)</span>}
            </span>
          )}
          <Button
            variant="outline" size="sm" className="text-xs h-7"
            onClick={handlePreview}
            disabled={!hasChanges || preview?.loading || editSeq.length < 6}
          >
            {preview?.loading ? 'Loading…' : 'Preview Alignment'}
          </Button>
        </div>

        {/* Alignment box */}
        <div className="flex-1 overflow-auto rounded border p-4" style={{ backgroundColor: COLORS.bg, position: 'relative' }}>
          {preview?.error && <div className="text-sm text-red-600 font-mono">{preview.error}</div>}
          {cur && <AlignmentView data={cur} />}
          {!cur && preview?.loading && <div className="text-sm text-muted-foreground italic">Computing alignment…</div>}
          {!cur && !preview?.loading && preview?.data && !preview.data.current && (
            <div className="text-sm text-red-600 font-mono">No candidate binding sites found.</div>
          )}
          {!cur && !preview && !data && (
            <div className="text-sm text-muted-foreground italic">Click "Preview Alignment" to see results.</div>
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

        {/* Bottom buttons */}
        <div className="px-1 mt-3 flex justify-end gap-2">
          {hasChanges && (
            <Button variant="outline" size="sm" onClick={() => { setEditSeq(primer?.primerSeq || ''); setPreview(null); }}>
              Reset
            </Button>
          )}
          {hasChanges && (
            <Button size="sm" onClick={handleApply} disabled={editSeq.length < 6}>
              Apply
            </Button>
          )}
        </div>

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
