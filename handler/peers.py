"""Peers: the mailbox two live agents talk through.

There are no sockets here. Each worker's stdout belongs to its own frontend, so
one agent cannot write into another's pipe, and the state root is the only
channel two processes share. A peer message is therefore a line appended to the
recipient's mailbox, and the recipient's own loop drains it.

Delivery has two entry points because "the next available point" is different
depending on what the recipient is doing. Mid-turn, the message rides the same
steer seam a person's typed words do, which lands it straight after a round's
tool results and never interrupts reasoning or a call in flight. Idle, the loop
turns it into an ordinary turn, which is what waking an asleep agent means.

A message is claimed by the sender and acknowledged by the recipient rather than
being deleted on read, so a turn that ends before the note is injected hands the
message back instead of swallowing it.

Nothing here trusts the sender's word for who it is: the name, the directory and
the liveness all come off the session the sender is standing in.
"""
import os, time, uuid

from handler.session import store

TTL = 300          # a peer untouched for this long is not offered on the roster
MAX_BODY = 32768   # one message. The same ceiling cmux puts on its own agent messages
TITLE_CHARS = 32   # of an auto-generated title, in a name that has to fit one head line
# How many peer messages, on either side, one exchange may carry before the next is
# refused. Two agents that each answer every message have no reason to stop, so the
# limit is mechanical rather than a rule in the prompt they might talk themselves out
# of. Counted off both mailboxes, so the two processes agree without talking to each
# other, and the refusal costs a deliberate second call the way the quiet warning does.
EXCHANGE_CAP = 12
MAILBOX = "peers.jsonl"

# Mail that has been handed to the loop but not yet injected, so a turn that
# ends mid-round gives it back rather than losing it. Keyed by message id.
_claimed: dict[str, dict] = {}

# Ids already carried by the turn that took them. `ack` pops, so it cannot answer
# "has this one been delivered yet" -- the next drain would see the record again.
_acked: set[str] = set()


# ---- where a peer lives ----

def mailbox(sid: str):
    return store.path(sid) / MAILBOX

def pid_path(sid: str):
    return store.path(sid) / "worker.pid"

def become(sid: str):
    """Record this worker's pid, so a peer can tell a slow agent from a dead one.

    meta.json's `updated` only moves when a round commits, so an agent ten minutes
    into a build is indistinguishable from one that was killed. The pid is the only
    thing that can tell them apart, and nothing else writes one.
    """
    p = pid_path(sid)
    me = str(os.getpid())
    try:
        if p.exists() and p.read_text().strip() == me: return
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(me)
    except OSError:
        pass

def alive(sid: str) -> bool:
    """Whether a worker holding this session is still running.

    A signalling check, not a proof: a recycled pid reads as alive. The cost of
    being wrong is one message to a mailbox nobody reads, which the sender is
    warned about anyway.
    """
    try:
        pid = int(pid_path(sid).read_text().strip())
    except (OSError, ValueError):
        return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True

def stamp(sid: str) -> float:
    """When this agent last did anything, as a sort key. The session directory's
    own mtime, which moves on every commit, beats re-reading a meta.json per peer."""
    try:
        return store.path(sid).stat().st_mtime
    except OSError:
        return 0.0


# ---- who a peer is ----

def _meta(sid: str) -> dict:
    return store.read_json(store.path(sid) / "meta.json")

def name(sid: str, meta: dict = None) -> str:
    """What to call this agent. Its project, or its title when the project does not
    say enough, which for two agents in one tree is most of the time.

    A title is auto-generated and a project is a fact, so the project leads and the
    title rides behind it. The title is clipped because the namer sometimes returns
    the whole opening message, and a head line that wraps is a head line nobody reads.
    """
    meta = meta or _meta(sid)
    cwd = str(meta.get("cwd") or "")
    home = str(store.home())
    short = "~" if cwd == home else (os.path.basename(cwd.rstrip("/")) or "~")
    title = str(meta.get("title") or "").strip().replace("\n", " ")
    if len(title) > TITLE_CHARS: title = title[:TITLE_CHARS - 1].rstrip() + "\u2026"
    return f"{short}: {title}" if title else short

def brief(sid: str) -> dict:
    """One agent, as `init` reports it."""
    meta = _meta(sid)
    return {"id": sid, "name": name(sid, meta), "cwd": meta.get("cwd", ""),
            "idle_for": int(max(0, time.time() - stamp(sid))),
            "alive": alive(sid)}


# ---- the roster ----

def reachable(sid: str) -> bool:
    """Offered on the roster: freshly active, or still running whatever it is doing.

    The pid is checked second because it is the expensive half, and the timestamp
    alone answers for every agent that has spoken recently. A slow agent that is
    still a process stays on the roster while it builds; a killed one drops off.
    """
    return time.time() - stamp(sid) <= TTL or alive(sid)

