import os, subprocess, time, ctypes, win32api, win32gui, win32process, win32con, psutil
from ctypes import wintypes

_SPI_GETWORKAREA = 0x0030
# A launched app gets no console of the one launching it: a console app would
# otherwise write into the terminal the TUI is drawing in. DETACHED_PROCESS for
# the app itself, CREATE_NO_WINDOW for the `cmd /c start` fallback that owns no
# window of its own but would otherwise attach one.
DETACHED_PROCESS = 0x00000008
CREATE_NO_WINDOW = 0x08000000
_SHELL_CLASSES = {
    "Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd",
    "NotifyIconOverflowWindow", "TaskListThumbnailWnd",
}


def _process_target(app: str) -> str:
    """The executable name a launched app will register as.

    A full path is what the tool's schema advertises, and comparing that whole
    path with psutil's process name never matches. The basename without .exe is
    what both the launch path and a bare app name have in common.
    """
    name = os.path.basename(str(app).strip()).strip('"').lower()
    return name[:-4] if name.endswith(".exe") else name


def _matches(name: str, target: str) -> bool:
    name = name.lower()
    if name.endswith(".exe"): name = name[:-4]
    return bool(name and target) and (name == target or target in name or name in target)


def _work_area() -> tuple[int, int, int, int]:
    """The primary desktop without the taskbar, in the process's pixel units.

    GetSystemMetrics measures the full display, so snapping to its bottom edge
    hides the lower part of both windows behind the taskbar. SPI_GETWORKAREA is
    the rectangle applications are meant to occupy and follows the taskbar when
    it is moved or resized.
    """
    rect = wintypes.RECT()
    try:
        if ctypes.windll.user32.SystemParametersInfoW(
                _SPI_GETWORKAREA, 0, ctypes.byref(rect), 0):
            if rect.right > rect.left and rect.bottom > rect.top:
                return int(rect.left), int(rect.top), int(rect.right), int(rect.bottom)
    except Exception:
        pass
    u = ctypes.windll.user32
    return 0, 0, int(u.GetSystemMetrics(0)), int(u.GetSystemMetrics(1))


def _title_target(app: str) -> str:
    """A window-title query when the argument is not an executable path."""
    raw = str(app).strip().strip('"')
    if not raw or raw.lower().endswith(".exe") or os.path.isabs(raw):
        return ""
    return raw.lower()


def _window_score(app: str, hwnd: int) -> int:
    """How well a visible window matches an app name, executable path or title."""
    try:
        if not win32gui.IsWindowVisible(hwnd):
            return 0
        cls = win32gui.GetClassName(hwnd)
        title = win32gui.GetWindowText(hwnd)
    except Exception:
        return 0
    if cls in _SHELL_CLASSES or not title.strip():
        return 0

    target = _process_target(app)
    wanted = _title_target(app)
    score = 0
    try:
        _, pid = win32process.GetWindowThreadProcessId(hwnd)
        name = psutil.Process(pid).name()
    except Exception:
        name = ""
    if name:
        stem = name[:-4] if name.lower().endswith(".exe") else name
        if target and stem.lower() == target:
            score += 40
        elif target and _matches(name, target):
            score += 20
    if wanted:
        title_l = title.lower()
        if title_l == wanted:
            score += 100
        elif wanted in title_l:
            score += 60
        elif title_l in wanted and len(title_l) >= 4:
            score += 30
    return score


def _find_window(app: str) -> int | None:
    """The best visible top-level window for an app name, path or window title.

    A title is needed for apps whose window belongs to a host process: Minecraft
    runs under javaw, and a store app is often hosted by ApplicationFrameHost.
    Matching only psutil's process name misses both and reports that a window
    which is plainly on screen was never found.
    """
    best = (0, None)
    def cb(hwnd, _):
        nonlocal best
        try:
            score = _window_score(app, hwnd)
        except Exception:
            return
        if score > best[0]:
            best = (score, hwnd)
    try:
        win32gui.EnumWindows(cb, None)
    except Exception:
        return None
    return best[1]

def get_frontmost() -> int | None:
    hwnd = win32gui.GetForegroundWindow()
    return hwnd or None

def open_app(app: str) -> int | None:
    app = str(app).strip()
    # An app that is already running must be focused, not launched a second time.
    # Searching first also lets a window title from a screenshot ("Minecraft* 26.3")
    # resolve to the existing window instead of being mistaken for an executable.
    existing = _find_window(app)
    if existing:
        return existing
    # Launched with no console of its own: a console app inherits the handle the
    # TUI is drawing on, and its output then lands inside the composer mid-frame.
    # DETACHED_PROCESS gives it none, and nothing here waits on its output anyway.
    try:
        subprocess.Popen(app, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL, creationflags=DETACHED_PROCESS)
    except Exception:
        # `start` runs the app under a fresh shell rather than beside the TUI, so
        # the detached console still belongs to the app. Its own output is dropped
        # by the pipes, which `start` would otherwise let through to the console.
        try:
            subprocess.Popen(["cmd", "/c", "start", "", app], shell=False,
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL, creationflags=CREATE_NO_WINDOW)
        except Exception: return None

    deadline = time.time() + 8
    while time.time() < deadline:
        time.sleep(0.2)
        found = _find_window(app)
        if found: return found
    return None

