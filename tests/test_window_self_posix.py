#!/usr/bin/env python3
"""The self-window questions the Windows test does not ask, on the backends that
have no `self_window` at all.

`tests/test_window_snap.py` proves the Windows climb. Linux and macOS do not have
that climb -- they have no `self_window`, no `foreground_app` and no `raise_window` --
so what is left to hold them to is the fallback behaviour in `tools/_window.py`:

    python tests/test_window_self_posix.py        (Windows, Linux or macOS)

No window server is needed: every backend below is a stand-in, which is the point.
These are the invariants that survive a platform with no window identity.
"""

import importlib.util, platform, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

FAILS = []


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond and detail else ""))
    if not cond:
        FAILS.append(name)


from tools import _window

SELF = _window.SELF_REGION
APP = _window.APP_REGION


class PosixBackend:
    """A backend shaped like _open_linux / _open_macos: no self_window, no
    foreground_app, and gui_apps keyed by whatever identity that platform has."""

    def __init__(self, apps=None):
        self.apps = dict(apps or {})
        self.calls = []

    def gui_apps(self):
        return dict(self.apps)

    def wait_for_window(self, handle):
        return True

    def snap_region(self, handle, x0, x1, focus=False):
        self.calls.append((handle, x0, x1, focus))
        return True


def reset(backend):
    """A fresh worker: the app already up predates it, so baseline() is the first look.

    Without the baseline the first park_new is the seeding look itself, which parks
    nothing on purpose -- that is the property, not a bug to work around.
    """
    _window._BACKEND = backend
    _window._seeded = False
    _window._seen = {}
    _window._parked = {}
    _window.baseline()
    return backend


# --- park_self: with no self_window, the name is the only answer --------------

backend = PosixBackend({"xterm": "xterm"})
reset(backend)
moved, raised = _window.park_self("xterm")
check("a backend without self_window still parks the terminal by name", moved)
check("...into the left strip, without focus",
      backend.calls == [("xterm", SELF[0], SELF[1], False)], backend.calls)

backend = PosixBackend({"xterm": "xterm"})
reset(backend)
moved, raised = _window.park_self(None)
check("no name and no self_window moves nothing", not moved and not raised)
check("...rather than parking some stranger",
      backend.calls == [], backend.calls)

backend = PosixBackend({"xterm": "xterm"})
reset(backend)
moved, raised = _window.park_self(None, raise_it=True)
check("raise_it cannot raise a window nobody can name", not raised and backend.calls == [],
      backend.calls)

# --- park_active: a backend without foreground_app is a no-op, not an error --

backend = PosixBackend({"xterm": "xterm"})
reset(backend)
check("park_active on a backend with no foreground_app answers nothing",
      _window.park_active("xterm") == "")
check("...and parks nothing", backend.calls == [], backend.calls)

# --- park_new: an app that appeared is parked right, and never raised here ----

backend = PosixBackend({"xterm": "xterm"})
reset(backend)
backend.apps["firefox"] = "firefox"
parked = _window.park_new("xterm", focus=False)          # screenshot passes False off Windows
check("a new app is parked", parked == ["firefox"], parked)
check("...on the right, unraised",
      backend.calls == [("firefox", APP[0], APP[1], False)], backend.calls)

backend = PosixBackend({"xterm": "xterm"})
reset(backend)
_window.park_new("xterm", focus=False)
backend.apps["firefox"] = "firefox"
_window.park_new("xterm", focus=False)
_window.park_new("xterm", focus=False)
check("a parked app is not parked twice", len(backend.calls) == 1, backend.calls)

# --- the identity each backend uses ------------------------------------------

# Windows: gui_apps is keyed by process name, so two windows of one process share a key.
backend = PosixBackend({"xterm": "xterm"})
reset(backend)
backend.apps["firefox"] = 11
_window.park_new("xterm", focus=False)
backend.apps["firefox"] = 22                            # a second window of the same app
_window.park_new("xterm", focus=False)
check("a second window of an app already seen is not parked",
      backend.calls == [(11, APP[0], APP[1], False)], backend.calls)

# Linux: gui_apps is keyed by window title, so a retitled window reads as a new app.
#
# A shell that `cd`s rewrites its title, and the same window then looks like an app
# the agent just opened, so it is parked into the right-hand slice. Left as a note
# rather than a check on purpose: a test that asserts a bug has to be rewritten by
# whoever fixes it, and failing on correct behaviour reads as the test's fault.
#
# Worth knowing when that fix is written: nothing on Linux currently names the
# terminal anyway. `frontmost_app` is filled only on Windows and macOS, so a Linux
# session passes self_name=None, park_self is a no-op and every window that appears
# is fair game for the right side.

print()
print("failed:", len(FAILS))
raise SystemExit(1 if FAILS else 0)
