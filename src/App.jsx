import React, { useState, useEffect } from 'react';
import SequenceEditor from './SequenceEditor';
import { getProject, openFile, setMethylation } from './api';

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
  { id: 'EcoRI', name: 'EcoRI', recSeq: 'GAATTC', recSeqPattern: 'GAATTC', compSeq: 'CTTAAG', recStart: 3, recEnd: 8, displayStart: 3, displayEnd: 8, cutIndex: 4, botCutIndex: 8, cutPairs: [{ topCutIndex: 4, botCutIndex: 8 }], recognitionStrand: 'top', isUnique: true },
  { id: 'HindIII', name: 'HindIII', recSeq: 'AAGCTT', recSeqPattern: 'AAGCTT', compSeq: 'TTCGAA', recStart: 22, recEnd: 27, displayStart: 22, displayEnd: 27, cutIndex: 23, botCutIndex: 27, cutPairs: [{ topCutIndex: 23, botCutIndex: 27 }], recognitionStrand: 'top', isUnique: true },
  { id: 'BamHI', name: 'BamHI', recSeq: 'GGATCC', recSeqPattern: 'GGATCC', compSeq: 'CCTAGG', recStart: 41, recEnd: 46, displayStart: 41, displayEnd: 46, cutIndex: 42, botCutIndex: 46, cutPairs: [{ topCutIndex: 42, botCutIndex: 46 }], recognitionStrand: 'top', isUnique: false },
  { id: 'XbaI_Cross', name: 'XbaI', recSeq: 'TCTAGA', recSeqPattern: 'TCTAGA', compSeq: 'AGATCT', recStart: 57, recEnd: 62, displayStart: 57, displayEnd: 62, cutIndex: 58, botCutIndex: 62, cutPairs: [{ topCutIndex: 58, botCutIndex: 62 }], recognitionStrand: 'top', isUnique: false },
  { id: 'EcoRI_2', name: 'EcoRI', recSeq: 'GAATTC', recSeqPattern: 'GAATTC', compSeq: 'CTTAAG', recStart: 333, recEnd: 338, displayStart: 333, displayEnd: 338, cutIndex: 334, botCutIndex: 338, cutPairs: [{ topCutIndex: 334, botCutIndex: 338 }], recognitionStrand: 'top', isUnique: false },
  { id: 'BsaI_Exo', name: 'BsaI', recSeq: 'GGTCTC', recSeqPattern: 'GGTCTC', compSeq: 'CCAGAG', recStart: 51, recEnd: 56, displayStart: 51, displayEnd: 69, cutIndex: 65, botCutIndex: 69, cutPairs: [{ topCutIndex: 65, botCutIndex: 69 }], recognitionStrand: 'top', isUnique: true },
  { id: 'Exo_Up', name: 'UpExo', recSeq: 'GATCGT', recSeqPattern: 'GATCGT', compSeq: 'CTAGCA', recStart: 86, recEnd: 91, displayStart: 74, displayEnd: 91, cutIndex: 78, botCutIndex: 74, cutPairs: [{ topCutIndex: 78, botCutIndex: 74 }], recognitionStrand: 'bottom', isUnique: true },
  { id: 'SameSeq_DiffCut_1', name: 'IsoA', recSeq: 'ATCGAT', recSeqPattern: 'ATCGAT', compSeq: 'TAGCTA', recStart: 108, recEnd: 113, displayStart: 108, displayEnd: 113, cutIndex: 110, botCutIndex: 112, cutPairs: [{ topCutIndex: 110, botCutIndex: 112 }], recognitionStrand: 'top', isUnique: false },
  { id: 'SameSeq_DiffCut_2', name: 'IsoB', recSeq: 'ATCGAT', recSeqPattern: 'ATCGAT', compSeq: 'TAGCTA', recStart: 108, recEnd: 113, displayStart: 108, displayEnd: 113, cutIndex: 112, botCutIndex: 114, cutPairs: [{ topCutIndex: 112, botCutIndex: 114 }], recognitionStrand: 'top', isUnique: false },
  { id: 'SameCut_DiffSeq_1', name: 'SymA', recSeq: 'GCGCGC', recSeqPattern: 'GCGCGC', compSeq: 'CGCGCG', recStart: 147, recEnd: 152, displayStart: 147, displayEnd: 152, cutIndex: 150, botCutIndex: 152, cutPairs: [{ topCutIndex: 150, botCutIndex: 152 }], recognitionStrand: 'top', isUnique: false },
  { id: 'SameCut_DiffSeq_2', name: 'SymB', recSeq: 'GTATAT', recSeqPattern: 'GTATAT', compSeq: 'CATATA', recStart: 149, recEnd: 154, displayStart: 149, displayEnd: 154, cutIndex: 150, botCutIndex: 152, cutPairs: [{ topCutIndex: 150, botCutIndex: 152 }], recognitionStrand: 'top', isUnique: false },
  { id: 'TailBlock', name: 'TailBlock', recSeq: 'ATCGAT', recSeqPattern: 'ATCGAT', compSeq: 'TAGCTA', recStart: 159, recEnd: 164, displayStart: 159, displayEnd: 164, cutIndex: 160, botCutIndex: 164, cutPairs: [{ topCutIndex: 160, botCutIndex: 164 }], recognitionStrand: 'top', isUnique: false },
  { id: 'DraIII', name: 'DraIII', recSeq: 'CACAAAGTG', recSeqPattern: 'CACNNNGTG', compSeq: 'GTGTTTCAC', recStart: 248, recEnd: 256, displayStart: 248, displayEnd: 256, cutIndex: 250, botCutIndex: 255, cutPairs: [{ topCutIndex: 250, botCutIndex: 255 }], recognitionStrand: 'top', spacers: [], isUnique: true },
];

