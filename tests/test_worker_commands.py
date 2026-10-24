#!/usr/bin/env python3
"""Drive the real worker over its wire, the way a frontend does.

The bug this exists for: `/compress` raised NameError out of main.py and killed the
worker. Compiling main.py did not catch it -- a NameError inside a function is not a
syntax error -- so the only thing that finds this class of bug is running the worker
and typing the command.

This starts main.py as a subprocess, sends it the envelopes a frontend sends, and
reads the replies off stdout. No model is called for the command itself; the fold's
summarizer will fail against a real provider in a test, which is fine: what is being
checked is that the worker stays alive and answers, not that the summary is good.

    python3 tests/test_worker_commands.py
"""
import json
import os
import queue
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FAILS = []


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond and detail else ""))
    if not cond: FAILS.append(name)


class Worker:
    """main.py, driven the way a frontend drives it."""

    def __init__(self):
        self.lines = queue.Queue()
        self.proc = subprocess.Popen(
            [sys.executable, "main.py"], cwd=str(ROOT),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, bufsize=1,
            env={**os.environ, "PYTHONPATH": str(ROOT),
                 # A config of its own, so a test never writes to the real one.
                 "CUACODE_HOME": os.environ.get("CUACODE_TEST_HOME", str(ROOT / ".test-home"))},
        )
        self.err = []
        threading.Thread(target=self._read, daemon=True).start()
        threading.Thread(target=self._read_err, daemon=True).start()

    def _read(self):
        for line in self.proc.stdout:
            line = line.strip()
            if not line: continue
            try: self.lines.put(json.loads(line))
            except json.JSONDecodeError: self.lines.put({"type": "junk", "data": {"raw": line}})

    def _read_err(self):
        for line in self.proc.stderr:
            self.err.append(line)

    def chat(self, text):
        """A message, in the shape the loop actually reads: a `cmd` whose action is
        `chat`. Sending a bare `chat` envelope looks reasonable and is silently
        dropped, which is what made the first run of this test report nothing."""
        return self.send("cmd", {"action": "chat", "text": text})

    def send(self, type_, data=None, id_=None):
        env = {"type": type_, "id": id_ or "t%d" % time.monotonic_ns(), "data": data or {}}
        self.proc.stdin.write(json.dumps(env) + "\n")
        self.proc.stdin.flush()
        return env["id"]

    def take(self, timeout=20.0):
        """Every reply that arrives within the window."""
        out, deadline = [], time.monotonic() + timeout
        while time.monotonic() < deadline:
            try: out.append(self.lines.get(timeout=0.2))
            except queue.Empty:
                if out: break
        return out

    def notice(self, contains, timeout=25.0):
        """Wait for a notice whose text mentions something, and return it.

        The model is streaming a reply over the same pipe, so taking whatever
        arrives first gets a token chunk and looks like a missing answer. The
        command's own notice is what this test is about.
        """
        deadline = time.monotonic() + timeout
        seen = []
        while time.monotonic() < deadline:
            try: e = self.lines.get(timeout=0.3)
            except queue.Empty: continue
            seen.append(e)
            d = e.get("data") or {}
            text = str(d.get("token") or "") + str(d.get("error") or "")
            if d.get("state") == "notice" and contains in text.lower():
                return text, seen
        return "", seen

    def alive(self): return self.proc.poll() is None

    def close(self):
        try: self.proc.stdin.close()
        except Exception: pass
        try: self.proc.wait(timeout=5)
        except Exception: self.proc.kill()


w = Worker()
try:
    # The ready line, which is what a frontend waits for before sending anything.
    ready = [e for e in w.take(30.0) if e.get("data", {}).get("state") == "ready"]
    check("the worker comes up and says ready", bool(ready), ready[:1])
    if not ready:
        print("stderr:\n" + "".join(w.err)[-3000:])
        raise SystemExit(1)
    sid = ready[0]["data"].get("session_id")

    # A turn, so there is a conversation for /compress to find.
    w.chat("hello")
    # Don't wait for a model reply; the command paths below are what is under test.

    # Every read-only and blocking inline command, through the real dispatcher.
    # The crash was here: /compress reached a NameError in fold_now.
    for cmd, expect in [
        ("/context", "context"),
        ("/usage", "usage"),
        # The command is /compact. /compress is the old spelling and still works;
        # both answer as "compact", which is what the notice says.
        ("/compact", "compact"),
        ("/compact off", "compact"),
        ("/compact auto", "compact"),
        ("/compact 400k", "compact"),
        ("/compact 40%", "compact"),
        ("/compact banana", "compact"),
        # The alias has to keep working for anyone with it in their fingers.
        ("/compress", "compact"),
        ("/compress 400k", "compact"),
    ]:
        w.chat(cmd)
        said, _ = w.notice(expect)
        check(f"{cmd!r} answers without dying", w.alive(),
              "worker exited:\n" + "".join(w.err)[-2000:])
        check(f"{cmd!r} says something about {expect}", bool(said), said[:200])

    check("the worker is still alive at the end", w.alive(),
          "exited:\n" + "".join(w.err)[-2000:])
finally:
    w.close()

print()
if FAILS:
    print("FAILED: " + ", ".join(FAILS))
    sys.exit(1)
print("all worker command checks passed")
