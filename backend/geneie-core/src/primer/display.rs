//! EMBOSS-style plain-text alignment display for primers.
//!
//! Converts a Smith-Waterman [`AlignmentResult`] into a three-line text block:
//!
//! ```text
//! Template       32 AGTTGTGTTCAAGCATATTTGCTGAGCGA 60
//!                    |||||||||||||||||||||||||||||
//! Primer          1 AGTTGTGTTCAAGCATATTTGCTGAGCGA 29
//!                 5' -> 3'
//! ```

use super::alignment::{AlignmentResult, Op};
use super::thermodynamics;

/// Format an [`AlignmentResult`] as EMBOSS-style pairwise alignment text.
///
/// - `primer_seq` — the full primer sequence (5′→3′) as displayed.
/// - `template_region` — the contiguous template segment that `result` was
///   aligned against (the same slice passed to [`super::alignment::align`]).
/// - `result` — the alignment result from the Smith-Waterman engine.
/// - `template_name` / `primer_name` — labels shown in the left column.
/// - `window_offset` — 0-based position of `template_region[0]` in the **full**
///   template sequence, used to compute 1-based display coordinates.
/// - `is_rev` — if true, add a `3' <- 5'` direction indicator; otherwise `5' -> 3'`.
pub fn format_alignment_text(
    primer_seq: &[u8],
    template_region: &[u8],
    result: &AlignmentResult,
    template_name: &str,
    primer_name: &str,
    window_offset: usize,
    is_rev: bool,
) -> String {
    let primer_upper: Vec<u8> = primer_seq.iter().map(|&b| b.to_ascii_uppercase()).collect();

    let mut template_aln = String::new();
    let mut match_aln = String::new();
    let mut primer_aln = String::new();

    for pair in &result.ops {
        let tb = pair.template_pos.map(|p| template_region[p].to_ascii_uppercase() as char);
        let pb = pair.primer_pos.map(|p| primer_upper[p] as char);

        match pair.op {
            Op::Match => {
                template_aln.push(tb.unwrap());
                match_aln.push('|');
                primer_aln.push(pb.unwrap());
            }
            Op::Mismatch => {
                template_aln.push(tb.unwrap());
                match_aln.push(' ');
                primer_aln.push(pb.unwrap());
            }
            Op::Del => {
                template_aln.push(tb.unwrap());
                match_aln.push(' ');
                primer_aln.push('-');
            }
            Op::Ins => {
                template_aln.push('-');
                match_aln.push(' ');
                primer_aln.push(pb.unwrap());
            }
        }
    }

    // 1-based display offsets for the aligned region
    let tstart = window_offset + result.template_start + 1;
    let tend   = window_offset + result.template_end;
    let pstart = result.primer_start + 1;
    let pend   = result.primer_end;

    // Layout: name=16, start=6 chars, seq=variable, end=6 chars
    let name_w = 16;
    let pos_w  = 6;

    let line1 = format!(
        "{:<nw$}{:>pw$} {} {:<pw$}",
        template_name, tstart, template_aln, tend,
        nw = name_w, pw = pos_w,
    );
    // Match line indentation: name_w(16) + 1 space + pos_w(6) + 1 space = 24 chars before seq.
    // So match chars need 24 leading spaces to align.  We right-pad an empty string:
    //   name_w(16) + "" + 1 space + pos_w(6) + "" + 1 space => 16+1+6+1 = 24.
    let line2 = format!(
        "{:<nw$}{:>pw$} {}",
        "", "", match_aln,
        nw = name_w, pw = pos_w,
    );
    let pstart_str = pstart.to_string();
    let pend_str = pend.to_string();

    let line3 = if is_rev {
        // (3') with a space before the start number.
        let label_left = format!("(3') {}", pstart_str);
        let name_chars = (name_w + pos_w + 1)
            .saturating_sub(label_left.len() + 1)
            .max(primer_name.len());
        format!(
            "{:nw$}(3') {} {} {} (5')",
            primer_name, pstart_str, primer_aln, pend_str,
            nw = name_chars,
        )
    } else {
        let label_left = format!("(5') {}", pstart_str);
        let name_chars = (name_w + pos_w + 1)
            .saturating_sub(label_left.len() + 1)
            .max(primer_name.len());
        format!(
            "{:nw$}(5') {} {} {} (3')",
            primer_name, pstart_str, primer_aln, pend_str,
            nw = name_chars,
        )
    };

    format!("{}\n{}\n{}", line1, line2, line3)
}

