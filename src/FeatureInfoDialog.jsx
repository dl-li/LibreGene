import { useState, useMemo, useRef } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { InlineNotice } from '@/components/ui/notice';
import { ChevronDown, ChevronRight, Trash2 } from 'lucide-react';
import { monoFont, locationString1based, locationStringTo0based } from './editorConstants';

/* ---------- HTML tag stripping ---------- */
function stripHtml(str) {
  return str.replace(/<[^>]*>/g, '');
}

/* ---------- Location helpers (UI strings are 1-based inclusive) ---------- */

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
  'CDS',
  'gene',
  'promoter',
  'terminator',
  'rep_origin',
  'misc_feature',
  'misc_binding',
  'misc_recomb',
  'misc_structure',
  'misc_difference',
  'misc_RNA',
  'primer_bind',
  'protein_bind',
  'mRNA',
  'rRNA',
  'tRNA',
  'snRNA',
  'snoRNA',
  'ncRNA',
  'precursor_RNA',
  'prim_transcript',
  'exon',
  'intron',
  "5'UTR",
  "3'UTR",
  'sig_peptide',
  'mat_peptide',
  'transit_peptide',
  'propeptide',
  'ribosome_binding_site',
  'operator',
  'enhancer',
  'attenuator',
  'regulatory',
  'CAAT_signal',
  'TATA_signal',
  '-35_signal',
  '-10_signal',
  'polyA_signal',
  'polyA_site',
  'repeat_region',
  'repeat_unit',
  'satellite',
  'LTR',
  'mobile_element',
  'transposon',
  'insertion_seq',
  'D-loop',
  'STS',
  'oriT',
  'assembly_gap',
  'centromere',
  'telomere',
  'gap',
  'variation',
  'modified_base',
  'sequence_conflict',
  'source',
];

// Protein projects annotate amino-acid features (GenBank/UniProt conventions).
const PROTEIN_FTYPE_OPTIONS = [
  'Region',
  'Domain',
  'Site',
  'Active Site',
  'Binding Site',
  'Modified Residue',
  'Glycosylation Site',
  'Disulfide Bond',
  'Signal Peptide',
  'Transit Peptide',
  'Propeptide',
  'mat_peptide',
  'Chain',
  'Peptide',
  'Misc Feature',
  'misc_feature',
  'repeat_region',
  'variant',
  'conflict',
  'unsure',
  'helix',
  'strand',
  'turn',
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
      quals.push({ key, value: stripHtml(v) });
    }
  }

  const rawNotes = rawQuals.get('note');
  if (rawNotes && rawNotes.length > 0) {
    for (const v of rawNotes) {
      const trimmed = stripHtml(v).trim();
      if (!trimmed) continue;
      if (skipNotePrefixes.some((p) => trimmed.startsWith(p))) continue;
      quals.push({ key: 'note', value: trimmed });
    }
  } else if (feature.notes) {
    const trimmed = stripHtml(feature.notes).trim();
    if (trimmed) quals.push({ key: 'note', value: trimmed });
  }

  if (feature.translation) {
    quals.push({
      key: 'translation',
      value: stripHtml(feature.translation).replace(/\s+/g, ''),
    });
  }

  return quals;
}

/* ---------- Strand ---------- */
const STRAND_OPTIONS = ['both', '+', '-'];
const STRAND_LABEL = { both: 'both', '+': '+', '-': '-' };

/* ---------- Shared bits ---------- */
const HIGHLIGHT = '#1E40AF';

const LABEL_CLS =
  'w-[76px] shrink-0 text-xs font-medium uppercase tracking-wide text-muted-foreground';

function ColorSwatch({ color, onChange }) {
  return (
    <span className="relative inline-block size-6 shrink-0">
      <span
        className="absolute inset-0 rounded-md shadow-sm ring-1 ring-inset ring-black/10"
        style={{ backgroundColor: color }}
      />
      <input
        type="color"
        value={color}
        onChange={(e) => onChange(e.target.value)}
        className="absolute inset-0 size-full cursor-pointer opacity-0"
      />
    </span>
  );
}

