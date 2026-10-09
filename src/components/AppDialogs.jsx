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

export default function AppDialogs({
  mcpGuideOpen,
  setMcpGuideOpen,
  mcpConfig,
  onMcpConfigChange,
  newSeqOpen,
  setNewSeqOpen,
  onCreateProject,
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
              Unsaved Changes
            </DialogTitle>
            <DialogDescription>
              This project has unsaved changes. Save before continuing?
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2 sm:gap-2">
            <DialogClose asChild>
              <Button variant="outline" onClick={onUnsavedCancel}>
                Cancel
              </Button>
            </DialogClose>
            <Button
              variant="outline"
              className="text-destructive hover:text-destructive"
              onClick={onUnsavedDiscard}
            >
              Don't Save
            </Button>
            <Button onClick={onUnsavedSave}>Save</Button>
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
            <DialogTitle>Add as Alignment?</DialogTitle>
            <DialogDescription>
              Add {dropConfirm?.paths.length === 1 ? 'this file' : 'these files'} to the current
              project as {dropConfirm?.paths.length === 1 ? 'an alignment' : 'alignments'}, or open
              as new {dropConfirm?.paths.length === 1 ? 'project' : 'projects'}?
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
              <Button variant="outline">Cancel</Button>
            </DialogClose>
            <Button variant="outline" onClick={onDropOpenAsNew}>
              Open as New File
            </Button>
            <Button onClick={onDropAddAsAlignment}>Add as Alignment</Button>
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
              Some Alignments Failed
            </DialogTitle>
            <DialogDescription>
              {dropResult?.added > 0
                ? `${dropResult.added} added, ${dropResult.failed.length} failed:`
                : 'No alignments could be added:'}
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
              <Button>OK</Button>
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
              {openError?.length > 1 ? 'Some Files Failed to Open' : 'Failed to Open File'}
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
              <Button>OK</Button>
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
              Quit LibreGene?
            </DialogTitle>
            <DialogDescription>Unsaved changes will be lost:</DialogDescription>
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
              <Button variant="outline">Cancel</Button>
            </DialogClose>
            <Button variant="destructive" onClick={() => forceQuit().catch(() => {})}>
              Quit Anyway
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
