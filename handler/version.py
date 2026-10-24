"""Which build of the tree the worker is running out of.

The same stamp the Rust half bakes in at build time, computed from the same git:
``<commits>.<short sha>``. Not a number out of a manifest, because that is the
number nobody bumps - two builds of one tree agree, and any other build does
not. The two languages each compute it rather than sharing a file, since a file
is one more thing to forget to update; `rust/build.rs` says the same thing on
the other side.

Nothing here asks the network. `/version` says which code this is, and
`cuacode doctor` is the command that asks origin about it.
"""

import subprocess
from pathlib import Path

# handler/version.py -> the tree the worker was started from.
ROOT = Path(__file__).resolve().parent.parent


def _git(*args) -> str | None:
    try:
        out = subprocess.run(
            ["git", *args], cwd=ROOT, capture_output=True, text=True, timeout=5
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if out.returncode != 0:
        return None
    return out.stdout.strip() or None


def stamp() -> str:
    """``412.52738a5``, or `unknown` when this tree is not a checkout."""
    count, sha = _git("rev-list", "--count", "HEAD"), _git("rev-parse", "--short", "HEAD")
    return f"{count}.{sha}" if count and sha else "unknown"
