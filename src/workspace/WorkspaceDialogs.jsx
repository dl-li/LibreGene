import { plugins } from '../plugins';
import AddAlignmentTextDialog from '../plugins/alignment/AddAlignmentTextDialog';
import SequenceEditDialog from '../dialogs/SequenceEditDialog';
import PrimerOverviewDialog from '../dialogs/PrimerOverviewDialog';
import DetectFeaturesDialog from '../dialogs/DetectFeaturesDialog';
import MyPrimersDialog from '../dialogs/MyPrimersDialog';
import MyEnzymesDialog from '../dialogs/MyEnzymesDialog';
import EnzymeDatabaseDialog from '../dialogs/EnzymeDatabaseDialog';
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
        .filter(
          (plugin) =>
            !disabledPlugins.includes(plugin.id) &&
            (isDna || !plugin.dnaOnly) &&
            (moleculeType === 'rna' || !plugin.rnaOnly) &&
            (moleculeType !== 'protein' || !plugin.notForProtein) &&
            (!plugin.dialogVisible || plugin.dialogVisible({ projectId, moleculeType, isTauri })),
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
    </>
  );
}
