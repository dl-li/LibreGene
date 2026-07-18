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
