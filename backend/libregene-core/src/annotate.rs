//! Automatic annotation of bare DNA sequences against the embedded SnapGene
//! (GenoLIB) feature database.
//!
//! Ports pLannotate's blastn → score → filter → overlap pipeline (`_filter.py`
//! / `annotate.py`) with a k-mer seed + ungapped extend matcher replacing
//! BLAST. Coordinates are 0-based inclusive; circular queries are searched
//! doubled (query + query) and wrapped back afterwards, so origin-wrapping
//! features are reported with `start > end` and split `segments`.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::models::Segment;

/// Seed length for the k-mer index (blastn `-word_size 12`).
const K: usize = 12;
/// Features this long or shorter use the near-exact window scan instead.
const SHORT_MAX: usize = 24;
/// Minimum identity (%) for a kept alignment (blastn `-perc_identity 95`).
const MIN_IDENTITY: f64 = 95.0;
/// Minimum alignment length in bp — evalue proxy (pLannotate report §6.2).
const MIN_HIT_LENGTH: usize = 13;
/// Seed length for the protein k-mer index over translated CDS (blastp
/// `-word_size 3`).
const K_AA: usize = 3;
/// Protein translations this long or shorter (aa) use the window scan instead.
const SHORT_MAX_AA: usize = 24;
/// Minimum seeded protein hit length in aa.
const MIN_AA_HIT_LENGTH: usize = 13;
/// Minimum aa-level hit length (shortest db CDS translates to 4 aa).
const MIN_AA_SHORT: usize = 4;
/// Trim ratio used for overlap detection.
const WIGGLE_RATIO: f64 = 0.15;
/// Score bonus when `pi_permatch == 100`.
const PERFECT_BONUS: f64 = 10.0;
/// Swiss-Prot ids that caused overlap trouble in pLannotate.
const BLACKLIST: [&str; 4] = ["P03851", "P03845", "ISS", "P03846"];
const DEFAULT_COLOR: &str = "#808080";

const FEATURES_FASTA: &str = include_str!("../data/features.fasta");
const FEATURES_TSV: &str = include_str!("../data/features.tsv");
const COLORS_TSV: &str = include_str!("../data/feature_colors.tsv");

/// One auto-annotated feature, shaped like `models::Feature` plus match stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotatedFeature {
    pub id: String,
    pub name: String,
    pub ftype: String,
    /// 0-based inclusive start; `> end` for origin-wrapping features.
    pub start: i64,
    /// 0-based inclusive end.
    pub end: i64,
    pub color: String,
    /// "+" forward strand, "-" reverse strand.
    pub strand: String,
    /// Linear segments; origin-wrapping features are split in two.
    pub segments: Vec<Segment>,
    /// Percent identity of the best local alignment.
    pub identity: f64,
    /// Covered fraction of the database feature in percent (`percmatch`).
    pub coverage: f64,
    pub fragment: bool,
    /// Score used for the overlap-resolution ordering.
    pub score: f64,
    /// Database description (`blurb`).
    pub notes: String,
    pub sseqid: String,
    /// "nt" for nucleotide-level matches, "aa" for protein-level CDS matches.
    pub match_level: String,
}

struct DbFeature {
    sseqid: String,
    name: String,
    ftype: String,
    blurb: String,
    fwd: Vec<u8>,
    rc: Vec<u8>,
    /// Translated fwd sequence; `Some` only for CDS features. Codon-optimized
    /// CDS are caught by matching this protein instead of the nucleotides.
    protein: Option<Vec<u8>>,
}

struct AnnotationDb {
    features: Vec<DbFeature>,
    /// 2-bit packed 12-mer → packed seed refs (strand<<48 | feature<<32 | offset).
    index: HashMap<u32, Vec<u64>>,
    /// 5-bit packed 3-aa-mer → packed seed refs (feature<<16 | offset). Only
    /// CDS translations are indexed; the query side supplies the strand via
    /// its six-frame translation.
    pindex: HashMap<u16, Vec<u32>>,
    /// Feature type (normalized) → fill color, mirroring pLannotate colors.csv.
    colors: HashMap<String, String>,
}

static DB: OnceLock<AnnotationDb> = OnceLock::new();

fn db() -> &'static AnnotationDb {
    DB.get_or_init(build_db)
}

fn parse_fasta(s: &str) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut id = String::new();
    let mut seq = Vec::new();
    for line in s.lines() {
        if line.starts_with('>') {
            if !id.is_empty() {
                out.push((std::mem::take(&mut id), std::mem::take(&mut seq)));
            }
            id = line[1..].split_whitespace().next().unwrap_or("").to_string();
        } else {
            seq.extend(line.bytes().map(|b| b.to_ascii_uppercase()));
        }
    }
    if !id.is_empty() {
        out.push((id, seq));
    }
    out
}

