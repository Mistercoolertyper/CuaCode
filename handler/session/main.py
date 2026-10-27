import os, sys
from pathlib import Path

from handler import usage
from handler.agent import effort, providers
from handler.session import blobs, replay, store

class Session:
    """One conversation, persisted as canonical records.

    What lands on disk is the input shape the adapters already take (user text, an assistant turn, a raw tool result), and
    loading replays those through the same adapters, so a conversation is not welded to the provider that recorded it."""

    def __init__(self, sid: str, meta: dict, records: list[dict]):
        self.id = sid
        self.dir = store.path(sid)
        self.meta = meta
        self._records = records   # committed, on disk, still blob-split
        self._pending = []        # this run, not yet committed
        self._mark = 0

    @classmethod
    def create(cls, provider: str = "ollama", model: str = "", effort_level: str = "",
               frontend: str = "") -> "Session":
        sid = store.new_id()
        now = store.now_iso()
        return cls(sid, {"id": sid, "created": now, "updated": now, "provider": provider,
                         "origin": provider, "model": model, "effort": effort_level,
                         # Which frontend made this session, so a controller can show
                         # only the ones it owns; empty for sessions predating the field.
                         "frontend": frontend,
                         # title_source says who chose the name and so whether
                         # anything may replace it; title_attempts caps the namer.
                         "title": "", "title_source": "", "title_attempts": 0,
                         "cwd": "", "records": 0, "turns": 0, "read_files": []}, [])

    @classmethod
    def open(cls, sid: str) -> "Session":
        d = store.path(sid)
        if not d.is_dir(): raise FileNotFoundError(f"no session {sid}")
        return cls(sid, store.read_json(d / "meta.json"), store.read_jsonl(d / "messages.jsonl"))

    @property
    def provider(self) -> str: return self.meta.get("provider") or "ollama"

    @property
    def effort(self) -> str:
        """How hard the model should think in this conversation.

        A property of the conversation, not of the account, like provider and model: reopening gets the
        depth it was run at. Blank means the provider's own default."""
        return self.meta.get("effort") or ""

    def set_effort(self, level: str):
        """Validated here, not at request time: a typo silently meaning "no
        thinking" is worse than a rejected setting."""
        level = level or ""
        if level and level not in effort.LADDER:
            raise ValueError(f"unknown effort: {level!r} (have {list(effort.LADDER)})")
        self.meta["effort"] = level
        # Same condition as set_provider: nothing is written before a round has
        # committed, so an abandoned session leaves no directory.
        if self._records: store.write_json(self.dir / "meta.json", self.meta)

    @property
    def blobs_dir(self) -> Path: return self.dir / "blobs"

    @property
    def notebook(self) -> Path:
        """Session-scoped scratch space: working files that belong to this conversation, not to the user's project.

        A path, not a mkdir, like the session directory itself: a session that never writes a scratch file leaves no empty
        directory behind, since the file tool creates parents on write."""
        return self.dir / "notebook"

    # ---- recording ----

    def round_start(self):
        """Mirror of the agent loop's rollback point. A cancel rewinds to
        here; rounds that completed earlier are kept."""
        self._mark = len(self._pending)

    def rewind(self): del self._pending[self._mark:]

    # Every record is stamped when it happens, so no second field is needed.
    # replay only reads the keys it knows, so `ts` never reaches the provider.
    def add_user(self, text: str, images: list = None, note: bool = False):
        # Asked before the record is appended, and at all because add_user also carries
        # runtime notices: a background job finishing must not name the conversation.
        first = not any(r.get("t") == "user" for r in self._records) and \
                not any(r.get("t") == "user" for r in self._pending)
        rec = {"t": "user", "ts": store.now_iso(), "text": text}
        # Runtime text the worker put in front of the model rather than the person's own
        # words: a stale todo, a steer note, a job that finished. Still a user record,
        # because that is the turn the provider was handed -- but a reopened
        # conversation has to draw it as the notice it was, and as a plain `user`
        # record it read back as a line the person had typed.
        if note: rec["note"] = True
        # Attachments ([{"name": "shot.png", "b64": "..."}]) ride the turn they arrived
        # with, part of what the user said; base64 becomes a blob ref on the way to disk.
        if images: rec["images"] = list(images)
        self._pending.append(rec)
        if first and not self.meta.get("title") and self.meta.get("title_source", "") in ("", "stub"):
            from integrations.memory import naming
            # "" for a greeting, deliberately: a session called "hi" says nothing.
            # The namer fills it in once there is something to name.
            self.meta["title"], self.meta["title_source"] = naming.provisional(text), "stub"
        # The first thing the user says makes the session real: folder and meta.json exist
        # from here, not from an exit a killed frontend never gives us, after the stub name is set.
        if first: self.persist()

    def add_spend(self, cost: dict):
        """One round a side loop ran -- a subagent, a page digest, the permission gate.

        Billed to this conversation whether or not its own loop ran it, so it is recorded
        here rather than left to be noticed as a gap later. Its own record type, which replay
        skips: a count is not a message the provider ever sent. Called from usage.absorb on
        the loop's own thread, so it needs no more care than the appends beside it.
        """
        if not cost: return
        self._pending.append({"t": "usage", "ts": store.now_iso(), "u": cost})

    def add_recall(self, text: str):
        """A pointer block the runtime put in front of the model.

        Its own record type, not glued onto the user's text, so a line the user never typed does not read back as one. replay
        folds it into the user turn it belongs to, since two user messages in a row is a 400 on anthropic."""
        self._pending.append({"t": "recall", "ts": store.now_iso(), "text": text})

    def add_peer(self, from_id: str, from_name: str, from_cwd: str, text: str):
        """Another agent's words, arriving mid-turn.

        Its own record type rather than a `recall`, which is text the runtime wrote
        about this machine: a reopened conversation has to be able to say a peer said
        this, and who the peer was. replay folds it into the user turn it rode, the
        same way and for the same reason."""
        self._pending.append({"t": "peer", "ts": store.now_iso(), "from": from_id,
                              "from_name": from_name, "from_cwd": from_cwd, "text": text})

    def set_title(self, title: str, source: str = "auto"):
        self.meta["title"], self.meta["title_source"] = title, source
        if self._records: store.write_json(self.dir / "meta.json", self.meta)

    def set_frontend(self, name: str):
        """Which frontend owns this session, stamped from the terminal envelope's
        term_program, so a controller can tell its own sessions from another's."""
        if name and name != self.meta.get("frontend"):
            self.meta["frontend"] = name
            if self._records: store.write_json(self.dir / "meta.json", self.meta)

    def set_pinned(self, on: bool):
        """The tree's pin, persisted with the conversation: an iso ts, oldest first.
        Written even before the first commit, which rewrites meta.json from memory."""
        if on:
            if not self.meta.get("pinned"): self.meta["pinned"] = store.now_iso()
        else:
            self.meta.pop("pinned", None)
        store.write_json(self.dir / "meta.json", self.meta)

    def set_cwd(self, path: str):
        """Where this conversation is happening, recorded for recall: matching
        directories is the strongest signal for what was done here last time."""
        if path and path != self.meta.get("cwd"):
            self.meta["cwd"] = str(path)
            if self._records: store.write_json(self.dir / "meta.json", self.meta)

    def persist(self):
        """Make the session real: meta.json on disk, no messages yet. Called from the
        first thing the user says, so the tree and palette can list it and a hard kill
        leaves it findable, while an opened-but-never-used session writes nothing.
        The cwd and frontend reached it earlier, in memory only."""
        if self._records: return   # a committed session already persists itself
        store.write_json(self.dir / "meta.json", self.meta)

    def add_assistant(self, thinking: str, content: str, native: list, usage: dict = None):
        # Recorded as canonical calls, never in the provider's own shape. A record outlives the
        # dialect that wrote it, and the shape is the dialect's: the same conversation has to
        # reopen after a provider moves from /api/chat to the openai schema, where a call is
        # spelled differently and the stored one is rejected.
        calls = [{"name": c.name, "args": c.args}
                 for c in providers.get(self.provider).parse_calls(native)]
        rec = {"t": "assistant", "ts": store.now_iso(), "p": self.provider,
               "thinking": thinking, "content": content, "calls": calls}
        # What the round cost: one round, one model, and a rewind takes it with the
        # round. Absent when the provider reported nothing, not the same as zero.
        if usage: rec["u"] = usage
        self._pending.append(rec)

    def add_tool(self, name: str, result: dict):
        self._pending.append({"t": "tool", "ts": store.now_iso(), "name": name, "result": result})

    def commit(self):
        """Append everything recorded since the last commit.

        An assistant turn whose tool_calls have no matching results is rejected on the next request, so it
        must never reach disk; committing per round caps the damage of a hard kill to the round in flight.

        Side rounds come in through the same door: whatever a subagent or digest billed since the last
        commit is taken first, under the conversation that is live now, which is the only hand it has left
        if the turn that started it is long over."""
        usage.absorb(self)
        if not self._pending: return
        out = [self._split(r) for r in self._pending]
        store.append_jsonl(self.dir / "messages.jsonl", out)
        self._records.extend(out)
        # Emptied, or the next commit writes this whole round again: the append is
        # against the file, and pending is everything since the last commit, not
        # since the last append. Deleting this line made every round land on disk once
        # per later commit and counted it once per later commit (2026-10-09).
        self._pending.clear()
        self._mark = 0
        self.meta.update(updated=store.now_iso(), records=len(self._records),
                         turns=sum(1 for r in self._records if r.get("t") == "assistant"),
                         read_files=_read_files(),
                         # Rolled up here so a conversation is answered from the meta
                         # files alone; recomputed, so a rewound round takes its cost.
                         usage=usage.of_records(self._records))
        store.write_json(self.dir / "meta.json", self.meta)

    def _split(self, rec: dict) -> dict:
        if rec.get("t") == "tool":
            return {**rec, "result": blobs.split(rec.get("result", {}), self.blobs_dir)}
        # A user turn carrying attachments is the other record with megabytes in
        # it, and it goes out through the same door.
        if rec.get("t") == "user" and rec.get("images"):
            return {**rec, "images": blobs.split(rec["images"], self.blobs_dir)}
        return rec

    # ---- loading ----

    @property
    def system(self) -> str:
        """Read fresh, never persisted, and never part of messages(): the openai dialect takes it
        as leading messages, anthropic as a request parameter."""
        return store.system_prompt()

    def records(self) -> list[dict]:
        """The canonical records, committed ones first.

        Frontends rebuilding a conversation read these rather than messages(): the same shape whichever
        provider wrote them, and they still carry the thinking and tool results the dialects drop."""
        return list(self._records) + list(self._pending)

    def messages(self) -> list[dict]:
        """Provider-shaped conversation history, system prompt excluded."""
        recs = [blobs.rehydrate(r, self.blobs_dir) for r in self._records]
        return replay.to_messages(recs, self.provider)

    def rewrite(self, records: list[dict], fold: dict = None):
        """Replace the record list with a folded one. Only compaction does this.

        Everything else in this class appends, because a conversation is a thing
        that happened and records are how it is remembered. A fold is the one
        exception: a deliberate lossy rewrite of the middle.

        Nothing is thrown away. The previous list is archived whole, under
        `folds/<n>.jsonl`, and meta.json gets a `compactions` entry recording what
        was folded, what it freed, and which archive holds the original -- so a
        fold can always be undone by hand and the session says so. Losing a user's
        history to save tokens would be a bad trade to make silently.
        """
        fold = fold or {}
        # Archive first, and only then replace: a crash between the two leaves the
        # full history on disk and a fold that never happened, which is the safe
        # failure. The other order loses the original.
        before = list(self._records) + list(self._pending)
        n = len(self.meta.get("compactions") or []) + 1
        archive = None
        if before:
            path = self.folds_dir / ("%02d.jsonl" % n)
            store.write_jsonl(path, before)
            archive = str(path.relative_to(self.dir))
        self._records = list(records)
        self._pending = []
        self._mark = len(self._records)
        # What happened, kept on the session rather than only in the summary: the
        # summary is what the model reads, this is what a person can check.
        entry = {"ts": store.now_iso(), "n": n,
                 "records_before": len(before), "records_after": len(self._records),
                 "folded": fold.get("folded", 0), "freed": fold.get("freed", 0),
                 "words": len(str(fold.get("summary") or "").split()),
                 "archive": archive}
        self.meta.setdefault("compactions", []).append(entry)
        self.meta["compacted"] = len(self.meta["compactions"])
        if self._records:
            store.write_jsonl(self.dir / "messages.jsonl", self._records)
            store.write_json(self.dir / "meta.json", self.meta)

    @property
    def folds_dir(self) -> Path:
        """Where the pre-fold history is archived. A path, not a mkdir: a session
        that never folded leaves no empty directory behind."""
        return self.dir / "folds"

    def set_model(self, model: str):
        """Which model this conversation is on. Recorded so a reopened session can
        say what wrote it."""
        self.meta["model"] = model
        if self._records: store.write_json(self.dir / "meta.json", self.meta)

    def set_provider(self, name: str):
        """Point the conversation at another provider. Records are canonical, so
        messages() rebuilds the whole history in the new dialect, images and all."""
        self.meta["provider"] = name
        if self._records: store.write_json(self.dir / "meta.json", self.meta)

    def restore_tool_state(self):
        """Re-seed the edit gate in tools/file, minus anything that moved on disk while
        the session was closed: a stale entry lets the agent edit unseen content."""
        fresh = [e["path"] for e in (self.meta.get("read_files") or []) if _unchanged(e)]
        if not fresh: return
        from tools.loader import load_tools
        load_tools(str(store.tools_dir()))   # importing the tools is what puts _common in sys.modules
        _seed_read_files(fresh)