def quiet_note(sid: str, meta: dict = None) -> str:
    """The warning a first send to a quiet agent comes back with.

    Whether a process is still holding it is the one thing that changes what the
    sender should do next, so it is the first thing said.
    """
    idle = int(max(0, time.time() - stamp(sid)))
    if alive(sid):
        return (f"{name(sid, meta)} has been quiet for {idle}s but a process is still holding it, "
                "so it will read this at its next round. Send the same call again to deliver it.")
    return (f"{name(sid, meta)} has been quiet for {idle}s and no process is holding it. "
            "Nothing will read what you send. Send the same call again if you want it written anyway.")

def _same_dir(a: str, b: str) -> bool:
    """Whether two recorded cwds are the same directory, by the filesystem's answer.

    Comparing the strings is wrong here: macOS is case-insensitive, so
    `/Users/x/documents/work` and `/Users/x/Documents/Work` are one directory and
    two strings, and a session is opened from whichever spelling the caller
    typed. The inode is the answer that does not depend on spelling, and
    samefile() is the stdlib's way to ask for it.

    A path that no longer exists has no inode and falls back to the exact string,
    which is right for a directory nobody is standing in any more.
    """
    if a == b:
        return True
    if not a or not b:
        return False
    try:
        return os.path.samefile(a, b)
    except OSError:
        return False

def roster(sid: str) -> list[dict]:
    """The other agents worth talking to: same project, reachable, this one left out.

    Same project rather than every session on the machine, because the useful peer is
    one standing in the code being discussed. A caller that knows an id can still
    reach a session this leaves off - see `send`.

    `same_cwd` is false when the peer is in the same directory written another way -
    a different spelling of the same path, which on a case-insensitive filesystem is
    still the same code. It is reported rather than filtered, because it changes what
    the sender should assume: the peer is looking at the same files, and the two of
    them can still be editing different lines of them.
    """
    mine = str(_meta(sid).get("cwd") or "")
    out, seen = [], set()
    for meta in store.list_sessions():
        other = str(meta.get("id") or "")
        # Newest first from list_sessions, and one project can hold several sessions
        # under the same title, so the first sighting of a title wins.
        if not other or other == sid or other in seen: continue
        there = str(meta.get("cwd") or "")
        if not _same_dir(there, mine): continue
        if not reachable(other): continue
        seen.add(other)
        row = brief(other)
        row["same_cwd"] = there == mine
        if not row["same_cwd"]:
            # The path is the same directory written differently, so say that rather
            # than leaving a false for the reader to interpret. What it changes is the
            # sender's assumption about the peer's view of the work, not its reachability.
            row["cwd_note"] = ("same directory, different spelling of the path (this filesystem "
                               "does not distinguish case): the peer is working in these same "
                               "files, so check what it is editing before touching them")
        out.append(row)
    return out


# ---- how much has passed between two agents ----

def _count(path) -> int:
    """Lines in a mailbox. Only ever counts what has been written, so a torn last line
    from a kill mid-append reads as that message not having happened."""
    try:
        with open(path, "rb") as f:
            return sum(1 for line in f if line.strip())
    except OSError:
        return 0

def exchange(a: str, b: str) -> int:
    """Everything that has passed between two agents, both directions.

    Counted on the mailboxes rather than kept on either side: neither process can see
    the other's memory, and both can see these two files. A message that was collected
    but not yet acknowledged is still on one of them, so the number only ever grows
    while an exchange is alive.
    """
    return _count(mailbox(a)) + _count(mailbox(b))

def exchange_note(other: str, n: int) -> str:
    """Why the next message was refused, and the one thing that makes it go through."""
    return (f"{n} messages have already passed between you and {name(other)} in this exchange, "
            f"which is the limit ({EXCHANGE_CAP}). Two agents answering each other have no "
            "natural end, so this stops here: answer the user instead. Send the same call "
            "again if this one message really matters, and it will go.")


# ---- sending ----

def send(to: str, body: str, frm: str) -> dict:
    """Append one message to the recipient's mailbox. Returns the stored record.

    The body is text with control characters stripped, for the same reason a tool
    result is: it crosses into a prompt, and a newline is the most it may carry.
    """
    meta = _meta(frm)
    rec = {"id": uuid.uuid4().hex[:12], "ts": store.now_iso(),
           "from": frm, "from_name": name(frm, meta), "from_cwd": meta.get("cwd", ""),
           "body": str(body or "").replace("\x00", "")}
    store.append_jsonl(mailbox(to), [rec])
    return rec

