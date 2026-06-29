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

  // Label
  if (feature.name) {
    quals.push({ key: 'label', value: feature.name });
  }

  // Notes — filter out synthetic color/direction notes
  const skipNotes = new Set(['color', 'direction']);
  if (feature.notes) {
    for (const note of feature.notes.split('; ')) {
      const trimmed = note.trim();
      if (!trimmed) continue;
      const [prefix] = trimmed.split(':');
      if (skipNotes.has(prefix)) continue;
      quals.push({ key: 'note', value: trimmed });
    }
  }

  // Raw GenBank qualifiers (gene, product, codon_start, etc.)
  if (feature.qualifiers && Array.isArray(feature.qualifiers) && feature.qualifiers.length) {
    for (const q of feature.qualifiers) {
      if (Array.isArray(q) && q.length >= 2) {
        quals.push({ key: String(q[0]), value: String(q[1] ?? '') });
      }
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

function GbLine({ label, children, isKey }) {
  if (isKey) {
    return (
      <div className="leading-6" style={{ fontFamily: '"Cascadia Code", ui-monospace, monospace', fontSize: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
        <span style={{ fontWeight: 700, color: HIGHLIGHT }}>{label}</span>
        <span>{children}</span>
      </div>
    );
  }
  // Location line — indent + no key highlight
  return (
    <div className="leading-6" style={{ fontFamily: '"Cascadia Code", ui-monospace, monospace', fontSize: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
      <span style={{ color: '#374151' }}>{label}</span>
    </div>
  );
}

/* ---------- Component ---------- */
export default function FeatureInfoDialog({ feature, open, onOpenChange }) {
  const { loc, ftype, lines } = useMemo(() => {
    if (!feature) return { loc: '', ftype: '', lines: [] };

    const loc = gbLocation(feature);
    const ftype = feature.ftype || 'misc_feature';
    const quals = extractQualifiers(feature);

    const lines = [];
    // First line: location with indented ftype
    lines.push({ isKey: false, label: `${ftype.padEnd(20)}${loc}` });

    for (const q of quals) {
      const escaped = q.value.includes('"') ? q.value.replace(/"/g, '\\"') : q.value;
      lines.push({ isKey: true, label: `/${q.key}`, children: `="${escaped}"` });
    }

    return { loc, ftype, lines };
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
          {lines.length > 0 && (
            <GbLine label={lines[0].label} isKey={false} />
          )}
          {lines.slice(1).length > 0 && <div className="mt-1" />}
          {lines.slice(1).map((line, i) => (
            <GbLine key={i} label={line.label} isKey={line.isKey}>{line.children}</GbLine>
          ))}
          {lines.length === 0 && (
            <div className="text-sm text-muted-foreground italic">No qualifier data</div>
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
