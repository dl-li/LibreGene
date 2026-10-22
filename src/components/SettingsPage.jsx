import { useEffect, useState } from 'react';
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { cn } from '@/lib/utils';
import {
  Dna,
  SlidersHorizontal,
  Scissors,
  FlaskConical,
  GitCompareArrows,
  Puzzle,
  ChevronDown,
  ChevronRight,
} from 'lucide-react';

const NUM_INPUT = 'w-20 h-8 px-2 py-0 text-sm text-right font-mono';

// Label + optional hint on the left, control(s) on the right.
function SettingRow({ label, hint, children }) {
  return (
    <div className="flex items-center justify-between gap-6 py-3">
      <div className="min-w-0">
        <div className="text-[13px] font-medium">{label}</div>
        {hint && <div className="mt-0.5 text-xs leading-relaxed text-muted-foreground">{hint}</div>}
      </div>
      <div className="flex shrink-0 items-center gap-1.5">{children}</div>
    </div>
  );
}

function NumberField({ value, min, max, step, onCommit, className }) {
  return (
    <Input
      className={cn(NUM_INPUT, className)}
      type="number"
      min={min}
      max={max}
      step={step}
      value={value}
      onChange={(e) => {
        if (e.target.value === '') return;
        const v = Number(e.target.value);
        if (Number.isFinite(v)) onCommit?.(v);
      }}
    />
  );
}

const METHYLATION_SYSTEMS = [
  { key: 'dam', label: 'Dam', hint: 'Methylates the adenine in GATC' },
  { key: 'dcm', label: 'Dcm', hint: 'Methylates the internal cytosine in CC(A/T)GG' },
  {
    key: 'ecoki',
    label: 'EcoKI',
    hint: 'Methylates adenines in AAC(N6)GTGC / GCAC(N6)GTT',
  },
];

const TM_FIELDS = [
  { key: 'naConc', label: 'Na⁺', unit: 'mM', mult: 1000, step: 1, min: 0, max: 500 },
  { key: 'mgConc', label: 'Mg²⁺', unit: 'mM', mult: 1000, step: 0.1, min: 0, max: 10 },
  { key: 'dntpConc', label: 'dNTPs', unit: 'mM', mult: 1000, step: 0.1, min: 0, max: 10 },
  { key: 'trisConc', label: 'Tris-HCl', unit: 'mM', mult: 1000, step: 1, min: 0, max: 200 },
  { key: 'primerConc', label: 'Primer', unit: 'nM', mult: 1e9, step: 50, min: 0, max: 5000 },
];

const CASE_DISPLAY_OPTIONS = [
  { value: 'asis', label: 'As-is' },
  { value: 'upper', label: 'Uppercase' },
  { value: 'lower', label: 'Lowercase' },
];

const CASE_DISPLAY_TYPES = [
  { key: 'dna', label: 'DNA', hint: 'e.g. .gbk, .dna, .fasta' },
  { key: 'rna', label: 'RNA', hint: 'e.g. .rna' },
  { key: 'protein', label: 'Protein', hint: 'e.g. .faa, .prot' },
];

