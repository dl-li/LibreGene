import { useState, useEffect, useRef, useCallback } from 'react';
import { AlertTriangle } from 'lucide-react';
import { monoFont } from '../editorConstants';

// ---------------------------------------------------------------------------
// WarningBadge — floating indicator in bottom-right corner for various
// warnings (e.g. primers with no binding sites, CDS non-triplet length).
// Click to expand, mouse-leave to close.
// ---------------------------------------------------------------------------
function WarningBadge({ warnings }) {
  const [expanded, setExpanded] = useState(false);
  const ref = useRef(null);

  const onClick = useCallback(() => setExpanded((v) => !v), []);

  useEffect(() => {
    if (!expanded) return;
    const el = ref.current;
    if (!el) return;
    const handler = () => setExpanded(false);
    el.addEventListener('mouseleave', handler);
    return () => el.removeEventListener('mouseleave', handler);
  }, [expanded]);

  const primerWarnings = warnings.filter((w) => w.type === 'primer');
  const cdsLenWarnings = warnings.filter((w) => w.type === 'cds_len');
  const cdsTransWarnings = warnings.filter((w) => w.type === 'cds_trans');

  return (
    <div ref={ref} style={{ position: 'fixed', bottom: 14, right: 28, zIndex: 40 }}>
      {!expanded && (
        <div
          onClick={onClick}
          style={{
            backgroundColor: '#fef3c7',
            color: '#92400e',
            border: '1px solid #fde68a',
            borderRadius: 8,
            fontSize: '11px',
            lineHeight: '1.2',
            padding: '3px 7px',
            cursor: 'pointer',
            display: 'flex',
            alignItems: 'center',
            gap: 3,
          }}
        >
          <AlertTriangle className="size-3.5" />
          <span>{warnings.length}</span>
        </div>
      )}
      {expanded && (
        <div
          onClick={onClick}
          style={{
            backgroundColor: '#fef3c7',
            color: '#92400e',
            border: '1px solid #fde68a',
            borderRadius: 8,
            fontSize: '12px',
            lineHeight: '1.4',
            padding: '6px 10px',
            boxShadow: '0 2px 8px rgba(0,0,0,0.12)',
            maxWidth: 320,
            cursor: 'pointer',
          }}
        >
          {primerWarnings.length > 0 && (
            <>
              <div className="flex items-center gap-1.5 mb-1">
                <AlertTriangle className="size-3.5 shrink-0" />
                <span className="font-semibold">Unmatched Primers</span>
              </div>
              <ul style={{ margin: 0, paddingLeft: 18, listStyle: 'disc' }}>
                {primerWarnings.map((w) => (
                  <li key={w.id} style={{ fontFamily: monoFont, fontSize: '11px' }}>
                    {w.name}
                  </li>
                ))}
              </ul>
            </>
          )}
          {(cdsLenWarnings.length > 0 || cdsTransWarnings.length > 0) && (
            <>
              {primerWarnings.length > 0 && <div style={{ height: 6 }} />}
              <div className="flex items-center gap-1.5 mb-1">
                <AlertTriangle className="size-3.5 shrink-0" />
                <span className="font-semibold">Check Translation:</span>
              </div>
              <ul style={{ margin: 0, paddingLeft: 18, listStyle: 'disc' }}>
                {[...cdsLenWarnings, ...cdsTransWarnings].map((w) => (
                  <li key={w.id} style={{ fontFamily: monoFont, fontSize: '11px' }}>
                    {w.name}
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      )}
    </div>
  );
}

export default WarningBadge;
