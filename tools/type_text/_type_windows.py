import ctypes, time

INPUT_KEYBOARD = 1
KEYEVENTF_UNICODE = 0x0004
KEYEVENTF_KEYUP = 0x0002

class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", ctypes.c_ushort), ("wScan", ctypes.c_ushort),
                ("dwFlags", ctypes.c_ulong), ("time", ctypes.c_ulong),
                ("dwExtraInfo", ctypes.c_size_t)]

class MOUSEINPUT(ctypes.Structure):
    # Present only to give the INPUT union its real size. On 64-bit Windows
    # MOUSEINPUT is 32 bytes, which makes INPUT 40; a struct holding just
    # KEYBDINPUT is 32, and SendInput rejects it with cbSize wrong.
    _fields_ = [("dx", ctypes.c_long), ("dy", ctypes.c_long),
                ("mouseData", ctypes.c_ulong), ("dwFlags", ctypes.c_ulong),
                ("time", ctypes.c_ulong), ("dwExtraInfo", ctypes.c_size_t)]

class _INPUTUNION(ctypes.Union):
    _fields_ = [("mi", MOUSEINPUT), ("ki", KEYBDINPUT)]

class INPUT(ctypes.Structure):
    _fields_ = [("type", ctypes.c_ulong), ("u", _INPUTUNION)]

def _send_char(ch: str, key_up: bool):
    flags = KEYEVENTF_UNICODE | (KEYEVENTF_KEYUP if key_up else 0)
    # UTF-16LE gives the surrogate pair for characters outside the BMP; wScan is
    # a 16-bit unit, so ord(ch) would truncate an emoji or a rare CJK glyph.
    encoded = ch.encode("utf-16-le")
    for i in range(0, len(encoded), 2):
        unit = int.from_bytes(encoded[i:i + 2], "little")
        inp = INPUT(type=INPUT_KEYBOARD)
        inp.u.ki = KEYBDINPUT(0, unit, flags, 0, 0)
        if ctypes.windll.user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(inp)) != 1:
            raise OSError("SendInput failed; the target may be elevated or the desktop is locked")

def type_text(text: str, delay: float = 0.01):
    for ch in text:
        _send_char(ch, key_up=False)
        _send_char(ch, key_up=True)
        if delay: time.sleep(delay)
