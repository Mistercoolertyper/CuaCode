import subprocess, time

def get_frontmost() -> str | None:
    r = subprocess.run(["xdotool", "getactivewindow"], capture_output=True, text=True)
    win_id = r.stdout.strip()
    return win_id or None

def open_app(app: str) -> str | None:
    # Detached from the TUI's terminal: a launch that writes to it would paint
    # inside the composer, and nothing here reads the app's output anyway.
    _quiet = dict(stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                  start_new_session=True)
    try: subprocess.Popen([app], **_quiet)
    except Exception: subprocess.Popen(["xdg-open", app], **_quiet)

    deadline = time.time() + 5
    while time.time() < deadline:
        time.sleep(0.4)
        r = subprocess.run(["xdotool", "search", "--name", app], capture_output=True, text=True)
        ids = [i for i in r.stdout.strip().split("\n") if i]
        if ids: return ids[0]
    return None

def snap_region(win_id: str | None, x_start_frac: float, x_end_frac: float, focus: bool = False) -> bool:
    """See _open_macos.snap_region. focus defaults to False so repositioning a
    window does not decide what has keyboard focus."""
    if not win_id: return False
    r = subprocess.run(["xdotool", "getdisplaygeometry"], capture_output=True, text=True)
    sw, sh = map(int, r.stdout.strip().split())
    x0, x1 = int(sw * x_start_frac), int(sw * x_end_frac)
    if focus: subprocess.run(["xdotool", "windowactivate", win_id], check=True)
    subprocess.run(["xdotool", "windowmove", win_id, str(x0), "0"], check=True)
    subprocess.run(["xdotool", "windowsize", win_id, str(x1 - x0), str(sh)], check=True)
    return True

def gui_apps() -> dict[str, str]:
    """{window name: window id} for every visible window.

    X has no notion of "the application" that can be asked from the outside, so the
    window title is the identity. That is coarse, but the only thing built on it is
    "was this here a moment ago".
    """
    r = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "."],
                       capture_output=True, text=True)
    out = {}
    for wid in r.stdout.split():
        n = subprocess.run(["xdotool", "getwindowname", wid],
                           capture_output=True, text=True).stdout.strip()
        if n: out.setdefault(n, wid)
    return out


def wait_for_window(win_id: str, timeout: float = 8) -> bool:
    """A window id was only handed out because the window exists."""
    return bool(win_id)
