//! Codon optimization — ports DNA Chisel's algorithms (MaximizeCAI /
//! MatchCodonUsage / HarmonizeRca) with 9 embedded Kazusa codon-usage
//! tables (python_codon_tables, CC0). Deterministic throughout (no rng).
//!
//! Coordinate convention: all template coordinates are 0-based inclusive;
//! coding-sequence coordinates are 0-based, 3 bases per codon.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::models::Feature;
use crate::translate::{codon_index, GENETIC_CODE};

const CODON_USAGE_TSV: &str = include_str!("../data/codon_usage.tsv");

/// Species keys in canonical (TSV) order.
const SPECIES_ORDER: [&str; 9] = [
    "b_subtilis",
    "c_elegans",
    "d_melanogaster",
    "e_coli",
    "g_gallus",
    "h_sapiens",
    "m_musculus",
    "m_musculus_domesticus",
    "s_cerevisiae",
];

/// One species' codon-usage table. `freqs` are the raw Kazusa relative
/// frequencies (synonymous groups sum to 1); `aa_of` is derived from the
/// standard genetic code; `fmax[aa]` is the max raw frequency within the
/// synonymous group (used as the CAI denominator).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodonUsageTable {
    pub species: String,
    pub freqs: HashMap<String, f64>,
    pub aa_of: HashMap<String, char>,
    pub fmax: HashMap<char, f64>,
}

impl CodonUsageTable {
    fn new(species: String) -> Self {
        CodonUsageTable {
            species,
            freqs: HashMap::new(),
            aa_of: HashMap::new(),
            fmax: HashMap::new(),
        }
    }

    fn finish(&mut self) {
        let mut fmax: HashMap<char, f64> = HashMap::new();
        for (codon, aa) in &self.aa_of {
            let f = self.freqs.get(codon).copied().unwrap_or(0.0).max(0.001);
            let e = fmax.entry(*aa).or_insert(0.001);
            if f > *e {
                *e = f;
            }
        }
        self.fmax = fmax;
    }
}

static TABLES: OnceLock<HashMap<String, CodonUsageTable>> = OnceLock::new();

fn tables() -> &'static HashMap<String, CodonUsageTable> {
    TABLES.get_or_init(|| {
        let mut out: HashMap<String, CodonUsageTable> = HashMap::new();
        for line in CODON_USAGE_TSV.lines().skip(1) {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() != 4 {
                continue;
            }
            let t = out
                .entry(f[0].to_string())
                .or_insert_with(|| CodonUsageTable::new(f[0].to_string()));
            let codon = f[2].to_string();
            let freq: f64 = f[3].parse().unwrap_or(0.0);
            t.freqs.insert(codon.clone(), freq);
            t.aa_of.insert(codon.clone(), aa_of_codon(&codon));
        }
        for t in out.values_mut() {
            t.finish();
        }
        out
    })
}

/// All built-in species keys, in canonical order.
pub fn list_species() -> Vec<&'static str> {
    SPECIES_ORDER.to_vec()
}

/// Look up a built-in table by species key (e.g. `"e_coli"`).
pub fn get_table(species: &str) -> Option<&'static CodonUsageTable> {
    let ts = tables();
    ts.get(species)
        .or_else(|| ts.get(&species.to_lowercase()))
}

/// Build a table from caller-supplied rows (Kazusa-format parse results),
/// e.g. a user-defined custom table. Later duplicate codons win.
pub fn table_from_custom(rows: &[(char, String, f64)]) -> CodonUsageTable {
    let mut t = CodonUsageTable::new("custom".to_string());
    for (aa, codon, freq) in rows {
        let c = codon.to_ascii_uppercase().replace('U', "T");
        t.freqs.insert(c.clone(), *freq);
        t.aa_of.insert(c, *aa);
    }
    t.finish();
    t
}

fn aa_of_codon(codon: &str) -> char {
    let b = codon.as_bytes();
    let (Some(i0), Some(i1), Some(i2)) = (codon_index(b[0]), codon_index(b[1]), codon_index(b[2]))
    else {
        return '?';
    };
    GENETIC_CODE[i0 * 16 + i1 * 4 + i2]
}

// ---------------------------------------------------------------------------
// Codon extraction
// ---------------------------------------------------------------------------

/// A feature's coding region extracted 5'→3', ready for optimization.
#[derive(Debug, Clone)]
pub struct CodingDna {
    pub codons: Vec<String>,
    /// 1-letter translation (includes '*' for stops).
    pub aa: String,
    /// Template spans covering the coding bases, in coding 5'→3' order.
    /// Each span is linear (`start <= end`) with `end - start + 1` bases in
    /// {1,2,3}; the entries of one codon always form contiguous slices of
    /// that codon in order (a codon straddling an exon boundary or the
    /// circular origin is split into two entries, as spec'd). Total length
    /// sums to `3 * codons.len()`. To write back: for each entry consume the
    /// next `end - start + 1` bases of the replacement coding sequence and
    /// write them into `template[start..=end]` — as-is for "+" strand,
    /// reverse-complemented for "-" strand.
    pub segments_on_template: Vec<(usize, usize)>,
}

/// Extract a feature's coding codons from the template sequence.
///
/// Mirrors `translate::translate_feature` (segments joined 5'→3', minus
/// strand reverse-complemented) but errors on ambiguous bases and on a
/// coding length not divisible by 3. A feature crossing the circular origin
/// (represented as `start > end` or a segment with `start > end`) is split
/// into linear pieces first.
pub fn extract_codons(
    project_seq: &str,
    feature: &Feature,
    topology: &str,
) -> Result<CodingDna, String> {
    let bytes = project_seq.as_bytes();
    let n = bytes.len() as i64;
    if n == 0 {
        return Err("empty sequence".to_string());
    }

    let push_seg = |segs: &mut Vec<(i64, i64)>, s: i64, e: i64| {
        if s <= e {
            segs.push((s, e));
        } else {
            if topology != "circular" {
                return Err("feature spans the origin but topology is not circular".to_string());
            }
            segs.push((s, n - 1));
            segs.push((0, e));
        }
        Ok(())
    };
    let mut segs: Vec<(i64, i64)> = Vec::new();
    if feature.segments.is_empty() {
        push_seg(&mut segs, feature.start, feature.end)?;
    } else {
        for seg in &feature.segments {
            push_seg(&mut segs, seg.start, seg.end)?;
        }
    }
    for &(s, e) in &segs {
        if s < 0 || e >= n {
            return Err(format!(
                "feature coordinate {}..{} out of range (1-based inclusive; sequence length {})",
                s + 1,
                e + 1,
                n
            ));
        }
    }

    let minus = feature.strand == "-";
    let mut coding: Vec<(i64, u8)> = Vec::new();
    if minus {
        for &(s, e) in segs.iter().rev() {
            for pos in (s..=e).rev() {
                let b = bytes[pos as usize];
                coding.push((pos, crate::utils::complement_char(b as char) as u8));
            }
        }
    } else {
        for &(s, e) in &segs {
            for pos in s..=e {
                coding.push((pos, bytes[pos as usize]));
            }
        }
    }

    if coding.len() % 3 != 0 {
        return Err(format!("coding length {} not divisible by 3", coding.len()));
    }

    let mut codons = Vec::with_capacity(coding.len() / 3);
    let mut aa = String::with_capacity(coding.len() / 3);
    let mut spans = Vec::with_capacity(coding.len() / 3);
    for chunk in coding.chunks(3) {
        let mut codon = String::with_capacity(3);
        for &(_, b) in chunk {
            let u = b.to_ascii_uppercase();
            if codon_index(u).is_none() {
                return Err(format!("ambiguous base '{}' in coding region", u as char));
            }
            codon.push(u as char);
        }
        // Split the codon's template positions into linear consecutive runs.
        let mut piece_start = 0;
        for j in 1..3 {
            if (chunk[j - 1].0 - chunk[j].0).abs() != 1 {
                let (mut a, mut b) = (chunk[piece_start].0, chunk[j - 1].0);
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                spans.push((a as usize, b as usize));
                piece_start = j;
            }
        }
        let (mut a, mut b) = (chunk[piece_start].0, chunk[2].0);
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        spans.push((a as usize, b as usize));

        codons.push(codon);
        aa.push(aa_of_codon(&codons[codons.len() - 1]));
    }

    Ok(CodingDna {
        codons,
        aa,
        segments_on_template: spans,
    })
}

