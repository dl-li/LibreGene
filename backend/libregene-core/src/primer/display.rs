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
use super::thermodynamics::TmParams;

/// Format an [`AlignmentResult`] into a compact centred 5-line block.
///
/// Lines 1 (template name) and 5 (primer name) are centred over the sequence.
/// Position numbers appear on line 2.  Match chars on line 3.  Arrow indicators
/// on line 4.  The frontend applies colours per line index.
///
/// `template_len` is the full template length for circular templates (position
/// numbers wrap modulo it); pass 0 for linear templates.
pub fn format_alignment_text(
    primer_seq: &[u8],
    template_region: &[u8],
    result: &AlignmentResult,
    template_name: &str,
    primer_name: &str,
    window_offset: usize,
    is_rev: bool,
    template_len: usize,
) -> String {
    let primer_upper: Vec<u8> = primer_seq.to_vec(); // preserve original case

    // Unaligned primer ends (free end-gaps: 5' tails / overhangs) are outside
    // `result.ops`; show them as overhangs flanking the aligned block.
    let left_tail: String = primer_upper[..result.primer_start]
        .iter()
        .map(|&b| b as char)
        .collect();
    let right_tail: String = primer_upper[result.primer_end..]
        .iter()
        .map(|&b| b as char)
        .collect();

    // Splice the overhangs back in, extending the template display so the
    // region facing each tail is shown too (paired bases marked with '|').
    let pair_fn = |p: u8, t: u8| {
        if is_rev {
            crate::primer::iupac::bases_pair(p, t)
        } else {
            crate::primer::iupac::bases_overlap(p, t)
        }
    };
    let tpl_b = |p: usize| template_region[p].to_ascii_uppercase() as char;

    let left_ext = left_tail.len().min(result.template_start);
    let right_ext = right_tail
        .len()
        .min(template_region.len().saturating_sub(result.template_end));

    let mut template_aln = String::new();
    let mut match_aln = String::new();
    // Left flank: pad where the tail has no template opposite, then template
    // bases before the aligned block facing the rest of the tail (the tail's
    // rightmost base sits opposite template_start - 1).
    for _ in left_ext..left_tail.len() {
        template_aln.push(' ');
        match_aln.push(' ');
    }
    for k in (0..left_ext).rev() {
        let t = template_region[result.template_start - 1 - k];
        let p = primer_upper[result.primer_start - 1 - k];
        template_aln.push(tpl_b(result.template_start - 1 - k));
        match_aln.push(if pair_fn(p, t) { '|' } else { ' ' });
    }
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

    // Right flank: template bases after the aligned block facing the right
    // tail, then pad where the tail has no template opposite.
    for k in 0..right_ext {
        let t = template_region[result.template_end + k];
        let p = primer_upper[result.primer_end + k];
        template_aln.push(tpl_b(result.template_end + k));
        match_aln.push(if pair_fn(p, t) { '|' } else { ' ' });
    }
    for _ in right_ext..right_tail.len() {
        template_aln.push(' ');
        match_aln.push(' ');
    }
    primer_aln = format!("{}{}{}", left_tail, primer_aln, right_tail);
    let seq_len = primer_aln.len();

    // 1-based display offsets covering the extended template span
    let tstart = window_offset + result.template_start - left_ext + 1;
    let tend_raw = window_offset + result.template_end + right_ext;
    let tend = if template_len > 0 && tend_raw > template_len {
        (tend_raw - 1) % template_len + 1
    } else {
        tend_raw
    };

    let tstart_str = tstart.to_string();
    let tend_str = tend.to_string();

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
    compute_tm_from_alignment_with_params(primer_seq, result, &TmParams::default())
}

