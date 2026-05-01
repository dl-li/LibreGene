"""Core data models — canonical JSON shape consumed by the frontend."""
from __future__ import annotations
from dataclasses import dataclass, field


@dataclass
class PrimerMismatchFeature:
    name: str
    start: int   # index within mismatchStr
    end: int     # index within mismatchStr


@dataclass
class BindingSite:
    """A single binding site for a primer (supports multi-site primers)."""
    match_start: int        # inclusive
    match_end: int          # inclusive
    match_str: str          # template-strand binding sequence
    tm: float = 0.0         # annealing temperature (°C)


@dataclass
class Primer:
    """Primer model. match_start/end/str are derived from binding_sites[0] for frontend compat."""
    id: str
    name: str
    type: str               # "fwd" | "rev"
    mismatch_str: str = ""  # 5' tail
    mismatch_features: list[PrimerMismatchFeature] = field(default_factory=list)
    color: str = "#166534"
    binding_sites: list[BindingSite] = field(default_factory=list)

    @property
    def match_start(self) -> int:
        return self.binding_sites[0].match_start if self.binding_sites else 0

    @property
    def match_end(self) -> int:
        return self.binding_sites[0].match_end if self.binding_sites else 0

    @property
    def match_str(self) -> str:
        return self.binding_sites[0].match_str if self.binding_sites else ""


@dataclass
class Feature:
    id: str
    name: str
    start: int              # overall min (inclusive)
    end: int                # overall max (inclusive)
    color: str = "#60A5FA"
    ftype: str = ""         # CDS, promoter, terminator, etc.
    segments: list[dict] = field(default_factory=list)  # [{start, end}, ...]
    notes: str = ""
    translation: str = ""            # AA sequence (CDS only)
    translation_segments: list[str] = field(default_factory=list)


@dataclass
class Enzyme:
    """Restriction enzyme — all positions absolute (0-based), frontend renders directly."""
    id: str
    name: str
    rec_seq: str               # recognition sequence (actual template)
    rec_start: int             # absolute start of recognition (inclusive)
    rec_end: int               # absolute end of recognition (inclusive)
    display_start: int         # absolute start of tooltip window
    display_end: int           # absolute end of tooltip window
    cut_index: int             # absolute top-strand cut position (cut between cut_index and cut_index+1)
    bot_cut_index: int         # absolute bottom-strand cut position (cut between bot_cut_index and bot_cut_index+1)
    top_cut_in_rec: int = 0    # cut position within rec_seq (informational)
    bot_cut_in_rec: int = 0
    comp_seq: str = ""
    rec_seq_pattern: str = ""  # enzyme recognition pattern (may include IUPAC codes, N for variable)
    spacers: list | None = None  # [{start, end}, ...] non-recognition regions in display (relative)
    is_unique: bool = True
    methylation_blocked: bool = False


@dataclass
class ProjectData:
    """Complete project state, serialised to JSON for the frontend."""
    sequence: str
    length: int
    topology: str = "circular"    # circular | linear
    features: list[Feature] = field(default_factory=list)
    primers: list[Primer] = field(default_factory=list)
    enzymes: list[Enzyme] = field(default_factory=list)
    roi: tuple[int, int] | None = None   # current region-of-interest
