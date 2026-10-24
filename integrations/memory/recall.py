"""What the user just said, matched against what is already known.

Pointers, never bodies. A hit puts one line in front of the model and the model
decides whether to spend a tool call opening it: a wrong pointer costs a line, a
wrong injected body costs the context and gets acted on.

A pointer only earns its line if it names something the model does not already have.
The memory tool's description lists every in-scope memory every turn, so pointing at
one of those restates the tool list, which teaches the model to skim the whole block.
Memories here are therefore drawn from what the index leaves out.

Runs on every user message, so it is lexical and structural only: no model call, no
network, no embedding, because anything taking a second here is felt on every turn.
"""
import os, re, time
from datetime import datetime, timezone

from handler.session import store
from integrations.memory import loader

MAX_MEMORIES = 3
MAX_SESSIONS = 2
MIN_TURNS = 2             # a one-turn session is a question and an answer, not a thing to reopen
# Two bars, because the corpora differ. A memory's description is a sentence written
# to be recognised by and there are tens of them, so one solid word is evidence;
# session titles are generated and there are hundreds, so one shared word between two
# of them means nothing.
MIN_MEMORY = 1.0
MIN_SESSION = 1.6
SESSION_POOL = 60         # newest sessions considered; older than that, search finds them by hand
HALF_LIFE_DAYS = 10.0

STOP = {
    "the","a","an","and","or","but","if","then","that","this","these","those","is","are","was",
    "were","be","been","being","to","of","in","on","at","for","with","from","by","as","it","its",
    "i","you","we","they","he","she","me","my","your","our","their","do","does","did","doing",
    "can","could","should","would","will","shall","may","might","must","have","has","had","not",
    "no","yes","so","just","also","get","got","make","made","let","lets","please","thanks","hi",
    "hey","hello","yo","ok","okay","what","when","where","which","who","why","how","again","now",
    "up","out","about","into","over","after","before","more","most","some","any","all","one","two",
}

# Words that point at something the sentence never named. When one turns up, the thing
# meant is usually the last thing that happened here rather than whatever shares a
# keyword, so recency becomes the signal rather than a tiebreak.
DEIXIS = re.compile(r"\b(that|those|it|again|earlier|before|last time|yesterday|the other day|"
                    r"we were|we did|you did|like we|same as|continue|resume|pick up|back to)\b", re.I)

def tokens(text: str) -> list[str]:
    """Words, plus the pieces of any path or identifier in them.

    Splitting on non-alphanumerics is what lets `handler/session/store.py` match a
    session that touched `store.py`, with no special case for paths.
    """
    out = []
    for raw in re.split(r"[^A-Za-z0-9_./\\-]+", (text or "").lower()):
        if not raw: continue
        for piece in re.split(r"[^a-z0-9]+", raw):
            if len(piece) > 1 and piece not in STOP and not piece.isdigit(): out.append(piece)
    return out

def _weights(docs: list[list[str]]) -> dict:
    """Rarity weights. A token in every document says nothing about which one
    is meant; a token in exactly one says everything."""
    n = max(len(docs), 1)
    df = {}
    for d in docs:
        for t in set(d): df[t] = df.get(t, 0) + 1
    return {t: (n / c) ** 0.5 for t, c in df.items()}

def _score(q: set, doc: list[str], w: dict) -> float:
    seen = set(doc)
    return sum(w.get(t, 1.0) for t in q if t in seen)

def _age_days(iso: str) -> float:
    try: return max((datetime.now(timezone.utc) - datetime.fromisoformat(iso)).total_seconds(), 0) / 86400
    except Exception: return 999.0

def _ago(iso: str) -> str:
    d = _age_days(iso)
    if d < 1 / 24: return "just now"
    if d < 1: return f"{int(d * 24)}h ago"
    if d < 30: return f"{int(d)}d ago"
    return f"{int(d / 30)}mo ago"

# Suggested-already, per session, in memory on purpose: the point is not to repeat a
# pointer the model declined once, and that judgement does not usefully survive a
# restart.
_shown: dict[str, set] = {}

def _fresh(sid: str, key: str) -> bool:
    seen = _shown.setdefault(sid or "-", set())
    if key in seen: return False
    seen.add(key)
    return True

def forget(sid: str):
    _shown.pop(sid or "-", None)

def _memory_hits(q: set, path: str, apps: list) -> list:
    # Out of scope on purpose: everything in scope is already listed in the memory
    # tool's description on this very request.
    mems = loader.out_of_scope(path, apps)
    if not mems: return []
    docs = [tokens(f"{m.name} {m.description} {m.type}") for m in mems]
    w = _weights(docs)
    out = []
    for m, d in zip(mems, docs):
        # The name is the fact's identity, chosen as the handle for exactly this, so
        # matching one counts for more than matching a word in the sentence.
        named = len(q & set(tokens(m.name)))
        # One shared word is a coincidence, not a match (the same reason session titles
        # need a higher bar); any word occurring once reads as decisive only because
        # rarity is measured against this handful. The name is the exception.
        if not named and len(q & set(d)) < 2: continue
        s = _score(q, d, w) + 0.5 * named
        if s >= MIN_MEMORY: out.append((s, m))
    return sorted(out, key=lambda p: -p[0])

