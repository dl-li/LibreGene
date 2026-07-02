//! Compact centred-layout alignment display for primers.
//!
//! Formats into a 5-line block:
//!
//! ```text
//!             Template
//!   120 CTACTAGGGCGAATTG 136
//!       ||||||||||||||||
//!  5' > CTACTAGGGCGAATTG > 3'
//!             V20-Fs
//! ```

use super::alignment::{AlignmentResult, Op};
use super::thermodynamics;

/// Format an [`AlignmentResult`] into a compact centred 5-line block.
///
/// Lines 1 (template name) and 5 (primer name) are centred over the sequence.
/// Position numbers appear on line 2.  Match chars on line 3.  Arrow indicators
/// on line 4.  The frontend applies colours per line index.
pub fn format_alignment_text(
    primer_seq: &[u8],
    template_region: &[u8],
    result: &AlignmentResult,
    template_name: &str,
    primer_name: &str,
    window_offset: usize,
    is_rev: bool,
) -> String {
    let primer_upper: Vec<u8> = primer_seq.to_vec(); // preserve original case

    let mut template_aln = String::new();
    let mut match_aln = String::new();
    let mut primer_aln = String::new();

    for pair in &result.ops {
        let tb =
            pair.template_pos.map(|p| template_region[p].to_ascii_uppercase() as char);
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

    // 1-based display offsets
    let tstart = window_offset + result.template_start + 1;
    let tend = window_offset + result.template_end;

    let tstart_str = tstart.to_string();
    let tend_str = tend.to_string();
    let seq_len = template_aln.len();

    // Position-number column width (dynamic to the actual digits).
    let pos_w = tstart_str.len().max(tend_str.len()).max(1);

    // Left prefix width: 1 space + pos_w digits + 1 space = pos_w + 2.
    // We match this to the arrow prefix so that the sequence column is aligned
    // across all lines.
    let arrow_left_str = if is_rev { "3' < " } else { "5' > " };
    // left_prefix_width ensures the arrow fits and the sequences line up
    let left_prefix_width = (pos_w + 2).max(arrow_left_str.len());

    // ----- Line 1: template name centred over the sequence -----
    let line1_seq_centred = centre(template_name, seq_len);
    let line1 = format!("{:>lw$}{}", "", line1_seq_centred, lw = left_prefix_width);

    // ----- Line 2: template sequence with flanking positions -----
    // Left prefix: right-justified position number, padded so the sequence
    // column aligns with lines 3 & 4.
    let extra_left = left_prefix_width.saturating_sub(pos_w + 1);
    let extra_right = left_prefix_width.saturating_sub(pos_w + 1);
    let line2 = format!(
        "{:>el$}{} {} {:<er$}",
        "",
        tstart_str,
        template_aln,
        tend_str,
        el = extra_left,
        er = extra_right,
    );

    // ----- Line 3: match / mismatch indicators -----
    let line3 = format!("{:>lw$}{}", "", match_aln, lw = left_prefix_width);

    // ----- Line 4: primer sequence with arrow indicators -----
    let arrow_right_str = if is_rev { "< 5'" } else { "> 3'" };
    // Left arrow part padded to left_prefix_width
    let left_arrow = format!("{:>lw$}", arrow_left_str, lw = left_prefix_width);
    let line4 = format!("{}{} {}", left_arrow, primer_aln, arrow_right_str);

    // ----- Line 5: primer name centred over the sequence -----
    let line5_seq_centred = centre(primer_name, seq_len);
    let line5 = format!("{:>lw$}{}", "", line5_seq_centred, lw = left_prefix_width);

    format!("{}\n{}\n{}\n{}\n{}", line1, line2, line3, line4, line5)
}

/// Pad `text` with spaces so it appears centred within `width`.
fn centre(text: &str, width: usize) -> String {
    if text.len() >= width {
        return text.to_string();
    }
    let left = (width - text.len()) / 2;
    let right = width - text.len() - left;
    format!("{:>l$}{}{:<r$}", "", text, "", l = left, r = right)
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
        assert!(text.lines().nth(2).unwrap().contains(" "), "expected a space in match line for mismatch");
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

        // Line 4 (index 3) is the primer arrow line.
        let primer_line = text.lines().nth(3).unwrap();
        assert!(primer_line.contains("3' <"), "expected 3' < in rev primer line");
        assert!(primer_line.contains("< 5'"), "expected < 5' in rev primer line");
        let arrow_pos = primer_line.find("3'").unwrap();
        let end_arrow_pos = primer_line.rfind("5'").unwrap();
        let first_base = primer_line.find('A').unwrap();
        assert!(arrow_pos < first_base,
            "3' should be before the sequence");
        assert!(end_arrow_pos > primer_line.rfind('C').unwrap_or(0),
            "5' should be after the sequence");
        // Line 5 (index 4) is the primer name.
        let name_line = text.lines().nth(4).unwrap();
        assert!(name_line.contains("Rev"), "expected Rev on the name line");
        println!("\n=== Rev direction ===\n{}", text);
    }
}
