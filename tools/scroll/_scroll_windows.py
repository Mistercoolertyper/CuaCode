import ctypes

MOUSEEVENTF_WHEEL = 0x0800
MOUSEEVENTF_HWHEEL = 0x01000

def scroll(x: int, y: int, dx: int, dy: int):
    ctypes.windll.user32.SetCursorPos(int(x), int(y))
    # Windows reports a positive wheel delta as up, while the tool's contract is
    # positive = down. A 100-200 unit request is meant to be one or two notches,
    # so map it to the platform's 120-unit notch rather than passing it through.
    if dy: ctypes.windll.user32.mouse_event(MOUSEEVENTF_WHEEL, 0, 0, -int(round(dy * 1.2)), 0)
    if dx: ctypes.windll.user32.mouse_event(MOUSEEVENTF_HWHEEL, 0, 0, int(round(dx * 1.2)), 0)
