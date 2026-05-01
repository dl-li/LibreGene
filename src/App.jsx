import React, { useState, useEffect } from 'react';
import SequenceEditor from './SequenceEditor';
import { getProject } from './api';

const baseSeq = "TACGAATTCGCCACCATGGCCATGAAGCTTGAGCTCGGATCCTCTAGAGCTGATCGATCGTAGCTAGCTAGCTGATCGATCGTAGCTAGCTAGCTAGCTACGATCGATCGATCGTAGCTAGCTAGCTGATCCTAGCTAGCTAGCTGATCGATCGTAGCATCGTAGC" +
  "ACGTGTGCTAGCTAGCGCTATATATATAGCGCGCGCGTATATAGCTAGCTAGCTCGATCGATCGTAGCTAGCTGCATCGATCGTAGCTGATCGTAGCTGATCGATCGTAGCTAGCTAGCTGATCATATCATGATCGATCGTATGCGCGCGCTATTAGCTAGCTGAT" +
  "CCGGAATTCCTCGAGAAGCTTTTCTAGAGGATCCTAGCCTAGCCTTAGCTAGCTAGCTGATCGATCGTCTAGAGCTCGAATTC";

const defaultFeatures = [
  { id: 'Promoter', name: 'T7 Promoter', start: 10, end: 40, color: '#34d399' },
  { id: 'CDS_Huge', name: 'Monomeric GFP', start: 105, end: 280, color: '#60A5FA' },
  { id: 'CDS_Split', name: 'Split CDS', segments: [{ start: 120, end: 155 }, { start: 195, end: 240 }], color: '#f59e0b' },
  { id: 'Intron', name: 'Intron 1', start: 150, end: 190, color: '#f472b6' },
  { id: 'TATA', name: 'TATA Box', start: 200, end: 220, color: '#a78bfa' },
  { id: 'Terminator', name: 'Terminator', start: 300, end: 320, color: '#9ca3af' },
];

const defaultEnzymes = [
  { id: 'EcoRI', name: 'EcoRI', cutIndex: 4, recSeq: 'GAATTC', recSeqPattern: 'GAATTC', compSeq: 'CTTAAG', recStart: 3, recEnd: 8, displayStart: 3, displayEnd: 8, topCutInRec: 1, botCutInRec: 5, botCutIndex: 8, isUnique: true },
  { id: 'HindIII', name: 'HindIII', cutIndex: 23, recSeq: 'AAGCTT', recSeqPattern: 'AAGCTT', compSeq: 'TTCGAA', recStart: 22, recEnd: 27, displayStart: 22, displayEnd: 27, topCutInRec: 1, botCutInRec: 5, botCutIndex: 27, isUnique: true },
  { id: 'BamHI', name: 'BamHI', cutIndex: 42, recSeq: 'GGATCC', recSeqPattern: 'GGATCC', compSeq: 'CCTAGG', recStart: 41, recEnd: 46, displayStart: 41, displayEnd: 46, topCutInRec: 1, botCutInRec: 5, botCutIndex: 46, isUnique: false },
  { id: 'XbaI_Cross', name: 'XbaI', cutIndex: 58, recSeq: 'TCTAGA', recSeqPattern: 'TCTAGA', compSeq: 'AGATCT', recStart: 57, recEnd: 62, displayStart: 57, displayEnd: 62, topCutInRec: 1, botCutInRec: 5, botCutIndex: 62, isUnique: false },
  { id: 'EcoRI_2', name: 'EcoRI', cutIndex: 334, recSeq: 'GAATTC', recSeqPattern: 'GAATTC', compSeq: 'CTTAAG', recStart: 333, recEnd: 338, displayStart: 333, displayEnd: 338, topCutInRec: 1, botCutInRec: 5, botCutIndex: 338, isUnique: false },
  { id: 'BsaI_Exo', name: 'BsaI', cutIndex: 65, botCutIndex: 69, recSeq: 'GGTCTC', recSeqPattern: 'GGTCTC', compSeq: 'CCAGAG', recStart: 51, recEnd: 56, displayStart: 51, displayEnd: 69, topCutInRec: 14, botCutInRec: 18, isUnique: true },
  { id: 'Exo_Up', name: 'UpExo', cutIndex: 78, botCutIndex: 74, recSeq: 'GATCGT', recSeqPattern: 'GATCGT', compSeq: 'CTAGCA', recStart: 86, recEnd: 91, displayStart: 74, displayEnd: 91, topCutInRec: -8, botCutInRec: -12, isUnique: true },
  { id: 'SameSeq_DiffCut_1', name: 'IsoA', cutIndex: 110, recSeq: 'ATCGAT', recSeqPattern: 'ATCGAT', compSeq: 'TAGCTA', recStart: 108, recEnd: 113, displayStart: 108, displayEnd: 113, topCutInRec: 2, botCutInRec: 4, botCutIndex: 112, isUnique: false },
  { id: 'SameSeq_DiffCut_2', name: 'IsoB', cutIndex: 112, recSeq: 'ATCGAT', recSeqPattern: 'ATCGAT', compSeq: 'TAGCTA', recStart: 108, recEnd: 113, displayStart: 108, displayEnd: 113, topCutInRec: 4, botCutInRec: 6, botCutIndex: 114, isUnique: false },
  { id: 'SameCut_DiffSeq_1', name: 'SymA', cutIndex: 150, recSeq: 'GCGCGC', recSeqPattern: 'GCGCGC', compSeq: 'CGCGCG', recStart: 147, recEnd: 152, displayStart: 147, displayEnd: 152, topCutInRec: 3, botCutInRec: 5, botCutIndex: 152, isUnique: false },
  { id: 'SameCut_DiffSeq_2', name: 'SymB', cutIndex: 150, recSeq: 'GTATAT', recSeqPattern: 'GTATAT', compSeq: 'CATATA', recStart: 149, recEnd: 154, displayStart: 149, displayEnd: 154, topCutInRec: 1, botCutInRec: 3, botCutIndex: 152, isUnique: false },
  { id: 'TailBlock', name: 'TailBlock', cutIndex: 160, recSeq: 'ATCGAT', recSeqPattern: 'ATCGAT', compSeq: 'TAGCTA', recStart: 159, recEnd: 164, displayStart: 159, displayEnd: 164, topCutInRec: 1, botCutInRec: 5, botCutIndex: 164, isUnique: false },
  { id: 'DraIII', name: 'DraIII', cutIndex: 250, recSeq: 'CACAAAGTG', recSeqPattern: 'CACNNNGTG', compSeq: 'GTGTTTCAC', recStart: 248, recEnd: 256, displayStart: 248, displayEnd: 256, topCutInRec: 2, botCutInRec: 7, botCutIndex: 255, spacers: [], isUnique: true },
];

