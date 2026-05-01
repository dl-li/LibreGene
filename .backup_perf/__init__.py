"""
Official enzyme engine plugin — scans for unique common restriction sites.
Computes absolute recognition positions so the frontend never does offset math.
"""
from __future__ import annotations
from geneie_core.plugin_protocol import PluginProtocol, PluginManifest
from geneie_core.core_api import CoreApi
from geneie_core.models import Enzyme


class EnzymeEnginePlugin(PluginProtocol):
    def __init__(self):
        self._manifest = PluginManifest(
            name="enzyme_engine", version="0.3.0",
            description="Scans for unique common restriction sites; emits absolute positions.",
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
        seq = Seq(project.sequence)
        analysis = Analysis(CommOnly, seq, linear=project.topology == "linear")
        sites_map = analysis.with_sites()
        result: list[Enzyme] = []

        for enz in CommOnly:
            cuts = sites_map.get(enz, [])
            if not cuts:
                continue
            is_unique = len(cuts) == 1
            rec_seq = str(enz.site)
            top_off, bot_off = _parse_elucidate(enz.elucidate(), rec_seq)

            for cut_index_1based in cuts:
                cut_index = cut_index_1based - 1  # 0-based

                # Locate recognition by searching template near the cut (handles both strands)
                ctx_start = max(0, cut_index - 30)
                ctx_end = min(len(project.sequence), cut_index + 30)
                ctx = project.sequence[ctx_start:ctx_end]

                rec_start, actual_rec, is_bottom = _locate_recognition(
                    ctx, rec_seq, cut_index - ctx_start, top_off,
                )
                rec_start += ctx_start  # absolute
                rec_len = len(actual_rec)

                # Compute display bounds: extend to include both cuts
                display_start = min(rec_start, cut_index)
                display_end = max(rec_start + rec_len - 1, cut_index)

                # Bottom cut: signed offset from top cut (works for bc < tc and bc > tc)
                bot_cut_index = cut_index + (bot_off - top_off)

                display_start = min(display_start, bot_cut_index)
                display_end = max(display_end, bot_cut_index)

                # top cut position within displayed context
                top_in_display = cut_index - display_start
                bot_in_display = bot_cut_index - display_start

                # spacer: non-recognition region within displayed context
                rec_offset = rec_start - display_start
                total_len = display_end - display_start + 1
                spacer = None
                if total_len > rec_len:
                    if rec_offset > 0:
                        spacer = {"start": 0, "end": rec_offset}
                    else:
                        spacer = {"start": rec_len, "end": total_len}

                rec_end = display_end
                rec_start = display_start

                result.append(Enzyme(
                    id=f'{str(enz)}_{cut_index}',
                    name=str(enz),
                    cut_index=cut_index,
                    rec_seq=actual_rec,
                    rec_start=rec_start,
                    rec_end=rec_end,
                    top_cut_in_rec=top_in_display,
                    bot_cut_in_rec=bot_in_display,
                    comp_seq=_complement(actual_rec),
                    spacer=spacer,
                    bot_cut_index=bot_cut_index,
                    is_unique=is_unique,
                ))

        project.enzymes[:] = result


def _parse_elucidate(eluc: str, rec_site: str) -> tuple[int, int]:
    """Parse Biopython elucidate() → (top_cut_in_rec, bot_cut_in_rec).

    Both 0-indexed WITHIN the recognition sequence (not the full elucidate string).
    Handles flanking context bases (e.g. 'N^GTSAC_N' for NmuCI).
    """
    clean = eluc.replace("^", "").replace("_", "")
    tc = eluc.find("^")
    bc = eluc.find("_")

    # Adjust for markers before the caret/underscore
    if tc >= 0:
        tc -= eluc[:tc].count("_")
    if bc >= 0:
        bc -= eluc[:bc].count("^")

    # Account for flanking bases in elucidate: find where rec_site begins in clean
    site_start = clean.find(rec_site)
    if site_start >= 0:
        tc -= site_start
        bc -= site_start

    if tc < 0:
        tc = 0
    if bc < 0:
        bc = 0

    return (max(0, tc), max(0, bc))


def _locate_recognition(ctx: str, site: str, cut_pos: int, top_off: int) -> tuple[int, str, bool]:
    """Find the recognition in context, trying forward then reverse-complement.

    Returns (pos_in_ctx, matched_seq, is_bottom_strand).
    """
    # Try forward
    pos = _fuzzy_find(ctx, site)
    if pos >= 0:
        return (pos, ctx[pos:pos + len(site)], False)

    # Try reverse complement
    rc_site = _complement(site)
    pos = _fuzzy_find(ctx, rc_site)
    if pos >= 0:
        return (pos, ctx[pos:pos + len(rc_site)], True)

    # Fallback: use elucidate offset from cut (assume top strand)
    pos = cut_pos - top_off
    if pos < 0:
        pos = 0
    return (pos, ctx[pos:pos + len(site)], False)


def _fuzzy_find(ctx: str, site: str) -> int:
    """Find site in ctx, handling IUPAC ambiguous bases."""
    import re
    iupac = {'N': '.', 'R': '[AG]', 'Y': '[CT]', 'W': '[AT]', 'S': '[CG]', 'K': '[GT]', 'M': '[AC]',
             'B': '[CGT]', 'D': '[AGT]', 'H': '[ACT]', 'V': '[ACG]'}
    pat = ''.join(iupac.get(c, c) for c in site)
    m = re.search(pat, ctx)
    return m.start() if m else -1


def _complement(seq: str) -> str:
    table = str.maketrans("ATGCatgc", "TACGtacg")
    return seq.translate(table)[::-1]


def create_plugin() -> PluginProtocol:
    return EnzymeEnginePlugin()