function StrandSegmented({ value, onSelect }) {
  return (
    <div className="inline-flex rounded-lg border border-input bg-muted/50 p-0.5">
      {STRAND_OPTIONS.map((s) => (
        <button
          key={s}
          type="button"
          onClick={() => onSelect(s)}
          className={
            'min-w-10 rounded-md px-2.5 py-1 font-mono text-xs font-bold transition-colors ' +
            (value === s
              ? 'bg-background text-foreground shadow-sm ring-1 ring-border'
              : 'text-muted-foreground hover:text-foreground')
          }
        >
          {STRAND_LABEL[s]}
        </button>
      ))}
    </div>
  );
}

function FtypeSelect({ value, onChange, autoFocus, onBlur, options = FTYPE_OPTIONS }) {
  return (
    <div className="relative inline-flex items-center">
      <select
        value={value}
        onChange={onChange}
        onBlur={onBlur}
        autoFocus={autoFocus}
        className="h-8 appearance-none rounded-md border border-input bg-background pl-2 pr-7 font-mono text-[13px] font-semibold outline-none transition-shadow focus:border-ring focus:ring-[3px] focus:ring-ring/40"
      >
        {options.map((o) => (
          <option key={o} value={o}>
            {o}
          </option>
        ))}
      </select>
      <ChevronDown className="pointer-events-none absolute right-2 size-3.5 text-muted-foreground" />
    </div>
  );
}

