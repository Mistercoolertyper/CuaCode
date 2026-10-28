#!/usr/bin/env python3
"""The agent's own window is the one its console is attached to.

A process name is not an identity on Windows: two terminals are two windows of the
same executable, and looking itself up by name moved the neighbour instead.

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
import psutil, win32con, win32gui, win32process


def fake_desktop(windows):
    """Stand in for the window server: `windows` is [(hwnd, title, process)].

    The real backend asks the same questions, so answering them here is what makes
    this a test of the matching rule rather than of a stub.
    """
    by_handle = {h: (t, p) for h, t, p in windows}
    saved = (win32gui.EnumWindows, win32gui.IsWindowVisible, win32gui.GetWindowText,
             win32gui.GetWindowLong, win32process.GetWindowThreadProcessId)

    win32gui.EnumWindows = lambda cb, extra: [cb(h, extra) for h in by_handle]
    win32gui.IsWindowVisible = lambda h: True
    win32gui.GetWindowText = lambda h: by_handle.get(h, ("", ""))[0]
    win32gui.GetWindowLong = lambda h, index: 0
    win32process.GetWindowThreadProcessId = lambda h: (1, by_handle.get(h, ("", ""))[1])

    real_process = psutil.Process

    class P:
        def __init__(self, pid):
            self.name = lambda: {"terminal.exe": "WindowsTerminal.exe",
                                 "explorer.exe": "explorer.exe"}.get(pid, "app.exe")

    psutil.Process = P

    def restore():
        (win32gui.EnumWindows, win32gui.IsWindowVisible, win32gui.GetWindowText,
         win32gui.GetWindowLong, win32process.GetWindowThreadProcessId) = saved
        psutil.Process = real_process

    return restore


original = {n: getattr(win, n) for n in ("_console_window", "_class_of", "_console_title")}
try:
    # A classic console is its own window; nothing else is consulted.
    win._console_window = lambda: 42
    win._class_of = lambda hwnd: "ConsoleWindowClass"
    win._console_title = lambda: ""
    check("a classic console window is the answer itself", win.self_window() == 42)

    win._console_window = lambda: 900
    win._class_of = lambda hwnd: "PseudoConsoleWindow"
    win._console_title = lambda: "C:\\Windows\\System32\\cmd.exe - cuacode  --resume"

    # The live shape: a second session of the same shell, a folder window named
    # after this checkout, and the real frame. The Explorer window is the one that
    # used to be moved left, and the second terminal is the one a prefix match
    # would pick instead.
    restore = fake_desktop([
        (330178, ".cuacode - File Explorer", "explorer.exe"),
        (8195276, "C:\\Windows\\System32\\cmd.exe - cuacode", "terminal.exe"),
        (527332, "C:\\Windows\\System32\\cmd.exe - cuacode  --resume", "terminal.exe"),
    ])
    try:
        got = win.self_window()
    finally:
        restore()
    check("the terminal hosting the console is found", got == 527332, got)
    check("...not a File Explorer window named after the checkout", got != 330178, got)
    check("...and not the other session of the same shell", got != 8195276, got)

    # Nothing answers to the console's title: no window at all is better than the
    # wrong one, because the caller moves whatever comes back.
    restore = fake_desktop([
        (330178, ".cuacode - File Explorer", "explorer.exe"),
        (8195276, "C:\\Windows\\System32\\cmd.exe - cuacode", "terminal.exe"),
    ])
    try:
        got = win.self_window()
    finally:
        restore()
    check("no window answers to the title, so none is claimed", got is None, got)

    # Explorer is never the frame around a console, even when it answers exactly.
    restore = fake_desktop([(555, "C:\\Windows\\System32\\cmd.exe - cuacode  --resume", "explorer.exe")])
    try:
        got = win.self_window()
    finally:
        restore()
    check("the shell is never the terminal", got is None, got)

    # The climb that used to do this cannot work, so it must not silently match.
    check("the process-owned window lookup no longer guesses", win._visible_window(1234, "anything") is None)

    # Two sessions in one terminal differ only by the flag on the end, so the exact
    # title is the only thing that separates them. This is the live shape before the
    # fix: both windows matched the console's title, and the wrong one was moved.
    restore = fake_desktop([
        (8195276, "C:\\Windows\\System32\\cmd.exe - cuacode", "terminal.exe"),
        (527332, "C:\\Windows\\System32\\cmd.exe - cuacode  --resume", "terminal.exe"),
    ])
    try:
        win._console_title = lambda: "C:\\Windows\\System32\\cmd.exe - cuacode  --resume"
        got_resume = win.self_window()
        win._console_title = lambda: "C:\\Windows\\System32\\cmd.exe - cuacode"
        got_plain = win.self_window()
    finally:
        restore()
    check("a session is told apart from the other by its exact title", got_resume == 527332, got_resume)
    check("...and the other session resolves to itself, not to the first one found",
          got_plain == 8195276, got_plain)

    # Prefix is the wider search, so two windows that only start with the title are
    # ambiguous. A guess here moves somebody else's window, which is the bug.
    restore = fake_desktop([
        (101, "C:\\Windows\\System32\\cmd.exe - cuacode x", "terminal.exe"),
        (102, "C:\\Windows\\System32\\cmd.exe - cuacode y", "terminal.exe"),
    ])
    try:
        win._console_title = lambda: "C:\\Windows\\System32\\cmd.exe - cuacode"
        got = win.self_window()
    finally:
        restore()
    check("two windows that only start with the title are a question, not an answer",
          got is None, got)

    # One prefix-only match is still usable: a terminal appends its own decoration.
    restore = fake_desktop([(103, "C:\\Windows\\System32\\cmd.exe - cuacode (unread)", "terminal.exe")])
    try:
        win._console_title = lambda: "C:\\Windows\\System32\\cmd.exe - cuacode"
        got = win.self_window()
    finally:
        restore()
    check("a single window that starts with the title is used", got == 103, got)

    # A window that merely MENTIONS the title is not this console. This is the
    # original bug one radius smaller: a short console title like `cuik` would be
    # found inside a browser tab or a log window, and that window would be moved.
    restore = fake_desktop([
        (201, "Pull request \u00b7 cuik \u00b7 GitHub - Firefox", "browser.exe"),
        (202, "build log: cuik finished", "terminal.exe"),
    ])
    try:
        win._console_title = lambda: "cuik"
        got = win.self_window()
    finally:
        restore()
    check("a window that only contains the title is not the terminal", got is None, got)

    # An elevated console answers `cmd.exe` while its window says
    # `Administrator: cmd.exe`. That is the same window, and the equality test
    # alone would miss it -- which loses the session most in need of parking.
    restore = fake_desktop([(301, "Administrator: cmd.exe", "terminal.exe")])
    try:
        win._console_title = lambda: "cmd.exe"
        got = win.self_window()
    finally:
        restore()
    check("an elevated shell is still this console", got == 301, got)

    # The decoration comes off before the forward comparison, so the elevated
    # window is not a prefix match against every console title starting with cmd.
    restore = fake_desktop([
        (401, "Administrator: cmd.exe", "terminal.exe"),
        (402, "cmd.exe - cuacode", "terminal.exe"),
    ])
    try:
        win._console_title = lambda: "cmd.exe - cuacode"
        got = win.self_window()
    finally:
        restore()
    check("the elevated window is not a prefix match for an unrelated console",
          got == 402, got)

    # --- the parent link, which is what makes identical titles survivable ---
    #
    # Three sessions of the same command give three windows with the same title.
    # The pseudo-console is a child of the frame that draws it, so that link is
    # exact and does not look at a title at all. Measured on the live desktop:
    # 8326030 -> 527332, 7277792 -> 8195276, 13699928 -> 8390972, two of those
    # titles identical.
    saved_getparent = win32gui.GetParent
    saved_owner = win32gui.GetWindow
    saved_iswindow = win32gui.IsWindow
    try:
        win._console_window = lambda: 900
        win._class_of = lambda hwnd: "PseudoConsoleWindow"
        # A title that matches two windows, and a parent that names exactly one.
        win._console_title = lambda: "C:\\Windows\\System32\\cmd.exe - cuacode"
        # Handles here are numbers, not real windows, so the liveness check has
        # to answer for them or _frame_of declines before looking at the parent.
        win32gui.IsWindow = lambda hwnd: bool(hwnd)
        parents = {900: 8390972}
        win32gui.GetParent = lambda hwnd: parents.get(hwnd, 0)
        win32gui.GetWindow = lambda hwnd, which: 0

        restore = fake_desktop([
            (8195276, "C:\\Windows\\System32\\cmd.exe - cuacode", "terminal.exe"),
            (8390972, "C:\\Windows\\System32\\cmd.exe - cuacode", "terminal.exe"),
        ])
        try:
            got = win.self_window()
        finally:
            restore()
        check("identical titles: the console's parent frame decides", got == 8390972, got)
        check("...and the title never gets a say", got != 8195276, got)

        # A pseudo-console with no parent and an ambiguous title: no answer. It
        # must not fall back to something arbitrary.
        win32gui.GetParent = lambda hwnd: 0
        restore = fake_desktop([
            (8195276, "C:\\Windows\\System32\\cmd.exe - cuacode", "terminal.exe"),
            (8390972, "C:\\Windows\\System32\\cmd.exe - cuacode", "terminal.exe"),
        ])
        try:
            got = win.self_window()
        finally:
            restore()
        check("no parent and a collided title claims nothing", got is None, got)

        # The owner is the same relationship from the other side, for a console
        # window that is owned rather than parented.
        win32gui.GetParent = lambda hwnd: 0
        win32gui.GetWindow = lambda hwnd, which: 8195276 if hwnd == 900 else 0
        restore = fake_desktop([
            (8195276, "C:\\Windows\\System32\\cmd.exe - cuacode", "terminal.exe"),
            (8390972, "C:\\Windows\\System32\\cmd.exe - cuacode", "terminal.exe"),
        ])
        try:
            got = win.self_window()
        finally:
            restore()
        check("an owned console window resolves through its owner", got == 8195276, got)
    finally:
        win32gui.GetParent = saved_getparent
        win32gui.GetWindow = saved_owner
        win32gui.IsWindow = saved_iswindow
finally:
    for name, value in original.items():
        setattr(win, name, value)

from tools import _window


class Backend:
    def __init__(self, hwnd, raise_hwnd=True):
        self.hwnd = hwnd
        self.raise_hwnd = raise_hwnd
        self.snapped = []
        self.raised = []

    def self_window(self):
        return self.hwnd

    def snap_region(self, handle, x0, x1, focus=False):
        self.snapped.append((handle, x0, x1, focus))
        return True

    def raise_window(self, hwnd):
        self.raised.append(hwnd)
        return self.raise_hwnd


class NamedBackend:
    """A backend with no self_window at all: the name is all it has."""

    def __init__(self):
        self.snapped = []

    def snap_region(self, handle, x0, x1, focus=False):
        self.snapped.append((handle, x0, x1, focus))
        return True


backend = Backend(888)
_window._BACKEND = backend
moved, raised = _window.park_self("WindowsTerminal")
check("the console window is moved, not a window of the name", moved)
check("...and the name is never asked for",
      backend.snapped == [(888, _window.SELF_REGION[0], _window.SELF_REGION[1], False)],
      backend.snapped)
check("not raised unless asked", raised is False and backend.raised == [], (raised, backend.raised))

backend = Backend(888)
_window._BACKEND = backend
moved, raised = _window.park_self("WindowsTerminal", raise_it=True)
check("asked to raise, the terminal goes to the front", raised and backend.raised == [888], backend.raised)
check("...without being given focus",
      backend.snapped == [(888, 0.0, 0.3, False)], backend.snapped)

backend = Backend(None)
_window._BACKEND = backend
moved, raised = _window.park_self("WindowsTerminal")
# A backend that can identify a window answering None is "no answer", not
# "try the name". The name is a process, and three sessions share it, so using
# it here moves one of the other two. Measured: that is the bug the user hit.
check("a backend that cannot find its window moves nothing", not moved and not raised)
check("...rather than parking a terminal the name happens to match",
      backend.snapped == [], backend.snapped)

named = NamedBackend()
_window._BACKEND = named
moved, raised = _window.park_self("WindowsTerminal")
check("a backend with no self_window at all uses the name", moved)
check("...using the name the caller passed",
      named.snapped == [("WindowsTerminal", 0.0, 0.3, False)], named.snapped)

backend = Backend(888, raise_hwnd=False)
_window._BACKEND = backend
moved, raised = _window.park_self("WindowsTerminal", raise_it=True)
check("a window manager that will not raise is not an error", moved and raised is False)

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
