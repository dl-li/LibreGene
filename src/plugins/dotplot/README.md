# Dotplot — distributable LibreGene plugin

Self-contained dot-plot (self-comparison) viewer for DNA/RNA sequences.
Everything the plugin needs lives in this directory; the only runtime
dependency is React.

## Files

| File | Role |
|---|---|
| `dotplot.js` | Zero-dependency core algorithm: sequence normalization, reverse complement, k-mer dot matching, tick spacing. Usable on its own in any JS project. |
| `DotplotDialog.jsx` | Canvas-based dialog UI (imperative canvas rendering; SVG froze WebKit at this dot density). |
| `ui.jsx` | Minimal dialog/label/notice primitives used by the dialog, so the plugin does not import the host's UI kit. |
| `index.js` | Plugin object for the LibreGene static registry (`src/plugins/index.js`). |
| `manifest.json` | Machine-readable metadata and the host contract. |

## Host contract

- The host renders the dialog component with props
  `{ open, onOpenChange, sequence, fileName }` (all except `open`/`onOpenChange` optional).
- `sequence` is a plain DNA/RNA string; the plugin uppercases it and folds U to T internally.
- Styling assumes Tailwind CSS with shadcn-style theme tokens
  (`bg-card`, `text-muted-foreground`, `border-border`, …). Without them the
  dialog still works but falls back to unstyled colors.

## Installing into a LibreGene host

1. Copy this directory to `src/plugins/dotplot/`.
2. Register it in the host's plugin registry:

```js
import dotplotPlugin from './dotplot';

export const plugins = [
  // ...
  dotplotPlugin,
];
```

3. Open it by setting the host's `pluginDialogs.dotplot` flag to `true`
   (in LibreGene this is wired to the "Examine Dotplot" item in the Diagrams
   menu).

## Using only the algorithm

`dotplot.js` has no imports at all:

```js
import { buildDotplot, normalizeSequence, tickStep } from './dotplot.js';

const seq = normalizeSequence('ACGU...');
const { direct, revcomp, truncated } = buildDotplot(seq, seq, 9);
```