fn rev_comp(seq: &[u8]) -> Vec<u8> {
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

fn encode_kmer(win: &[u8]) -> Option<u32> {
    let mut key = 0u32;
    for &b in win {
        let v = match b {
            b'A' => 0u32,
            b'C' => 1,
            b'G' => 2,
            b'T' => 3,
            _ => return None,
        };
        key = (key << 2) | v;
    }
    Some(key)
}

/// 5-bit encoding of the 20 standard amino acids; `*`/`?`/anything else is
/// excluded from seeds.
fn encode_aa(b: u8) -> Option<u16> {
    Some(match b {
        b'A' => 0,
        b'R' => 1,
        b'N' => 2,
        b'D' => 3,
        b'C' => 4,
        b'Q' => 5,
        b'E' => 6,
        b'G' => 7,
        b'H' => 8,
        b'I' => 9,
        b'L' => 10,
        b'K' => 11,
        b'M' => 12,
        b'F' => 13,
        b'P' => 14,
        b'S' => 15,
        b'T' => 16,
        b'W' => 17,
        b'Y' => 18,
        b'V' => 19,
        _ => return None,
    })
}

fn encode_aa_kmer(win: &[u8]) -> Option<u16> {
    let mut key = 0u16;
    for &b in win {
        key = (key << 5) | encode_aa(b)?;
    }
    Some(key)
}

fn build_db() -> AnnotationDb {
    let mut meta: HashMap<String, (&str, &str, &str)> = HashMap::new();
    for line in FEATURES_TSV.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() >= 4 {
            // sseqid join is case-insensitive: the fasta header "E4ORF6"
            // corresponds to tsv sseqid "E4orf6".
            meta.insert(f[0].to_lowercase(), (f[1], f[2], f[3]));
        }
    }

    let mut features = Vec::new();
    for (id, seq) in parse_fasta(FEATURES_FASTA) {
        let (name, ftype, blurb) = match meta.get(&id.to_lowercase()) {
            Some((n, t, b)) => (n.to_string(), t.to_string(), b.to_string()),
            None => (id.clone(), "misc_feature".to_string(), String::new()),
        };
        let protein = (ftype == "CDS").then(|| crate::translate::translate_nt(&seq));
        features.push(DbFeature {
            rc: rev_comp(&seq),
            fwd: seq,
            sseqid: id,
            name,
            ftype,
            blurb,
            protein,
        });
    }

    let mut index: HashMap<u32, Vec<u64>> = HashMap::with_capacity(2_000_000);
    for (fi, f) in features.iter().enumerate() {
        for (strand, pat) in [(0u64, &f.fwd), (1, &f.rc)] {
            if pat.len() < K {
                continue;
            }
            for offset in 0..=pat.len() - K {
                let Some(key) = encode_kmer(&pat[offset..offset + K]) else {
                    continue;
                };
                index
                    .entry(key)
                    .or_default()
                    .push((strand << 48) | ((fi as u64) << 32) | (offset as u64));
            }
        }
    }

    let mut pindex: HashMap<u16, Vec<u32>> = HashMap::with_capacity(200_000);
    for (fi, f) in features.iter().enumerate() {
        let Some(prot) = &f.protein else {
            continue;
        };
        if prot.len() < K_AA {
            continue;
        }
        for offset in 0..=prot.len() - K_AA {
            let Some(key) = encode_aa_kmer(&prot[offset..offset + K_AA]) else {
                continue;
            };
            pindex
                .entry(key)
                .or_default()
                .push(((fi as u32) << 16) | offset as u32);
        }
    }

    let mut colors = HashMap::new();
    for line in COLORS_TSV.lines() {
        let mut it = line.split('\t');
        if let (Some(t), Some(c)) = (it.next(), it.next()) {
            if !t.is_empty() && !c.is_empty() {
                colors.insert(t.to_string(), c.to_string());
            }
        }
    }

    AnnotationDb {
        features,
        index,
        pindex,
        colors,
    }
}

/// A candidate local alignment of one database feature against the query.
struct RawHit {
    feat: usize,
    /// +1 forward, -1 reverse.
    strand: i8,
    qstart: i64,
    qend: i64,
    /// Hit length in query coordinate units (nt for DNA queries, aa for
    /// protein queries; aa-level hits on DNA are expressed in nt, 3×aa).
    length: usize,
    pident: f64,
    /// Coverage denominator: feature length in hit coordinate units.
    cov_denom: usize,
    /// True when the hit comes from matching the translated CDS protein.
    aa_level: bool,
}

impl RawHit {
    fn nt(feat: usize, strand: i8, qstart: i64, qend: i64, length: usize, pident: f64) -> Self {
        RawHit {
            feat,
            strand,
            qstart,
            qend,
            length,
            pident,
            cov_denom: 0, // filled from the feature length in finalize()
            aa_level: false,
        }
    }
}

/// Full-length near-exact window scan for short features (< 25 bp): the
/// k-mer seed path can miss them, so slide the whole feature and count
/// mismatches, allowing ≤ 5% differences.
fn scan_short(feat: usize, pat: &[u8], query: &[u8], strand: i8, out: &mut Vec<RawHit>) {
    let m = pat.len();
    if m > query.len() {
        return;
    }
    let budget = (m as f64 * 0.05) as usize;
    for p in 0..=query.len() - m {
        let mut mm = 0usize;
        for j in 0..m {
            if query[p + j] != pat[j] {
                mm += 1;
                if mm > budget {
                    break;
                }
            }
        }
        if mm <= budget {
            out.push(RawHit::nt(
                feat,
                strand,
                p as i64,
                (p + m - 1) as i64,
                m,
                100.0 * (m - mm) as f64 / m as f64,
            ));
        }
    }
}

fn collect_seeds(query: &[u8], index: &HashMap<u32, Vec<u64>>) -> Vec<(u16, u8, u32, u16)> {
    let mut out = Vec::new();
    if query.len() < K {
        return out;
    }
    for p in 0..=query.len() - K {
        let Some(key) = encode_kmer(&query[p..p + K]) else {
            continue;
        };
        if let Some(entries) = index.get(&key) {
            for &e in entries {
                out.push(((e >> 32) as u16, (e >> 48) as u8, p as u32, e as u16));
            }
        }
    }
    out
}

/// A maximal run of consecutive 12-mer seeds on one diagonal (ungapped
/// alignment), i.e. one candidate extension start.
struct Run {
    feat: u16,
    /// 0 forward, 1 reverse.
    strand: u8,
    q_lo: i64,
    q_hi: i64,
    o_lo: i64,
    o_hi: i64,
}

