def foreground_window() -> int:
    """The window in front, if it plausibly frames this session.

    Captured once at boot: by the time a tool runs, focus is usually on the app
    being driven. A name tells two terminals of the same app apart no better
    than zero, but a handle names exactly one window. The check is what makes
    that safe - somebody who alt-tabs during startup would otherwise hand the
    agent an unrelated window to move. A console class, a known console host, or
    a window owned by one of our ancestors all qualify.
    """
    try:
        import os, psutil, win32gui, win32process
        hwnd = int(win32gui.GetForegroundWindow() or 0)
        if not hwnd or not win32gui.IsWindow(hwnd):
            return 0
        if win32gui.GetClassName(hwnd) in ("ConsoleWindowClass", "CASCADIA_HOSTING_WINDOW_CLASS"):
            return hwnd
        _, pid = win32process.GetWindowThreadProcessId(hwnd)
        name = psutil.Process(pid).name().lower()
        if name in ("windowsterminal.exe", "openconsole.exe", "conhost.exe", "wt.exe"):
            return hwnd
        process, seen = psutil.Process(os.getpid()), set()
        for _ in range(16):
            if process.pid in seen:
                break
            if process.pid == pid:
                return hwnd
            seen.add(process.pid)
            process = process.parent()
            if process is None:
                break
        return 0
    except Exception:
        return 0


def active_app_name() -> str:
    try:
        import win32gui, win32process, psutil
        hwnd = win32gui.GetForegroundWindow()
        _, pid = win32process.GetWindowThreadProcessId(hwnd)
        return psutil.Process(pid).name().replace(".exe", "")
    except Exception:
        return "unknown"

_dpi_done = False

def _dpi_aware():
    """DPI awareness is process-wide and one-shot, and has to be set before the first
    capture. Set late, one shot came back in logical pixels and the next in physical, and
    the grid the agent clicks from silently changed units mid-session."""
    global _dpi_done
    if _dpi_done: return
    _dpi_done = True
    import ctypes
    for call in (lambda: ctypes.windll.user32.SetProcessDpiAwarenessContext(-4),  # per-monitor v2
                 lambda: ctypes.windll.shcore.SetProcessDpiAwareness(2),
                 lambda: ctypes.windll.user32.SetProcessDPIAware()):
        try:
            if call(): return
        except Exception:
            continue

def capture():
    from PIL import Image
    import mss
    _dpi_aware()
    with mss.mss() as sct:
        shot = sct.grab(sct.monitors[1])
        return Image.frombytes("RGB", shot.size, shot.rgb)

def detect_scale(img_w: int, img_h: int) -> tuple[float, int, int]:
    """Primary monitor size in the units this process sees.

    Both readings are taken after _dpi_aware(), so a DPI-aware process gets physical
    pixels from GetSystemMetrics and from mss alike, scale 1.0, which is the unit
    SetCursorPos speaks. The ratio is still computed rather than assumed, because a
    failed awareness call flips both to virtualized pixels. Errors are not swallowed:
    a wrong scale mislabels the grid silently.
    """
    import ctypes
    u = ctypes.windll.user32
    logical_w, logical_h = int(u.GetSystemMetrics(0)), int(u.GetSystemMetrics(1))
    if not logical_w or not logical_h:
        raise RuntimeError("primary monitor reported a zero size; cannot map screenshot pixels to click coordinates")
    sx, sy = img_w / logical_w, img_h / logical_h
    if abs(sx - sy) > 0.01:
        raise RuntimeError(
            f"screenshot is {img_w}x{img_h} against a {logical_w}x{logical_h} primary monitor, "
            f"which is not a uniform scale ({sx:.3f} x {sy:.3f}); refusing to guess, "
            "because clicks would land off target")
    return sx, logical_w, logical_h

FONT_PATH = "C:\\Windows\\Fonts\\segoeui.ttf"