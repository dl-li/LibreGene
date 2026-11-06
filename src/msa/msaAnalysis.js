let rowCounter = 0;

export function makeMsaRow(name, sequence, description = '') {
  rowCounter += 1;
  return {
    id: globalThis.crypto?.randomUUID?.() || `row-${Date.now()}-${rowCounter}`,
    name: name || `Sequence ${rowCounter}`,
    description,
    sequence: String(sequence || '')
      .replace(/[\s-]/g, '')
      .toUpperCase(),
    alignedSequence: '',
  };
}

export function parseFastaRecords(text) {
  const records = [];
  let current = null;
  for (const line of String(text || '').split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    if (trimmed.startsWith('>')) {
      const header = trimmed.slice(1).trim();
      const split = header.search(/\s/);
      const name = split < 0 ? header : header.slice(0, split);
      const description = split < 0 ? '' : header.slice(split).trim();
      current = makeMsaRow(name || `Sequence ${records.length + 1}`, '', description);
      records.push(current);
    } else {
      if (!current) {
        current = makeMsaRow('Sequence 1', '');
        records.push(current);
      }
      current.sequence += trimmed.replace(/[\s-]/g, '').toUpperCase();
    }
  }
  return records.filter((row) => row.sequence);
}

export function detectMsaMoleculeType(rows) {
  let hasU = false;
  let hasT = false;
  for (const row of rows) {
    for (const c of row.sequence.toUpperCase()) {
      if (!/[A-Z*?]/.test(c)) continue;
      if (!'ACGTURYSWKMBDHVN'.includes(c)) return 'protein';
      hasU ||= c === 'U';
      hasT ||= c === 'T';
    }
  }
  return hasU && !hasT ? 'rna' : 'dna';
}

export function rowDisplaySequence(row, aligned) {
  return aligned ? row.alignedSequence || '' : row.sequence || '';
}

export function computeConsensus(rows, aligned = true) {
  const sequences = rows.map((row) => rowDisplaySequence(row, aligned));
  const length = sequences.reduce((max, sequence) => Math.max(max, sequence.length), 0);
  const consensus = [];
  const conservation = [];
  const gapFractions = [];
  for (let col = 0; col < length; col++) {
    const counts = new Map();
    let gaps = 0;
    for (const sequence of sequences) {
      const c = sequence[col] || '-';
      if (c === '-') {
        gaps += 1;
      } else {
        counts.set(c, (counts.get(c) || 0) + 1);
      }
    }
    let symbol = '-';
    let max = 0;
    for (const [candidate, count] of counts) {
      if (count > max) {
        symbol = candidate;
        max = count;
      }
    }
    const occupied = sequences.length - gaps;
    consensus.push(symbol);
    conservation.push(occupied ? max / occupied : 0);
    gapFractions.push(sequences.length ? gaps / sequences.length : 0);
  }
  return {
    sequence: consensus.join(''),
    conservation,
    gapFractions,
  };
}

