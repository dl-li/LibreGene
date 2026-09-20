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
  History,
  Map as MapIcon,
  Circle,
  Minus,
  Triangle,
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
import { ENZYME_PROVIDER_OPTIONS } from './enzymeProviders';

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

const NAV_BUTTON_CLASS =
  'flex items-center gap-1.5 rounded-full px-3 py-1.5 text-sm text-foreground/80 outline-none transition-all duration-150 hover:bg-accent hover:text-foreground active:scale-95 data-[state=open]:bg-accent data-[state=open]:text-foreground';

// Left click runs onLeftClick (when given) and right click opens the menu;
// without onLeftClick both left and right click open the menu. Radix opens
// the menu on left pointerdown by default — preventDefault() there opts out.
function NavMenu({ icon: Icon, label, onLeftClick, contentClassName, children }) {
  const [open, setOpen] = useState(false);
  const pressWasOpenRef = useRef(false);
  return (
    <DropdownMenu modal={false} open={open} onOpenChange={setOpen}>
      <DropdownMenuTrigger asChild>
        <button
          className={cn(NAV_BUTTON_CLASS, 'group')}
          type="button"
          onPointerDown={(e) => {
            if (e.button === 0 && onLeftClick) e.preventDefault();
          }}
          onClick={() => onLeftClick?.()}
          onContextMenu={(e) => {
            e.preventDefault();
            setOpen(true);
          }}
        >
          <span className="relative -ml-1.5 flex items-center justify-center">
            <Icon
              className={cn(
                'size-4 transition-opacity',
                open ? 'opacity-0' : 'group-hover:opacity-0',
              )}
            />
            {/* Menu-affordance disc: appears on button hover (hollow caret),
                solid caret on disc hover, flipped while the menu is open. */}
            <span
              role="button"
              tabIndex={-1}
              aria-label={`Open ${label} menu`}
              className={cn(
                'group/caret absolute -inset-1 flex cursor-pointer items-center justify-center rounded-full text-teal-700 transition-opacity dark:text-teal-400',
                open ? 'opacity-100' : 'opacity-0 group-hover:opacity-100',
              )}
              onPointerDown={(e) => {
                pressWasOpenRef.current = open;
                e.stopPropagation();
              }}
              onClick={(e) => {
                e.stopPropagation();
                // Radix closes the open menu on this same pointerdown (capture
                // phase, unstoppable) — don't reopen it on the following click.
                if (!pressWasOpenRef.current) setOpen(true);
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                setOpen(true);
              }}
            >
              <Triangle
                strokeWidth={3}
                className={cn(
                  'size-3 transition-transform duration-150 group-hover/caret:hidden',
                  open && 'rotate-180',
                )}
              />
              <Triangle
                strokeWidth={3}
                className={cn(
                  'hidden size-3 fill-current transition-transform duration-150 group-hover/caret:block',
                  open && 'rotate-180',
                )}
              />
            </span>
          </span>
          <span>{label}</span>
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="center" className={contentClassName}>
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
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
  enzymeProvider = 'all',
  onEnzymeProviderChange,
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
  onOpenMapView,
  background = 'none',
  backgroundOptions = [],
  onBackgroundChange,
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
  onToggleTopology,
  moleculeType = 'dna',
  onOpenSnapshots,
}) {
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [searchScope, setSearchScope] = useState('all');
  const inputRef = useRef(null);

  // rna/protein projects are single-strand sequences: keep Edit/Features/Search,
  // hide DNA-only tooling (primers, enzymes, alignment, antisense/translation).
  const isDna = moleculeType === 'dna';

  // Left-click toggles "Show As Background" when that background is offered
  // for this molecule type (protein has none → fall back to the examine dialog).
  const bgToggle = (value) =>
    onBackgroundChange && backgroundOptions.some((o) => o.value === value)
      ? () => onBackgroundChange(background === value ? 'none' : value)
      : null;
  const mapBackgroundToggle = bgToggle('map');
  const foldingBackgroundToggle = bgToggle('folding');

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

  // Close the search bar with Esc even when focus has left the input.
  useEffect(() => {
    if (!searchOpen) return undefined;
    const onKey = (e) => {
      if (e.key !== 'Escape') return;
      if (e.target === inputRef.current) return; // handled by onSearchKeyDown
      e.preventDefault();
      setSearchOpen(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
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
        {/* Edit: left or right click opens the menu */}
        <NavMenu icon={Pencil} label="Edit" contentClassName="min-w-52 overflow-visible">
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
          {isDna && onToggleTopology && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={onToggleTopology}>
                {topology === 'circular' ? (
                  <>
                    <Minus /> Linearize
                  </>
                ) : (
                  <>
                    <Circle /> Circularize
                  </>
                )}
              </DropdownMenuItem>
            </>
          )}
          {/* SnapGene history snapshots (project from a .dna file) */}
          {onOpenSnapshots && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={onOpenSnapshots}>
                <History /> History
              </DropdownMenuItem>
            </>
          )}
        </NavMenu>

        {/* Features: left click toggles visibility, right click opens menu */}
        <NavMenu
          icon={Tag}
          label="Features"
          onLeftClick={onToggleFeatures}
          contentClassName="min-w-52 overflow-visible"
        >
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
        </NavMenu>

        {/* Primers (DNA only): left click toggles visibility, right click opens menu */}
        {isDna && (
          <NavMenu
            icon={ArrowRight}
            label="Primers"
            onLeftClick={onTogglePrimers}
            contentClassName="min-w-52 overflow-visible"
          >
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
          </NavMenu>
        )}

        {/* Enzymes (DNA only): left click toggles visibility, right click opens menu */}
        {isDna && (
          <NavMenu
            icon={Scissors}
            label="Enzymes"
            onLeftClick={onToggleEnzymes}
            contentClassName="min-w-52 overflow-visible"
          >
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
            <DropdownMenuSub>
              <DropdownMenuSubTrigger inset>Choose Provider</DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="min-w-44">
                <DropdownMenuRadioGroup
                  value={enzymeProvider}
                  onValueChange={onEnzymeProviderChange}
                >
                  {ENZYME_PROVIDER_OPTIONS.map((opt) => (
                    <DropdownMenuRadioItem key={opt.value} value={opt.value}>
                      {opt.label}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
            <DropdownMenuItem onSelect={onOpenMyEnzymes}>
              <Scissors /> My Enzymes…
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={onOpenEnzymeDatabase}>
              <Database /> Enzyme Database…
            </DropdownMenuItem>
          </NavMenu>
        )}

        {/* Alignment (DNA only): left click opens the manager, right click opens menu */}
        {isDna && alignmentEnabled && (
          <NavMenu
            icon={ChartNoAxesGantt}
            label="Align"
            onLeftClick={onManageAlignments}
            contentClassName="min-w-56 overflow-visible"
          >
            <DropdownMenuCheckboxItem checked={showAlignments} onCheckedChange={onToggleAlignments}>
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
          </NavMenu>
        )}

        {/* Plasmid Map: left click toggles the background, right click opens menu */}
        {onOpenMapView && (
          <NavMenu
            icon={MapIcon}
            label="Map"
            onLeftClick={mapBackgroundToggle || onOpenMapView}
            contentClassName="min-w-44"
          >
            <DropdownMenuItem onSelect={onOpenMapView}>
              <MapIcon /> Examine Map
            </DropdownMenuItem>
          </NavMenu>
        )}

        {/* RNA Folding (RNA only): left click toggles the background, right click opens menu */}
        {moleculeType === 'rna' && onOpenRnaFold && (
          <NavMenu
            icon={AudioWaveform}
            label="Folding"
            onLeftClick={foldingBackgroundToggle || onOpenRnaFold}
            contentClassName="min-w-52"
          >
            <DropdownMenuItem onSelect={onOpenRnaFold}>
              <AudioWaveform /> Examine Secondary Structure
            </DropdownMenuItem>
          </NavMenu>
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
              {shortNucQuery
                ? 'Enter at least 3 bases'
                : navTotal > 0
                  ? `${navIndex + 1}/${navTotal}`
                  : '0/0'}
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
