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
  Dna,
  ArrowDownWideNarrow,
  ScanSearch,
  AudioWaveform,
  Grid2x2,
  Rows3,
  History,
  Check,
  Map as MapIcon,
  Waypoints,
  Circle,
  Eye,
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
import { plugins, pluginAvailable } from './plugins';
import { T, useTranslation } from '@/i18n';
import NavStateNotice, { useNavStateNotice } from './components/NavStateNotice';

const SEARCH_SCOPES = ['all', 'seq', 'feature', 'primer', 'enzyme'];

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
  'flex items-center gap-1.5 rounded-full px-3 py-1.5 text-sm font-semibold text-foreground/80 outline-none transition-all duration-150 hover:bg-accent hover:text-foreground active:scale-95 data-[state=open]:bg-accent data-[state=open]:text-foreground';

// Left click runs onLeftClick (when given) and right click opens the menu;
// without onLeftClick both left and right click open the menu. Radix opens
// the menu on left pointerdown by default — preventDefault() there opts out.
function NavMenu({ icon: Icon, label, onLeftClick, contentClassName, children }) {
  const { t } = useTranslation();
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
              strokeWidth={2.25}
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
              aria-label={t('Open {label} menu', { label: t(label) })}
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
          <span>
            <T>{label}</T>
          </span>
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
  onCreateMsa,
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
  // Per-plugin Features-menu checkbox state, keyed by plugin id:
  // { [pluginId]: { checked, onToggle } } for plugins declaring
  // featuresMenuItem in the registry.
  pluginToggles = {},
  disabledPlugins = [],
  showOrfs,
  onToggleOrfs,
  onCreateFeature,
  onOpenDetectFeatures,
  onOpenCodonOptimization,
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
  onSearchOpenChange,
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
  onOpenDotplot,
  onOpenGel,
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
  onOpenSessionEditLog,
  viewMode = 'wrap',
  onViewModeChange,
  showDoubleStrand = false,
  onToggleDoubleStrand,
  showTextColor = true,
  onToggleTextColor,
  alwaysExpandFeatures = false,
  onToggleAlwaysExpandFeatures,
  alwaysExpandPrimers = false,
  onToggleAlwaysExpandPrimers,
  featureLabelsBelow = false,
  onFeatureLabelsBelowChange,
  thickFeatureLines = false,
  onToggleThickFeatureLines,
}) {
  const { t, locale } = useTranslation();
  const { notice, showNotice } = useNavStateNotice();
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [searchScope, setSearchScope] = useState('all');
  const inputRef = useRef(null);

  const toggleState = (label, current, onChange) => () => {
    const next = !current;
    onChange?.(next);
    showNotice(label, next);
  };
  const checkState = (label, onChange) => (value) => {
    const next = value === true;
    onChange?.(next);
    showNotice(label, next);
  };
  const viewModeLabel = (mode) => (mode === 'wrap' ? 'Wrap (Vertical)' : 'Continuous (Horizontal)');
  const backgroundLabel = (value) =>
    backgroundOptions.find((option) => option.value === value)?.label || value;

  // rna/protein projects are single-strand sequences: keep Edit/Features/Search,
  // hide DNA-only tooling (primers, enzymes, alignment, antisense/translation).
  const isDna = moleculeType === 'dna';

  // Left click toggles the editor background: turning it on picks the
  // per-molecule-type default diagram (RNA → folding, DNA/protein → map),
  // turning it off goes back to none. Protein has no background option →
  // fall back to the examine dialog.
  const DEFAULT_DIAGRAM_BY_TYPE = { rna: 'folding' };
  const diagramOptions = backgroundOptions.filter((o) => o.value !== 'none');
  const defaultDiagram =
    diagramOptions.find((o) => o.value === (DEFAULT_DIAGRAM_BY_TYPE[moleculeType] || 'map'))
      ?.value || diagramOptions[0]?.value;
  const diagramBackgroundToggle =
    onBackgroundChange && defaultDiagram
      ? () => {
          const next = background === 'none' ? defaultDiagram : 'none';
          onBackgroundChange(next);
          showNotice('Background', backgroundLabel(next));
        }
      : null;
  const hasFolding = moleculeType === 'rna' && !!onOpenRnaFold;
  const hasDotplot = moleculeType !== 'protein' && !!onOpenDotplot;
  const hasGel = isDna && !!onOpenGel;

  const SCOPE_WORDS = {
    all: 'anything',
    seq: 'sequences',
    feature: 'features',
    primer: 'primers',
    enzyme: 'enzymes',
  };

  const setSearchVisibility = useCallback(
    (open) => {
      setSearchOpen(open);
      onSearchOpenChange?.(open);
    },
    [onSearchOpenChange],
  );

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
      setSearchVisibility(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [searchOpen, setSearchVisibility]);

  useEffect(() => {
    if (!openSearchRef) return;
    openSearchRef.current = () => setSearchVisibility(true);
    return () => {
      openSearchRef.current = null;
    };
  }, [openSearchRef, setSearchVisibility]);

  const toggleSearch = useCallback(() => {
    setSearchVisibility(!searchOpen);
  }, [searchOpen, setSearchVisibility]);

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
        setSearchVisibility(false);
      }
    },
    [query, onSearch, searchScope, setSearchVisibility],
  );

  const navTotal = searchNav && searchNav.query === query ? searchNav.total : 0;
  const navIndex = searchNav && searchNav.query === query ? searchNav.index : -1;
  // 1-2 base nucleotide queries are rejected by findSeqMatches (min length 3);
  // surface a hint instead of silently showing "0/0".
  const shortNucQuery = /^[ACGTURYSWKMBDHVN]{1,2}$/.test(query?.trim().toUpperCase() || '');

  // Plugins contributing a visibility toggle (e.g. GC content); shown in the
  // View menu.
  const viewTogglePlugins = plugins.filter(
    (p) =>
      p.featuresMenuItem &&
      pluginAvailable(p, { moleculeType, disabledPlugins }) &&
      pluginToggles[p.id],
  );
  const hasExpandItems =
    !!onToggleAlwaysExpandFeatures ||
    (isDna && !!onToggleAlwaysExpandPrimers) ||
    !!onFeatureLabelsBelowChange ||
    !!onToggleThickFeatureLines;
  const hasTrackItems = !!onToggleOrfs || viewTogglePlugins.length > 0;
  const hasStrandItems = (isDna && !!onToggleDoubleStrand) || !!onToggleTextColor;

  // Left click on View re-applies the most recently changed View-menu option;
  // before any menu interaction (or when the layout mode itself was last
  // changed) it toggles wrap/continuous.
  const [lastViewOption, setLastViewOption] = useState(null);
  const viewCheckedChange = (key, label, onChange) => (value) => {
    setLastViewOption(key);
    checkState(label, onChange)(value);
  };
  const viewLeftClick = (() => {
    if (lastViewOption === 'orfs' && onToggleOrfs)
      return toggleState('Show ORFs', showOrfs, onToggleOrfs);
    if (lastViewOption === 'dsRuler' && onToggleDoubleStrand)
      return toggleState('Show Both Strands', showDoubleStrand, onToggleDoubleStrand);
    if (lastViewOption === 'textColor' && onToggleTextColor)
      return toggleState('Show Text Color', showTextColor, onToggleTextColor);
    if (lastViewOption === 'expandFeatures' && onToggleAlwaysExpandFeatures)
      return toggleState('Expand All Features', alwaysExpandFeatures, onToggleAlwaysExpandFeatures);
    if (lastViewOption === 'expandPrimers' && onToggleAlwaysExpandPrimers)
      return toggleState('Expand All Primers', alwaysExpandPrimers, onToggleAlwaysExpandPrimers);
    if (lastViewOption === 'featureLabelsBelow' && onFeatureLabelsBelowChange)
      return toggleState('Feature Labels Below', featureLabelsBelow, onFeatureLabelsBelowChange);
    if (lastViewOption === 'thickFeatureLines' && onToggleThickFeatureLines)
      return toggleState('Thick Feature Lines', thickFeatureLines, onToggleThickFeatureLines);
    const pluginToggle = lastViewOption && pluginToggles[lastViewOption];
    const plugin = lastViewOption && viewTogglePlugins.find((p) => p.id === lastViewOption);
    if (pluginToggle && plugin)
      return toggleState(
        plugin.featuresMenuItem.label,
        pluginToggle.checked,
        pluginToggle.onToggle,
      );
    if (!onViewModeChange) return undefined;
    return () => {
      const next = viewMode === 'wrap' ? 'continuous' : 'wrap';
      onViewModeChange(next);
      showNotice('View', viewModeLabel(next));
    };
  })();

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
      <NavStateNotice notice={notice} />
      <div
        className={cn(
          'nav-bar-enter relative z-10 flex items-center rounded-full border bg-background/80 px-2 py-1.5 shadow-lg backdrop-blur-md',
          locale === 'zh' ? 'gap-1.5' : 'gap-0.5',
        )}
      >
        {/* Edit: left or right click opens the menu */}
        <NavMenu icon={Pencil} label="Edit" contentClassName="min-w-52 overflow-visible">
          <DropdownMenuItem disabled={!canDirectSave} onSelect={onSave}>
            <Save /> <T>{'Save'}</T>
            <DropdownMenuShortcut>⌘S</DropdownMenuShortcut>
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={onSaveAs}>
            <FileDown /> <T>{'Save As…'}</T>
            <DropdownMenuShortcut>⇧⌘S</DropdownMenuShortcut>
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={onCreateMsa}>
            <Rows3 /> <T>{'New MSA from Sequence'}</T>
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem disabled={!canUndo} onSelect={onUndo}>
            <Undo2 /> <T>{'Undo'}</T>
            <DropdownMenuShortcut>⌘Z</DropdownMenuShortcut>
          </DropdownMenuItem>
          <DropdownMenuItem disabled={!canRedo} onSelect={onRedo}>
            <Redo2 /> <T>{'Redo'}</T>
            <DropdownMenuShortcut>⇧⌘Z</DropdownMenuShortcut>
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            disabled={!hasSelection && !hasTranslationSelection}
            onSelect={onCopySense}
          >
            <CopyPlus /> <T>{'Copy (+) Strand'}</T>
          </DropdownMenuItem>
          {isDna && (
            <DropdownMenuItem
              disabled={!hasSelection && !hasTranslationSelection}
              onSelect={onCopyAntisense}
            >
              <CopyMinus /> <T>{'Copy (−) Strand'}</T>
            </DropdownMenuItem>
          )}
          {isDna && (
            <DropdownMenuItem disabled={!hasTranslationSelection} onSelect={onCopyTranslation}>
              <CopyX /> <T>{'Copy Translation'}</T>
            </DropdownMenuItem>
          )}
          <DropdownMenuItem disabled={!canPaste} onSelect={onPaste}>
            <ClipboardPaste /> <T>{'Paste'}</T>
            <DropdownMenuShortcut>⌘V</DropdownMenuShortcut>
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem disabled={!hasTextSelection} onSelect={onToUppercase}>
            <CaseUpper /> <T>{'To Uppercase'}</T>
          </DropdownMenuItem>
          <DropdownMenuItem disabled={!hasTextSelection} onSelect={onToLowercase}>
            <CaseLower /> <T>{'To Lowercase'}</T>
          </DropdownMenuItem>
          {isDna && onOpenCodonOptimization && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={onOpenCodonOptimization}>
                <Dna /> <T>{'Codon Optimization…'}</T>
              </DropdownMenuItem>
            </>
          )}
          {isDna && onToggleTopology && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                onSelect={() => {
                  onToggleTopology();
                  showNotice('Topology', topology === 'circular' ? 'Linear' : 'Circular');
                }}
              >
                {topology === 'circular' ? (
                  <>
                    <Minus /> <T>{'Linearize'}</T>
                  </>
                ) : (
                  <>
                    <Circle /> <T>{'Circularize'}</T>
                  </>
                )}
              </DropdownMenuItem>
            </>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={onOpenSessionEditLog}>
            <History /> <T>{'Session Edit Log…'}</T>
          </DropdownMenuItem>
          {/* SnapGene history snapshots (project from a .dna file) */}
          {onOpenSnapshots && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={onOpenSnapshots}>
                <History /> <T>{'History'}</T>
              </DropdownMenuItem>
            </>
          )}
        </NavMenu>

        {/* View: left click re-applies the last-changed option (default:
            toggle wrap/continuous), right click opens the menu */}
        <NavMenu
          icon={Eye}
          label="View"
          onLeftClick={viewLeftClick}
          contentClassName="min-w-52 overflow-visible"
        >
          <DropdownMenuRadioGroup
            value={viewMode}
            onValueChange={(v) => {
              setLastViewOption('mode');
              onViewModeChange?.(v);
              showNotice('View', viewModeLabel(v));
            }}
          >
            <DropdownMenuRadioItem value="wrap">
              <T>{'Wrap (Vertical)'}</T>
            </DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="continuous">
              <T>{'Continuous (Horizontal)'}</T>
            </DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
          {(hasTrackItems || hasStrandItems || hasExpandItems) && <DropdownMenuSeparator />}
          {/* Track overlays: ORFs, GC content, … */}
          {onToggleOrfs && (
            <DropdownMenuCheckboxItem
              checked={showOrfs}
              onCheckedChange={viewCheckedChange('orfs', 'Show ORFs', onToggleOrfs)}
            >
              <T>{'Show ORFs'}</T>
            </DropdownMenuCheckboxItem>
          )}
          {viewTogglePlugins.map((p) => {
            const checked = !!pluginToggles[p.id].checked;
            const toggle = (value) => {
              const next = typeof value === 'boolean' ? value : !checked;
              setLastViewOption(p.id);
              pluginToggles[p.id].onToggle();
              showNotice(p.featuresMenuItem.label, next);
            };
            const legend = p.featuresMenuItem.legend;
            // Plugins with a color legend (e.g. the protein property tracks)
            // get a submenu: the trigger itself stays the toggle, hovering it
            // reveals the non-interactive legend rows.
            if (!legend?.length) {
              return (
                <DropdownMenuCheckboxItem key={p.id} checked={checked} onCheckedChange={toggle}>
                  <T>{p.featuresMenuItem.label}</T>
                </DropdownMenuCheckboxItem>
              );
            }
            return (
              <DropdownMenuSub key={p.id}>
                <DropdownMenuSubTrigger inset className="relative [&>svg]:hidden" onClick={toggle}>
                  <span className="absolute left-2 flex h-3.5 w-3.5 items-center justify-center">
                    {checked && <Check className="h-4 w-4" />}
                  </span>
                  <T>{p.featuresMenuItem.label}</T>
                </DropdownMenuSubTrigger>
                <DropdownMenuSubContent>
                  {legend.map((entry) => (
                    <div
                      key={entry.label}
                      className="flex select-none items-center gap-2 px-2 py-1 text-xs text-muted-foreground"
                    >
                      <span
                        className="h-3 w-3 rounded-sm border border-black/20"
                        style={{ backgroundColor: entry.color }}
                      />
                      <T>{entry.label}</T>
                    </div>
                  ))}
                </DropdownMenuSubContent>
              </DropdownMenuSub>
            );
          })}
          {/* Strand display */}
          {hasTrackItems && hasStrandItems && <DropdownMenuSeparator />}
          {isDna && onToggleDoubleStrand && (
            <DropdownMenuCheckboxItem
              checked={showDoubleStrand}
              onCheckedChange={viewCheckedChange(
                'dsRuler',
                'Show Both Strands',
                onToggleDoubleStrand,
              )}
            >
              <T>{'Show Both Strands'}</T>
            </DropdownMenuCheckboxItem>
          )}
          {onToggleTextColor && (
            <DropdownMenuCheckboxItem
              checked={showTextColor}
              onCheckedChange={viewCheckedChange('textColor', 'Show Text Color', onToggleTextColor)}
            >
              <T>{'Show Text Color'}</T>
            </DropdownMenuCheckboxItem>
          )}
          {hasExpandItems && (
            <>
              {(hasTrackItems || hasStrandItems) && <DropdownMenuSeparator />}
              {onToggleAlwaysExpandFeatures && (
                <DropdownMenuCheckboxItem
                  checked={alwaysExpandFeatures}
                  onCheckedChange={viewCheckedChange(
                    'expandFeatures',
                    'Expand All Features',
                    onToggleAlwaysExpandFeatures,
                  )}
                >
                  <T>{'Expand All Features'}</T>
                </DropdownMenuCheckboxItem>
              )}
              {isDna && onToggleAlwaysExpandPrimers && (
                <DropdownMenuCheckboxItem
                  checked={alwaysExpandPrimers}
                  onCheckedChange={viewCheckedChange(
                    'expandPrimers',
                    'Expand All Primers',
                    onToggleAlwaysExpandPrimers,
                  )}
                >
                  <T>{'Expand All Primers'}</T>
                </DropdownMenuCheckboxItem>
              )}
              {onFeatureLabelsBelowChange && (
                <DropdownMenuCheckboxItem
                  checked={viewMode === 'continuous' ? true : featureLabelsBelow}
                  disabled={viewMode === 'continuous'}
                  onCheckedChange={viewCheckedChange(
                    'featureLabelsBelow',
                    'Feature Labels Below',
                    onFeatureLabelsBelowChange,
                  )}
                >
                  <T>{'Feature Labels Below'}</T>
                </DropdownMenuCheckboxItem>
              )}
              {onToggleThickFeatureLines && (
                <DropdownMenuCheckboxItem
                  checked={thickFeatureLines}
                  onCheckedChange={viewCheckedChange(
                    'thickFeatureLines',
                    'Thick Feature Lines',
                    onToggleThickFeatureLines,
                  )}
                >
                  <T>{'Thick Feature Lines'}</T>
                </DropdownMenuCheckboxItem>
              )}
            </>
          )}
        </NavMenu>

        {/* Features: left click toggles visibility, right click opens menu */}
        <NavMenu
          icon={Tag}
          label="Features"
          onLeftClick={toggleState('Show Features', showFeatures, onToggleFeatures)}
          contentClassName="min-w-52 overflow-visible"
        >
          <DropdownMenuCheckboxItem
            checked={showFeatures}
            onCheckedChange={checkState('Show Features', onToggleFeatures)}
          >
            <T>{'Show Features'}</T>
          </DropdownMenuCheckboxItem>
          <DropdownMenuSeparator />
          {onToggleAlwaysExpandFeatures && (
            <DropdownMenuCheckboxItem
              checked={alwaysExpandFeatures}
              onCheckedChange={viewCheckedChange(
                'expandFeatures',
                'Expand All Features',
                onToggleAlwaysExpandFeatures,
              )}
            >
              <T>{'Expand All Features'}</T>
            </DropdownMenuCheckboxItem>
          )}
          {onFeatureLabelsBelowChange && (
            <DropdownMenuCheckboxItem
              checked={viewMode === 'continuous' ? true : featureLabelsBelow}
              disabled={viewMode === 'continuous'}
              onCheckedChange={viewCheckedChange(
                'featureLabelsBelow',
                'Feature Labels Below',
                onFeatureLabelsBelowChange,
              )}
            >
              <T>{'Feature Labels Below'}</T>
            </DropdownMenuCheckboxItem>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={onCreateFeature}>
            <Plus /> <T>{'Create Feature'}</T>
            <DropdownMenuShortcut>⌘T</DropdownMenuShortcut>
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={onOpenDetectFeatures} disabled={!onOpenDetectFeatures}>
            <ScanSearch /> <T>{'Detect Common Features'}</T>
          </DropdownMenuItem>
        </NavMenu>

        {/* Primers (DNA only): left click toggles visibility, right click opens menu */}
        {isDna && (
          <NavMenu
            icon={ArrowRight}
            label="Primers"
            onLeftClick={toggleState('Show Primers', showPrimers, onTogglePrimers)}
            contentClassName="min-w-52 overflow-visible"
          >
            <DropdownMenuCheckboxItem
              checked={showPrimers}
              onCheckedChange={checkState('Show Primers', onTogglePrimers)}
            >
              <T>{'Show Primers'}</T>
            </DropdownMenuCheckboxItem>
            {onToggleAlwaysExpandPrimers && (
              <DropdownMenuCheckboxItem
                checked={alwaysExpandPrimers}
                onCheckedChange={viewCheckedChange(
                  'expandPrimers',
                  'Expand All Primers',
                  onToggleAlwaysExpandPrimers,
                )}
              >
                <T>{'Expand All Primers'}</T>
              </DropdownMenuCheckboxItem>
            )}
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={onCreatePrimer}>
              <Plus /> <T>{'Create Primer'}</T>
              <DropdownMenuShortcut>⌘R</DropdownMenuShortcut>
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={onOpenPrimerOverview}>
              <ArrowDownWideNarrow /> <T>{'Primer Overview'}</T>
            </DropdownMenuItem>
            <DropdownMenuSub>
              <DropdownMenuSubTrigger inset>
                <T>{'My Primer Collection'}</T>
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="min-w-48">
                <DropdownMenuItem onSelect={onOpenMyPrimers}>
                  <ListChecks /> <T>{'My Primer Collection…'}</T>
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  disabled={!hasSelectedPrimer}
                  onSelect={onAddCurrentPrimerToMyPrimers}
                >
                  <Plus /> <T>{'Add Current Primer'}</T>
                </DropdownMenuItem>
                <DropdownMenuItem disabled={!hasPrimers} onSelect={onAddAllPrimersToMyPrimers}>
                  <CopyPlus /> <T>{'Add All from This File'}</T>
                </DropdownMenuItem>
                <DropdownMenuCheckboxItem
                  checked={autoAddPrimers}
                  onCheckedChange={checkState('Auto-add from Opened Files', onToggleAutoAddPrimers)}
                >
                  <T>{'Auto-add from Opened Files'}</T>
                </DropdownMenuCheckboxItem>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
            {primerDesignEnabled && (
              <DropdownMenuSub>
                <DropdownMenuSubTrigger inset>
                  <T>{'Primer Design'}</T>
                </DropdownMenuSubTrigger>
                <DropdownMenuSubContent className="min-w-44">
                  <DropdownMenuItem onSelect={() => onPrimerDesign?.('amplify')}>
                    <T>{'Amplify Fragment'}</T>
                  </DropdownMenuItem>
                  <DropdownMenuItem onSelect={() => onPrimerDesign?.('oepcr')}>
                    <T>{'OE-PCR'}</T>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    disabled={topology !== 'circular'}
                    onSelect={() => onPrimerDesign?.('mutagenesis')}
                  >
                    <T>{'PCR Mutagenesis'}</T>
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
            onLeftClick={toggleState('Show Enzyme Sites', showEnzymes, onToggleEnzymes)}
            contentClassName="min-w-52 overflow-visible"
          >
            <DropdownMenuCheckboxItem
              checked={showEnzymes}
              onCheckedChange={checkState('Show Enzyme Sites', onToggleEnzymes)}
            >
              <T>{'Show Enzyme Sites'}</T>
            </DropdownMenuCheckboxItem>
            <DropdownMenuSeparator />
            <DropdownMenuSub>
              <DropdownMenuSubTrigger inset>
                <T>{'Enzyme Set'}</T>
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="min-w-44">
                <DropdownMenuRadioGroup
                  value={enzymeFilter}
                  onValueChange={(value) => {
                    onEnzymeFilterChange?.(value);
                    showNotice(
                      'Enzyme Set',
                      value === 'myEnzymes'
                        ? 'My Enzymes'
                        : ENZYME_FILTER_OPTIONS.find((option) => option.value === value)?.label,
                    );
                  }}
                >
                  {myEnzymes.length > 0 && (
                    <>
                      <DropdownMenuRadioItem value="myEnzymes">
                        <T>{'My Enzymes'}</T>
                      </DropdownMenuRadioItem>
                      <DropdownMenuSeparator />
                    </>
                  )}
                  {ENZYME_FILTER_OPTIONS.map((opt) => (
                    <DropdownMenuRadioItem
                      key={opt.value}
                      value={opt.value}
                      className={opt.className}
                    >
                      {opt.value === 'unique' ? (
                        <span>
                          <strong>
                            <T>{'Unique'}</T>
                          </strong>{' '}
                          <T>{'Cutters'}</T>
                        </span>
                      ) : opt.value === 'unique6' ? (
                        <span>
                          <strong>
                            <T>{'Unique'}</T>
                          </strong>{' '}
                          <T>{'6 bp'}</T>
                        </span>
                      ) : opt.value === 'twice' ? (
                        <span>
                          <T>{'Twice-cutter'}</T>
                          <sup>²</sup>
                        </span>
                      ) : opt.value === 'unique+twice' ? (
                        <span>
                          <strong>
                            <T>{'Unique'}</T>
                          </strong>{' '}
                          <T>{'+ Twice-cutter'}</T>
                          <sup>²</sup>
                        </span>
                      ) : (
                        <T>{opt.label}</T>
                      )}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
            <DropdownMenuSub>
              <DropdownMenuSubTrigger inset>
                <T>{'Provider'}</T>
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="min-w-44">
                <DropdownMenuRadioGroup
                  value={enzymeProvider}
                  onValueChange={(value) => {
                    onEnzymeProviderChange?.(value);
                    showNotice(
                      'Provider',
                      ENZYME_PROVIDER_OPTIONS.find((option) => option.value === value)?.label,
                    );
                  }}
                >
                  {ENZYME_PROVIDER_OPTIONS.map((opt) => (
                    <DropdownMenuRadioItem key={opt.value} value={opt.value}>
                      {<T>{opt.label}</T>}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
            <DropdownMenuItem onSelect={onOpenMyEnzymes}>
              <Scissors /> <T>{'My Enzymes…'}</T>
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={onOpenEnzymeDatabase}>
              <Database /> <T>{'Enzyme Database…'}</T>
            </DropdownMenuItem>
            {hasGel && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem onSelect={() => onOpenGel()}>
                  <Rows3 /> <T>{'Digest and Run Gel'}</T>
                </DropdownMenuItem>
              </>
            )}
          </NavMenu>
        )}

        {/* Alignment (DNA only): right click opens menu. Left click: no alignments →
            open the manager; some checked → toggle Show Alignments; all unchecked →
            enable Show Alignments and check them all. */}
        {isDna && alignmentEnabled && (
          <NavMenu
            icon={ChartNoAxesGantt}
            label="Align"
            onLeftClick={
              alignments.length === 0
                ? onManageAlignments
                : alignments.some((a) => !hiddenAlignIds.includes(a.id))
                  ? toggleState('Show Alignments', showAlignments, onToggleAlignments)
                  : () => {
                      if (!showAlignments) onToggleAlignments?.();
                      alignments.forEach((a) => {
                        if (hiddenAlignIds.includes(a.id)) onToggleAlignmentVisible?.(a.id);
                      });
                      showNotice('Show Alignments', true);
                    }
            }
            contentClassName="min-w-56 overflow-visible"
          >
            <DropdownMenuCheckboxItem
              checked={showAlignments}
              onCheckedChange={checkState('Show Alignments', onToggleAlignments)}
            >
              <T>{'Show Alignments'}</T>
            </DropdownMenuCheckboxItem>
            {alignments.length > 0 && <DropdownMenuSeparator />}
            {alignments.map((a) => (
              <DropdownMenuCheckboxItem
                key={a.id}
                checked={!hiddenAlignIds.includes(a.id)}
                onCheckedChange={(value) => {
                  onToggleAlignmentVisible?.(a.id);
                  showNotice('Show {name}', value === true, { name: a.name });
                }}
              >
                <span className="truncate">{a.name}</span>
                <DropdownMenuShortcut className="ml-4 text-muted-foreground">
                  {a.identity != null ? `${(a.identity * 100).toFixed(0)}%` : ''}
                </DropdownMenuShortcut>
              </DropdownMenuCheckboxItem>
            ))}
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={onAddAlignmentFile}>
              <FileUp /> <T>{'Add from File…'}</T>
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={onAddAlignmentText}>
              <Type /> <T>{'Add from Text…'}</T>
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={onManageAlignments}>
              <ListChecks /> <T>{'Manage Alignments…'}</T>
            </DropdownMenuItem>
          </NavMenu>
        )}

        {/* Diagrams: left click toggles the background, right click opens menu */}
        {(onOpenMapView || hasFolding || hasDotplot || hasGel) && (
          <NavMenu
            icon={Waypoints}
            label="Diagrams"
            onLeftClick={diagramBackgroundToggle || onOpenMapView || onOpenRnaFold}
            contentClassName="min-w-52 overflow-visible"
          >
            {onBackgroundChange && backgroundOptions.length > 0 && (
              <>
                <DropdownMenuRadioGroup
                  value={background}
                  onValueChange={(value) => {
                    onBackgroundChange(value);
                    showNotice('Background', backgroundLabel(value));
                  }}
                >
                  {backgroundOptions.map((o) => (
                    <DropdownMenuRadioItem key={o.value} value={o.value}>
                      <T>{o.label}</T>
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
                <DropdownMenuSeparator />
              </>
            )}
            {onOpenMapView && (
              <DropdownMenuItem onSelect={onOpenMapView}>
                <MapIcon /> <T>{'Examine Map'}</T>
              </DropdownMenuItem>
            )}
            {hasFolding && (
              <DropdownMenuItem onSelect={onOpenRnaFold}>
                <AudioWaveform /> <T>{'Examine Secondary Structure'}</T>
              </DropdownMenuItem>
            )}
            {hasDotplot && (
              <DropdownMenuItem onSelect={onOpenDotplot}>
                <Grid2x2 /> <T>{'Examine Dotplot'}</T>
              </DropdownMenuItem>
            )}
            {hasGel && (
              <DropdownMenuItem onSelect={onOpenGel}>
                <Rows3 /> <T>{'Digest and Run Gel'}</T>
              </DropdownMenuItem>
            )}
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
          <Search className="size-4" strokeWidth={2.25} />
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
            placeholder={t('Search {scope} (Tab to switch)', {
              scope: t(SCOPE_WORDS[searchScope]),
            })}
            className="min-w-0 flex-1 rounded-full bg-muted/60 px-3 py-1.5 text-sm outline-none placeholder:text-muted-foreground"
            style={{ border: 'none' }}
            tabIndex={searchOpen ? 0 : -1}
          />
          {query && (
            <span className="mr-0.5 flex items-center gap-0.5 text-xs text-muted-foreground tabular-nums">
              {shortNucQuery ? (
                <T>{'Enter at least 3 bases'}</T>
              ) : navTotal > 0 ? (
                `${navIndex + 1}/${navTotal}`
              ) : (
                '0/0'
              )}
              <button
                type="button"
                aria-label={t('Previous match')}
                onClick={() => onSearch?.(query, 'prev', searchScope)}
                className="rounded-full p-1 transition-colors hover:bg-accent hover:text-foreground"
              >
                <ChevronUp className="size-3.5" strokeWidth={2.25} />
              </button>
              <button
                type="button"
                aria-label={t('Next match')}
                onClick={() => onSearch?.(query, 'next', searchScope)}
                className="rounded-full p-1 transition-colors hover:bg-accent hover:text-foreground"
              >
                <ChevronDown className="size-3.5" strokeWidth={2.25} />
              </button>
            </span>
          )}
          <button
            type="button"
            aria-label={t('Close search')}
            onClick={toggleSearch}
            className="flex items-center justify-center rounded-full bg-accent p-2 text-foreground outline-none transition-all duration-150 hover:text-foreground active:scale-90"
          >
            <X className="size-4" strokeWidth={2.25} />
          </button>
        </div>
      </div>
    </div>
  );
}
