import { useEffect, useRef } from 'react';

// fornac.css is a full-page stylesheet (global `svg`/`text` rules) that breaks
// the rest of the app, so only the needed rules are inlined here, scoped to
// .forna-view.
const SCOPED_CSS = `
.forna-view svg { display: block; min-width: 100%; width: 100%; min-height: 100%; }
.forna-view circle.node { stroke: #ccc; stroke-width: 1px; opacity: 1; fill: white; }
.forna-view circle.node.label { stroke: transparent; stroke-width: 0; fill: white; }
.forna-view circle.outline_node { stroke-width: 1px; fill: red; visibility: hidden; }
.forna-view circle.outline_node.selected { visibility: visible; }
.forna-view line.link { stroke: #999; stroke-opacity: 0.8; stroke-width: 2; }
.forna-view line.basepair { stroke: red; }
.forna-view line.fake { stroke: green; }
.forna-view line.pseudoknot { stroke: red; }
.forna-view path.node { display: none; }
.forna-view text { pointer-events: none; }
.forna-view text.node-label { font-size: 10.5px; font-weight: bold; font-family: 'Cascadia Code', ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace; color: rgb(100,100,100); transform: translateY(1.3px); }
.forna-view .transparent { fill: transparent; stroke-width: 0; stroke-opacity: 0; opacity: 0; visibility: hidden; }
`;

let viewCounter = 0;
let scriptPromise = null;

// fornac is a 2016-era UMD bundle whose default-export interop breaks under
// Vite's dev pre-bundling (TDZ on `default`). Load it as a classic script
// instead: with no CJS/AMD environment present it sets `window.fornac`.
// The bundle also embeds a style-loader that injects a GLOBAL stylesheet
// (`svg { min-width:100% }` etc.) into <head> on load — remove any style
// tags it adds, or the whole app's SVG UI breaks.
function loadFornac() {
  if (!scriptPromise) {
    scriptPromise = import('fornac/dist/scripts/fornac.js?url').then(
      ({ default: url }) =>
        new Promise((resolve, reject) => {
          if (window.fornac) return resolve(window.fornac);
          const existing = new Set(document.head.querySelectorAll('style'));
          const s = document.createElement('script');
          s.src = url;
          s.onload = () => {
            document.head.querySelectorAll('style').forEach((t) => {
              if (!existing.has(t)) t.remove();
            });
            resolve(window.fornac);
          };
          s.onerror = () => reject(new Error('Failed to load fornac'));
          document.head.appendChild(s);
        }),
    );
  }
  return scriptPromise;
}

export default function FornaView({ sequence, structure, height = 520 }) {
  const hostRef = useRef(null);

  useEffect(() => {
    if (!sequence || !structure) return undefined;
    const host = hostRef.current;
    let disposed = false;
    let container = null;

    loadFornac().then((fornac) => {
      if (disposed) return;
      host.innerHTML = '';
      const div = document.createElement('div');
      div.id = `rna-fold-view-${++viewCounter}`;
      div.style.width = '100%';
      div.style.height = `${height}px`;
      host.appendChild(div);
      container = new fornac.FornaContainer(`#${div.id}`, {
        applyForce: true,
        allowPanningAndZooming: true,
        initialSize: [div.clientWidth || 800, height],
      });
      container.addRNA(structure, { sequence });
    });

    return () => {
      disposed = true;
      try {
        container?.force?.stop();
      } catch {
        // fornac internals may already be torn down
      }
      host.innerHTML = '';
    };
  }, [sequence, structure, height]);

  return (
    <div className="forna-view">
      <style>{SCOPED_CSS}</style>
      <div ref={hostRef} />
    </div>
  );
}
