pub mod ab1;
pub mod color;
pub mod dna;
pub mod fasta;
pub mod gbk;
pub mod gpt;

use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use crate::models::ProjectData;

/// Parse a file, dispatching on extension.
pub fn parse_file(path: &Path) -> io::Result<ProjectData> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "gbk" | "gb" | "genbank" | "gbf" | "gbff" => gbk::parse_gbk(path),
        "dna" | "rna" | "prot" => dna::parse_snapgene(path),
        // gpt plus the NCBI GenPept variants — same hand-rolled protein GenBank
        // parser (gb-io can't handle the amino-acid alphabet).
        "gpt" | "gp" | "gpe" | "gpff" => gpt::parse_gpt(path),
        "fasta" | "fa" | "fna" | "fas" | "ffn" | "fsa" | "frn" => fasta::parse_fasta(path),
        // Protein FASTA — the extension is the signal, no alphabet sniffing.
        "faa" => fasta::parse_fasta_with_molecule_type(path, "protein"),
        "ab1" => ab1::parse_ab1(path),
        // .seq carries no format in the extension — sniff the content.
        "seq" => parse_seq(path),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported file extension: .{}", other),
        )),
    }
}

/// `.seq` files come in several flavors — look at the first non-empty line:
/// `LOCUS` → GenBank, `>` → FASTA.
fn parse_seq(path: &Path) -> io::Result<ProjectData> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        return if trimmed.starts_with('>') {
            fasta::parse_fasta(path)
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
}
