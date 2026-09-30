import React from 'react';
import { CircularMap, LinearMap } from '../MapView';

// ---------------------------------------------------------------------------
// MapWatermark — non-interactive plasmid map rendered as a faint overlay on
// top of the editor (toggled from the Map dialog footer). Lives outside the
// main container because `contain: layout style` breaks position: fixed.
// ---------------------------------------------------------------------------
const noop = () => {};

const MapWatermark = React.memo(function MapWatermark({ length, features, topology, name, sel }) {
  if (!length) return null;
  return (
    <div
      aria-hidden
      className="[&_*]:pointer-events-none"
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 10,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        pointerEvents: 'none',
      }}
    >
      <div style={{ width: 680, opacity: 0.1 }}>
        {topology === 'circular' ? (
          <CircularMap
            length={length}
            features={features}
            name={name}
            selection={sel}
            bg="transparent"
            onSelect={noop}
            onClear={noop}
            onFeatureOpen={noop}
            hideLabels
          />
        ) : (
          <LinearMap
            length={length}
            features={features}
            name={name}
            selection={sel}
            onSelect={noop}
            onClear={noop}
            onFeatureOpen={noop}
            hideLabels
          />
        )}
      </div>
    </div>
  );
});

export default MapWatermark;