# tools/file/main.py puts its helpers on sys.path directly, so _common lands
# as a top-level module rather than under a package.
def _read_files() -> list[dict]:
    """Snapshot the edit gate with enough to detect drift later: paths alone would be a
    lie after a reload, three days and an outside edit on."""
    m = sys.modules.get("_common")
    if not m: return []
    out = []
    for p in sorted(getattr(m, "read_files", ())):
        # mtime+size rather than a hash: commit runs every round, and the
        # agent reads files it has no reason to re-checksum that often.
        try: st = os.stat(p)
        except OSError: continue
        out.append({"path": p, "mtime_ns": st.st_mtime_ns, "size": st.st_size})
    return out

def _unchanged(e: dict) -> bool:
    try: st = os.stat(e["path"])
    except (OSError, KeyError, TypeError): return False
    return st.st_mtime_ns == e.get("mtime_ns") and st.st_size == e.get("size")

def _seed_read_files(paths):
    m = sys.modules.get("_common")
    if m and hasattr(m, "read_files"): m.read_files.update(paths)


if __name__ == "__main__":
    # One runnable check: opening a frontend and closing it again should cost nothing.
    # The folder appears on the first message, so a closed frontend leaves no trace.
    import tempfile
    with tempfile.TemporaryDirectory() as tmp:
        os.environ["CUACODE_HOME"] = tmp
        s = Session.create(provider="ollama", model="")
        assert not s.dir.is_dir(), "a booted session writes nothing"
        s.set_cwd("/tmp/wherever")
        s.set_frontend("vscode")
        assert not s.dir.is_dir(), "nor does learning where it is running"
        s.add_user("hello there")
        assert (s.dir / "meta.json").is_file(), "the first message makes it real"
        meta = (s.dir / "meta.json").read_text()
        assert "/tmp/wherever" in meta and "vscode" in meta, meta
        assert not (s.dir / "messages.jsonl").exists(), "nothing is committed yet"
        s.commit()
        assert (s.dir / "messages.jsonl").is_file(), "a round commits what it recorded"
        # One round, one record: commit appends what is pending and then empties it,
        # so a second commit with nothing new writes nothing. Without the clear, each
        # commit re-appends everything so far and a round is counted once per later
        # commit, which is how meta.json came to report 1445 rounds for 50 real ones.
        s.add_user("a second thing")
        s.commit()
        s.commit()
        n = len([l for l in (s.dir / "messages.jsonl").read_text().splitlines() if l.strip()])
        assert n == 2, f"two rounds, two records; got {n}"
        assert s.meta["records"] == 2, s.meta["records"]
    print("session birth ok")
