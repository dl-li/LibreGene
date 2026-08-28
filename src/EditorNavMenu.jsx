import { useState, useRef, useEffect, useCallback } from 'react';
import {
  Pencil,
  Tag,
  ArrowRight,
  Scissors,
  Search,
  X,
  Undo2,
  Redo2,
  CopyPlus,
  CopyMinus,
  CopyX,
  ClipboardPaste,
  CaseUpper,
  CaseLower,
  Plus,
  ChevronUp,
  ChevronDown,
  ChartNoAxesGantt,
  FileUp,
  FileDown,
  Type,
  ListChecks,
  Save,
  Database,
  ArrowDownWideNarrow,
  ScanSearch,
  AudioWaveform,
} from 'lucide-react';
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuCheckboxItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubTrigger,
  DropdownMenuSubContent,
  DropdownMenuShortcut,
} from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';

const ENZYME_FILTER_OPTIONS = [
  { value: 'all', label: 'All Enzymes' },
  { value: 'unique+twice', label: 'Highlighted Cutters' },
  { value: 'unique', label: 'Unique Cutters' },
  { value: 'unique6', label: 'Unique 6 bp' },
  { value: 'twice', label: 'Twice-cutter' },
  { value: 'blunt', label: 'Blunt', className: 'bg-amber-800/10' },
  { value: 'overhang5', label: "5' Overhang" },
  { value: 'overhang3', label: "3' Overhang" },
  { value: 'iis', label: 'Type IIS', className: 'bg-teal-700/10' },
  { value: 'rec4', label: '4 bp Recognition' },
  { value: 'rec5', label: '5 bp Recognition' },
  { value: 'rec6', label: '6 bp Recognition' },
  { value: 'rec8p', label: '≥8 bp Recognition' },
];

function NavTrigger({ icon: Icon, label }) {
  return (
    <DropdownMenuTrigger asChild>
      <button
        className="flex items-center gap-1.5 rounded-full px-3 py-1.5 text-sm text-foreground/80 outline-none transition-all duration-150 hover:bg-accent hover:text-foreground active:scale-95 data-[state=open]:bg-accent data-[state=open]:text-foreground"
        type="button"
      >
        <Icon className="size-4" />
        <span>{label}</span>
      </button>
    </DropdownMenuTrigger>
  );
}