def _focus(hwnd: int) -> bool:
    """Bring a window to the front, past the foreground lock.

    A background process calling SetForegroundWindow is refused and the taskbar
    button flashes instead: Windows only lets a process take the foreground when
    it is already allowed to. Attaching to the input queue of the window that
    does hold the foreground for the length of the call is the documented way
    around that, and this agent is acting on the person's behalf either way.
    """
    try:
        if win32gui.IsIconic(hwnd):
            win32gui.ShowWindow(hwnd, win32con.SW_RESTORE)
        if win32gui.GetForegroundWindow() == hwnd:
            return True
        current = win32api.GetCurrentThreadId()
        threads = {win32process.GetWindowThreadProcessId(hwnd)[0]}
        front = win32gui.GetForegroundWindow()
        if front:
            threads.add(win32process.GetWindowThreadProcessId(front)[0])
        attached = []
        try:
            for tid in threads:
                if tid and tid != current:
                    try:
                        win32process.AttachThreadInput(current, tid, True)
                        attached.append(tid)
                    except Exception:
                        pass
            win32gui.BringWindowToTop(hwnd)
            win32gui.SetForegroundWindow(hwnd)
        finally:
            for tid in attached:
                try: win32process.AttachThreadInput(current, tid, False)
                except Exception: pass
        return win32gui.GetForegroundWindow() == hwnd
    except Exception:
        return False


def snap_region(hwnd_or_name: int | str | None, x_start_frac: float, x_end_frac: float, focus: bool = False) -> bool:
    """See _open_macos.snap_region. focus defaults to False so repositioning a
    window does not decide what has keyboard focus."""
    if not hwnd_or_name: return False
    hwnd = hwnd_or_name
    if isinstance(hwnd_or_name, str):
        # The frontend probes this name with PowerShell, which can hand back several
        # lines when more than one process owns the foreground window.
        hwnd = _find_window(hwnd_or_name.strip().splitlines()[0].strip())
        if not hwnd: return False
    # pywin32 wants a plain int HWND: a PyHANDLE or a float off the wire raises
    # "The object is not a PyHANDLE object" from inside ShowWindow/MoveWindow and
    # used to take the whole screenshot down. Coerce here; a bad handle is "not snapped".
    try: hwnd = int(hwnd)
    except (TypeError, ValueError): return False
    if not hwnd or not win32gui.IsWindow(hwnd): return False
    left, top, right, bottom = _work_area()
    width, height = right - left, bottom - top
    if width <= 0 or height <= 0: return False
    x0 = left + int(round(width * x_start_frac))
    x1 = left + int(round(width * x_end_frac))
    x0 = max(left, min(right - 1, x0))
    x1 = max(x0 + 1, min(right, x1))

    try:
        before = win32gui.GetForegroundWindow()
        show = win32gui.GetWindowPlacement(hwnd)[1]
        restore = show in (win32con.SW_SHOWMAXIMIZED, win32con.SW_SHOWMINIMIZED) or win32gui.IsIconic(hwnd)
        # A maximized window ignores MoveWindow. Restoring it activates it, so put
        # focus back when the caller asked not to steal it.
        win32gui.ShowWindow(hwnd, win32con.SW_RESTORE if restore else win32con.SW_SHOWNOACTIVATE)
        if focus:
            _focus(hwnd)
        elif restore and before and before != hwnd:
            try: win32gui.SetForegroundWindow(before)
            except Exception: pass

        x1 = max(x0 + 1, x1)
        win32gui.MoveWindow(hwnd, x0, top, x1 - x0, height, True)
        rect = win32gui.GetWindowRect(hwnd)
        if (abs(rect[0] - x0) <= 4 and abs(rect[1] - top) <= 4
                and abs(rect[2] - x1) <= 4 and abs(rect[3] - bottom) <= 4):
            return True

        # Some windows apply their own frame rules on the first MoveWindow. A
        # second explicit position makes the result verifiable rather than
        # reporting a snap that never happened.
        flags = win32con.SWP_NOZORDER | (0 if focus else win32con.SWP_NOACTIVATE)
        win32gui.SetWindowPos(hwnd, 0, x0, top, x1 - x0, height,
                              flags | win32con.SWP_SHOWWINDOW)
        rect = win32gui.GetWindowRect(hwnd)
        return (abs(rect[0] - x0) <= 4 and abs(rect[1] - top) <= 4
                and abs(rect[2] - x1) <= 4 and abs(rect[3] - bottom) <= 4)
    except Exception:
        return False

