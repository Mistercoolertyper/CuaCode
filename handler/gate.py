"""Which files this process has read, and what they looked like when it did.

The file tool's edit gate runs on this: a file has to be read before it is
written, and reading it records the fingerprint it had, so an edit against a file
that moved underneath us is refused rather than quietly overwriting somebody
else's change.

It cannot live in the tool that owns the behaviour, because it has two other
readers. A session saves it into `meta.json` and re-seeds it when the
conversation is reopened, and the project-docs block asks it what has already
been read. Both used to reach for `sys.modules.get("_common")` -- a *top-level*
module that existed only because `tools/file/main.py` put its own folder on
`sys.path` -- and one of them called `load_tools()` purely for the side effect of
importing all 28 tools so that the module would be there. Rename the helper, or
open a session before any tool has loaded, and the gate silently forgot every
read: a session that claims it read nothing, and an edit gate that lets the next
turn overwrite what the last one never saw.

Nothing here opens a file. It is a record of facts the file tool decided to put
in, plus the two questions asked of that record.
"""
import os

# Paths read this session, gating edit. Plain strings because meta.json stores
# them and JSON has no opinions.
read_files: set[str] = set()

# What those files looked like when read: enough to notice one moved underneath
# us. Absent for a path re-seeded from a saved session, which was checked against
# the disk at restore time instead.
seen: dict[str, dict] = {}


def mark(path, meta: dict | None = None) -> None:
    """Record that this path has been read, and what it looked like."""
    read_files.add(str(path))
    if meta is not None: seen[str(path)] = meta


def forget(path) -> None:
    """It has not been read any more: deleted, or written by us."""
    read_files.discard(str(path))
    seen.pop(str(path), None)


def moved(src, dest, meta: dict | None) -> None:
    """A rename carries both facts across: the new path is read, the old one not.

    The fingerprint follows when there is one, with its mtime refreshed to the
    file's new one -- a rename does not change the content, and an edit straight
    after it should not be refused for a timestamp the rename itself moved.
    """
    forget(src)
    read_files.add(str(dest))
    if meta is None: return
    try: meta = {**meta, "mtime_ns": os.stat(dest).st_mtime_ns}
    except OSError: pass
    seen[str(dest)] = meta


def same_as_recorded(entry: dict) -> bool:
    """Whether a `snapshot()` entry still describes the file on disk.

    A missing or unreadable file is not the same file: an edit against what a
    session remembered of a path that has since gone would create it from a stale
    copy.
    """
    try: st = os.stat(entry["path"])
    except (OSError, KeyError, TypeError): return False
    return st.st_mtime_ns == entry.get("mtime_ns") and st.st_size == entry.get("size")


def snapshot() -> list[dict]:
    """What `meta.json` keeps: the paths, and enough to detect drift later.

    Paths alone would be a lie after a reload, three days and an outside edit on.
    mtime+size rather than a hash: a session commits every round, and the agent
    reads files it has no reason to re-checksum that often.
    """
    out = []
    for p in sorted(read_files):
        try: st = os.stat(p)
        except OSError: continue
        out.append({"path": p, "mtime_ns": st.st_mtime_ns, "size": st.st_size})
    return out


def restore(entries: list[dict]) -> None:
    """Re-seed the gate from a saved session, minus anything that moved since.

    A stale entry is the dangerous direction: it would let an edit through on
    content nobody has seen.
    """
    read_files.update(e["path"] for e in entries if isinstance(e, dict) and same_as_recorded(e))


if __name__ == "__main__":
    # One runnable check: a restored entry survives a reload only while the file it
    # describes is the file on disk, and a rename carries the gate with it.
    import tempfile
    from pathlib import Path

    with tempfile.TemporaryDirectory() as tmp:
        keep, gone = Path(tmp, "keep.txt"), Path(tmp, "gone.txt")
        keep.write_text("same")
        gone.write_text("old")
        mark(keep, {"mtime_ns": keep.stat().st_mtime_ns, "size": keep.stat().st_size})
        mark(gone, {"mtime_ns": gone.stat().st_mtime_ns, "size": gone.stat().st_size})
        saved = snapshot()
        assert {e["path"] for e in saved} == {str(keep), str(gone)}, saved

        read_files.clear(); seen.clear()
        gone.write_text("rewritten while the session was closed")
        restore(saved)
        assert str(keep) in read_files, "an unchanged file stays read"
        assert str(gone) not in read_files, "a file that moved is not read"

        mark(keep, {"size": 4})
        moved(keep, Path(tmp, "moved.txt"), seen[str(keep)])
        assert str(keep) not in read_files and str(Path(tmp, "moved.txt")) in read_files
        forget(Path(tmp, "moved.txt"))
        assert not read_files and str(Path(tmp, "moved.txt")) not in seen
    print("gate ok")
