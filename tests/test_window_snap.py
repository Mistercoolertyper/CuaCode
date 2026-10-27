#!/usr/bin/env python3
"""The agent's own window is the one its console is attached to.

A process name is not an identity on Windows: two terminals are two windows of
the same executable, and a session that looked itself up by name moved the
neighbouring terminal instead. These checks pin the console walk, the boot-time
handle, and the caller that has to prefer a handle over a name.

    python tests/test_window_snap.py
"""

import importlib.util, platform, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

FAILS = []


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond and detail else ""))
    if not cond: FAILS.append(name)


if platform.system() != "Windows":
    print("skip: window handles are a Windows question")
    raise SystemExit(0)

spec = importlib.util.spec_from_file_location(
    "tools.app_open._open_windows", ROOT / "tools" / "app_open" / "_open_windows.py")
win = importlib.util.module_from_spec(spec)
spec.loader.exec_module(win)

original = {n: getattr(win, n) for n in
            ("_console_window", "_console_title", "_class_of", "_pid_of",
             "_visible_window", "_parent_pid")}
try:
    win._console_window = lambda: 42
    win._class_of = lambda hwnd: "ConsoleWindowClass"
    check("a classic console window is the answer itself", win.self_window() == 42)

    win._console_window = lambda: 900
    win._class_of = lambda hwnd: "PseudoConsoleWindow"
    win._console_title = lambda: "quik"
    win._pid_of = lambda hwnd: 100
    seen = []

    def visible(pid, hint=""):
        seen.append((pid, hint))
        return 777 if pid == 200 else None

    win._visible_window = visible
    win._parent_pid = lambda pid: 200 if pid == 100 else 0
    check("a pseudo-console climbs to the frame around the tab", win.self_window() == 777)
    check("the climb starts at the console's own process", bool(seen) and seen[0][0] == 100, seen)
    check("the console title is the tie-breaker", bool(seen) and seen[0][1] == "quik", seen)
finally:
    for name, value in original.items():
        setattr(win, name, value)

from tools import _window


class Backend:
    def __init__(self, hwnd, alive=True):
        self.hwnd = hwnd
        self.alive = alive
        self.snapped = []

    def self_window(self):
        return self.hwnd

    def window_exists(self, hwnd):
        return self.alive

    def snap_region(self, handle, x0, x1, focus=False):
        self.snapped.append((handle, x0, x1, focus))
        return True


backend = Backend(888)
_window._BACKEND = backend
check("the boot handle wins over every name lookup", _window.park_self("WindowsTerminal", 555))
check("...and is the only window moved",
      backend.snapped == [(555, _window.SELF_REGION[0], _window.SELF_REGION[1], False)],
      backend.snapped)
check("a handle is enough without a name", _window.park_self(None, 556))

backend = Backend(777)
_window._BACKEND = backend
check("a stale boot handle falls back to the console window", _window.park_self("WindowsTerminal", 555))

backend = Backend(None, alive=False)
_window._BACKEND = backend
check("no handle anywhere falls back to the name", _window.park_self("WindowsTerminal", 0))
check("...using the name the caller passed",
      backend.snapped == [("WindowsTerminal", 0.0, 0.3, False)], backend.snapped)

class ParkBackend:
    """A backend that records what a park was asked to do."""

    def __init__(self):
        self.calls = []

    def gui_apps(self):
        return {"demo": 11}

    def wait_for_window(self, hwnd):
        return True

    def snap_region(self, handle, x0, x1, focus=False):
        self.calls.append((handle, focus))
        return True


_window._BACKEND = None
park = ParkBackend()
_window._BACKEND = park
_window._seeded = True
_window._seen = {}
_window._parked = {}
_window.park_new("cuacode", focus=True)
check("a newly parked app is raised, not just resized", park.calls == [(11, True)], park.calls)

_window._BACKEND = None
print()
print("failed:", len(FAILS))
raise SystemExit(1 if FAILS else 0)
