#!/usr/bin/env python3
"""A turn that lost a block says so, and a fold outside a turn finds its session.

Two fallbacks, both of which used to be silent:

- `recall.block`, the docs block and the skill block are each wrapped in
  `except Exception` because none of them may fail a turn. They set the value to
  "" and said nothing, so a turn ran without the project's own notes and looked
  exactly like one that had them.
- `_compact_now`'s "fall back to the live session" imported `handler.session.naming`,
  a module that has never existed. The ImportError was swallowed by the same
  `except Exception: pass`, so `/compact` with no turn around it did nothing, and
  did it quietly.

    python3 tests/test_turn_blocks.py
"""
import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
os.environ.setdefault("CUACODE_HOME", tempfile.mkdtemp(prefix="cuacode-blocks-"))

FAILS = []


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond and detail else ""))
    if not cond: FAILS.append(name)


# ---- 1. a fold with no turn around it finds the live session ----

from handler.agent import main as loop
from integrations.memory import naming


class Live:
    """Enough of a Session for the fold's first question: is there one at all."""
    asked = False

    def records(self):
        Live.asked = True
        return []


naming.set_live(Live())
said = loop._compact_now([], used=10, limit=5, ctx={})
check("a fold outside a turn reads the live session", Live.asked,
      "it returned without asking the session anything")
check("and says there is nothing to fold", said == "", said)


# ---- 2. a block that failed is a notice, and it names the block ----

import handler.worker as worker_mod
from handler.worker import Worker


class FakeIPC:
    terminal_info = {"cwd": str(ROOT), "term_program": "test"}

    def __init__(self):
        import threading
        self.sent = []
        self.background = threading.Event()
        self._cancelled = threading.Event()

    def send(self, type_, data=None, id_=None):
        self.sent.append((type_, data))
        return id_ or "fake"

    def reply(self, env, type_, data=None):
        self.sent.append((type_, data))
        return env.id

    def begin_run(self): pass
    def end_run(self): pass
    def cancelled(self): return self._cancelled.is_set()
    def take_steer(self): return []
    def call(self, *a, **k): return None


def raises(*a, **k):
    raise RuntimeError("the mailbox is a directory, not a file")


real_block, real_docs, real_skill = worker_mod.recall.block, worker_mod.instructions.docs_block, worker_mod.skills.invocation
worker_mod.recall.block = raises
worker_mod.instructions.docs_block = raises
worker_mod.skills.invocation = raises
ipc = FakeIPC()
w = Worker(ipc, detached=True)
events = []
try:
    gen = w.turn("hello")
    for typ, payload in gen:
        events.append((typ, payload))
        if len([e for e in events if (e[1] or {}).get("state") == "notice"]) >= 3: break
    gen.close()
finally:
    worker_mod.recall.block, worker_mod.instructions.docs_block, worker_mod.skills.invocation = real_block, real_docs, real_skill

notices = [str((p or {}).get("token", "")) for t, p in events if (p or {}).get("state") == "notice"]
joined = " | ".join(notices)
check("three failed blocks, three notices", len(notices) == 3, joined)
for named, word in (("recall", "recall"), ("docs", "docs block"), ("skill", "skill block")):
    check(f"the {named} failure is named",
          any(word in n for n in notices), joined)
check("each notice carries the error",
      all("the mailbox is a directory" in n for n in notices), joined)
check("and says what the turn goes without",
      any("carries no recalled" in n for n in notices), joined)

if FAILS:
    print(f"\n{len(FAILS)} check(s) failed: " + ", ".join(FAILS))
    sys.exit(1)
print("\nall turn-block checks passed")
