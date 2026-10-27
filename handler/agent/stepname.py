"""What a step was doing, in a few words.

A compact screen shows one line per stretch of work, and that line wants a name.
The thought's own first sentence is a sentence, not a label, and asking the model
that is holding the conversation would cost a round trip on the clock that
matters. So a small model is asked instead, given only the thought and the calls
that came out of it: never the conversation, never a tool result.

The answer goes back as `token/step_name` carrying the first words of the thought
it names, because the frontend's block ids are its own counter and the worker
cannot address a row with one. Both ends hold the thought, so both ends can meet
on it.

ponytail: one namer at a time, and a step it cannot start for is left unnamed.
A queue would name steps minutes after they scrolled away, and a compact screen
without a name is what it was before this existed.
"""
import json
import re
import threading
from collections import deque

from handler import config

# A head line, not a sentence. Long enough for a filename, short enough that a
# step is still one line.
MAX_NAME = 46
# The handle the frontend matches on, and enough of the thought to say what it is.
HINT = 64
# Below this there is nothing to name: "hm" names itself.
MIN_THOUGHT = 24

SYSTEM = """You label one step of an AI coding agent's work, for a transcript that
shows the conversation one line per step.

The line sits under the user's request and above the agent's answer, so the
request is already on screen. You are given what the user asked, what the agent
was thinking, what it ran, and the names the last two steps were given.

Return the step's name as JSON.

- Two to five words, forty characters at most. It is a line in a list, not a
  sentence: it gets cut, and a cut name is a bad name.
- Name the action and the thing it acted on: "Tracing match() callers",
  "Compiling the router against tests", "Reading the worker's IPC".
- The calls are what actually happened; the thinking is the intent. When they
  disagree the calls win: a step that ran three greps is about searching,
  whatever the thinking said it was going to do.
- Keep the identifier that makes this step different from the one above it: a
  filename, a command, a function, an error. That is what the reader scans for.
- The step above this one, if named, is a step that already happened. Say what
  this one did that it did not: "Patching the wrap" under "Reading the wrap",
  not "Reading the wrap" again. If it really is the same work carrying on, name
  what moved on.
- Never count without saying what: "3 files" says nothing, "Sizing the three
  frontends" says what the step was for.
- Never quote the agent's own typography: no "====", no "foo -> bar", no
  backticks in the name. Plain words.
- No trailing period, no quotes, no code fences, no "the agent", no "let me".
- English, whatever language the work is in.

Good: "Adding segment matching to the router", "Checking why the log is in local
time", "Running the suite after the write".
Bad: "Reasoning about stuff", "Requoting ==== after interpolation", "3 files",
"Doing a read of the file", "Let me check the tests"."""

SCHEMA = {"properties": {
    "name": {"type": "string", "description": "The step's name, five words or fewer."}},
    "required": ["name"]}

# A thought, from both ends rather than only the top. A long reasoning trace opens
# by restating the request and closes on the decision it is about to act on, and the
# middle is where it worked that out. Cutting the head only kept the restatement and
# threw away the decision, which is the half that says what the step is for.
THOUGHT_HEAD = 300
THOUGHT_TAIL = 650
# How much of one argument, and how many calls, fit in a prompt that has to come
# back before the row it names is scrolled off.
ARG_WIDTH = 72
CALLS = 8

# The arguments that say what a call was pointed at, in the order they are worth
# saying. `action` last: it is the verb, and a verb without its object is exactly
# what made every file call read as "file(read)".
SAYS = ("path", "file", "command", "cmd", "pattern", "query", "url", "app",
        "name", "description", "goal", "question", "steps", "combo", "text",
        "action")
# A body: an old/new text, a whole file, a base64 image. It says nothing about
# what the step was for and is the largest thing in the call.
BODIES = ("old", "new", "edits", "content", "body", "data", "region", "zoom")

_lock = threading.Lock()
_busy = False
# The names the last few steps were given, newest last. A run of ten steps all
# called "Reading the router" is a list nobody can scan; the step above is the one
# thing that says what this step is *not* doing again.
_recent = deque(maxlen=2)


def clean(name: str) -> str:
    """One head line: no quotes, no trailing period, cut on a word boundary."""
    t = re.sub(r"\s+", " ", (name or "").strip()).strip("\"'`* ")
    t = t.rstrip(".")
    if len(t) > MAX_NAME:
        t = t[:MAX_NAME].rsplit(" ", 1)[0].rstrip(" ,;:-") + "\u2026"
    return t


