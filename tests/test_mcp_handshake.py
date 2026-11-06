#!/usr/bin/env python3
"""When both MCP openings fail, the error names both.

`_handshake` probes the stateless protocol and falls back to the legacy
`initialize`. The fallback's exception propagated as the whole story, so a
failure read as "the legacy opening failed" when the probe had already said
something more useful -- or the other way round, depending on which server.

    python3 tests/test_mcp_handshake.py
"""
import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
os.environ.setdefault("CUACODE_HOME", tempfile.mkdtemp(prefix="cuacode-mcp-"))

from integrations.mcp import client

FAILS = []


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond and detail else ""))
    if not cond: FAILS.append(name)


def refuse(what):
    def _raise(*a, **k):
        raise what
    return _raise


# auto: probe says legacy, legacy handshake then fails too.
conn = client.Connection("demo", {"transport": "stdio"})
conn._discover = refuse(client._Legacy("no answer to server/discover in 3s"))
conn._legacy_handshake = refuse(client.MCPError("initialize: server exited during startup"))
try:
    conn._handshake()
    check("a server that answers neither protocol is an error", False, "it returned")
except client.MCPError as e:
    said = str(e)
    check("the error says neither answered", "neither protocol answered" in said, said)
    check("and carries the probe's reason", "no answer to server/discover" in said, said)
    check("and the legacy opening's reason", "server exited during startup" in said, said)
    check("and names both revisions", client.MODERN in said and client.PROTOCOL in said, said)
    check("and names the server", said.startswith("demo:"), said)

# modern: the fallback is refused on purpose, and the probe's reason is the error.
conn = client.Connection("demo", {"transport": "stdio", "protocol": "modern"})
conn._discover = refuse(client._Legacy("speaks only 2025-06-18"))
conn._legacy_handshake = refuse(AssertionError("the fallback must not run"))
try:
    conn._handshake()
    check("a modern-only config refuses the fallback", False, "it returned")
except client.MCPError as e:
    check("the refusal keeps the probe's reason", "speaks only 2025-06-18" in str(e), str(e))

# legacy: forced, no probe at all.
ran = []
conn = client.Connection("demo", {"transport": "stdio", "protocol": "legacy"})
conn._discover = refuse(AssertionError("a forced legacy config must not probe"))
conn._legacy_handshake = lambda: ran.append(True)
conn._handshake()
check("a forced legacy config goes straight to initialize", ran == [True], ran)

if FAILS:
    print(f"\n{len(FAILS)} check(s) failed: " + ", ".join(FAILS))
    sys.exit(1)
print("\nall MCP handshake checks passed")