const defaultPrimers = [
  { id: 'Fwd1', name: 'Fwd Start', type: 'fwd', matchStart: 0, matchEnd: 15, mismatchStr: 'cagta', matchStr: 'TACGAATTCGCCACCA', color: '#166534' },
  { id: 'Fwd1b', name: 'Fwd Overlap', type: 'fwd', matchStart: 5, matchEnd: 22, mismatchStr: 'ggcc', matchStr: baseSeq.substring(5, 23), color: '#166534' },
  { id: 'Rev1', name: 'Rev Mid', type: 'rev', matchStart: 48, matchEnd: 65, mismatchStr: 'tttt', matchStr: baseSeq.substring(48, 66), color: '#166534' },
  { id: 'Rev1b', name: 'Rev Overlap', type: 'rev', matchStart: 52, matchEnd: 70, mismatchStr: 'aaaa', matchStr: baseSeq.substring(52, 71), color: '#166534' },
  { id: 'Fwd2', name: 'Fwd Inner HA', type: 'fwd', matchStart: 180, matchEnd: 195, mismatchStr: 'gatactatacgatgttccagattacgctctgc', mismatchFeatures: [{ name: 'HA tag', start: 6, end: 14 }], matchStr: baseSeq.substring(180, 196), color: '#166534' },
  { id: 'Rev2', name: 'Rev End BamHI', type: 'rev', matchStart: 270, matchEnd: 295, mismatchStr: 'ggatccatcg', mismatchFeatures: [{ name: 'BamHI', start: 0, end: 5 }], matchStr: baseSeq.substring(270, 296), color: '#166534' },
];

function NumInput({ label, value, onChange, min, max, step }) {
  return (
    <label className="flex items-center justify-between gap-1 text-xs">
      <span className="text-gray-500 w-28 truncate" title={label}>{label}</span>
      <input className="w-14 border rounded px-1 py-0 text-xs text-right font-mono" type="number"
        value={value} min={min} max={max} step={step || 1}
        onChange={e => onChange(Number(e.target.value))} />
    </label>
  );
}

