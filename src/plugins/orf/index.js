import { findOrfs as findOrfsCommand } from '../../tauriApi';

// ORF Search plugin — the sequence scan itself now runs in the backend
// (libregene_core::orf::find_orfs, ignoring feature annotations) for
// start→stop in-frame ORFs on both strands; ORFs ≥ MIN_AA are displayed as
// virtual, display-only CDS features (never persisted).

const MIN_AA = 75;

// Lighter variants of the primer F/R theme colors (#166534 / #4A148C)
export const ORF_COLORS = { fwd: '#8BB29A', rev: '#A58AC6' };

// The backend returns real Feature JSON with qualifiers [["orf","true"]];
// restore the virtual `orf: true` flag the renderer keys off (ORFs get the
// lowest feature-track priority).
export async function findOrfs(minAa = MIN_AA) {
  const features = await findOrfsCommand(minAa);
  return features.map((f) => ({
    ...f,
    orf: (f.qualifiers || []).some(([k, v]) => k === 'orf' && v === 'true'),
  }));
}

export default {
  id: 'orf',
  name: 'Show ORFs',
  description: 'Scan both strands for start→stop open reading frames',
  version: '1.0.0',
  dialogKey: null,
  // DNA-only: ORF scanning (both strands, genetic code) is meaningless for
  // rna/protein projects.
  dnaOnly: true,
  // ORF visibility is toggled from the Features nav menu (ProjectWorkspace),
  // not from the sidebar.
  sidebarItems: [],
  dialog: null,
};