def target(to: str) -> dict | None:
    """The session an id or a name resolves to, or None.

    An exact id first, then a unique name prefix, so a caller that only ever saw
    the roster can still name one.
    """
    want = (to or "").strip()
    if not want: return None
    for meta in store.list_sessions():
        if str(meta.get("id") or "") == want: return meta
    hits = [m for m in store.list_sessions()
            if name(str(m.get("id") or ""), m).lower().startswith(want.lower())]
    return hits[0] if len(hits) == 1 else None


# ---- receiving ----

def collect(sid: str) -> list[dict]:
    """Move new mail off the mailbox and into the claimed set, oldest first.

    The rename is the whole concurrency story: a message appended after it lands in
    a fresh file and waits for the next drain, so nothing is read and then thrown
    away by a writer that got there first.
    """
    p = mailbox(sid)
    if not p.exists(): return []
    tmp = p.with_name(p.name + "." + uuid.uuid4().hex)
    try:
        os.replace(p, tmp)
    except OSError:
        return []
    got = store.read_jsonl(tmp)
    try: tmp.unlink()
    except OSError: pass
    for rec in got:
        if rec.get("id"): _claimed[rec["id"]] = rec
    return [r for r in got if r.get("id")]

def claimed(sid: str = "") -> list[dict]:
    """Everything collected, unacknowledged and not yet handed to a turn, oldest first."""
    return sorted((r for r in _claimed.values() if r.get("id") not in _acked),
                  key=lambda r: str(r.get("ts") or ""))

def drain(sid: str) -> list[dict]:
    """New mail plus anything claimed and unacked, for the round seam.

    `claimed()` alone only sees the in-memory set, and the only thing filling it is
    `collect()`, which an idle agent reaches through `as_cmds`. So a round seam asking
    `claimed()` finds nothing on the path that has to work: an agent inside a long
    turn, where the main loop sits in generate() and never reaches the idle branch.
    This is the collect the seam was missing.

    Skipped inside a subagent: the mailbox belongs to the worker's own session, and a
    subagent shares its parent's `ctx` down to `session_dir`, so a peer's words would
    land in an agent that is not the one being talked to.
    """
    try:
        from handler.agent import subagent
        if subagent.depth() > 0: return []
    except Exception:
        pass
    collect(sid)
    return claimed(sid)

def ack(ids: list[str]):
    """Hand a delivered message over for good. Called once the model has actually
    been given it, so a turn that dies before the note is injected keeps the mail.

    The id is kept in `_acked`: that set is what stops the seam injecting the same words
    again, since the record is gone from `_claimed` either way.
    """
    for i in ids:
        _claimed.pop(i, None)
        _acked.add(i)

def render(rec: dict) -> str:
    """One message, as the model reads it. The sender is named in the open tag
    rather than left to prose, so a peer can tell whose words these are without
    inferring it from the text.
    """
    return ("<peer_message from=\"%s\" session=\"%s\" cwd=\"%s\" sent=\"%s\">\n%s\n</peer_message>"
            % (rec.get("from_name", "?"), rec.get("from", "?"), rec.get("from_cwd", ""),
               rec.get("ts", ""), rec.get("body", "")))

def head(rec: dict) -> str:
    """The line a frontend draws for it. Everything after the first newline is
    hidden behind it, so the head has to say who and what in one line.

    The mark that leads it is the frontend's own (fastrs draws the row with a
    four-pointed star), so this is text only: a glyph here would be a second
    character for the same meaning.
    """
    return f"peer message from {rec.get('from_name', '?')} ({rec.get('from', '?')})"

def as_cmds(sid: str) -> list:
    """Claimed mail as `cmd` envelopes, shaped like the ones a frontend sends, so an
    idle agent takes a peer message through the same door a person's would.

    This claims and does not release: a message handed to an idle agent goes through the
    turn as an ordinary chat, so the round seam inside that turn would find the same
    message still claimed and inject it a second time. The ack below is what stops it:
    `claimed()` leaves an acknowledged id out, so what the turn already carries is not
    said again.
    """
    from handler.protocol import Envelope
    out = []
    for rec in collect(sid):
        text = f"agent {rec.get('from_name')} sent this while you were idle\n\n{render(rec)}"
        out.append(Envelope("cmd", rec["id"], {"action": "chat", "text": text,
                                               "peer": {"id": rec.get("id"), "from": rec.get("from"),
                                                        "name": rec.get("from_name"),
                                                        "from_cwd": rec.get("from_cwd"),
                                                        "body": rec.get("body", ""),
                                                        "text": render(rec)}}))
    if out: ack([e.id for e in out])
    return out