/// Group seed hits per (feature, strand, diagonal) and split each diagonal
/// into runs of consecutive query positions.
fn cluster_runs(mut seeds: Vec<(u16, u8, u32, u16)>) -> Vec<Run> {
    seeds.sort_by_key(|&(fi, st, q, o)| (fi, st, o as i64 - q as i64, q));
    let mut runs = Vec::new();
    let mut i = 0;
    while i < seeds.len() {
        let (fi, st, _, _) = seeds[i];
        let delta = seeds[i].3 as i64 - seeds[i].2 as i64;
        let mut j = i + 1;
        while j < seeds.len()
            && seeds[j].0 == fi
            && seeds[j].1 == st
            && seeds[j].3 as i64 - seeds[j].2 as i64 == delta
        {
            j += 1;
        }
        let mut start = i;
        while start < j {
            let mut end = start + 1;
            while end < j && seeds[end].2 == seeds[end - 1].2 + 1 {
                end += 1;
            }
            let (_, _, qs, os) = seeds[start];
            let (_, _, qe, oe) = seeds[end - 1];
            runs.push(Run {
                feat: fi,
                strand: st,
                q_lo: qs as i64,
                q_hi: qe as i64 + K as i64 - 1,
                o_lo: os as i64,
                o_hi: oe as i64 + K as i64 - 1,
            });
            start = end;
        }
        i = j;
    }
    runs
}

/// Extend one seed run left/right without gaps, keeping the cumulative
/// identity ≥ 95% (mismatch budget `length / 20`), then trim the trailing
/// mismatches so the alignment starts and ends on a match (HSP-like).
fn extend_run(run: &Run, query: &[u8], d: &AnnotationDb) -> Option<RawHit> {
    let f = &d.features[run.feat as usize];
    let pat: &[u8] = if run.strand == 0 { &f.fwd } else { &f.rc };
    let mut q_lo = run.q_lo;
    let mut q_hi = run.q_hi;
    let mut o_lo = run.o_lo;
    let mut o_hi = run.o_hi;
    let mut m = 0usize;
    let mut len = (q_hi - q_lo + 1) as usize;

    loop {
        if q_lo == 0 || o_lo == 0 {
            break;
        }
        let miss = (query[q_lo as usize - 1] != pat[o_lo as usize - 1]) as usize;
        if (m + miss) * 20 > len + 1 {
            break;
        }
        q_lo -= 1;
        o_lo -= 1;
        m += miss;
        len += 1;
    }
    loop {
        if q_hi as usize + 1 >= query.len() || o_hi as usize + 1 >= pat.len() {
            break;
        }
        let miss = (query[q_hi as usize + 1] != pat[o_hi as usize + 1]) as usize;
        if (m + miss) * 20 > len + 1 {
            break;
        }
        q_hi += 1;
        o_hi += 1;
        m += miss;
        len += 1;
    }
    while q_lo <= q_hi && o_lo <= o_hi && query[q_lo as usize] != pat[o_lo as usize] {
        q_lo += 1;
        o_lo += 1;
        len -= 1;
        m -= 1;
    }
    while q_lo <= q_hi && o_lo <= o_hi && query[q_hi as usize] != pat[o_hi as usize] {
        q_hi -= 1;
        o_hi -= 1;
        len -= 1;
        m -= 1;
    }
    if len < MIN_HIT_LENGTH {
        return None;
    }
    Some(RawHit::nt(
        run.feat as usize,
        if run.strand == 0 { 1 } else { -1 },
        q_lo,
        q_hi,
        len,
        100.0 * (len - m) as f64 / len as f64,
    ))
}

/// A protein-level hit in aa coordinates of one query frame (or of the whole
/// query for protein sequences).
struct ProtHit {
    feat: usize,
    astart: i64,
    aend: i64,
    length: usize,
    pident: f64,
}

/// Full-length near-exact window scan for short protein translations
/// (≤ 24 aa): slide the whole pattern and count mismatches, allowing ≤ 5%.
fn scan_short_protein(feat: usize, pat: &[u8], query: &[u8], out: &mut Vec<ProtHit>) {
    let m = pat.len();
    if m > query.len() {
        return;
    }
    let budget = (m as f64 * 0.05) as usize;
    for p in 0..=query.len() - m {
        let mut mm = 0usize;
        for j in 0..m {
            if query[p + j] != pat[j] {
                mm += 1;
                if mm > budget {
                    break;
                }
            }
        }
        if mm <= budget {
            out.push(ProtHit {
                feat,
                astart: p as i64,
                aend: (p + m - 1) as i64,
                length: m,
                pident: 100.0 * (m - mm) as f64 / m as f64,
            });
        }
    }
}

/// A maximal run of consecutive 3-aa seeds on one diagonal. The db side
/// indexes only forward translations, so the query frame carries the strand.
struct ProtRun {
    feat: u16,
    q_lo: i64,
    q_hi: i64,
    o_lo: i64,
    o_hi: i64,
}

fn collect_prot_seeds(query: &[u8], pindex: &HashMap<u16, Vec<u32>>) -> Vec<(u16, u32, u16)> {
    let mut out = Vec::new();
    if query.len() < K_AA {
        return out;
    }
    for p in 0..=query.len() - K_AA {
        let Some(key) = encode_aa_kmer(&query[p..p + K_AA]) else {
            continue;
        };
        if let Some(entries) = pindex.get(&key) {
            for &e in entries {
                out.push(((e >> 16) as u16, p as u32, (e & 0xffff) as u16));
            }
        }
    }
    out
}