const defaultPrimers = [
  { id: 'Fwd1', name: 'Fwd Start', type: 'fwd', matchStart: 0, matchEnd: 15, mismatchStr: 'cagta', matchStr: 'TACGAATTCGCCACCA', color: '#166534' },
  { id: 'Fwd1b', name: 'Fwd Overlap', type: 'fwd', matchStart: 5, matchEnd: 22, mismatchStr: 'ggcc', matchStr: baseSeq.substring(5, 23), color: '#166534' },
  { id: 'Rev1', name: 'Rev Mid', type: 'rev', matchStart: 48, matchEnd: 65, mismatchStr: 'tttt', matchStr: baseSeq.substring(48, 66), color: '#166534' },
  { id: 'Rev1b', name: 'Rev Overlap', type: 'rev', matchStart: 52, matchEnd: 70, mismatchStr: 'aaaa', matchStr: baseSeq.substring(52, 71), color: '#166534' },
  { id: 'Fwd2', name: 'Fwd Inner HA', type: 'fwd', matchStart: 180, matchEnd: 195, mismatchStr: 'gatactatacgatgttccagattacgctctgc', mismatchFeatures: [{ name: 'HA tag', start: 6, end: 14 }], matchStr: baseSeq.substring(180, 196), color: '#166534' },
  { id: 'Rev2', name: 'Rev End BamHI', type: 'rev', matchStart: 270, matchEnd: 295, mismatchStr: 'ggatccatcg', mismatchFeatures: [{ name: 'BamHI', start: 0, end: 5 }], matchStr: baseSeq.substring(270, 296), color: '#166534' },
];

