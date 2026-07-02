use std::path::Path;

#[test]
fn test_swarm_hr_r_binding() {
    let project = geneie_core::file_io::parse_file(
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../test/flySWARM.dna"))
    ).expect("Failed to parse flySWARM.dna");

    eprintln!("Sequence length: {}", project.sequence.len());
    eprintln!("Topology: {}", project.topology);
    eprintln!("Number of primers: {}", project.primers.len());

    let hr_r = project.primers.iter()
        .find(|p| p.name == "swarm-HR-R")
        .expect("swarm-HR-R not found");

    eprintln!("swarm-HR-R: seq={} len={}", hr_r.primer_seq, hr_r.primer_seq.len());

    // RC of primer — what template should contain
    let rc: String = hr_r.primer_seq.chars()
        .rev()
        .map(|c| match c {
            'A' => 'T', 'T' => 'A', 'G' => 'C', 'C' => 'G',
            _ => c,
        })
        .collect();
    eprintln!("RC(primer)={}", rc);

    // Search for RC in template
    if let Some(pos) = project.sequence.find(&rc) {
        eprintln!("RC found at template pos {} (1-based: {}-{})", pos, pos+1, pos+rc.len());
    } else {
        eprintln!("RC of primer NOT found in template (case-sensitive)");
        // Try case-insensitive
        let template_upper = project.sequence.to_ascii_uppercase();
        if let Some(pos) = template_upper.find(&rc) {
            eprintln!("RC found in UPPERCASED template at pos {}", pos);
        } else {
            eprintln!("RC STILL NOT FOUND even in uppercased template");
        }
    }

    // Now compute binding sites via the matcher
    let sites = geneie_core::primer::align::compute_binding_sites(
        &project.sequence, &hr_r.primer_seq, "rev", "swarm-HR-R",
        &project.topology, 0.0,
    );

    eprintln!("Binding sites via compute_binding_sites: {}", sites.len());
    for s in &sites {
        eprintln!("  strand={} tm={:.1} t_start={} t_end={} fp_len={}",
            s.strand, s.tm, s.template_start, s.template_end, s.alignment.display_sequence.len());
    }

    // Try with 3' anchor (DEFAULT_LIMIT=13) — check matcher directly
    let tpl_bytes = project.sequence.as_bytes();
    let primer_bytes = hr_r.primer_seq.as_bytes();

    let anchor_sites = geneie_core::primer::matcher::find_annealing_positions(
        primer_bytes, tpl_bytes, 13, true, // use_complement = true for rev
    );
    eprintln!("\nDirect matcher results (use_complement=true, limit=13): {}", anchor_sites.len());
    for s in &anchor_sites {
        eprintln!("  template_start={} footprint_len={} has_ambiguous={}",
            s.template_start, s.footprint_len, s.has_ambiguous);
    }

    // Also try with use_complement=false
    let anchor_sites_fwd = geneie_core::primer::matcher::find_annealing_positions(
        primer_bytes, tpl_bytes, 13, false,
    );
    eprintln!("\nDirect matcher results (use_complement=false, limit=13): {}", anchor_sites_fwd.len());
    for s in &anchor_sites_fwd {
        eprintln!("  template_start={} footprint_len={} has_ambiguous={}",
            s.template_start, s.footprint_len, s.has_ambiguous);
    }

    // The first assertion: the primer should find at least one binding site
    assert!(!sites.is_empty(), "swarm-HR-R should find at least one binding site!");
}
