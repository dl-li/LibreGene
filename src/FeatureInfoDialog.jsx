import React, { useState, useMemo, useRef, useCallback } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { validateFeatureLocation } from './tauriApi';

/* ---------- GenBank location helpers ---------- */
function gbLocation(feature) {
  const segs = feature.segments?.length
    ? feature.segments
    : [{ start: feature.start, end: feature.end }];
  const parts = segs.map(s => `${s.start + 1}..${s.end + 1}`);
  const joined = parts.length > 1 ? `join(${parts.join(', ')})` : parts[0];
  return feature.strand === '-' ? `complement(${joined})` : joined;
}

// Strip complement(...) wrapper, return inner string (or the original if not wrapped)
function stripComplement(loc) {
  const m = loc.match(/^complement\((.+)\)$/);
  return m ? m[1] : loc;
}

// Wrap with complement(...) if not already wrapped
function wrapComplement(loc) {
  if (loc.startsWith('complement(')) return loc;
  return `complement(${loc})`;
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

/* ---------- Direction options ---------- */
const DIR_OPTIONS = [
  { value: '.', label: 'None' },
  { value: '+', label: 'Forward (+)', desc: '5\' → 3\'' },
  { value: '-', label: 'Reverse (-)', desc: '3\' → 5\'' },
];

/* ---------- Component ---------- */
export default function FeatureInfoDialog({ feature, open, onOpenChange, onFtypeChange, onFeatureColorChange, onFeatureLocationChange, onFeatureNameChange, onFeatureAdd, newFeatureLocation }) {
  const [editingFtype, setEditingFtype] = useState(false);
  const [editingLoc, setEditingLoc] = useState(false);
  const [locInput, setLocInput] = useState('');
  const [locError, setLocError] = useState('');
  const [nameInput, setNameInput] = useState('');

  // Create mode state
  const [createLocInput, setCreateLocInput] = useState('');
  const [createStrand, setCreateStrand] = useState('.');
  const [createFtype, setCreateFtype] = useState('misc_feature');
  const [createColor, setCreateColor] = useState('#60A5FA');
  const [createName, setCreateName] = useState('');
  const [createError, setCreateError] = useState('');
  const [createValidating, setCreateValidating] = useState(false);

  const isNewFeature = !feature && newFeatureLocation !== undefined;

  // Initialize create mode when dialog opens
  const prevOpenRef = useRef(false);
  if (open && !prevOpenRef.current && isNewFeature) {
    prevOpenRef.current = true;
    const initial = newFeatureLocation || '';
    setCreateLocInput(initial);
    setCreateStrand('.');
    setCreateFtype('misc_feature');
    setCreateColor('#60A5FA');
    setCreateName('New Feature');
    setCreateError('');
    setLocError('');
  }
  if (!open) {
    prevOpenRef.current = false;
  }

  // Sync name input when feature changes (edit mode)
  const prevFeatureId = useRef(null);
  if (feature?.id !== prevFeatureId.current) {
    prevFeatureId.current = feature?.id;
    if (feature) setNameInput(feature.name || '');
  }

  const currentFtype = isNewFeature ? createFtype : (feature?.ftype || 'misc_feature');

  const locLabel = useMemo(() => {
    if (isNewFeature) return createLocInput || '(set location)';
    return feature ? gbLocation(feature) : '';
  }, [feature, isNewFeature, createLocInput]);

  const qualifierLines = useMemo(() => {
    if (!feature) return [];
    const quals = extractQualifiers(feature);
    return quals.map(q => {
      const escaped = q.value.includes('"') ? q.value.replace(/"/g, '\\"') : q.value;
      return { key: `/${q.key}`, label: `/${q.key}`, children: `="${escaped}"`, isKey: true };
    });
  }, [feature]);

  if (!feature && !isNewFeature) return null;

  // ── Direction change handler (create mode) ──
  const handleStrandChange = useCallback((newStrand) => {
    setCreateError('');
    setCreateStrand(newStrand);
    if (newStrand === '-') {
      // Wrap with complement()
      setCreateLocInput(prev => wrapComplement(stripComplement(prev)));
    } else {
      // Strip complement() wrapper
      setCreateLocInput(prev => stripComplement(prev));
    }
  }, []);

  // ── Location input change (create mode) ──
  const handleCreateLocChange = useCallback((val) => {
    setCreateLocInput(val);
    setCreateError('');
    // Auto-detect strand from location string
    if (val.startsWith('complement(')) {
      setCreateStrand('-');
    }
  }, []);

  // ── Create feature ──
  const handleCreate = useCallback(async () => {
    if (!onFeatureAdd || !createLocInput.trim()) return;
    setCreateValidating(true);
    setCreateError('');
    try {
      const result = await validateFeatureLocation(createLocInput.trim());
      if (result && result.valid) {
        const segments = result.segments || [{ start: result.start, end: result.end }];
        const id = `feat_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
        await onFeatureAdd({
          id,
          name: createName || 'New Feature',
          start: result.start,
          end: result.end,
          color: createColor,
          ftype: createFtype,
          segments,
          strand: result.strand || '.',
          notes: '',
          translation: '',
          qualifiers: [],
        });
        onOpenChange(false);
      } else {
        setCreateError(result?.error || 'Invalid location');
      }
    } catch (e) {
      setCreateError(String(e));
    }
    setCreateValidating(false);
  }, [createLocInput, createName, createColor, createFtype, onFeatureAdd, onOpenChange]);

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

  // ── Render ──
  return (
    <Dialog open={open} onOpenChange={(open) => {
      if (!open) { setEditingFtype(false); setEditingLoc(false); setLocError(''); setCreateError(''); }
      onOpenChange(open);
    }}>
      <DialogContent className="sm:max-w-2xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle className="text-base flex items-center gap-2" style={{ paddingRight: '16px' }}>
            {isNewFeature && (
              <span style={{ position: 'relative', display: 'inline-block', width: 18, height: 18 }}>
                <span
                  style={{
                    position: 'absolute', inset: 0,
                    backgroundColor: createColor,
                    border: '2px solid #000',
                    borderRadius: 2,
                    pointerEvents: 'none',
                  }}
                />
                <input
                  type="color"
                  value={createColor}
                  onChange={(e) => setCreateColor(e.target.value)}
                  style={{
                    position: 'absolute', inset: 0,
                    width: '100%', height: '100%',
                    padding: 0, border: 'none',
                    opacity: 0, cursor: 'pointer',
                  }}
                />
              </span>
            )}
            {!isNewFeature && (
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
            )}
            {isNewFeature ? (
              <input
                value={createName}
                onChange={(e) => setCreateName(e.target.value)}
                placeholder="Feature name"
                style={{
                  fontWeight: 600, fontSize: 'inherit',
                  border: 'none', borderBottom: '1px dashed #cbd5e1', outline: 'none',
                  background: 'transparent', padding: '0 0 2px 0', minWidth: 80, flex: 1,
                }}
                onClick={(e) => e.stopPropagation()}
              />
            ) : (
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
            )}
          </DialogTitle>
        </DialogHeader>

        {/* Type & Location */}
        <div className="flex flex-col gap-1.5 mt-1.5 mb-0.5 px-1">
          {/* Type */}
          <div style={{ fontSize: '14px', lineHeight: '1.4' }}>
            <span style={{ fontWeight: 600, color: '#6B7280' }}>Type: </span>
            {isNewFeature ? (
              <select
                value={createFtype}
                onChange={(e) => setCreateFtype(e.target.value)}
                className="text-sm border rounded px-1 py-0.5"
                style={{ fontWeight: 700, fontFamily: MONO }}
              >
                {FTYPE_OPTIONS.map(o => (
                  <option key={o} value={o}>{o}</option>
                ))}
              </select>
            ) : editingFtype ? (
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

          {/* Location */}
          <div style={{ fontSize: '14px', lineHeight: '1.4' }}>
            <span style={{ fontWeight: 600, color: '#6B7280' }}>Location: </span>
            {isNewFeature ? (
              <input
                value={createLocInput}
                onChange={(e) => handleCreateLocChange(e.target.value)}
                placeholder="e.g. 100..200 or complement(300..400)"
                className="text-sm border rounded px-1 py-0.5"
                style={{
                  fontWeight: 700, fontFamily: MONO, width: 'auto', minWidth: 200,
                  borderColor: createError ? '#dc2626' : undefined,
                }}
                autoFocus
              />
            ) : editingLoc ? (
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

          {/* Direction selector (create mode only) */}
          {isNewFeature && (
            <div style={{ fontSize: '14px', lineHeight: '1.4' }}>
              <span style={{ fontWeight: 600, color: '#6B7280' }}>Direction: </span>
              <span className="inline-flex items-center gap-1 ml-1">
                {DIR_OPTIONS.map(opt => (
                  <button
                    key={opt.value}
                    onClick={() => handleStrandChange(opt.value)}
                    style={{
                      fontSize: '12px', fontWeight: 700, fontFamily: MONO,
                      padding: '2px 8px',
                      cursor: 'pointer',
                      background: createStrand === opt.value ? '#1f2937' : '#f3f4f6',
                      color: createStrand === opt.value ? '#fff' : '#374151',
                      border: `1px solid ${createStrand === opt.value ? '#1f2937' : '#d1d5db'}`,
                      borderRadius: 4,
                    }}
                  >
                    {opt.label}
                  </button>
                ))}
              </span>
              <span className="text-xs text-muted-foreground ml-2">
                {createStrand === '-' ? 'Location will be wrapped in complement(...)' : ''}
              </span>
            </div>
          )}
        </div>

        {/* Qualifiers (edit mode only) */}
        {!isNewFeature && (
          <div
            className="flex-1 overflow-y-auto rounded border p-4 mt-2"
            style={{ backgroundColor: '#faf9f7' }}
          >
            {qualifierLines.length > 0 ? qualifierLines.map((line, i) => (
              <div key={i} className="leading-6" style={{ fontFamily: MONO, fontSize: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
                <span style={{ fontWeight: 700, color: HIGHLIGHT }}>{line.label}</span>
                <span>{line.children}</span>
              </div>
            )) : (
              <div className="text-sm text-muted-foreground italic">No qualifiers</div>
            )}
          </div>
        )}

        {/* Create error */}
        {createError && (
          <div className="mx-1 mt-2 px-2 py-1 rounded text-[11px] flex items-center gap-1"
            style={{ backgroundColor: '#fef2f2', color: '#991b1b', border: '1px solid #fecaca' }}>
            <span>⚠ {createError}</span>
          </div>
        )}

        {/* Create button (create mode only) */}
        {isNewFeature && (
          <div className="px-1 mt-3 flex justify-end gap-2">
            <Button
              size="sm"
              onClick={handleCreate}
              disabled={!createLocInput.trim() || createValidating}
            >
              {createValidating ? 'Validating…' : 'Create Feature'}
            </Button>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
