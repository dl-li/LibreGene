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

> **Windows build** — see [`BUILD-WINDOWS.md`](BUILD-WINDOWS.md) for the toolchain setup and `npx tauri build` instructions.

<p align="center">
  <img src="ScreenShot.png" alt="LibreGene Screenshot" width="700" />
</p>

## Features

- **SVG plasmid map** with multi-line wrapped sequence display
- **Feature annotation** — CDS, promoters, terminators, compound features, custom colors, strand toggling
- **Primer visualization** — Add, align, Tm calculation (nearest-neighbor), binding site display
- **Restriction enzymes** — Built-in 900+ enzyme DB, methylation-aware filtering, single/unique cutter views, methylation-sensitive/dependent pattern detection
- **Sequence alignment** — Import and visualize multi-read alignments (.ab1, FASTA) alongside the reference sequence
- **ORF search** — Scan and display open reading frames on both strands
- **Plugin system** — Extensible architecture for adding custom tools (alignment viewer and ORF search shipped as built-in plugins)
- **Multi-project tabs** — Switch between plasmids in the sidebar
- **Full undo/redo** — Sequence edits and feature changes
- **GenBank / SnapGene I/O** — Read/write .gb/.gbk, read .dna (SnapGene), with enriched color & primer annotations


## Quick Start

```bash
npm install
npx tauri dev
```

Requires Node.js ≥ 20, Rust ≥ 1.75, and [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/).

## License

GNU General Public License v3.0 — see [LICENSE](LICENSE).

Copyright (C) 2025 dl-li
