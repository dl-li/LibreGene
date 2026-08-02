import { useState, useCallback } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { X, Copy, Check, FileDown, Plus, FileUp, Trash2 } from 'lucide-react';
import { parseEnzymeText, exportEnzymeText } from './myEnzymes';
import { saveTextDialog, writeTextFile } from './tauriApi';

export default function MyEnzymesDialog({ open, onOpenChange, enzymes = [], onChange }) {
  const [importText, setImportText] = useState('');
  const [singleName, setSingleName] = useState('');
  const [copied, setCopied] = useState(false);

  const handleAddParsed = useCallback(
    (parsed) => {
      if (!parsed.length) return;
      onChange?.([...enzymes, ...parsed]);
      setImportText('');
    },
    [enzymes, onChange],
  );

  const handleAddSingle = useCallback(() => {
    const parsed = parseEnzymeText(singleName);
    if (parsed.length) handleAddParsed(parsed);
    setSingleName('');
  }, [singleName, handleAddParsed]);

  const handleImport = useCallback(() => {
    handleAddParsed(parseEnzymeText(importText));
  }, [importText, handleAddParsed]);

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(exportEnzymeText(enzymes)).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    });
  }, [enzymes]);

  const handleSave = useCallback(async () => {
    const path = await saveTextDialog('my-enzymes.txt');
    if (!path) return;
    try {
      await writeTextFile(path, exportEnzymeText(enzymes));
    } catch (e) {
      console.error('export enzymes error:', e);
    }
  }, [enzymes]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl max-h-[80vh] flex flex-col px-8">
        <DialogHeader>
          <DialogTitle>My Enzymes ({enzymes.length})</DialogTitle>
        </DialogHeader>

        <div className="flex-1 overflow-auto -mx-8 px-8">
          {enzymes.length === 0 ? (
            <div className="py-8 text-center text-sm text-muted-foreground">
              No enzymes in your set yet. Add names below (e.g. EcoRI, BamHI, NotI).
            </div>
          ) : (
            <div className="flex flex-wrap gap-1.5 pb-4">
              {enzymes.map((name) => (
                <span
                  key={name}
                  className="inline-flex items-center gap-1 rounded-md border border-border bg-muted px-2 py-1 font-mono text-xs"
                >
                  {name}
                  <button
                    type="button"
                    title="Remove"
                    onClick={() => onChange?.(enzymes.filter((e) => e !== name))}
                    className="rounded p-px text-muted-foreground transition-colors hover:text-destructive"
                  >
                    <X className="size-3" />
                  </button>
                </span>
              ))}
            </div>
          )}

          <div className="border-t border-border/60 pt-3">
            <div className="mb-1.5 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              Add enzyme
            </div>
            <div className="flex items-center gap-2">
              <input
                value={singleName}
                onChange={(e) => setSingleName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleAddSingle();
                }}
                placeholder="EcoRI, BamHI, NotI…"
                spellCheck={false}
                className="h-8 min-w-0 flex-1 rounded-md border border-input bg-transparent px-3 text-sm outline-none transition-shadow focus:border-ring focus:ring-[3px] focus:ring-ring/50"
              />
              <Button
                size="sm"
                variant="outline"
                onClick={handleAddSingle}
                disabled={!singleName.trim()}
              >
                <Plus className="size-3.5" /> Add
              </Button>
            </div>

            <div className="mb-1 mt-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              Import from text
            </div>
            <div className="flex items-start gap-2">
              <textarea
                value={importText}
                onChange={(e) => setImportText(e.target.value)}
                placeholder={'Paste enzyme names, comma separated:\nAfeI, AgeI, ApaI, AscI, AseI'}
                spellCheck={false}
                rows={3}
                className="min-w-0 flex-1 resize-none rounded-md border border-input bg-transparent px-3 py-2 font-mono text-xs outline-none transition-shadow focus:border-ring focus:ring-[3px] focus:ring-ring/50"
              />
              <Button
                size="sm"
                variant="outline"
                onClick={handleImport}
                disabled={!importText.trim()}
              >
                <FileUp className="size-3.5" /> Import
              </Button>
            </div>
          </div>
        </div>

        <DialogFooter className="mt-1">
          <div className="flex w-full items-center gap-2">
            {enzymes.length > 0 && (
              <Button
                variant="ghost"
                size="sm"
                className="mr-auto text-destructive hover:bg-destructive/10 hover:text-destructive"
                onClick={() => onChange?.([])}
              >
                <Trash2 className="size-3.5" /> Clear All
              </Button>
            )}
            <Button variant="outline" size="sm" onClick={handleCopy} disabled={!enzymes.length}>
              {copied ? (
                <Check className="size-3.5 text-emerald-500" />
              ) : (
                <Copy className="size-3.5" />
              )}
              {copied ? 'Copied' : 'Copy'}
            </Button>
            <Button variant="outline" size="sm" onClick={handleSave} disabled={!enzymes.length}>
              <FileDown className="size-3.5" /> Export
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