def _said(key: str, value) -> str:
    """One argument as part of a line: a string as written, a list or object as
    JSON, either cut on a word so a long command still says its first words.

    Two shapes are shortened the way a person would say them. A path is its last
    two segments: the four directories above them are the same for every call in a
    session, and they were eating the whole width -- `…/quik/src/view.rs` became
    `view.rs`. A command loses its leading `cd <dir> && `: the worker prefixes
    every shell call with it, so the first sixty characters said nothing and the
    actual command was the part that got cut.
    """
    text = value if isinstance(value, str) else json.dumps(value, ensure_ascii=False)
    text = " ".join(text.split())
    if key in ("path", "file") and text.count("/") > 2:
        text = "/".join(text.rsplit("/", 2)[-2:])
    elif key in ("command", "cmd"):
        text = re.sub(r"^cd\s+\S+\s*&&\s*", "", text)
    return text if len(text) <= ARG_WIDTH else text[:ARG_WIDTH].rsplit(" ", 1)[0] + "\u2026"


def heads(calls: list) -> str:
    """What the step ran, one line each: the call, and what it was pointed at.

    No results: a result is kilobytes of file or stdout and says nothing about
    what the step was for. The call and its object is the whole signal, and it is
    a dozen tokens -- so the object is picked by name rather than taken from
    whatever argument happened to be short enough. Picking by position is what
    turned every long shell command into a bare `shell` and every edit into
    `file(edit)`: the identifier the name is supposed to keep was the one field
    dropped, and the step was named by a verb.
    """
    out = []
    for call in calls or []:
        if not isinstance(call, dict):
            continue
        name = call.get("name") or "?"
        args = call.get("args") if isinstance(call.get("args"), dict) else {}
        said = [f"{k}={_said(k, args[k])}" for k in SAYS
                if args.get(k) not in (None, "", [], {}, 0)]
        if not said:
            # Nothing in the shape we know: the longest string that is not a body.
            rest = [(k, v) for k, v in args.items()
                    if k not in BODIES and isinstance(v, str) and v]
            said = [_said(*max(rest, key=lambda kv: len(kv[1])))] if rest else []
        out.append(f"{name}({', '.join(said)})" if said else name)
    return "\n".join(out[:CALLS])


def excerpt(text: str) -> str:
    """A thought in the amount a name needs: its opening and its ending."""
    t = (text or "").strip()
    if len(t) <= THOUGHT_HEAD + THOUGHT_TAIL:
        return t
    return f"{t[:THOUGHT_HEAD]}\n[...]\n{t[-THOUGHT_TAIL:]}"


def prompt(asked: str, think: str, ran: str) -> str:
    """The whole of what a name is decided on: the request, the step above, the
    thought and the calls. One definition, because a second copy is how the thing
    the namer reads and the thing this file tests come apart."""
    above = "\n".join(_recent)
    return (f"What the user asked:\n{asked[:400] or '(not known)'}\n\n"
            f"The steps just above this one were named:\n{above or '(none)'}\n\n"
            f"What the agent was thinking:\n{think or '(nothing)'}\n\n"
            f"What it ran:\n{ran or '(nothing)'}")


def _work(send, asked: str, thought: str, ran: str, provider: str, model: str):
    """One naming, on its own thread. Never raises: a name is a nicety."""
    global _busy
    try:
        from handler.agent.subagent import AgentSpec, run as run_agent
        r = run_agent(AgentSpec(name="step-namer", tools=[], effort="low", max_rounds=1,
                                system=SYSTEM, schema=SCHEMA,
                                provider=provider, model=model or None),
                      prompt(asked, thought, ran))
        got = (r.get("output") or {}) if not r.get("error") else {}
        name = clean(str(got.get("name") or ""))
        if name:
            with _lock:
                _recent.append(name)
            send("token", {"state": "step_name", "token": name, "of": thought[:HINT]})
    except Exception:
        pass
    finally:
        with _lock:
            _busy = False


def maybe_name(send, thinking: str, calls: list, asked: str = ""):
    """Name a step that just closed, if anything is going to.

    `send` is the worker's own send: this module never imports it, so it can be
    tested without a pipe and cannot accidentally write to one twice.
    """
    global _busy
    thinking = (thinking or "").strip()
    if len(thinking) < MIN_THOUGHT:
        return False
    provider, model = config.steps_helper()
    if not provider:
        return False
    with _lock:
        if _busy:
            return False
        _busy = True
    threading.Thread(target=_work, daemon=True,
                     args=(send, asked, excerpt(thinking), heads(calls), provider, model)).start()
    return True
