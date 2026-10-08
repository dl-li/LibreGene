pub mod ab1;
pub mod color;
pub mod dna;
pub mod fasta;
pub mod gbk;
pub mod gpt;
pub mod snapgene_history;

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

use crate::models::ProjectData;

/// Upper bound for any single input file read through the parsers (sequence
/// documents, Sanger traces, SnapGene history). Real inputs top out around a
/// bacterial chromosome (~15 MB); anything larger is corrupt or crafted, and
/// reading it whole would just balloon memory.
pub const MAX_INPUT_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// Fail fast when the file is larger than [`MAX_INPUT_FILE_BYTES`], before
/// any reader pulls it into memory. Callers re-read from disk later (e.g. the
/// history views), so they re-check at their own read site.
pub fn ensure_within_size_limit(path: &Path) -> io::Result<()> {
    let len = fs::metadata(path)?.len();
    if len > MAX_INPUT_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is {} bytes — over the {} byte input cap",
                path.display(),
                len,
                MAX_INPUT_FILE_BYTES
            ),
        ));
    }
    Ok(())
}

/// Parse a file, dispatching on extension.
pub fn parse_file(path: &Path) -> io::Result<ProjectData> {
    parse_file_with_molecule_type(path, None)
}

/// Like [`parse_file`], but `molecule_type` ("dna" | "rna" | "protein") forces
/// the type of text formats whose extension alone is ambiguous: a `.prot`
/// carrying plain text, and a `.fa/.fasta` explicitly requested as protein.
/// Binary SnapGene documents still take their type from the file header.
pub fn parse_file_with_molecule_type(
    path: &Path,
    molecule_type: Option<&str>,
) -> io::Result<ProjectData> {
    ensure_within_size_limit(path)?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "gbk" | "gb" | "genbank" | "gbf" | "gbff" => gbk::parse_gbk(path),
        // .dna/.rna/.prot are normally SnapGene binary, but text exports (raw
        // sequence or FASTA, e.g. a plain protein .prot) are common enough to
        // accept — fall back to text parsing when the SnapGene cookie is absent.
        "dna" | "rna" | "prot" => {
            if is_snapgene_document(path) {
                dna::parse_snapgene(path)
            } else {
                let mt = molecule_type.unwrap_or(match ext.as_str() {
                    "rna" => "rna",
                    "prot" => "protein",
                    _ => "dna",
                });
                parse_text_sequence(path, mt)
            }
        }
        // gpt plus the NCBI GenPept variants — same hand-rolled protein GenBank
        // parser (gb-io can't handle the amino-acid alphabet).
        "gpt" | "gp" | "gpe" | "gpff" => gpt::parse_gpt(path),
        "fasta" | "fa" | "fna" | "fas" | "ffn" | "fsa" | "frn" => {
            fasta::parse_fasta_with_molecule_type(path, molecule_type.unwrap_or("dna"))
        }
        // Protein FASTA — the extension is the signal, no alphabet sniffing.
        "faa" => fasta::parse_fasta_with_molecule_type(path, "protein"),
        "ab1" => ab1::parse_ab1(path),
        // .seq carries no format in the extension — sniff the content.
        "seq" => parse_seq(path, molecule_type),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported file extension: .{}", other),
        )),
    }
    .map(normalize_rna_thymine)
}

/// True when the file starts with the SnapGene cookie
/// (`0x09` | BE u32 14 | "SnapGene") — the same check `parse_snapgene` makes,
/// done up front so text `.dna/.rna/.prot` files can take the text path.
fn is_snapgene_document(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut cookie = [0u8; 13];
    if file.read_exact(&mut cookie).is_err() {
        return false;
    }
    cookie[0] == 0x09 && &cookie[5..13] == b"SnapGene"
}

/// Parse a plain-text sequence file: FASTA when the first non-empty line is a
/// `>` header, otherwise the raw sequence letters (whitespace stripped).
fn parse_text_sequence(path: &Path, molecule_type: &str) -> io::Result<ProjectData> {
    let text = fs::read_to_string(path)?;
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if first.trim_start().starts_with('>') {
        return fasta::parse_fasta_with_molecule_type(path, molecule_type);
    }

    let sequence: String = text
        .chars()
        .filter(|c| c.is_ascii_alphabetic() || *c == '*')
        .flat_map(|c| c.to_uppercase())
        .collect();
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    Ok(ProjectData {
        name,
        topology: "linear".to_string(),
        molecule_type: molecule_type.to_string(),
        length: sequence.len() as i64,
        sequence,
        ..Default::default()
    })
}

