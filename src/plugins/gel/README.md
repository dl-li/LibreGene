# Digest and Run Gel — distributable LibreGene plugin

Self-contained in-silico restriction digest + gel electrophoresis simulator
for DNA sequences. Everything the plugin needs lives in this directory; the
only runtime dependency is React.

## Files

| File            | Role                                                                                                                                                                                                       |
| --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `gel.js`        | Zero-dependency core: ladder parsing, enzyme site grouping/filtering, partial-digest enumeration, gel migration model (topology, concentration, run time, diffusion). Usable on its own in any JS project. |
| `GelDialog.jsx` | Dialog UI: enzyme picker (single/double/other cutters, cutting efficiency), ladder presets + custom ladders, gel matrix/concentration/run-time controls, SVG gel rendering.                                |
| `ui.jsx`        | Minimal dialog/label/notice primitives so the plugin does not import the host's UI kit.                                                                                                                    |
| `index.js`      | Plugin object for the LibreGene static registry (`src/plugins/index.js`).                                                                                                                                  |
| `manifest.json` | Machine-readable metadata and the host contract.                                                                                                                                                           |

## Host contract

- The host renders the dialog component with props
  `{ open, onOpenChange, sequence, fileName, enzymes, topology, myEnzymes, dialogState }`.
- `enzymes` is the per-site restriction list from project data
  (`[{ name, recSeq, cutIndex, botCutIndex, ... }]`, 0-based coordinates).
- `topology` is `'circular'` or `'linear'`.
- `myEnzymes` is the user's saved enzyme name list; adds a "My Enzymes" group.
- `dialogState` may be `{ enzymes: string[] }` to preselect enzymes on open
  (used by the enzyme-label context menu and the editor's enzyme selection).
- Styling assumes Tailwind CSS with shadcn-style theme tokens.
- The default enzyme group is read once from localStorage `enzymeFilter`
  (the editor's current enzyme set).

## Simulation model

- **Digest**: each selected enzyme cuts at every recognition site with the
  chosen per-site efficiency; partial-digest products are enumerated exactly
  (≤16 sites) or approximated (full digest + one-site-missed partials).
- **Topology**: uncut circular DNA runs as a single supercoiled band
  (apparent size 0.95×); any cut linearizes the molecule.
- **Migration**: log-size mobility within a range set by gel matrix and
  concentration; band intensity is proportional to fragment mass (length ×
  molar fraction).
- **Diffusion**: band width grows with run time and is larger for small
  fragments and for topoisomers.
- **Run time**: a conservative default is derived from the ladder (brightest
  band at ~72% of the gel, smallest band still on the gel); the slider scales
  it 0.3×–3×.

## Installing into a LibreGene host

1. Copy this directory to `src/plugins/gel/`.
2. Register it in the host's plugin registry:

```js
import gelPlugin from './gel';

export const plugins = [
  // ...
  gelPlugin,
];
```

3. Open it by setting the host's `pluginDialogs.gel` flag to `true`
   (in LibreGene this is wired to the "Digest and Run Gel" item in the
   Diagrams menu).

## Using only the simulation

`gel.js` has no imports at all:

```js
import { digest, simulateLane, gelRange, parseLadderText } from './gel.js';

const bands = simulateLane({
  length: 5000,
  circular: true,
  cuts: [1000, 3000],
  efficiency: 0.9,
  range: gelRange('agarose', 1.0),
  timeFactor: 1,
});
```
