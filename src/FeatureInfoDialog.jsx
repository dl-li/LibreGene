import React, { useState, useMemo, useRef } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';

/* ---------- GenBank location helpers ---------- */
function gbLocation(feature) {
  const segs = feature.segments?.length
    ? feature.segments
    : [{ start: feature.start, end: feature.end }];
  const parts = segs.map(s => `${s.start + 1}..${s.end + 1}`);
  const joined = parts.length > 1 ? `join(${parts.join(', ')})` : parts[0];
  return feature.strand === '-' ? `complement(${joined})` : joined;
}

/* ---------- Ftype options ---------- */
const FTYPE_OPTIONS = [
  'CDS', 'gene', 'promoter', 'terminator', 'rep_origin', 'misc_feature',
  'misc_binding', 'misc_recomb', 'misc_structure', 'misc_difference', 'misc_RNA',
  'primer_bind', 'protein_bind',
  'mRNA', 'rRNA', 'tRNA', 'snRNA', 'snoRNA', 'ncRNA', 'precursor_RNA', 'prim_transcript',
  'exon', 'intron', "5'UTR", "3'UTR",
  'sig_peptide', 'mat_peptide', 'transit_peptide', 'propeptide',
  'ribosome_binding_site',
  'operator', 'enhancer', 'attenuator', 'regulatory',
  'CAAT_signal', 'TATA_signal', '-35_signal', '-10_signal',
  'polyA_signal', 'polyA_site',
  'repeat_region', 'repeat_unit', 'satellite', 'LTR',
  'mobile_element', 'transposon', 'insertion_seq',
  'D-loop', 'STS', 'oriT',
  'assembly_gap', 'centromere', 'telomere', 'gap', 'variation',
  'modified_base', 'sequence_conflict',
  'source',
];

/* ---------- Qualifier extraction ---------- */
function extractQualifiers(feature) {
  const quals = [];
  const skipNotePrefixes = ['color:', 'direction:'];

  if (feature.name) {
    quals.push({ key: 'label', value: feature.name });
  }

  const rawQuals = new Map();
  if (feature.qualifiers && Array.isArray(feature.qualifiers)) {
    for (const q of feature.qualifiers) {
      if (Array.isArray(q) && q.length >= 2) {
        const key = String(q[0]);
        const val = String(q[1] ?? '');
        if (!rawQuals.has(key)) rawQuals.set(key, []);
        rawQuals.get(key).push(val);
      }
    }
  }

  for (const [key, vals] of rawQuals) {
    if (key === 'note') continue;
    for (const v of vals) {
      quals.push({ key, value: v });
    }
  }

  const rawNotes = rawQuals.get('note');
  if (rawNotes && rawNotes.length > 0) {
    for (const v of rawNotes) {
      const trimmed = v.trim();
      if (!trimmed) continue;
      if (skipNotePrefixes.some(p => trimmed.startsWith(p))) continue;
      quals.push({ key: 'note', value: trimmed });
    }
  } else if (feature.notes) {
    const trimmed = feature.notes.trim();
    if (trimmed) quals.push({ key: 'note', value: trimmed });
  }

  if (feature.translation) {
    quals.push({
      key: 'translation',
      value: feature.translation.replace(/\s+/g, ''),
    });
  }

  return quals;
}

/* ---------- Styles ---------- */
const HIGHLIGHT = '#1E40AF';
const MONO = '"Cascadia Code", ui-monospace, monospace';

