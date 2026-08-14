import { useState, useRef, useEffect } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogClose,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Repeat } from 'lucide-react';

// IUPAC 互补碱基对照表（含简并碱基）
const IUPAC_COMP = {
  A: 'T',
  T: 'A',
  U: 'A',
  C: 'G',
  G: 'C',
  R: 'Y',
  Y: 'R',
  S: 'S',
  W: 'W',
  K: 'M',
  M: 'K',
  B: 'V',
  D: 'H',
  H: 'D',
  V: 'B',
  N: 'N',
};

function reverseComplement(seq) {
  let result = '';
  for (let i = seq.length - 1; i >= 0; i--) {
    const ch = seq[i];
    const upper = ch.toUpperCase();
    const comp = IUPAC_COMP[upper] || upper;
    result += ch === upper ? comp : comp.toLowerCase();
  }
  return result;
}

/**
 * 去掉空白字符
 */
function stripWhitespace(s) {
  return s.replace(/\s/g, '');
}

/**
 * 只保留英文字母（IUPAC 碱基均为 A-Z），其余字符直接过滤
 */
function filterLetters(s) {
  return (s || '').replace(/[^a-zA-Z]/g, '');
}

/**
 * 标记序列中的非法字符位置 → 返回 { chars, positions } 用于显示
 */
const MODE_TITLE = {
  insert: 'Insert Sequence',
  delete: 'Delete Sequence',
  replace: 'Edit Sequence',
};

/**
 * SequenceEditDialog — 序列编辑确认弹窗
 *
 * Props:
 *   open         - boolean 是否显示
 *   mode         - 'insert' | 'delete' | 'replace'
 *   cursorIndex  - 光标位置（insert mode）
 *   selStart     - 选区起点（delete / replace mode）
 *   selEnd       - 选区终点（delete / replace mode）
 *   selectedText - 被选中的序列文本
 *   initialText  - 预填文本（例如粘贴时从剪贴板读出的内容）
 *   onConfirm    - (result: { type, sequence? }) => void
 *   onCancel     - () => void
 */
