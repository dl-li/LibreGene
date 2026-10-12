# LibreGene Plugins

Plugins extend LibreGene with new tools (dialogs, tracks, menu entries). They
are compiled into the app — there is no runtime loading — so contributing a
plugin means opening a pull request against this repository.

## Directory layout

```
src/plugins/
├── index.js                  # static registry — every plugin must be listed here
├── README.md                 # this file
├── <id>/                     # your plugin goes here, one directory per plugin
├── builtin/                  # the editor's original built-in plugins
└── example-plugin-*/         # self-contained reference examples (dotplot, gel)
```

- **`<id>/`** — the default location for a new plugin: a single
  camelCase directory at the top level of `src/plugins/`.
- **`builtin/`** — where the pre-existing built-in plugins live. Don't add new
  plugins here.
- **`example-plugin-*`** — two fully self-contained example plugins. Treat
  them as documentation you can run: read them to see the plugin object,
  dialog contract and styling conventions in action.

## The plugin object

Each plugin's `index.js` default-exports a plain object:

```js
const myPlugin = {
  id: 'myPlugin', // unique, camelCase; used for disable state & dialog keys
  name: 'My Plugin',
  description: 'One line shown in the Plugins settings window',
  version: '1.0.0',

  // Fully declarative hooks — work with zero host-side changes:
  dialogKey: 'myPlugin', // key into the pluginDialogs open-state map; null if
  // no dialog. May differ from id (e.g. snapgeneHistory uses 'snapshots').
  dialog: MyDialog, // React component, receives { open, onOpenChange, ... }
  // Optional predicate gating every entry of the plugin; receives
  // { projectId, moleculeType, isTauri }.
  dialogVisible: ({ projectId, moleculeType, isTauri }) => true,
  sidebarItems: [{ dialogKey, label, tooltip, icon }],

  // Molecule-type gating (optional):
  dnaOnly: false, // DNA projects only
  rnaOnly: false, // RNA projects only
  notForProtein: false, // hidden for protein projects

  // View-menu visibility toggle (optional). State is persisted per plugin id
  // in localStorage (key `pluginToggle:<id>`); defaultChecked defaults to
  // false. Also gates the track hook's `enabled` flag.
  featuresMenuItem: { label, defaultChecked },

  // Numeric field in Settings (optional). Persisted like the toggle (key
  // `pluginSetting:<id>`); defaultValue falls back to min. Values are rounded
  // and clamped to [min, max].
  settingsField: { label, description, min, max, unit, defaultValue },

  // Custom track below the sequence (optional). useLane receives
  // { cleanSeq, topology, moleculeType, enabled, settingValue } and returns
  // { height, ... } | null; render(ctx, lane) draws it.
  track: { useLane, render },
};

export default myPlugin;
```

Then register it in `index.js`:

```js
import myPlugin from './myPlugin';

export const plugins = [
  // ...
  myPlugin,
];
```

## Conventions

- **Disable-ability is mandatory.** Every plugin must disappear completely
  when the user disables it (localStorage `disabledPlugins`): sidebar entries,
  dialogs and nav-menu items are all derived from the registry, so register
  every entry point through the plugin object rather than hard-coding buttons.
- **Track rendering goes through the visual-cell stream.** A `track` plugin
  implements `useLane(ctx)` + `render(ctx, lane)`. Anything anchored to
  template columns must convert coordinates with `ctx`'s
  `colVis`/`colRuns`/`colFromVis` helpers — never assume a linear
  column→x mapping (see `builtin/gcContent/track.jsx`).
- **Dialog props are the host contract.** Every plugin dialog is rendered with
  the same fixed prop set (see `src/workspace/WorkspaceDialogs.jsx`):
  `{ open, onOpenChange, projectId, sequence, fileName, features, enzymes,
myEnzymes, enzymeFilter, topology, alignments, dialogState, onAddAlignment,
onRemoveAlignment, onOpenSnapshot, onProjectChanged, watermark,
onToggleWatermark }`. Take what you need and ignore the rest. Close the
  dialog via `onOpenChange(false)`; do not unmount it yourself.
  `dialogState` carries whatever the opener stored in
  `pluginDialogs[dialogKey]` (e.g. `{ enzymes: [...] }` to preselect gel
  enzymes).
- **Gating has a single source of truth.** Disabled state, molecule-type flags
  and `dialogVisible` are evaluated by `pluginAvailable(plugin, ctx)` from
  `src/plugins/index.js` — every consumer (sidebar, dialogs, View menu,
  tracks, nav-menu openers) goes through it, so declare the flags on the
  plugin object and they apply everywhere.
- **JSON fields are camelCase**, matching the rest of the codebase.
- **Respect molecule type.** If your tool is meaningless for RNA/protein, set
  the gating flags instead of checking `moleculeType` yourself.
- Keep comments minimal and in English; commit messages are English
  (`feat:` / `fix:` / `refactor:`).

## Host wiring

Everything except nav-menu entries works purely from the registry: `dialog`,
`sidebarItems`, molecule gating, disable-ability, `featuresMenuItem` (View-menu
toggle), `settingsField` (Settings field) and `track` lanes all function
without touching host code — toggle/field state is created and persisted
automatically per plugin id (`src/hooks/useSettings.js`).

The one exception is **nav-menu entries** (e.g. the Diagrams menu): the menu
item itself is wired in `src/EditorNavMenu.jsx` with an `onOpen<Plugin>`
callback threaded from `src/ProjectWorkspace.jsx`, gated there via
`pluginGate('<id>')` (a `pluginAvailable` wrapper). Follow the `onOpenDotplot`
/ `onOpenGel` examples.

## Verify before submitting

```bash
npx vite build    # frontend compiles
npm run lint      # ESLint
npm test          # vitest
```

Then open the app (`npx tauri dev`), confirm the plugin appears in the
**Plugins** window (sidebar → Plugins), and toggle it off/on to check that all
of its entry points vanish and return.

## Distributable plugins

If you want your plugin to be reusable outside LibreGene, keep it
self-contained like the examples: no imports outside its own directory (React
excepted), its own minimal UI primitives (`ui.jsx`), and a `manifest.json` +
`README.md` documenting the host contract. Self-containment is a property of
the plugin, not a naming requirement — your directory is still
`src/plugins/<id>/`. The two example plugins are the reference implementation.
