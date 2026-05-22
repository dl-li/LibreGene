import React, { useState, useEffect, useRef, useCallback, useMemo, startTransition } from 'react';
import SequenceEditor from './SequenceEditor';
import { getProject, openFile, setMethylation, isTauri, openFileDialog, listenProjectUpdates, getProjects, activateProject } from './tauriApi';
import DebugPanel from './components/DebugPanel';
import { SidebarProvider, Sidebar, SidebarContent, SidebarGroup, SidebarGroupContent, SidebarGroupLabel, SidebarHeader, SidebarMenu, SidebarMenuButton, SidebarMenuItem } from '@/components/ui/sidebar';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Empty, EmptyContent, EmptyDescription, EmptyMedia, EmptyTitle } from '@/components/ui/empty';
import { Button } from '@/components/ui/button';
import { TooltipProvider } from '@/components/ui/tooltip';
import { Dna, FolderOpen, ChevronDown } from 'lucide-react';
import { getFileIcon } from './fileIcons';

const EMPTY_ARRAY = [];

export default function App() {
  const [sequence, setSequence] = useState(null);
  const [features, setFeatures] = useState(EMPTY_ARRAY);
  const [enzymes, setEnzymes] = useState(EMPTY_ARRAY);
  const [primers, setPrimers] = useState(EMPTY_ARRAY);
  const [backendStatus, setBackendStatus] = useState(isTauri ? 'online' : 'offline');

  // Multi-project state
  const [projects, setProjects] = useState([]);
  const [activeId, setActiveId] = useState(null);

  // Debug toggles
  const [debugOpen, setDebugOpen] = useState(false);
  const [showFeatures, setShowFeatures] = useState(true);
  const [showPrimers, setShowPrimers] = useState(true);
  const [enzymeFilter, setEnzymeFilter] = useState('unique');
  const [methylationSystems, setMethylationSystems] = useState(['dam', 'dcm', 'ecoki']);
  const [methylationOverlap, setMethylationOverlap] = useState(2);
  const [openPath, setOpenPath] = useState('/Users/lidonglin/Documents/Geneie/test/pUC-GW-Amp.gb');
  const [fileStatus, setFileStatus] = useState('');
  const sequenceRef = useRef(sequence);
  const projectCacheRef = useRef({}); // { [id]: { sequence, features, enzymes, primers, methKey } }
  const switchGenRef = useRef(0);      // generation counter to cancel stale async responses

  // Cache methylation settings key — used to detect stale cache entries
  const methKey = useMemo(() => methylationSystems.join(',') + '|' + methylationOverlap, [methylationSystems, methylationOverlap]);
  const methKeyRef = useRef(methKey);
  useEffect(() => { methKeyRef.current = methKey; }, [methKey]);

  // Keep cache in sync with latest state (captures post-methylation enzymes)
  useEffect(() => {
    if (activeId && sequence) {
      projectCacheRef.current[activeId] = { sequence, features, enzymes, primers, methKey };
    }
  }, [activeId, sequence, features, enzymes, primers, methKey]);

  // Layout parameters (行距 / 特征 / 引物排版)
  const [layoutParams, setLayoutParams] = useState({
    // 引物排版
    fwdMatchY: 25, revMatchY: 15, misYDelta: 4,
    fwdBaseTextY: 8, revBaseTextY: 18,
    fwdLabelY: 8, revLabelY: 20,
    trackGap: 36,
    fwdAboveBase: 30, fwdAboveExtra: 23, fwdAboveNonTailExtra: 5,
    revBelowBase: 26, revBelowExtra: 25, revBelowNonTailExtra: 5,
    hoverExpand: 26, arrowHeadLen: 7, arrowHeadHeight: 5,
    // 行距
    minRowGap: 27,
    rowContentGap: 25,
    minAboveSpace: 36,
    minBelowSpace: 0,
    // 特征排版
    featTrackHeight: 18,
    featBaseOffset: 14,
    featLabelPad: 12,
    // 酶切位点排版
    enzTrackHeight: 18,
    enzLineGap: 18,
    enzLabelBase: 51,
    enzAbovePad: 10,
  });
  const setLP = (key, value) => setLayoutParams(prev => ({ ...prev, [key]: value }));

  const editorFeatures = useMemo(() => showFeatures ? features : EMPTY_ARRAY, [showFeatures, features]);
  const editorPrimers = useMemo(() => showPrimers ? primers : EMPTY_ARRAY, [showPrimers, primers]);
  const editorLayoutParams = useMemo(() => layoutParams, [layoutParams]);

  const hasProject = sequence !== null && sequence.length > 0;

  // Sidebar hover state
  const [sidebarHover, setSidebarHover] = useState(false);
  const sidebarLeaveRef = useRef(null);

  const handleSidebarEnter = useCallback(() => {
    clearTimeout(sidebarLeaveRef.current);
    setSidebarHover(true);
  }, []);

  const handleSidebarLeave = useCallback(() => {
    sidebarLeaveRef.current = setTimeout(() => setSidebarHover(false), 250);
  }, []);

  // Sync project list from backend
  const refreshProjects = useCallback(async () => {
    try {
      const data = await getProjects();
      if (data && !data.error) {
        setProjects(data.projects || []);
        setActiveId(data.activeId || null);
      }
    } catch {}
  }, []);

  useEffect(() => {
    let listener = null;
    let cancelled = false;

    async function connect() {
      setBackendStatus(isTauri ? 'online' : 'connecting');
      try {
        const data = await getProject('all');
        if (cancelled) return;
        if (data && !data.error && data.sequence) {
          setSequence(data.sequence);
          setFeatures(data.features || []);
          setEnzymes(data.enzymes || []);
          setPrimers(data.primers || []);
          setBackendStatus('online');
        }
        await refreshProjects();
      } catch {
        if (!cancelled) setBackendStatus(isTauri ? 'online' : 'offline');
      }
    }

    connect();

    listener = listenProjectUpdates((msg) => {
      if (cancelled) return;
      if (msg.type === 'project' && msg.data) {
        const newSeq = msg.data.sequence;
        if (newSeq) {
          if (msg.activeId) {
            projectCacheRef.current[msg.activeId] = {
              sequence: newSeq,
              features: msg.data.features || EMPTY_ARRAY,
              enzymes: msg.data.enzymes || EMPTY_ARRAY,
              primers: msg.data.primers || EMPTY_ARRAY,
              methKey: methKeyRef.current,
            };
          }
          startTransition(() => {
            setSequence(newSeq);
            setFeatures(msg.data.features || EMPTY_ARRAY);
            setEnzymes(msg.data.enzymes || EMPTY_ARRAY);
            setPrimers(msg.data.primers || EMPTY_ARRAY);
          });
        }
      }
      if (msg.projects) setProjects(msg.projects);
      if (msg.activeId !== undefined) setActiveId(msg.activeId);
    });

    return () => { cancelled = true; if (listener) listener.close(); };
  }, []);

  // Sync methylation systems with backend, then fetch updated enzymes.
  // Only fires when methylation settings change (NOT on every project load).
  const syncMethylation = useCallback(async () => {
    if (backendStatus !== 'online') return;
    try {
      await setMethylation(methylationSystems, methylationOverlap);
      const data = await getProject('all');
      if (data && !data.error) setEnzymes(data.enzymes || []);
    } catch (e) {
      console.error('methylation sync error:', e);
    }
  }, [methylationSystems, methylationOverlap, backendStatus]);

  // Re-sync enzymes when methylation settings change
  useEffect(() => {
    if (backendStatus !== 'online' || !sequence) return;
    syncMethylation();
  }, [methylationSystems, methylationOverlap]);

  useEffect(() => { sequenceRef.current = sequence; }, [sequence]);

  const displayEnzymes = useMemo(() => {
    const all = enzymes || [];
    if (enzymeFilter === 'all') return all;
    if (enzymeFilter === 'unique') return all.filter(e => e.isUnique);
    if (enzymeFilter === 'unique6') return all.filter(e => e.isUnique && e.recSeq?.length === 6);
    const cutType = e => e.cutType || (e.botCutIndex - e.cutIndex === 0 ? 'blunt' : e.botCutIndex - e.cutIndex > 0 ? '5overhang' : '3overhang');
    if (enzymeFilter === 'blunt') return all.filter(e => cutType(e) === 'blunt');
    if (enzymeFilter === 'overhang5') return all.filter(e => cutType(e) === '5overhang');
    if (enzymeFilter === 'overhang3') return all.filter(e => cutType(e) === '3overhang');
    if (enzymeFilter === 'iis') return all.filter(e => {
      const pairs = e.cutPairs || [{ topCutIndex: e.cutIndex, botCutIndex: e.botCutIndex }];
      return pairs.some(cp => {
        const d = [cp.topCutIndex - e.recEnd, e.recStart - cp.topCutIndex, cp.botCutIndex - e.recEnd, e.recStart - cp.botCutIndex];
        return d.some(v => v >= 2);
      });
    });
    if (enzymeFilter === 'rec4') return all.filter(e => e.recSeq?.length === 4);
    if (enzymeFilter === 'rec5') return all.filter(e => e.recSeq?.length === 5);
    if (enzymeFilter === 'rec6') return all.filter(e => e.recSeq?.length === 6);
    if (enzymeFilter === 'rec8p') return all.filter(e => (e.recSeq?.length || 0) >= 8);
    return all.filter(e => e.isUnique);
  }, [enzymes, enzymeFilter]);

  const handleOpenFile = useCallback(async () => {
    let paths;
    if (isTauri) {
      paths = await openFileDialog();
      if (!paths || !paths.length) return;
    } else {
      paths = [openPath];
    }
    setFileStatus('loading...');
    try {
      let lastData = null;
      for (const p of paths) {
        const data = await openFile(p);
        if (data && data.sequence) {
          // Pre-cache every opened file immediately
          projectCacheRef.current[p] = {
            sequence: data.sequence,
            features: data.features || EMPTY_ARRAY,
            enzymes: data.enzymes || EMPTY_ARRAY,
            primers: data.primers || EMPTY_ARRAY,
            methKey,
          };
          lastData = data;
        }
      }
      await refreshProjects();

      if (lastData && lastData.sequence) {
        setSequence(lastData.sequence);
        setFeatures(lastData.features || EMPTY_ARRAY);
        setEnzymes(lastData.enzymes || EMPTY_ARRAY);
        setPrimers(lastData.primers || EMPTY_ARRAY);
        // Apply methylation to newly opened file
        syncMethylation();
      }
      setFileStatus('ok');
    } catch (e) {
      setFileStatus('error: ' + e.message);
    }
  }, [isTauri, openPath, methKey, refreshProjects, syncMethylation]);

  const handleActivateProject = useCallback(async (id) => {
    const gen = ++switchGenRef.current;

    // Instant switch from cache for perceived speed
    const cached = projectCacheRef.current[id];
    if (cached) {
      const methFresh = cached.methKey === methKey;
      setActiveId(id);
      setSequence(cached.sequence);
      setFeatures(cached.features);
      setEnzymes(cached.enzymes);
      setPrimers(cached.primers);
      if (!methFresh) {
        syncMethylation();
      }
    }

    try {
      const data = await activateProject(id);
      if (switchGenRef.current !== gen) return;

      if (data && !data.error && data.sequence) {
        if (!cached) {
          projectCacheRef.current[id] = {
            sequence: data.sequence,
            features: data.features || EMPTY_ARRAY,
            enzymes: data.enzymes || EMPTY_ARRAY,
            primers: data.primers || EMPTY_ARRAY,
            methKey,
          };
          setActiveId(id);
          setSequence(data.sequence);
          setFeatures(data.features || EMPTY_ARRAY);
          setEnzymes(data.enzymes || EMPTY_ARRAY);
          setPrimers(data.primers || EMPTY_ARRAY);
          syncMethylation();
        }

        if (data.projects) setProjects(data.projects);
      }
    } catch (e) {
      console.error('activate project error:', e);
    }
  }, [methKey, syncMethylation]);

  // Extract filename from path
  const fileName = (p) => {
    const s = (p.name || p.id || '').replace(/\\/g, '/');
    return s.split('/').pop() || s;
  };

  // Collapsible "Open Files" group
  const [filesOpen, setFilesOpen] = useState(true);

  const sidebarContent = (
    <Sidebar collapsible="icon" variant="sidebar" className="transition-[width] duration-300 ease-out">
      <SidebarHeader className="flex flex-row items-center gap-2 px-3 py-2">
        <Dna className="size-5 shrink-0 text-primary" />
        <span className="font-semibold text-sm group-data-[state=collapsed]:hidden">Geneie</span>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton onClick={handleOpenFile}>
                  <FolderOpen className="size-4" />
                  <span>Open File</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
        {projects.length > 0 && (
          <Collapsible open={filesOpen} onOpenChange={setFilesOpen}>
            <SidebarGroup>
              <CollapsibleTrigger asChild>
                <SidebarGroupLabel className="cursor-pointer">
                  Open Files
                  <ChevronDown className={`ml-auto size-4 shrink-0 transition-transform ${filesOpen ? 'rotate-0' : '-rotate-90'}`} />
                </SidebarGroupLabel>
              </CollapsibleTrigger>
              <CollapsibleContent>
                <SidebarGroupContent>
                  <SidebarMenu>
                    {projects.map(p => (
                      <SidebarMenuItem key={p.id}>
                        <SidebarMenuButton
                          onClick={() => handleActivateProject(p.id)}
                          isActive={p.id === activeId}
                        >
                          {(() => { const Icon = getFileIcon(fileName(p)); return <Icon className="size-4 shrink-0" />; })()}
                          <span className="truncate">{fileName(p)}</span>
                        </SidebarMenuButton>
                      </SidebarMenuItem>
                    ))}
                  </SidebarMenu>
                </SidebarGroupContent>
              </CollapsibleContent>
            </SidebarGroup>
          </Collapsible>
        )}
      </SidebarContent>
    </Sidebar>
  );

  return (
    <TooltipProvider>
      <SidebarProvider
        open={sidebarHover}
        style={{ "--sidebar-width": "12rem" }}
      >
        <div className="relative min-h-screen w-full bg-[#fdfbf7]">
          {hasProject && (
            <div
              className="absolute left-0 top-0 bottom-0 z-40"
              onMouseEnter={handleSidebarEnter}
              onMouseLeave={handleSidebarLeave}
            >
              {sidebarContent}
            </div>
          )}
          <main className="w-full transition-[padding] duration-300 ease-out">
            {hasProject ? (
              <SequenceEditor
                sequence={sequence}
                features={editorFeatures}
                enzymes={displayEnzymes}
                primers={editorPrimers}
                charsPerLine={60}
                layoutParams={editorLayoutParams}
              />
            ) : (
              <Empty className="min-h-screen">
                <EmptyMedia variant="icon">
                  <Dna className="size-6" />
                </EmptyMedia>
                <EmptyTitle>No file opened</EmptyTitle>
                <EmptyDescription>
                  Open a GenBank (.gbk), SnapGene (.dna), or FASTA (.fasta) file to get started.
                </EmptyDescription>
                <EmptyContent>
                  <Button onClick={handleOpenFile}>
                    <FolderOpen className="size-4 mr-2" />
                    Open File
                  </Button>
                </EmptyContent>
              </Empty>
            )}
          </main>
        </div>

        <DebugPanel
          open={debugOpen}
          onOpenChange={setDebugOpen}
          showFeatures={showFeatures} setShowFeatures={setShowFeatures}
          features={features}
          showPrimers={showPrimers} setShowPrimers={setShowPrimers}
          primers={primers}
          enzymeFilter={enzymeFilter} setEnzymeFilter={setEnzymeFilter}
          enzymes={enzymes} displayEnzymes={displayEnzymes}
          methylationSystems={methylationSystems} setMethylationSystems={setMethylationSystems}
          methylationOverlap={methylationOverlap} setMethylationOverlap={setMethylationOverlap}
          isTauri={isTauri} openPath={openPath} setOpenPath={setOpenPath}
          fileStatus={fileStatus}
          layoutParams={layoutParams} setLP={setLP}
          onOpenFile={handleOpenFile}
        />

        <button
          className="fixed bottom-3 right-3 w-3.5 h-3.5 rounded-full z-50 opacity-60 hover:opacity-100 hover:scale-125 transition-all cursor-pointer border-0"
          style={{ background: backendStatus === 'online' || isTauri ? '#22c55e' : backendStatus === 'connecting' ? '#f59e0b' : '#9ca3af' }}
          onClick={() => setDebugOpen(true)}
          title={isTauri ? 'Desktop mode — Click for debug' : backendStatus === 'online' ? 'Backend connected — Click for debug' : backendStatus === 'connecting' ? 'Connecting...' : 'Offline — Click for debug'}
        />
      </SidebarProvider>
    </TooltipProvider>
  );
}