def _session_hits(q: set, path: str, sid: str, deictic: bool) -> list:
    """Past conversations worth reopening.

    Reads meta only: list_sessions() refuses to open messages.jsonl on purpose, and
    scoring a hundred sessions would otherwise mean reading a hundred conversations'
    worth of screenshots.
    """
    here = os.path.abspath(os.path.expanduser(path)) if path else ""
    metas = [m for m in store.list_sessions()[:SESSION_POOL]
             if m.get("id") != sid and (m.get("title") or m.get("cwd"))
             and int(m.get("turns") or 0) >= MIN_TURNS]
    if not metas: return []
    docs = [tokens(" ".join([m.get("title", ""), os.path.basename(m.get("cwd", "") or ""),
                             " ".join(os.path.basename(f.get("path", "")) for f in (m.get("read_files") or [])[:20])]))
            for m in metas]
    w = _weights(docs)
    out = []
    for m, d in zip(metas, docs):
        same_dir = bool(here and m.get("cwd") and os.path.abspath(m["cwd"]) == here)
        s = _score(q, d, w)
        # Same directory is the strongest free signal, so it stands on its own: "what
        # did we do here" has to find this session even when the words share nothing.
        if same_dir: s += 1.4
        if not s: continue
        # Recency decays a match rather than making one. A deictic prompt names nothing,
        # so there it may carry the whole score.
        recency = 0.5 ** (_age_days(m.get("updated", "")) / HALF_LIFE_DAYS)
        s = s * (0.6 + 0.4 * recency) + (1.5 * recency if deictic and same_dir else 0)
        if s >= MIN_SESSION: out.append((s, m))
    return sorted(out, key=lambda p: -p[0])

def block(text: str, sid: str = "", path: str = None, apps: list = None) -> list[str]:
    """The recall notes for one user message. Empty list when nothing matched.

    Empty is the expected answer most turns: a block that appears every time gets read
    as furniture, and then the one that mattered is furniture too.

    Two notes rather than one, because a memory and a past conversation are not the
    same kind of thing: a fact written down to be reused against a place to go
    looking. Under one heading the reader has to sort them apart first.
    """
    q = set(tokens(text))
    if not q and not DEIXIS.search(text or ""): return []
    path = path if path is not None else loader.cwd()
    deictic = bool(DEIXIS.search(text or ""))

    mem, ses = [], []
    try:
        for s, m in _memory_hits(q, path, apps):
            if len(mem) >= MAX_MEMORIES: break
            if _fresh(sid, f"m:{m.name}"): mem.append(f"- `{m.name}` [{m.scope}] - {m.description}")
    except Exception: pass
    try:
        for s, meta in _session_hits(q, path, sid, deictic):
            if len(ses) >= MAX_SESSIONS: break
            if not _fresh(sid, f"s:{meta['id']}"): continue
            title = (meta.get("title") or "").strip() or "untitled"
            where = "same directory" if (path and meta.get("cwd") and
                                         os.path.abspath(meta["cwd"]) == os.path.abspath(path)) else ""
            tail = ", ".join(x for x in (f"{meta.get('turns', 0)} turns", _ago(meta.get("updated", "")), where) if x)
            ses.append(f"- `{meta['id']}` \"{title}\" - {tail}")
    except Exception: pass

    # The first paragraph is for the human: frontends draw a notice's opening paragraph
    # and drop the rest, which is addressed to the model.
    out = []
    if mem:
        out.append(f"recall: {len(mem)} {'memory' if len(mem) == 1 else 'memories'} outside this scope\n\n"
                   "<recall-memories>\n"
                   "Memories that your tool list does *not* carry here -- they belong to another\n"
                   "project or to an app that is not on screen, so this is the only mention of them\n"
                   "you will get. Matched by word overlap on the name and the line, which is a guess.\n"
                   "Nothing has been read. `memory` with action `load` opens one; ignore the rest\n"
                   "silently -- do not mention them.\n"
                   + "\n".join(mem) + "\n</recall-memories>")
    if ses:
        out.append(f"recall: {len(ses)} earlier session{'' if len(ses) == 1 else 's'} that may be related\n\n"
                   "<recall-sessions>\n"
                   "Past conversations, matched on their titles, directories and the files they read.\n"
                   "A match is a guess, not evidence. `memory` with action `session` and the id reads\n"
                   "one back as text if it bears on what was asked; ignore the rest silently -- do not\n"
                   "mention them.\n"
                   + "\n".join(ses) + "\n</recall-sessions>")
    return out
