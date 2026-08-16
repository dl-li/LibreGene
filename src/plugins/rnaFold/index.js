import { AudioWaveform } from 'lucide-react';
import RnaFoldDialog from './RnaFoldDialog';

const rnaFoldPlugin = {
  id: 'rnaFold',
  name: 'RNA Folding',
  dialogKey: 'rnaFold',
  // RNA-only: MFE secondary-structure prediction is meaningful only for
  // single-stranded RNA projects.
  rnaOnly: true,
  sidebarItems: [
    {
      dialogKey: 'rnaFold',
      label: 'RNA Folding',
      tooltip: 'Predict MFE secondary structure (Turner 2004)',
      icon: AudioWaveform,
    },
  ],
  dialog: RnaFoldDialog,
};

export default rnaFoldPlugin;