// ---------------------------------------------------------------------------
// CAI
// ---------------------------------------------------------------------------

fn w_single(codon: &str, table: &CodonUsageTable) -> f64 {
    let Some(aa) = table.aa_of.get(codon).copied() else {
        return 1.0;
    };
    let fmax = table.fmax.get(&aa).copied().unwrap_or(0.001).max(0.001);
    let f = table.freqs.get(codon).copied().unwrap_or(0.0).max(0.001);
    f / fmax
}

/// CAI = exp((1/N) Σ ln w_i) with w_i = freq(codon_i) / fmax(aa_i) and a
/// zero-frequency floor of 0.001. Empty input yields 1.0.
pub fn compute_cai(codons: &[String], table: &CodonUsageTable) -> f64 {
    if codons.is_empty() {
        return 1.0;
    }
    let n = codons.len() as f64;
    let sum: f64 = codons.iter().map(|c| w_single(c, table).max(1e-12).ln()).sum();
    (sum / n).exp()
}

fn gc_content(dna: &[u8]) -> f64 {
    if dna.is_empty() {
        return 0.0;
    }
    let g = dna
        .iter()
        .filter(|&&b| b == b'G' || b == b'C' || b == b'g' || b == b'c')
        .count();
    g as f64 / dna.len() as f64
}

// ---------------------------------------------------------------------------
// Optimization
// ---------------------------------------------------------------------------

/// Optimization strategy (JSON: `use_best_codon` / `match_codon_usage` /
/// `harmonize_rca`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptimizeMethod {
    UseBestCodon,
    MatchCodonUsage,
    HarmonizeRca,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeOptions {
    pub method: OptimizeMethod,
    /// Species table of the original sequence; used by `HarmonizeRca` to
    /// compute each original codon's relative codon adaptation. When absent,
    /// `HarmonizeRca` degrades to `MatchCodonUsage`.
    pub original_table: Option<CodonUsageTable>,
    /// Restriction-site recognition sequences to avoid (IUPAC codes allowed).
    pub avoid_enzyme_sites: Vec<String>,
    /// (window bp, min GC, max GC); `None` disables the GC check.
    pub gc_window: Option<(usize, f64, f64)>,
    /// Max homopolymer run length (default 6).
    pub max_homopolymer: usize,
    /// k-mer length; any k-mer occurring more than once is a violation
    /// (default 12 — long enough that random 4^k collisions are negligible
    /// for gene-length sequences, while real tandem/inverted repeats of
    /// ≥ 12 bp still trigger).
    pub max_repeat: usize,
}

impl Default for OptimizeOptions {
    fn default() -> Self {
        OptimizeOptions {
            method: OptimizeMethod::UseBestCodon,
            original_table: None,
            avoid_enzyme_sites: Vec::new(),
            gc_window: None,
            max_homopolymer: 6,
            max_repeat: 12,
        }
    }
}

/// One applied synonymous replacement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repair {
    pub codon_index: usize,
    pub old: String,
    pub new: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeResult {
    pub new_codons: Vec<String>,
    pub cai_before: f64,
    pub cai_after: f64,
    pub gc_before: f64,
    pub gc_after: f64,
    pub repairs: Vec<Repair>,
    pub unresolved: Vec<String>,
}

/// Optimize codon usage (DNA Chisel port). `original` must already be a
/// valid coding sequence (see [`extract_codons`]); codons whose amino acid
/// is unknown to the table are kept verbatim.
pub fn optimize_codons(
    original: &[String],
    table: &CodonUsageTable,
    opts: &OptimizeOptions,
) -> OptimizeResult {
    let cai_before = compute_cai(original, table);
    let gc_before = gc_content(&join(original));

    let draft: Vec<String> = match opts.method {
        OptimizeMethod::UseBestCodon => original.iter().map(|c| best_codon(c, table)).collect(),
        OptimizeMethod::HarmonizeRca => match &opts.original_table {
            Some(ot) => harmonize_rca(original, ot, table),
            None => match_usage(original, table),
        },
        OptimizeMethod::MatchCodonUsage => match_usage(original, table),
    };

    let (new_codons, repairs, unresolved) = repair(draft, table, opts);

    OptimizeResult {
        cai_after: compute_cai(&new_codons, table),
        gc_after: gc_content(&join(&new_codons)),
        new_codons,
        cai_before,
        gc_before,
        repairs,
        unresolved,
    }
}

/// Reverse translation: build an optimized coding sequence directly from an
/// amino acid string, with no template DNA involved. Accepts the 20 canonical
/// amino acid letters plus `*` for a stop codon (whitespace stripped,
/// lower-case folded); anything else errors with the offending position.
///
/// Draft codons follow `method`: use_best_codon picks the table's most-used
/// codon per amino acid; match_codon_usage (and harmonize_rca, which has no
/// source sequence here) spread each amino acid's positions across the table
/// frequencies. The repair layer then applies as in [`optimize_codons`].
/// `cai_before`/`gc_before` describe the pre-repair draft.
pub fn optimize_from_aa(
    aa: &str,
    table: &CodonUsageTable,
    opts: &OptimizeOptions,
) -> Result<OptimizeResult, String> {
    let cleaned: String = aa
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if cleaned.is_empty() {
        return Err("amino acid sequence is empty".to_string());
    }
    for (i, c) in cleaned.char_indices() {
        if !is_aa_char(c) {
            return Err(format!(
                "invalid amino acid '{}' at position {} (expected a canonical amino acid or '*')",
                c, i
            ));
        }
    }

    let mut draft: Vec<String> = Vec::with_capacity(cleaned.len());
    for c in cleaned.chars() {
        draft.push(best_codon_for_aa(c, table).ok_or_else(|| {
            format!("amino acid '{}' has no codon in the species table", c)
        })?);
    }
    let draft = match opts.method {
        OptimizeMethod::UseBestCodon => draft,
        OptimizeMethod::MatchCodonUsage | OptimizeMethod::HarmonizeRca => {
            match_usage(&draft, table)
        }
    };

    let cai_before = compute_cai(&draft, table);
    let gc_before = gc_content(&join(&draft));
    let (new_codons, repairs, unresolved) = repair(draft, table, opts);

    Ok(OptimizeResult {
        cai_after: compute_cai(&new_codons, table),
        gc_after: gc_content(&join(&new_codons)),
        new_codons,
        cai_before,
        gc_before,
        repairs,
        unresolved,
    })
}

fn is_aa_char(c: char) -> bool {
    matches!(c, 'A' | 'C' | 'D' | 'E' | 'F' | 'G' | 'H' | 'I' | 'K' | 'L' | 'M' | 'N' | 'P'
        | 'Q' | 'R' | 'S' | 'T' | 'V' | 'W' | 'Y' | '*')
}

fn join(codons: &[String]) -> Vec<u8> {
    let mut out = Vec::with_capacity(codons.len() * 3);
    for c in codons {
        out.extend_from_slice(c.as_bytes());
    }
    out
}

fn synonyms(aa: char, table: &CodonUsageTable) -> Vec<String> {
    let mut out: Vec<String> = table
        .freqs
        .iter()
        .filter(|(c, _)| table.aa_of.get(*c) == Some(&aa))
        .map(|(c, _)| c.clone())
        .collect();
    out.sort();
    out
}

fn best_codon(codon: &str, table: &CodonUsageTable) -> String {
    let Some(aa) = table.aa_of.get(codon).copied() else {
        return codon.to_string();
    };
    best_codon_for_aa(aa, table).unwrap_or_else(|| codon.to_string())
}

