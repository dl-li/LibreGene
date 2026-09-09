<p align="center">
  <img src="src-tauri/icons/icon.svg" alt="LibreGene" width="128" />
</p>

<h1 align="center">LibreGene — Plasmid Editor</h1>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-GPLv3-blue.svg" alt="License: GPL v3" /></a>
</p>

<p align="center"><a href="README.zh-CN.md">中文版</a></p>

> **⚠️ Early Development** — APIs and file formats are not yet stable.
> **Tested on macOS and Windows** — Linux builds are not yet verified.

A lightweight cross-platform desktop plasmid editor. SVG rendering, feature annotation, primer design & visualization, restriction digestion analysis — all free and open-source.

<p align="center">
  <img src="screenshots/feature-translation.png" alt="Feature translation in LibreGene" width="700" />
</p>

## Features

### Feature Annotation

- **Clean, minimal display** — CDS and mRNA features are translated on the fly, amino acids rendered right under the sequence
- **Auto-detection** — paste a sequence into the *New Sequence* dialog and common features (promoters, resistance markers, ori, tags…) are detected via the [pLannotate](https://github.com/mmcguffi/pLannotate) feature database and can be annotated with one click
- **ORF search** — scan open reading frames on both strands, all six frames, and display them inline
- Compound (multi-segment) features, custom colors, strand toggling, enriched GenBank I/O that preserves colors and primers

<p align="center">
  <img src="screenshots/orf.png" alt="ORF search results" width="49%" />
  <img src="screenshots/new-project-dialog.png" alt="New project dialog with auto-detected features" width="49%" />
</p>

### Primers

- **Binding visualization** — each primer is aligned against the template with mismatches, annealing region and Tm (nearest-neighbor) shown at a glance; binding-site search follows [pydna](https://github.com/pydna-group/pydna)'s annealing algorithm
- **Assisted design** — fragment amplification, overlap-extension PCR (OE-PCR) and site-directed mutagenesis, with candidate primers ranked by Tm / GC% and optional restriction-site tails

<p align="center">
  <img src="screenshots/primer-dialog.png" alt="Primer binding details" width="49%" />
  <img src="screenshots/primer-design.png" alt="Primer design dialog" width="49%" />
</p>

### Codon Optimization

- **9 species codon usage tables** — optimize any CDS/mRNA feature for the expression host of your choice ([Kazusa](https://www.kazusa.or.jp/codon/) usage data)
- **Three strategies** — use best codon, match codon usage (keeps natural synonymous diversity), or harmonize relative codon adaptation against the source species; algorithms ported from [DNA Chisel](https://github.com/Edinburgh-Genome-Foundry/DnaChisel)
- **Preview before applying** — translation check, CAI and GC% before/after at a glance; equal-length synonymous substitution leaves feature coordinates untouched; optionally avoid creating specified restriction sites

<p align="center">
  <img src="screenshots/codon-optimize.png" alt="Codon optimization dialog" width="700" />
</p>

### Restriction Enzymes

- **Built-in 900+ enzyme database** (exported from [Biopython](https://biopython.org)'s `Bio.Restriction`) with methylation-aware filtering
- **Clear categorization** — unique cutters, twice cutters, blunt ends and Type IIS enzymes are distinguished visually, and cut sites are marked on the feature scrollbar for quick navigation
- **Custom enzyme sets** — define your own enzyme collections and switch between them

<p align="center">
  <img src="screenshots/enzymes.png" alt="Restriction enzyme view" width="700" />
</p>

### Sequence Alignment

- **Multiple formats** — import Sanger reads (.ab1), FASTA or GenBank sequences and align them against the reference
- Mismatches, insertions and deletions are highlighted in place, per-read identity is computed automatically

<p align="center">
  <img src="screenshots/alignment.png" alt="Sequence alignment view" width="700" />
</p>

### RNA Secondary Structure

- **MFE folding** — minimum-free-energy prediction with the Turner 2004 nearest-neighbor model, powered by [RibossFold](https://github.com/mirditalab/RibossFold) running as WebAssembly
- **Interactive layout** — classic force-directed structure view from [forna](https://github.com/ViennaRNA/forna) (ViennaRNA), with pan/zoom and draggable nucleotides

<p align="center">
  <img src="screenshots/rna-fold.png" alt="RNA secondary structure prediction" width="700" />
</p>

### MCP / LLM Agent Integration

LibreGene embeds an [MCP](https://modelcontextprotocol.io) server (loopback only at `127.0.0.1:8766`, Bearer-token auth) so an LLM agent in your terminal can operate the open plasmid like a real user — while the UI updates live. Launch LibreGene, open the *MCP Server* dialog in the sidebar (or the "Connect an LLM agent via MCP" link on the empty screen), and copy the auto-generated setup prompt — with the URL and access token embedded — to your agent; it configures the connection itself.

18 tools are exposed, covering the full editing workflow:

- **Projects & files** — `open_project`, `save_file` (with subsequence/region export), `close_project`, `list_projects`
- **Reading** — `read_sequence`, `get_project_overview`, `get_region_view`, `search_sequence` (IUPAC fuzzy search, peptide queries expanded to degenerate codons)
- **Editing** — `edit_sequence` (insert/delete/replace), `set_feature` (create & update)
- **Primers** — `add_primer`, `list_primers`, `check_primer_binding` (binding sites + Tm), `design_primers` (amplify / OE-PCR / mutagenesis)
- **Analysis** — `find_restriction_sites`, `find_orfs`, `add_alignment`, `optimize_cds` (codon optimization for 9 species)

#### Example Tasks

Ready-to-run agent tasks live in [`examples/tasks`](examples/tasks), each with a `Prompt.txt` you can hand to your agent and a reference result:

- **[Primer Design](examples/tasks/Primer%20Design)** — clone mEGFP into a BamHI/HindIII-digested BlueScribe vector: design cloning primers with restriction tails, pick colony-PCR verification primers, then design A206K mutagenesis primers
- **[Alignment](examples/tasks/Alignment)** — given pVA-MCS and three Sanger `.ab1` reads, determine which samples successfully mutated away the BbsI site
- **[RNAi](examples/tasks/RNAi)** — given a vector and an experimental protocol, design an RNAi plasmid targeting a gene of interest with a given antisense sequence
- **[CAR-T](examples/tasks/CAR-T)** — given the amino-acid sequence of an anti-CD19 CAR (FMC63 scFv–CD28–CD3ζ), design a pLVX-CAR-IRES-ZsGreen1 expression vector

### And More

- **SVG plasmid map** with multi-line wrapped sequence display
- **DNA / RNA / protein modes** — single-strand editors for RNA and protein sequences
- **Plugin system** — extensible architecture for adding custom tools
- **Multi-project tabs** — switch between plasmids in the sidebar
- **Full undo/redo** — sequence edits and feature changes
- **GenBank / SnapGene / FASTA I/O** — read/write .gb/.gbk and protein .gpt; read .dna/.rna/.prot ([SnapGene](https://www.snapgene.com)), FASTA and Sanger .ab1

## Installation

**macOS (Homebrew, Apple Silicon):**

```bash
brew install --cask dl-li/libregene/libregene
```

The app is not notarized yet, so on first launch use right-click → **Open** (or approve it in System Settings → Privacy & Security).

Alternatively, download installers for macOS (dmg), Windows (msi / nsis) and Linux (Flatpak) directly from [GitHub Releases](https://github.com/dl-li/LibreGene/releases).

## Quick Start

```bash
npm install
npx tauri dev
```

Requires Node.js ≥ 20, Rust ≥ 1.75, and [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/).

## License

GNU General Public License v3.0 — see [LICENSE](LICENSE).

Copyright (C) 2025 dl-li
