import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
  DialogClose,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { AlertTriangle } from 'lucide-react';
import { fileNameOf } from '../recentFiles';
import { forceQuit } from '../tauriApi';
import McpGuideDialog from '../dialogs/McpGuideDialog';
import NewSequenceDialog from '../dialogs/NewSequenceDialog';
import FastaSplitDialog from '../dialogs/FastaSplitDialog';
import { T, useTranslation } from '@/i18n';

export default function AppDialogs({
  mcpGuideOpen,
  setMcpGuideOpen,
  mcpConfig,
  onMcpConfigChange,
  newSeqOpen,
  setNewSeqOpen,
  onCreateProject,
  onCreateMsa,
  unsavedDialog,
  onUnsavedCancel,
  onUnsavedDiscard,
  onUnsavedSave,
  dropConfirm,
  setDropConfirm,
  onDropOpenAsNew,
  onDropAddAsAlignment,
  dropResult,
  setDropResult,
  openError,
  setOpenError,
  fastaSplit,
  onFastaSplitChoice,
  quitRequest,
  setQuitRequest,
}) {
  const { t } = useTranslation();
  return (
    <>
      <McpGuideDialog
        open={mcpGuideOpen}
        onOpenChange={setMcpGuideOpen}
        mcpConfig={mcpConfig}
        onMcpConfigChange={onMcpConfigChange}
      />

      <NewSequenceDialog
        open={newSeqOpen}
        onOpenChange={setNewSeqOpen}
        onConfirm={onCreateProject}
        onCreateMsa={onCreateMsa}
      />

      {/* --- Unsaved Changes Dialog --- */}
      <Dialog
        open={unsavedDialog.open}
        onOpenChange={(open) => {
          if (!open) onUnsavedCancel();
        }}
      >
        <DialogContent className="sm:max-w-md" onInteractOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2.5">
              <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-amber-100 text-amber-600">
                <AlertTriangle className="size-4" />
              </span>
              <T>{'Unsaved Changes'}</T>
            </DialogTitle>
            <DialogDescription>
              <T>{'This project has unsaved changes. Save before continuing?'}</T>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2 sm:gap-2">
            <DialogClose asChild>
              <Button variant="outline" onClick={onUnsavedCancel}>
                <T>{'Cancel'}</T>
              </Button>
            </DialogClose>
            <Button
              variant="outline"
              className="text-destructive hover:text-destructive"
              onClick={onUnsavedDiscard}
            >
              <T>{"Don't Save"}</T>
            </Button>
            <Button onClick={onUnsavedSave}>
              <T>{'Save'}</T>
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* --- Drag-drop: add as alignment or open as new file --- */}
      <Dialog
        open={!!dropConfirm}
        onOpenChange={(open) => {
          if (!open) setDropConfirm(null);
        }}
      >
        <DialogContent className="sm:max-w-md" onInteractOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle>
              <T>{'Add as Alignment?'}</T>
            </DialogTitle>
            <DialogDescription>
              {t(
                dropConfirm?.paths.length === 1
                  ? 'Add this file to the current project as an alignment, or open it as a new project?'
                  : 'Add these files to the current project as alignments, or open them as new projects?',
              )}
            </DialogDescription>
          </DialogHeader>
          <ul className="max-h-40 overflow-auto rounded-md border border-border/60 px-3 py-2 text-xs font-mono text-muted-foreground">
            {(dropConfirm?.paths || []).map((p) => (
              <li key={p} className="truncate py-0.5" title={p}>
                {fileNameOf(p)}
              </li>
            ))}
          </ul>
          <DialogFooter className="gap-2 sm:gap-2">
            <DialogClose asChild>
              <Button variant="outline">
                <T>{'Cancel'}</T>
              </Button>
            </DialogClose>
            <Button variant="outline" onClick={onDropOpenAsNew}>
              <T>{'Open as New File'}</T>
            </Button>
            <Button onClick={onDropAddAsAlignment}>
              <T>{'Add as Alignment'}</T>
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* --- Drag-drop: per-file failure feedback --- */}
      <Dialog
        open={!!dropResult}
        onOpenChange={(open) => {
          if (!open) setDropResult(null);
        }}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2.5">
              <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-amber-100 text-amber-600">
                <AlertTriangle className="size-4" />
              </span>
              <T>{'Some Alignments Failed'}</T>
            </DialogTitle>
            <DialogDescription>
              {dropResult?.added > 0 ? (
                t('{added} added; {failed} failed:', {
                  added: dropResult.added,
                  failed: dropResult.failed.length,
                })
              ) : (
                <T>{'No alignments could be added:'}</T>
              )}
            </DialogDescription>
          </DialogHeader>
          <ul className="max-h-48 overflow-auto rounded-md border border-border/60 px-3 py-2 text-xs text-muted-foreground">
            {(dropResult?.failed || []).map((f) => (
              <li key={f.path} className="py-1">
                <span className="font-mono">{fileNameOf(f.path)}</span>
                <span className="block text-destructive/80">{f.error}</span>
              </li>
            ))}
          </ul>
          <DialogFooter>
            <DialogClose asChild>
              <Button>
                <T>{'OK'}</T>
              </Button>
            </DialogClose>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      {/* --- Open File: per-file failure feedback --- */}
      <Dialog
        open={!!openError}
        onOpenChange={(open) => {
          if (!open) setOpenError(null);
        }}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2.5">
              <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-amber-100 text-amber-600">
                <AlertTriangle className="size-4" />
              </span>
              {openError?.length > 1 ? (
                <T>{'Some Files Failed to Open'}</T>
              ) : (
                <T>{'Failed to Open File'}</T>
              )}
            </DialogTitle>
          </DialogHeader>
          <ul className="max-h-48 overflow-auto rounded-md border border-border/60 px-3 py-2 text-xs text-muted-foreground">
            {(openError || []).map((f) => (
              <li key={f.path} className="py-1">
                <span className="font-mono">{fileNameOf(f.path)}</span>
                <span className="block text-destructive/80">{f.error}</span>
              </li>
            ))}
          </ul>
          <DialogFooter>
            <DialogClose asChild>
              <Button>
                <T>{'OK'}</T>
              </Button>
            </DialogClose>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      {/* --- Multi-record FASTA: split into separate projects? --- */}
      <FastaSplitDialog target={fastaSplit} onChoice={onFastaSplitChoice} />
      {/* --- Tray Quit: unsaved-changes confirmation --- */}
      <Dialog
        open={!!quitRequest}
        onOpenChange={(open) => {
          if (!open) setQuitRequest(null);
        }}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2.5">
              <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-amber-100 text-amber-600">
                <AlertTriangle className="size-4" />
              </span>
              <T>{'Quit LibreGene?'}</T>
            </DialogTitle>
            <DialogDescription>
              <T>{'Unsaved changes will be lost:'}</T>
            </DialogDescription>
          </DialogHeader>
          <ul className="max-h-48 overflow-auto rounded-md border border-border/60 px-3 py-2 text-xs text-muted-foreground">
            {(quitRequest || []).map((id) => (
              <li key={id} className="py-1 font-mono">
                {fileNameOf(id)}
              </li>
            ))}
          </ul>
          <DialogFooter>
            <DialogClose asChild>
              <Button variant="outline">
                <T>{'Cancel'}</T>
              </Button>
            </DialogClose>
            <Button variant="destructive" onClick={() => forceQuit().catch(() => {})}>
              <T>{'Quit Anyway'}</T>
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
