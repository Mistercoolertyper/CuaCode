"""Which MCP servers exist on this machine.

The bundled file here, the user's in ~/.cuacode, and a name collision goes to the user's. Read every time rather than cached, so a server registered mid-conversation is usable next turn. The bundled file is deliberately empty: an MCP server is a local process with the user's privileges, so nothing should be running by default just because it sat in the repo.
The config key is `mcpServers`, spelled as Claude Desktop and Claude Code spell it, so a block pastes between them without translation.
"""
from __future__ import annotations

import json
import platform
import sys
from pathlib import Path

from handler import catalog

FILENAME = "servers.json"


def bundled_file() -> Path:
    return catalog.bundled("mcp") / FILENAME


def user_dir() -> Path:
    return catalog.user_dir("mcp")


def user_config() -> Path:
    return user_dir() / FILENAME


def _read(path: Path) -> dict:
    if not path.exists():
        return {}
    try:
        data = json.loads(path.read_text() or "{}")
    except (json.JSONDecodeError, OSError) as exc:
        print(f"[mcp] ignoring {path}: {exc}", file=sys.stderr)
        return {}
    if not isinstance(data, dict):
        return {}
    # Bare {name: {...}} is accepted too, so a hand-written file that skipped the
    # wrapper still loads.
    servers = data.get("mcpServers", data)
    return {k: v for k, v in servers.items() if isinstance(v, dict)}


def load_servers() -> dict[str, dict]:
    found: dict[str, dict] = {}
    for path in (bundled_file(), user_config()):                # user second: it overwrites
        found.update(_read(path))

    out = {}
    for name, cfg in found.items():
        if cfg.get("enabled") is False:
            continue
        # A macOS-only server on Linux is not an error to report later, it
        # is a server that should never have been offered.
        wanted = cfg.get("platform")
        if wanted and platform.system().lower() != str(wanted).lower():
            continue
        out[name] = cfg
    return dict(sorted(out.items()))


def get(name: str) -> dict:
    servers = load_servers()
    if name not in servers: raise catalog.unknown("MCP server", name, servers)
    return servers[name]