export default function SettingsPage({
  open,
  onOpenChange,
  // Per-plugin settings-field state, keyed by plugin id:
  // { [pluginId]: { value, onChange } } for plugins declaring settingsField.
  pluginSettings = {},
  methylationSystems,
  setMethylationSystems,
  methylationOverlap,
  setMethylationOverlap,
  maxEnzymeSites = 50,
  onMaxEnzymeSitesChange,
  primerSeedLength,
  setPrimerSeedLength,
  alignmentAlgorithm = 'blast',
  onAlignmentAlgorithmChange,
  tmParams,
  setTmParams,
  caseDisplay = { dna: 'asis', rna: 'asis', protein: 'upper' },
  onCaseDisplayChange,
  highContrastTextColors = false,
  onToggleHighContrastTextColors,
  plugins = [],
  disabledPlugins = [],
  onTogglePlugin,
  focusSection,
}) {
  const [section, setSection] = useState('sequence');
  const [showBuiltinPlugins, setShowBuiltinPlugins] = useState(false);

  const trackFieldPlugins = plugins.filter(
    (p) => p.settingsField && !disabledPlugins.includes(p.id),
  );
  const distributablePlugins = plugins.filter((p) => p.distributable);
  const builtinPlugins = plugins.filter((p) => !p.distributable);

  const sections = [
    {
      id: 'sequence',
      label: 'Sequence',
      icon: Dna,
      description: 'How sequence letters are displayed',
    },
    trackFieldPlugins.length > 0 && {
      id: 'tracks',
      label: 'Tracks',
      icon: SlidersHorizontal,
      description: 'Options for the analysis tracks drawn alongside the sequence',
    },
    {
      id: 'enzymes',
      label: 'Enzymes',
      icon: Scissors,
      description: 'Restriction-site display and methylation blocking',
    },
    {
      id: 'primers',
      label: 'Primers',
      icon: FlaskConical,
      description: 'Primer binding analysis and Tm calculation conditions',
    },
    {
      id: 'alignment',
      label: 'Alignment',
      icon: GitCompareArrows,
      description: 'How sequencing reads are aligned to the template',
    },
    {
      id: 'plugins',
      label: 'Plugins',
      icon: Puzzle,
      description: 'Enable or disable installed plugins',
    },
  ].filter(Boolean);

  const activeSection = sections.find((s) => s.id === section) || sections[0];

  // Re-open on the requested section (sidebar "Plugins" entry → plugins).
  useEffect(() => {
    if (!open) return;
    if (focusSection === 'plugins') setSection('plugins');
    else setSection('sequence');
  }, [open]);

  const renderPluginRow = (p) => (
    <div key={p.id} className="flex items-start gap-2.5 py-2.5">
      <Checkbox
        id={`st-plugin-${p.id}`}
        className="mt-0.5"
        checked={!disabledPlugins.includes(p.id)}
        onCheckedChange={() => onTogglePlugin?.(p.id)}
      />
      <div className="flex min-w-0 flex-col">
        <Label htmlFor={`st-plugin-${p.id}`} className="cursor-pointer text-[13px] font-medium">
          {p.name}
          {p.version && (
            <span className="ml-1.5 text-xs font-normal text-muted-foreground">v{p.version}</span>
          )}
          {p.author && (
            <span className="ml-1.5 text-xs font-normal text-muted-foreground">by {p.author}</span>
          )}
        </Label>
        {p.description && (
          <span className="mt-0.5 text-xs leading-relaxed text-muted-foreground">
            {p.description}
          </span>
        )}
      </div>
    </div>
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[min(36rem,85vh)] gap-0 overflow-hidden p-0 sm:max-w-2xl">
        {/* ── Section nav ── */}
        <div className="flex w-44 shrink-0 flex-col border-r border-border/60 bg-muted/40">
          <DialogTitle className="px-4 pb-2 pt-4">Settings</DialogTitle>
          <nav className="flex flex-col gap-0.5 p-2">
            {sections.map((s) => (
              <button
                key={s.id}
                type="button"
                onClick={() => setSection(s.id)}
                className={cn(
                  'flex items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[13px] transition-colors',
                  s.id === activeSection.id
                    ? 'bg-background font-medium text-foreground shadow-sm'
                    : 'text-muted-foreground hover:bg-background/60 hover:text-foreground',
                )}
              >
                <s.icon className="size-3.5 shrink-0" />
                {s.label}
              </button>
            ))}
          </nav>
        </div>

        {/* ── Section content ── */}
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="border-b border-border/60 px-5 pb-3 pt-4">
            <h2 className="text-sm font-semibold">{activeSection.label}</h2>
            <p className="mt-0.5 text-xs text-muted-foreground">{activeSection.description}</p>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-5 py-2">
            {activeSection.id === 'sequence' && (
              <div className="divide-y divide-border/60">
                {CASE_DISPLAY_TYPES.map((t) => (
                  <SettingRow
                    key={t.key}
                    label={`${t.label} case display`}
                    hint={
                      t.key === 'protein'
                        ? 'Applies to the editor, map view and copied text'
                        : `As-is keeps the case stored in the file (${t.hint})`
                    }
                  >
                    <Select
                      value={caseDisplay[t.key] || 'asis'}
                      onValueChange={(v) => onCaseDisplayChange?.(t.key, v)}
                    >
                      <SelectTrigger className="h-8 w-32 px-2 py-0 text-sm">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {CASE_DISPLAY_OPTIONS.map((o) => (
                          <SelectItem key={o.value} value={o.value}>
                            {o.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </SettingRow>
                ))}
                <SettingRow
                  label="High contrast text colors"
                  hint="Display palette-based text colors vividly and use matching presets"
                >
                  <Checkbox
                    checked={highContrastTextColors}
                    onCheckedChange={onToggleHighContrastTextColors}
                  />
                </SettingRow>
              </div>
            )}

            {activeSection.id === 'tracks' && (
              <div className="divide-y divide-border/60">
                {trackFieldPlugins.map((p) => {
                  const field = p.settingsField;
                  const setting = pluginSettings[p.id];
                  return (
                    <SettingRow
                      key={p.id}
                      label={`${p.name} — ${field.label}`}
                      hint={field.description}
                    >
                      <NumberField
                        min={field.min}
                        max={field.max}
                        value={setting?.value ?? ''}
                        onCommit={(v) => setting?.onChange?.(v)}
                      />
                      {field.unit && (
                        <span className="text-sm text-muted-foreground">{field.unit}</span>
                      )}
                    </SettingRow>
                  );
                })}
              </div>
            )}

            {activeSection.id === 'enzymes' && (
              <div className="divide-y divide-border/60">
                <SettingRow
                  label="Hide enzymes with many cut sites"
                  hint='Applies to every enzyme filter, including "All"'
                >
                  <span className="text-xs text-muted-foreground">&gt;</span>
                  <NumberField min={1} value={maxEnzymeSites} onCommit={onMaxEnzymeSitesChange} />
                  <span className="text-sm text-muted-foreground">sites</span>
                </SettingRow>
                <div className="py-3">
                  <div className="text-[13px] font-medium">Methylation blocking</div>
                  <div className="mt-0.5 text-xs leading-relaxed text-muted-foreground">
                    Sites overlapping these methylation systems are marked as blocked
                  </div>
                  <div className="mt-2.5 divide-y divide-border/60 rounded-md border border-border/60 px-3">
                    {METHYLATION_SYSTEMS.map((sys) => (
                      <div key={sys.key} className="flex items-center justify-between gap-4 py-2">
                        <div className="min-w-0">
                          <Label
                            htmlFor={`st-meth-${sys.key}`}
                            className="cursor-pointer text-[13px] font-normal"
                          >
                            {sys.label}
                          </Label>
                          <div className="text-[11px] text-muted-foreground">{sys.hint}</div>
                        </div>
                        <Checkbox
                          id={`st-meth-${sys.key}`}
                          checked={methylationSystems.includes(sys.key)}
                          onCheckedChange={(v) =>
                            setMethylationSystems(
                              v
                                ? [...methylationSystems, sys.key]
                                : methylationSystems.filter((s) => s !== sys.key),
                            )
                          }
                        />
                      </div>
                    ))}
                  </div>
                </div>
                <SettingRow
                  label="Detection window"
                  hint="A methylation target within ±N bp of a recognition site blocks methylation-sensitive enzymes"
                >
                  <span className="text-sm text-muted-foreground">±</span>
                  <NumberField
                    min={0}
                    max={10}
                    value={methylationOverlap}
                    onCommit={setMethylationOverlap}
                  />
                  <span className="text-sm text-muted-foreground">bp</span>
                </SettingRow>
              </div>
            )}

            {activeSection.id === 'primers' && (
              <div className="divide-y divide-border/60">
                <SettingRow
                  label="Seed region length"
                  hint="3′ seed used when checking primer binding"
                >
                  <NumberField
                    min={6}
                    max={20}
                    value={primerSeedLength}
                    onCommit={setPrimerSeedLength}
                  />
                  <span className="text-sm text-muted-foreground">bp</span>
                </SettingRow>
                <div className="py-3">
                  <div className="text-[13px] font-medium">Tm calculation conditions</div>
                  <div className="mt-2.5 divide-y divide-border/60 rounded-md border border-border/60 px-3">
                    {TM_FIELDS.map(({ key, label, unit, mult, step, min, max }) => (
                      <div key={key} className="flex items-center justify-between gap-4 py-2">
                        <span className="text-[13px]">{label}</span>
                        <div className="flex items-center gap-1.5">
                          <Input
                            className={NUM_INPUT}
                            type="number"
                            step={step}
                            min={min}
                            max={max}
                            value={Math.round(tmParams[key] * mult * 100) / 100}
                            onChange={(e) => {
                              const raw = parseFloat(e.target.value);
                              if (!isNaN(raw) && raw >= min && raw <= max) {
                                setTmParams({ ...tmParams, [key]: raw / mult });
                              }
                            }}
                          />
                          <span className="w-7 text-sm text-muted-foreground">{unit}</span>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            )}

            {activeSection.id === 'alignment' && (
              <div className="divide-y divide-border/60">
                <SettingRow
                  label="Algorithm"
                  hint="BLAST chains multiple colinear hits, so split or multi-hit reads align in full; Smith-Waterman returns a single local block plus at most one flank"
                >
                  <Select
                    value={alignmentAlgorithm}
                    onValueChange={(v) => onAlignmentAlgorithmChange?.(v)}
                  >
                    <SelectTrigger className="h-8 w-44 px-2 py-0 text-sm">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="blast">BLAST</SelectItem>
                      <SelectItem value="smith-waterman">Smith-Waterman</SelectItem>
                    </SelectContent>
                  </Select>
                </SettingRow>
              </div>
            )}

            {activeSection.id === 'plugins' && (
              <div>
                <div className="divide-y divide-border/60">
                  {distributablePlugins.map(renderPluginRow)}
                </div>
                {builtinPlugins.length > 0 && (
                  <div className="pt-1.5">
                    <button
                      type="button"
                      onClick={() => setShowBuiltinPlugins((v) => !v)}
                      className="flex w-full items-center gap-1.5 py-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground hover:text-foreground"
                    >
                      {showBuiltinPlugins ? (
                        <ChevronDown className="size-3.5" />
                      ) : (
                        <ChevronRight className="size-3.5" />
                      )}
                      Built-in plugins ({builtinPlugins.length})
                    </button>
                    {showBuiltinPlugins && (
                      <div className="divide-y divide-border/60">
                        {builtinPlugins.map(renderPluginRow)}
                      </div>
                    )}
                  </div>
                )}
                {plugins.length === 0 && (
                  <div className="py-2 text-sm text-muted-foreground">No plugins installed</div>
                )}
              </div>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
