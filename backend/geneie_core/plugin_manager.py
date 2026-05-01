"""Plugin auto-discovery and lifecycle management."""
from __future__ import annotations
import importlib
import pkgutil
from pathlib import Path
from .plugin_protocol import PluginProtocol


def discover_plugins(plugin_dir: str = "geneie_plugins") -> list[PluginProtocol]:
    """Scan the plugin directory for packages exposing a `create_plugin()` function."""
    plugins: list[PluginProtocol] = []

    try:
        pkg = importlib.import_module(plugin_dir)
        pkg_path = Path(pkg.__path__[0])
    except (ImportError, AttributeError):
        return plugins

    for _, name, is_pkg in pkgutil.iter_modules([str(pkg_path)]):
        if not is_pkg:
            continue
        try:
            mod = importlib.import_module(f"{plugin_dir}.{name}")
            if hasattr(mod, "create_plugin"):
                plugin = mod.create_plugin()
                plugins.append(plugin)
        except Exception as e:
            print(f"[geneie] Failed to load plugin '{name}': {e}")

    return plugins
