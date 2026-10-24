#!/usr/bin/env python3
"""Record a real stdout stream from the worker into fixtures/replay.jsonl.

    python3 fixtures/record.py <worker>/main.py <session-id> [out.jsonl]

Boots the worker against a throwaway CUACODE_HOME seeded with a copy of one
session, so nothing of the user's is written to: a replay never commits. The
blobs come too, because session.load calls messages(), which rehydrates every
record's images; notebook/ and file_undo/ do not and can be enormous. Sends the terminal envelope, session.list and
session.load, and writes every stdout line verbatim.

What comes out is a frontend's whole contract in one file: no worker, no model,
no network. A Rust frontend decodes it in a test and proves itself in a second.

Re-run it after the wire changes; it is the fixture's only writer.
"""

import json
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
TIMEOUT = 30.0


def pump(q, lines, until, timeout=TIMEOUT):
    """Collect stdout lines until `until(parsed)` is true, or time runs out."""
    end = time.time() + timeout
    while time.time() < end:
        try:
            line = q.get(timeout=0.2)
        except queue.Empty:
            continue
        if line is None:
            return False
        lines.append(line)
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        if until(ev):
            return True
    return False


def state_of(ev):
    d = ev.get("data")
    return d.get("state") if isinstance(d, dict) else None


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    worker = sys.argv[1]
    sid = sys.argv[2]
    out = Path(sys.argv[3]) if len(sys.argv) > 3 else REPO / "fixtures" / "replay.jsonl"

    src = Path(os.environ.get("CUACODE_HOME", "~/.cuacode")).expanduser() / "sessions" / sid
    if not (src / "meta.json").is_file():
        sys.exit(f"no session {sid} under {src.parent}")

    home = Path(tempfile.mkdtemp(prefix="cuacode-fixture-"))
    dst = home / "sessions" / sid
    dst.parent.mkdir(parents=True)
    shutil.copytree(src, dst, ignore=shutil.ignore_patterns(
        "notebook", "file_undo", "screenshots"))

    # The terminal envelope's cwd is the session's own project, not wherever this
    # script was run from. It is what the worker recalls memories and project docs
    # against, and a fixture is committed: the recording must not carry the path of
    # the machine that made it.
    cwd = json.loads((dst / "meta.json").read_text()).get("cwd") or str(REPO)

    env = {**os.environ, "CUACODE_HOME": str(home)}
    proc = subprocess.Popen([sys.executable, worker], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, bufsize=1, env=env, cwd=str(REPO))

    lines, q = [], queue.Queue()
    threading.Thread(target=lambda: [q.put(l) for l in proc.stdout] or q.put(None),
                     daemon=True).start()
    errq = queue.Queue()
    threading.Thread(target=lambda: errq.put(proc.stderr.read()), daemon=True).start()

    def send(typ, data, id_):
        proc.stdin.write(json.dumps({"type": typ, "id": id_, "data": data}) + "\n")
        proc.stdin.flush()

    ok = pump(q, lines, lambda ev: state_of(ev) == "ready", timeout=20)
    print("boot:", "ready" if ok else "TIMED OUT")

    send("terminal", {"term": "xterm-256color", "term_program": "fixture",
                      "tty": "/dev/ttys000", "cwd": cwd}, "t1")
    send("cmd", {"action": "session.list"}, "s-list")
    pump(q, lines, lambda ev: ev.get("type") == "sessions", timeout=10)

    send("cmd", {"action": "session.load", "id": sid}, "s-load")
    done = pump(q, lines, lambda ev: ev.get("id") == "s-load" and state_of(ev) == "done",
                timeout=60)
    print("replay:", "done" if done else "TIMED OUT")

    send("cmd", {"action": "stop"}, "quit")
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
    tail = errq.get() or ""

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("".join(lines))

    mix = Counter((json.loads(l).get("type"), state_of(json.loads(l)))
                  for l in lines if l.strip())
    print(f"\nwrote {out}  ({len(lines)} lines, {out.stat().st_size // 1024} KiB)")
    for (typ, st), n in mix.most_common():
        print(f"  {n:5}  {typ:10} {st or '-'}")
    if tail.strip():
        print("\nstderr:\n" + tail.strip()[-2000:])
    shutil.rmtree(home, ignore_errors=True)
    return 0 if (ok and done) else 1


if __name__ == "__main__":
    sys.exit(main())
