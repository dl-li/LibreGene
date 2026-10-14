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
  return (
    <>
      {isDna && (
        <PrimerOverviewDialog
          open={primerOverviewOpen}
          onOpenChange={setPrimerOverviewOpen}
          primers={primers}
          alignmentCacheRef={alignmentCacheRef}
          onEditPrimer={(p) => {
            openPrimerEditorRef.current?.(p);
          }}
        />
      )}

      <DetectFeaturesDialog
        open={detectFeaturesOpen}
        onOpenChange={setDetectFeaturesOpen}
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
          const DialogComp = plugin.dialog;
          if (!DialogComp) return null;
          return (
            <DialogComp
              key={plugin.id}
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
            <DialogTitle>Delete Primer?</DialogTitle>
            <DialogDescription>
              Primer "{primers.find((p) => p.id === primerDeleteConfirm)?.name}" will be removed
              from this file. You can undo this with Ctrl+Z.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2 sm:gap-2">
            <Button variant="outline" onClick={() => setPrimerDeleteConfirm(null)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={async () => {
                const id = primerDeleteConfirm;
                setPrimerDeleteConfirm(null);
                await handleDeletePrimer(id);
              }}
            >
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