/// The table's most-used codon for an amino acid (ties → lexicographically
/// smallest), or `None` when the table has no codon for it.
fn best_codon_for_aa(aa: char, table: &CodonUsageTable) -> Option<String> {
    let mut best: Option<(f64, &str)> = None;
    for (c, f) in &table.freqs {
        if table.aa_of.get(c) == Some(&aa) {
            let better = match best {
                None => true,
                Some((bf, bc)) => *f > bf + 1e-12 || ((f - bf).abs() <= 1e-12 && c.as_str() < bc),
            };
            if better {
                best = Some((*f, c.as_str()));
            }
        }
    }
    best.map(|(_, c)| c.to_string())
}

/// Relative codon adaptation: freq / group max.
fn rca(codon: &str, table: &CodonUsageTable) -> f64 {
    let f = table.freqs.get(codon).copied().unwrap_or(0.0).max(0.001);
    let aa = table.aa_of.get(codon).copied().unwrap_or('?');
    let fmax = table.fmax.get(&aa).copied().unwrap_or(0.001).max(0.001);
    f / fmax
}

/// For each codon, pick the synonymous codon whose RCA in the target table
/// is closest to the original codon's RCA in its own table.
fn harmonize_rca(original: &[String], orig_table: &CodonUsageTable, table: &CodonUsageTable) -> Vec<String> {
    original
        .iter()
        .map(|c| {
            let Some(aa) = table.aa_of.get(c).copied() else {
                return c.clone();
            };
            let rca_orig = rca(c, orig_table);
            let mut best: Option<(f64, String)> = None;
            for (cand, freq) in &table.freqs {
                if table.aa_of.get(cand) != Some(&aa) {
                    continue;
                }
                let fmax = table.fmax.get(&aa).copied().unwrap_or(0.001).max(0.001);
                let rca_t = *freq / fmax;
                let d = (rca_t - rca_orig).abs();
                match &best {
                    None => best = Some((d, cand.clone())),
                    Some((bd, bc)) => {
                        if d < *bd - 1e-12 || ((d - *bd).abs() <= 1e-12 && cand < bc) {
                            best = Some((d, cand.clone()));
                        }
                    }
                }
            }
            best.map(|(_, c)| c).unwrap_or_else(|| c.clone())
        })
        .collect()
}

/// MatchCodonUsage: minimize Σ_aa Σ_c |count_c − n_aa·f_table(c)|. Initial
/// draft assigns each amino acid's positions proportionally to the table
/// frequencies (floor + remainder to the max-freq codon), then a greedy
/// best-improvement local search runs until convergence (≤ 2000 rounds).
fn match_usage(codons: &[String], table: &CodonUsageTable) -> Vec<String> {
    let mut cur = codons.to_vec();
    let mut by_aa: HashMap<char, Vec<usize>> = HashMap::new();
    for (i, c) in codons.iter().enumerate() {
        if let Some(aa) = table.aa_of.get(c) {
            by_aa.entry(*aa).or_default().push(i);
        }
    }
    let positions_of_aa: HashMap<char, Vec<usize>> = by_aa.clone();

    for (aa, positions) in by_aa.iter() {
        let n = positions.len();
        let mut codons_aa: Vec<(String, f64)> = table
            .freqs
            .iter()
            .filter(|(c, _)| table.aa_of.get(*c) == Some(aa))
            .map(|(c, f)| (c.clone(), *f))
            .collect();
        codons_aa.sort_by(|a, b| a.0.cmp(&b.0));
        let mut rem_counts: Vec<usize> = codons_aa
            .iter()
            .map(|(_, f)| (f * n as f64).floor() as usize)
            .collect();
        let sum: usize = rem_counts.iter().sum();
        let leftover = n - sum;
        if leftover > 0 {
            let mut best_i = 0;
            for j in 1..codons_aa.len() {
                if codons_aa[j].1 > codons_aa[best_i].1
                    || (codons_aa[j].1 == codons_aa[best_i].1 && codons_aa[j].0 < codons_aa[best_i].0)
                {
                    best_i = j;
                }
            }
            rem_counts[best_i] += leftover;
        }
        // Deterministic spread: each position takes the codon with the most
        // remaining count (ties → lexicographically smallest codon).
        let mut rem = rem_counts;
        let mut assigned: Vec<String> = Vec::with_capacity(n);
        for _ in 0..n {
            let mut best_j = 0;
            for j in 1..codons_aa.len() {
                if rem[j] > rem[best_j]
                    || (rem[j] == rem[best_j] && codons_aa[j].0 < codons_aa[best_j].0)
                {
                    best_j = j;
                }
            }
            assigned.push(codons_aa[best_j].0.clone());
            rem[best_j] -= 1;
        }
        for (pos, c) in positions.iter().zip(assigned) {
            cur[*pos] = c;
        }
    }

    let mut counts: HashMap<(char, String), usize> = HashMap::new();
    for c in &cur {
        if let Some(aa) = table.aa_of.get(c) {
            *counts.entry((*aa, c.clone())).or_insert(0) += 1;
        }
    }

    for _ in 0..2000 {
        let mut best: Option<(f64, usize, String, String)> = None;
        for (i, c) in cur.iter().enumerate() {
            let Some(aa) = table.aa_of.get(c).copied() else {
                continue;
            };
            let n_aa = positions_of_aa.get(&aa).map(|v| v.len()).unwrap_or(0) as f64;
            let t_c = n_aa * table.freqs.get(c).copied().unwrap_or(0.0);
            let cnt_c = counts.get(&(aa, c.clone())).copied().unwrap_or(0) as f64;
            let mut cands: Vec<String> = table
                .freqs
                .iter()
                .filter(|(cc, _)| table.aa_of.get(*cc) == Some(&aa) && *cc != c)
                .map(|(cc, _)| cc.clone())
                .collect();
            cands.sort();
            for cand in cands {
                let t_cand = n_aa * table.freqs.get(&cand).copied().unwrap_or(0.0);
                let cnt_cand = counts.get(&(aa, cand.clone())).copied().unwrap_or(0) as f64;
                let before = (cnt_c - t_c).abs() + (cnt_cand - t_cand).abs();
                let after = ((cnt_c - 1.0) - t_c).abs() + ((cnt_cand + 1.0) - t_cand).abs();
                let imp = before - after;
                if imp > 1e-9 {
                    let better = match &best {
                        None => true,
                        Some((bimp, bi, bc, _)) => {
                            imp > *bimp + 1e-12
                                || ((imp - bimp).abs() <= 1e-12 && (i < *bi || (i == *bi && cand < *bc)))
                        }
                    };
                    if better {
                        best = Some((imp, i, c.clone(), cand));
                    }
                }
            }
        }
        match best {
            None => break,
            Some((_, i, old, new)) => {
                let aa = table.aa_of[&cur[i]];
                *counts.get_mut(&(aa, old)).unwrap() -= 1;
                *counts.entry((aa, new.clone())).or_insert(0) += 1;
                cur[i] = new;
            }
        }
    }
    cur
}

// ---------------------------------------------------------------------------
// Repair layer (shared by all methods)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Violation {
    start: usize,
    end: usize,
    reason: &'static str,
}

struct RepairParams<'a> {
    max_homopolymer: usize,
    k: usize,
    sites: Vec<Vec<u8>>,
    gc_window: Option<(usize, f64, f64)>,
    radius: usize,
    table: &'a CodonUsageTable,
}

fn revcomp_bytes(seq: &[u8]) -> Vec<u8> {
    seq.iter()
        .rev()
        .map(|&b| match b {
            b'A' => b'T',
            b'T' => b'A',
            b'G' => b'C',
            b'C' => b'G',
            _ => b'N',
        })
        .collect()
}

fn iupac_matches(seq: &[u8], pat: &[u8]) -> bool {
    seq.iter()
        .zip(pat.iter())
        .all(|(&s, &p)| crate::primer::iupac::bases_overlap(s, p))
}

