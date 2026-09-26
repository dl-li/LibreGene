//! Temporary diagnostic: align two real files and dump the conversion result.
use libregene_core::align::{align_read_with, AlignAlgorithm};

fn main() {
    let template_path = std::env::args().nth(1).expect("template path");
    let read_path = std::env::args().nth(2).expect("read path");
    let t = libregene_core::file_io::parse_file(std::path::Path::new(&template_path)).unwrap();
    let r = libregene_core::file_io::parse_file(std::path::Path::new(&read_path)).unwrap();
    let circular = t.topology == "circular";
    println!(
        "template: {} len={} circular={}",
        t.name,
        t.sequence.len(),
        circular
    );
    println!("read: {} len={}", r.name, r.sequence.len());
    let aln = match align_read_with(&t.sequence, &r.sequence, circular, AlignAlgorithm::BlastN) {
        Some(a) => a,
        None => {
            println!("rejected: no significant alignment");
            return;
        }
    };
    println!(
        "aln: strand={} identity={:.4} segments={} insertions={} len={}",
        aln.strand,
        aln.identity,
        aln.segments.len(),
        aln.insertions.len(),
        aln.length
    );
    let read = if aln.strand == "-" {
        libregene_core::utils::reverse_complement(&r.sequence)
    } else {
        r.sequence.clone()
    };
    println!("read first 40: {}", &read[..read.len().min(40)]);
    println!("read last 40: {}", &read[read.len().saturating_sub(40)..]);
    for seg in &aln.segments {
        println!(
            "  seg {}..{} chars[:30]={:?} chars[-30:]={:?}",
            seg.start,
            seg.end,
            &seg.chars[..seg.chars.len().min(30)],
            &seg.chars[seg.chars.len().saturating_sub(30)..]
        );
        let dels = seg.chars.matches('-').count();
        println!("    deletion columns: {dels}");
    }
    for ins in &aln.insertions {
        println!("  ins at {} len={}: {:?}", ins.pos, ins.bases.len(), ins.bases);
    }
    // Where does the read's aligned part start/end in read coordinates?
    let ins_total: usize = aln.insertions.iter().map(|i| i.bases.len()).sum();
    let seg_chars: usize = aln.segments.iter().map(|s| s.chars.bytes().filter(|&c| c != b'-').count()).sum();
    let mapped = seg_chars;
    println!(
        "read bases mapped={} in insertions={} total={} read_len={}",
        mapped,
        ins_total,
        mapped + ins_total,
        read.len()
    );
    let diff = libregene_core::align::alignment_diff(&aln, &t.sequence);
    println!(
        "diff: mismatches={} deletions={} insertions={}",
        diff.mismatches.len(),
        diff.deletions.len(),
        diff.insertions.len()
    );
}
