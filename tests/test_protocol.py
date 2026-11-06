#!/usr/bin/env python3
"""The wire refuses what it cannot read, and the refusal carries the value.

Both halves of this used to fail quietly. `Envelope.from_dict` turned a `data`
that was a string into `{}`, so `{"type":"cmd","id":"1","data":"hello"}` was
answered `unknown action: None` -- the caller's spelling blamed for the caller's
shape. And a line that was not JSON was dropped by the reader thread with a bare
`except json.JSONDecodeError: pass`, so the client waited for a reply nothing was
ever going to send.

    python3 tests/test_protocol.py
"""
import json
import os
import queue
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from handler.protocol import Envelope

FAILS = []


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond and detail else ""))
    if not cond: FAILS.append(name)


# ---- 1. the shape, without a process ----

BAD = [
    ({"type": "cmd", "id": "1", "data": "hello"}, ("data", "str")),
    ({"type": "cmd", "id": "1", "data": [1, 2]}, ("data", "list")),
    ({"type": "cmd", "id": 7, "data": {}}, ("id", "int")),
    ({"id": "1", "data": {}}, ("no type",)),
    ([1, 2], ("JSON object", "list")),
    ("hello", ("JSON object", "str")),
]
for raw, words in BAD:
    try:
        Envelope.from_dict(raw)
        check(f"refuses {raw!r}", False, "it parsed")
    except ValueError as e:
        missing = [w for w in words if w not in str(e)]
        check(f"refuses {raw!r} naming {words}", not missing, str(e))

# The compatibility the frontends are built on, which must survive the refusal:
# an unknown type, an unknown field, and a missing data all still parse.
e = Envelope.from_dict({"type": "teleport", "id": "x", "data": {"action": "jump"}, "extra": 1})
check("an unknown type still parses", e.type == "teleport")
check("an unknown field is carried, not rejected", e.data == {"action": "jump"})
check("a missing data is {}",
      Envelope.from_dict({"type": "status", "id": "x"}).data == {})
check("a null data is {}",
      Envelope.from_dict({"type": "status", "id": "x", "data": None}).data == {})


# ---- 2. the process: a bad line is answered, and the worker keeps serving ----

class Worker:
    def __init__(self):
        self.home = tempfile.mkdtemp(prefix="cuacode-proto-")
        self.lines = queue.Queue()
        self.proc = subprocess.Popen(
            [sys.executable, "main.py"], cwd=str(ROOT),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, bufsize=1,
            env={**os.environ, "PYTHONPATH": str(ROOT), "CUACODE_HOME": self.home},
        )
        threading.Thread(target=self._read, daemon=True).start()
        threading.Thread(target=lambda: self.proc.stderr.read(), daemon=True).start()

    def _read(self):
        for line in self.proc.stdout:
            line = line.strip()
            if line:
                try: self.lines.put(json.loads(line))
                except json.JSONDecodeError: self.lines.put({"type": "junk", "data": {"raw": line}})

    def raw(self, line):
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()

    def take(self, seconds=2.0):
        out, deadline = [], time.monotonic() + seconds
        while time.monotonic() < deadline:
            try: out.append(self.lines.get(timeout=0.1))
            except queue.Empty: continue
        return out

    def stop(self):
        self.proc.stdin.close()
        try: self.proc.wait(timeout=10)
        except Exception: self.proc.kill()


w = Worker()
ready = w.take(8)
check("the worker comes up", any((e.get("data") or {}).get("state") == "ready" for e in ready),
      str(ready[:2]))

w.raw("this is not json")
w.raw('{"type":"cmd","id":"shape","data":"hello"}')
w.raw("[1, 2]")
said = [e for e in w.take(3) if (e.get("data") or {}).get("state") == "error"]
errors = [str((e.get("data") or {}).get("error", "")) for e in said]

check("three bad lines, three answers", len(said) == 3, errors)
check("the line that was not JSON is named",
      any("not JSON" in e and "line 1" in e for e in errors), errors)
check("the data that was a string is named",
      any("data must be a JSON object" in e and "str" in e for e in errors), errors)
check("the shape error answers on its own id",
      any(e.get("id") == "shape" for e in said), [e.get("id") for e in said])
check("the line that was not an object is named",
      any("JSON object" in e and "list" in e for e in errors), errors)

# Still serving: a refusal must not be the last thing the worker ever says.
w.raw('{"type":"cmd","id":"after","data":{"action":"command.list"}}')
after = w.take(1.5)
check("the worker still answers a good command afterwards",
      any(e.get("id") == "after" and e.get("type") == "commands" for e in after),
      str(after)[:300])
w.stop()

if FAILS:
    print(f"\n{len(FAILS)} check(s) failed: " + ", ".join(FAILS))
    sys.exit(1)
print("\nall protocol checks passed")