/// RNA files in the wild often carry DNA-alphabet sequences (T instead of U);
/// normalize on open so the rest of the app sees a consistent RNA alphabet.
fn normalize_rna_thymine(mut project: ProjectData) -> ProjectData {
    if project.molecule_type == "rna" && project.sequence.contains(['T', 't']) {
        project.sequence = project
            .sequence
            .chars()
            .map(|c| match c {
                'T' => 'U',
                't' => 'u',
                c => c,
            })
            .collect();
    }
    project
}

/// `.seq` files come in several flavors — look at the first non-empty line:
/// `LOCUS` → GenBank, `>` → FASTA.
fn parse_seq(path: &Path, molecule_type: Option<&str>) -> io::Result<ProjectData> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        return if trimmed.starts_with('>') {
            fasta::parse_fasta_with_molecule_type(path, molecule_type.unwrap_or("dna"))
        } else if trimmed.starts_with("LOCUS") || trimmed.starts_with("locus") {
            gbk::parse_gbk(path)
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "cannot infer format from .seq file {}: expected GenBank (LOCUS) or FASTA (>)",
                    path.display()
                ),
            ))
        };
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("empty .seq file: {}", path.display()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("test_data")
    }

    fn write_temp(ext: &str, content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "libregene_parse_file_test_{}_{}.{}",
            std::process::id(),
            ext,
            ext
        ));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn gbf_gbff_map_to_genbank_parser() {
        let src = std::fs::read_to_string(test_data_dir().join("Primary-miR-1.gbk")).unwrap();
        let baseline = parse_file(&test_data_dir().join("Primary-miR-1.gbk")).unwrap();
        for ext in ["gbf", "gbff"] {
            let path = write_temp(ext, &src);
            let parsed = parse_file(&path).unwrap();
            assert_eq!(parsed.molecule_type, baseline.molecule_type, ".{}", ext);
            assert_eq!(parsed.sequence, baseline.sequence, ".{}", ext);
            assert_eq!(parsed.name, baseline.name, ".{}", ext);
            std::fs::remove_file(&path).ok();
        }
    }

    #[test]
    fn fasta_variants_parse_as_dna() {
        for ext in ["fas", "ffn", "fsa", "frn"] {
            let path = write_temp(ext, ">seq\nACGTACGT\n");
            let parsed = parse_file(&path).unwrap();
            assert_eq!(parsed.molecule_type, "dna", ".{}", ext);
            assert_eq!(parsed.sequence, "ACGTACGT", ".{}", ext);
            assert_eq!(parsed.topology, "linear", ".{}", ext);
            std::fs::remove_file(&path).ok();
        }
    }

    #[test]
    fn faa_parses_as_protein() {
        let path = write_temp("faa", ">insulin_b\nMVSHHFVGAG\n*");
        let parsed = parse_file(&path).unwrap();
        assert_eq!(parsed.molecule_type, "protein");
        assert_eq!(parsed.sequence, "MVSHHFVGAG*");
        assert_eq!(parsed.length, 11);
        std::fs::remove_file(&path).ok();
    }

    fn write_named(name: &str, content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "libregene_parse_file_named_{}_{}",
            std::process::id(),
            name
        ));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn text_prot_parses_as_protein() {
        // FASTA content in a .prot file.
        let fasta = write_named("car.prot", ">car\nMALPVTALLLP*\n");
        let parsed = parse_file(&fasta).unwrap();
        assert_eq!(parsed.molecule_type, "protein");
        assert_eq!(parsed.sequence, "MALPVTALLLP*");
        assert_eq!(parsed.name, "car");
        std::fs::remove_file(&fasta).ok();

        // Raw protein text (no FASTA header).
        let raw = write_named("raw.prot", "MKV\nGLA*\n");
        let parsed = parse_file(&raw).unwrap();
        assert_eq!(parsed.molecule_type, "protein");
        assert_eq!(parsed.sequence, "MKVGLA*");
        assert_eq!(parsed.length, 7);
        std::fs::remove_file(&raw).ok();
    }

    #[test]
    fn fasta_with_protein_hint_parses_as_protein() {
        let path = write_named("hint.fasta", ">p\nMVSHHFVGAG*\n");
        let dna = parse_file(&path).unwrap();
        assert_eq!(dna.molecule_type, "dna", "no hint → nucleotide");
        let prot = parse_file_with_molecule_type(&path, Some("protein")).unwrap();
        assert_eq!(prot.molecule_type, "protein");
        assert_eq!(prot.sequence, "MVSHHFVGAG*");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn binary_snapgene_prot_still_parses() {
        let path = test_data_dir().join("mCherry.prot");
        if path.exists() {
            let parsed = parse_file(&path).unwrap();
            assert_eq!(parsed.molecule_type, "protein");
            assert_eq!(parsed.length, 237);
        }
    }

    #[test]
    fn gp_variants_map_to_genpept_parser() {
        let src = std::fs::read_to_string(test_data_dir().join("mCherry-gp.gp")).unwrap();
        for ext in ["gp", "gpe", "gpff"] {
            let path = write_temp(ext, &src);
            let parsed = parse_file(&path).unwrap();
            assert_eq!(parsed.molecule_type, "protein", ".{}", ext);
            assert_eq!(parsed.length, 237, ".{}", ext);
            assert!(parsed.sequence.ends_with('*'), ".{}", ext);
            std::fs::remove_file(&path).ok();
        }
    }

    #[test]
    fn seq_content_is_sniffed() {
        // GenBank content (LOCUS on the first line) → GenBank parser.
        let gbk_src = std::fs::read_to_string(test_data_dir().join("Primary-miR-1.gbk")).unwrap();
        let gbk_path = write_temp("seq", &gbk_src);
        let parsed = parse_file(&gbk_path).unwrap();
        assert_eq!(
            parsed.sequence,
            parse_file(&test_data_dir().join("Primary-miR-1.gbk"))
                .unwrap()
                .sequence
        );
        std::fs::remove_file(&gbk_path).ok();

        // FASTA content (">" on the first line) → FASTA parser.
        let fasta_path = write_temp("seq", ">my_seq\nTTGCAACG\n");
        let parsed = parse_file(&fasta_path).unwrap();
        assert_eq!(parsed.sequence, "TTGCAACG");
        assert_eq!(parsed.topology, "linear");
        std::fs::remove_file(&fasta_path).ok();

        // Ambiguous content → error.
        let bad_path = write_temp("seq", "plain text with no format marker\n");
        assert!(parse_file(&bad_path).is_err());
        std::fs::remove_file(&bad_path).ok();
    }

    #[test]
    fn rna_thymine_is_normalized_to_uracil() {
        let gbk_path = write_temp(
            "gbk",
            "LOCUS       testrna                 12 bp ss-RNA     linear   SYN 01-JAN-2000\n\
             ORIGIN\n        1 acgtacgtacgt\n//\n",
        );
        let parsed = parse_file(&gbk_path).unwrap();
        assert_eq!(parsed.molecule_type, "rna");
        assert_eq!(parsed.sequence, "acguacguacgu");
        std::fs::remove_file(&gbk_path).ok();

        // DNA files keep their T.
        let dna_path = write_temp(
            "gbk",
            "LOCUS       testdna                 12 bp DNA     linear   SYN 01-JAN-2000\n\
             ORIGIN\n        1 acgtacgtacgt\n//\n",
        );
        let parsed = parse_file(&dna_path).unwrap();
        assert_eq!(parsed.molecule_type, "dna");
        assert_eq!(parsed.sequence, "acgtacgtacgt");
        std::fs::remove_file(&dna_path).ok();
    }

    #[test]
    fn rejects_files_over_the_size_cap() {
        // A sparse file: set_len reserves the length without writing data, so
        // the test is instant even for a 256 MiB "file".
        let path = std::env::temp_dir().join(format!(
            "libregene_oversize_{}.gbk",
            std::process::id()
        ));
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(MAX_INPUT_FILE_BYTES + 1).unwrap();
        drop(f);
        let err = parse_file(&path).unwrap_err();
        assert!(
            err.to_string().contains("cap"),
            "expected a size-cap error, got: {err}"
        );
        std::fs::remove_file(&path).ok();
    }
}