export default function App() {
  const [sequence, setSequence] = useState(baseSeq);
  const [features, setFeatures] = useState(defaultFeatures);
  const [enzymes, setEnzymes] = useState(defaultEnzymes);
  const [primers, setPrimers] = useState(defaultPrimers);
  const [backendStatus, setBackendStatus] = useState('offline');

  // Debug toggles
  const [panelOpen, setPanelOpen] = useState(false);
  const [showFeatures, setShowFeatures] = useState(true);
  const [showPrimers, setShowPrimers] = useState(true);
  const [enzymeFilter, setEnzymeFilter] = useState('unique'); // 'unique' | 'all'

  useEffect(() => {
    let ws = null;
    let cancelled = false;

    async function connect() {
      setBackendStatus('connecting');
      try {
        const data = await getProject();
        if (cancelled) return;
        if (data && !data.error) {
          setSequence(data.sequence || baseSeq);
          setFeatures(data.features || defaultFeatures);
          setEnzymes(data.enzymes || defaultEnzymes);
          setPrimers(data.primers || defaultPrimers);
          setBackendStatus('online');
        }
      } catch {
        if (!cancelled) setBackendStatus('offline');
      }
    }

    connect();

    try {
      ws = new WebSocket('ws://127.0.0.1:8765/ws');
      ws.onopen = () => { if (!cancelled) setBackendStatus('online'); };
      ws.onmessage = (e) => {
        try {
          const msg = JSON.parse(e.data);
          if (msg.type === 'project' && msg.data && !cancelled) {
            setSequence(msg.data.sequence || baseSeq);
            setFeatures(msg.data.features || defaultFeatures);
            setEnzymes(msg.data.enzymes || defaultEnzymes);
            setPrimers(msg.data.primers || defaultPrimers);
          }
        } catch {}
      };
      ws.onclose = () => { if (!cancelled) setBackendStatus('offline'); };
    } catch {}

    return () => { cancelled = true; if (ws) ws.close(); };
  }, []);

  // Enzyme filter logic
  const displayEnzymes = (() => {
    const all = enzymes || [];
    if (enzymeFilter === 'all') return all;
    if (enzymeFilter === 'unique') return all.filter(e => e.isUnique);
    // Unique 6-cutters
    if (enzymeFilter === 'unique6') return all.filter(e => e.isUnique && e.recSeq?.length === 6);
    // By overhang type
    const stagger = e => e.botCutInRec - e.topCutInRec;
    if (enzymeFilter === 'blunt') return all.filter(e => stagger(e) === 0);
    if (enzymeFilter === 'overhang5') return all.filter(e => stagger(e) > 0);
    if (enzymeFilter === 'overhang3') return all.filter(e => stagger(e) < 0);
    // Type IIS (cut far from recognition)
    if (enzymeFilter === 'iis') return all.filter(e => {
      const rl = e.recSeq?.length || 6;
      return Math.abs(e.topCutInRec) > rl || Math.abs(e.botCutInRec) > rl;
    });
    // By rec length
    if (enzymeFilter === 'rec4') return all.filter(e => e.recSeq?.length === 4);
    if (enzymeFilter === 'rec5') return all.filter(e => e.recSeq?.length === 5);
    if (enzymeFilter === 'rec6') return all.filter(e => e.recSeq?.length === 6);
    if (enzymeFilter === 'rec8p') return all.filter(e => (e.recSeq?.length || 0) >= 8);
    return all.filter(e => e.isUnique); // default
  })();

  return (
    <div className="w-full min-h-screen bg-[#fdfbf7] relative">
      <SequenceEditor
        sequence={sequence}
        features={showFeatures ? features : []}
        enzymes={displayEnzymes}
        primers={showPrimers ? primers : []}
        charsPerLine={60}
      />

      {/* Collapsible debug panel — right side */}
      <div className={`fixed top-0 right-0 h-screen bg-white/90 backdrop-blur border-l border-gray-200 shadow-lg z-50 transition-all duration-200 ${panelOpen ? 'w-56' : 'w-8'}`}>
        <button
          className="absolute -left-6 top-4 w-6 h-12 bg-white border border-gray-200 rounded-l flex items-center justify-center text-gray-500 hover:text-gray-800"
          onClick={() => setPanelOpen(!panelOpen)}
          title="Toggle debug panel"
        >
          {panelOpen ? '▶' : '◀'}
        </button>

        {panelOpen && (
          <div className="p-3 pt-12 space-y-4 text-sm text-gray-700 overflow-y-auto h-full">
            <h3 className="font-bold text-gray-900 border-b pb-1">Debug Panel</h3>

            <label className="flex items-center gap-2 cursor-pointer">
              <input type="checkbox" checked={showFeatures} onChange={e => setShowFeatures(e.target.checked)} />
              Features ({features.length})
            </label>

            <label className="flex items-center gap-2 cursor-pointer">
              <input type="checkbox" checked={showPrimers} onChange={e => setShowPrimers(e.target.checked)} />
              Primers ({primers.length})
            </label>

            <div>
              <div className="font-medium mb-1">Enzymes ({enzymes.length} total, {displayEnzymes.length} shown)</div>
              <select className="w-full text-xs border rounded p-1" value={enzymeFilter}
                onChange={e => setEnzymeFilter(e.target.value)}>
                <option value="unique">Unique only</option>
                <option value="all">All with cuts</option>
                <optgroup label="By overhang">
                  <option value="blunt">Blunt</option>
                  <option value="overhang5">5′ overhang</option>
                  <option value="overhang3">3′ overhang</option>
                </optgroup>
                <optgroup label="By rec length">
                  <option value="unique6">Unique 6‑cutters</option>
                  <option value="rec4">4 bp</option>
                  <option value="rec5">5 bp</option>
                  <option value="rec6">6 bp</option>
                  <option value="rec8p">≥8 bp</option>
                </optgroup>
                <optgroup label="Special">
                  <option value="iis">Type IIS</option>
                </optgroup>
              </select>
            </div>
          </div>
        )}
      </div>

      <div
        className="fixed bottom-3 right-3 w-2.5 h-2.5 rounded-full z-50 opacity-60"
        style={{ background: backendStatus === 'online' ? '#22c55e' : backendStatus === 'connecting' ? '#f59e0b' : '#9ca3af' }}
        title={backendStatus === 'online' ? 'Backend connected' : backendStatus === 'connecting' ? 'Connecting...' : 'Offline (demo data)'}
      />
    </div>
  );
}