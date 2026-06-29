import React, { useMemo } from 'react';
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

/* ---------- Qualifier extraction ---------- */
function extractQualifiers(feature) {
  const quals = [];
  const skipNotePrefixes = ['color:', 'direction:'];

  // Label
  if (feature.name) {
    quals.push({ key: 'label', value: feature.name });
  }

  // Collect raw qualifiers into a Map<key, values[]>
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

  // Non-note raw qualifiers (gene, product, codon_start, etc.)
  for (const [key, vals] of rawQuals) {
    if (key === 'note') continue;
    for (const v of vals) {
      quals.push({ key, value: v });
    }
  }

  // Notes: use individual entries from raw qualifiers whenever possible
  const rawNotes = rawQuals.get('note');
  if (rawNotes && rawNotes.length > 0) {
    for (const v of rawNotes) {
      const trimmed = v.trim();
      if (!trimmed) continue;
      if (skipNotePrefixes.some(p => trimmed.startsWith(p))) continue;
      quals.push({ key: 'note', value: trimmed });
    }
  } else if (feature.notes) {
    // Backward compat: display the entire notes field as one /note entry
    const trimmed = feature.notes.trim();
    if (trimmed) {
      quals.push({ key: 'note', value: trimmed });
    }
  }

  // Translation (remove newlines)
  if (feature.translation) {
    quals.push({
      key: 'translation',
      value: feature.translation.replace(/\s+/g, ''),
    });
  }

  return quals;
}

/* ---------- Line renderer ---------- */
const HIGHLIGHT = '#1E40AF'; // enzyme active blue
const MONO = '"Cascadia Code", ui-monospace, monospace';

function GbLine({ label, children, isKey }) {
  if (isKey) {
    return (
      <div className="leading-6" style={{ fontFamily: MONO, fontSize: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
        <span style={{ fontWeight: 700, color: HIGHLIGHT }}>{label}</span>
        <span>{children}</span>
      </div>
    );
  }
  // Highlighted header line (ftype or location)
  return (
    <div className="leading-6" style={{ fontFamily: MONO, fontSize: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
      <span style={{ fontWeight: 600, color: HIGHLIGHT }}>{label}</span>
    </div>
  );
}

/* ---------- Component ---------- */
export default function FeatureInfoDialog({ feature, open, onOpenChange }) {
  const lines = useMemo(() => {
    if (!feature) return [];

    const loc = gbLocation(feature);
    const ftype = feature.ftype || 'misc_feature';
    const quals = extractQualifiers(feature);

    const lines = [];
    // Line 1: ftype (highlighted)
    lines.push({ key: 'ftype', label: ftype });
    // Line 2: location (highlighted)
    lines.push({ key: 'loc', label: loc });

    // Qualifier lines
    for (const q of quals) {
      const escaped = q.value.includes('"') ? q.value.replace(/"/g, '\\"') : q.value;
      const label = `/${q.key}`;
      const children = `="${escaped}"`;
      lines.push({ key: label, label, children, isKey: true });
    }

    return lines;
  }, [feature]);

  if (!feature) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle className="text-base">Feature Info — {feature.name}</DialogTitle>
        </DialogHeader>

        <div
          className="flex-1 overflow-y-auto rounded border p-4 mt-2"
          style={{ backgroundColor: '#faf9f7' }}
        >
          {lines.map((line, i) => (
            line.isKey ? (
              <div key={i} className="leading-6" style={{ fontFamily: MONO, fontSize: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
                <span style={{ fontWeight: 700, color: HIGHLIGHT }}>{line.label}</span>
                <span>{line.children}</span>
              </div>
            ) : (
              <div key={i} className="leading-6" style={{ fontFamily: MONO, fontSize: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
                <span style={{ fontWeight: 600, color: HIGHLIGHT }}>{line.label}</span>
              </div>
            )
          ))}
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
