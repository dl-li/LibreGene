//! Compact flanked-layout alignment display for primers.
//!
//! Formats into a 5-line block with 10 bp of template flank on each side of
//! the aligned region and position markers at its boundaries. The dialog
//! reorders the block for forward primers (primer on top) so the markers sit
//! next to the template line and point at it:
//!
//! ```text
//!           ↑120             ↑136
//! NNNNNNNNNNCTACTAGGGCGAATTGNNNNNNNNNN
//!           ||||||||||||||||
//!      5' > CTACTAGGGCGAATTG > 3'
//!                 V20-Fs
//! ```
//!
//! Forward primers use ↑ markers, reverse primers use ↓.

use super::alignment::{AlignmentResult, Op};
use super::thermodynamics;
use super::thermodynamics::TmParams;

/// Format an [`AlignmentResult`] into a compact 5-line block.
///
/// Line 1 carries ↑/↓ position markers at the boundaries of the aligned
/// region (↓ for reverse primers). Line 2 shows the template with up to 10 bp
/// of flanking sequence on each side. Line 3 has match chars, line 4 the
/// primer with arrow indicators, line 5 the primer name centred over the
/// aligned region. The frontend applies colours per line index.
///
/// `template_len` is the full template length: position numbers wrap modulo
/// it when `circular`, and it bounds the ellipsis detection at fragment ends.
pub fn format_alignment_text(
    primer_seq: &[u8],
    template_region: &[u8],
    result: &AlignmentResult,
    _template_name: &str,
    primer_name: &str,
    window_offset: usize,
    is_rev: bool,
    template_len: usize,
    circular: bool,
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

    let pair_fn = |p: u8, t: u8| {
        if is_rev {
            crate::primer::iupac::bases_pair(p, t)
        } else {
            crate::primer::iupac::bases_overlap(p, t)
        }
    };
    let tpl_b = |p: usize| template_region[p].to_ascii_uppercase() as char;

    // Up to 10 bp of template flank on each side of the aligned block (less
    // near the edges of a linear template).
    const FLANK: usize = 10;
    let lf = FLANK.min(result.template_start);
    let rf = FLANK.min(template_region.len().saturating_sub(result.template_end));

    // Aligned block columns.
    let mut tpl_block = String::new();
    let mut match_block = String::new();
    let mut primer_block = String::new();
    for pair in &result.ops {
        let tb =
            pair.template_pos.map(|p| template_region[p].to_ascii_uppercase() as char);
        let pb = pair.primer_pos.map(|p| primer_upper[p] as char);

        match pair.op {
            Op::Match => {
                tpl_block.push(tb.unwrap());
                match_block.push('|');
                primer_block.push(pb.unwrap());
            }
            Op::Mismatch => {
                tpl_block.push(tb.unwrap());
                match_block.push(' ');
                primer_block.push(pb.unwrap());
            }
            Op::Del => {
                tpl_block.push(tb.unwrap());
                match_block.push(' ');
                primer_block.push('-');
            }
            Op::Ins => {
                tpl_block.push('-');
                match_block.push(' ');
                primer_block.push(pb.unwrap());
            }
        }
    }
    let block_len = match_block.len();

    // Leading column where the aligned block starts: wide enough for the left
    // flank, the 5' tail plus its arrow prefix.
    let arrow_left_str = if is_rev { "3' < " } else { "5' > " };
    let arrow_right_str = if is_rev { "< 5'" } else { "> 3'" };
    let lead = lf.max(left_tail.len() + arrow_left_str.len()).max(arrow_left_str.len());
    let trail = rf.max(right_tail.len());

    // ----- Line 1: position markers at the aligned region boundaries -----
    let wrap_pos = |p: usize| if circular { (p - 1) % template_len + 1 } else { p };
    let tstart = wrap_pos(window_offset + result.template_start + 1);
    let tend = wrap_pos(window_offset + result.template_end);
    let marker = if is_rev { '↓' } else { '↑' };
    let s1: Vec<char> = format!("{}{}", marker, tstart).chars().collect();
    let s2: Vec<char> = format!("{}{}", marker, tend).chars().collect();
    // Arrow chars sit exactly over the first / last aligned template base;
    // the position digits extend rightward from each arrow.
    let mut s2_start = lead + block_len.saturating_sub(1);
    if s2_start < lead + s1.len() + 1 {
        s2_start = lead + s1.len() + 1;
    }
    let mut line1_chars = vec![' '; s2_start + s2.len()];
    for (i, &c) in s1.iter().enumerate() {
        line1_chars[lead + i] = c;
    }
    for (i, &c) in s2.iter().enumerate() {
        line1_chars[s2_start + i] = c;
    }
    let line1: String = line1_chars.iter().collect::<String>().trim_end().to_string();

    // ----- Line 2: template sequence with 10 bp flanks -----
    let mut line2 = String::new();
    line2.push_str(&" ".repeat(lead - lf));
    for k in (0..lf).rev() {
        line2.push(tpl_b(result.template_start - 1 - k));
    }
    line2.push_str(&tpl_block);
    for k in 0..rf {
        line2.push(tpl_b(result.template_end + k));
    }

    // ----- Line 3: match / mismatch indicators -----
    let mut m = vec![' '; lead + block_len + trail];
    for (i, c) in match_block.chars().enumerate() {
        m[lead + i] = c;
    }
    // Tail bases facing template flank still get match indicators.
    let lext = left_tail.len().min(lf);
    for k in 0..lext {
        let p = primer_upper[result.primer_start - 1 - k];
        let t = template_region[result.template_start - 1 - k];
        if pair_fn(p, t) {
            m[lead - 1 - k] = '|';
        }
    }
    let rext = right_tail.len().min(rf);
    for k in 0..rext {
        let p = primer_upper[result.primer_end + k];
        let t = template_region[result.template_end + k];
        if pair_fn(p, t) {
            m[lead + block_len + k] = '|';
        }
    }
    let line3: String = m.iter().collect::<String>().trim_end().to_string();

    // ----- Line 4: primer sequence with arrow indicators -----
    let mut line4 = String::new();
    line4.push_str(&" ".repeat(lead - left_tail.len() - arrow_left_str.len()));
    line4.push_str(arrow_left_str);
    line4.push_str(&left_tail);
    line4.push_str(&primer_block);
    line4.push_str(&right_tail);
    line4.push(' ');
    line4.push_str(arrow_right_str);

    // ----- Line 5: primer name centred over the aligned region -----
    let line5 = format!(
        "{}{}",
        " ".repeat(lead),
        centre(primer_name, block_len)
    )
    .trim_end()
    .to_string();

    // Ellipses mark template bases hidden beyond the displayed flanks. On
    // circular templates both sides continue (unless the whole template fits
    // in view); on linear ones each side stops at the fragment end.
    let shown_span = lf + (result.template_end - result.template_start) + rf;
    let (left_more, right_more) = if circular {
        let more = shown_span < template_len;
        (more, more)
    } else {
        (
            window_offset + result.template_start > lf,
            window_offset + result.template_end + rf < template_len,
        )
    };
    let pre = if left_more { "··· " } else { "    " };
    let right_dots = if right_more { " ···" } else { "" };

    format!(
        "    {}\n{}{}{}\n    {}\n    {}\n    {}",
        line1, pre, line2, right_dots, line3, line4, line5
    )
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
        let text = format_alignment_text(primer, template, &result, "Template", "Primer", 0, false, template.len(), false);

        assert!(text.contains("AGTTGTGTTCAAGCATATTTGCTGAGCGA"));
        assert!(text.contains("|||||||||||||||||||||||||||"));
        assert!(text.contains("Primer"));
        assert!(text.contains("5'"));
        let marker_line = text.lines().next().unwrap();
        assert!(marker_line.contains("↑6"), "start marker:\n{}", text);
        assert!(marker_line.contains("↑34"), "end marker:\n{}", text);
        println!("\n=== Exact match ===\n{}", text);
    }

    #[test]
    fn test_format_with_mismatch() {
        let primer = b"ATGCATGCAAAA";
        let template = b"NNNNATGCATTCAAAANNNN";
        let result = align(primer, template).unwrap();
        let text = format_alignment_text(primer, template, &result, "Template", "Primer", 0, false, template.len(), false);

        assert!(text.contains("ATGCAT"));
        assert!(text.lines().nth(2).unwrap().contains(" "), "expected a space in match line for mismatch");
        println!("\n=== With mismatch ===\n{}", text);
    }

    #[test]
    fn test_format_window_offset() {
        let primer = b"ATGCATGC";
        let template = b"NNNNNNNNATGCATGCNNNNNNNN";
        let result = align(primer, template).unwrap();
        let text = format_alignment_text(primer, template, &result, "Tpl", "Fwd", 8, false, 8 + template.len(), false);

        assert!(text.contains("↑17"), "start marker:\n{}", text);
        assert!(text.contains("Fwd"));
        println!("\n=== Window offset 8 ===\n{}", text);
    }

    #[test]
    fn test_format_rev_direction() {
        let primer = b"ATGCATGC";
        let template = b"NNNNNNNNATGCATGCNNNNNNNN";
        let result = align(primer, template).unwrap();
        let text = format_alignment_text(primer, template, &result, "Tpl", "Rev", 8, true, 8 + template.len(), false);

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
        // visible as an overhang at the right (5') end of the primer line,
        // with template flank bases opposite it (up to the 10 bp flank).
        let query = b"CCTCGTTAGTGTCCACTCGTTTTTTGAGCTCGCC";
        let template = b"NNNNGGAGCAATCACAGGTGAGCAAAAAAGCCACCATGGNNNN";
        let result = crate::primer::alignment::align_first_base_constrained_rev(query, template)
            .unwrap();

        for circular in [true, false] {
            let text = format_alignment_text(
                query, template, &result, "Tpl", "Rev", 0, true, template.len(), circular,
            );
            let primer_line = text.lines().nth(3).unwrap();
            assert!(
                primer_line.contains("GAGCTCGCC"),
                "tail overhang must appear in primer line:\n{}",
                text
            );
            let template_line = text.lines().nth(1).unwrap();
            assert!(
                template_line.contains("GCCACCATG"),
                "template line must show flank under the tail:\n{}",
                text
            );
        }
        println!("\n=== Rev tail overhang ===");
    }

    #[test]
    fn test_format_fwd_tail_overhang() {
        // Forward primer with a 5' tail; template has upstream N bases.
        let primer = b"GGGGGGATGCATGC";
        let template = b"NNNNNNATGCATGC";
        let result = crate::primer::alignment::align_3prime_constrained(primer, template).unwrap();
        let text =
            format_alignment_text(primer, template, &result, "Tpl", "Fwd", 0, false, template.len(), true);
        let template_line = text.lines().nth(1).unwrap();
        assert!(
            template_line.contains("NNNNNNATGCATGC"),
            "template line must extend under the tail:\n{}",
            text
        );

        // Linear: flanks are still shown (they are template sequence, not
        // circular wrap-around), the tail faces the right flank.
        let text = format_alignment_text(primer, template, &result, "Tpl", "Fwd", 0, false, template.len(), false);
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
            format_alignment_text(primer, template, &result, "Tpl", "Rev", 3120, true, 3125, true);
        let marker_line = text.lines().next().unwrap();
        assert!(marker_line.contains("↓3121"), "start marker:\n{}", text);
        assert!(marker_line.contains("↓3"), "end marker must wrap to 3:\n{}", text);
        assert!(!text.contains("3128"), "unwrapped position leaked:\n{}", text);
        println!("\n=== Circular wrap ===\n{}", text);
    }

    #[test]
    fn test_format_ellipsis() {
        // Linear fragment, primer covers the very start: no left ellipsis,
        // right ellipsis because the fragment continues past the right flank.
        let primer = b"ATGCATGC";
        let template = b"ATGCATGCNNNNNNNNNNNNNNNN";
        let result = align(primer, template).unwrap();
        let text = format_alignment_text(
            primer, template, &result, "Tpl", "Fwd", 0, false, template.len(), false,
        );
        let template_line = text.lines().nth(1).unwrap();
        assert!(!template_line.trim_start().starts_with('·'), "no left ellipsis:\n{}", text);
        assert!(template_line.trim_end().ends_with('·'), "right ellipsis:\n{}", text);

        // Same fragment fully covered by block + flanks: no ellipses at all.
        let template = b"ATGCATGCNNNN";
        let result = align(primer, template).unwrap();
        let text = format_alignment_text(
            primer, template, &result, "Tpl", "Fwd", 0, false, template.len(), false,
        );
        assert!(!text.contains('·'), "no ellipses when fully shown:\n{}", text);

        // Circular template always continues on both sides.
        let text = format_alignment_text(
            primer, template, &result, "Tpl", "Fwd", 0, false, 3125, true,
        );
        let template_line = text.lines().nth(1).unwrap();
        assert!(template_line.starts_with('·'), "circular left ellipsis:\n{}", text);
        assert!(template_line.trim_end().ends_with('·'), "circular right ellipsis:\n{}", text);
    }
}
