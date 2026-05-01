"""
Plugin protocol — all plugins implement this interface.
Core only depends on this protocol, never on concrete plugins.
"""
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any


@dataclass
class PluginManifest:
    """Metadata exposed by every plugin for discovery."""
    name: str
    version: str
    description: str = ""
    author: str = ""
    cli_commands: list[Any] = field(default_factory=list)  # click.Command
    api_routes: list[Any] = field(default_factory=list)    # fastapi.APIRouter or None


class PluginProtocol(ABC):
    """Every plugin (official or third-party) implements this."""

    @property
    @abstractmethod
    def manifest(self) -> PluginManifest: ...

    @abstractmethod
    def on_load(self, core_api: "CoreApi") -> None:
        """Called once when the plugin is loaded. Receives restricted core API."""
        ...