fn mark_homopolymer(
    seg: &[u8],
    offset: usize,
    hp: usize,
    marked: &mut [bool],
    mut violations: Option<&mut Vec<Violation>>,
) {
    if hp == 0 || seg.is_empty() {
        return;
    }
    let mut run_start = 0;
    for i in 1..=seg.len() {
        if i < seg.len() && seg[i] == seg[i - 1] {
            continue;
        }
        let len = i - run_start;
        if len > hp {
            for j in run_start..i {
                marked[j] = true;
            }
            if let Some(v) = violations.as_deref_mut() {
                v.push(Violation {
                    start: offset + run_start,
                    end: offset + i - 1,
                    reason: "homopolymer",
                });
            }
        }
        run_start = i;
    }
}

fn mark_kmers(
    seg: &[u8],
    offset: usize,
    k: usize,
    mult: &dyn Fn(&str, usize) -> usize,
    marked: &mut [bool],
    mut violations: Option<&mut Vec<Violation>>,
) {
    if k == 0 || seg.len() < k {
        return;
    }
    for s in 0..=seg.len() - k {
        let kmer = std::str::from_utf8(&seg[s..s + k]).unwrap();
        if mult(kmer, offset + s) >= 2 {
            for j in s..s + k {
                marked[j] = true;
            }
            if let Some(v) = violations.as_deref_mut() {
                v.push(Violation {
                    start: offset + s,
                    end: offset + s + k - 1,
                    reason: "repeat",
                });
            }
        }
    }
}

fn mark_enzyme(
    seg: &[u8],
    offset: usize,
    sites: &[Vec<u8>],
    marked: &mut [bool],
    mut violations: Option<&mut Vec<Violation>>,
) {
    if sites.is_empty() {
        return;
    }
    for pat in sites {
        let plen = pat.len();
        if plen == 0 || plen > seg.len() {
            continue;
        }
        for s in 0..=seg.len() - plen {
            if iupac_matches(&seg[s..s + plen], pat) {
                for j in s..s + plen {
                    marked[j] = true;
                }
                if let Some(v) = violations.as_deref_mut() {
                    v.push(Violation {
                        start: offset + s,
                        end: offset + s + plen - 1,
                        reason: "enzyme_site",
                    });
                }
            }
        }
        let rc = revcomp_bytes(seg);
        for s in 0..=rc.len() - plen {
            if iupac_matches(&rc[s..s + plen], pat) {
                let a = seg.len() - plen - s;
                let b = seg.len() - 1 - s;
                for j in a..=b {
                    marked[j] = true;
                }
                if let Some(v) = violations.as_deref_mut() {
                    v.push(Violation {
                        start: offset + a,
                        end: offset + b,
                        reason: "enzyme_site",
                    });
                }
            }
        }
    }
}

fn mark_gc(
    seg: &[u8],
    offset: usize,
    gc: Option<(usize, f64, f64)>,
    marked: &mut [bool],
    mut violations: Option<&mut Vec<Violation>>,
) {
    let Some((win, min, max)) = gc else {
        return;
    };
    if win == 0 || win > seg.len() {
        return;
    }
    for s in 0..=seg.len() - win {
        let g = seg[s..s + win]
            .iter()
            .filter(|&&b| b == b'G' || b == b'C')
            .count();
        let frac = g as f64 / win as f64;
        if frac < min || frac > max {
            for j in s..s + win {
                marked[j] = true;
            }
            if let Some(v) = violations.as_deref_mut() {
                v.push(Violation {
                    start: offset + s,
                    end: offset + s + win - 1,
                    reason: "gc_window",
                });
            }
        }
    }
}

fn mark_all(
    seg: &[u8],
    offset: usize,
    params: &RepairParams,
    mult: &dyn Fn(&str, usize) -> usize,
    marked: &mut [bool],
    mut violations: Option<&mut Vec<Violation>>,
) {
    mark_homopolymer(seg, offset, params.max_homopolymer, marked, violations.as_deref_mut());
    mark_kmers(seg, offset, params.k, mult, marked, violations.as_deref_mut());
    mark_enzyme(seg, offset, &params.sites, marked, violations.as_deref_mut());
    mark_gc(seg, offset, params.gc_window, marked, violations.as_deref_mut());
}

fn build_kmer_counts(dna: &[u8], k: usize) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    if k == 0 || dna.len() < k {
        return out;
    }
    for s in 0..=dna.len() - k {
        *out.entry(String::from_utf8_lossy(&dna[s..s + k]).into_owned())
            .or_insert(0) += 1;
    }
    out
}

/// Full-sequence violation scan: returns per-base flags and the violation
/// regions (one per offending occurrence) for reporting.
fn detect_all(
    dna: &[u8],
    params: &RepairParams,
    counts: &HashMap<String, usize>,
) -> (Vec<bool>, Vec<Violation>) {
    let mut marked = vec![false; dna.len()];
    let mut violations = Vec::new();
    let mult = |kmer: &str, _s: usize| counts.get(kmer).copied().unwrap_or(0);
    mark_all(dna, 0, params, &mult, &mut marked, Some(&mut violations));
    (marked, violations)
}

/// Evaluate one synonymous replacement at codon `p`: returns the number of
/// violation-flagged bases eliminated in the local window around the change
/// (negative when the change creates violations) and the CAI weight ratio
/// w(new)/w(old). The window radius covers the reach of every detector, so
/// positions outside it cannot change status; edge inaccuracies cancel out
/// of the before/after difference.
fn eval_candidate(
    joined: &[u8],
    p: usize,
    old: &str,
    cand: &str,
    params: &RepairParams,
    counts: &HashMap<String, usize>,
) -> (isize, f64) {
    let len = joined.len();
    let lo = (3 * p).saturating_sub(params.radius);
    let hi = (3 * p + 3 + params.radius).min(len);
    let seg = &joined[lo..hi];
    let mut seg2 = seg.to_vec();
    let cbytes = cand.as_bytes();
    for j in 0..3 {
        seg2[3 * p - lo + j] = cbytes[j];
    }

    let k = params.k;
    let mut orig_occ: HashMap<String, usize> = HashMap::new();
    let mut cand_occ: HashMap<String, usize> = HashMap::new();
    if k > 0 && k <= len {
        let s0 = (3 * p + 3).saturating_sub(k).max(lo);
        for t in s0..3 * p + 3 {
            if t + k > len {
                break;
            }
            let rs = t - lo;
            let o = std::str::from_utf8(&seg[rs..rs + k]).unwrap();
            let c = std::str::from_utf8(&seg2[rs..rs + k]).unwrap();
            *orig_occ.entry(o.to_string()).or_insert(0) += 1;
            *cand_occ.entry(c.to_string()).or_insert(0) += 1;
        }
    }

    let count = |s: &[u8], mult: &dyn Fn(&str, usize) -> usize| {
        let mut marked = vec![false; s.len()];
        mark_all(s, lo, params, mult, &mut marked, None);
        marked.iter().filter(|&&m| m).count()
    };

    // Before: base counts already reflect the current sequence.
    let mult_before = |kmer: &str, _s: usize| counts.get(kmer).copied().unwrap_or(0);
    let before = count(seg, &mult_before);

    // After: k-mers overlapping the changed bases get the adjusted
    // multiplicity (base + occurrences created − occurrences removed).
    let mult_after = |kmer: &str, s: usize| {
        let base = counts.get(kmer).copied().unwrap_or(0);
        if k > 0 && s < 3 * p + 3 && s + k > 3 * p {
            base + cand_occ.get(kmer).copied().unwrap_or(0) - orig_occ.get(kmer).copied().unwrap_or(0)
        } else {
            base
        }
    };
    let after = count(&seg2, &mult_after);

    let wratio = {
        let wo = w_single(old, params.table).max(1e-12);
        w_single(cand, params.table) / wo
    };
    (before as isize - after as isize, wratio)
}