/* ---------- Component ---------- */
export default function FeatureInfoDialog({
  feature,
  open,
  onOpenChange,
  onFtypeChange,
  onFeatureColorChange,
  onFeatureLocationChange,
  onFeatureNameChange,
  onFeatureStrandChange,
  newFeatureLoc,
  onFeatureAdd,
  onDeleteFeature,
  features,
  moleculeType = 'dna',
}) {
  // Protein projects annotate amino-acid features; DNA list is nucleotide-centric.
  const isProtein = moleculeType === 'protein';
  const ftypeOptions = isProtein ? PROTEIN_FTYPE_OPTIONS : FTYPE_OPTIONS;
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

  const locLabel = useMemo(() => (feature ? locationString1based(feature) : ''), [feature]);

  const qualifierLines = useMemo(() => {
    if (!feature) return [];
    const quals = extractQualifiers(feature);
    return quals.map((q) => {
      const escaped = q.value.includes('"') ? q.value.replace(/"/g, '\\"') : q.value;
      return { key: `/${q.key}`, label: `/${q.key}`, children: `="${escaped}"`, isKey: true };
    });
  }, [feature]);

  // Name conflict check for create mode
  const nameConflict = useMemo(() => {
    if (!isNewFeature) return false;
    const trimmed = createName.trim();
    if (!trimmed) return false;
    return (features || []).some((f) => f.name === trimmed);
  }, [createName, features, isNewFeature]);

  // --- Create mode --------------------------------
  const handleStrandSelect = (next) => {
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
      setCreateLocError('Please enter a location');
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
        // UI input is 1-based; the Tauri layer parses 0-based strings
        locationStr: locationStringTo0based(trimmedLoc),
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
      await onFeatureLocationChange(feature.id, locationStringTo0based(value));
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

  const handleEditStrandSelect = (next) => {
    const cur = feature.strand === '-' ? '-' : feature.strand === '+' ? '+' : 'both';
    if (next === cur) return;
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
  };

  // --- Render create mode ---
  if (isNewFeature) {
    return (
      <Dialog
        open={open}
        onOpenChange={(o) => {
          if (!o) onOpenChange(false);
        }}
      >
        <DialogContent className="sm:max-w-2xl max-h-[80vh] flex flex-col">
          <DialogHeader>
            <DialogTitle>New Feature</DialogTitle>
          </DialogHeader>

          <div className="grid grid-cols-[76px_1fr] items-center gap-x-3 gap-y-3.5">
            {/* Name */}
            <span className={LABEL_CLS}>Name</span>
            <div className="flex items-center gap-3 min-w-0">
              <input
                value={createName}
                onChange={(e) => {
                  setCreateName(e.target.value);
                  setCreateError('');
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleCreateApply();
                  else if (e.key === 'Escape') {
                    setCreateName('New Feature');
                    e.target.blur();
                  }
                }}
                className="min-w-0 flex-1 border-b border-dashed border-input bg-transparent py-0.5 text-sm font-semibold outline-none transition-colors focus:border-primary"
                onClick={(e) => e.stopPropagation()}
              />
              <ColorSwatch color={createColor} onChange={setCreateColor} />
            </div>

            {/* Type */}
            <span className={LABEL_CLS}>Type</span>
            <div>
              <FtypeSelect
                value={createFtype}
                onChange={(e) => setCreateFtype(e.target.value)}
                options={ftypeOptions}
              />
            </div>

            {/* Location */}
            <span className={LABEL_CLS}>Location</span>
            <div className="min-w-0">
              <input
                value={createLoc}
                onChange={(e) => {
                  setCreateLoc(e.target.value);
                  setCreateLocError('');
                  setCreateError('');
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleCreateApply();
                }}
                autoFocus
                className="h-8 w-full rounded-md border border-input bg-background px-2 font-mono text-[13px] font-semibold outline-none transition-shadow focus:border-ring focus:ring-[3px] focus:ring-ring/40"
                placeholder="1-based inclusive, e.g. 1..100"
                title="1-based inclusive, e.g. 1..100, join(1..100,200..300), complement(50..80)"
              />
              {createLocError && (
                <div className="mt-1 font-mono text-[11px] text-red-600">{createLocError}</div>
              )}
            </div>

            {/* Strand */}
            <span className={LABEL_CLS}>Strand</span>
            <div>
              <StrandSegmented value={createStrandDir} onSelect={handleStrandSelect} />
            </div>
          </div>

          {/* Create error */}
          {createError && <InlineNotice tone="error">{createError}</InlineNotice>}

          {/* Name conflict warning */}
          {nameConflict && (
            <InlineNotice tone="warning">
              Name "{createName}" is already used by another feature
            </InlineNotice>
          )}

          {/* Footer */}
          <DialogFooter className="mt-1">
            <div className="flex justify-end gap-2">
              <Button variant="outline" size="sm" onClick={handleCancel}>
                Cancel
              </Button>
              <Button
                size="sm"
                onClick={handleCreateApply}
                disabled={!createLoc.trim()}
              >
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
    <Dialog
      open={open}
      onOpenChange={(open) => {
        if (!open) {
          setEditingFtype(false);
          setEditingLoc(false);
          setLocError('');
          setNameInput(feature?.name || '');
          setNameDirty(false);
          setQualifiersOpen(false);
        }
        onOpenChange(open);
      }}
    >
      <DialogContent className="sm:max-w-2xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>Feature</DialogTitle>
        </DialogHeader>

        <div className="grid grid-cols-[76px_1fr] items-center gap-x-3 gap-y-3.5">
          {/* Name */}
          <span className={LABEL_CLS}>Name</span>
          <div className="flex items-center gap-3 min-w-0">
            <input
              value={nameInput}
              onChange={(e) => {
                setNameInput(e.target.value);
                setNameDirty(true);
              }}
              onBlur={() => {
                if (nameInput !== feature.name) setNameDirty(true);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  handleApply();
                } else if (e.key === 'Escape') {
                  setNameInput(feature.name);
                  setNameDirty(false);
                  e.target.blur();
                }
              }}
              className="min-w-0 flex-1 border-b border-dashed border-input bg-transparent py-0.5 text-sm font-semibold outline-none transition-colors focus:border-primary"
              onClick={(e) => e.stopPropagation()}
            />
            <ColorSwatch
              color={feature.color || '#60A5FA'}
              onChange={(c) => onFeatureColorChange?.(feature.id, c)}
            />
          </div>

          {/* Type */}
          <span className={LABEL_CLS}>Type</span>
          <div>
            {editingFtype ? (
              <FtypeSelect
                value={currentFtype}
                autoFocus
                onBlur={() => setEditingFtype(false)}
                onChange={(e) => {
                  setEditingFtype(false);
                  onFtypeChange?.(feature.id, e.target.value);
                }}
                options={
                  ftypeOptions.includes(currentFtype)
                    ? ftypeOptions
                    : [currentFtype, ...ftypeOptions]
                }
              />
            ) : (
              <button
                type="button"
                className="rounded-md bg-muted px-2 py-1 font-mono text-[13px] font-semibold text-foreground transition-shadow hover:ring-2 hover:ring-ring/40"
                title="Click to change type"
                onClick={() => {
                  setEditingLoc(false);
                  setEditingFtype(true);
                }}
              >
                {currentFtype}
              </button>
            )}
          </div>

          {/* Location */}
          <span className={LABEL_CLS}>Location</span>
          <div className="min-w-0">
            {editingLoc ? (
              <div className="flex items-center gap-2">
                <input
                  value={locInput}
                  onChange={(e) => {
                    setLocInput(e.target.value);
                    setLocError('');
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') submitLocation(locInput);
                    else if (e.key === 'Escape') {
                      setEditingLoc(false);
                      setLocError('');
                    }
                  }}
                  autoFocus
                  placeholder="1-based inclusive"
                  className="h-8 min-w-0 flex-1 rounded-md border border-input bg-background px-2 font-mono text-[13px] font-semibold outline-none transition-shadow focus:border-ring focus:ring-[3px] focus:ring-ring/40"
                />
                <Button
                  size="sm"
                  className="h-8"
                  onClick={() => submitLocation(locInput)}
                  disabled={!!locError}
                >
                  Apply
                </Button>
              </div>
            ) : (
              <button
                type="button"
                className="max-w-full truncate rounded-md bg-muted px-2 py-1 font-mono text-[13px] font-semibold text-foreground transition-shadow hover:ring-2 hover:ring-ring/40"
                title="Click to edit location (1-based inclusive)"
                onClick={() => {
                  setEditingFtype(false);
                  setLocInput(locLabel);
                  setEditingLoc(true);
                  setLocError('');
                }}
              >
                {locLabel}
              </button>
            )}
            {locError && editingLoc && (
              <div className="mt-1 font-mono text-[11px] text-red-600">{locError}</div>
            )}
          </div>

          {/* Strand */}
          <span className={LABEL_CLS}>Strand</span>
          <div>
            <StrandSegmented
              value={feature.strand === '-' ? '-' : feature.strand === '+' ? '+' : 'both'}
              onSelect={handleEditStrandSelect}
            />
          </div>
        </div>

        {/* Qualifiers — collapsible, default collapsed */}
        <div className="overflow-hidden rounded-lg border border-border/70 bg-muted/40">
          <button
            type="button"
            onClick={() => setQualifiersOpen((v) => !v)}
            className="flex w-full items-center gap-1.5 px-3 py-2 text-xs font-semibold text-muted-foreground transition-colors hover:text-foreground"
          >
            <ChevronRight
              className={`size-3.5 transition-transform duration-150 ${qualifiersOpen ? 'rotate-90' : ''}`}
            />
            Qualifiers
            {qualifierLines.length > 0 && (
              <span className="rounded-full bg-accent px-1.5 py-px text-[10px] font-medium tabular-nums text-accent-foreground">
                {qualifierLines.length}
              </span>
            )}
          </button>
          {qualifiersOpen && (
            <div className="max-h-60 overflow-y-auto border-t border-border/60 px-3.5 py-2.5">
              {qualifierLines.map((line, i) => (
                <div
                  key={i}
                  className="leading-6"
                  style={{
                    fontFamily: monoFont,
                    fontSize: '12px',
                    whiteSpace: 'pre-wrap',
                    wordBreak: 'break-all',
                  }}
                >
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
        <DialogFooter className="mt-1">
          <div className="flex w-full items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              className="mr-auto text-destructive hover:bg-destructive/10 hover:text-destructive"
              onClick={async () => {
                await onDeleteFeature(feature.id);
                onOpenChange(false);
              }}
            >
              <Trash2 className="size-3.5" />
              Delete
            </Button>
            <Button variant="outline" size="sm" onClick={handleCancel}>
              Cancel
            </Button>
            <Button size="sm" onClick={handleApply}>
              Apply
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