export default function SequenceEditDialog({
  open,
  mode = 'insert',
  cursorIndex = null,
  selStart = null,
  selEnd = null,
  selectedText = '',
  initialText = '',
  onConfirm,
  onCancel,
  moleculeType = 'dna',
}) {
  // Length unit: base pairs (DNA), nucleotides (ss-RNA), amino acids (protein).
  const isProtein = moleculeType === 'protein';
  const unit = moleculeType === 'dna' ? 'bp' : moleculeType === 'protein' ? 'aa' : 'nt';
  // 输入框中的文本
  const [inputText, setInputText] = useState('');
  const inputRef = useRef(null);

  // 每次打开弹窗时预填文本（粘贴场景用 initialText，手打时保持清空）
  useEffect(() => {
    if (open) {
      setInputText(filterLetters(initialText));
    }
  }, [open, initialText]);

  // 打开后自动聚焦输入框，光标移到末尾（便于接着预填字符继续输入）
  useEffect(() => {
    if (open && (mode === 'insert' || mode === 'replace')) {
      // 小延迟确保 DOM 渲染完成
      const timer = setTimeout(() => {
        const el = inputRef.current;
        if (!el) return;
        el.focus();
        const len = el.value.length;
        el.setSelectionRange(len, len);
      }, 50);
      return () => clearTimeout(timer);
    }
  }, [open, mode]);

  const cleaned = stripWhitespace(inputText);
  const insertLen = cleaned.length;
  const deleteLen =
    mode === 'delete' || mode === 'replace' ? stripWhitespace(selectedText).length : 0;
  const netChange = mode === 'replace' ? insertLen - deleteLen : 0;

  // 可提交条件：非删除模式需要内容不为空（输入已被过滤为纯字母）
  const canConfirm = mode === 'delete' || (mode !== 'delete' && inputText.trim().length > 0);

  const handleConfirm = () => {
    if (!canConfirm) return;

    const result = { type: mode };
    if (mode !== 'delete') {
      result.sequence = cleaned;
    }
    onConfirm(result);
  };

  const handleKeyDown = (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleConfirm();
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(open) => {
        if (!open) onCancel();
      }}
    >
      <DialogContent className="sm:max-w-md" onInteractOutside={(e) => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle>{MODE_TITLE[mode] || 'Edit Sequence'}</DialogTitle>
        </DialogHeader>

        <div className="space-y-3">
          {/* 光标/选区位置信息 */}
          <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
            {mode === 'insert' && cursorIndex !== null && (
              <>
                <span>Cursor</span>
                <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] font-semibold text-foreground">
                  {cursorIndex}
                </code>
              </>
            )}
            {(mode === 'delete' || mode === 'replace') && selStart !== null && selEnd !== null && (
              <>
                <span>Selection</span>
                <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] font-semibold text-foreground">
                  {selStart} – {selEnd}
                </code>
                <span className="tabular-nums">
                  ({deleteLen} {unit})
                </span>
              </>
            )}
          </div>

          {/* 删除/替换：显示被选中的序列 */}
          {(mode === 'delete' || mode === 'replace') && selectedText && (
            <div>
              <label className="text-xs font-medium text-muted-foreground block mb-1.5">
                {mode === 'delete' ? 'Region to delete' : 'Region to replace (old)'}
              </label>
              <div className="max-h-[120px] overflow-auto select-all break-all rounded-lg border border-border/70 bg-muted/50 p-2.5 font-mono text-xs leading-relaxed">
                {selectedText}
              </div>
            </div>
          )}

          {/* 插入/替换：输入新序列 */}
          {(mode === 'insert' || mode === 'replace') && (
            <div>
              <label className="text-xs font-medium text-muted-foreground block mb-1.5">
                {mode === 'insert' ? 'Sequence to insert' : 'New sequence'}
              </label>
              <textarea
                ref={inputRef}
                value={inputText}
                onChange={(e) => setInputText(filterLetters(e.target.value))}
                onKeyDown={handleKeyDown}
                placeholder={isProtein ? 'Enter amino acid sequence…' : 'Enter DNA / RNA sequence…'}
                rows={4}
                spellCheck={false}
                className="font-mono text-sm w-full rounded-lg border border-input bg-transparent p-2.5 resize-y min-h-[80px] leading-relaxed outline-none transition-shadow focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40"
              />
            </div>
          )}

          {/* 统计信息 */}
          <div className="flex flex-wrap gap-1.5 text-xs">
            {mode === 'insert' && (
              <span className="inline-flex items-center gap-1.5 rounded-md border border-emerald-200 bg-emerald-50 px-2 py-0.5 text-emerald-700">
                Insert
                <span className="font-semibold tabular-nums">
                  +{insertLen} {unit}
                </span>
              </span>
            )}
            {mode === 'delete' && (
              <span className="inline-flex items-center gap-1.5 rounded-md border border-red-200 bg-red-50 px-2 py-0.5 text-red-700">
                Delete
                <span className="font-semibold tabular-nums">
                  −{deleteLen} {unit}
                </span>
              </span>
            )}
            {mode === 'replace' && (
              <>
                <span className="inline-flex items-center gap-1.5 rounded-md border border-red-200 bg-red-50 px-2 py-0.5 text-red-700">
                  Delete
                  <span className="font-semibold tabular-nums">
                    −{deleteLen} {unit}
                  </span>
                </span>
                <span className="inline-flex items-center gap-1.5 rounded-md border border-emerald-200 bg-emerald-50 px-2 py-0.5 text-emerald-700">
                  Insert
                  <span className="font-semibold tabular-nums">
                    +{insertLen} {unit}
                  </span>
                </span>
                <span
                  className={`inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5 ${netChange > 0 ? 'border-emerald-200 bg-emerald-50 text-emerald-700' : netChange < 0 ? 'border-red-200 bg-red-50 text-red-700' : 'border-border bg-muted text-muted-foreground'}`}
                >
                  Net
                  <span className="font-semibold tabular-nums">
                    {netChange > 0 ? '+' : ''}
                    {netChange} {unit}
                  </span>
                </span>
              </>
            )}
          </div>
        </div>

        <DialogFooter className="gap-2 sm:gap-2">
          {(mode === 'insert' || mode === 'replace') &&
            inputText.trim().length > 0 &&
            !isProtein && (
              <Button
                variant="secondary"
                size="sm"
                onClick={() => setInputText(reverseComplement(inputText))}
                className="mr-auto"
              >
                <Repeat className="size-3.5" />
                Reverse Complement
              </Button>
            )}
          <DialogClose asChild>
            <Button variant="outline" onClick={onCancel}>
              Cancel
            </Button>
          </DialogClose>
          <Button
            variant={mode === 'delete' ? 'destructive' : 'default'}
            disabled={!canConfirm}
            onClick={handleConfirm}
          >
            {mode === 'insert' && 'Confirm Insertion'}
            {mode === 'delete' && 'Confirm Deletion'}
            {mode === 'replace' && 'Apply'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
