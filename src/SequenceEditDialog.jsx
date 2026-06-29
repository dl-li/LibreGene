import React, { useState, useRef, useEffect } from 'react'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogClose,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { AlertTriangle, Info } from 'lucide-react'

// IUPAC 核苷酸字符集（含简并碱基）
const IUPAC_BASES = new Set('ATGCURYSWKMBDHVN')

// IUPAC 互补碱基对照表（含简并碱基）
const IUPAC_COMP = {
  'A': 'T', 'T': 'A', 'U': 'A', 'C': 'G', 'G': 'C',
  'R': 'Y', 'Y': 'R',
  'S': 'S', 'W': 'W',
  'K': 'M', 'M': 'K',
  'B': 'V', 'D': 'H', 'H': 'D', 'V': 'B',
  'N': 'N',
}

function reverseComplement(seq) {
  let result = ''
  for (let i = seq.length - 1; i >= 0; i--) {
    const ch = seq[i]
    const upper = ch.toUpperCase()
    const comp = IUPAC_COMP[upper] || upper
    result += ch === upper ? comp : comp.toLowerCase()
  }
  return result
}

/**
 * 去掉空白字符
 */
function stripWhitespace(s) {
  return s.replace(/\s/g, '')
}

/**
 * 返回输入字符串中的非法字符列表（去重、保留大小写显示）
 */
function getInvalidChars(s) {
  const seen = new Set()
  for (const ch of s) {
    if (ch.trim() && !IUPAC_BASES.has(ch.toUpperCase())) {
      seen.add(ch)
    }
  }
  return [...seen]
}

/**
 * 标记序列中的非法字符位置 → 返回 { chars, positions } 用于显示
 */
function getInvalidCharDetails(s) {
  const results = []
  for (let i = 0; i < s.length; i++) {
    const ch = s[i]
    if (ch.trim() && !IUPAC_BASES.has(ch.toUpperCase())) {
      results.push({ char: ch, pos: i })
    }
  }
  return results
}

