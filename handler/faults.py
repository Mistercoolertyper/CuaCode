"""Failures a layer survived: said once, with the subject in the sentence.

A fallback that reports the failure is fine; one that hides it is the bug this
module exists to stop. Every `except Exception` in this tree that decides to
keep going calls `note()` with what it was doing, what went wrong, and what the
caller can do about it -- so the failure has somewhere to be read instead of
being turned into an empty answer.

stderr is the protocol's own diagnostics channel (WIRE.md), so nothing here
reaches the wire and no event shape changes. Said once per distinct sentence:
`registry()` re-asks the self-describing tools every turn, and a broken skill
file would otherwise be printed on every turn of every conversation.
"""
import sys

_seen: set[str] = set()


def note(subject: str, error, next_move: str = "") -> str:
    """Say one failure, and return the sentence it said.

    subject names what was being attempted (``skills: ~/.cuacode/skills/x``),
    error is the exception or its text, next_move is what to do about it. The
    return value is for callers with somewhere better than stderr to put it --
    a turn's notice, a tool's error, a menu's description -- so the same words
    are not spelled twice.
    """
    said = f"{subject}: {error}"
    if next_move:
        said = f"{said} - {next_move}"
    line = f"cuacode: {said}"
    if line not in _seen:
        _seen.add(line)
        print(line, file=sys.stderr, flush=True)
    return said
