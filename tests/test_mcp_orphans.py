"""An MCP server must not outlive the worker that started it.

A lightpanda server orphaned that way on 2026-10-24 held a whole core for a week
before anyone looked. Two rules keep it from repeating, and both are checked here
against real processes rather than a mock: a pidfile whose owner is gone gets swept,
and one whose owner is alive is somebody else's server and is left alone.
"""
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
os.environ["CUACODE_HOME"] = tempfile.mkdtemp(prefix="mcp-check-")

from integrations.mcp import client


def _sleep() -> subprocess.Popen:
    proc = subprocess.Popen(["sleep", "300"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.05)
    return proc


def _dead_pid() -> int:
    proc = subprocess.Popen(["true"])
    proc.wait()
    return proc.pid


def _pidfile(name: str, pid: int, owner: int) -> Path:
    path = client._pidfile(name)
    path.write_text(json.dumps({"name": name, "pid": pid, "owner": owner, "argv0": "sleep"}))
    return path


def test_an_orphan_is_ended_and_its_file_is_gone():
    proc = _sleep()
    path = _pidfile("orphan", proc.pid, _dead_pid())
    ended = client.sweep()
    assert ended == [f"orphan (pid {proc.pid})"], ended
    assert proc.wait(timeout=5) is not None, "the orphan is still running"
    assert not path.exists(), "the pidfile of an ended server is still there"


def test_a_live_owners_server_is_left_alone():
    proc = _sleep()
    path = _pidfile("busy", proc.pid, os.getpid())      # this process is the owner
    try:
        assert client.sweep() == [], "a running session's server was ended"
        assert proc.poll() is None, "a running session's server was killed"
    finally:
        proc.kill()
        proc.wait()
        path.unlink(missing_ok=True)


def test_a_recycled_pid_is_not_a_reason_to_kill():
    proc = _sleep()
    path = _pidfile("recycled", proc.pid, _dead_pid())
    rec = json.loads(path.read_text())
    rec["argv0"] = "something-else-entirely"           # the pid now names another program
    path.write_text(json.dumps(rec))
    try:
        assert client.sweep() == [], "a process whose command line did not match was killed"
        assert proc.poll() is None
    finally:
        proc.kill()
        proc.wait()
        path.unlink(missing_ok=True)


def test_a_server_pinned_at_a_core_is_ended():
    """The watcher's own case: 100% CPU with nothing asked of it, which is exactly
    what the lightpanda server was doing when it was found a week later."""
    proc = subprocess.Popen([sys.executable, "-c", "while True: pass"])
    conn = client.Connection("pinned", {"command": "nothing"})
    wedge, watch = client.WEDGE_SEC, client.WATCH_SEC
    client.WEDGE_SEC, client.WATCH_SEC = 2.0, 0.25
    try:
        time.sleep(0.3)
        conn._watch(proc)
        assert proc.wait(timeout=5) is not None, "a server pinned at a full core was left running"
    finally:
        client.WEDGE_SEC, client.WATCH_SEC = wedge, watch
        if proc.poll() is None:
            proc.kill()
            proc.wait()


def test_cputime_reads_ps_and_answers_none_for_a_dead_pid():
    assert client._cputime(os.getpid()) is not None
    assert client._cputime(_dead_pid()) is None


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_"):
            fn()
            print(f"ok  {name}")