fn cluster_prot_runs(mut seeds: Vec<(u16, u32, u16)>) -> Vec<ProtRun> {
    seeds.sort_by_key(|&(fi, q, o)| (fi, o as i64 - q as i64, q));
    let mut runs = Vec::new();
    let mut i = 0;
    while i < seeds.len() {
        let fi = seeds[i].0;
        let delta = seeds[i].2 as i64 - seeds[i].1 as i64;
        let mut j = i + 1;
        while j < seeds.len() && seeds[j].0 == fi && seeds[j].2 as i64 - seeds[j].1 as i64 == delta {
            j += 1;
        }
        let mut start = i;
        while start < j {
            let mut end = start + 1;
            while end < j && seeds[end].1 == seeds[end - 1].1 + 1 {
                end += 1;
            }
            let (_, qs, os) = seeds[start];
            let (_, qe, oe) = seeds[end - 1];
            runs.push(ProtRun {
                feat: fi,
                q_lo: qs as i64,
                q_hi: qe as i64 + K_AA as i64 - 1,
                o_lo: os as i64,
                o_hi: oe as i64 + K_AA as i64 - 1,
            });
            start = end;
        }
        i = j;
    }
    runs
}

/// Ungapped protein extension with the same ≥ 95% cumulative identity budget
/// (`len / 20` mismatches) and match-anchored trimming as `extend_run`.
fn extend_prot_run(run: &ProtRun, query: &[u8], d: &AnnotationDb) -> Option<ProtHit> {
    let pat: &[u8] = d.features[run.feat as usize].protein.as_ref()?;
    let mut q_lo = run.q_lo;
    let mut q_hi = run.q_hi;
    let mut o_lo = run.o_lo;
    let mut o_hi = run.o_hi;
    let mut m = 0usize;
    let mut len = (q_hi - q_lo + 1) as usize;

    loop {
        if q_lo == 0 || o_lo == 0 {
            break;
        }
        let miss = (query[q_lo as usize - 1] != pat[o_lo as usize - 1]) as usize;
        if (m + miss) * 20 > len + 1 {
            break;
        }
        q_lo -= 1;
        o_lo -= 1;
        m += miss;
        len += 1;
    }
    loop {
        if q_hi as usize + 1 >= query.len() || o_hi as usize + 1 >= pat.len() {
            break;
        }
        let miss = (query[q_hi as usize + 1] != pat[o_hi as usize + 1]) as usize;
        if (m + miss) * 20 > len + 1 {
            break;
        }
        q_hi += 1;
        o_hi += 1;
        m += miss;
        len += 1;
    }
    while q_lo <= q_hi && o_lo <= o_hi && query[q_lo as usize] != pat[o_lo as usize] {
        q_lo += 1;
        o_lo += 1;
        len -= 1;
        m -= 1;
    }
    while q_lo <= q_hi && o_lo <= o_hi && query[q_hi as usize] != pat[o_hi as usize] {
        q_hi -= 1;
        o_hi -= 1;
        len -= 1;
        m -= 1;
    }
    if len < MIN_AA_HIT_LENGTH {
        return None;
    }
    Some(ProtHit {
        feat: run.feat as usize,
        astart: q_lo,
        aend: q_hi,
        length: len,
        pident: 100.0 * (len - m) as f64 / len as f64,
    })
}

/// Match a protein query against the translated CDS features; hits are in aa
/// coordinates of `query`.
fn match_protein(query: &[u8]) -> Vec<ProtHit> {
    let d = db();
    let mut out = Vec::new();
    for (fi, f) in d.features.iter().enumerate() {
        let Some(prot) = &f.protein else {
            continue;
        };
        if prot.len() >= MIN_AA_SHORT && prot.len() <= SHORT_MAX_AA {
            scan_short_protein(fi, prot, query, &mut out);
        }
    }
    for run in cluster_prot_runs(collect_prot_seeds(query, &d.pindex)) {
        if let Some(h) = extend_prot_run(&run, query, d) {
            out.push(h);
        }
    }
    out
}

/// Convert an aa-coordinate hit in one translated frame to nt coordinates on
/// the (possibly doubled) forward query of length `n`.
fn prot_hit_to_nt(h: ProtHit, frame: usize, strand: i8, n: usize) -> RawHit {
    let f = frame as i64;
    let (qstart, qend) = if strand > 0 {
        (3 * h.astart + f, 3 * h.aend + f + 2)
    } else {
        let n = n as i64;
        (n - 1 - (3 * h.aend + f + 2), n - 1 - (3 * h.astart + f))
    };
    let cov_denom = db().features[h.feat]
        .protein
        .as_ref()
        .map_or(0, |p| 3 * p.len());
    RawHit {
        feat: h.feat,
        strand,
        qstart,
        qend,
        length: 3 * h.length,
        pident: h.pident,
        cov_denom,
        aa_level: true,
    }
}

fn match_features(query: &[u8], circular: bool) -> Vec<RawHit> {
    let d = db();
    let doubled: Vec<u8> = if circular {
        [&query[..], &query[..]].concat()
    } else {
        query.to_vec()
    };
    let mut hits = Vec::new();
    for (fi, f) in d.features.iter().enumerate() {
        let len = f.fwd.len();
        if len >= MIN_HIT_LENGTH && len <= SHORT_MAX {
            scan_short(fi, &f.fwd, &doubled, 1, &mut hits);
            scan_short(fi, &f.rc, &doubled, -1, &mut hits);
        }
    }
    let seeds = collect_seeds(&doubled, &d.index);
    for run in cluster_runs(seeds) {
        if let Some(h) = extend_run(&run, &doubled, d) {
            hits.push(h);
        }
    }
    // Protein-level pass: translate the six frames and match the CDS
    // translations, so codon-optimized CDS (same protein, drifted nucleotides)
    // are still found.
    let n = doubled.len();
    for f in 0..3usize {
        let prot = crate::translate::translate_nt(&doubled[f..]);
        for h in match_protein(&prot) {
            hits.push(prot_hit_to_nt(h, f, 1, n));
        }
    }
    let rc = rev_comp(&doubled);
    for f in 0..3usize {
        let prot = crate::translate::translate_nt(&rc[f..]);
        for h in match_protein(&prot) {
            hits.push(prot_hit_to_nt(h, f, -1, n));
        }
    }
    hits
}

