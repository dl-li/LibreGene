import { useState } from 'react';
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { Input } from '@/components/ui/input';
import { Separator } from '@/components/ui/separator';
import { FolderOpen } from 'lucide-react';

function NumInput({ label, value, onChange, min, max, step }) {
  return (
    <div className="flex items-center justify-between gap-2">
      <Label className="text-xs font-normal text-muted-foreground w-36 shrink-0 truncate" title={label}>{label}</Label>
      <Input className="w-16 h-7 px-1.5 py-0 text-xs text-right font-mono" type="number"
        value={value} min={min} max={max} step={step || 1}
        onChange={e => onChange(Number(e.target.value))} />
    </div>
  );
}

function CollapsibleSection({ title, defaultOpen, children }) {
  const [open, setOpen] = useState(defaultOpen !== false);
  return (
    <div>
      <button
        className="flex items-center gap-1.5 w-full text-xs font-medium text-foreground py-1.5 hover:text-primary transition-colors cursor-pointer"
        onClick={() => setOpen(!open)}>
        <span className="text-muted-foreground text-[10px]">{open ? '▾' : '▸'}</span>
        {title}
      </button>
      {open && <div className="space-y-1.5 mt-1 mb-3">{children}</div>}
    </div>
  );
}

export default function DebugPanel({
  open,
  onOpenChange,
  showFeatures, setShowFeatures, features,
  showPrimers, setShowPrimers, primers,
  enzymeFilter, setEnzymeFilter, enzymes, displayEnzymes,
  methylationSystems, setMethylationSystems,
  methylationOverlap, setMethylationOverlap,
  isTauri, openPath, setOpenPath, fileStatus,
  layoutParams, setLP,
  primerSeedLength, setPrimerSeedLength,
  onOpenFile,
}) {
  const lp = layoutParams;

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" className="w-80 sm:max-w-xs overflow-y-auto" showCloseButton={true}>
        <SheetHeader className="px-0 pt-2 pb-1">
          <SheetTitle className="text-base">Debug 面板</SheetTitle>
        </SheetHeader>

        <div className="space-y-3 text-sm">
          {/* ── 显示开关 ── */}
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2">
              <Checkbox id="dp-features" checked={showFeatures}
                onCheckedChange={(v) => setShowFeatures(!!v)} />
              <Label htmlFor="dp-features" className="cursor-pointer text-xs">特征 ({features.length})</Label>
            </div>
            <div className="flex items-center gap-2">
              <Checkbox id="dp-primers" checked={showPrimers}
                onCheckedChange={(v) => setShowPrimers(!!v)} />
              <Label htmlFor="dp-primers" className="cursor-pointer text-xs">引物 ({primers.length})</Label>
            </div>
          </div>

          <Separator />

          {/* ── 文件加载 ── */}
          <div>
            <div className="text-xs font-medium mb-1.5">文件</div>
            <Button variant="default" size="sm" className="w-full h-8 text-xs" onClick={onOpenFile}>
              <FolderOpen className="h-3.5 w-3.5 mr-1.5" /> 打开文件
            </Button>
            {!isTauri && (
              <>
                <Input className="w-full text-[11px] h-7 mt-1.5 font-mono" type="text"
                  value={openPath} onChange={e => setOpenPath(e.target.value)}
                  placeholder="/path/to/file.gbk" />
                <div className="flex flex-wrap gap-1 mt-1.5">
                  {['test/pUC-GW-Amp.gb', 'test/flySWARM.dna'].map(f => (
                    <Button key={f} variant="secondary" size="sm" className="text-[10px] h-6 px-2"
                      onClick={() => setOpenPath('/Users/lidonglin/Documents/Geneie/' + f)}>{f}</Button>
                  ))}
                </div>
              </>
            )}
            {fileStatus && <div className="text-[11px] mt-1 text-muted-foreground">{fileStatus}</div>}
          </div>

          <Separator />

          {/* ── 酶过滤器 ── */}
          <div>
            <div className="text-xs font-medium mb-1.5">
              酶切位点 ({enzymes.length} 总, {displayEnzymes.length} 显示)
            </div>
            <select className="flex h-8 w-full rounded-md border border-input bg-background px-2 py-1 text-xs ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
              value={enzymeFilter} onChange={e => setEnzymeFilter(e.target.value)}>
              <option value="unique">仅唯一切点</option>
              <option value="all">全部有切点</option>
              <optgroup label="按粘性末端">
                <option value="blunt">平末端</option>
                <option value="overhang5">5′ 粘性</option>
                <option value="overhang3">3′ 粘性</option>
              </optgroup>
              <optgroup label="按识别长度">
                <option value="unique6">唯一 6 bp</option>
                <option value="rec4">4 bp</option>
                <option value="rec5">5 bp</option>
                <option value="rec6">6 bp</option>
                <option value="rec8p">≥8 bp</option>
              </optgroup>
              <optgroup label="特殊">
                <option value="iis">Type IIS</option>
              </optgroup>
            </select>
          </div>

          {/* ── 甲基化 ── */}
          <div>
            <div className="text-xs font-medium mb-1.5">甲基化</div>
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <Checkbox id="dp-meth-dam" checked={methylationSystems.includes('dam')}
                  onCheckedChange={(v) => setMethylationSystems(v
                    ? [...methylationSystems, 'dam']
                    : methylationSystems.filter(s => s !== 'dam'))} />
                <Label htmlFor="dp-meth-dam" className="cursor-pointer text-xs">Dam</Label>
              </div>
              <div className="flex items-center gap-2">
                <Checkbox id="dp-meth-dcm" checked={methylationSystems.includes('dcm')}
                  onCheckedChange={(v) => setMethylationSystems(v
                    ? [...methylationSystems, 'dcm']
                    : methylationSystems.filter(s => s !== 'dcm'))} />
                <Label htmlFor="dp-meth-dcm" className="cursor-pointer text-xs">Dcm</Label>
              </div>
              <div className="flex items-center gap-2">
                <Checkbox id="dp-meth-ecoki" checked={methylationSystems.includes('ecoki')}
                  onCheckedChange={(v) => setMethylationSystems(v
                    ? [...methylationSystems, 'ecoki']
                    : methylationSystems.filter(s => s !== 'ecoki'))} />
                <Label htmlFor="dp-meth-ecoki" className="cursor-pointer text-xs">EcoKI</Label>
              </div>
            </div>
            <div className="flex items-center gap-2 mt-1.5">
              <Label className="text-xs text-muted-foreground">重叠：</Label>
              <Input className="w-14 h-7 px-1 py-0 text-xs text-right font-mono" type="number"
                min="0" max="10" value={methylationOverlap}
                onChange={e => setMethylationOverlap(Number(e.target.value))} />
              <span className="text-[11px] text-muted-foreground">bp</span>
            </div>
          </div>

          <Separator />

          {/* ── 引物分析 ── */}
          <div>
            <div className="text-xs font-medium mb-1.5">引物分析</div>
            <NumInput label="种子区长度 (bp)" value={primerSeedLength}
              onChange={setPrimerSeedLength} min={6} max={20} />
          </div>

          <Separator />

          {/* ── 排版参数 ── */}

          {/* 行距 */}
          <CollapsibleSection title="行距">
            <NumInput label="最小行间距 (px)" value={lp.minRowGap} onChange={v => setLP('minRowGap', v)} min={8} max={80} />
            <NumInput label="行内容间距 (px)" value={lp.rowContentGap} onChange={v => setLP('rowContentGap', v)} min={0} max={40} />
            <NumInput label="上方交互区 (px)" value={lp.minAboveSpace} onChange={v => setLP('minAboveSpace', v)} min={8} max={60} />
            <NumInput label="下方交互区 (px)" value={lp.minBelowSpace} onChange={v => setLP('minBelowSpace', v)} min={0} max={40} />
          </CollapsibleSection>

          {/* 特征排版 */}
          <CollapsibleSection title="特征排版">
            <NumInput label="特征行高 (px)" value={lp.featTrackHeight} onChange={v => setLP('featTrackHeight', v)} min={6} max={40} />
            <NumInput label="特征距序列 (px)" value={lp.featBaseOffset} onChange={v => setLP('featBaseOffset', v)} min={4} max={40} />
            <NumInput label="特征标签内边距 (px)" value={lp.featLabelPad} onChange={v => setLP('featLabelPad', v)} min={0} max={30} />
          </CollapsibleSection>

          {/* 酶切位点排版 */}
          <CollapsibleSection title="酶切位点排版">
            <NumInput label="酶标签行高 (px)" value={lp.enzTrackHeight} onChange={v => setLP('enzTrackHeight', v)} min={6} max={40} />
            <NumInput label="酶连线到序列 (px)" value={lp.enzLineGap} onChange={v => setLP('enzLineGap', v)} min={4} max={40} />
            <NumInput label="酶标签基位 (px)" value={lp.enzLabelBase} onChange={v => setLP('enzLabelBase', v)} min={20} max={100} />
            <NumInput label="酶区域上边距 (px)" value={lp.enzAbovePad} onChange={v => setLP('enzAbovePad', v)} min={0} max={30} />
          </CollapsibleSection>

          {/* F 引物排版 */}
          <CollapsibleSection title="F 引物排版">
            <NumInput label="匹配线距序列 (px)" value={lp.fwdMatchY} onChange={v => setLP('fwdMatchY', v)} min={0} max={100} />
            <NumInput label="碱基文字 Y (px)" value={lp.fwdBaseTextY} onChange={v => setLP('fwdBaseTextY', v)} min={0} max={40} />
            <NumInput label="名称标签 Y (px)" value={lp.fwdLabelY} onChange={v => setLP('fwdLabelY', v)} min={0} max={40} />
            <NumInput label="上方基础间距 (px)" value={lp.fwdAboveBase} onChange={v => setLP('fwdAboveBase', v)} min={0} max={80} />
            <NumInput label="尾部附加间距 (px)" value={lp.fwdAboveExtra} onChange={v => setLP('fwdAboveExtra', v)} min={0} max={60} />
            <NumInput label="无尾部间距 (px)" value={lp.fwdAboveNonTailExtra} onChange={v => setLP('fwdAboveNonTailExtra', v)} min={0} max={30} />
          </CollapsibleSection>

          {/* R 引物排版 */}
          <CollapsibleSection title="R 引物排版">
            <NumInput label="匹配线距序列 (px)" value={lp.revMatchY} onChange={v => setLP('revMatchY', v)} min={0} max={100} />
            <NumInput label="碱基文字 Y (px)" value={lp.revBaseTextY} onChange={v => setLP('revBaseTextY', v)} min={0} max={40} />
            <NumInput label="名称标签 Y (px)" value={lp.revLabelY} onChange={v => setLP('revLabelY', v)} min={0} max={40} />
            <NumInput label="下方基础间距 (px)" value={lp.revBelowBase} onChange={v => setLP('revBelowBase', v)} min={0} max={80} />
            <NumInput label="尾部附加间距 (px)" value={lp.revBelowExtra} onChange={v => setLP('revBelowExtra', v)} min={0} max={60} />
            <NumInput label="无尾部间距 (px)" value={lp.revBelowNonTailExtra} onChange={v => setLP('revBelowNonTailExtra', v)} min={0} max={30} />
          </CollapsibleSection>

          {/* 引物通用 */}
          <CollapsibleSection title="引物通用">
            <NumInput label="轨道间距 (px)" value={lp.trackGap} onChange={v => setLP('trackGap', v)} min={0} max={80} />
            <NumInput label="错配文字 Y 偏移 (px)" value={lp.misYDelta} onChange={v => setLP('misYDelta', v)} min={0} max={20} />
            <NumInput label="悬停展开 (px)" value={lp.hoverExpand} onChange={v => setLP('hoverExpand', v)} min={0} max={60} />
            <NumInput label="箭头长度 (px)" value={lp.arrowHeadLen} onChange={v => setLP('arrowHeadLen', v)} min={0} max={20} />
            <NumInput label="箭头高度 (px)" value={lp.arrowHeadHeight} onChange={v => setLP('arrowHeadHeight', v)} min={0} max={20} />
          </CollapsibleSection>
        </div>
      </SheetContent>
    </Sheet>
  );
}
