<p align="center">
  <img src="src-tauri/icons/icon.svg" alt="LibreGene" width="128" />
</p>

<h1 align="center">LibreGene — Plasmid Editor</h1>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-GPLv3-blue.svg" alt="License: GPL v3" /></a>
</p>

> **⚠️ Early Development** — APIs and file formats are not yet stable. Expect breaking changes.
>
> [中文版 README](README.zh-CN.md)

**LibreGene** is a lightweight, cross-platform desktop plasmid editor built with React + Vite + Tauri v2 + Rust. It renders plasmid maps entirely in SVG and is free, open-source software for everyday molecular cloning work.

## Features

- **SVG rendering** — Smooth, scalable plasmid maps with multi-line wrapped sequence display
- **Features** — Annotate CDS, promoters, terminators, and more. Supports compound (joined) features with custom colors and strand toggling.
- **Primers** — Add, align, and visualize primers with binding sites. Tm calculation via nearest-neighbor thermodynamics.
- **Restriction enzymes** — Built-in enzyme database (900+ enzymes), methylation-aware filtering, single/unique cutter views.
- **Multi-window** — Open multiple plasmids in separate OS windows simultaneously.
- **Multi-project tabs** — Switch between plasmids in the sidebar.
- **Undo/Redo** — Full history for sequence edits and feature changes.
- **GenBank I/O** — Parse and write GenBank files (.gb/.gbk), including enriched formats with color and primer annotations.
- **Methylation analysis** — Detect methylation-sensitive and methylation-dependent restriction patterns.

### Advantages

- **Lightweight** (~15 MB binary) — Much smaller than comparable commercial tools
- **Free & open-source** — No licenses, no subscriptions
- **Fast** — Pure Rust backend, SVG rendering via React, no heavy native widgets
- **Cross-platform** — macOS, Windows, Linux (via Tauri v2)

### Limitations

- **Early stage** — Many features are still placeholder/prototype (PCR analysis, primer design, enzyme database management)
- **Single file focus** — No multi-record GenBank or database management
- **No sequence alignment** — BLAST / pairwise alignment not yet implemented

## Quick Start

### Prerequisites

- Node.js ≥ 20
- Rust ≥ 1.75
- Tauri v2 system dependencies: [tauri.app/start/prerequisites](https://v2.tauri.app/start/prerequisites/)

### Development

```bash
# Install frontend dependencies
npm install

# Launch desktop app
npx tauri dev
```

### Commands

| Command | Description |
|---------|-------------|
| `npm run dev` | Vite dev server (not for standalone use) |
| `npm run build` | Frontend-only build check |
| `npx tauri dev` | **Launch desktop app** |
| `npx tauri build` | Build production binary |
| `npm run format` | Prettier formatting |
| `npm run lint` | ESLint check |

#### Rust backend

```bash
cd backend
cargo test -p libregene-core --lib              # Unit tests (117)
cargo test -p libregene-core --test roundtrip_test   # Round-trip I/O test
cargo build -p libregene                           # Build Tauri backend
```

## Project Structure

```
LibreGene/
├── src/                  # Frontend React source
│   ├── App.jsx           # Top-level state + sidebar + routing
│   ├── SequenceEditor.jsx# Core SVG editor (~2474 LOC, to be split)
│   ├── editorConstants.js# Layout constants (cw, startX, baseSeqY)
│   ├── editHistory.js    # Undo/redo stack
│   └── components/       # shadcn-based UI components
├── backend/              # Rust workspace
│   ├── libregene-core/   # Core library: models, enzyme, primer, file_io
│   └── test_data/        # Integration test data (JSON exports)
├── src-tauri/            # Tauri v2 desktop shell
│   └── src/              # Tauri commands + AppState
├── assets/Fonts/         # Font files (Cascadia Code, TeX Gyre)
├── public/assets/Fonts/  # Font copy for Vite dev server
└── test/                 # Test .dna / .gbk files
```

## Roadmap

`EditorNavMenu.jsx` has placeholder menu items (disabled, labeled "coming soon"):

- **Features**: Always Expand Feature
- **Primers**: My Primers, PCR Analysis, Primer Design, Options
- **Enzymes**: Custom Enzyme Sets, Enzyme Database, Restriction Analysis

See [AGENTS.md](AGENTS.md) for internal development notes.

## License

Copyright (C) 2025 dl-li

This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.

You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.

### Third-Party Licenses

See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) for licenses of bundled fonts and dependencies.