struct ScoredHit {
    hit: RawHit,
    percmatch: f64,
    pi_permatch: f64,
    score: f64,
    wstart: i64,
    wend: i64,
}

/// Inclusive circular interval as one or two linear segments; `exclude_start`
/// opens the left end when the interval does not wrap (pLannotate
/// `_circular_segments`).
fn circular_segments(start: i64, end: i64, qlen: usize, exclude_start: bool) -> Vec<(i64, i64)> {
    let mut s = start;
    let e = end;
    if exclude_start && s < e {
        s += 1;
    }
    if s <= e {
        vec![(s, e)]
    } else {
        let mut out = Vec::new();
        if e >= 0 {
            out.push((0, e));
        }
        if s < qlen as i64 {
            out.push((s, qlen as i64 - 1));
        }
        out
    }
}

fn segments_overlap(a: &[(i64, i64)], b: &[(i64, i64)]) -> bool {
    a.iter()
        .any(|&(a0, a1)| b.iter().any(|&(b0, b1)| a0.max(b0) <= a1.min(b1)))
}

/// Diverges from pLannotate, which additionally requires CDS hit lengths to
/// be multiples of 3 — that assumes intron-less db entries and marks every
/// imperfect genomic (segmented) CDS match as a fragment. `pident >= 95` is
/// already guaranteed by the retain filter in finalize().
fn is_fragment(ftype: &str, percmatch: f64, pi_permatch: f64) -> bool {
    if ftype != "CDS" {
        percmatch < 95.0
    } else {
        !(pi_permatch == 100.0 || percmatch > 95.0)
    }
}

fn color_for(ftype: &str, d: &AnnotationDb) -> String {
    // colors.csv keys "origin of replication"; db type is "rep_origin".
    let key = ftype.replace("rep_origin", "origin of replication");
    d.colors
        .get(&key)
        .cloned()
        .unwrap_or_else(|| DEFAULT_COLOR.to_string())
}

fn segments_for(start: i64, end: i64, qlen: usize, color: &str) -> (i64, i64, Vec<Segment>) {
    let seg = |s: i64, e: i64| Segment {
        start: s,
        end: e,
        color: Some(color.to_string()),
    };
    if start <= end {
        (start, end, vec![seg(start, end)])
    } else {
        (
            start,
            end,
            vec![seg(start, qlen as i64 - 1), seg(0, end)],
        )
    }
}