const MODE_TITLE = {
  insert: 'Insert Sequence',
  delete: 'Delete Sequence',
  replace: 'Edit Sequence',
}

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
}) {
  // 输入框中的文本
  const [inputText, setInputText] = useState('')
  const inputRef = useRef(null)

  // 每次打开弹窗时预填文本（粘贴场景用 initialText，手打时保持清空）
  useEffect(() => {
    if (open) {
      setInputText(initialText)
    }
  }, [open, initialText])

  // 打开后自动聚焦输入框
  useEffect(() => {
    if (open && (mode === 'insert' || mode === 'replace')) {
      // 小延迟确保 DOM 渲染完成
      const timer = setTimeout(() => {
        inputRef.current?.focus()
      }, 50)
      return () => clearTimeout(timer)
    }
  }, [open, mode])

  const cleaned = stripWhitespace(inputText)
  const insertLen = cleaned.length
  const deleteLen = mode === 'delete' || mode === 'replace' ? stripWhitespace(selectedText).length : 0
  const netChange = mode === 'replace' ? insertLen - deleteLen : 0

  const invalidChars = mode !== 'delete' && inputText ? getInvalidChars(inputText) : []
  const hasInvalid = invalidChars.length > 0

  // 可提交条件：非删除模式需要内容不为空且无非法字符
  const canConfirm = mode === 'delete' || (mode !== 'delete' && inputText.trim().length > 0 && !hasInvalid)

  const handleConfirm = () => {
    if (!canConfirm) return

    const result = { type: mode }
    if (mode !== 'delete') {
      result.sequence = cleaned
    }
    onConfirm(result)
  }

  const handleKeyDown = (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleConfirm()
    }
  }

  return (
    <Dialog open={open} onOpenChange={(open) => { if (!open) onCancel() }}>
      <DialogContent className="sm:max-w-md" onInteractOutside={(e) => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle>{MODE_TITLE[mode] || 'Edit Sequence'}</DialogTitle>
        </DialogHeader>

        <div className="space-y-3">

          {/* 光标/选区位置信息 */}
          <div className="text-xs text-muted-foreground">
            {mode === 'insert' && cursorIndex !== null && (
              <span>Cursor: <code className="font-mono text-foreground">{cursorIndex}</code></span>
            )}
            {(mode === 'delete' || mode === 'replace') && selStart !== null && selEnd !== null && (
              <span>
                Selection: <code className="font-mono text-foreground">{selStart} – {selEnd}</code>
                {' '}({deleteLen} bp)
              </span>
            )}
          </div>

          {/* 删除/替换：显示被选中的序列 */}
          {(mode === 'delete' || mode === 'replace') && selectedText && (
            <div>
              <label className="text-xs text-muted-foreground block mb-1">
                {mode === 'delete' ? 'Region to delete:' : 'Region to replace (old):'}
              </label>
              <div className="font-mono text-xs bg-muted rounded p-2 break-all leading-relaxed select-all overflow-x-auto max-h-[120px] overflow-y-auto border">
                {selectedText}
              </div>
            </div>
          )}

          {/* 插入/替换：输入新序列 */}
          {(mode === 'insert' || mode === 'replace') && (
            <div>
              <label className="text-xs text-muted-foreground block mb-1">
                {mode === 'insert' ? 'Enter sequence to insert:' : 'Enter new sequence:'}
              </label>
              <textarea
                ref={inputRef}
                value={inputText}
                onChange={(e) => setInputText(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder="Enter DNA / RNA sequence…"
                rows={4}
                spellCheck={false}
                className="font-mono text-sm w-full rounded-md border bg-transparent p-2 resize-y min-h-[80px] leading-relaxed outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
              />
            </div>
          )}

          {/* 统计信息 */}
          <div className="flex flex-wrap gap-3 text-xs">
            {mode === 'insert' && (
              <span className="inline-flex items-center gap-1">
                <span className="text-muted-foreground">Insert</span>
                {' '}
                <span className="font-semibold text-emerald-600">{insertLen} bp</span>
              </span>
            )}
            {mode === 'delete' && (
              <span className="inline-flex items-center gap-1">
                <span className="text-muted-foreground">Delete</span>
                {' '}
                <span className="font-semibold text-destructive">{deleteLen} bp</span>
              </span>
            )}
            {mode === 'replace' && (
              <>
                <span className="inline-flex items-center gap-1">
                  <span className="text-muted-foreground">Delete</span>
                  {' '}
                  <span className="font-semibold text-destructive">{deleteLen} bp</span>
                </span>
                <span className="text-muted-foreground">|</span>
                <span className="inline-flex items-center gap-1">
                  <span className="text-muted-foreground">Insert</span>
                  {' '}
                  <span className="font-semibold text-emerald-600">{insertLen} bp</span>
                </span>
                <span className="text-muted-foreground">|</span>
                <span className="inline-flex items-center gap-1">
                  <span className="text-muted-foreground">Net</span>
                  {' '}
                  <span className={`font-semibold ${netChange > 0 ? 'text-emerald-600' : netChange < 0 ? 'text-destructive' : ''}`}>
                    {netChange > 0 ? '+' : ''}{netChange}
                  </span>{' '}
                  <span className="text-muted-foreground">bp</span>
                </span>
              </>
            )}
          </div>

          {/* 非法字符警告 */}
          {hasInvalid && (
            <div className="flex items-start gap-2 rounded-md bg-amber-50 dark:bg-amber-950/30 border border-amber-200 dark:border-amber-800 p-2 text-xs text-amber-700 dark:text-amber-300">
              <AlertTriangle className="size-3.5 mt-0.5 shrink-0" />
              <div>
                <span className="font-medium">Non-standard characters:</span>{' '}
                {invalidChars.map((ch, i) => (
                  <code key={i} className="mx-0.5 px-1 bg-amber-100 dark:bg-amber-900 rounded text-[11px]">
                    {'`'}{ch}{'`'}
                  </code>
                ))}
              </div>
            </div>
          )}

        </div>

        <DialogFooter className="gap-2 sm:gap-0">
          {(mode === 'insert' || mode === 'replace') && inputText.trim().length > 0 && !hasInvalid && (
            <button
              onClick={() => setInputText(reverseComplement(inputText))}
              style={{
                fontSize: '12px', fontWeight: 600,
                padding: '4px 12px', cursor: 'pointer',
                background: '#f3f4f6', color: '#1f2937',
                border: '1px solid #d1d5db', borderRadius: 4,
              }}
              className="hover:bg-gray-200 transition-colors mr-auto"
            >Reverse Complement</button>
          )}
          <DialogClose asChild>
            <Button variant="outline" onClick={onCancel}>Cancel</Button>
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
  )
}
