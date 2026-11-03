import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  AlignLeft,
  Columns2,
  Copy,
  Download,
  FilePlus2,
  Grid2X2,
  LoaderCircle,
  Redo2,
  Rows3,
  Save,
  Sigma,
  Undo2,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  alignMsa,
  exportMsaFasta,
  getProjectById,
  isTauri,
  saveFile,
  saveFileDialog,
  updateMsa,
} from '../tauriApi';
import MsaEditor from './MsaEditor';
import { AddSequencesDialog, MsaSequencesDialog, PairwiseStatsDialog } from './MsaDialogs';
import {
  averagePairwiseIdentity,
  computePairwiseMatrix,
  detectMsaMoleculeType,
  selectionToFasta,
} from './msaAnalysis';

function snapshotOf(data) {
  return {
    rows: data.msaRows || data.rows || [],
    moleculeType: data.moleculeType || 'dna',
    aligned: data.msaAligned ?? data.aligned ?? false,
    referenceRowId: data.referenceRowId || null,
  };
}

function snapshotKey(snapshot) {
  return JSON.stringify({
    rows: snapshot.rows,
    moleculeType: snapshot.moleculeType,
    aligned: snapshot.aligned,
  });
}

function isMsaPath(path) {
  return /\.msa\.fasta?$/i.test(path || '');
}

