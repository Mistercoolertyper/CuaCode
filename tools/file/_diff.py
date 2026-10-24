import difflib

from _common import DIFF_CAP

def unified(before: str, after: str, name: str) -> str:
    """What changed, handed back with the result.

    An edit that matched the wrong hunk, or landed at the wrong indentation, looks
    exactly like a successful one in an {"ok": true} until several calls later.
    Echoing the patch lets the model review its own edit in the same turn.
    """
    lines = list(difflib.unified_diff(before.splitlines(), after.splitlines(),
                                      fromfile=f"a/{name}", tofile=f"b/{name}",
                                      lineterm="", n=3))
    if len(lines) > DIFF_CAP:
        lines = lines[:DIFF_CAP] + [f"... {len(lines) - DIFF_CAP} more diff lines"]
    return "\n".join(lines)
