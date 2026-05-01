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
  { id: 'EcoRI', name: 'EcoRI', cutIndex: 4, seq: 'GAATTC', compSeq: 'CTTAAG', topCut: 1, botCut: 5, isUnique: true },
  { id: 'HindIII', name: 'HindIII', cutIndex: 23, seq: 'AAGCTT', compSeq: 'TTCGAA', topCut: 1, botCut: 5, isUnique: true },
  { id: 'BamHI', name: 'BamHI', cutIndex: 40, seq: 'GGATCC', compSeq: 'CCTAGG', topCut: 1, botCut: 5, isUnique: false },
  { id: 'XbaI_Cross', name: 'XbaI', cutIndex: 58, seq: 'TCTAGA', compSeq: 'AGATCT', topCut: 1, botCut: 5, isUnique: false },
  { id: 'EcoRI_2', name: 'EcoRI', cutIndex: 334, seq: 'GAATTC', compSeq: 'CTTAAG', topCut: 1, botCut: 5, isUnique: false },
  { id: 'BsaI_Exo', name: 'BsaI', cutIndex: 65, botCutIndex: 69, recStart: 51, seq: 'GGTCTC', compSeq: 'CCAGAG', topCut: 1, botCut: 5, isUnique: true },
  { id: 'Exo_Up', name: 'UpExo', cutIndex: 78, botCutIndex: 74, recStart: 86, seq: 'GATCGT', compSeq: 'CTAGCA', topCut: -8, botCut: -12, isUnique: true },
  { id: 'SameSeq_DiffCut_1', name: 'IsoA', cutIndex: 110, seq: 'ATCGAT', compSeq: 'TAGCTA', topCut: 2, botCut: 4, isUnique: false },
  { id: 'SameSeq_DiffCut_2', name: 'IsoB', cutIndex: 112, seq: 'ATCGAT', compSeq: 'TAGCTA', topCut: 4, botCut: 6, isUnique: false },
  { id: 'SameCut_DiffSeq_1', name: 'SymA', cutIndex: 150, seq: 'GCGCGC', compSeq: 'CGCGCG', topCut: 3, botCut: 5, isUnique: false },
  { id: 'SameCut_DiffSeq_2', name: 'SymB', cutIndex: 150, seq: 'GTATAT', compSeq: 'CATATA', topCut: 1, botCut: 3, isUnique: false },
  { id: 'TailBlock', name: 'TailBlock', cutIndex: 160, seq: 'ATCGAT', compSeq: 'TAGCTA', topCut: 1, botCut: 5, isUnique: false },
  { id: 'DraIII', name: 'DraIII', cutIndex: 250, seq: 'CACNNNGTG', compSeq: 'GTGNNNCAC', topCut: 6, botCut: 3, spacer: { start: 3, end: 6 }, isUnique: true },
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

  // Filter enzymes based on debug setting
  const displayEnzymes = enzymeFilter === 'unique'
    ? (enzymes || []).filter(e => e.isUnique)
    : enzymes;

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
              <div className="font-medium mb-1">Enzymes ({enzymes.length})</div>
              <label className="flex items-center gap-2 cursor-pointer ml-1">
                <input type="radio" name="enzymeFilter" checked={enzymeFilter === 'unique'}
                  onChange={() => setEnzymeFilter('unique')} />
                Unique only
              </label>
              <label className="flex items-center gap-2 cursor-pointer ml-1">
                <input type="radio" name="enzymeFilter" checked={enzymeFilter === 'all'}
                  onChange={() => setEnzymeFilter('all')} />
                All
              </label>
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