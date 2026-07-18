import React, { useState, useRef, useEffect, useCallback } from 'react';
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
}) {
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState('');
  const inputRef = useRef(null);

  useEffect(() => {
    if (searchOpen) inputRef.current?.focus();
  }, [searchOpen]);

  const toggleSearch = useCallback(() => {
    setSearchOpen((v) => !v);
  }, []);

  const onSearchKeyDown = useCallback(
    (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        if (query) onSearch?.(query);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        setSearchOpen(false);
      }
    },
    [query, onSearch],
  );

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
            <DropdownMenuItem disabled={!hasSelection} onSelect={onCopySense}>
              <CopyPlus /> Copy Sense
            </DropdownMenuItem>
            <DropdownMenuItem disabled={!hasSelection} onSelect={onCopyAntisense}>
              <CopyMinus /> Copy Antisense
            </DropdownMenuItem>
            <DropdownMenuItem disabled={!hasSelection} onSelect={onCopyTranslation}>
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
                <DropdownMenuRadioGroup
                  value={enzymeFilter}
                  onValueChange={onEnzymeFilterChange}
                >
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

        {/* Search */}
        <div className="flex items-center">
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onSearchKeyDown}
            placeholder="Search sequence…"
            className={cn(
              'rounded-full bg-muted/60 text-sm outline-none transition-all duration-300 ease-[cubic-bezier(0.34,1.3,0.64,1)] placeholder:text-muted-foreground',
              searchOpen ? 'mr-1 w-44 px-3 py-1.5' : 'w-0 px-0 py-1.5 opacity-0',
            )}
            style={{ border: 'none' }}
            tabIndex={searchOpen ? 0 : -1}
          />
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
