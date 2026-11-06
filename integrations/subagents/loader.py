"""Agents that live in files.

Same shape as an AgentSpec built inline, and as a tool's Description.md: yaml
frontmatter for the machine-readable half, body for the system prompt. Nothing
here does any running; it reads files into specs and hands them to
subagent.run(), which does not care where a spec came from.

Two directories. The ones shipped with the app live next to this file; the
user's live in ~/.cuacode/subagents, and a user file wins on a name collision so
a bundled agent can be overridden without editing the repo.
"""
from pathlib import Path

from tools.loader import parse_frontmatter
from handler import catalog
from handler.agent.subagent import AgentSpec

_cache = None

def user_dir() -> Path:
    return catalog.user_dir("subagents")

def _spec(path: Path) -> AgentSpec:
    meta, body = parse_frontmatter(path.read_text())
    tools = meta.get("tools")
    if isinstance(tools, str):
        # "*" is every tool, which is not the same as the empty list, which is
        # deliberately none. A bare string otherwise means one tool name.
        tools = None if tools.strip() == "*" else [tools]
    return AgentSpec(
        name=meta.get("name") or path.stem,
        description=meta.get("description", ""),
        system=body,
        # Three spellings, and the middle one is the one that matters: None is
        # every tool, [] is none, a list is those. A file that left the key out
        # reads as None, which is what AgentSpec's own comment promises; the
        # expression that used to be here was a no-op whose comment claimed the
        # opposite of what it did.
        tools=tools,
        schema=meta.get("output"),
        provider=meta.get("provider"),
        model=meta.get("model"),
        # Absent means inherit the conversation's level. A file that does name
        # one is taken at its word, see subagent._effort.
        effort=meta.get("effort", ""),
        max_rounds=int(meta.get("max_rounds", 8)),
        params=meta.get("params"))

def load_agents(refresh: bool = False) -> dict:
    """{name: AgentSpec}. Cached, because this is read on every tool-registry
    build; pass refresh to pick up a file you just wrote."""
    global _cache
    if _cache is not None and not refresh: return _cache
    out = {}
    for f in catalog.layered_files("subagents", "*.md"):   # user last: it overwrites
        try: spec = _spec(f)
        except Exception: continue        # one malformed file must not hide the rest
        out[spec.name] = spec
    _cache = out
    return out

def get(name: str) -> AgentSpec:
    agents = load_agents()
    if name not in agents: raise catalog.unknown("agent", name, agents)
    return agents[name]