def gui_apps() -> dict[str, int]:
    """{process name: hwnd} for every visible top-level window with a title.

    Keyed by process rather than by window so an app opening a second window does not
    read as a second launch; the first window found is the one snap_region would pick.
    """
    out = {}
    def cb(hwnd, _):
        if not win32gui.IsWindowVisible(hwnd): return
        try:
            if win32gui.GetClassName(hwnd) in _SHELL_CLASSES: return
        except Exception:
            return
        if not win32gui.GetWindowText(hwnd): return
        _, pid = win32process.GetWindowThreadProcessId(hwnd)
        try: name = psutil.Process(pid).name()
        except Exception: return
        out.setdefault(name, hwnd)
    try:
        win32gui.EnumWindows(cb, None)
    except Exception:
        return {}
    return out


def foreground_app() -> dict | None:
    """The visible app window currently in front, or None for the desktop/shell.

    Screenshot parking uses this even when the app was not launched by the
    agent. Otherwise an already-running app, or one started from its own
    launcher, stayed wherever it was while only the terminal moved left.
    """
    try:
        hwnd = win32gui.GetForegroundWindow()
        if not hwnd or not win32gui.IsWindowVisible(hwnd):
            return None
        cls = win32gui.GetClassName(hwnd)
        title = win32gui.GetWindowText(hwnd)
        if cls in _SHELL_CLASSES or not title.strip():
            return None
        _, pid = win32process.GetWindowThreadProcessId(hwnd)
        name = psutil.Process(pid).name()
        if not name:
            return None
        return {"handle": int(hwnd), "name": name, "title": title, "class_name": cls}
    except Exception:
        return None


def _pid_of(hwnd) -> int:
    """The process that owns a window, or 0 when the handle is already gone."""
    try:
        _, pid = win32process.GetWindowThreadProcessId(int(hwnd))
        return int(pid or 0)
    except Exception:
        return 0


def _class_of(hwnd) -> str:
    try: return win32gui.GetClassName(hwnd)
    except Exception: return ""


def _console_window() -> int:
    """The window this process's console is attached to, or 0 when it has none."""
    try: return int(ctypes.windll.kernel32.GetConsoleWindow() or 0)
    except Exception: return 0


def _console_title() -> str:
    """What this console calls itself, which a terminal window usually repeats."""
    try:
        size = 1024
        buf = ctypes.create_unicode_buffer(size)
        if ctypes.windll.kernel32.GetConsoleTitleW(buf, size):
            return buf.value.strip()
    except Exception:
        pass
    return ""


def _visible_window(pid: int, hint: str = "") -> int | None:
    """The visible, non-tool window a process owns, preferring a title that matches
    the console's own: a terminal names its window after the tab that is showing."""
    found = []
    def cb(hwnd, _):
        if _pid_of(hwnd) != pid: return
        try:
            if not win32gui.IsWindowVisible(hwnd): return
            if win32gui.GetWindowLong(hwnd, win32con.GWL_EXSTYLE) & win32con.WS_EX_TOOLWINDOW: return
        except Exception:
            return
        cls = _class_of(hwnd)
        title = win32gui.GetWindowText(hwnd).strip()
        if not title or cls in _SHELL_CLASSES:
            return
        if cls in ("ConsoleWindowClass", "PseudoConsoleWindow"):
            return
        found.append((hwnd, title))
    try:
        win32gui.EnumWindows(cb, None)
    except Exception:
        return None
    if not found:
        return None
    if hint:
        low = hint.lower()
        for hwnd, title in found:
            title_l = title.lower()
            if low == title_l or low in title_l or title_l in low:
                return hwnd
    return found[0][0]


def _parent_pid(pid: int) -> int:
    try: return int(psutil.Process(pid).ppid() or 0)
    except Exception: return 0


def self_window() -> int | None:
    """The window hosting this process, not just some window of the same app.

    A classic console window is the answer itself; under Windows Terminal the console
    handle is the tab's hidden pseudo-console, so the walk climbs the processes that
    own it until one of them has the visible frame.
    """
    console = _console_window()
    if console and _class_of(console) == "ConsoleWindowClass":
        return console
    hint = _console_title()
    pid = _pid_of(console) if console else 0
    if not pid:
        pid = os.getpid()
    seen = set()
    for _ in range(16):
        if not pid or pid in seen:
            break
        seen.add(pid)
        found = _visible_window(pid, hint)
        if found:
            return found
        pid = _parent_pid(pid)
    return None


def wait_for_window(hwnd: int, timeout: float = 8) -> bool:
    """The handle came out of an enumeration of live windows; confirm it still is."""
    try: return bool(hwnd) and bool(win32gui.IsWindow(int(hwnd)))
    except Exception: return False
