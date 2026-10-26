use std::time::Instant;

use libregene_core::models::{AlignSegment, Alignment, Feature, Primer, ProjectData};
use libregene_core::{align, enzyme, primer};

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    length: usize,
    features: usize,
    primers: usize,
    alignments: usize,
    iterations: usize,
}

#[derive(Default)]
struct Samples {
    enzyme: Vec<f64>,
    primer: Vec<f64>,
    translation: Vec<f64>,
    alignment: Vec<f64>,
    total: Vec<f64>,
}

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

fn project(scenario: Scenario) -> ProjectData {
    let sequence = sequence(scenario.length);
    let feature_span = (scenario.length - 301).max(1);
    let features = (0..scenario.features)
        .map(|i| {
            let start = (i * 97) % feature_span;
            Feature {
                id: format!("f{i}"),
                name: format!("Feature {i}"),
                start: start as i64,
                end: (start + 297) as i64,
                color: "#60A5FA".to_string(),
                ftype: if i % 10 == 0 { "CDS" } else { "misc_feature" }.to_string(),
                segments: Vec::new(),
                strand: "+".to_string(),
                notes: String::new(),
                translation: String::new(),
                qualifiers: Vec::new(),
            }
        })
        .collect();
    let primer_span = (scenario.length - 21).max(1);
    let primers = (0..scenario.primers)
        .map(|i| {
            let start = (i * 431) % primer_span;
            Primer {
                id: format!("p{i}"),
                name: format!("Primer {i}"),
                r#type: if i % 2 == 0 { "fwd" } else { "rev" }.to_string(),
                primer_seq: sequence[start..start + 20].to_string(),
                binding_sites: Vec::new(),
            }
        })
        .collect();
    let read_length = 1000.min(scenario.length);
    let alignment_span = (scenario.length - read_length).max(1);
    let alignments = (0..scenario.alignments)
        .map(|i| {
            let start = (i * 1999) % alignment_span;
            let read = sequence[start..start + read_length].to_string();
            Alignment {
                id: format!("a{i}"),
                name: format!("Read {i}"),
                length: read_length,
                strand: "+".to_string(),
                identity: 1.0,
                segments: vec![AlignSegment {
                    start,
                    end: start + read_length - 1,
                    chars: read.clone(),
                }],
                seq: read,
                ..Default::default()
            }
        })
        .collect();
    ProjectData {
        sequence,
        length: scenario.length as i64,
        topology: "circular".to_string(),
        molecule_type: "dna".to_string(),
        features,
        primers,
        alignments,
        ..Default::default()
    }
}

fn measure(scenario: Scenario) -> Samples {
    let mut samples = Samples::default();
    for _ in 0..scenario.iterations {
        let mut project = project(scenario);
        let total_started = Instant::now();

        let started = Instant::now();
        enzyme::recompute(&mut project);
        samples
            .enzyme
            .push(started.elapsed().as_secs_f64() * 1000.0);

        let started = Instant::now();
        primer::recompute(&mut project);
        samples
            .primer
            .push(started.elapsed().as_secs_f64() * 1000.0);

        let started = Instant::now();
        libregene_core::translate::refresh_feature_translations(&mut project);
        samples
            .translation
            .push(started.elapsed().as_secs_f64() * 1000.0);

        let started = Instant::now();
        align::realign_project(&mut project);
        samples
            .alignment
            .push(started.elapsed().as_secs_f64() * 1000.0);

        samples
            .total
            .push(total_started.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(project.enzymes.len());
    }
    samples
}

fn percentile(values: &mut [f64], percentile: usize) -> f64 {
    values.sort_by(f64::total_cmp);
    values[(values.len() - 1) * percentile / 100]
}

fn report(name: &str, values: &mut [f64]) {
    println!(
        "  {name:<12} p50 {:>9.3} ms   p95 {:>9.3} ms",
        percentile(values, 50),
        percentile(values, 95)
    );
}

fn main() {
    let scenarios = [
        Scenario {
            name: "5kb_no_reads",
            length: 5_000,
            features: 20,
            primers: 10,
            alignments: 0,
            iterations: 9,
        },
        Scenario {
            name: "10kb_no_reads",
            length: 10_000,
            features: 100,
            primers: 50,
            alignments: 0,
            iterations: 9,
        },
        Scenario {
            name: "10kb_1_read",
            length: 10_000,
            features: 100,
            primers: 50,
            alignments: 1,
            iterations: 9,
        },
        Scenario {
            name: "10kb_5_reads",
            length: 10_000,
            features: 100,
            primers: 50,
            alignments: 5,
            iterations: 7,
        },
        Scenario {
            name: "10kb_20_reads",
            length: 10_000,
            features: 100,
            primers: 50,
            alignments: 20,
            iterations: 5,
        },
        Scenario {
            name: "50kb_5_reads",
            length: 50_000,
            features: 100,
            primers: 50,
            alignments: 5,
            iterations: 5,
        },
        Scenario {
            name: "100kb_extreme",
            length: 100_000,
            features: 1000,
            primers: 200,
            alignments: 50,
            iterations: 1,
        },
    ];

    let mut warmup = project(scenarios[0]);
    enzyme::recompute(&mut warmup);
    primer::recompute(&mut warmup);

    for scenario in scenarios {
        println!(
            "\n{scenario_name}: {} bp, {} features, {} primers, {} alignments, {} iterations",
            scenario.length,
            scenario.features,
            scenario.primers,
            scenario.alignments,
            scenario.iterations,
            scenario_name = scenario.name
        );
        let mut samples = measure(scenario);
        report("enzyme", &mut samples.enzyme);
        report("primer", &mut samples.primer);
        report("translation", &mut samples.translation);
        report("alignment", &mut samples.alignment);
        report("total", &mut samples.total);
    }
}