const BLOSUM62_ORDER = 'ARNDCQEGHILKMFPSTWYV';
const BLOSUM62_ROWS = [
  [4, -1, -2, -2, 0, -1, -1, 0, -2, -1, -1, -1, -1, -2, -1, 1, 0, -3, -2, 0],
  [-1, 5, 0, -2, -3, 1, 0, -2, 0, -3, -2, 2, -1, -3, -2, -1, -1, -3, -2, -3],
  [-2, 0, 6, 1, -3, 0, 0, 0, 1, -3, -3, 0, -2, -3, -2, 1, 0, -4, -2, -3],
  [-2, -2, 1, 6, -3, 0, 2, -1, -1, -3, -4, -1, -3, -3, -1, 0, -1, -4, -3, -3],
  [0, -3, -3, -3, 9, -3, -4, -3, -3, -1, -1, -3, -1, -2, -3, -1, -1, -2, -2, -1],
  [-1, 1, 0, 0, -3, 5, 2, -2, 0, -3, -2, 1, 0, -3, -1, 0, -1, -2, -1, -2],
  [-1, 0, 0, 2, -4, 2, 5, -2, 0, -3, -3, 1, -2, -3, -1, 0, -1, -3, -2, -2],
  [0, -2, 0, -1, -3, -2, -2, 6, -2, -4, -4, -2, -3, -3, -2, 0, -2, -2, -3, -3],
  [-2, 0, 1, -1, -3, 0, 0, -2, 8, -3, -3, -1, -2, -1, -2, -1, -2, -2, 2, -3],
  [-1, -3, -3, -3, -1, -3, -3, -4, -3, 4, 2, -3, 1, 0, -3, -2, -1, -3, -1, 3],
  [-1, -2, -3, -4, -1, -2, -3, -4, -3, 2, 4, -2, 2, 0, -3, -2, -1, -2, -1, 1],
  [-1, 2, 0, -1, -3, 1, 1, -2, -1, -3, -2, 5, -1, -3, -1, 0, -1, -3, -2, -2],
  [-1, -1, -2, -3, -1, 0, -2, -3, -2, 1, 2, -1, 5, 0, -2, -1, -1, -1, -1, 1],
  [-2, -3, -3, -3, -2, -3, -3, -3, -1, 0, 0, -3, 0, 6, -4, -2, -2, 1, 3, -1],
  [-1, -2, -2, -1, -3, -1, -1, -2, -2, -3, -3, -1, -2, -4, 7, -1, -1, -4, -3, -2],
  [1, -1, 1, 0, -1, 0, 0, 0, -1, -2, -2, 0, -1, -2, -1, 4, 1, -3, -2, -2],
  [0, -1, 0, -1, -1, -1, -1, -2, -2, -1, -1, -1, -1, -2, -1, 1, 5, -2, -2, 0],
  [-3, -3, -4, -4, -2, -2, -3, -2, -2, -3, -2, -3, -1, 1, -4, -3, -2, 11, 2, -3],
  [-2, -2, -2, -3, -2, -1, -2, -3, 2, -1, -1, -2, -1, 3, -3, -2, -2, 2, 7, -1],
  [0, -3, -3, -3, -1, -2, -2, -3, -3, 3, 1, -2, 1, -1, -2, -2, 0, -3, -1, 4],
];

export function blosum62Score(a, b) {
  const i = BLOSUM62_ORDER.indexOf(a);
  const j = BLOSUM62_ORDER.indexOf(b);
  if (i < 0 || j < 0) return a === '*' && b === '*' ? 1 : -1;
  return BLOSUM62_ROWS[i][j];
}

export function pairwiseMsaStats(sequenceA, sequenceB, moleculeType = 'protein') {
  let matches = 0;
  let positives = 0;
  let compared = 0;
  const length = Math.min(sequenceA.length, sequenceB.length);
  for (let i = 0; i < length; i++) {
    const a = sequenceA[i].toUpperCase();
    const b = sequenceB[i].toUpperCase();
    if (a === '-' || b === '-') continue;
    compared += 1;
    if (a === b) matches += 1;
    if (moleculeType === 'protein' && blosum62Score(a, b) > 0) positives += 1;
  }
  return {
    compared,
    identity: compared ? matches / compared : null,
    similarity: moleculeType === 'protein' && compared ? positives / compared : null,
  };
}

export function computePairwiseMatrix(rows, aligned, moleculeType) {
  return rows.map((rowA) =>
    rows.map((rowB) =>
      pairwiseMsaStats(
        rowDisplaySequence(rowA, aligned),
        rowDisplaySequence(rowB, aligned),
        moleculeType,
      ),
    ),
  );
}

export function averagePairwiseIdentity(matrix) {
  let sum = 0;
  let count = 0;
  for (let i = 0; i < matrix.length; i++) {
    for (let j = i + 1; j < matrix.length; j++) {
      if (matrix[i][j].identity != null) {
        sum += matrix[i][j].identity;
        count += 1;
      }
    }
  }
  return count ? sum / count : null;
}

export function selectionToFasta(rows, aligned, selection) {
  if (!selection || !rows.length) return '';
  const startRow = Math.max(0, Math.min(selection.startRow, selection.endRow));
  const endRow = Math.min(rows.length - 1, Math.max(selection.startRow, selection.endRow));
  const startCol = Math.max(0, Math.min(selection.startCol, selection.endCol));
  const endCol = Math.max(startCol, Math.max(selection.startCol, selection.endCol));
  return rows
    .slice(startRow, endRow + 1)
    .map((row) => {
      const description = row.description ? ` ${row.description}` : '';
      const sequence = rowDisplaySequence(row, aligned).slice(startCol, endCol + 1);
      return `>${row.name}${description}\n${sequence}`;
    })
    .join('\n');
}
