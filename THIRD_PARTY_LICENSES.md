# Third-Party Licenses

LibreGene bundles or depends on the following third-party works.

## Fonts

### Cascadia Code
- **Source**: Microsoft (https://github.com/microsoft/cascadia-code)
- **License**: SIL Open Font License 1.1
- **Files**: `assets/Fonts/CascadiaCode.woff2`, `assets/Fonts/CascadiaCodeItalic.woff2`

SIL Open Font License 1.1 — https://openfontlicense.org

### TeX Gyre Heros / TeX Gyre Termes
- **Source**: GUST (http://www.gust.org.pl/projects/e-foundry/tex-gyre)
- **License**: GUST Font License (LPPL 1.3c)
- **Files**: `assets/Fonts/texgyreheros-*.otf`, `assets/Fonts/texgyretermes-*.otf`

The GUST Font License is based on the LaTeX Project Public License (LPPL) version 1.3c.
See `assets/Fonts/TEXGYRE-LICENSE.txt` for the full license text.

---

## JavaScript / npm Dependencies

This project uses npm packages, each under its own license. Key runtime dependencies include:

| Package | License |
|---------|---------|
| React 19 | MIT |
| React DOM 19 | MIT |
| Radix UI (various) | MIT |
| lucide-react | ISC |
| Tailwind CSS v4 | MIT |
| class-variance-authority | Apache-2.0 |
| clsx | MIT |
| tailwind-merge | MIT |
| tw-animate-css | MIT |
| shadcn | MIT |
| Tauri API v2 | Apache-2.0 / MIT |

See `package.json` and `package-lock.json` for the full list and their respective licenses.

### RibossFold (ribossfold-wasm)
- **Source**: The Riboseek Development Team (https://github.com/mirditalab/RibossFold)
- **License**: MIT
- **Usage**: RNA minimum-free-energy secondary structure prediction (Turner 2004 model), compiled to WebAssembly; loaded lazily by the RNA Folding plugin (`src/plugins/rnaFold/`)

### fornac
- **Source**: ViennaRNA forna by Peter Kerpedjiev et al. (https://github.com/ViennaRNA/forna)
- **License**: Apache-2.0
- **Usage**: Force-directed RNA secondary structure visualization in the RNA Folding plugin; bundles d3 v3 (BSD-3-Clause)

---

## Algorithms & Data Adapted from Other Projects

### DNA Chisel
- **Source**: Edinburgh Genome Foundry (https://github.com/Edinburgh-Genome-Foundry/DnaChisel)
- **License**: MIT
- **Usage**: Codon optimization algorithms (MaximizeCAI / MatchCodonUsage / HarmonizeRca) ported to Rust in `backend/libregene-core/src/codon.rs`; codon-usage tables from Kazusa's `python_codon_tables` (CC0, https://pypi.org/project/python-codon-tables/)

### Biopython
- **Source**: Biopython Project (https://biopython.org)
- **License**: Biopython License Agreement (MIT-style)
- **Usage**: Restriction enzyme database exported from `Bio.Restriction` via `backend/scripts/export_enzymes.py` (data ultimately from REBASE); Tm and salt-correction formulas in `backend/libregene-core/src/primer/thermodynamics.rs` follow Biopython's `MeltingTemp` / `salt_correction` methods

### pydna
- **Source**: pydna group (https://github.com/pydna-group/pydna)
- **License**: BSD-3-Clause
- **Usage**: Primer annealing / binding-site search algorithm (3' anchor + greedy 5' extension) reimplemented in `backend/libregene-core/src/primer/matcher.rs` and `primer/screening.rs`

## Rust / Cargo Dependencies

This project uses Rust crates, each under its own license. Key dependencies include:

| Crate | License |
|-------|---------|
| tauri v2 | Apache-2.0 / MIT |
| tauri-plugin-dialog | Apache-2.0 / MIT |
| tauri-plugin-opener | Apache-2.0 / MIT |
| serde / serde_json | Apache-2.0 / MIT |
| tokio | MIT |
| gb-io | MIT |
| regex | Apache-2.0 / MIT |
| bio | MIT |
| rayon | Apache-2.0 / MIT |
| quick-xml | MIT |
| csv | MIT |
| chrono | Apache-2.0 / MIT |
| thiserror | Apache-2.0 / MIT |
| itertools | MIT |
| aho-corasick | MIT |

See `backend/libregene-core/Cargo.toml` and `src-tauri/Cargo.toml` for the full list.

---

## Auto-Annotation Data

### pLannotate / SnapGene (GenoLIB) feature database
- **Source**: pLannotate by Matthew J. McGuffie & Jeffrey E. Barrick (The University of Texas at Austin), https://github.com/mmcguffi/pLannotate
- **License**: GPL-3.0-only (code); database files distributed under the pLannotate project
- **Files**: `backend/libregene-core/data/features.fasta`, `features.tsv`, `feature_colors.tsv`, `feature_orientation.txt`
- **Paper**: McGuffie & Barrick, "pLannotate: engineered plasmid annotation", Nucleic Acids Research 2021, doi:10.1093/nar/gkab374
- **Data provenance**: The feature sequences trace to the SnapGene feature database (originating from the GenoLIB biological part database, cross-referenced/deduplicated against Addgene GenBank records) plus additional curated elements; the FPbase/Rfam/Swiss-Prot parts of pLannotate are not bundled.
- The matching engine in `backend/libregene-core/src/annotate.rs` is an independent Rust reimplementation of pLannotate's algorithm (scoring, filtering, overlap elimination, circular wrap), not a copy of its code.
