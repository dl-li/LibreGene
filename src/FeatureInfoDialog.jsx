import React, { useState, useMemo, useRef } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { monoFont } from './editorConstants';

/* ---------- GenBank location helpers ---------- */
function gbLocation(feature) {
  const segs = feature.segments?.length
    ? feature.segments
    : [{ start: feature.start, end: feature.end }];
  const parts = segs.map(s => `${s.start + 1}..${s.end + 1}`);
  const joined = parts.length > 1 ? `join(${parts.join(', ')})` : parts[0];
  return feature.strand === '-' ? `complement(${joined})` : joined;
}

function unwrapComplement(loc) {
  const m = loc.match(/^complement\((.+)\)$/i);
  return m ? m[1] : loc;
}

function wrapComplement(loc) {
  if (/^complement\(/i.test(loc.trim())) return loc;
  return `complement(${loc.trim()})`;
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

/* ---------- Strand cycle ---------- */
const STRAND_CYCLE = { 'both': '+', '+': '-', '-': 'both' };
const STRAND_LABEL = { 'both': 'both', '+': '+', '-': '-' };

/* ---------- Styles ---------- */
const HIGHLIGHT = '#1E40AF';

/* ---------- Component ---------- */
export default function FeatureInfoDialog({ feature, open, onOpenChange, onFtypeChange, onFeatureColorChange, onFeatureLocationChange, onFeatureNameChange, onFeatureStrandChange, newFeatureLoc, onFeatureAdd, features }) {
  // --- Shared state ---
  const [qualifiersOpen, setQualifiersOpen] = useState(false);

  // --- Edit mode state ---
  const [editingFtype, setEditingFtype] = useState(false);
  const [editingLoc, setEditingLoc] = useState(false);
  const [locInput, setLocInput] = useState('');
  const [locError, setLocError] = useState('');
  const [nameInput, setNameInput] = useState('');
  const [nameDirty, setNameDirty] = useState(false);

  // --- Create mode state ---
  const isNewFeature = !feature && newFeatureLoc !== undefined;
  const [createLoc, setCreateLoc] = useState('');
  const [createFtype, setCreateFtype] = useState('misc_feature');
  const [createColor, setCreateColor] = useState('#60A5FA');
  const [createStrandDir, setCreateStrandDir] = useState('both'); // 'both' | '+' | '-'
  const [createName, setCreateName] = useState('New Feature');
  const [createLocError, setCreateLocError] = useState('');
  const [createError, setCreateError] = useState('');

  // Sync when dialog opens in create mode
  const prevOpenRef = useRef(false);
  if (open && !prevOpenRef.current && isNewFeature) {
    prevOpenRef.current = true;
    setCreateLoc(newFeatureLoc || '');
    setCreateName('New Feature');
    setCreateFtype('misc_feature');
    setCreateColor('#60A5FA');
    setCreateStrandDir('both');
    setCreateLocError('');
    setCreateError('');
  } else if (!open) {
    prevOpenRef.current = false;
  }

  // Sync when feature changes (edit mode)
  const prevFeatureId = useRef(null);
  if (feature?.id !== prevFeatureId.current) {
    prevFeatureId.current = feature?.id;
    if (feature) setNameInput(feature.name || '');
    setNameDirty(false);
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

  // Name conflict check for create mode
  const nameConflict = useMemo(() => {
    if (!isNewFeature) return false;
    const trimmed = createName.trim();
    if (!trimmed) return false;
    return (features || []).some(f => f.name === trimmed);
  }, [createName, features, isNewFeature]);

  // --- Create mode --------------------------------
  const handleStrandCycle = () => {
    const next = STRAND_CYCLE[createStrandDir];
    let newLoc = createLoc;
    if (createStrandDir === '-' && next !== '-') {
      newLoc = unwrapComplement(createLoc);
    } else if (createStrandDir !== '-' && next === '-') {
      newLoc = wrapComplement(createLoc);
    }
    setCreateStrandDir(next);
    setCreateLoc(newLoc);
    setCreateLocError('');
  };

  const handleCreateApply = async () => {
    const trimmedLoc = createLoc.trim();
    if (!trimmedLoc) {
      setCreateLocError('请填写 location');
      return;
    }
    if (!onFeatureAdd) return;
    setCreateError('');
    try {
      await onFeatureAdd({
        id: `feature_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`,
        name: createName || 'New Feature',
        ftype: createFtype,
        color: createColor,
        locationStr: trimmedLoc,
      });
      onOpenChange(false);
    } catch (e) {
      setCreateError(String(e));
    }
  };

  // --- Edit mode --------------------------------
  if (!feature && !isNewFeature) return null;

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

  const handleCancel = () => {
    if (isNewFeature) {
      onOpenChange(false);
      return;
    }
    setNameInput(feature?.name || '');
    setNameDirty(false);
    onOpenChange(false);
  };

  const handleApply = async () => {
    if (nameDirty && onFeatureNameChange) {
      await onFeatureNameChange(feature.id, nameInput);
    }
    setNameDirty(false);
    onOpenChange(false);
  };

  // --- Render create mode ---
  if (isNewFeature) {
    return (
      <Dialog open={open} onOpenChange={(o) => { if (!o) onOpenChange(false); }}>
        <DialogContent className="sm:max-w-2xl max-h-[80vh] flex flex-col">
          <DialogHeader>
            <DialogTitle className="text-base">New Feature</DialogTitle>
          </DialogHeader>

          {/* Name + Color row */}
          <div className="flex items-center gap-2 px-1 mb-3" style={{ minHeight: 28 }}>
            <span className="text-sm font-medium text-muted-foreground whitespace-nowrap">Name:</span>
            <input
              value={createName}
              onChange={(e) => { setCreateName(e.target.value); setCreateError(''); }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleCreateApply();
                else if (e.key === 'Escape') { setCreateName('New Feature'); e.target.blur(); }
              }}
              style={{
                fontWeight: 600, fontSize: 'inherit',
                border: 'none', borderBottom: '1px dashed #cbd5e1', outline: 'none',
                background: 'transparent', padding: '0 0 2px 0', minWidth: 80, flex: 1,
              }}
              onClick={(e) => e.stopPropagation()}
            />
            <span style={{ position: 'relative', display: 'inline-block', width: 18, height: 18, flexShrink: 0 }}>
              <span style={{ position: 'absolute', inset: 0, backgroundColor: createColor, border: '2px solid #000', borderRadius: 2, pointerEvents: 'none' }} />
              <input
                type="color"
                value={createColor}
                onChange={(e) => setCreateColor(e.target.value)}
                style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', padding: 0, border: 'none', opacity: 0, cursor: 'pointer' }}
              />
            </span>
          </div>

          {/* Type row */}
          <div className="flex items-center gap-2 px-1 mb-3" style={{ fontSize: '14px', lineHeight: '1.4' }}>
            <span style={{ fontWeight: 600, color: '#6B7280' }}>Type: </span>
            <select
              value={createFtype}
              onChange={(e) => setCreateFtype(e.target.value)}
              className="text-sm border rounded px-1 py-0.5"
              style={{ fontWeight: 700, fontFamily: monoFont }}
            >
              {FTYPE_OPTIONS.map(o => (
                <option key={o} value={o}>{o}</option>
              ))}
            </select>
          </div>

          {/* Location row */}
          <div className="flex items-start gap-2 px-1 mb-3 flex-col" style={{ fontSize: '14px', lineHeight: '1.4' }}>
            <div className="flex items-center gap-2 w-full">
              <span style={{ fontWeight: 600, color: '#6B7280', whiteSpace: 'nowrap' }}>Location: </span>
              <input
                value={createLoc}
                onChange={(e) => { setCreateLoc(e.target.value); setCreateLocError(''); setCreateError(''); }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleCreateApply();
                }}
                autoFocus
                className="text-sm border rounded px-1 py-0.5 flex-1"
                style={{ fontWeight: 700, fontFamily: monoFont, minWidth: 200 }}
                placeholder="e.g. 11..456"
              />
            </div>
            {createLocError && (
              <div style={{ color: '#dc2626', fontSize: '11px', fontFamily: monoFont }}>{createLocError}</div>
            )}
          </div>

          {/* Strand — single cycling button */}
          <div className="flex items-center gap-2 px-1 mb-3" style={{ fontSize: '14px', lineHeight: '1.4' }}>
            <span style={{ fontWeight: 600, color: '#6B7280' }}>Strand: </span>
            <button
              onClick={handleStrandCycle}
              title="Click to cycle: both → + → - → both"
              style={{
                padding: '2px 14px', borderRadius: 4, cursor: 'pointer',
                fontWeight: 700, fontFamily: monoFont, fontSize: '13px',
                border: '1px solid #d1d5db', backgroundColor: '#fff', color: '#374151',
              }}
            >
              {STRAND_LABEL[createStrandDir]}
            </button>
          </div>

          {/* Create error */}
          {createError && (
            <div className="mx-1 mb-2 px-2 py-1 rounded text-xs"
              style={{ backgroundColor: '#fee2e2', color: '#991b1b', border: '1px solid #fecaca' }}>
              {createError}
            </div>
          )}

          {/* Name conflict warning */}
          {nameConflict && (
            <div className="mx-1 mb-2 px-2 py-1 rounded text-xs"
              style={{ backgroundColor: '#fef9c3', color: '#854d0e', border: '1px solid #fde047' }}>
              Name "{createName}" is already used by another feature
            </div>
          )}

          {/* Footer */}
          <DialogFooter className="mt-3">
            <div className="flex justify-end gap-2">
              <Button variant="outline" size="sm" onClick={handleCancel}>Cancel</Button>
              <Button size="sm" onClick={handleCreateApply} disabled={!createLoc.trim() || !!nameConflict}>
                Create Feature
              </Button>
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }

  // --- Render edit mode ---
  return (
    <Dialog open={open} onOpenChange={(open) => {
      if (!open) {
        setEditingFtype(false);
        setEditingLoc(false);
        setLocError('');
        setNameInput(feature?.name || '');
        setNameDirty(false);
        setQualifiersOpen(false);
      }
      onOpenChange(open);
    }}>
      <DialogContent className="sm:max-w-2xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle className="text-base">Feature</DialogTitle>
        </DialogHeader>

        {/* Name + Color row */}
        <div className="flex items-center gap-2 px-1 mb-3" style={{ minHeight: 28 }}>
          <span className="text-sm font-medium text-muted-foreground whitespace-nowrap">Name:</span>
          <input
            value={nameInput}
            onChange={(e) => { setNameInput(e.target.value); setNameDirty(true); }}
            onBlur={() => {
              if (nameInput !== feature.name) setNameDirty(true);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') { handleApply(); }
              else if (e.key === 'Escape') { setNameInput(feature.name); setNameDirty(false); e.target.blur(); }
            }}
            style={{
              fontWeight: 600, fontSize: 'inherit',
              border: 'none', borderBottom: '1px dashed #cbd5e1', outline: 'none',
              background: 'transparent', padding: '0 0 2px 0', minWidth: 80, flex: 1,
            }}
            onClick={(e) => e.stopPropagation()}
          />
          <span style={{ position: 'relative', display: 'inline-block', width: 18, height: 18, flexShrink: 0 }}>
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
        </div>

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
                style={{ fontWeight: 700, fontFamily: monoFont }}
              >
                {FTYPE_OPTIONS.map(o => (
                  <option key={o} value={o}>{o}</option>
                ))}
              </select>
            ) : (
              <span
                style={{ fontWeight: 700, fontFamily: monoFont, color: '#1f2937', textDecoration: 'underline', cursor: 'pointer' }}
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
                  style={{ fontWeight: 700, fontFamily: monoFont, width: 'auto', minWidth: 200 }}
                />
                <button
                  onClick={() => submitLocation(locInput)}
                  disabled={!!locError}
                  style={{
                    fontSize: '14px', fontWeight: 700, fontFamily: monoFont,
                    padding: '2px 10px', cursor: 'pointer',
                    background: '#000', color: '#fff', border: 'none', borderRadius: 4,
                  }}
                >Apply</button>
              </span>
            ) : (
              <span
                style={{ fontWeight: 700, fontFamily: monoFont, color: '#1f2937', textDecoration: 'underline', cursor: 'pointer' }}
                onClick={() => { setEditingFtype(false); setLocInput(locLabel); setEditingLoc(true); setLocError(''); }}
              >{locLabel}</span>
            )}
            {locError && editingLoc && (
              <div style={{ color: '#dc2626', fontSize: '11px', fontFamily: monoFont, marginTop: 2 }}>
                {locError}
              </div>
            )}
          </div>

          {/* Strand — single cycling button (edit mode) */}
          <div style={{ fontSize: '14px', lineHeight: '1.4' }}>
            <span style={{ fontWeight: 600, color: '#6B7280' }}>Strand: </span>
            <button
              onClick={() => {
                const cur = feature.strand === '-' ? '-' : (feature.strand === '+' ? '+' : 'both');
                const next = STRAND_CYCLE[cur];
                const newStrand = next === 'both' ? '.' : next;
                let newLoc = locLabel;
                if (cur === '-' && next !== '-') {
                  newLoc = unwrapComplement(locLabel);
                } else if (cur !== '-' && next === '-') {
                  newLoc = wrapComplement(locLabel);
                }
                // Update location if complement changed
                if (newLoc !== locLabel && onFeatureLocationChange) {
                  submitLocation(newLoc);
                }
                // Update strand directly for both transitions
                if (onFeatureStrandChange && (cur === 'both' || next === 'both')) {
                  onFeatureStrandChange(feature.id, newStrand);
                }
              }}
              title="Click to cycle: both → + → - → both"
              style={{
                padding: '2px 14px', borderRadius: 4, cursor: 'pointer',
                fontWeight: 700, fontFamily: monoFont, fontSize: '13px',
                border: '1px solid #d1d5db', backgroundColor: '#fff', color: '#374151',
              }}
            >
              {STRAND_LABEL[feature.strand === '-' ? '-' : (feature.strand === '+' ? '+' : 'both')]}
            </button>
          </div>
        </div>

        {/* Qualifiers — collapsible, default collapsed */}
        <div className="mt-2 rounded border" style={{ backgroundColor: '#faf9f7' }}>
          <div
            onClick={() => setQualifiersOpen(v => !v)}
            style={{
              display: 'flex', alignItems: 'center', gap: 6,
              padding: '6px 12px', cursor: 'pointer',
              fontSize: '12px', fontWeight: 600, color: '#6B7280',
              userSelect: 'none',
            }}
          >
            <span style={{ transform: qualifiersOpen ? 'rotate(90deg)' : 'rotate(0)', transition: 'transform 0.15s', fontSize: '10px' }}>▶</span>
            Qualifiers {qualifierLines.length > 0 && `(${qualifierLines.length})`}
          </div>
          {qualifiersOpen && (
            <div className="p-4 pt-2 overflow-y-auto" style={{ maxHeight: 240 }}>
              {qualifierLines.map((line, i) => (
                <div key={i} className="leading-6" style={{ fontFamily: monoFont, fontSize: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
                  <span style={{ fontWeight: 700, color: HIGHLIGHT }}>{line.label}</span>
                  <span>{line.children}</span>
                </div>
              ))}
              {qualifierLines.length === 0 && (
                <div className="text-sm text-muted-foreground italic">No qualifiers</div>
              )}
            </div>
          )}
        </div>

        {/* Footer */}
        <DialogFooter className="mt-3">
          <div className="flex justify-end gap-2">
            <Button variant="outline" size="sm" onClick={handleCancel}>Cancel</Button>
            <Button size="sm" onClick={handleApply}>Apply</Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