/// Score, wrap, filter, dedup and resolve overlaps — mirrors pLannotate's
/// `filter_and_clean_hits` ordering (score → sort → wrap → quality filters →
/// greedy overlap removal).
fn finalize(hits: Vec<RawHit>, qlen: usize, circular: bool) -> Vec<AnnotatedFeature> {
    let d = db();
    let mut scored: Vec<ScoredHit> = hits
        .into_iter()
        .map(|hit| {
            let slen = if hit.aa_level {
                hit.cov_denom
            } else {
                d.features[hit.feat].fwd.len()
            };
            let percmatch = hit.length as f64 / slen as f64 * 100.0;
            let abs_pm = 100.0 - (100.0 - percmatch).abs();
            let pi_pm = hit.pident * abs_pm / 100.0;
            let mut score = pi_pm / 100.0 * hit.length as f64;
            if pi_pm == 100.0 {
                score *= PERFECT_BONUS;
            }
            let wiggle = (hit.length as f64 * WIGGLE_RATIO) as i64;
            ScoredHit {
                wstart: hit.qstart + wiggle,
                wend: hit.qend - wiggle,
                hit,
                percmatch,
                pi_permatch: pi_pm,
                score,
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.hit.length.cmp(&a.hit.length))
            .then(
                b.percmatch
                    .partial_cmp(&a.percmatch)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    if circular {
        let q = qlen as i64;
        for s in &mut scored {
            if s.hit.qstart >= q {
                s.hit.qstart -= q;
            }
            if s.hit.qend >= q {
                s.hit.qend -= q;
            }
            if s.wstart >= q {
                s.wstart -= q;
            }
            if s.wend >= q {
                s.wend -= q;
            }
        }
    }

    scored.retain(|s| {
        let f = &d.features[s.hit.feat];
        // aa-level hits are already length-filtered in aa units by the
        // seeded/short-scan paths (≥ 13 aa or full short pattern ≥ 4 aa).
        let len_ok = s.hit.aa_level || s.hit.length >= MIN_HIT_LENGTH;
        !BLACKLIST.contains(&f.sseqid.as_str())
            && f.ftype != "primer_bind"
            && s.hit.pident >= MIN_IDENTITY
            && len_ok
            && s.pi_permatch > 3.0
    });

    // Both copies of a circular hit land on the same wrapped interval.
    let mut seen = HashSet::new();
    scored.retain(|s| {
        seen.insert((
            s.hit.feat,
            s.hit.qstart,
            s.hit.qend,
            s.hit.length,
            (s.hit.pident * 100.0).round() as i64,
        ))
    });

    let mut dropped = vec![false; scored.len()];
    for i in 0..scored.len() {
        if dropped[i] {
            continue;
        }
        let occupied = circular_segments(scored[i].hit.qstart, scored[i].hit.qend, qlen, true);
        for j in i + 1..scored.len() {
            if dropped[j] {
                continue;
            }
            let trimmed = circular_segments(scored[j].wstart, scored[j].wend, qlen, false);
            if segments_overlap(&occupied, &trimmed) {
                dropped[j] = true;
            }
        }
    }

    let mut out = Vec::new();
    for (idx, s) in scored.iter().enumerate() {
        if dropped[idx] {
            continue;
        }
        let f = &d.features[s.hit.feat];
        let mut ftype = f.ftype.clone();
        if ftype == "origin of replication" {
            ftype = "rep_origin".to_string();
        }
        let fragment = is_fragment(&ftype, s.percmatch, s.pi_permatch);
        let name = if fragment {
            format!("{} (fragment)", f.name)
        } else {
            f.name.clone()
        };
        let color = color_for(&ftype, d);
        let (start, end, segments) = segments_for(s.hit.qstart, s.hit.qend, qlen, &color);
        out.push(AnnotatedFeature {
            id: format!("auto-{}", out.len()),
            name,
            ftype,
            start,
            end,
            color,
            strand: if s.hit.strand > 0 {
                "+".to_string()
            } else {
                "-".to_string()
            },
            segments,
            identity: s.hit.pident,
            coverage: s.percmatch,
            fragment,
            score: s.score,
            notes: f.blurb.clone(),
            sseqid: f.sseqid.clone(),
            match_level: if s.hit.aa_level {
                "aa".to_string()
            } else {
                "nt".to_string()
            },
        });
    }
    out
}

/// Annotate a bare DNA sequence against the embedded SnapGene feature
/// database. `circular` doubles the query so origin-wrapping features are
/// found; reported coordinates are 0-based inclusive.
pub fn annotate_sequence(seq: &str, circular: bool) -> Vec<AnnotatedFeature> {
    let query = seq.to_ascii_uppercase().into_bytes();
    let qlen = query.len();
    if qlen == 0 {
        return Vec::new();
    }
    let hits = match_features(&query, circular);
    finalize(hits, qlen, circular)
}

/// Annotate a bare amino-acid sequence against the translated CDS features of
/// the embedded database (protein projects). Only CDS-type features are
/// detectable; coordinates are 0-based inclusive aa positions, strand "+".
pub fn annotate_protein(seq: &str, circular: bool) -> Vec<AnnotatedFeature> {
    let query = seq.to_ascii_uppercase().into_bytes();
    let qlen = query.len();
    if qlen == 0 {
        return Vec::new();
    }
    let doubled: Vec<u8> = if circular {
        [&query[..], &query[..]].concat()
    } else {
        query
    };
    let d = db();
    let hits = match_protein(&doubled)
        .into_iter()
        .map(|h| {
            let cov_denom = d.features[h.feat].protein.as_ref().map_or(0, |p| p.len());
            RawHit {
                feat: h.feat,
                strand: 1,
                qstart: h.astart,
                qend: h.aend,
                length: h.length,
                pident: h.pident,
                cov_denom,
                aa_level: true,
            }
        })
        .collect();
    finalize(hits, qlen, circular)
}

#[cfg(test)]
pub(crate) fn db_protein_for_test(sseqid: &str) -> Option<String> {
    db().features
        .iter()
        .find(|f| f.sseqid == sseqid)
        .and_then(|f| f.protein.clone())
        .map(|p| String::from_utf8(p).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn db_feature(sseqid: &str) -> &'static DbFeature {
        db().features
            .iter()
            .find(|f| f.sseqid == sseqid)
            .unwrap_or_else(|| panic!("db feature {sseqid} not found"))
    }

    fn seq_from_gbk(gbk: &str) -> String {
        let mut out = String::new();
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
                out.extend(line.chars().filter(|c| c.is_ascii_alphabetic()));
            }
        }
        out
    }

    #[test]
    fn genomic_cds_hit_not_fragment() {
        // The vermilion case: a genomic CDS entry (with introns) matched at
        // 96.9% coverage / 95.2% identity must count as complete.
        assert!(!is_fragment("CDS", 96.9, 92.3));
        assert!(is_fragment("CDS", 94.9, 94.9));
        assert!(!is_fragment("CDS", 100.0, 100.0));
        // Non-CDS rule unchanged: fragment below 95% coverage.
        assert!(is_fragment("promoter", 94.9, 94.9));
        assert!(!is_fragment("promoter", 95.0, 95.0));
    }

    #[test]
    fn puc19_circular_annotates_core_features() {
        let gbk = include_str!("../../../examples/pUC19 Annotated.gbk");
        let seq = seq_from_gbk(gbk);
        assert_eq!(seq.len(), 2686);

        let t0 = Instant::now();
        let feats = annotate_sequence(&seq, true);
        let first = t0.elapsed();
        let t1 = Instant::now();
        let _ = annotate_sequence(&seq, true); // warm index
        let cached = t1.elapsed();

        println!(
            "pUC19 ({} bp, circular): {} features | first call (incl. index build) {:.1?} | cached {:.1?}",
            seq.len(),
            feats.len(),
            first,
            cached
        );
        for f in &feats {
            println!(
                "  {:<24} {:<12} {:>4}..{:<4} {}  id {:.1}  cov {:.1}%  frag {}",
                f.name, f.ftype, f.start, f.end, f.strand, f.identity, f.coverage, f.fragment
            );
        }
        assert!(cached.as_secs_f64() < 1.0, "annotation too slow: {cached:?}");

        let amp = feats
            .iter()
            .find(|f| f.name == "AmpR" && f.ftype == "CDS")
            .expect("AmpR CDS");
        assert_eq!(amp.strand, "-");
        assert!((amp.start - 1625).abs() <= 15, "AmpR start {}", amp.start);
        assert!((amp.end - 2485).abs() <= 15, "AmpR end {}", amp.end);

        let ori = feats
            .iter()
            .find(|f| f.ftype == "rep_origin")
            .expect("rep_origin");
        assert!((ori.start - 866).abs() <= 15, "ori start {}", ori.start);
        assert!((ori.end - 1454).abs() <= 15, "ori end {}", ori.end);

        let lacz = feats.iter().find(|f| f.name == "lacZα").expect("lacZα feature");
        // the 174 bp db feature matches a 166 bp window of the 324 bp lacZα
        // fragment at [241, 406] (inside the gbk span 145..468); 95.4%
        // coverage at 97.6% identity counts as complete.
        assert!(!lacz.fragment, "lacZα at 95.4% coverage should be complete");
        assert!(
            (lacz.start - 241).abs() <= 10,
            "lacZα start {}",
            lacz.start
        );

        let mcs = feats.iter().find(|f| f.name == "MCS").expect("MCS");
        assert!((mcs.start - 395).abs() <= 10, "MCS start {}", mcs.start);
        assert!((mcs.end - 451).abs() <= 10, "MCS end {}", mcs.end);

        let amp_prom = feats
            .iter()
            .find(|f| f.name == "AmpR promoter")
            .expect("AmpR promoter");
        assert!(
            (amp_prom.start - 2486).abs() <= 10,
            "AmpR promoter start {}",
            amp_prom.start
        );

        for expect in [
            "lac promoter",
            "CAP binding site",
            "lac operator",
            "bom",
            "lacI",
            "rop",
        ] {
            assert!(
                feats.iter().any(|f| f.name.contains(expect)),
                "missing {expect}"
            );
        }

        assert!(
            feats.iter().all(|f| f.ftype != "primer_bind"),
            "primer_bind features must be filtered"
        );
    }

    #[test]
    fn short_feature_exact_hit() {
        let f = db_feature("lac_operator_(symmetric)"); // 20 bp protein_bind
        assert_eq!(f.fwd.len(), 20);
        let filler = "CGCATGTACGCATGACGTACGTAGCTAGCTAGCATCGATGCTAGCATGCA";
        let start = filler.len();
        let seq = format!("{}{}{}", filler, String::from_utf8(f.fwd.clone()).unwrap(), filler);
        let feats = annotate_sequence(&seq, false);
        let hit = feats
            .iter()
            .find(|x| x.start == start as i64 && x.end == start as i64 + 19 && x.ftype == "protein_bind")
            .expect("20 bp protein_bind hit");
        assert_eq!(hit.strand, "+");
        assert!(!hit.fragment);
        assert!((hit.identity - 100.0).abs() < 1e-9);
    }

    #[test]
    fn circular_origin_wrapping_feature() {
        let f = db_feature("MCS_(8)"); // 57 bp
        assert_eq!(f.fwd.len(), 57);
        // 80 bp circle: MCS tail (27 bp) at the start, MCS head (30 bp) at the
        // end, so the feature crosses the origin at position 80/0.
        let filler = "GCATGCATGGCATTCGAGCGTAC"; // 23 bp
        let seq = format!(
            "{}{}{}",
            String::from_utf8(f.fwd[30..].to_vec()).unwrap(),
            filler,
            String::from_utf8(f.fwd[..30].to_vec()).unwrap()
        );
        assert_eq!(seq.len(), 80);
        let feats = annotate_sequence(&seq, true);
        let hit = feats
            .iter()
            .find(|x| x.name == "MCS")
            .expect("MCS crossing the origin");
        assert_eq!(hit.strand, "+");
        assert!(!hit.fragment);
        assert!(hit.start > hit.end, "wrapped hit {}-{}", hit.start, hit.end);
        assert_eq!(hit.start, 50);
        assert_eq!(hit.end, 26);
        let segs: Vec<(i64, i64)> = hit.segments.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(segs, vec![(50, 79), (0, 26)]);
    }

    #[test]
    fn reverse_strand_feature() {
        let f = db_feature("MCS_(8)");
        let filler = "GATCGATCGATCGTAGCTAGCATCGATCGATCGATGC";
        let start = filler.len();
        let seq = format!("{}{}{}", filler, String::from_utf8(f.rc.clone()).unwrap(), filler);
        let feats = annotate_sequence(&seq, false);
        let hit = feats
            .iter()
            .find(|x| x.name == "MCS" && x.start == start as i64)
            .expect("reverse-strand MCS");
        assert_eq!(hit.end, start as i64 + 56);
        assert_eq!(hit.strand, "-");
        assert!((hit.identity - 100.0).abs() < 1e-9);
    }

    #[test]
    fn overlap_elimination_keeps_highest_score() {
        // lacZ (3075 bp) contains lacZ_alpha (174 bp) as a substring: a query
        // carrying the full lacZ gene produces overlapping hits for both, and
        // the greedy overlap removal must keep only the higher-scoring lacZ.
        let lacz = db_feature("lacZ");
        let lacza = db_feature("lacZ_alpha");
        let lacz_seq = String::from_utf8(lacz.fwd.clone()).unwrap();
        let lacza_seq = String::from_utf8(lacza.fwd.clone()).unwrap();
        assert!(lacz_seq.contains(&lacza_seq), "lacZ_alpha must be in lacZ");

        let filler = "GTACGATCGTAGCTAGCATGCTAGCTAGCATCGATCG";
        let start = filler.len() as i64;
        let seq = format!("{}{}", filler, lacz_seq);
        let feats = annotate_sequence(&seq, false);
        let hit = feats
            .iter()
            .find(|x| x.ftype == "CDS" && x.name == "lacZ")
            .expect("full lacZ hit");
        assert_eq!(
            (hit.start, hit.end),
            (start, start + lacz.fwd.len() as i64 - 1)
        );
        assert!(
            !feats.iter().any(|x| x.name == "lacZα"),
            "lower-scoring lacZα overlapping lacZ must be dropped"
        );
    }

    /// Reverse-translate a protein with an alternative codon per residue, so
    /// the result encodes the same protein with drifted nucleotides.
    fn alt_reverse_translate(prot: &[u8]) -> String {
        fn codon(aa: u8) -> &'static [u8] {
            match aa {
                b'A' => b"GCA",
                b'R' => b"CGT",
                b'N' => b"AAT",
                b'D' => b"GAT",
                b'C' => b"TGT",
                b'Q' => b"CAA",
                b'E' => b"GAA",
                b'G' => b"GGT",
                b'H' => b"CAC",
                b'I' => b"ATT",
                b'L' => b"TTA",
                b'K' => b"AAA",
                b'M' => b"ATG",
                b'F' => b"TTT",
                b'P' => b"CCT",
                b'S' => b"AGT",
                b'T' => b"ACA",
                b'W' => b"TGG",
                b'Y' => b"TAT",
                b'V' => b"GTT",
                b'*' => b"TAA",
                other => panic!("no alt codon for {:?}", other as char),
            }
        }
        let mut out = String::with_capacity(prot.len() * 3);
        for &aa in prot {
            out.push_str(std::str::from_utf8(codon(aa)).unwrap());
        }
        out
    }

    #[test]
    fn codon_optimized_cds_detected_at_protein_level() {
        let kan = db_feature("KanR_(3)");
        let prot = kan.protein.as_ref().expect("CDS has a translation");
        let opt = alt_reverse_translate(prot);
        // The re-coded sequence must exceed the nt mismatch budget, proving
        // the nucleotide path alone cannot produce a full-length hit.
        let mismatches = kan
            .fwd
            .iter()
            .zip(opt.as_bytes())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            mismatches * 20 > kan.fwd.len(),
            "alt codons diverge too little: {mismatches}/{}",
            kan.fwd.len()
        );

        let filler = "CGCATGTACGCATGACGTACGTAGCTAGCTAGCATCGATGCTAGCATGCA";
        let start = filler.len() as i64;
        let seq = format!("{}{}{}", filler, opt, filler);
        let feats = annotate_sequence(&seq, false);
        let hit = feats
            .iter()
            .find(|x| x.name == "KanR" && x.ftype == "CDS")
            .expect("codon-optimized KanR");
        assert_eq!(hit.match_level, "aa");
        assert_eq!(hit.strand, "+");
        assert_eq!(hit.start, start);
        assert_eq!(hit.end, start + opt.len() as i64 - 1);
        assert!((hit.identity - 100.0).abs() < 1e-9);
        assert!(!hit.fragment);
    }

    #[test]
    fn codon_optimized_cds_reverse_strand() {
        let kan = db_feature("KanR_(3)");
        let opt = alt_reverse_translate(kan.protein.as_ref().unwrap());
        let opt_rc = String::from_utf8(rev_comp(opt.as_bytes())).unwrap();
        let filler = "CGCATGTACGCATGACGTACGTAGCTAGCTAGCATCGATGCTAGCATGCA";
        let start = filler.len() as i64;
        let seq = format!("{}{}{}", filler, opt_rc, filler);
        let feats = annotate_sequence(&seq, false);
        let hit = feats
            .iter()
            .find(|x| x.name == "KanR" && x.ftype == "CDS")
            .expect("codon-optimized KanR on the reverse strand");
        assert_eq!(hit.match_level, "aa");
        assert_eq!(hit.strand, "-");
        assert_eq!(hit.start, start);
        assert_eq!(hit.end, start + opt_rc.len() as i64 - 1);
    }

    #[test]
    fn protein_query_matches_cds_translations() {
        let kan = db_feature("KanR_(3)");
        let prot = String::from_utf8(kan.protein.clone().unwrap()).unwrap();
        let feats = annotate_protein(&prot, false);
        let hit = feats
            .iter()
            .find(|x| x.name == "KanR" && x.ftype == "CDS")
            .expect("KanR protein hit");
        assert_eq!(hit.match_level, "aa");
        assert_eq!(hit.strand, "+");
        assert_eq!((hit.start, hit.end), (0, prot.len() as i64 - 1));
        assert!((hit.identity - 100.0).abs() < 1e-9);
        assert!((hit.coverage - 100.0).abs() < 1e-9);
        assert!(!hit.fragment);
    }

    #[test]
    fn short_protein_tag_detected() {
        let his = db_feature("6xHis");
        let prot = String::from_utf8(his.protein.clone().unwrap()).unwrap();
        assert_eq!(prot.len(), 6);
        // Protein query: exact 6 aa hit from the window scan.
        let feats = annotate_protein(&format!("MGG{}GG", prot), false);
        let hit = feats
            .iter()
            .find(|x| x.name.contains("His"))
            .expect("6xHis in protein query");
        assert_eq!((hit.start, hit.end), (3, 8));
        assert!((hit.identity - 100.0).abs() < 1e-9);

        // DNA query with synonymous codons: invisible to the nt short scan
        // (budget 0 mismatches), caught at the protein level.
        let opt = alt_reverse_translate(his.protein.as_ref().unwrap());
        assert_ne!(opt.as_bytes(), his.fwd.as_slice());
        let filler = "CGCATGTACGCATGACGTACGTAGCTAGCTAGCATCGATGCTAGCATGCA";
        let start = filler.len() as i64;
        let seq = format!("{}{}{}", filler, opt, filler);
        let feats = annotate_sequence(&seq, false);
        let hit = feats
            .iter()
            .find(|x| x.name.contains("His"))
            .expect("6xHis with synonymous codons");
        assert_eq!(hit.match_level, "aa");
        assert_eq!((hit.start, hit.end), (start, start + 17));
    }
}