function PrimerLayoutDebug({ primerParams, setPP }) {
  const [open, setOpen] = useState(false);
  const pp = primerParams;

  return (
    <div>
      <button className="font-medium text-xs text-gray-700 hover:text-gray-900 w-full text-left py-1 border-t"
        onClick={() => setOpen(!open)}>
        {open ? '▾' : '▸'} Primer Layout
      </button>
      {open && (
        <div className="space-y-2 mt-1 mb-2">
          <div className="text-xs text-gray-400 font-medium">Match &amp; Mismatch</div>
          <NumInput label="fwdMatchY" value={pp.fwdMatchY} onChange={v => setPP('fwdMatchY', v)} min={0} max={100} />
          <NumInput label="revMatchY" value={pp.revMatchY} onChange={v => setPP('revMatchY', v)} min={0} max={100} />
          <NumInput label="misYDelta" value={pp.misYDelta} onChange={v => setPP('misYDelta', v)} min={0} max={20} />

          <div className="text-xs text-gray-400 font-medium">Text Position</div>
          <NumInput label="fwdBaseTextY" value={pp.fwdBaseTextY} onChange={v => setPP('fwdBaseTextY', v)} min={0} max={40} />
          <NumInput label="revBaseTextY" value={pp.revBaseTextY} onChange={v => setPP('revBaseTextY', v)} min={0} max={40} />
          <NumInput label="fwdLabelY" value={pp.fwdLabelY} onChange={v => setPP('fwdLabelY', v)} min={0} max={40} />
          <NumInput label="revLabelY" value={pp.revLabelY} onChange={v => setPP('revLabelY', v)} min={0} max={40} />

          <div className="text-xs text-gray-400 font-medium">Track &amp; Spacing</div>
          <NumInput label="trackGap" value={pp.trackGap} onChange={v => setPP('trackGap', v)} min={0} max={80} />
          <NumInput label="fwdAboveBase" value={pp.fwdAboveBase} onChange={v => setPP('fwdAboveBase', v)} min={0} max={80} />
          <NumInput label="fwdAboveExtra" value={pp.fwdAboveExtra} onChange={v => setPP('fwdAboveExtra', v)} min={0} max={60} />
          <NumInput label="fwdAboveNonTailExtra" value={pp.fwdAboveNonTailExtra} onChange={v => setPP('fwdAboveNonTailExtra', v)} min={0} max={30} />
          <NumInput label="revBelowBase" value={pp.revBelowBase} onChange={v => setPP('revBelowBase', v)} min={0} max={80} />
          <NumInput label="revBelowExtra" value={pp.revBelowExtra} onChange={v => setPP('revBelowExtra', v)} min={0} max={60} />
          <NumInput label="revBelowNonTailExtra" value={pp.revBelowNonTailExtra} onChange={v => setPP('revBelowNonTailExtra', v)} min={0} max={30} />

          <div className="text-xs text-gray-400 font-medium">Interaction</div>
          <NumInput label="hoverExpand" value={pp.hoverExpand} onChange={v => setPP('hoverExpand', v)} min={0} max={60} />
          <NumInput label="arrowHeadLen" value={pp.arrowHeadLen} onChange={v => setPP('arrowHeadLen', v)} min={0} max={20} />
          <NumInput label="arrowHeadHeight" value={pp.arrowHeadHeight} onChange={v => setPP('arrowHeadHeight', v)} min={0} max={20} />

          <div className="text-xs text-gray-400 font-medium">Enzyme Avoidance</div>
          <NumInput label="highestYBase" value={pp.highestYBase} onChange={v => setPP('highestYBase', v)} min={0} max={80} />
          <NumInput label="highestYFwdTail" value={pp.highestYFwdTail} onChange={v => setPP('highestYFwdTail', v)} min={0} max={80} />
          <NumInput label="highestYFwdNoTail" value={pp.highestYFwdNoTail} onChange={v => setPP('highestYFwdNoTail', v)} min={0} max={80} />
          <NumInput label="highestYExtra" value={pp.highestYExtra} onChange={v => setPP('highestYExtra', v)} min={0} max={60} />
        </div>
      )}
    </div>
  );
}

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
  const [methylationSystems, setMethylationSystems] = useState(['dam', 'dcm', 'ecoki']);
  const [methylationOverlap, setMethylationOverlap] = useState(2);
  const [openPath, setOpenPath] = useState('/Users/lidonglin/Documents/Geneie/test/pUC-GW-Amp.gb');
  const [fileStatus, setFileStatus] = useState('');

  // Primer layout parameters — all magic numbers exposed for debugging
  const [primerParams, setPrimerParams] = useState({
    fwdMatchY: 30, revMatchY: 26, misYDelta: 4,
    fwdBaseTextY: 8, revBaseTextY: 18,
    fwdLabelY: 8, revLabelY: 20,
    trackGap: 36,
    fwdAboveBase: 30, fwdAboveExtra: 23, fwdAboveNonTailExtra: 5,
    revBelowBase: 26, revBelowExtra: 25, revBelowNonTailExtra: 5,
    hoverExpand: 26, arrowHeadLen: 7, arrowHeadHeight: 5,
    highestYBase: 26, highestYFwdTail: 42, highestYFwdNoTail: 38, highestYExtra: 24,
  });
  const setPP = (key, value) => setPrimerParams(prev => ({ ...prev, [key]: value }));

  useEffect(() => {
    let ws = null;
    let cancelled = false;

    async function connect() {
      setBackendStatus('connecting');
      try {
        const needsAll = ['blunt', 'overhang5', 'overhang3', 'iis', 'rec4', 'rec5', 'rec6', 'rec8p'].includes(enzymeFilter);
        const filter = needsAll ? 'all' : enzymeFilter === 'all' ? 'all' : 'unique';
        const data = await getProject(filter);
        if (cancelled) return;
        if (data && !data.error) {
          setSequence(data.sequence || baseSeq);
          setFeatures(data.features || []);
          setEnzymes(data.enzymes || []);
          setPrimers(data.primers || []);
          setBackendStatus('online');
          // Sync methylation right away (the effect also fires via backendStatus change,
          // but this avoids a flash of un-methylated data).
          setMethylation(methylationSystems, methylationOverlap).then(() => {
            return getProject(filter);
          }).then(d2 => {
            if (d2 && !d2.error) setEnzymes(d2.enzymes || []);
          }).catch(() => {});
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

  // Sync methylation systems with backend
  useEffect(() => {
    if (backendStatus !== 'online') return;
    setMethylation(methylationSystems, methylationOverlap).then(() => {
      const needsAll = ['blunt', 'overhang5', 'overhang3', 'iis', 'rec4', 'rec5', 'rec6', 'rec8p'].includes(enzymeFilter);
      const filter = needsAll ? 'all' : enzymeFilter === 'all' ? 'all' : 'unique';
      return getProject(filter);
    }).then(data => {
      if (data && !data.error) setEnzymes(data.enzymes || []);
    }).catch(e => console.error('methylation sync error:', e));
  }, [methylationSystems, methylationOverlap, backendStatus]);

  // Refetch when enzyme filter changes (switching unique ↔ all)
  useEffect(() => {
    if (backendStatus !== 'online') return;
    const needsAll = ['blunt', 'overhang5', 'overhang3', 'iis', 'rec4', 'rec5', 'rec6', 'rec8p'].includes(enzymeFilter);
    const filter = needsAll ? 'all' : enzymeFilter === 'all' ? 'all' : 'unique';
    getProject(filter).then(data => {
      if (data && !data.error) setEnzymes(data.enzymes || []);
    }).catch(() => {});
  }, [enzymeFilter]);

  // Enzyme filter logic
  const displayEnzymes = (() => {
    const all = enzymes || [];
    if (enzymeFilter === 'all') return all;
    if (enzymeFilter === 'unique') return all.filter(e => e.isUnique);
    // Unique 6-cutters
    if (enzymeFilter === 'unique6') return all.filter(e => e.isUnique && e.recSeq?.length === 6);
    // By overhang type (use stored cutType from database, fall back to computed)
    const cutType = e => e.cutType || (e.botCutIndex - e.cutIndex === 0 ? 'blunt' : e.botCutIndex - e.cutIndex > 0 ? '5overhang' : '3overhang');
    if (enzymeFilter === 'blunt') return all.filter(e => cutType(e) === 'blunt');
    if (enzymeFilter === 'overhang5') return all.filter(e => cutType(e) === '5overhang');
    if (enzymeFilter === 'overhang3') return all.filter(e => cutType(e) === '3overhang');
    // Type IIS (both cuts fall outside recognition window)
    if (enzymeFilter === 'iis') return all.filter(e => {
      const outside = pos => pos < e.recStart || pos > e.recEnd;
      return outside(e.cutIndex) || outside(e.botCutIndex);
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
        primerParams={primerParams}
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

            {/* ── File loader ── */}
            <div>
              <div className="font-medium mb-1">Open file</div>
              <input className="w-full text-xs border rounded p-1 mb-1 font-mono" type="text"
                value={openPath} onChange={e => setOpenPath(e.target.value)}
                placeholder="/path/to/file.gbk" />
              <button className="w-full text-xs bg-gray-800 text-white rounded p-1 hover:bg-gray-700"
                onClick={async () => {
                  setFileStatus('loading...');
                  try {
                    await openFile(openPath);
                    // Fetch project data as fallback (WebSocket may also deliver)
                    const needsAll = ['blunt', 'overhang5', 'overhang3', 'iis', 'rec4', 'rec5', 'rec6', 'rec8p'].includes(enzymeFilter);
                    const filter = needsAll ? 'all' : enzymeFilter === 'all' ? 'all' : 'unique';
                    const data = await getProject(filter);
                    if (data && !data.error) {
                      setSequence(data.sequence || baseSeq);
                      setFeatures(data.features || []);
                      setEnzymes(data.enzymes || []);
                      setPrimers(data.primers || []);
                    }
                    setFileStatus('ok');
                  } catch (e) {
                    setFileStatus('error: ' + e.message);
                  }
                }}>Open</button>
              {fileStatus && <div className="text-xs mt-1 text-gray-500">{fileStatus}</div>}
              <div className="flex flex-wrap gap-1 mt-1">
                {['test/pUC-GW-Amp.gb', 'test/flySWARM.dna'].map(f => (
                  <button key={f} className="text-xs bg-gray-100 rounded px-1 hover:bg-gray-200"
                    onClick={() => setOpenPath('/Users/lidonglin/Documents/Geneie/' + f)}>{f}</button>
                ))}
              </div>
            </div>

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

            {/* ── Methylation ── */}
            <div>
              <div className="font-medium mb-1">Methylation</div>
              <label className="flex items-center gap-1 text-xs mb-1"><input type="checkbox" checked={methylationSystems.includes('dam')}
                onChange={e => setMethylationSystems(e.target.checked ? [...methylationSystems, 'dam'] : methylationSystems.filter(s => s !== 'dam'))} /> Dam</label>
              <label className="flex items-center gap-1 text-xs mb-1"><input type="checkbox" checked={methylationSystems.includes('dcm')}
                onChange={e => setMethylationSystems(e.target.checked ? [...methylationSystems, 'dcm'] : methylationSystems.filter(s => s !== 'dcm'))} /> Dcm</label>
              <label className="flex items-center gap-1 text-xs mb-1"><input type="checkbox" checked={methylationSystems.includes('ecoki')}
                onChange={e => setMethylationSystems(e.target.checked ? [...methylationSystems, 'ecoki'] : methylationSystems.filter(s => s !== 'ecoki'))} /> EcoKI</label>
              <label className="flex items-center gap-1 text-xs">Overlap: <input type="number" min="0" max="10" className="w-10 border rounded p-0.5 text-xs"
                value={methylationOverlap} onChange={e => setMethylationOverlap(Number(e.target.value))} /> bp</label>
            </div>

            {/* ── Primer Layout ── */}
            <PrimerLayoutDebug primerParams={primerParams} setPP={setPP} />
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