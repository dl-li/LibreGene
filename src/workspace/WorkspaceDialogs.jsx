import { lazy, Suspense, useEffect, useState } from 'react';
import { plugins, pluginAvailable } from '../plugins';
import AddAlignmentTextDialog from '../plugins/builtin/alignment/AddAlignmentTextDialog';
import SequenceEditDialog from '../dialogs/SequenceEditDialog';
import PrimerOverviewDialog from '../dialogs/PrimerOverviewDialog';
import DetectFeaturesDialog from '../dialogs/DetectFeaturesDialog';
import MyPrimersDialog from '../dialogs/MyPrimersDialog';
import MyEnzymesDialog from '../dialogs/MyEnzymesDialog';
import EnzymeDatabaseDialog from '../dialogs/EnzymeDatabaseDialog';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../components/ui/dialog';
import { Button } from '../components/ui/button';
import { isTauri } from '../tauriApi';
import { T, useTranslation } from '@/i18n';

const lazyDialogs = new Map();

function PluginDialog({ plugin, open, ...props }) {
  const [hasOpened, setHasOpened] = useState(open);
  useEffect(() => {
    if (open) setHasOpened(true);
  }, [open]);
  if (!hasOpened || (!plugin.dialog && !plugin.dialogLoader)) return null;
  if (plugin.dialogLoader && !lazyDialogs.has(plugin.id)) {
    lazyDialogs.set(plugin.id, lazy(plugin.dialogLoader));
  }
  const DialogComp = plugin.dialog || lazyDialogs.get(plugin.id);
  return (
    <Suspense fallback={null}>
      <DialogComp open={open} {...props} />
    </Suspense>
  );
}

export default function WorkspaceDialogs({
  projectId,
  moleculeType,
  isDna,
  sequence,
  features,
  enzymes,
  topology,
  alignments,
  primers,
  tmParams,
  myPrimers,
  myEnzymes,
  enzymeFilter,
  enzymeProvider,
  onEnzymeProviderChange,
  disabledPlugins,
  onOpenSnapshot,
  mapName,
  background,
  setEditorBackground,
  alignmentCacheRef,
  openPrimerEditorRef,
  primerOverviewOpen,
  setPrimerOverviewOpen,
  detectFeaturesOpen,
  setDetectFeaturesOpen,
  myPrimersOpen,
  setMyPrimersOpen,
  myEnzymesOpen,
  setMyEnzymesOpen,
  enzymeDbOpen,
  setEnzymeDbOpen,
  myPrimerBinding,
  pluginDialogs,
  setPluginDialogs,
  alignTextOpen,
  setAlignTextOpen,
  editDialog,
  handleEditConfirm,
  handleEditCancel,
  primerDeleteConfirm,
  setPrimerDeleteConfirm,
  handleDeletePrimer,
  handleFeatureAdd,
  handleAddMyPrimerToFile,
  handleAddAllBindingPrimers,
  handleDeleteMyPrimer,
  handleMyEnzymesChange,
  handleAddAlignment,
  handleAddAlignmentText,
  handleRemoveAlignment,
  refreshProject,
}) {
  const { t } = useTranslation();
  return (
    <>
      {isDna && (
        <PrimerOverviewDialog
          open={primerOverviewOpen}
          onOpenChange={setPrimerOverviewOpen}
          primers={primers}
          tmParams={tmParams}
          alignmentCacheRef={alignmentCacheRef}
          onEditPrimer={(p) => {
            openPrimerEditorRef.current?.(p);
          }}
        />
      )}

      <DetectFeaturesDialog
        open={detectFeaturesOpen}
        onOpenChange={setDetectFeaturesOpen}
        projectId={projectId}
        features={features}
        onAddFeature={handleFeatureAdd}
      />

      {isDna && (
        <MyPrimersDialog
          open={myPrimersOpen}
          onOpenChange={setMyPrimersOpen}
          myPrimers={myPrimers}
          currentPrimers={primers}
          binding={myPrimerBinding}
          onAddPrimer={handleAddMyPrimerToFile}
          onAddAllBinding={handleAddAllBindingPrimers}
          onDelete={handleDeleteMyPrimer}
        />
      )}

      {isDna && (
        <MyEnzymesDialog
          open={myEnzymesOpen}
          onOpenChange={setMyEnzymesOpen}
          enzymes={myEnzymes}
          onChange={handleMyEnzymesChange}
        />
      )}

      {isDna && (
        <EnzymeDatabaseDialog
          open={enzymeDbOpen}
          onOpenChange={setEnzymeDbOpen}
          enzymeProvider={enzymeProvider}
          onEnzymeProviderChange={onEnzymeProviderChange}
          projectEnzymes={enzymes}
          plasmidLength={sequence?.length ?? 0}
        />
      )}

      {plugins
        .filter((plugin) =>
          pluginAvailable(plugin, { moleculeType, disabledPlugins, projectId, isTauri }),
        )
        .map((plugin) => {
          if (!plugin.dialog && !plugin.dialogLoader) return null;
          return (
            <PluginDialog
              key={plugin.id}
              plugin={plugin}
              open={!!pluginDialogs[plugin.dialogKey]}
              onOpenChange={(open) =>
                setPluginDialogs((prev) => ({
                  ...prev,
                  [plugin.dialogKey]: open,
                }))
              }
              projectId={projectId}
              topology={topology}
              enzymes={enzymes}
              myEnzymes={myEnzymes}
              enzymeFilter={enzymeFilter}
              dialogState={pluginDialogs[plugin.dialogKey]}
              onOpenSnapshot={onOpenSnapshot}
              sequence={sequence}
              fileName={mapName}
              alignments={alignments}
              onAddAlignment={handleAddAlignment}
              onRemoveAlignment={handleRemoveAlignment}
              features={features}
              onProjectChanged={refreshProject}
              watermark={background === 'folding'}
              onToggleWatermark={() =>
                setEditorBackground(moleculeType, background === 'folding' ? 'none' : 'folding')
              }
            />
          );
        })}

      {isDna && (
        <AddAlignmentTextDialog
          open={alignTextOpen}
          onOpenChange={setAlignTextOpen}
          onSubmit={handleAddAlignmentText}
        />
      )}

      {/* --- Sequence Edit Dialog --- */}
      <SequenceEditDialog
        open={editDialog.open}
        mode={editDialog.mode}
        cursorIndex={editDialog.cursorIndex}
        selStart={editDialog.selStart}
        selEnd={editDialog.selEnd}
        selectedText={editDialog.selectedText}
        initialText={editDialog.initialText}
        onConfirm={handleEditConfirm}
        onCancel={handleEditCancel}
        moleculeType={moleculeType}
        clipboardMeta={editDialog.clipboardMeta}
      />

      {/* --- Primer delete confirmation --- */}
      <Dialog
        open={!!primerDeleteConfirm}
        onOpenChange={(open) => {
          if (!open) setPrimerDeleteConfirm(null);
        }}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>
              <T>{'Delete Primer?'}</T>
            </DialogTitle>
            <DialogDescription>
              {t('Delete primer "{name}"? You can undo this with Ctrl+Z.', {
                name: primers.find((p) => p.id === primerDeleteConfirm)?.name,
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2 sm:gap-2">
            <Button variant="outline" onClick={() => setPrimerDeleteConfirm(null)}>
              <T>{'Cancel'}</T>
            </Button>
            <Button
              variant="destructive"
              onClick={async () => {
                const id = primerDeleteConfirm;
                setPrimerDeleteConfirm(null);
                await handleDeletePrimer(id);
              }}
            >
              <T>{'Delete'}</T>
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