/// Like [`compute_tm_from_alignment`] but with configurable PCR conditions.
pub fn compute_tm_from_alignment_with_params(
    primer_seq: &[u8],
    result: &AlignmentResult,
    params: &TmParams,
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
    thermodynamics::compute_tm_with_params(&matched, params)
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
        let text = format_alignment_text(primer, template, &result, "Template", "Primer", 0, false, 0);

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
        let text = format_alignment_text(primer, template, &result, "Template", "Primer", 0, false, 0);

        assert!(text.contains("ATGCAT"));
        assert!(text.lines().nth(2).unwrap().contains(" "), "expected a space in match line for mismatch");
        println!("\n=== With mismatch ===\n{}", text);
    }

    #[test]
    fn test_format_window_offset() {
        let primer = b"ATGCATGC";
        let template = b"NNNNNNNNATGCATGCNNNNNNNN";
        let result = align(primer, template).unwrap();
        let text = format_alignment_text(primer, template, &result, "Tpl", "Fwd", 8, false, 0);

        assert!(text.contains("Tpl"));
        assert!(text.contains("Fwd"));
        println!("\n=== Window offset 8 ===\n{}", text);
    }

    #[test]
    fn test_format_rev_direction() {
        let primer = b"ATGCATGC";
        let template = b"NNNNNNNNATGCATGCNNNNNNNN";
        let result = align(primer, template).unwrap();
        let text = format_alignment_text(primer, template, &result, "Tpl", "Rev", 8, true, 0);

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

    #[test]
    fn test_format_rev_tail_overhang() {
        // Reverse primer with a non-matching 5' tail: the tail must still be
        // visible as an overhang at the right (5') end of the primer line.
        let query = b"CCTCGTTAGTGTCCACTCGTTTTTTGAGCTCGCC";
        let template = b"NNNNGGAGCAATCACAGGTGAGCAAAAAAGCCACCATGGNNNN";
        let result = crate::primer::alignment::align_first_base_constrained_rev(query, template)
            .unwrap();
        let text = format_alignment_text(query, template, &result, "Tpl", "Rev", 0, true, 0);
        let primer_line = text.lines().nth(3).unwrap();
        assert!(
            primer_line.contains("GAGCTCGCC"),
            "tail overhang must appear in primer line:\n{}",
            text
        );
        // The template region facing the tail must be shown, not left blank.
        let template_line = text.lines().nth(1).unwrap();
        assert!(
            template_line.contains("GCCACCATG"),
            "template line must extend under the tail:\n{}",
            text
        );
        println!("\n=== Rev tail overhang ===\n{}", text);
    }

    #[test]
    fn test_format_fwd_tail_overhang() {
        // Forward primer whose 5' tail hangs off the template left edge.
        let primer = b"GGGGGGATGCATGC";
        let template = b"ATGCATGC";
        let result = align(primer, template).unwrap();
        let text = format_alignment_text(primer, template, &result, "Tpl", "Fwd", 0, false, 0);
        let primer_line = text.lines().nth(3).unwrap();
        assert!(
            primer_line.contains("GGGGGGATGCATGC"),
            "5' tail must appear in primer line:\n{}",
            text
        );
        println!("\n=== Fwd tail overhang ===\n{}", text);
    }

    #[test]
    fn test_format_circular_position_wrap() {
        // Circular template: the displayed end position wraps modulo the
        // template length instead of running past it.
        let primer = b"ATGCATGC";
        let template = b"ATGCATGCNNNN";
        let result = align(primer, template).unwrap();
        // Window starts at 3120 (0-based) of a 3125-bp circular template; raw
        // end would be 3120 + 8 = 3128 > 3125, wrapping to 3.
        let text =
            format_alignment_text(primer, template, &result, "Tpl", "Rev", 3120, true, 3125);
        let template_line = text.lines().nth(1).unwrap();
        assert!(template_line.contains("3121"), "start position:\n{}", text);
        assert!(
            template_line.trim_end().ends_with('3'),
            "end position must wrap to 3:\n{}",
            text
        );
        assert!(!text.contains("3128"), "unwrapped position leaked:\n{}", text);
        println!("\n=== Circular wrap ===\n{}", text);
    }
}