/// Best synonymous fix for one violated codon during a repair round:
/// the replacement maximizing (violations eliminated, then CAI weight
/// ratio, then lexicographic order); `None` when no replacement helps.
fn best_fix_for_position(
    joined: &[u8],
    p: usize,
    old: &str,
    aa: char,
    params: &RepairParams,
    counts: &HashMap<String, usize>,
) -> Option<(String, isize, f64)> {
    let mut best: Option<(String, isize, f64)> = None;
    for cand in synonyms(aa, params.table) {
        if cand.as_str() == old {
            continue;
        }
        let (elim, wratio) = eval_candidate(joined, p, old, &cand, params, counts);
        if elim > 0 {
            let better = match &best {
                None => true,
                Some((bc, belim, bw)) => {
                    elim > *belim
                        || (elim == *belim
                            && (wratio > *bw + 1e-12
                                || ((wratio - *bw).abs() <= 1e-12 && cand.as_str() < bc.as_str())))
                }
            };
            if better {
                best = Some((cand, elim, wratio));
            }
        }
    }
    best
}

/// A position whose violation survives this many consecutive unfixable
/// rounds is abandoned (recorded in `unresolved`) instead of being retried
/// forever.
const STRIKE_LIMIT: usize = 3;

fn repair(
    draft: Vec<String>,
    table: &CodonUsageTable,
    opts: &OptimizeOptions,
) -> (Vec<String>, Vec<Repair>, Vec<String>) {
    let mut cur = draft;
    let hp = opts.max_homopolymer.max(1);
    let k = opts.max_repeat;
    let sites: Vec<Vec<u8>> = opts
        .avoid_enzyme_sites
        .iter()
        .map(|s| s.trim().to_ascii_uppercase().into_bytes())
        .filter(|s| !s.is_empty())
        .collect();
    let site_max = sites.iter().map(|s| s.len()).max().unwrap_or(0);
    let win = opts.gc_window.map(|(w, _, _)| w).unwrap_or(0);
    let params = RepairParams {
        max_homopolymer: hp,
        k,
        sites,
        gc_window: opts.gc_window,
        radius: hp.max(k).max(site_max).max(win) * 2 + 16,
        table,
    };

    let mut repairs = Vec::new();
    let mut counts = build_kmer_counts(&join(&cur), k);
    let mut strikes = vec![0usize; cur.len()];

    for _ in 0..500 {
        let joined = join(&cur);
        let (marked, violations) = detect_all(&joined, &params, &counts);
        if !marked.iter().any(|&m| m) {
            break;
        }
        let mut in_vc = vec![false; cur.len()];
        let mut vc: Vec<usize> = Vec::new();
        for p in 0..cur.len() {
            if marked[3 * p] || marked[3 * p + 1] || marked[3 * p + 2] {
                vc.push(p);
                in_vc[p] = true;
            }
        }
        // Positions no longer violated stop accumulating strikes.
        for (p, s) in strikes.iter_mut().enumerate() {
            if !in_vc[p] {
                *s = 0;
            }
        }
        // Per-position best fix; unfixable positions accumulate strikes.
        let mut fixes: Vec<(usize, String, isize, f64, &'static str)> = Vec::new();
        for &p in &vc {
            if strikes[p] >= STRIKE_LIMIT {
                continue;
            }
            let reason = violations
                .iter()
                .find(|v| v.start <= 3 * p + 2 && v.end >= 3 * p)
                .map(|v| v.reason)
                .unwrap_or("unknown");
            let Some(aa) = table.aa_of.get(&cur[p]).copied() else {
                strikes[p] += 1;
                continue;
            };
            match best_fix_for_position(&joined, p, &cur[p], aa, &params, &counts) {
                Some((cand, elim, wratio)) => {
                    strikes[p] = 0;
                    fixes.push((p, cand, elim, wratio, reason));
                }
                None => strikes[p] += 1,
            }
        }
        // Apply fixes in position order, each verified against the current
        // working state by an exact global violation scan. Local-window
        // evals miss far-side k-mer duplicates (fixing one occurrence of a
        // repeated k-mer can re-duplicate a distant one), which caused
        // churn; a fix is committed only when it strictly reduces the
        // global violation count, otherwise it is rolled back and counts as
        // a strike toward giving up on that position.
        fixes.sort_by_key(|f| f.0);
        let mut working = cur.clone();
        let mut working_marked = marked.iter().filter(|&&m| m).count();
        let mut committed = 0;
        for (p, cand, _, _, reason) in fixes {
            if strikes[p] >= STRIKE_LIMIT {
                continue;
            }
            let mut tjoined = join(&working);
            tjoined[3 * p..3 * p + 3].copy_from_slice(cand.as_bytes());
            let tcounts = build_kmer_counts(&tjoined, k);
            let (tmarked, _) = detect_all(&tjoined, &params, &tcounts);
            let tc = tmarked.iter().filter(|&&m| m).count();
            if tc < working_marked {
                working[p] = cand.clone();
                working_marked = tc;
                strikes[p] = 0;
                repairs.push(Repair {
                    codon_index: p,
                    old: cur[p].clone(),
                    new: cand,
                    reason: reason.to_string(),
                });
                committed += 1;
            } else {
                strikes[p] += 1;
            }
        }
        if committed == 0 {
            break;
        }
        cur = working;
        counts = build_kmer_counts(&join(&cur), k);
    }

    let joined = join(&cur);
    let (_, violations) = detect_all(&joined, &params, &counts);
    let unresolved: Vec<String> = violations
        .iter()
        .map(|v| format!("{} {}..{}", v.reason, v.start, v.end))
        .collect();
    (cur, repairs, unresolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feat(id: &str, start: i64, end: i64, strand: &str, segments: Vec<(i64, i64)>) -> Feature {
        Feature {
            id: id.to_string(),
            name: id.to_string(),
            start,
            end,
            color: "#000000".to_string(),
            ftype: "CDS".to_string(),
            segments: segments
                .into_iter()
                .map(|(start, end)| crate::models::Segment {
                    start,
                    end,
                    color: None,
                })
                .collect(),
            strand: strand.to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        }
    }

    fn codons(s: &str) -> Vec<String> {
        s.as_bytes()
            .chunks(3)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect()
    }

    fn translate(c: &[String]) -> String {
        c.iter().map(|x| aa_of_codon(x)).collect()
    }

    /// Σ_aa Σ_c |count_c − n_aa·f_table(c)| — the MatchCodonUsage objective.
    fn l1_distance(codons: &[String], table: &CodonUsageTable) -> f64 {
        let mut by_aa: HashMap<char, Vec<&String>> = HashMap::new();
        for c in codons {
            if let Some(aa) = table.aa_of.get(c) {
                by_aa.entry(*aa).or_default().push(c);
            }
        }
        let mut total = 0.0;
        for (aa, v) in by_aa {
            let n = v.len() as f64;
            let mut cnt: HashMap<&str, usize> = HashMap::new();
            for c in v {
                *cnt.entry(c.as_str()).or_insert(0) += 1;
            }
            for (c, f) in &table.freqs {
                if table.aa_of.get(c) == Some(&aa) {
                    let cnt = *cnt.get(c.as_str()).unwrap_or(&0) as f64;
                    total += (cnt - n * f).abs();
                }
            }
        }
        total
    }

    // ------------------------------------------------------------------
    // Table loading
    // ------------------------------------------------------------------

    #[test]
    fn builtin_tables_complete_and_normalized() {
        assert_eq!(list_species().len(), 9);
        for s in list_species() {
            let t = get_table(s).unwrap_or_else(|| panic!("missing species {s}"));
            assert_eq!(t.species, s);
            assert_eq!(t.freqs.len(), 64, "species {s} must cover all 64 codons");
            assert_eq!(t.aa_of.len(), 64);
            // synonymous groups normalize to ≈ 1
            let mut by_aa: HashMap<char, f64> = HashMap::new();
            for (c, aa) in &t.aa_of {
                *by_aa.entry(*aa).or_insert(0.0) += t.freqs.get(c).copied().unwrap_or(0.0);
            }
            for (aa, sum) in by_aa {
                assert!(
                    (sum - 1.0).abs() < 0.02,
                    "species {s} aa {aa} sums to {sum}"
                );
            }
            // all codons T-form (no U)
            assert!(!t.freqs.keys().any(|c| c.contains('U')));
        }
    }

    #[test]
    fn ecoli_e_group_gaa_dominates() {
        let t = get_table("e_coli").unwrap();
        assert!(t.freqs["GAA"] > t.freqs["GAG"]);
        assert_eq!(t.aa_of["GAA"], 'E');
        assert_eq!(t.aa_of["ATG"], 'M');
        assert_eq!(t.aa_of["TGA"], '*');
        // group max: E → GAA
        assert!((t.fmax[&'E'] - t.freqs["GAA"]).abs() < 1e-12);
    }

    #[test]
    fn custom_table_build() {
        let t = table_from_custom(&[
            ('E', "GAA".into(), 0.9),
            ('E', "gag".into(), 0.1),
            ('M', "AUG".into(), 1.0),
        ]);
        assert_eq!(t.species, "custom");
        assert_eq!(t.freqs["GAA"], 0.9);
        assert!((t.fmax[&'E'] - 0.9).abs() < 1e-12);
        assert_eq!(t.aa_of["GAG"], 'E');
        assert_eq!(t.aa_of["ATG"], 'M');
    }

    // ------------------------------------------------------------------
    // CAI
    // ------------------------------------------------------------------

    #[test]
    fn cai_hand_computed() {
        let t = get_table("e_coli").unwrap();
        // GAG: w = 0.31 / 0.69
        let expect = t.freqs["GAG"] / t.freqs["GAA"];
        let got = compute_cai(&[codons("GAG")[0].clone()], t);
        assert!((got - expect).abs() < 1e-9, "{got} vs {expect}");
        // all-best codons → 1.0
        assert!((compute_cai(&codons("GAACTG"), t) - 1.0).abs() < 1e-9);
        // empty → 1.0
        assert_eq!(compute_cai(&[], t), 1.0);
    }

    #[test]
    fn cai_zero_frequency_floor() {
        let t = table_from_custom(&[('E', "GAA".into(), 0.9), ('E', "GAG".into(), 0.0)]);
        let got = compute_cai(&[codons("GAG")[0].clone()], &t);
        let expect = 0.001 / 0.9;
        assert!((got - expect).abs() < 1e-12, "{got} vs {expect}");
    }

    // ------------------------------------------------------------------
    // Optimization methods
    // ------------------------------------------------------------------

    #[test]
    fn use_best_codon_raises_cai_keeps_translation() {
        let t = get_table("e_coli").unwrap();
        let original = codons("GAGGAGGAG");
        let res = optimize_codons(&original, t, &OptimizeOptions::default());
        assert_eq!(res.new_codons.len(), original.len());
        assert_eq!(translate(&res.new_codons), translate(&original), "translation must be preserved");
        assert_eq!(res.new_codons, vec!["GAA", "GAA", "GAA"]);
        assert!(res.cai_after > res.cai_before + 1e-9);
        assert!(res.unresolved.is_empty());
    }

    #[test]
    fn match_codon_usage_distribution_closer_to_table() {
        let t = get_table("e_coli").unwrap();
        let egfp = "ATGGTGAGCAAGGGCGAGGAGCTGTTCACCGGGGTGGTGCCCATCCTGGTCGAGCTGGACGGCGACGTAAACGGCCACAAGTTCAGCGTGTCCGGCGAGGGCGAGGGCGATGCCACCTACGGCAAGCTGACCCTGAAGTTCATCTGCACCACCGGCAAGCTGCCCGTGCCCTGGCCCACCCTCGTGACCACCCTGACCTACGGCGTGCAGTGCTTCAGCCGCTACCCCGACCACATGAAGCAGCACGACTTCTTCAAGTCCGCCATGCCCGAAGGCTACGTCCAGGAGCGCACCATCTTCTTCAAGGACGACGGCAACTACAAGACCCGCGCCGAGGTGAAGTTCGAGGGCGACACCCTGGTGAACCGCATCGAGCTGAAGGGCATCGACTTCAAGGAGGACGGCAACATCCTGGGGCACAAGCTGGAGTACAACTACAACAGCCACAACGTCTATATCATGGCCGACAAGCAGAAGAACGGCATCAAGGTGAACTTCAAGATCCGCCACAACATCGAGGACGGCAGCGTGCAGCTCGCCGACCACTACCAGCAGAACACCCCCATCGGCGACGGCCCCGTGCTGCTGCCCGACAACCACTACCTGAGCACCCAGTCCGCCCTGAGCAAAGACCCCAACGAGAAGCGCGATCACATGGTCCTGCTGGAGTTCGTGACCGCCGCCGGGATCACTCTCGGCATGGACGAGCTGTACAAGTAA";
        let original = codons(egfp);
        let opts = OptimizeOptions {
            method: OptimizeMethod::MatchCodonUsage,
            ..OptimizeOptions::default()
        };
        let res = optimize_codons(&original, t, &opts);
        assert_eq!(res.new_codons.len(), original.len());
        assert_eq!(translate(&res.new_codons), translate(&original));
        let d_best = l1_distance(&{
            let mut o = OptimizeOptions::default();
            o.method = OptimizeMethod::UseBestCodon;
            optimize_codons(&original, t, &o).new_codons
        }, t);
        let d_match = l1_distance(&res.new_codons, t);
        let d_orig = l1_distance(&original, t);
        assert!(d_match < d_best, "match {d_match} vs best {d_best}");
        assert!(d_match < d_orig, "match {d_match} vs original {d_orig}");
        assert!(res.unresolved.is_empty());
    }

    #[test]
    fn harmonize_rca_falls_back_to_match_usage() {
        let t = get_table("e_coli").unwrap();
        let original = codons("GAGGAGGAGGAAGAA");
        // no original table → degrades to match usage; still valid + translation kept
        let res = optimize_codons(&original, t, &OptimizeOptions {
            method: OptimizeMethod::HarmonizeRca,
            ..OptimizeOptions::default()
        });
        assert_eq!(res.new_codons.len(), original.len());
        assert_eq!(translate(&res.new_codons), translate(&original));
    }

    #[test]
    fn harmonize_rca_with_original_table() {
        let t = get_table("e_coli").unwrap();
        let ot = get_table("h_sapiens").unwrap();
        let original = codons("CTGGTGAGCAAGAAA"); // L V S K K
        let res = optimize_codons(&original, t, &OptimizeOptions {
            method: OptimizeMethod::HarmonizeRca,
            original_table: Some(ot.clone()),
            ..OptimizeOptions::default()
        });
        assert_eq!(res.new_codons.len(), original.len());
        assert_eq!(translate(&res.new_codons), translate(&original));
        // per codon: the chosen codon's target-table RCA must be no farther
        // from the original codon's source-table RCA than the original codon
        // itself is
        for (i, (old, new)) in original.iter().zip(&res.new_codons).enumerate() {
            let rca_orig = rca(old, &ot);
            let d_chosen = (rca(new, t) - rca_orig).abs();
            let d_original = (rca(old, t) - rca_orig).abs();
            assert!(d_chosen <= d_original + 1e-9, "position {i}: {old} -> {new}");
        }
    }

    #[test]
    fn match_codon_usage_trades_cai_for_distribution() {
        // On an already-optimal (all-best-codon) sequence, match_usage spreads
        // codons per table frequencies: the L1 distribution distance improves
        // while CAI drops — expected, since distribution matching is not CAI
        // maximization.
        let t = get_table("e_coli").unwrap();
        let egfp_aa = "MVSKGEELFTGVVPILVELDGDVNGHKFSVSGEGEGDATYGKLTLKFICTTGKLPVPWPTLVTTLTYGVQCFSRYPDHMKQHDFFKSAMPEGYVQERTIFFKDDGNYKTRAEVKFEGDTLVNRIELKGIDFKEDGNILGHKLEYNYNSHNVYIMADKQKNGIKVNFKIRHNIEDGSVQLADHYQQNTPIGDGPVLLPDNHYLSTQSALSKDPNEKRDHMVLLEFVTAAGITLGMDELYK";
        let mut original: Vec<String> = Vec::new();
        for aa in egfp_aa.chars() {
            let mut best: Option<(String, f64)> = None;
            for (c, f) in &t.freqs {
                if t.aa_of.get(c) == Some(&aa) {
                    match &best {
                        None => best = Some((c.clone(), *f)),
                        Some((bc, bf)) => {
                            if *f > *bf || (*f == *bf && c < bc) {
                                best = Some((c.clone(), *f));
                            }
                        }
                    }
                }
            }
            original.push(best.expect("aa in genetic code").0);
        }
        let res = optimize_codons(&original, t, &OptimizeOptions {
            method: OptimizeMethod::MatchCodonUsage,
            ..OptimizeOptions::default()
        });
        assert_eq!(res.new_codons.len(), original.len());
        assert_eq!(translate(&res.new_codons), translate(&original));
        assert!(
            res.cai_after < res.cai_before - 1e-9,
            "cai_after {} should drop below cai_before {}",
            res.cai_after,
            res.cai_before
        );
        assert!(l1_distance(&res.new_codons, t) < l1_distance(&original, t));
    }

    // ------------------------------------------------------------------
    // Reverse translation (aa → optimized DNA)
    // ------------------------------------------------------------------

    #[test]
    fn reverse_translate_use_best_codon_matches_table() {
        let t = get_table("e_coli").unwrap();
        // Deliberately non-repetitive aa string so the repair layer has
        // nothing to fix and the all-best-codon draft survives verbatim.
        let aa = "MWTALSKRGVEVKQNFEDHIPLGRDCYSA";
        let res = optimize_from_aa(aa, t, &OptimizeOptions::default()).unwrap();
        assert_eq!(res.new_codons.len(), aa.len());
        for (c, a) in res.new_codons.iter().zip(aa.chars()) {
            assert_eq!(*c, best_codon_for_aa(a, t).unwrap(), "aa {a}");
        }
        assert_eq!(translate(&res.new_codons), aa, "translation must match input");
        // all best codons → CAI 1.0
        assert!((res.cai_after - 1.0).abs() < 1e-9);
        assert!(res.unresolved.is_empty(), "unresolved: {:?}", res.unresolved);
    }

    #[test]
    fn reverse_translate_match_usage_spreads_per_table() {
        let t = get_table("s_cerevisiae").unwrap();
        let aa = "MKKKKKKKKKKKKKKKKKKKK";
        let res = optimize_from_aa(aa, t, &OptimizeOptions {
            method: OptimizeMethod::MatchCodonUsage,
            ..OptimizeOptions::default()
        }).unwrap();
        assert_eq!(res.new_codons.len(), aa.len());
        assert_eq!(translate(&res.new_codons), aa);
        // match_usage spreads K across AAG/AAA; a pure best-codon draft would
        // be all one codon (K → AAG in s_cerevisiae), so the result must use
        // at least two distinct K codons.
        let mut seen = std::collections::HashSet::new();
        for c in &res.new_codons {
            seen.insert(c.as_str());
        }
        assert!(seen.len() >= 2, "match_usage must spread codons: {seen:?}");
    }

    #[test]
    fn reverse_translate_accepts_trailing_stop_and_lowercase() {
        let t = get_table("e_coli").unwrap();
        let res = optimize_from_aa("mvskgeeft*", t, &OptimizeOptions::default()).unwrap();
        assert_eq!(res.new_codons.len(), 10);
        assert_eq!(translate(&res.new_codons), "MVSKGEEFT*");
        let stop = res.new_codons.last().unwrap();
        assert_eq!(t.aa_of[stop], '*');
    }

    #[test]
    fn reverse_translate_rejects_invalid_and_missing_aa() {
        let t = get_table("e_coli").unwrap();
        assert!(optimize_from_aa("MVSKX", t, &OptimizeOptions::default()).is_err());
        assert!(optimize_from_aa("B", t, &OptimizeOptions::default()).is_err());
        assert!(optimize_from_aa("", t, &OptimizeOptions::default()).is_err());
        assert!(optimize_from_aa("   \n ", t, &OptimizeOptions::default()).is_err());
    }

    #[test]
    fn reverse_translate_repair_keeps_translation() {
        let t = get_table("e_coli").unwrap();
        // All-lysine protein: best codon AAA produces a homopolymer run that
        // the repair layer must break without changing the translation.
        let aa = "MKKKKKKKK";
        let res = optimize_from_aa(aa, t, &OptimizeOptions::default()).unwrap();
        assert_eq!(translate(&res.new_codons), aa);
        let dna = String::from_utf8(join(&res.new_codons)).unwrap();
        let mut run = 1;
        let mut max_run = 1;
        for b in dna.as_bytes().windows(2) {
            if b[0] == b[1] {
                run += 1;
                max_run = max_run.max(run);
            } else {
                run = 1;
            }
        }
        assert!(max_run <= 6, "max homopolymer run {max_run}: {dna}");
    }

    // ------------------------------------------------------------------
    // Repair layer
    // ------------------------------------------------------------------

    #[test]
    fn repair_breaks_homopolymers_and_keeps_translation() {
        let t = get_table("e_coli").unwrap();
        // M + 4×K: use_best_codon drafts a 12 bp poly-A run plus repeated
        // 8-mers. Synonymous AAA→AAG swaps must break both while keeping the
        // translation identical. (Longer K-runs are unfixable in general —
        // K has only two codons — and would land in `unresolved`.)
        let original = codons(&format!("ATG{}", "A".repeat(12)));
        assert_eq!(original.len(), 5);
        let res = optimize_codons(&original, t, &OptimizeOptions::default());
        assert_eq!(res.new_codons.len(), original.len());
        assert_eq!(translate(&res.new_codons), translate(&original));
        assert!(res.unresolved.is_empty(), "unresolved: {:?}", res.unresolved);
        assert!(!res.repairs.is_empty());
        let dna = String::from_utf8(join(&res.new_codons)).unwrap();
        let mut run = 1;
        let mut max_run = 1;
        for b in dna.as_bytes().windows(2) {
            if b[0] == b[1] {
                run += 1;
                max_run = max_run.max(run);
            } else {
                run = 1;
            }
        }
        assert!(max_run <= 6, "max homopolymer run {max_run}: {dna}");
        let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for s in 0..=dna.len() - 12 {
            *seen.entry(&dna[s..s + 12]).or_insert(0) += 1;
        }
        assert!(seen.values().all(|&c| c == 1), "repeated 12-mers: {seen:?}");
    }

    #[test]
    fn repair_removes_repeated_kmers() {
        let t = get_table("e_coli").unwrap();
        // S×6: best codon AGC repeated → 12-mer repeats in the draft
        // ("AGCAGCAGCAGC" at offset 0/3/6).
        let original = codons("AGCAGCAGCAGCAGCAGC");
        let res = optimize_codons(&original, t, &OptimizeOptions::default());
        assert_eq!(res.new_codons.len(), original.len());
        assert_eq!(translate(&res.new_codons), translate(&original));
        let dna = String::from_utf8(join(&res.new_codons)).unwrap();
        assert!(res.unresolved.is_empty(), "unresolved: {:?}", res.unresolved);
        let k = 12;
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for s in 0..=dna.len() - k {
            *seen.entry(&dna[s..s + k]).or_insert(0) += 1;
        }
        assert!(seen.values().all(|&c| c == 1), "repeated {k}-mers: {seen:?}");
    }

    #[test]
    fn avoid_enzyme_sites_removes_gaattc() {
        let t = get_table("e_coli").unwrap();
        // E F: best codons GAA TTT (F's best is TTT), so the draft is already
        // site-free; verify the option is accepted and stays site-free.
        let original = codons("GAATTC");
        let res = optimize_codons(&original, t, &OptimizeOptions {
            avoid_enzyme_sites: vec!["GAATTC".to_string()],
            ..OptimizeOptions::default()
        });
        assert_eq!(res.new_codons.len(), original.len());
        assert_eq!(translate(&res.new_codons), translate(&original));
        let dna = String::from_utf8(join(&res.new_codons)).unwrap();
        assert!(!dna.contains("GAATTC"), "site still present: {dna}");
        assert!(res.unresolved.is_empty(), "unresolved: {:?}", res.unresolved);
    }

    #[test]
    fn repair_removes_best_codon_created_enzyme_site() {
        let t = get_table("e_coli").unwrap();
        // K T: best codons AAA ACC = "AAAACC", which the repair must break
        // when it is listed as a forbidden site (both strands).
        let original = codons("AAAACC");
        let res = optimize_codons(&original, t, &OptimizeOptions {
            avoid_enzyme_sites: vec!["AAAACC".to_string()],
            ..OptimizeOptions::default()
        });
        assert_eq!(res.new_codons.len(), original.len());
        assert_eq!(translate(&res.new_codons), translate(&original));
        let dna = String::from_utf8(join(&res.new_codons)).unwrap();
        assert!(!dna.contains("AAAACC"), "site still present: {dna}");
        assert!(!dna.contains("GGTTTT"), "site on reverse strand: {dna}");
        assert!(res.unresolved.is_empty(), "unresolved: {:?}", res.unresolved);
        assert!(!res.repairs.is_empty(), "repair must fire");
    }

    // ------------------------------------------------------------------
    // extract_codons
    // ------------------------------------------------------------------

    #[test]
    fn extract_forward_feature() {
        let f = feat("gfp", 0, 11, "+", vec![]);
        let cd = extract_codons("ATGGTGAGCAAATAA", &f, "linear").unwrap();
        assert_eq!(cd.codons, codons("ATGGTGAGCAAA"));
        assert_eq!(cd.aa, "MVSK");
        assert_eq!(cd.segments_on_template, vec![(0, 2), (3, 5), (6, 8), (9, 11)]);
    }

    #[test]
    fn extract_reverse_feature() {
        let f = feat("amp", 0, 11, "-", vec![]);
        let cd = extract_codons("ATGAAATTTAAA", &f, "linear").unwrap();
        assert_eq!(cd.codons, codons("TTTAAATTTCAT"));
        assert_eq!(cd.aa, "FKFH");
        // each codon's template span is ascending (write-back direction)
        assert_eq!(cd.segments_on_template, vec![(9, 11), (6, 8), (3, 5), (0, 2)]);
    }

    #[test]
    fn extract_segmented_feature() {
        let f = feat("seg", 0, 5, "+", vec![(0, 2), (3, 5)]);
        let cd = extract_codons("ATGAAATTTAAA", &f, "linear").unwrap();
        assert_eq!(cd.codons, codons("ATGAAA"));
        assert_eq!(cd.aa, "MK");
        assert_eq!(cd.segments_on_template, vec![(0, 2), (3, 5)]);
    }

    #[test]
    fn extract_circular_wrap_feature() {
        // 12 bp circle; feature [10..=3] wraps through the origin.
        let f = feat("wrap", 10, 3, "+", vec![]);
        let cd = extract_codons("AACCGGTTAACC", &f, "circular").unwrap();
        // positions 10,11,0,1,2,3 = C,C,A,A,C,C
        assert_eq!(cd.codons, codons("CCAACC"));
        assert_eq!(cd.aa, "PT");
        // codon 1 crosses the origin → split into two linear entries
        assert_eq!(cd.segments_on_template, vec![(10, 11), (0, 0), (1, 3)]);
        let total: usize = cd.segments_on_template.iter().map(|&(a, b)| b - a + 1).sum();
        assert_eq!(total, cd.codons.len() * 3);
    }

    #[test]
    fn extract_rejects_bad_length_and_ambiguous() {
        let f = feat("x", 0, 4, "+", vec![]);
        assert!(extract_codons("ATGGTGAGCA", &f, "linear").is_err()); // 5 bases
        let f = feat("n", 0, 5, "+", vec![]);
        assert!(extract_codons("ATGNNN", &f, "linear").is_err());
        let f = feat("oob", 0, 100, "+", vec![]);
        assert!(extract_codons("ATGGTGAGCA", &f, "linear").is_err());
        let f = feat("badwrap", 10, 3, "+", vec![]);
        assert!(extract_codons("AACCGGTTAACC", &f, "linear").is_err()); // wrap on linear
    }

    #[test]
    fn write_back_roundtrip_on_plus_and_minus() {
        // Simulate what the Tauri layer will do: write new codons back through
        // segments_on_template and verify the translation stays correct.
        for (seq, f, want_codons) in [
            ("ATGGTGAGCAAATAA", feat("p", 0, 11, "+", vec![]), "GAAACCGTTAAA"),
            ("ATGAAATTTAAA", feat("m", 0, 11, "-", vec![]), "TTTAAATTTCAT"),
        ] {
            let cd = extract_codons(seq, &f, "linear").unwrap();
            let new_codons = codons(want_codons);
            let mut tmpl: Vec<u8> = seq.as_bytes().to_vec();
            let mut buf = String::new();
            for c in &new_codons {
                buf.push_str(c);
            }
            let coding = buf.as_bytes();
            let mut off = 0;
            let minus = f.strand == "-";
            for &(a, b) in &cd.segments_on_template {
                let piece = &coding[off..off + (b - a + 1)];
                if minus {
                    let rc = revcomp_bytes(piece);
                    tmpl[a..=b].copy_from_slice(&rc);
                } else {
                    tmpl[a..=b].copy_from_slice(piece);
                }
                off += b - a + 1;
            }
            let new_seq = String::from_utf8(tmpl).unwrap();
            assert_eq!(extract_codons(&new_seq, &f, "linear").unwrap().codons, new_codons);
        }
    }
    #[test]
    fn cas9_real_sequence_repair_converges() {
        // Regression guard for the repair layer: the 1369-codon Cas9 CDS from
        // pCas9.gbk must converge in a few rounds with a small repair count
        // and (near-)empty unresolved — previously it capped 500 rounds and
        // reported hundreds of unresolved 8-mer repeats.
        let gbk = include_str!("../../../examples/pCas9.gbk");
        let mut seq = String::new();
        let mut in_seq = false;
        for line in gbk.lines() {
            if line.starts_with("ORIGIN") {
                in_seq = true;
                continue;
            }
            if in_seq {
                if line.starts_with("//") {
                    break;
                }
                seq.extend(line.chars().filter(|c| c.is_ascii_alphabetic()));
            }
        }
        // Cas9 CDS 2225..6331 (1-based) → 0-based inclusive 2224..6330
        let f = feat("cas9", 2224, 6330, "+", vec![]);
        let cd = extract_codons(&seq, &f, "circular").expect("extract cas9");
        assert_eq!(cd.codons.len(), 1369);

        for (method, species, label) in [
            (OptimizeMethod::UseBestCodon, "e_coli", "use_best_codon+e_coli"),
            (OptimizeMethod::MatchCodonUsage, "s_cerevisiae", "match_codon_usage+s_cerevisiae"),
        ] {
            let t = get_table(species).unwrap();
            let res = optimize_codons(&cd.codons, t, &OptimizeOptions {
                method,
                ..OptimizeOptions::default()
            });
            eprintln!(
                "{label}: cai {:.4} -> {:.4}, gc {:.3} -> {:.3}, repairs {}, unresolved {}",
                res.cai_before, res.cai_after, res.gc_before, res.gc_after,
                res.repairs.len(),
                res.unresolved.len()
            );
            assert_eq!(res.new_codons.len(), cd.codons.len());
            assert_eq!(translate(&res.new_codons), translate(&cd.codons));
            assert!(
                res.repairs.len() < 100,
                "repair count {label}: {} (was capped at 500)",
                res.repairs.len()
            );
            assert!(
                res.unresolved.len() < 10,
                "unresolved {label}: {} (was hundreds)",
                res.unresolved.len()
            );
        }
        // use_best_codon must raise CAI substantially
        let t = get_table("e_coli").unwrap();
        let res = optimize_codons(&cd.codons, t, &OptimizeOptions::default());
        assert!(res.cai_after > res.cai_before + 0.3, "cai {:.3} -> {:.3}", res.cai_before, res.cai_after);
    }
}
