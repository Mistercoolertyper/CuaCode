"""What the five file-backed integrations have in common.

A skill, a subagent, a memory, an MCP server and a workflow are five kinds of one
thing: files shipped with the app, the same kind under `~/.cuacode`, and the
user's copy winning on a name collision. Each of the five used to spell that out
itself, and the spellings had drifted. Three put the bundled directory at
`Path(__file__).parent` and the workflow one computed it from `parents[2]`, which
agree only because of where the two files happen to live. Four skip rules
disagreed about a leading `_` and a leading `.`, and subagents had no rule at
all, so `_draft.md` in `~/.cuacode/subagents` was loaded as an agent and offered
to the model. Four wrote the same "unknown X: ... (have ...)" sentence.

Nothing here knows what a skill is or what a workflow does. It answers three
questions -- where a kind's files live, which of them count, and in what order
they shadow each other -- and every loader keeps its own parsing.
"""
from pathlib import Path

from handler.session import store

# handler/catalog.py -> handler -> the repo root. One spelling, and the only one:
# `Path(__file__).parent` was right for a loader that sat inside its own
# integrations/<kind>/ and wrong for the workflow loader that did not.
_ROOT = Path(__file__).resolve().parents[1]


def bundled(kind: str) -> Path:
    """The directory of one kind shipped with the app."""
    return _ROOT / "integrations" / kind


def user_dir(kind: str) -> Path:
    """The user's own directory for one kind, created on first read.

    Created, not merely named: every one of these is a directory a person is
    expected to drop a file into, and one that does not exist yet is a `cp` that
    fails for no stated reason.
    """
    d = store.home() / kind
    d.mkdir(parents=True, exist_ok=True)
    return d


def draft(p: Path) -> bool:
    """A name that is a draft or hidden, and so is not installed.

    One rule for every kind. `_` is the prefix a person uses for a file they are
    still writing; `.` is what the OS leaves behind.
    """
    return p.name.startswith(("_", "."))


def files(d: Path, pattern: str) -> list[Path]:
    """One directory's entries matching a glob, drafts and hidden names removed."""
    if not d.is_dir(): return []
    return sorted(p for p in d.glob(pattern) if p.is_file() and not draft(p))


def folders(d: Path) -> list[Path]:
    """One directory's subdirectories, drafts and hidden names removed."""
    if not d.is_dir(): return []
    return sorted(p for p in d.iterdir() if p.is_dir() and not draft(p))


def layered_files(kind: str, pattern: str) -> list[Path]:
    """Every file of one kind: bundled first, the user's last.

    Load order is the override rule. A caller that keys a dict by name and lets a
    later path win needs nothing else; a caller that has to tell the two apart
    asks `bundled` and `user_dir` separately.
    """
    return files(bundled(kind), pattern) + files(user_dir(kind), pattern)


def layered_dirs(kind: str) -> list[Path]:
    """Every subdirectory of one kind, bundled first, the user's last."""
    return folders(bundled(kind)) + folders(user_dir(kind))


def unknown(kind: str, name: str, have) -> ValueError:
    """The one sentence for a name that is not installed."""
    return ValueError(f"unknown {kind}: {name!r} (have {sorted(have) or 'none installed'})")


if __name__ == "__main__":
    # One runnable check: the two directory rules this module exists to unify.
    assert draft(Path("_draft.md")) and draft(Path("~$notes.docx")) is False
    assert draft(Path(".DS_Store")) and not draft(Path("explorer.md"))
    assert bundled("skills").is_dir(), bundled("skills")
    assert bundled("workflows").is_dir(), bundled("workflows")
    assert unknown("skill", "x", {"a": 1}).args[0] == "unknown skill: 'x' (have ['a'])"
    print("catalog ok")