export default function MsaWorkspace({
  projectId,
  projectName,
  initialData,
  hidden,
  viewMode,
  onViewModeChange,
  agentLocked,
  onDirtyChange,
  registerHandle,
  onProjectsSync,
  onRekey,
  leftViewportInset = 0,
}) {
  const [rows, setRows] = useState(initialData?.msaRows || []);
  const [moleculeType, setMoleculeType] = useState(initialData?.moleculeType || 'dna');
  const [aligned, setAligned] = useState(initialData?.msaAligned || false);
  const [referenceRowId, setReferenceRowId] = useState(initialData?.referenceRowId || null);
  const [loading, setLoading] = useState(!initialData);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [dirty, setDirty] = useState(initialData?.dirty ?? projectId.startsWith('untitled-'));
  const [historyVersion, setHistoryVersion] = useState(0);
  const [colorMode, setColorMode] = useState('base');
  const [selection, setSelection] = useState(null);
  const [hiddenRowIds, setHiddenRowIds] = useState(() => new Set());
  const [addOpen, setAddOpen] = useState(false);
  const [sequencesOpen, setSequencesOpen] = useState(false);
  const [statsOpen, setStatsOpen] = useState(false);
  const projectIdRef = useRef(projectId);
  const dirtyRef = useRef(dirty);
  const baselineRef = useRef(snapshotKey(snapshotOf(initialData || {})));
  const historyRef = useRef({ stack: [snapshotOf(initialData || {})], index: 0 });
  const selectionAnchorRef = useRef(null);
  const currentSnapshotRef = useRef(null);
  currentSnapshotRef.current = { rows, moleculeType, aligned, referenceRowId };

  const updateDirty = useCallback((snapshot) => {
    const next =
      projectIdRef.current.startsWith('untitled-') || snapshotKey(snapshot) !== baselineRef.current;
    dirtyRef.current = next;
    setDirty(next);
  }, []);

  const applySnapshot = useCallback((snapshot) => {
    setRows(snapshot.rows || []);
    setMoleculeType(snapshot.moleculeType || 'dna');
    setAligned(!!snapshot.aligned);
    setReferenceRowId(snapshot.referenceRowId || null);
    setSelection(null);
    selectionAnchorRef.current = null;
  }, []);

  useEffect(() => {
    projectIdRef.current = projectId;
  }, [projectId]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    const load = initialData?.msaRows
      ? Promise.resolve(initialData)
      : getProjectById(projectId, 'all');
    load
      .then((data) => {
        if (cancelled) return;
        if (!data || data.error) {
          setError(data?.error || 'Failed to load MSA');
          return;
        }
        const snapshot = snapshotOf(data);
        applySnapshot(snapshot);
        historyRef.current = { stack: [snapshot], index: 0 };
        baselineRef.current = snapshotKey(snapshot);
        const nextDirty = data.dirty === true || projectId.startsWith('untitled-');
        dirtyRef.current = nextDirty;
        setDirty(nextDirty);
        setHistoryVersion((version) => version + 1);
      })
      .catch((loadError) => {
        if (!cancelled) setError(loadError?.message || String(loadError));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [projectId, initialData, applySnapshot]);

  useEffect(() => {
    onDirtyChange(projectId, dirty);
  }, [projectId, dirty, onDirtyChange]);

  const commit = useCallback(
    async (snapshot, historyMode = 'push') => {
      if (agentLocked) return { ok: false, error: 'Project is locked by an agent' };
      try {
        const data = await updateMsa(
          projectIdRef.current,
          snapshot.rows,
          snapshot.moleculeType,
          snapshot.aligned,
          snapshot.referenceRowId,
        );
        if (!data || data.error) return { ok: false, error: data?.error || 'MSA update failed' };
        const next = snapshotOf(data);
        applySnapshot(next);
        if (historyMode === 'push') {
          const history = historyRef.current;
          history.stack = history.stack.slice(0, history.index + 1);
          history.stack.push(next);
          if (history.stack.length > 50) history.stack.shift();
          history.index = history.stack.length - 1;
        }
        setHistoryVersion((version) => version + 1);
        updateDirty(next);
        if (data.projects) onProjectsSync(data.projects);
        setError('');
        return { ok: true };
      } catch (commitError) {
        return { ok: false, error: commitError?.message || String(commitError) };
      }
    },
    [agentLocked, applySnapshot, onProjectsSync, updateDirty],
  );

  const runAndReport = useCallback(async (operation) => {
    const result = await operation();
    if (!result?.ok) setError(result?.error || 'Operation failed');
    return result;
  }, []);

  const handleAlign = useCallback(async () => {
    if (busy || agentLocked) return;
    setBusy(true);
    setError('');
    try {
      const data = await alignMsa(projectIdRef.current);
      if (!data || data.error) {
        setError(data?.error || 'Alignment failed');
        return;
      }
      const next = {
        ...snapshotOf(data),
        referenceRowId: referenceRowId || data.referenceRowId || null,
      };
      applySnapshot(next);
      const history = historyRef.current;
      history.stack = history.stack.slice(0, history.index + 1);
      history.stack.push(next);
      if (history.stack.length > 50) history.stack.shift();
      history.index = history.stack.length - 1;
      setHistoryVersion((version) => version + 1);
      updateDirty(next);
      if (data.projects) onProjectsSync(data.projects);
      setNotice('Aligned with MUSCLE 5.3');
    } catch (alignError) {
      setError(alignError?.message || String(alignError));
    } finally {
      setBusy(false);
    }
  }, [busy, agentLocked, referenceRowId, applySnapshot, onProjectsSync, updateDirty]);

  const handleUndo = useCallback(async () => {
    const history = historyRef.current;
    if (busy || history.index <= 0) return;
    const targetIndex = history.index - 1;
    const result = await commit(history.stack[targetIndex], 'none');
    if (result.ok) {
      history.index = targetIndex;
      setHistoryVersion((version) => version + 1);
    } else {
      setError(result.error);
    }
  }, [busy, commit]);

  const handleRedo = useCallback(async () => {
    const history = historyRef.current;
    if (busy || history.index >= history.stack.length - 1) return;
    const targetIndex = history.index + 1;
    const result = await commit(history.stack[targetIndex], 'none');
    if (result.ok) {
      history.index = targetIndex;
      setHistoryVersion((version) => version + 1);
    } else {
      setError(result.error);
    }
  }, [busy, commit]);

  const finishSave = useCallback(
    (result, sourceId) => {
      if (result.rekeyedFrom === sourceId) {
        projectIdRef.current = result.projectId;
        onRekey(sourceId, result.projectId);
      }
      const clean = result.dirty === false && result.currentRevision === result.savedRevision;
      if (clean) {
        baselineRef.current = snapshotKey(currentSnapshotRef.current);
        dirtyRef.current = false;
        setDirty(false);
      }
      return clean;
    },
    [onRekey],
  );

  const handleSaveAs = useCallback(async () => {
    if (!isTauri) return false;
    const rawName = projectIdRef.current.startsWith('untitled-')
      ? projectName || 'alignment'
      : projectIdRef.current.replace(/\\/g, '/').split('/').pop();
    const base = rawName.replace(/\.msa\.fasta?$/i, '').replace(/\.[^.]+$/, '');
    let path = await saveFileDialog(`${base}.msa.fa`, 'fa');
    if (!path) return false;
    if (!isMsaPath(path)) {
      path = path.replace(/\.fasta?$/i, '') + '.msa.fa';
    }
    try {
      const result = await saveFile(projectIdRef.current, path);
      if (result && !result.error) {
        setNotice('Saved aligned FASTA');
        return finishSave(result, projectIdRef.current);
      }
      setError(result?.error || 'Save failed');
    } catch (saveError) {
      setError(saveError?.message || String(saveError));
    }
    return false;
  }, [finishSave, projectName]);

  const handleSave = useCallback(async () => {
    if (!isTauri) return false;
    if (!isMsaPath(projectIdRef.current)) return handleSaveAs();
    try {
      const result = await saveFile(projectIdRef.current, projectIdRef.current);
      if (result && !result.error) {
        setNotice('Saved aligned FASTA');
        return finishSave(result, projectIdRef.current);
      }
      setError(result?.error || 'Save failed');
    } catch (saveError) {
      setError(saveError?.message || String(saveError));
    }
    return false;
  }, [finishSave, handleSaveAs]);

  const handleExport = useCallback(async () => {
    const rawName = projectName || 'alignment';
    const base = rawName.replace(/\.msa\.fasta?$/i, '').replace(/\.[^.]+$/, '');
    const path = await saveFileDialog(`${base}-unaligned.fa`, 'fa');
    if (!path) return;
    try {
      const result = await exportMsaFasta(projectIdRef.current, path, true);
      if (result?.error) setError(result.error);
      else setNotice('Exported unaligned FASTA');
    } catch (exportError) {
      setError(exportError?.message || String(exportError));
    }
  }, [projectName]);

  useEffect(() => {
    registerHandle(projectId, {
      save: handleSave,
      saveAs: handleSaveAs,
      isDirty: () => dirtyRef.current,
      moleculeType,
      projectKind: 'msa',
      alignmentEnabled: false,
    });
    return () => registerHandle(projectId, null);
  }, [projectId, registerHandle, handleSave, handleSaveAs, moleculeType]);

  useEffect(() => {
    if (hidden) return undefined;
    const handler = (event) => {
      if (!(event.metaKey || event.ctrlKey)) return;
      const key = event.key.toLowerCase();
      if (key === 's') {
        event.preventDefault();
        if (event.shiftKey) handleSaveAs();
        else handleSave();
      } else if (key === 'z') {
        event.preventDefault();
        if (event.shiftKey) handleRedo();
        else handleUndo();
      } else if (key === 'y') {
        event.preventDefault();
        handleRedo();
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [hidden, handleSave, handleSaveAs, handleUndo, handleRedo]);

  const visibleRows = useMemo(() => {
    const filtered = rows.filter((row) => !hiddenRowIds.has(row.id));
    if (!referenceRowId) return filtered;
    const reference = filtered.find((row) => row.id === referenceRowId);
    return reference
      ? [reference, ...filtered.filter((row) => row.id !== referenceRowId)]
      : filtered;
  }, [rows, hiddenRowIds, referenceRowId]);

  useEffect(() => {
    if (
      referenceRowId &&
      (hiddenRowIds.has(referenceRowId) || !rows.some((row) => row.id === referenceRowId))
    ) {
      setReferenceRowId(null);
    }
    if (!referenceRowId && colorMode === 'reference') setColorMode('consensus');
  }, [referenceRowId, hiddenRowIds, rows, colorMode]);

  const pairwiseMatrix = useMemo(
    () => computePairwiseMatrix(visibleRows, aligned, moleculeType),
    [visibleRows, aligned, moleculeType],
  );
  const averageIdentity = averagePairwiseIdentity(pairwiseMatrix);
  const columnCount = aligned
    ? visibleRows[0]?.alignedSequence.length || 0
    : visibleRows.reduce((max, row) => Math.max(max, row.sequence.length), 0);

  const handleSelection = useCallback((rowIndex, col, dragging) => {
    if (!dragging || !selectionAnchorRef.current) {
      selectionAnchorRef.current = { row: rowIndex, col };
    }
    const anchor = selectionAnchorRef.current;
    setSelection({ startRow: anchor.row, endRow: rowIndex, startCol: anchor.col, endCol: col });
  }, []);

  const copySelection = useCallback(async () => {
    if (!selection) return;
    const text = selectionToFasta(visibleRows, aligned, selection);
    if (!text) return;
    await navigator.clipboard.writeText(text);
    setNotice('Selection copied as FASTA');
  }, [selection, visibleRows, aligned]);

  const history = historyRef.current;
  const canUndo = historyVersion >= 0 && history.index > 0;
  const canRedo = historyVersion >= 0 && history.index < history.stack.length - 1;

  return (
    <main
      className={hidden ? 'hidden' : 'flex h-full min-h-0 w-full flex-col bg-background'}
      style={{ paddingLeft: leftViewportInset }}
    >
      <div className="flex flex-wrap items-center gap-1.5 border-b border-border/60 px-3 py-2">
        <Button size="sm" onClick={handleAlign} disabled={busy || agentLocked || !rows.length}>
          {busy ? (
            <LoaderCircle className="size-4 animate-spin" />
          ) : (
            <AlignLeft className="size-4" />
          )}
          {aligned ? 'Realign' : 'Align'}
        </Button>
        <Button size="sm" variant="outline" onClick={() => setAddOpen(true)} disabled={agentLocked}>
          <FilePlus2 className="size-4" /> Add
        </Button>
        <Button size="sm" variant="outline" onClick={() => setSequencesOpen(true)}>
          <Rows3 className="size-4" /> Sequences
        </Button>
        <Button
          size="sm"
          variant="outline"
          onClick={() => setStatsOpen(true)}
          disabled={visibleRows.length < 2}
        >
          <Sigma className="size-4" /> Statistics
        </Button>
        <div className="mx-1 h-5 w-px bg-border" />
        <Select
          value={moleculeType}
          onValueChange={(value) =>
            runAndReport(() => commit({ ...currentSnapshotRef.current, moleculeType: value }))
          }
        >
          <SelectTrigger className="h-8 w-28 text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="dna">DNA</SelectItem>
            <SelectItem value="rna">RNA</SelectItem>
            <SelectItem value="protein">Protein</SelectItem>
          </SelectContent>
        </Select>
        <Select value={colorMode} onValueChange={setColorMode}>
          <SelectTrigger className="h-8 w-32 text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="base">Residue colors</SelectItem>
            <SelectItem value="consensus">vs Consensus</SelectItem>
            <SelectItem value="reference" disabled={!referenceRowId}>
              vs Reference
            </SelectItem>
          </SelectContent>
        </Select>
        <div className="flex rounded-md border border-border/60 p-0.5">
          <Button
            size="icon"
            variant={viewMode === 'wrap' ? 'secondary' : 'ghost'}
            className="size-7"
            title="Vertical (Wrap)"
            onClick={() => onViewModeChange('wrap')}
          >
            <Grid2X2 className="size-4" />
          </Button>
          <Button
            size="icon"
            variant={viewMode === 'continuous' ? 'secondary' : 'ghost'}
            className="size-7"
            title="Horizontal (Continuous)"
            onClick={() => onViewModeChange('continuous')}
          >
            <Columns2 className="size-4" />
          </Button>
        </div>
        <div className="ml-auto flex items-center gap-1">
          <Button
            size="icon"
            variant="ghost"
            className="size-8"
            title="Undo"
            disabled={!canUndo || busy}
            onClick={handleUndo}
          >
            <Undo2 className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className="size-8"
            title="Redo"
            disabled={!canRedo || busy}
            onClick={handleRedo}
          >
            <Redo2 className="size-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className="size-8"
            title="Copy selection as FASTA"
            disabled={!selection}
            onClick={copySelection}
          >
            <Copy className="size-4" />
          </Button>
          <Button size="sm" variant="outline" onClick={handleExport}>
            <Download className="size-4" /> Ungapped
          </Button>
          <Button size="sm" variant="outline" onClick={handleSave}>
            <Save className="size-4" /> Save
          </Button>
        </div>
      </div>

      {!aligned && !loading && (
        <div className="border-b border-amber-200 bg-amber-50 px-3 py-1.5 text-xs text-amber-800">
          Sequences are not aligned. Add at least two sequences and choose Align; saving now
          preserves the ungapped collection.
        </div>
      )}
      {error && (
        <div className="flex items-center justify-between border-b border-red-200 bg-red-50 px-3 py-1.5 text-xs text-red-700">
          <span>{error}</span>
          <button onClick={() => setError('')} className="ml-3 underline">
            Dismiss
          </button>
        </div>
      )}
      {notice && (
        <button
          className="border-b border-teal-200 bg-teal-50 px-3 py-1 text-left text-xs text-teal-800"
          onClick={() => setNotice('')}
        >
          {notice}
        </button>
      )}

      {loading ? (
        <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">
          <LoaderCircle className="mr-2 size-4 animate-spin" /> Loading MSA…
        </div>
      ) : (
        <MsaEditor
          rows={visibleRows}
          aligned={aligned}
          moleculeType={moleculeType}
          viewMode={viewMode}
          referenceRowId={referenceRowId}
          colorMode={colorMode}
          selection={selection}
          onSelectionChange={handleSelection}
          hidden={hidden}
        />
      )}

      <div className="flex items-center gap-4 border-t border-border/60 px-3 py-1.5 text-xs text-muted-foreground">
        <span>
          {rows.length} sequences{hiddenRowIds.size ? ` (${hiddenRowIds.size} hidden)` : ''}
        </span>
        <span>{columnCount} columns</span>
        <span>
          Average pairwise identity:{' '}
          {averageIdentity == null ? '—' : `${(averageIdentity * 100).toFixed(1)}%`}
        </span>
        <span className="ml-auto">MUSCLE 5.3 · gap-excluded identity</span>
      </div>

      <AddSequencesDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        onConfirm={(newRows) =>
          runAndReport(() =>
            commit({
              ...currentSnapshotRef.current,
              rows: [...rows, ...newRows],
              moleculeType: detectMsaMoleculeType([...rows, ...newRows]),
              aligned: false,
            }),
          )
        }
      />
      <MsaSequencesDialog
        open={sequencesOpen}
        onOpenChange={setSequencesOpen}
        rows={rows}
        hiddenRowIds={hiddenRowIds}
        referenceRowId={referenceRowId}
        onToggleHidden={(id) =>
          setHiddenRowIds((previous) => {
            const next = new Set(previous);
            if (next.has(id)) next.delete(id);
            else if (next.size < rows.length - 1) next.add(id);
            return next;
          })
        }
        onSetReference={(id) => {
          setReferenceRowId(id);
          if (!id && colorMode === 'reference') setColorMode('consensus');
        }}
        onChange={(nextRows) =>
          runAndReport(() =>
            commit({
              ...currentSnapshotRef.current,
              rows: nextRows,
            }),
          )
        }
      />
      <PairwiseStatsDialog
        open={statsOpen}
        onOpenChange={setStatsOpen}
        rows={visibleRows}
        aligned={aligned}
        moleculeType={moleculeType}
      />
    </main>
  );
}
