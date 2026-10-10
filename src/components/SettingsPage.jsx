import { Fragment, useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogClose,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { Input } from '@/components/ui/input';
import { Separator } from '@/components/ui/separator';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Settings, Puzzle, ChevronDown, ChevronRight } from 'lucide-react';

const SECTION_TITLE =
  'text-[11px] font-semibold uppercase tracking-wider text-muted-foreground mb-1.5';

export default function SettingsPage({
  open,
  onOpenChange,
  alwaysExpandFeatures = false,
  onToggleAlwaysExpandFeatures,
  featureLabelsBelow = false,
  onFeatureLabelsBelowChange,
  // Per-plugin settings-field state, keyed by plugin id:
  // { [pluginId]: { value, onChange } } for plugins declaring settingsField.
  pluginSettings = {},
  methylationSystems,
  setMethylationSystems,
  methylationOverlap,
  setMethylationOverlap,
  primerSeedLength,
  setPrimerSeedLength,
  alignmentAlgorithm = 'blast',
  onAlignmentAlgorithmChange,
  tmParams,
  setTmParams,
  plugins = [],
  disabledPlugins = [],
  onTogglePlugin,
  focusSection,
}) {
  const isPluginsView = focusSection === 'plugins';
  const [showBuiltinPlugins, setShowBuiltinPlugins] = useState(false);

  const distributablePlugins = plugins.filter((p) => p.distributable);
  const builtinPlugins = plugins.filter((p) => !p.distributable);

  const renderPluginRow = (p) => (
    <div key={p.id} className="flex items-start gap-2">
      <Checkbox
        id={`st-plugin-${p.id}`}
        className="mt-0.5"
        checked={!disabledPlugins.includes(p.id)}
        onCheckedChange={() => onTogglePlugin?.(p.id)}
      />
      <div className="flex flex-col">
        <Label htmlFor={`st-plugin-${p.id}`} className="cursor-pointer text-sm font-normal">
          {p.name}
        </Label>
        {p.description && <span className="text-xs text-muted-foreground">{p.description}</span>}
      </div>
    </div>
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {isPluginsView ? (
              <Puzzle className="size-4 text-muted-foreground" />
            ) : (
              <Settings className="size-4 text-muted-foreground" />
            )}
            {isPluginsView ? 'Plugins' : 'Settings'}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {!isPluginsView && (
            <>
              {/* ── Feature display ── */}
              <div>
                <div className={SECTION_TITLE}>Features</div>
                <div className="space-y-3">
                  <div className="flex items-start gap-2">
                    <Checkbox
                      id="st-always-expand-features"
                      className="mt-0.5"
                      checked={alwaysExpandFeatures}
                      onCheckedChange={() => onToggleAlwaysExpandFeatures?.()}
                    />
                    <div className="flex flex-col">
                      <Label
                        htmlFor="st-always-expand-features"
                        className="cursor-pointer text-sm font-normal"
                      >
                        Always expand features
                      </Label>
                      <span className="text-xs text-muted-foreground">
                        Keep features fully expanded without hovering over them
                      </span>
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <Label className="text-sm text-muted-foreground shrink-0">
                      Feature label position
                    </Label>
                    <Select
                      value={featureLabelsBelow ? 'below' : 'side'}
                      onValueChange={(v) => onFeatureLabelsBelowChange?.(v === 'below')}
                    >
                      <SelectTrigger className="w-28 h-8 px-2 py-0 text-sm">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="side">Side</SelectItem>
                        <SelectItem value="below">Below</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <span className="block text-xs text-muted-foreground">
                    Side places name labels on the feature line's left/right extension; Below places
                    them under the feature line
                  </span>
                </div>
              </div>

              <Separator />

              {/* ── Plugin settings fields (registry settingsField hook, e.g. GC Content) ── */}
              {plugins
                .filter((p) => p.settingsField && !disabledPlugins.includes(p.id))
                .map((p) => {
                  const field = p.settingsField;
                  const setting = pluginSettings[p.id];
                  return (
                    <Fragment key={p.id}>
                      <div>
                        <div className={SECTION_TITLE}>{p.name}</div>
                        <div className="flex items-center gap-2">
                          <Label className="text-sm text-muted-foreground shrink-0">
                            {field.label}
                          </Label>
                          <Input
                            className="w-16 h-8 px-2 py-0 text-sm text-right font-mono"
                            type="number"
                            min={field.min}
                            max={field.max}
                            value={setting?.value ?? ''}
                            onChange={(e) => {
                              if (e.target.value === '') return;
                              const v = Number(e.target.value);
                              if (Number.isFinite(v)) setting?.onChange?.(v);
                            }}
                          />
                          {field.unit && (
                            <span className="text-sm text-muted-foreground">{field.unit}</span>
                          )}
                        </div>
                        {field.description && (
                          <span className="block text-xs text-muted-foreground mt-1.5">
                            {field.description}
                          </span>
                        )}
                      </div>
                      <Separator />
                    </Fragment>
                  );
                })}

              {/* ── Methylation ── */}
              <div>
                <div className={SECTION_TITLE}>Methylation</div>
                <div className="space-y-2">
                  <div className="flex items-center gap-2">
                    <Checkbox
                      id="st-meth-dam"
                      checked={methylationSystems.includes('dam')}
                      onCheckedChange={(v) =>
                        setMethylationSystems(
                          v
                            ? [...methylationSystems, 'dam']
                            : methylationSystems.filter((s) => s !== 'dam'),
                        )
                      }
                    />
                    <Label htmlFor="st-meth-dam" className="cursor-pointer text-sm font-normal">
                      Dam
                    </Label>
                  </div>
                  <div className="flex items-center gap-2">
                    <Checkbox
                      id="st-meth-dcm"
                      checked={methylationSystems.includes('dcm')}
                      onCheckedChange={(v) =>
                        setMethylationSystems(
                          v
                            ? [...methylationSystems, 'dcm']
                            : methylationSystems.filter((s) => s !== 'dcm'),
                        )
                      }
                    />
                    <Label htmlFor="st-meth-dcm" className="cursor-pointer text-sm font-normal">
                      Dcm
                    </Label>
                  </div>
                  <div className="flex items-center gap-2">
                    <Checkbox
                      id="st-meth-ecoki"
                      checked={methylationSystems.includes('ecoki')}
                      onCheckedChange={(v) =>
                        setMethylationSystems(
                          v
                            ? [...methylationSystems, 'ecoki']
                            : methylationSystems.filter((s) => s !== 'ecoki'),
                        )
                      }
                    />
                    <Label htmlFor="st-meth-ecoki" className="cursor-pointer text-sm font-normal">
                      EcoKI
                    </Label>
                  </div>
                </div>
                <div className="flex items-center gap-2 mt-3">
                  <Label className="text-sm text-muted-foreground shrink-0">
                    Detection window:
                  </Label>
                  <span className="text-sm text-muted-foreground">±</span>
                  <Input
                    className="w-16 h-8 px-2 py-0 text-sm text-right font-mono"
                    type="number"
                    min="0"
                    max="10"
                    value={methylationOverlap}
                    onChange={(e) => {
                      if (e.target.value === '') return;
                      const v = Number(e.target.value);
                      if (Number.isFinite(v)) setMethylationOverlap(v);
                    }}
                  />
                  <span className="text-sm text-muted-foreground">bp</span>
                </div>
              </div>

              <Separator />

              {/* ── Sequence alignment ── */}
              <div>
                <div className={SECTION_TITLE}>Alignment</div>
                <div className="space-y-2">
                  <div className="flex items-center gap-2">
                    <Label className="text-sm text-muted-foreground shrink-0">Algorithm</Label>
                    <Select
                      value={alignmentAlgorithm}
                      onValueChange={(v) => onAlignmentAlgorithmChange?.(v)}
                    >
                      <SelectTrigger className="w-44 h-8 px-2 py-0 text-sm">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="blast">BLAST</SelectItem>
                        <SelectItem value="smith-waterman">Smith-Waterman</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <span className="block text-xs text-muted-foreground">
                    BLAST chains multiple colinear hits, so split or multi-hit reads align in full;
                    Smith-Waterman returns a single local block plus at most one flank
                  </span>
                </div>
              </div>

              <Separator />

              {/* ── Primer analysis ── */}
              <div>
                <div className={SECTION_TITLE}>Primer Analysis</div>
                <div className="flex items-center gap-2">
                  <Label className="text-sm text-muted-foreground">Seed region length</Label>
                  <Input
                    className="w-16 h-8 px-2 py-0 text-sm text-right font-mono"
                    type="number"
                    value={primerSeedLength}
                    min={6}
                    max={20}
                    onChange={(e) => {
                      if (e.target.value === '') return;
                      const v = Number(e.target.value);
                      if (Number.isFinite(v)) setPrimerSeedLength(v);
                    }}
                  />
                  <span className="text-sm text-muted-foreground">bp</span>
                </div>
              </div>

              <Separator />

              {/* ── Tm parameters ── */}
              <div>
                <div className={SECTION_TITLE}>Tm Calculation</div>
                <div className="space-y-2.5">
                  {[
                    {
                      key: 'naConc',
                      label: 'Na⁺',
                      unit: 'mM',
                      mult: 1000,
                      step: 1,
                      min: 0,
                      max: 500,
                    },
                    {
                      key: 'mgConc',
                      label: 'Mg²⁺',
                      unit: 'mM',
                      mult: 1000,
                      step: 0.1,
                      min: 0,
                      max: 10,
                    },
                    {
                      key: 'dntpConc',
                      label: 'dNTPs',
                      unit: 'mM',
                      mult: 1000,
                      step: 0.1,
                      min: 0,
                      max: 10,
                    },
                    {
                      key: 'trisConc',
                      label: 'Tris-HCl',
                      unit: 'mM',
                      mult: 1000,
                      step: 1,
                      min: 0,
                      max: 200,
                    },
                    {
                      key: 'primerConc',
                      label: 'Primer',
                      unit: 'nM',
                      mult: 1e9,
                      step: 50,
                      min: 0,
                      max: 5000,
                    },
                  ].map(({ key, label, unit, mult, step, min, max }) => (
                    <div key={key} className="flex items-center gap-2">
                      <Label className="text-sm text-muted-foreground w-16 shrink-0">{label}</Label>
                      <Input
                        className="w-20 h-8 px-2 py-0 text-sm text-right font-mono"
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
                      <span className="text-sm text-muted-foreground">{unit}</span>
                    </div>
                  ))}
                </div>
              </div>
            </>
          )}

          {/* ── Plugins (only when opened from the sidebar Plugins entry) ── */}
          {isPluginsView && (
            <div className="space-y-2">
              {distributablePlugins.map(renderPluginRow)}
              {builtinPlugins.length > 0 && (
                <div className="pt-1">
                  <button
                    type="button"
                    onClick={() => setShowBuiltinPlugins((v) => !v)}
                    className="flex w-full items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground hover:text-foreground"
                  >
                    {showBuiltinPlugins ? (
                      <ChevronDown className="size-3.5" />
                    ) : (
                      <ChevronRight className="size-3.5" />
                    )}
                    Built-in plugins ({builtinPlugins.length})
                  </button>
                  {showBuiltinPlugins && (
                    <div className="mt-2 space-y-2">{builtinPlugins.map(renderPluginRow)}</div>
                  )}
                </div>
              )}
              {plugins.length === 0 && (
                <div className="text-sm text-muted-foreground">No plugins installed</div>
              )}
            </div>
          )}
        </div>

        <DialogFooter>
          <DialogClose asChild>
            <Button variant="outline">Close</Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
