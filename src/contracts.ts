export type MoleculeType = 'dna' | 'rna' | 'protein';
export type Topology = 'circular' | 'linear';

export interface Segment {
  start: number;
  end: number;
  color?: string | null;
}

export interface Feature {
  id: string;
  name: string;
  start: number;
  end: number;
  color: string;
  ftype: string;
  segments: Segment[];
  strand: string;
  notes: string;
  translation: string;
  qualifiers: [string, string][];
  orf?: boolean;
}

export interface TextColorRange {
  start: number;
  end: number;
  color: string;
}

export interface AlignmentRenderData {
  displaySequence: string;
  insertionMap?: Record<string, unknown>;
  mismatchIndices?: number[];
  startOffset?: number;
}

export interface PrimerBindingSite {
  primerId: string;
  strand: 1 | -1;
  templateStart: number;
  templateEnd: number;
  tm: number;
  gcContent: number;
  matchScore: number;
  has3PrimeMismatch?: boolean;
  fivePrimeTail?: string;
  threePrimeTail?: string;
  displayTail?: string;
  alignment: AlignmentRenderData;
}

export interface Primer {
  id: string;
  name: string;
  type: 'fwd' | 'rev';
  primerSeq: string;
  bindingSites: PrimerBindingSite[];
  color?: string;
  [key: string]: unknown;
}

export interface AlignSegment {
  start: number;
  end: number;
  chars: string;
}

export interface AlignInsertion {
  pos: number;
  bases: string;
}

export interface Alignment {
  id: string;
  name: string;
  length: number;
  strand: '+' | '-';
  identity: number;
  segments: AlignSegment[];
  insertions: AlignInsertion[];
  seq: string;
  tracePath?: string | null;
}

export interface CutPair {
  topCutIndex: number;
  botCutIndex: number;
}

export interface Enzyme {
  id: string;
  name: string;
  recSeq: string;
  recStart: number;
  recEnd: number;
  displayStart: number;
  displayEnd: number;
  cutIndex: number;
  botCutIndex: number;
  cutPairs: CutPair[];
  recognitionStrand: string;
  compSeq?: string;
  recSeqPattern?: string;
  spacers?: Segment[] | null;
  isUnique: boolean;
  truncated?: boolean;
  cutType?: string;
  cutTwice?: boolean;
  [key: string]: unknown;
}

export interface ProjectData {
  sequence: string;
  length: number;
  topology: Topology;
  moleculeType: MoleculeType;
  features: Feature[];
  primers: Primer[];
  alignments: Alignment[];
  enzymes: Enzyme[];
  methylationSystems?: string[];
  methylationOverlap?: number;
  roi?: [number, number] | null;
  tracePath?: string | null;
  textColorsTop: TextColorRange[];
  textColorsBottom: TextColorRange[];
  enzymeCount?: number;
  enzymeFilter?: string;
  dirty?: boolean;
  id?: string;
  error?: string;
}

export interface ProjectSummary {
  id: string;
  name: string;
  length: number;
  topology: Topology;
  moleculeType: MoleculeType;
  dirty: boolean;
  agentLocked?: boolean | null;
}

export interface ProjectListResponse {
  projects: ProjectSummary[];
  activeId: string | null;
  error?: string;
}

export interface ProjectMutationResponse extends ProjectData {
  projects?: ProjectSummary[];
  activeId?: string | null;
}

export interface ProjectSnapshot {
  sequence: string;
  features: Feature[];
  primers: Primer[];
  textColorsTop: TextColorRange[];
  textColorsBottom: TextColorRange[];
}

export interface ProjectUpdateEvent extends ProjectListResponse {
  source?: string | null;
  data?: ProjectData;
  projectData?: Record<string, ProjectData>;
  changedProjectId?: string | null;
}

export interface TmParams {
  naConc?: number;
  mgConc?: number;
  dntpConc?: number;
  trisConc?: number;
  primerConc?: number;
}

export interface PrimerAlignmentResult {
  current?: PrimerBindingSite;
  alternatives?: PrimerBindingSite[];
  alignment?: string;
  [key: string]: unknown;
}

export interface Chromatogram {
  traceA: number[];
  traceC: number[];
  traceG: number[];
  traceT: number[];
  peakLocations: number[];
}
