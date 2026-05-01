"""CLI entry point — every operation maps to a click command."""
from __future__ import annotations
import json
import click
from pathlib import Path
from .file_io import parse_file
from .models import ProjectData


# Global project state (in-memory; later replaced by a proper manager)
_project: ProjectData | None = None
_roi: tuple[int, int] | None = None


def _ensure_project():
    if _project is None:
        raise click.UsageError("No project loaded. Use 'geneie open <file>' first.")


def _roi_view(data: str) -> str:
    """Return the portion of data within ROI, or full data if no ROI."""
    if _roi is None:
        return data
    s, e = _roi
    return data[s:e + 1]


# ── Top-level CLI ──────────────────────────────────────────────

@click.group()
def main():
    """Geneie — plasmid editor backend."""


# ── File commands ──────────────────────────────────────────────

@main.command()
@click.argument("path", type=click.Path(exists=True))
def open(path: str):
    """Open a .gbk, .dna, or .fasta file."""
    global _project
    _project = parse_file(Path(path))
    click.echo(f"Loaded {_project.length} bp, {len(_project.features)} features")


@main.command()
@click.argument("path", type=click.Path())
def save(path: str):
    """Save current project as .gbk (100% round-trip fidelity)."""
    _ensure_project()
    from .file_io import write_gbk
    write_gbk(_project, Path(path))
    click.echo(f"Saved → {path}")


# ── Sequence commands ──────────────────────────────────────────

@main.group()
def seq():
    """Sequence operations."""


@seq.command()
@click.argument("range_spec", required=False)
def select(range_spec: str | None):
    """Select a sequence range, e.g. '10..40'. Omit to show selection."""
    _ensure_project()
    global _roi
    if range_spec is None:
        if _roi:
            click.echo(f"ROI: {_roi[0]}..{_roi[1]}")
        else:
            click.echo("No ROI set (viewing full sequence)")
        return
    parts = range_spec.split("..")
    s, e = int(parts[0]), int(parts[1])
    if s < 0 or e >= _project.length or s > e:
        raise click.BadParameter(f"Range {range_spec} out of bounds (0..{_project.length - 1})")
    _roi = (s, e)
    frag = _project.sequence[s:e + 1]
    click.echo(f"ROI: {s}..{e} ({len(frag)} bp)\n{frag}")


@seq.command()
def deselect():
    """Clear region-of-interest."""
    global _roi
    _roi = None
    click.echo("ROI cleared.")


@seq.command()
def show():
    """Show the full sequence (respects ROI)."""
    _ensure_project()
    seq = _roi_view(_project.sequence)
    click.echo(seq)


# ── ROI commands ───────────────────────────────────────────────

@main.group()
def roi():
    """Region-of-interest management (alias for seq select/deselect)."""


@roi.command()
@click.argument("range_spec")
def set(range_spec: str):
    """Set ROI, e.g. '10..280'."""
    ctx = click.get_current_context()
    ctx.invoke(select, range_spec=range_spec)


@roi.command()
def clear():
    """Clear ROI."""
    ctx = click.get_current_context()
    ctx.invoke(deselect)


@roi.command()
def show():
    """Show current ROI."""
    ctx = click.get_current_context()
    ctx.invoke(select, range_spec=None)


# ── Feature commands ───────────────────────────────────────────

@main.group()
def feature():
    """Feature operations."""


@feature.command(name="list")
def feature_list():
    """List features (respects ROI)."""
    _ensure_project()
    for f in _project.features:
        if _roi and (f.end < _roi[0] or f.start > _roi[1]):
            continue
        segs = f.segments or [{"start": f.start, "end": f.end}]
        seg_str = ", ".join(f"{s['start']}..{s['end']}" for s in segs)
        click.echo(f"[{f.id}] {f.name} ({f.ftype}) {seg_str}")


# ── Enzyme commands ────────────────────────────────────────────

@main.group()
def enzyme():
    """Enzyme operations."""


@enzyme.command(name="list")
@click.option("--unique", is_flag=True, help="Only unique cutters")
def enzyme_list(unique: bool):
    """List enzyme cut sites (plugin computed)."""
    _ensure_project()
    if not _project.enzymes:
        click.echo("No enzyme data. Install the enzyme_engine plugin.")
        return
    for e in _project.enzymes:
        if unique and not e.is_unique:
            continue
        if _roi and (e.cut_index < _roi[0] or e.cut_index > _roi[1]):
            continue
        tag = " (unique)" if e.is_unique else ""
        click.echo(f"{e.name} @ {e.cut_index}{tag}")


# ── Primer commands ────────────────────────────────────────────

@main.group()
def primer():
    """Primer operations."""


@primer.command(name="list")
def primer_list():
    """List primers."""
    _ensure_project()
    for p in _project.primers:
        click.echo(f"[{p.id}] {p.name} ({p.type}) {p.match_start}..{p.match_end}")


# ── Export ─────────────────────────────────────────────────────

@main.command()
@click.option("--output", "-o", type=click.Path(), help="Output file (default: stdout)")
def export(output: str | None):
    """Export project as JSON."""
    _ensure_project()
    import dataclasses
    data = dataclasses.asdict(_project)
    if _roi:
        s, e = _roi
        data["sequence"] = data["sequence"][s:e + 1]
        data["length"] = e - s + 1
        data["roi"] = [s, e]
    payload = json.dumps(data, indent=2)
    if output:
        Path(output).write_text(payload)
        click.echo(f"Exported to {output}")
    else:
        click.echo(payload)


# ── Server ─────────────────────────────────────────────────────

@main.command()
@click.option("--host", default="127.0.0.1", help="Bind address")
@click.option("--port", default=8765, help="Port")
def serve(host: str, port: int):
    """Start the backend API server."""
    from .server import start
    click.echo(f"Geneie backend → http://{host}:{port}")
    start(host, port)


if __name__ == "__main__":
    main()
