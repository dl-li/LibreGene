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
import { Settings } from 'lucide-react';

const SECTION_TITLE =
  'text-[11px] font-semibold uppercase tracking-wider text-muted-foreground mb-1.5';

export default function SettingsPage({
  open,
  onOpenChange,
  methylationSystems,
  setMethylationSystems,
  methylationOverlap,
  setMethylationOverlap,
  primerSeedLength,
  setPrimerSeedLength,
  plugins = [],
  disabledPlugins = [],
  onTogglePlugin,
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Settings className="size-4 text-muted-foreground" />
            Settings
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* ── 甲基化 ── */}
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
              <Label className="text-sm text-muted-foreground shrink-0">Detection window:</Label>
              <span className="text-sm text-muted-foreground">±</span>
              <Input
                className="w-16 h-8 px-2 py-0 text-sm text-right font-mono"
                type="number"
                min="0"
                max="10"
                value={methylationOverlap}
                onChange={(e) => setMethylationOverlap(Number(e.target.value))}
              />
              <span className="text-sm text-muted-foreground">bp</span>
            </div>
          </div>

          <Separator />

          {/* ── 引物分析 ── */}
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
                onChange={(e) => setPrimerSeedLength(Number(e.target.value))}
              />
              <span className="text-sm text-muted-foreground">bp</span>
            </div>
          </div>

          <Separator />

          {/* ── 插件 ── */}
          <div>
            <div className={SECTION_TITLE}>Plugins</div>
            <div className="space-y-2">
              {plugins.map((p) => (
                <div key={p.id} className="flex items-center gap-2">
                  <Checkbox
                    id={`st-plugin-${p.id}`}
                    checked={!disabledPlugins.includes(p.id)}
                    onCheckedChange={() => onTogglePlugin?.(p.id)}
                  />
                  <Label
                    htmlFor={`st-plugin-${p.id}`}
                    className="cursor-pointer text-sm font-normal"
                  >
                    {p.name}
                  </Label>
                </div>
              ))}
              {plugins.length === 0 && (
                <div className="text-sm text-muted-foreground">No plugins installed</div>
              )}
            </div>
          </div>
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
