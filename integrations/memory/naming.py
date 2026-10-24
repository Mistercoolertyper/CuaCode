"""What a conversation is called.

A session is found again by its name, the only part of the record that has to be readable in a list of forty, and the first message is a bad name (people open with "hi"). So a stub stands in until a small model call, once the conversation has a subject, returns a real name: three attempts at most, at turns 1, 3 and 6, stopping at the first confident answer.
That call runs on its own thread and never touches disk or the wire, handing its answer to a queue the loop drains between turns, because the alternative is a background thread writing meta.json while the main one is committing to it.
"""
import queue, re, threading

from handler.session import store
from integrations.memory.recall import tokens

# Turn counts at which naming is attempted. Not every turn: the answer rarely changes
# and each attempt is a request.
THRESHOLDS = (1, 3, 6)
MAX_TITLE = 48

# Who set the title, strongest first: auto-naming may only overwrite what auto-naming
# or the stub put there, never a name the user or the agent chose deliberately.
PRECEDENCE = ("user", "agent", "auto", "stub", "")

SYSTEM = """You name conversations. You are given the opening of one; return a short label for it.

- 48 characters at most. Shorter is better.
- Say what is being done, not that a conversation happened. "fix screenshot
  capture on windows", not "discussion about a screenshot problem".
- Verb first when there is an action. A noun phrase when there is not.
- Keep the concrete identifier if there is one -- a filename, an app, an error,
  a command. That is what makes one session findable among forty.
- Lowercase, except things that are actually capitalised: names, identifiers,
  file paths.
- No trailing period. Never the words "session", "chat", "conversation",
  "discussion", "help with", "assistance", "user asks".
- English, whatever language the conversation is in, unless the subject itself
  is a phrase in another language.

Set confident=false when the opening is only a greeting, small talk, or too
vague to name -- an honest false is worth more than a label like "general
questions", and you will be asked again once there is more to go on."""

SCHEMA = {"properties": {
    "title": {"type": "string", "description": "The label, 48 characters or fewer."},
    "confident": {"type": "boolean",
                  "description": "False if there is not yet enough here to name it."}},
    "required": ["title", "confident"]}

_done: "queue.Queue[dict]" = queue.Queue()
_running: set = set()
_lock = threading.Lock()

# The conversation the loop is on: tools share the process, so the memory tool retitles
# the live object rather than a meta.json the next commit would silently undo.
_live = None

def set_live(session):
    global _live
    _live = session

def live():
    return _live

def low_signal(text: str) -> bool:
    """Whether a message says anything worth naming a conversation after: content
    words after the stoplist, which already holds the greetings, two being the bar."""
    return len(set(tokens(text))) < 2

def provisional(text: str) -> str:
    """The placeholder shown until a real name arrives, or "" for a greeting.

    Empty on purpose: a frontend showing the session id is telling the truth.
    """
    if low_signal(text): return ""
    line = re.sub(r"\s+", " ", (text or "").strip().splitlines()[0] if text.strip() else "")
    return clean(line)

def clean(title: str) -> str:
    t = re.sub(r"\s+", " ", (title or "").strip()).strip("\"'` ")
    t = re.sub(r"^(session|chat|conversation|task)\s*[:\-–]\s*", "", t, flags=re.I)
    t = t.rstrip(".")
    if len(t) > MAX_TITLE: t = t[:MAX_TITLE].rsplit(" ", 1)[0].rstrip(" ,;:-") + "…"
    return t

def _rank(source: str) -> int:
    try: return PRECEDENCE.index(source or "")
    except ValueError: return len(PRECEDENCE)

def may_overwrite(meta: dict, source: str = "auto") -> bool:
    return _rank(source) <= _rank(meta.get("title_source", ""))

def _opening(records: list, limit: int = 8) -> str:
    """Enough of the conversation to name it, and no more.

    Tool names rather than tool results: "screenshot, click, type_text" says what the work was in ten
    tokens, where the results would be a megabyte of base64."""
    parts, tools = [], []
    for r in records[:limit]:
        t = r.get("t")
        if t == "user" and r.get("text"): parts.append("User: " + r["text"][:500])
        elif t == "assistant" and r.get("content"): parts.append("Assistant: " + r["content"][:400])
        elif t == "tool" and r.get("name") and r["name"] not in tools: tools.append(r["name"])
    if tools: parts.append("Tools used: " + ", ".join(tools[:10]))
    return "\n\n".join(parts)[:4000]

def _work(sid: str, opening: str, current: str, provider: str, model: str):
    from handler.agent.subagent import AgentSpec, run as run_agent
    out = {"sid": sid, "title": "", "confident": False}
    try:
        prompt = (f"Current label: {current!r}\n\n" if current else "") + \
                 f"Opening of the conversation:\n\n{opening}"
        r = run_agent(AgentSpec(name="namer", tools=[], effort="low", max_rounds=2,
                                system=SYSTEM, schema=SCHEMA, provider=provider, model=model or None),
                      prompt)
        if not r.get("error"):
            got = r.get("output") or {}
            out["title"] = clean(str(got.get("title") or ""))
            out["confident"] = bool(got.get("confident")) and bool(out["title"])
    except Exception:
        # A name is a nicety: a failure is not worth surfacing to the user and should
        # not reach the turn that follows.
        pass
    finally:
        with _lock: _running.discard(sid)
        _done.put(out)

def maybe_start(session) -> bool:
    """Fire the namer if this session is due one. Returns whether it did."""
    meta = session.meta
    attempts = int(meta.get("title_attempts") or 0)
    if attempts >= len(THRESHOLDS): return False
    if not may_overwrite(meta, "auto"): return False
    if int(meta.get("turns") or 0) < THRESHOLDS[attempts]: return False
    with _lock:
        if session.id in _running: return False
        _running.add(session.id)
    meta["title_attempts"] = attempts + 1
    threading.Thread(target=_work, daemon=True,
                     args=(session.id, _opening(session.records()), meta.get("title", ""),
                           meta.get("provider") or "", meta.get("model") or "")).start()
    return True

def drain() -> list[dict]:
    """Finished namings, for the loop to apply where it is safe to."""
    out = []
    while True:
        try: out.append(_done.get_nowait())
        except queue.Empty: return out

def rename(sid: str, title: str, source: str = "agent", session=None) -> dict | None:
    """Retitle one session, live object or file, whichever this id is.

    Returns None when a stronger hand already named it: a late auto-namer must not undo a title the user
    typed, and neither must the agent."""
    title = clean(title)
    if not sid or not title: return None
    session = session if session is not None else live()
    if session is not None and getattr(session, "id", None) == sid:
        if not may_overwrite(session.meta, source): return None
        session.set_title(title, source=source)
        return {"session_id": sid, "title": title}
    p = store.path(sid) / "meta.json"
    meta = store.read_json(p)
    if not meta or not may_overwrite(meta, source): return None
    meta["title"], meta["title_source"] = title, source
    store.write_json(p, meta)
    return {"session_id": sid, "title": title}

def apply(result: dict, session=None) -> dict | None:
    """Put a finished naming where it belongs, if it is still wanted."""
    if not result.get("confident") or not result.get("title"): return None
    return rename(result["sid"], result["title"], source="auto", session=session)
