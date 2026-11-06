#!/usr/bin/env python3
"""A menu that could not be read says so instead of saying "nothing installed".

Five tools put their list in their own description, and each one caught every
exception from its loader and returned {} or []. describe() then told the model
"No skills are installed", "No subagents are installed", "No workflows are
installed", "No MCP servers are registered", "Nothing remembered yet" -- five
statements of fact about the machine, all of them false, all of them acted on.

    python3 tests/test_menu_failures.py
"""
import importlib
import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
os.environ.setdefault("CUACODE_HOME", tempfile.mkdtemp(prefix="cuacode-menu-"))

FAILS = []


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond and detail else ""))
    if not cond: FAILS.append(name)


def raises(*a, **k):
    raise PermissionError("the folder is mode 000")


# (module, attribute on it, the module it is a facade for, the function to break,
#  what describe() used to say, and what has to appear in its place)
CASES = [
    ("tools.skill.main", "skills", "load_skills", "No skills are installed"),
    ("tools.agent.main", "agents", "load_agents", "No subagents are installed"),
    ("tools.workflow.main", "wf", "listing", "No workflows are installed"),
    ("tools.mcp.main", "loader", "load_servers", "No MCP servers are registered"),
]

for module_name, attr, fn_name, old_claim in CASES:
    mod = importlib.import_module(module_name)
    target = getattr(mod, attr)
    original = getattr(target, fn_name)
    setattr(target, fn_name, raises)
    try:
        described = mod.describe("the tool's own description")
        schema = mod.schema()
    finally:
        setattr(target, fn_name, original)
    check(f"{fn_name}: the failure is in the description",
          "could not be read" in described and "mode 000" in described, described[-160:])
    check(f"{fn_name}: and it does not claim there are none", old_claim not in described, described[-160:])
    check(f"{fn_name}: the schema is still a schema", isinstance(schema, dict) and "properties" in schema, schema)

# memory's list is the index itself rather than a folder of files, so it is a
# string appended to the description instead of a list of names.
mem = importlib.import_module("tools.memory.main")
original = mem.loader.index_lines
mem.loader.index_lines = raises
try:
    described = mem.describe("the tool's own description")
finally:
    mem.loader.index_lines = original
check("the memory index says it could not be read",
      "could not be read" in described and "mode 000" in described, described[-160:])
check("and does not claim nothing is remembered",
      "Nothing remembered yet" not in described, described[-160:])

if FAILS:
    print(f"\n{len(FAILS)} check(s) failed: " + ", ".join(FAILS))
    sys.exit(1)
print("\nall menu checks passed")
