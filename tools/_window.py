"""One place that decides where the agent's windows go.

app_open parks the app it launches and screenshot re-parks the terminal, but an app
that arrived some other way (`firefox &` from the shell, a browser an MCP server
starts for itself) opened wherever it last felt like being, usually on top of the
terminal. So the rule lives here rather than inside one tool: an app that was not
running when the agent started looking is an app the agent caused, and it belongs in
the same right-hand slice as everything else it drives.

State is module level, so it is per worker process, which is the lifetime over which
"new" means "the agent opened it".
"""
import importlib.util, os, platform, threading
from pathlib import Path

OS = platform.system()

# The terminal keeps the left third because it is read, not driven; whatever is
# being driven gets the rest.
SELF_REGION = (0.0, 0.3)
APP_REGION = (0.3, 1.0)

_LOCK = threading.Lock()
_BACKEND = None
_seen: dict = {}            # app -> handle, everything known to have been running
_seeded = False             # whether _seen means anything yet
_parked: dict = {}          # app -> handle, everything this module put in APP_REGION


def _app_key(name) -> str:
    """A process name without a path or .exe, for comparing identities."""
    base = os.path.basename(str(name or "").strip().strip('"')).lower()
    return base[:-4] if base.endswith(".exe") else base


def _same_app(a, b) -> bool:
    a, b = _app_key(a), _app_key(b)
    return bool(a and b) and (a == b or a in b or b in a)


def backend():
    """The platform window module, loaded once for the whole process.

    Loaded by path rather than imported, because tools/ is not a package the tool
    folders import from each other. Loading it once matters: the module holds the
    Accessibility handles and the record of where each window was last verified to be
    parked, and a second copy would mean a second set of empty caches.
    """
    global _BACKEND
    if _BACKEND is None:
        name = {"Darwin": "_open_macos", "Windows": "_open_windows"}.get(OS, "_open_linux")
        path = Path(__file__).parent / "app_open" / f"{name}.py"
        spec = importlib.util.spec_from_file_location(f"tools.app_open.{name}", path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        _BACKEND = mod
    return _BACKEND


def _apps() -> dict:
    """{app name: handle snap_region accepts}, or {} if the platform cannot say.

    Nothing here is allowed to raise: parking is cosmetic and every caller is in the
    middle of doing something that is not, so a window manager that cannot answer must
    not fail a shell command or lose a screenshot.
    """
    try:
        return backend().gui_apps() or {}
    except Exception:                                           # noqa: BLE001
        return {}


def _snap(handle, region, focus: bool) -> bool:
    try:
        return bool(backend().snap_region(handle, region[0], region[1], focus=focus))
    except Exception:                                           # noqa: BLE001
        return False


def _wait_for_window(handle) -> bool:
    """Give a just-launched app time to have a window worth moving."""
    try:
        return bool(backend().wait_for_window(handle))
    except Exception:                                           # noqa: BLE001
        return True


def baseline() -> dict:
    """Record what is running now, so anything later is the agent's doing.

    Called before a command that might launch something, and implicitly by the first
    park_new, so a session that never takes a baseline still does not sweep up the
    windows the user already had open.
    """
    global _seeded
    apps = _apps()
    with _LOCK:
        if apps:
            _seen.update(apps)
            _seeded = True
    return apps


def park_new(self_name: str | None = None, focus: bool = True) -> list:
    """Park every app that has appeared since the last look. Returns their names.

    focus defaults to True because the caller is reporting a launch, and an app the
    agent just opened is the app it is about to drive.
    """
    global _seeded
    apps = _apps()
    if not apps:
        return []
    with _LOCK:
        if not _seeded:
            # First look of the session: everything up right now predates the agent, so
            # it is remembered, not moved.
            _seen.update(apps)
            _seeded = True
            return []
        fresh = [n for n in apps if n not in _seen and not _same_app(n, self_name)]
        _seen.update(apps)

    parked = []
    for name in fresh:
        handle = apps[name]
        # A name in the app list is a process, not yet a window: apps register with the
        # window server well before they have drawn anything, and a snap that lands in
        # that gap moves nothing and reports failure.
        if not _wait_for_window(handle):
            continue
        if _snap(handle, APP_REGION, focus):
            parked.append(name)
            with _LOCK:
                _parked[name] = handle
    return parked


def park_known(self_name: str | None = None) -> list:
    """Re-park what has already been parked once, without raising anything.

    Apps drift: they restore a saved frame, open a second window, or resize after the
    page they loaded settles, and nothing else re-checks. This runs on the screenshot
    path, where a drifted window is about to be photographed sitting over the terminal.
    An app still parked costs one geometry read.
    """
    with _LOCK:
        known = list(_parked.items())
    still = []
    for name, handle in known:
        if _same_app(name, self_name):
            continue
        if _snap(handle, APP_REGION, False):
            still.append(name)
    return still


def park_active(self_name: str | None = None, focus: bool = False) -> str:
    """Park the foreground app, unless it is the agent's own terminal.

    `park_new` only knows apps that appeared after a baseline. An app that was
    already open, or one started by its own launcher, was never in that set, so
    screenshots moved the terminal left while the window being driven stayed
    where it was. This asks the platform for the actual foreground window on
    every capture and parks it on the right side.

    Backends without foreground_app (the non-Windows implementations) are a
    no-op, not an error: parking is best-effort and must never fail a capture.
    """
    try:
        find = getattr(backend(), "foreground_app", None)
        info = find() if find else None
        if not info:
            return ""
        name, handle = str(info.get("name") or ""), info.get("handle")
        if not name or _same_app(name, self_name) or not _wait_for_window(handle):
            return ""
        if not _snap(handle, APP_REGION, focus):
            return ""
        remember(handle, name)
        return name
    except Exception:                                           # noqa: BLE001
        return ""


def park_self(self_name: str | None, self_handle=None) -> bool:
    """The agent's own terminal, in the left strip, never raised.

    A handle beats a name here. Two terminals are two windows of the same
    executable, and a name can only say "some window of this app": an elevated
    session holding that name would move the other terminal instead of itself.
    The frontend passes the window that was in front when this session started,
    and Windows can also name the window from the console it is attached to.
    The other backends keep the name they have always used.
    """
    if self_handle:
        try:
            if backend().window_exists(self_handle) and _snap(self_handle, SELF_REGION, False):
                return True
        except Exception:                                       # noqa: BLE001
            pass
    try:
        find = getattr(backend(), "self_window", None)
        hwnd = find() if find else None
    except Exception:                                           # noqa: BLE001
        hwnd = None
    if hwnd:
        return _snap(hwnd, SELF_REGION, False)
    if not self_name:
        return False
    return _snap(self_name, SELF_REGION, False)


def forget(name: str) -> None:
    with _LOCK:
        _parked.pop(name, None)


def remember(handle, name: str | None = None) -> None:
    """Record a window somebody else parked, so it is re-checked with the rest.

    app_open does its own snap, since it is the only caller that wants the window
    raised; without this the app it opened would be the one app nothing looked at again.
    """
    with _LOCK:
        _parked[name or (handle if isinstance(handle, str) else str(handle))] = handle
