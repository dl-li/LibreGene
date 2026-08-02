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

## MCP / LLM Agent Integration

LibreGene embeds an [MCP](https://modelcontextprotocol.io) server (enabled by default, loopback only) so an LLM agent in your terminal can operate the app like a real user — open files, inspect plasmid structure, edit sequence and features, design primers, run ORF/PCR analysis — while the UI updates live. See *MCP Server* in the sidebar (or the "Connect an LLM agent via MCP" link on the empty screen) for setup guidance and per-client config snippets.

Register it with your agent CLI:

```bash
# Kimi Code CLI
kimi mcp add --transport http libregene http://127.0.0.1:8766/mcp

# Claude Code
claude mcp add --transport http libregene http://127.0.0.1:8766/mcp
```

Or via a standard `mcpServers` config file:

```json
{
  "mcpServers": {
    "libregene": {
      "type": "http",
      "url": "http://127.0.0.1:8766/mcp"
    }
  }
}
```

The server runs inside the Tauri process, so launch LibreGene first, then ask your agent to e.g. "open `examples/pUC19 Annotated.gbk` and summarize it".

## License

GNU General Public License v3.0 — see [LICENSE](LICENSE).

Copyright (C) 2025 dl-li
