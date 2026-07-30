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