/* ---------- Component ---------- */
export default function FeatureInfoDialog({ feature, open, onOpenChange, onFtypeChange, onFeatureColorChange, onFeatureLocationChange, onFeatureNameChange }) {
  const [editingFtype, setEditingFtype] = useState(false);
  const [editingLoc, setEditingLoc] = useState(false);
  const [locInput, setLocInput] = useState('');
  const [locError, setLocError] = useState('');
  const [nameInput, setNameInput] = useState('');

  // Sync name input when feature changes
  const prevFeatureId = useRef(null);
  if (feature?.id !== prevFeatureId.current) {
    prevFeatureId.current = feature?.id;
    if (feature) setNameInput(feature.name || '');
  }

  const currentFtype = feature?.ftype || 'misc_feature';

  const locLabel = useMemo(() => feature ? gbLocation(feature) : '', [feature]);

  const qualifierLines = useMemo(() => {
    if (!feature) return [];
    const quals = extractQualifiers(feature);
    return quals.map(q => {
      const escaped = q.value.includes('"') ? q.value.replace(/"/g, '\\"') : q.value;
      return { key: `/${q.key}`, label: `/${q.key}`, children: `="${escaped}"`, isKey: true };
    });
  }, [feature]);

  if (!feature) return null;

  const submitLocation = async (value) => {
    setEditingLoc(false);
    if (!onFeatureLocationChange) return;
    try {
      await onFeatureLocationChange(feature.id, value);
    } catch (e) {
      setLocError(String(e));
      setEditingLoc(true);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(open) => {
      if (!open) { setEditingFtype(false); setEditingLoc(false); setLocError(''); }
      onOpenChange(open);
    }}>
      <DialogContent className="sm:max-w-2xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle className="text-base flex items-center gap-2" style={{ paddingRight: '16px' }}>
            <span style={{ position: 'relative', display: 'inline-block', width: 18, height: 18 }}>
              <span
                style={{
                  position: 'absolute', inset: 0,
                  backgroundColor: feature.color || '#60A5FA',
                  border: '2px solid #000',
                  borderRadius: 2,
                  pointerEvents: 'none',
                }}
              />
              <input
                type="color"
                value={feature.color || '#60A5FA'}
                onChange={(e) => onFeatureColorChange?.(feature.id, e.target.value)}
                style={{
                  position: 'absolute', inset: 0,
                  width: '100%', height: '100%',
                  padding: 0, border: 'none',
                  opacity: 0, cursor: 'pointer',
                }}
              />
            </span>
            <input
              value={nameInput}
              onChange={(e) => setNameInput(e.target.value)}
              onBlur={() => {
                if (nameInput !== feature.name) onFeatureNameChange?.(feature.id, nameInput);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') { e.target.blur(); }
                else if (e.key === 'Escape') { setNameInput(feature.name); e.target.blur(); }
              }}
              style={{
                fontWeight: 600, fontSize: 'inherit',
                border: 'none', borderBottom: '1px dashed #cbd5e1', outline: 'none',
                background: 'transparent', padding: '0 0 2px 0', minWidth: 80, flex: 1,
              }}
              onClick={(e) => e.stopPropagation()}
            />
          </DialogTitle>
        </DialogHeader>

        {/* Type & Location outside the box */}
        <div className="flex flex-col gap-1.5 mt-1.5 mb-0.5 px-1">
          <div style={{ fontSize: '14px', lineHeight: '1.4' }}>
            <span style={{ fontWeight: 600, color: '#6B7280' }}>Type: </span>
            {editingFtype ? (
              <select
                value={currentFtype}
                onChange={(e) => {
                  setEditingFtype(false);
                  onFtypeChange?.(feature.id, e.target.value);
                }}
                onBlur={() => setEditingFtype(false)}
                autoFocus
                className="text-sm border rounded px-1 py-0.5"
                style={{ fontWeight: 700, fontFamily: MONO }}
              >
                {FTYPE_OPTIONS.map(o => (
                  <option key={o} value={o}>{o}</option>
                ))}
              </select>
            ) : (
              <span
                style={{ fontWeight: 700, fontFamily: MONO, color: '#1f2937', textDecoration: 'underline', cursor: 'pointer' }}
                onClick={() => { setEditingLoc(false); setEditingFtype(true); }}
              >{currentFtype}</span>
            )}
          </div>
          <div style={{ fontSize: '14px', lineHeight: '1.4' }}>
            <span style={{ fontWeight: 600, color: '#6B7280' }}>Location: </span>
            {editingLoc ? (
              <span className="inline-flex items-center gap-2">
                <input
                  value={locInput}
                  onChange={(e) => { setLocInput(e.target.value); setLocError(''); }}
                  onKeyDown={(e) => {
                    if (e.key === 'Escape') { setEditingLoc(false); setLocError(''); }
                  }}
                  autoFocus
                  className="text-sm border rounded px-1 py-0.5 inline-block"
                  style={{ fontWeight: 700, fontFamily: MONO, width: 'auto', minWidth: 200 }}
                />
                <button
                  onClick={() => submitLocation(locInput)}
                  disabled={!!locError}
                  style={{
                    fontSize: '14px', fontWeight: 700, fontFamily: MONO,
                    padding: '2px 10px', cursor: 'pointer',
                    background: '#000', color: '#fff', border: 'none', borderRadius: 4,
                  }}
                >Apply</button>
              </span>
            ) : (
              <span
                style={{ fontWeight: 700, fontFamily: MONO, color: '#1f2937', textDecoration: 'underline', cursor: 'pointer' }}
                onClick={() => { setEditingFtype(false); setLocInput(locLabel); setEditingLoc(true); setLocError(''); }}
              >{locLabel}</span>
            )}
            {locError && editingLoc && (
              <div style={{ color: '#dc2626', fontSize: '11px', fontFamily: MONO, marginTop: 2 }}>
                {locError}
              </div>
            )}
          </div>
        </div>

        {/* Qualifiers inside the box */}
        <div
          className="flex-1 overflow-y-auto rounded border p-4 mt-2"
          style={{ backgroundColor: '#faf9f7' }}
        >
          {qualifierLines.map((line, i) => (
            <div key={i} className="leading-6" style={{ fontFamily: MONO, fontSize: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
              <span style={{ fontWeight: 700, color: HIGHLIGHT }}>{line.label}</span>
              <span>{line.children}</span>
            </div>
          ))}
          {qualifierLines.length === 0 && (
            <div className="text-sm text-muted-foreground italic">No qualifiers</div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
