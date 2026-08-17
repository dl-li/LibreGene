export const DESIGN_MODES = {
  amplify: {
    title: 'Amplify Fragment',
    segments: 1,
    minLen: 50,
    prompts: ['Select the fragment or feature to amplify'],
  },
  oepcr: {
    title: 'OE-PCR',
    segments: 2,
    minLen: 50,
    prompts: ['Select fragment (1/2) for OE-PCR', 'Select fragment (2/2) for OE-PCR'],
  },
  mutagenesis: {
    title: 'PCR Mutagenesis',
    segments: 1,
    maxLen: 10,
    circularOnly: true,
    prompts: ['Select the base pairs to mutate'],
  },
};

export default {
  id: 'primerDesign',
  name: 'Primer Design',
  description: 'Design Amplify / OE-PCR / Mutagenesis primers from a selection',
  version: '1.0.0',
  dialogKey: null,
  dnaOnly: true,
  sidebarItems: [],
  dialog: null,
  // The interactive pick-on-sequence flow is wired directly in
  // SequenceEditor/EditorNavMenu (nav menu entry only), not via dialogKey.
  navMenuOnly: true,
};
