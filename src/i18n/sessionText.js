const rules = [
  [
    /^Inserted (\d+) (bp|nt|aa) before position (\d+) \((.*)\)$/,
    (m, t) =>
      t('Inserted {count} {unit} before position {position} ({sequence})', {
        count: m[1],
        unit: m[2],
        position: m[3],
        sequence: m[4],
      }),
  ],
  [
    /^Deleted (\d+) (bp|nt|aa) at (\d+)–(\d+) \((.*)\)$/,
    (m, t) =>
      t('Deleted {count} {unit} at {start}–{end} ({sequence})', {
        count: m[1],
        unit: m[2],
        start: m[3],
        end: m[4],
        sequence: m[5],
      }),
  ],
  [
    /^Replaced (\d+) (bp|nt|aa) at (\d+)–(\d+) with (\d+) (bp|nt|aa) \((.*) → (.*)\)$/,
    (m, t) =>
      t(
        'Replaced {count} {unit} at {start}–{end} with {inserted} {insertedUnit} ({before} → {after})',
        {
          count: m[1],
          unit: m[2],
          start: m[3],
          end: m[4],
          inserted: m[5],
          insertedUnit: m[6],
          before: m[7],
          after: m[8],
        },
      ),
  ],
  [
    /^Added feature ‘(.*)’ \((.*), (.*)\)$/,
    (m, t) =>
      t('Added feature ‘{name}’ ({type}, {location})', { name: m[1], type: m[2], location: m[3] }),
  ],
  [/^Added (\d+) features$/, (m, t) => t('Added {count} features', { count: m[1] })],
  [/^Deleted feature ‘(.*)’$/, (m, t) => t('Deleted feature ‘{name}’', { name: m[1] })],
  [/^Deleted (\d+) features$/, (m, t) => t('Deleted {count} features', { count: m[1] })],
  [/^Updated feature ‘(.*)’$/, (m, t) => t('Updated feature ‘{name}’', { name: m[1] })],
  [/^Updated (\d+) features$/, (m, t) => t('Updated {count} features', { count: m[1] })],
  [/^Added primer ‘(.*)’$/, (m, t) => t('Added primer ‘{name}’', { name: m[1] })],
  [/^Added (\d+) primers$/, (m, t) => t('Added {count} primers', { count: m[1] })],
  [/^Deleted primer ‘(.*)’$/, (m, t) => t('Deleted primer ‘{name}’', { name: m[1] })],
  [/^Deleted (\d+) primers$/, (m, t) => t('Deleted {count} primers', { count: m[1] })],
  [/^Updated (\d+) primers?$/, (m, t) => t('Updated {count} primers', { count: m[1] })],
  [/^Added alignment ‘(.*)’$/, (m, t) => t('Added alignment ‘{name}’', { name: m[1] })],
  [/^Added (\d+) alignments$/, (m, t) => t('Added {count} alignments', { count: m[1] })],
  [/^Removed alignment ‘(.*)’$/, (m, t) => t('Removed alignment ‘{name}’', { name: m[1] })],
  [/^Removed (\d+) alignments$/, (m, t) => t('Removed {count} alignments', { count: m[1] })],
  [
    /^Aligned (\d+) sequences with MUSCLE$/,
    (m, t) => t('Aligned {count} sequences with MUSCLE', { count: m[1] }),
  ],
  [
    /^Updated MSA rows \((\d+) sequences\)$/,
    (m, t) => t('Updated MSA rows ({count} sequences)', { count: m[1] }),
  ],
  [
    /^Changed molecule type to (.*)$/,
    (m, t) =>
      t('Changed molecule type to {type}', {
        type: ['dna', 'rna'].includes(m[1]) ? m[1].toUpperCase() : t(m[1]),
      }),
  ],
  [
    /^Changed topology to (.*)$/,
    (m, t) => t('Changed topology to {topology}', { topology: t(m[1]) }),
  ],
  [
    /^Set region of interest to (\d+)–(\d+)$/,
    (m, t) => t('Set region of interest to {start}–{end}', { start: m[1], end: m[2] }),
  ],
];

const literals = {
  'Changed sequence text colors': 'Changed sequence text colors',
  'Cleared region of interest': 'Cleared region of interest',
  'Updated project metadata': 'Updated project metadata',
  'Edited project': 'Edited project',
};

export function localizeSessionText(t, text) {
  for (const [pattern, format] of rules) {
    const match = text.match(pattern);
    if (match) return format(match, t);
  }
  return literals[text] ? t(literals[text]) : text;
}
