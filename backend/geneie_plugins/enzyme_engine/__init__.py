"""
Enzyme engine plugin v1.1 — precise per-base coordinate computation.
Computes absolute display bounds so the frontend never does offset math.
Handles all enzyme types: standard, Type IIS, bottom-strand, N-containing.
"""
from __future__ import annotations
import re
from geneie_core.plugin_protocol import PluginProtocol, PluginManifest
from geneie_core.core_api import CoreApi
from geneie_core.models import Enzyme

IUPAC = {'N': '.', 'R': '[AG]', 'Y': '[CT]', 'W': '[AT]', 'S': '[CG]', 'K': '[GT]', 'M': '[AC]',
         'B': '[CGT]', 'D': '[AGT]', 'H': '[ACT]', 'V': '[ACG]'}

IUPAC_COMP = str.maketrans({
    'A': 'T', 'T': 'A', 'G': 'C', 'C': 'G',
    'R': 'Y', 'Y': 'R', 'W': 'W', 'S': 'S',
    'K': 'M', 'M': 'K', 'B': 'V', 'D': 'H',
    'H': 'D', 'V': 'B', 'N': 'N',
    'a': 't', 't': 'a', 'g': 'c', 'c': 'g',
})

DNA_COMP = str.maketrans("ATGCatgc", "TACGtacg")


class EnzymeEnginePlugin(PluginProtocol):
    def __init__(self):
        self._manifest = PluginManifest(
            name="enzyme_engine", version="1.1.0",
            description="Precise per-base enzyme site computation with absolute display bounds.",
        )

    @property
    def manifest(self) -> PluginManifest:
        return self._manifest

    def on_load(self, core: CoreApi) -> None:
        core.on_sequence_changed(self._recompute)
        if core.project is not None:
            self._recompute(core.project)

    def _recompute(self, project) -> None:
        if not project.sequence or len(project.sequence) < 4:
            project.enzymes.clear()
            return

        try:
            from Bio.Restriction import CommOnly, Analysis
        except ImportError:
            return
        from Bio.Seq import Seq

        seq = project.sequence
        analysis = Analysis(CommOnly, Seq(seq), linear=project.topology == "linear")
        sites_map = analysis.with_sites()
        result: list[Enzyme] = []

        for enz in CommOnly:
            cuts = sites_map.get(enz, [])
            if not cuts:
                continue
            is_unique = len(cuts) == 1
            if len(cuts) > 200:
                continue
            site = str(enz.site)
            rec_len = len(site)
            top_off, bot_off = _parse_elucidate(enz.elucidate(), site)

            for enz_idx, ci_1b in enumerate(cuts):
                # cut_index: 0-based — cut separates bases (cut_index-1) and cut_index
                cut_index = ci_1b - 1

                # Locate recognition in template (±30 bp window)
                ctx_s = max(0, cut_index - 30)
                ctx_e = min(len(seq), cut_index + 30)
                rel_cut = cut_index - ctx_s

                rec_start_rel, matched_seq, is_bottom = _locate(
                    seq[ctx_s:ctx_e], site, rel_cut, top_off,
                )
                rec_start = ctx_s + rec_start_rel
                rec_end = rec_start + rec_len - 1

                # Compute bot_cut_index with signed stagger
                bot_cut_index = cut_index + (bot_off - top_off)

                # Display bounds: cover rec + both sides of every cut.
                # A cut at position p separates bases (p-1) and p; min/max
                # naturally only extend the bounds when a cut falls outside rec.
                disp_start = min(rec_start, cut_index - 1, bot_cut_index - 1)
                disp_end = max(rec_end, cut_index, bot_cut_index)

                # rec_seq_pattern as it applies to the template strand
                rec_pattern = _complement_iupac(site) if is_bottom else site

                # Spacers: non-recognition regions within the display (half-open)
                spacers = []
                if disp_start < rec_start:
                    spacers.append({"start": 0, "end": rec_start - disp_start})
                ro = rec_start - disp_start
                if ro + rec_len < disp_end - disp_start + 1:
                    spacers.append({"start": ro + rec_len, "end": disp_end - disp_start + 1})

                result.append(Enzyme(
                    id=f"{enz}_{cut_index}_{enz_idx}",
                    name=str(enz),
                    cut_index=cut_index,
                    rec_seq=matched_seq,
                    rec_seq_pattern=rec_pattern,
                    rec_start=rec_start,
                    rec_end=rec_end,
                    display_start=disp_start,
                    display_end=disp_end,
                    top_cut_in_rec=cut_index - rec_start,
                    bot_cut_in_rec=bot_cut_index - rec_start,
                    comp_seq=_complement(matched_seq),
                    bot_cut_index=bot_cut_index,
                    spacers=spacers or None,
                    is_unique=is_unique,
                ))

        project.enzymes[:] = result


# ── Helpers ────────────────────────────────────────────────────

def _parse_elucidate(eluc: str, site: str) -> tuple[int, int]:
    """Return (top_cut, bot_cut) — 0-indexed positions of cuts within recognition.

    top_cut = index within rec where the cut occurs (between top_cut-1 and top_cut).
    When no explicit bottom cut mark, the bottom cut is at the symmetrical position.
    """
    clean = eluc.replace("^", "").replace("_", "")
    tc = eluc.find("^")
    bc = eluc.find("_")
    if tc >= 0:
        tc -= eluc[:tc].count("_")
    if bc >= 0:
        bc -= eluc[:bc].count("^")
    if tc < 0:
        tc = 0
    if bc < 0:
        # No explicit bottom mark → symmetrical position from the end
        bc = len(site) - tc
    # Adjust for flanking context bases in elucidate
    ss = clean.find(site)
    if ss >= 0:
        tc -= ss
        bc -= ss
    return (max(0, tc), max(0, bc))


def _locate(ctx: str, site: str, cut_pos: int, top_off: int) -> tuple[int, str, bool]:
    """Find recognition in ctx nearest to the expected position (cut_pos - top_off)."""
    expected = cut_pos - top_off
    rec_len = len(site)

    # Try forward strand
    matches = _fuzzy_find_all(ctx, site)
    if matches:
        p = _closest(matches, expected, cut_pos, top_off)
        return (p, ctx[p:p + rec_len], False)

    # Try reverse complement
    rc = _complement(site)
    matches = _fuzzy_find_all(ctx, rc)
    if matches:
        p = _closest(matches, expected, cut_pos, top_off)
        return (p, ctx[p:p + len(rc)], True)

    # Fallback
    p = max(0, cut_pos - top_off)
    return (p, ctx[p:p + rec_len], False)


def _fuzzy_find_all(ctx: str, site: str) -> list[int]:
    """Return all match positions for an IUPAC pattern in ctx."""
    pat = ''.join(IUPAC.get(c, c) for c in site)
    return [m.start() for m in re.finditer(pat, ctx)]


def _closest(positions: list[int], target: int, cut_pos: int, top_off: int) -> int:
    """Return the position closest to target. Tiebreak: prefer the one where
    (cut_pos - pos) is closer to top_off — i.e. the cut falls at the right
    offset within the recognition."""
    return min(positions, key=lambda p: (abs(p - target), abs((cut_pos - p) - top_off)))


def _complement(seq: str) -> str:
    return seq.translate(DNA_COMP)[::-1]


def _complement_iupac(seq: str) -> str:
    """Reverse-complement a sequence that may contain IUPAC ambiguity codes."""
    return seq.translate(IUPAC_COMP)[::-1]


def create_plugin() -> PluginProtocol:
    return EnzymeEnginePlugin()
