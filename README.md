<p align="center">
  <img src="src-tauri/icons/icon.svg" alt="LibreGene" width="128" />
</p>

<h1 align="center">LibreGene — Plasmid Editor</h1>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-GPLv3-blue.svg" alt="License: GPL v3" /></a>
</p>

> **⚠️ Early Development** — APIs and file formats are not yet stable.
> **Tested on macOS only** — Linux/Windows builds are not yet verified.
>
> [中文版](README.zh-CN.md)

A lightweight cross-platform desktop plasmid editor. SVG rendering, feature annotation, primer design & visualization, restriction digestion analysis — all free and open-source.

<p align="center">
  <img src="ScreenShot.png" alt="LibreGene Screenshot" width="700" />
</p>

## Features

- **SVG plasmid map** with multi-line wrapped sequence display
- **Feature annotation** — CDS, promoters, terminators, compound features, custom colors, strand toggling
- **Primer visualization** — Add, align, Tm calculation (nearest-neighbor), binding site display
- **Restriction enzymes** — Built-in 900+ enzyme DB, methylation-aware filtering, single/unique cutter views, methylation-sensitive/dependent pattern detection
- **Sequence alignment** — Import and visualize multi-read alignments (.ab1, FASTA) alongside the reference sequence
- **Plugin system** — Extensible architecture for adding custom tools (alignment viewer shipped as built-in plugin)
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
