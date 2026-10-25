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
import re
import threading

from handler import config

# A head line, not a sentence. Long enough for a filename, short enough that a
# step is still one line.
MAX_NAME = 46
# The handle the frontend matches on, and enough of the thought to say what it is.
HINT = 64
# Below this there is nothing to name: "hm" names itself.
MIN_THOUGHT = 24

SYSTEM = """You name the steps of an AI coding agent's work, for a one-line-per-step
display. You are given one step: what the agent was thinking, and the tool calls
it made.

Return a name for the step as JSON.

- Three to six words. Shorter is better.
- Say what the step is doing, not that a step happened. "Checking the router's
  tests", not "step where the agent checks something".
- Start from the work, not the speaker: no "the agent", no "I", no "let me".
- Keep the concrete identifier when there is one: a filename, a command, an error.
  That is what makes one step in forty findable.
- No trailing period, no quotes, no code fences.
- English, whatever language the work is in."""

SCHEMA = {"properties": {
    "name": {"type": "string", "description": "The step's name, six words or fewer."}},
    "required": ["name"]}

_lock = threading.Lock()
_busy = False


def clean(name: str) -> str:
    """One head line: no quotes, no trailing period, cut on a word boundary."""
    t = re.sub(r"\s+", " ", (name or "").strip()).strip("\"'`* ")
    t = t.rstrip(".")
    if len(t) > MAX_NAME:
        t = t[:MAX_NAME].rsplit(" ", 1)[0].rstrip(" ,;:-") + "\u2026"
    return t


def heads(calls: list) -> str:
    """What the step ran, one line each, as the call's own name and argument.

    No results: a result is kilobytes of file or stdout and says nothing about
    what the step was for. The call and what it was pointed at is the whole
    signal, and it is a dozen tokens.
    """
    out = []
    for call in calls or []:
        if not isinstance(call, dict):
            continue
        name = call.get("name") or "?"
        args = call.get("args") if isinstance(call.get("args"), dict) else {}
        hint = next((str(v) for v in args.values()
                     if isinstance(v, str) and v and len(v) < 80), "")
        out.append(f"{name}({hint})" if hint else name)
    return "\n".join(out[:8])


def _work(send, thought: str, ran: str, provider: str, model: str):
    """One naming, on its own thread. Never raises: a name is a nicety."""
    global _busy
    try:
        from handler.agent.subagent import AgentSpec, run as run_agent
        prompt = f"Thinking:\n{thought[:900]}\n\nCalls:\n{ran or '(none)'}"
        r = run_agent(AgentSpec(name="step-namer", tools=[], effort="low", max_rounds=1,
                                system=SYSTEM, schema=SCHEMA,
                                provider=provider, model=model or None), prompt)
        got = (r.get("output") or {}) if not r.get("error") else {}
        name = clean(str(got.get("name") or ""))
        if name:
            send("token", {"state": "step_name", "token": name, "of": thought[:HINT]})
    except Exception:
        pass
    finally:
        with _lock:
            _busy = False


def maybe_name(send, thinking: str, calls: list):
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
                     args=(send, thinking, heads(calls), provider, model)).start()
    return True
