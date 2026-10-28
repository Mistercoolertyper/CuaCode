import subprocess

_BUTTONS = {"left": "1", "right": "3", "middle": "2"}
# Silent: xdotool writes nothing on success, but its error goes to stderr, which
# is the terminal the TUI is drawing in.
_QUIET = dict(stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def click(x: int, y: int, button: str = "left", clicks: int = 1):
    subprocess.run(["xdotool", "mousemove", str(x), str(y)], check=True, **_QUIET)
    subprocess.run(["xdotool", "click", "--repeat", str(clicks), _BUTTONS[button]], check=True, **_QUIET)