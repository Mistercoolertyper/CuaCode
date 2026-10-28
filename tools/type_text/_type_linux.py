import subprocess

def type_text(text: str, delay: float = 0.01):
    # Silent: xdotool reports a key it cannot type on stderr, which is the
    # terminal the TUI draws in.
    subprocess.run(["xdotool", "type", "--delay", str(int(delay * 1000)), text], check=True,
                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL)
