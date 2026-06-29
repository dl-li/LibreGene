import React, { useState, useMemo } from 'react';
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
export default function FeatureInfoDialog({ feature, open, onOpenChange, onFtypeChange, onFeatureColorChange, onFeatureLocationChange }) {
  const [editingFtype, setEditingFtype] = useState(false);
  const [editingLoc, setEditingLoc] = useState(false);
  const [locInput, setLocInput] = useState('');
  const [locError, setLocError] = useState('');

  const lines = useMemo(() => {
    if (!feature) return [];
    const loc = gbLocation(feature);
    const ftype = feature.ftype || 'misc_feature';
    const quals = extractQualifiers(feature);
    const lines = [];
    lines.push({ key: 'ftype', label: ftype });
    lines.push({ key: 'loc', label: loc });
    for (const q of quals) {
      const escaped = q.value.includes('"') ? q.value.replace(/"/g, '\\"') : q.value;
      lines.push({ key: `/${q.key}`, label: `/${q.key}`, children: `="${escaped}"`, isKey: true });
    }
    return lines;
  }, [feature]);

  if (!feature) return null;

  const currentFtype = feature.ftype || 'misc_feature';
  const locLabel = lines.find(l => l.key === 'loc')?.label || '';

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
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle className="text-base flex items-center gap-2">
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
            {feature.name}
          </DialogTitle>
        </DialogHeader>

        <div
          className="flex-1 overflow-y-auto rounded border p-4 mt-2"
          style={{ backgroundColor: '#faf9f7' }}
        >
          {lines.map((line, i) => {
            if (line.isKey) {
              return (
                <div key={i} className="leading-6" style={{ fontFamily: MONO, fontSize: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
                  <span style={{ fontWeight: 700, color: HIGHLIGHT }}>{line.label}</span>
                  <span>{line.children}</span>
                </div>
              );
            }
            if (line.key === 'ftype') {
              return (
                <div key={i} className="leading-6" style={{ fontFamily: MONO, fontSize: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
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
                      style={{ fontWeight: 900, fontFamily: MONO }}
                    >
                      {FTYPE_OPTIONS.map(o => (
                        <option key={o} value={o}>{o}</option>
                      ))}
                    </select>
                  ) : (
                    <span
                      style={{ fontWeight: 900, color: '#1f2937', textDecoration: 'underline', cursor: 'pointer' }}
                      onDoubleClick={() => setEditingFtype(true)}
                      title="Double-click to edit"
                    >{line.label}</span>
                  )}
                </div>
              );
            }
            // Location line — editable with backend validation
            return (
              <div key={i} className="leading-6" style={{ fontFamily: MONO, fontSize: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
                {editingLoc ? (
                  <div>
                    <input
                      value={locInput}
                      onChange={(e) => { setLocInput(e.target.value); setLocError(''); }}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') { submitLocation(locInput); }
                        else if (e.key === 'Escape') { setEditingLoc(false); setLocError(''); }
                      }}
                      onBlur={() => { if (!locError) { setEditingLoc(false); } }}
                      autoFocus
                      className="w-full text-sm border rounded px-1 py-0.5"
                      style={{ fontWeight: 900, fontFamily: MONO }}
                    />
                    {locError && (
                      <div style={{ color: '#dc2626', fontSize: '11px', fontFamily: MONO, marginTop: 2 }}>
                        {locError}
                      </div>
                    )}
                  </div>
                ) : (
                  <span
                    style={{ fontWeight: 900, color: '#1f2937', textDecoration: 'underline', cursor: 'pointer' }}
                    onDoubleClick={() => { setLocInput(locLabel); setEditingLoc(true); setLocError(''); }}
                    title="Double-click to edit"
                  >{line.label}</span>
                )}
              </div>
            );
          })}
          {lines.length === 0 && (
            <div className="text-sm text-muted-foreground italic">No data</div>
          )}
        </div>

        <div className="flex justify-end mt-2">
          <span className="text-xs text-muted-foreground">
            GenBank format
          </span>
        </div>
      </DialogContent>
    </Dialog>
  );
}