/// Compute melting temperature from the aligned primer bases in an
/// [`AlignmentResult`].  Extracts every primer base that participates
/// (matches, mismatches, and insertions) and runs nearest-neighbour Tm.
pub fn compute_tm_from_alignment(
    primer_seq: &[u8],
    result: &AlignmentResult,
) -> f64 {
    let matched: String = result
        .ops
        .iter()
        .filter_map(|op| op.primer_pos)
        .map(|i| primer_seq[i].to_ascii_uppercase() as char)
        .collect();
    if matched.len() < 2 {
        return 0.0;
    }
    thermodynamics::compute_tm(&matched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primer::alignment::align;

    #[test]
    fn test_format_exact_match() {
        let primer = b"AGTTGTGTTCAAGCATATTTGCTGAGCGA";
        let template = b"NNNNNAGTTGTGTTCAAGCATATTTGCTGAGCGANNNNN";
        let result = align(primer, template).unwrap();
        let text = format_alignment_text(primer, template, &result, "Template", "Primer", 0, false);

        assert!(text.contains("AGTTGTGTTCAAGCATATTTGCTGAGCGA"));
        assert!(text.contains("|||||||||||||||||||||||||||"));
        assert!(text.contains("Template"));
        assert!(text.contains("Primer"));
        assert!(text.contains("5'"));
        println!("\n=== Exact match ===\n{}", text);
    }

    #[test]
    fn test_format_with_mismatch() {
        let primer = b"ATGCATGCAAAA";
        let template = b"NNNNATGCATTCAAAANNNN";
        let result = align(primer, template).unwrap();
        let text = format_alignment_text(primer, template, &result, "Template", "Primer", 0, false);

        assert!(text.contains("ATGCAT"));
        assert!(text.lines().nth(1).unwrap().contains(" "), "expected a space in match line for mismatch");
        println!("\n=== With mismatch ===\n{}", text);
    }

    #[test]
    fn test_format_window_offset() {
        let primer = b"ATGCATGC";
        let template = b"NNNNNNNNATGCATGCNNNNNNNN";
        let result = align(primer, template).unwrap();
        let text = format_alignment_text(primer, template, &result, "Tpl", "Fwd", 8, false);

        assert!(text.contains("Tpl"));
        assert!(text.contains("Fwd"));
        println!("\n=== Window offset 8 ===\n{}", text);
    }

    #[test]
    fn test_format_rev_direction() {
        let primer = b"ATGCATGC";
        let template = b"NNNNNNNNATGCATGCNNNNNNNN";
        let result = align(primer, template).unwrap();
        let text = format_alignment_text(primer, template, &result, "Tpl", "Rev", 8, true);

        // Rev mode: primer line should have (3') before start, (5') after end.
        let primer_line = text.lines().nth(2).unwrap();
        assert!(primer_line.contains("(3')"), "expected (3') in rev primer line");
        assert!(primer_line.contains("(5')"), "expected (5') in rev primer line");
        let three_pos = primer_line.find("(3')").unwrap();
        let five_pos = primer_line.rfind("(5')").unwrap();
        let end_pos = primer_line.rfind("8").unwrap();
        assert!(three_pos < primer_line.find('1').unwrap(),
            "(3') should be before start position");
        assert!(five_pos > end_pos,
            "(5') should be after end position");
        println!("\n=== Rev direction ===\n{}", text);
    }
}
