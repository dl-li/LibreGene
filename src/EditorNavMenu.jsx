import { useState, useRef, useEffect, useCallback } from 'react';
import {
  Pencil,
  Tag,
  ArrowRight,
  Scissors,
  Search,
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
  Type,
  ListChecks,
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
  { value: 'unique', label: 'Unique Cutters' },
  { value: 'unique6', label: 'Unique 6 bp' },
  { value: 'blunt', label: 'Blunt' },
  { value: 'overhang5', label: "5' Overhang" },
  { value: 'overhang3', label: "3' Overhang" },
  { value: 'iis', label: 'Type IIS' },
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

function PlaceholderItem({ label }) {
  return (
    <DropdownMenuItem disabled className="justify-between">
      <span>{label}</span>
      <span className="ml-4 text-xs text-muted-foreground">Soon</span>
    </DropdownMenuItem>
  );
}

export default function EditorNavMenu({
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
  onCreateFeature,
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
}) {
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [searchScope, setSearchScope] = useState('all');
  const inputRef = useRef(null);

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

  const onQueryChange = useCallback(
    (e) => {
      const q = e.target.value;
      setQuery(q);
      onSearch?.(q, 'reset', searchScope);
    },
    [onSearch, searchScope],
  );

  const onSearchKeyDown = useCallback(
    (e) => {
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
      <div className="nav-bar-enter flex items-center gap-0.5 rounded-full border bg-background/80 px-2 py-1.5 shadow-lg backdrop-blur-md">
        {/* Edit */}
        <DropdownMenu modal={false}>
          <NavTrigger icon={Pencil} label="Edit" />
          <DropdownMenuContent side="top" align="center" className="min-w-52 overflow-visible">
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
              <CopyPlus /> Copy Sense
            </DropdownMenuItem>
            <DropdownMenuItem
              disabled={!hasSelection && !hasTranslationSelection}
              onSelect={onCopyAntisense}
            >
              <CopyMinus /> Copy Antisense
            </DropdownMenuItem>
            <DropdownMenuItem disabled={!hasTranslationSelection} onSelect={onCopyTranslation}>
              <CopyX /> Copy Translation
            </DropdownMenuItem>
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
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={onCreateFeature}>
              <Plus /> Create Feature
              <DropdownMenuShortcut>⌘T</DropdownMenuShortcut>
            </DropdownMenuItem>
            <PlaceholderItem label="Always Expand Feature" />
          </DropdownMenuContent>
        </DropdownMenu>

        {/* Primers */}
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
            <PlaceholderItem label="My Primers" />
            <PlaceholderItem label="PCR Analysis" />
            <PlaceholderItem label="Primer Design" />
            <PlaceholderItem label="Options" />
          </DropdownMenuContent>
        </DropdownMenu>

        {/* Enzymes */}
        <DropdownMenu modal={false}>
          <NavTrigger icon={Scissors} label="Enzymes" />
          <DropdownMenuContent side="top" align="center" className="min-w-52 overflow-visible">
            <DropdownMenuCheckboxItem checked={showEnzymes} onCheckedChange={onToggleEnzymes}>
              Show Enzyme Sites
            </DropdownMenuCheckboxItem>
            <DropdownMenuSeparator />
            <DropdownMenuSub>
              <DropdownMenuSubTrigger>Choose Enzyme Set</DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="min-w-44">
                <DropdownMenuRadioGroup value={enzymeFilter} onValueChange={onEnzymeFilterChange}>
                  {ENZYME_FILTER_OPTIONS.map((opt) => (
                    <DropdownMenuRadioItem key={opt.value} value={opt.value}>
                      {opt.label}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
            <PlaceholderItem label="Customize Enzyme Set" />
            <PlaceholderItem label="Enzyme Database" />
            <PlaceholderItem label="Digestion Analysis" />
          </DropdownMenuContent>
        </DropdownMenu>

        {/* Alignment */}
        {alignmentEnabled && (
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

        {/* Search */}
        <div className="flex items-center">
          <input
            ref={inputRef}
            value={query}
            onChange={onQueryChange}
            onKeyDown={onSearchKeyDown}
            placeholder={`Search ${SCOPE_WORDS[searchScope]} (Tab to switch)`}
            className={cn(
              'rounded-full bg-muted/60 text-sm outline-none transition-all duration-300 ease-[cubic-bezier(0.34,1.3,0.64,1)] placeholder:text-muted-foreground',
              searchOpen ? 'mr-1 w-72 px-3 py-1.5' : 'w-0 px-0 py-1.5 opacity-0',
            )}
            style={{ border: 'none' }}
            tabIndex={searchOpen ? 0 : -1}
          />
          {searchOpen && query && (
            <span className="mr-0.5 flex items-center gap-0.5 text-xs text-muted-foreground tabular-nums">
              {navTotal > 0 ? `${navIndex + 1}/${navTotal}` : '0/0'}
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
            onClick={toggleSearch}
            className={cn(
              'flex items-center justify-center rounded-full p-2 text-foreground/80 outline-none transition-all duration-150 hover:bg-accent hover:text-foreground active:scale-90',
              searchOpen && 'bg-accent text-foreground',
            )}
          >
            <Search className="size-4" />
          </button>
        </div>
      </div>
    </div>
  );
}
