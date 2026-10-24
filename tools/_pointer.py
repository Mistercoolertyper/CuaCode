"""Did the pointer event actually happen.

CGEventPost does not fail when the process is missing Accessibility permission: the event is dropped and the call returns as though it had worked. The only way to tell from inside is to read the pointer back, because a posted event carries the cursor with it.
The read cannot be immediate: the post is asynchronous and WindowServer moves the cursor a few milliseconds later, so it polls until the pointer arrives rather than reading once, which would report a phantom dropped click in front of a user.
A mismatch that survives the deadline asks the Accessibility API rather than assuming permission is the problem, since a pointer elsewhere with permission in hand is ordinary.
"""

import time

_DEADLINE = 0.25      # seconds to wait for the pointer to arrive; ~24x the worst case seen
_GAP = 0.001          # between reads; a read costs ~30us, so this is nearly all sleep
_TOLERANCE = 1        # px, for the rounding between logical points and the read-back

_DROPPED = ("the pointer never moved and this app does not have Accessibility "
            "permission, so the event was dropped (System Settings > Privacy & "
            "Security > Accessibility). Tell the user rather than retrying")

_ELSEWHERE = ("the event was delivered -- Accessibility permission is present -- but the "
              "pointer ended up somewhere else: the coordinate may have been off-screen "
              "and clamped, or the user or the app moved the pointer afterwards. Check "
              "the next screenshot before re-aiming")

_UNKNOWN = ("the pointer is not where the event was aimed. It may have been off-screen and "
            "clamped, moved afterwards by the user or the app, or stopped before it reached "
            "the target. Check the next screenshot before re-aiming")


def _trusted():
    """Whether this process is allowed to post events, or None where unanswerable.

    None on every platform but macOS, and on a macOS without pyobjc: "cannot ask" has to stay distinct from "asked, and the
    answer is no", since only the second is worth telling the user to fix a setting over."""
    try:
        from ApplicationServices import AXIsProcessTrusted
    except Exception:
        return None
    try:
        return bool(AXIsProcessTrusted())
    except Exception:
        return None


def verify(plat, x: int, y: int) -> dict:
    """Result fields describing where the pointer actually ended up.

    Empty when it arrived where it was sent, and on a platform whose module defines no cursor() to ask:
    an absent read-back reports nothing rather than guessing."""
    read = getattr(plat, "cursor", None)
    if read is None:
        return {}
    deadline = time.monotonic() + _DEADLINE
    while True:
        landed = read()
        if landed is None:
            return {}
        if abs(landed[0] - x) <= _TOLERANCE and abs(landed[1] - y) <= _TOLERANCE:
            return {}
        if time.monotonic() >= deadline:
            trusted = _trusted()
            return {"landed_at": list(landed),
                    "warning": _DROPPED if trusted is False else
                               _ELSEWHERE if trusted else _UNKNOWN}
        time.sleep(_GAP)
