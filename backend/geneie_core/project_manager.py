"""In-memory project manager. Maintains global state, fires change callbacks."""
from __future__ import annotations
from typing import Callable
from .models import ProjectData


class ProjectManager:
    def __init__(self):
        self._project: ProjectData | None = None
        self._roi: tuple[int, int] | None = None
        self._callbacks: list[Callable[[ProjectData], None]] = []

    # ── project ──

    @property
    def project(self) -> ProjectData | None:
        return self._project

    def load(self, p: ProjectData) -> None:
        self._project = p
        self._roi = None

    # ── ROI ──

    @property
    def roi(self) -> tuple[int, int] | None:
        return self._roi

    def set_roi(self, s: int, e: int) -> None:
        if self._project is None:
            raise ValueError("No project loaded")
        if s < 0 or e >= self._project.length or s > e:
            raise ValueError(f"ROI {s}..{e} out of bounds")
        self._roi = (s, e)

    def clear_roi(self) -> None:
        self._roi = None

    def roi_sequence(self) -> str:
        if self._project is None:
            return ""
        if self._roi:
            return self._project.sequence[self._roi[0]:self._roi[1] + 1]
        return self._project.sequence

    # ── callbacks ──

    def on_change(self, cb: Callable[[ProjectData], None]) -> None:
        self._callbacks.append(cb)

    def on_sequence_changed(self, cb: Callable[[ProjectData], None]) -> None:
        """Alias for on_change — called by plugins after recomputation."""
        self._callbacks.append(cb)

    def notify(self) -> None:
        if self._project is None:
            return
        for cb in self._callbacks:
            cb(self._project)

    # ── edits ──

    def update_sequence(self, seq: str) -> None:
        if self._project is None:
            return
        self._project.sequence = seq.upper()
        self._project.length = len(seq)
        self.notify()

    def update_features(self, feats: list) -> None:
        if self._project is None:
            return
        from .models import Feature
        self._project.features = [Feature(**f) for f in feats]
        self.notify()

    def update_primers(self, primers: list) -> None:
        if self._project is None:
            return
        from .models import Primer
        self._project.primers = [Primer(**p) for p in primers]
        self.notify()
