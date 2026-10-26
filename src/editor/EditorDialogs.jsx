import FeatureInfoDialog from '../dialogs/FeatureInfoDialog';
import EnzymeDetailDialog from '../dialogs/EnzymeDetailDialog';
import PrimerAlignmentDialog from '../dialogs/PrimerAlignmentDialog';
import PrimerDesignDialog from '../plugins/builtin/primerDesign/PrimerDesignDialog';

export default function EditorDialogs({
  featureInfoFeature,
  createFeatureLoc,
  setFeatureInfoFeature,
  setCreateFeatureLoc,
  onFtypeChange,
  onFeatureColorChange,
  onFeatureLocationChange,
  onFeatureNameChange,
  onFeatureStrandChange,
  onFeatureAdd,
  onDeleteFeature,
  features,
  moleculeType,
  enzymeDetailRecord,
  setEnzymeDetailRecord,
  enzymeDbRef,
  providerIndexRef,
  enzymeDetailCutSites,
  cleanSeq,
  enzymeProvider,
  primerAlignmentPrimer,
  createPrimerSeq,
  setPrimerAlignmentPrimer,
  setCreatePrimerSeq,
  primerAlignmentCache,
  seedLength,
  onPrimerChange,
  onPrimerDelete,
  primers,
  tmParams,
  projectId,
  designDialog,
  setDesignDialog,
  topology,
}) {
  return (
    <>
      <FeatureInfoDialog
        feature={featureInfoFeature}
        open={featureInfoFeature !== null || createFeatureLoc !== null}
        onOpenChange={(open) => {
          if (!open) {
            setFeatureInfoFeature(null);
            setCreateFeatureLoc(null);
          }
        }}
        onFtypeChange={onFtypeChange}
        onFeatureColorChange={onFeatureColorChange}
        onFeatureLocationChange={onFeatureLocationChange}
        onFeatureNameChange={onFeatureNameChange}
        onFeatureStrandChange={onFeatureStrandChange}
        newFeatureLoc={createFeatureLoc}
        onFeatureAdd={onFeatureAdd}
        onDeleteFeature={onDeleteFeature}
        features={features}
        moleculeType={moleculeType}
      />
      <EnzymeDetailDialog
        open={enzymeDetailRecord !== null}
        onOpenChange={(open) => {
          if (!open) setEnzymeDetailRecord(null);
        }}
        record={enzymeDetailRecord}
        dbRecords={enzymeDbRef.current}
        providerIndex={providerIndexRef.current}
        cutSites={enzymeDetailCutSites}
        plasmidLength={cleanSeq.length}
        currentProvider={enzymeProvider}
      />
      <PrimerAlignmentDialog
        primer={primerAlignmentPrimer}
        alignmentData={primerAlignmentCache[primerAlignmentPrimer?.id]}
        open={primerAlignmentPrimer !== null || createPrimerSeq !== null}
        onOpenChange={(open) => {
          if (!open) {
            setPrimerAlignmentPrimer(null);
            setCreatePrimerSeq(null);
          }
        }}
        seedLength={seedLength}
        newPrimerSeq={createPrimerSeq}
        onPrimerChange={onPrimerChange}
        onDeletePrimer={onPrimerDelete}
        primers={primers}
        tmParams={tmParams}
        projectId={projectId}
      />
      <PrimerDesignDialog
        open={designDialog !== null}
        onOpenChange={(open) => {
          if (!open) setDesignDialog(null);
        }}
        mode={designDialog?.mode}
        segments={designDialog?.segments}
        sequence={cleanSeq}
        topology={topology}
        tmParams={tmParams}
        onPrimerChange={onPrimerChange}
      />
    </>
  );
}