export default function EditorNavMenu({
  onSave,
  onSaveAs,
  canDirectSave = true,
  canUndo,
  canRedo,
  onUndo,
  onRedo,
  hasSelection,
  hasTextSelection,
  canPaste,
  onCopySense,
  onCopyAntisense,
  onCopyTranslation,
  onPaste,
  hasTranslationSelection,
  onToUppercase,
  onToLowercase,
  showFeatures,
  onToggleFeatures,
  showOrfs,
  onToggleOrfs,
  onCreateFeature,
  onOpenDetectFeatures,
  showPrimers,
  onTogglePrimers,
  onCreatePrimer,
  showEnzymes,
  onToggleEnzymes,
  enzymeFilter,
  onEnzymeFilterChange,
  onSearch,
  searchNav,
  openSearchRef,
  alignments = [],
  alignmentEnabled = true,
  showAlignments = true,
  onToggleAlignments,
  hiddenAlignIds = [],
  onToggleAlignmentVisible,
  onAddAlignmentFile,
  onAddAlignmentText,
  onManageAlignments,
  onOpenRnaFold,
  onPrimerDesign,
  primerDesignEnabled = true,
  onOpenPrimerOverview,
  onOpenMyPrimers,
  onAddCurrentPrimerToMyPrimers,
  onAddAllPrimersToMyPrimers,
  autoAddPrimers = false,
  onToggleAutoAddPrimers,
  hasSelectedPrimer = false,
  hasPrimers = false,
  onOpenMyEnzymes,
  onOpenEnzymeDatabase,
  myEnzymes = [],
  topology,
  moleculeType = 'dna',
}) {
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [searchScope, setSearchScope] = useState('all');
  const inputRef = useRef(null);

  // rna/protein projects are single-strand sequences: keep Edit/Features/Search,
  // hide DNA-only tooling (primers, enzymes, alignment, antisense/translation).
  const isDna = moleculeType === 'dna';

  const SEARCH_SCOPES = ['all', 'seq', 'feature', 'primer', 'enzyme'];
  const SCOPE_WORDS = {
    all: 'anything',
    seq: 'sequences',
    feature: 'features',
    primer: 'primers',
    enzyme: 'enzymes',
  };

  useEffect(() => {
    if (searchOpen) inputRef.current?.focus();
  }, [searchOpen]);

  useEffect(() => {
    if (!openSearchRef) return;
    openSearchRef.current = () => setSearchOpen(true);
    return () => {
      openSearchRef.current = null;
    };
  }, [openSearchRef]);

  const toggleSearch = useCallback(() => {
    setSearchOpen((v) => !v);
  }, []);

  const searchDebounceRef = useRef(null);
  useEffect(() => () => clearTimeout(searchDebounceRef.current), []);

  const onQueryChange = useCallback(
    (e) => {
      const q = e.target.value;
      setQuery(q);
      // Debounce: scanning the whole sequence on every keystroke is wasteful
      clearTimeout(searchDebounceRef.current);
      searchDebounceRef.current = setTimeout(() => onSearch?.(q, 'reset', searchScope), 200);
    },
    [onSearch, searchScope],
  );

  const onSearchKeyDown = useCallback(
    (e) => {
      // Drop any pending debounce so Enter/Tab navigation isn't clobbered by a
      // stale timer (which would reset the nav index / use the old scope).
      clearTimeout(searchDebounceRef.current);
      if (e.key === 'Enter') {
        e.preventDefault();
        if (query) onSearch?.(query, e.shiftKey ? 'prev' : 'next', searchScope);
      } else if (e.key === 'Tab') {
        e.preventDefault();
        const next = SEARCH_SCOPES[(SEARCH_SCOPES.indexOf(searchScope) + 1) % SEARCH_SCOPES.length];
        setSearchScope(next);
        if (query) onSearch?.(query, 'reset', next);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        setSearchOpen(false);
      }
    },
    [query, onSearch, searchScope],
  );

  const navTotal = searchNav && searchNav.query === query ? searchNav.total : 0;
  const navIndex = searchNav && searchNav.query === query ? searchNav.index : -1;
  // 1-2 base nucleotide queries are rejected by findSeqMatches (min length 3);
  // surface a hint instead of silently showing "0/0".
  const shortNucQuery = /^[ACGTURYSWKMBDHVN]{1,2}$/.test(query?.trim().toUpperCase() || '');

  return (
    <div
      style={{
        position: 'fixed',
        bottom: 16,
        left: '50%',
        transform: 'translateX(-50%)',
        zIndex: 40,
      }}
    >
      <div className="nav-bar-enter relative flex items-center gap-0.5 rounded-full border bg-background/80 px-2 py-1.5 shadow-lg backdrop-blur-md">
        {/* Edit */}
        <DropdownMenu modal={false}>
          <NavTrigger icon={Pencil} label="Edit" />
          <DropdownMenuContent side="top" align="center" className="min-w-52 overflow-visible">
            <DropdownMenuItem disabled={!canDirectSave} onSelect={onSave}>
              <Save /> Save
              <DropdownMenuShortcut>⌘S</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={onSaveAs}>
              <FileDown /> Save As…
              <DropdownMenuShortcut>⇧⌘S</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem disabled={!canUndo} onSelect={onUndo}>
              <Undo2 /> Undo
              <DropdownMenuShortcut>⌘Z</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuItem disabled={!canRedo} onSelect={onRedo}>
              <Redo2 /> Redo
              <DropdownMenuShortcut>⇧⌘Z</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              disabled={!hasSelection && !hasTranslationSelection}
              onSelect={onCopySense}
            >
              <CopyPlus /> Copy (+) Strand
            </DropdownMenuItem>
            {isDna && (
              <DropdownMenuItem
                disabled={!hasSelection && !hasTranslationSelection}
                onSelect={onCopyAntisense}
              >
                <CopyMinus /> Copy (−) Strand
              </DropdownMenuItem>
            )}
            {isDna && (
              <DropdownMenuItem disabled={!hasTranslationSelection} onSelect={onCopyTranslation}>
                <CopyX /> Copy Translation
              </DropdownMenuItem>
            )}
            <DropdownMenuItem disabled={!canPaste} onSelect={onPaste}>
              <ClipboardPaste /> Paste
              <DropdownMenuShortcut>⌘V</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem disabled={!hasTextSelection} onSelect={onToUppercase}>
              <CaseUpper /> To Uppercase
            </DropdownMenuItem>
            <DropdownMenuItem disabled={!hasTextSelection} onSelect={onToLowercase}>
              <CaseLower /> To Lowercase
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>

        {/* Features */}
        <DropdownMenu modal={false}>
          <NavTrigger icon={Tag} label="Features" />
          <DropdownMenuContent side="top" align="center" className="min-w-52 overflow-visible">
            <DropdownMenuCheckboxItem checked={showFeatures} onCheckedChange={onToggleFeatures}>
              Show Features
            </DropdownMenuCheckboxItem>
            {onToggleOrfs && (
              <DropdownMenuCheckboxItem checked={showOrfs} onCheckedChange={onToggleOrfs}>
                Show ORFs
              </DropdownMenuCheckboxItem>
            )}
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={onCreateFeature}>
              <Plus /> Create Feature
              <DropdownMenuShortcut>⌘T</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={onOpenDetectFeatures} disabled={!onOpenDetectFeatures}>
              <ScanSearch /> Detect Common Features
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>

        {/* Primers (DNA only) */}
        {isDna && (
          <DropdownMenu modal={false}>
            <NavTrigger icon={ArrowRight} label="Primers" />
            <DropdownMenuContent side="top" align="center" className="min-w-52 overflow-visible">
              <DropdownMenuCheckboxItem checked={showPrimers} onCheckedChange={onTogglePrimers}>
                Show Primers
              </DropdownMenuCheckboxItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={onCreatePrimer}>
                <Plus /> Create Primer
                <DropdownMenuShortcut>⌘R</DropdownMenuShortcut>
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={onOpenPrimerOverview}>
                <ArrowDownWideNarrow /> Primer Overview
              </DropdownMenuItem>
              <DropdownMenuSub>
                <DropdownMenuSubTrigger inset>My Primer Collection</DropdownMenuSubTrigger>
                <DropdownMenuSubContent className="min-w-48">
                  <DropdownMenuItem onSelect={onOpenMyPrimers}>
                    <ListChecks /> My Primer Collection…
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem
                    disabled={!hasSelectedPrimer}
                    onSelect={onAddCurrentPrimerToMyPrimers}
                  >
                    <Plus /> Add Current Primer
                  </DropdownMenuItem>
                  <DropdownMenuItem disabled={!hasPrimers} onSelect={onAddAllPrimersToMyPrimers}>
                    <CopyPlus /> Add All from This File
                  </DropdownMenuItem>
                  <DropdownMenuCheckboxItem
                    checked={autoAddPrimers}
                    onCheckedChange={onToggleAutoAddPrimers}
                  >
                    Auto-add from Opened Files
                  </DropdownMenuCheckboxItem>
                </DropdownMenuSubContent>
              </DropdownMenuSub>
              {primerDesignEnabled && (
                <DropdownMenuSub>
                  <DropdownMenuSubTrigger inset>Primer Design</DropdownMenuSubTrigger>
                  <DropdownMenuSubContent className="min-w-44">
                    <DropdownMenuItem onSelect={() => onPrimerDesign?.('amplify')}>
                      Amplify Fragment
                    </DropdownMenuItem>
                    <DropdownMenuItem onSelect={() => onPrimerDesign?.('oepcr')}>
                      OE-PCR
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      disabled={topology !== 'circular'}
                      onSelect={() => onPrimerDesign?.('mutagenesis')}
                    >
                      PCR Mutagenesis
                    </DropdownMenuItem>
                  </DropdownMenuSubContent>
                </DropdownMenuSub>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        )}

        {/* Enzymes (DNA only) */}
        {isDna && (
          <DropdownMenu modal={false}>
            <NavTrigger icon={Scissors} label="Enzymes" />
            <DropdownMenuContent side="top" align="center" className="min-w-52 overflow-visible">
              <DropdownMenuCheckboxItem checked={showEnzymes} onCheckedChange={onToggleEnzymes}>
                Show Enzyme Sites
              </DropdownMenuCheckboxItem>
              <DropdownMenuSeparator />
              <DropdownMenuSub>
                <DropdownMenuSubTrigger inset>Choose Enzyme Set</DropdownMenuSubTrigger>
                <DropdownMenuSubContent className="min-w-44">
                  <DropdownMenuRadioGroup value={enzymeFilter} onValueChange={onEnzymeFilterChange}>
                    {ENZYME_FILTER_OPTIONS.map((opt) => (
                      <DropdownMenuRadioItem
                        key={opt.value}
                        value={opt.value}
                        className={opt.className}
                      >
                        {opt.value === 'unique' ? (
                          <span>
                            <strong>Unique</strong> Cutters
                          </span>
                        ) : opt.value === 'unique6' ? (
                          <span>
                            <strong>Unique</strong> 6 bp
                          </span>
                        ) : opt.value === 'twice' ? (
                          <span>
                            Twice-cutter<sup>²</sup>
                          </span>
                        ) : opt.value === 'unique+twice' ? (
                          <span>
                            <strong>Unique</strong> + Twice-cutter<sup>²</sup>
                          </span>
                        ) : (
                          opt.label
                        )}
                      </DropdownMenuRadioItem>
                    ))}
                    {myEnzymes.length > 0 && (
                      <>
                        <DropdownMenuSeparator />
                        <DropdownMenuRadioItem value="myEnzymes">My Enzymes</DropdownMenuRadioItem>
                      </>
                    )}
                  </DropdownMenuRadioGroup>
                </DropdownMenuSubContent>
              </DropdownMenuSub>
              <DropdownMenuItem onSelect={onOpenMyEnzymes}>
                <Scissors /> My Enzymes…
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onOpenEnzymeDatabase}>
                <Database /> Enzyme Database…
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )}

        {/* Alignment (DNA only) */}
        {isDna && alignmentEnabled && (
          <DropdownMenu modal={false}>
            <NavTrigger icon={ChartNoAxesGantt} label="Align" />
            <DropdownMenuContent side="top" align="center" className="min-w-56 overflow-visible">
              <DropdownMenuCheckboxItem
                checked={showAlignments}
                onCheckedChange={onToggleAlignments}
              >
                Show Alignments
              </DropdownMenuCheckboxItem>
              {alignments.length > 0 && <DropdownMenuSeparator />}
              {alignments.map((a) => (
                <DropdownMenuCheckboxItem
                  key={a.id}
                  checked={!hiddenAlignIds.includes(a.id)}
                  onCheckedChange={() => onToggleAlignmentVisible?.(a.id)}
                >
                  <span className="truncate">{a.name}</span>
                  <DropdownMenuShortcut className="ml-4 text-muted-foreground">
                    {a.identity != null ? `${(a.identity * 100).toFixed(0)}%` : ''}
                  </DropdownMenuShortcut>
                </DropdownMenuCheckboxItem>
              ))}
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={onAddAlignmentFile}>
                <FileUp /> Add from File…
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onAddAlignmentText}>
                <Type /> Add from Text…
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onManageAlignments}>
                <ListChecks /> Manage Alignments…
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )}

        {/* RNA Folding: top-level button, RNA projects only (plugin can be disabled) */}
        {moleculeType === 'rna' && onOpenRnaFold && (
          <button
            type="button"
            onClick={onOpenRnaFold}
            title="Predict RNA secondary structure"
            className="flex items-center gap-1.5 rounded-full px-3 py-1.5 text-sm text-foreground/80 outline-none transition-all duration-150 hover:bg-accent hover:text-foreground active:scale-95"
          >
            <AudioWaveform className="size-4" />
            <span>Folding</span>
          </button>
        )}

        {/* Search: icon stays in flow; expanding overlay covers the other buttons */}
        <button
          type="button"
          onClick={toggleSearch}
          className={cn(
            'flex items-center justify-center rounded-full p-2 text-foreground/80 outline-none transition-all duration-150 hover:bg-accent hover:text-foreground active:scale-90',
            searchOpen && 'bg-accent text-foreground',
          )}
        >
          <Search className="size-4" />
        </button>
        <div
          className={cn(
            'absolute inset-0 z-10 flex items-center gap-0.5 rounded-full bg-background/80 px-2 py-1.5 backdrop-blur-md transition-[clip-path] duration-300 ease-[cubic-bezier(0.34,1.3,0.64,1)]',
            !searchOpen && 'pointer-events-none',
          )}
          style={{
            clipPath: searchOpen ? 'inset(0 0 0 0 round 999px)' : 'inset(0 0 0 100% round 999px)',
          }}
        >
          <input
            ref={inputRef}
            value={query}
            onChange={onQueryChange}
            onKeyDown={onSearchKeyDown}
            placeholder={`Search ${SCOPE_WORDS[searchScope]} (Tab to switch)`}
            className="min-w-0 flex-1 rounded-full bg-muted/60 px-3 py-1.5 text-sm outline-none placeholder:text-muted-foreground"
            style={{ border: 'none' }}
            tabIndex={searchOpen ? 0 : -1}
          />
          {query && (
            <span className="mr-0.5 flex items-center gap-0.5 text-xs text-muted-foreground tabular-nums">
              {shortNucQuery ? (
                'Enter at least 3 bases'
              ) : navTotal > 0 ? (
                `${navIndex + 1}/${navTotal}`
              ) : (
                '0/0'
              )}
              <button
                type="button"
                aria-label="Previous match"
                onClick={() => onSearch?.(query, 'prev', searchScope)}
                className="rounded-full p-1 transition-colors hover:bg-accent hover:text-foreground"
              >
                <ChevronUp className="size-3.5" />
              </button>
              <button
                type="button"
                aria-label="Next match"
                onClick={() => onSearch?.(query, 'next', searchScope)}
                className="rounded-full p-1 transition-colors hover:bg-accent hover:text-foreground"
              >
                <ChevronDown className="size-3.5" />
              </button>
            </span>
          )}
          <button
            type="button"
            aria-label="Close search"
            onClick={toggleSearch}
            className="flex items-center justify-center rounded-full bg-accent p-2 text-foreground outline-none transition-all duration-150 hover:text-foreground active:scale-90"
          >
            <X className="size-4" />
          </button>
        </div>
      </div>
    </div>
  );
}
