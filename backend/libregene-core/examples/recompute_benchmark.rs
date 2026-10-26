use std::time::Instant;

use libregene_core::models::{AlignSegment, Alignment, Feature, Primer, ProjectData};
use libregene_core::{align, enzyme, primer};

fn sequence(length: usize) -> String {
    (0..length)
        .map(|i| match (i * 31 + i / 7) % 4 {
            0 => 'A',
            1 => 'C',
            2 => 'G',
            _ => 'T',
        })
        .collect()
}

fn project() -> ProjectData {
    let sequence = sequence(100_000);
    let features = (0..1000)
        .map(|i| Feature {
            id: format!("f{i}"),
            name: format!("Feature {i}"),
            start: (i * 97) as i64,
            end: (i * 97 + 297) as i64,
            color: "#60A5FA".to_string(),
            ftype: if i % 10 == 0 { "CDS" } else { "misc_feature" }.to_string(),
            segments: Vec::new(),
            strand: "+".to_string(),
            notes: String::new(),
            translation: String::new(),
            qualifiers: Vec::new(),
        })
        .collect();
    let primers = (0..200)
        .map(|i| Primer {
            id: format!("p{i}"),
            name: format!("Primer {i}"),
            r#type: if i % 2 == 0 { "fwd" } else { "rev" }.to_string(),
            primer_seq: sequence[i * 431..i * 431 + 20].to_string(),
            binding_sites: Vec::new(),
        })
        .collect();
    let alignments = (0..50)
        .map(|i| Alignment {
            id: format!("a{i}"),
            name: format!("Read {i}"),
            length: 1000,
            strand: "+".to_string(),
            identity: 1.0,
            segments: vec![AlignSegment {
                start: i * 1999,
                end: i * 1999 + 999,
                chars: sequence[i * 1999..i * 1999 + 1000].to_string(),
            }],
            seq: sequence[i * 1999..i * 1999 + 1000].to_string(),
            ..Default::default()
        })
        .collect();
    ProjectData {
        sequence,
        length: 100_000,
        topology: "circular".to_string(),
        molecule_type: "dna".to_string(),
        features,
        primers,
        alignments,
        ..Default::default()
    }
}

fn main() {
    let iterations = 5;
    let started = Instant::now();
    for _ in 0..iterations {
        let mut project = project();
        enzyme::recompute(&mut project);
        primer::recompute(&mut project);
        libregene_core::translate::refresh_feature_translations(&mut project);
        align::realign_project(&mut project);
        std::hint::black_box(project.enzymes.len());
    }
    let elapsed = started.elapsed();
    println!(
        "100 kb, 1000 features, 200 primers, 50 alignments: {iterations} iterations, total {:?}, mean {:?}",
        elapsed,
        elapsed / iterations
    );
}
