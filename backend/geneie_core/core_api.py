"""Restricted core API exposed to plugins."""
from __future__ import annotations
from typing import Protocol


class CoreApi(Protocol):
    """Plugins receive an object implementing this protocol.

    It gives them read access to the current project state and
    the ability to register callbacks, but never direct mutation.
    """

    @property
    def project(self) -> "ProjectData":
        """Current project (read-only snapshot for plugins)."""
        ...

    @property
    def roi(self) -> tuple[int, int] | None:
        """Current region-of-interest (inclusive), or None."""
        ...

    def on_sequence_changed(self, callback) -> None:
        """Register a callback(project) fired after every sequence edit."""
        ...
