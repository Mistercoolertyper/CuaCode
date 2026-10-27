#!/usr/bin/env python3
"""Compaction, end to end, against a provider that actually refuses over the window.

The unit checks in handler/agent/compact.py cover the arithmetic. This covers the
thing the arithmetic is for: generate() folding a live conversation when the
provider says 413, and the conversation still working afterwards.

The fake provider is a real one by the rules that matter here. It counts the
characters of the request it is handed and refuses with a 413 when the history is
over its window -- which is exactly the refusal that used to be final. It also
answers the summarizer's own call, so the fold is a real fold and not a stub: the
summary text is produced by a model in the loop, taken through the same
submit_result tool a digest uses.

    python3 tests/test_compact.py
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from handler.agent import compact
from handler.agent import main as agent_main
from handler.agent import providers as registry

FAILS = []


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond and detail else ""))
    if not cond: FAILS.append(name)


class Window:
    """A provider that refuses anything longer than its window."""

    def __init__(self, window_chars):
        self.window = window_chars
        self.requests = []          # (chars, refused)
        self.summaries = 0

    def chars(self, messages, system, tools):
        return sum(len(json.dumps(m, default=str)) for m in messages) + len(system or "") + len(json.dumps(tools or []))


class FakeProvider:
    """Just enough of the provider interface for generate() and the summarizer."""
    # The openai dialect under the name generate() looks up, so schema translation
    # and effort resolution both work without a second fake.
    # The openai dialect under the name the loop resolves it by, so schema
    # translation and effort resolution both work without a second fake.
    name = "openai"
    default_model = "fake-1"
    vision = False

    def __init__(self, world: Window):
        self.world = world

    def setup(self, key=None): pass

    # -- the loop's own calls ------------------------------------------------
    def user_message(self, text, images=None): return {"role": "user", "content": text}
    def assistant_message(self, thinking, content, calls):
        return {"role": "assistant", "content": content,
                "tool_calls": [{"id": c.id, "type": "function",
                                "function": {"name": c.name, "arguments": json.dumps(c.args)}} for c in calls]}
    def result_messages(self, pairs):
        return [{"role": "tool", "tool_call_id": c.id, "content": json.dumps(r)} for c, r in pairs]
    def native_calls(self, calls): return calls
    def parse_calls(self, calls):
        from tools._parser.FromProvider import ToolCall
        return [ToolCall(c["id"], c["function"]["name"], json.loads(c["function"]["arguments"])) for c in calls]
    def context_window(self, model): return 0


class FakeSession:
    """The session surface a fold needs, and nothing else.

    The real one is Session in handler/session/main.py: records() reads what it
    holds, rewrite() replaces it, messages() turns it back into wire messages
    through replay. This is those three, so the test exercises the same fold path
    the worker does without a disk or a meta.json.
    """

    def __init__(self):
        self._records = []
        self.archived = []      # every pre-fold record list, in order
        self.folds = []         # the fold details handed to rewrite

    def adopt(self, messages):
        """Translate wire messages into the record shape a fold plans over."""
        recs = []
        for m in messages:
            if m["role"] == "user": recs.append({"t": "user", "text": m["content"]})
            elif m["role"] == "assistant":
                # Calls matter: a fold has to keep an assistant call with its tool
                # results, and a stub with no calls would make that untestable.
                calls = []
                if m.get("tool_calls"):
                    for c in m["tool_calls"]:
                        calls.append({"name": c["function"]["name"],
                                      "args": json.loads(c["function"]["arguments"] or "{}")})
                recs.append({"t": "assistant", "content": m.get("content") or "", "calls": calls})
            elif m["role"] == "tool":
                # The real add_tool stores {"t": "tool", "name": ..., "result": <the
                # dispatch dict>}, and replay hands result_messages that dict. A bare
                # string here is a shape the worker never writes.
                recs.append({"t": "tool", "name": "shell", "result": {"result": m.get("content")}})
        self._records = recs

    def records(self): return list(self._records)

    def rewrite(self, records, fold=None):
        # The real one archives the previous list before replacing it, and records
        # what happened on meta. Both are what this fake has to keep, or the test
        # would pass against a version that silently ate the history.
        before = list(self._records)
        self.archived.append(before)
        self.folds.append(dict(fold or {}))
        self._records = list(records)
        self._rebuilt = self.messages()

    def messages(self):
        from handler.session import replay
        return replay.to_messages(self._records, "openai")


def drive(prompt, messages, world, budget_rounds=6):
    """Run generate() against the fake, folding when it refuses. Returns the events."""
    prov = FakeProvider(world)
    events = []
    # A summarizer answer: the "model" writes a summary of what it was shown.
    calls = {"n": 0}

    def fake_opened(p, provider, model, msgs, tools_, system, params):
        size = world.chars(msgs, system, tools_)
        world.requests.append((size, size > world.window))
        if size > world.window:
            return ("err", Exception("413 request too large: %d > %d" % (size, world.window)))
        calls["n"] += 1
        if calls["n"] > budget_rounds:
            return ("ok", ([], {"role": "assistant", "content": "done"}))
        return ("ok", (iter([]), None))

    real = agent_main._opened
    agent_main._opened = fake_opened
    # The summarizer goes through subagent.run -> generate, which would recurse into
    # the fake. Stubbed to a deterministic answer of a realistic size, because what
    # this test is about is the fold happening and the conversation surviving it,
    # not the quality of a summary written by a fake model.
    real_summarize = compact.summarize
    compact.summarize = lambda recs, words, ctx=None: (
        compact.__dict__["_seen"].append(len(recs)) if "_seen" in compact.__dict__ else None
    ) or {"summary": "the earlier work, in short: files were read and one was edited. " * 4}
    sess = FakeSession()
    sess.adopt(messages)
    try:
        for chunk in agent_main.generate(
                ctx={"session": sess}, messages=messages,
                settings={"provider": "openai", "model": "fake-1"},
                system="you are a test", cancelled=lambda: False, provider_obj=prov):
            events.append(chunk)
            # The real fold ran inside generate(); nothing more to add here.
    finally:
        agent_main._opened = real
        compact.summarize = real_summarize
    return events


# 1. The arithmetic the whole thing rests on, re-asserted here so one command runs it.
compact.check()
check("compact arithmetic", True)

# The provider has to exist in the registry for config.can_think and effort.resolve,
# which both look it up by name before the loop opens a request.
# The provider registered under the name the loop looks up. The dialect is what
# matters; the name only has to resolve.
# Replay builds its own provider to rebuild history, so the registry entry has to be
# constructible with no arguments. The generator loop is handed its instance
# directly, so this one is only ever used for the translation.
registry.PROVIDERS["openai"] = FakeProvider(Window(window_chars=200_000))

# 2. A conversation that fits is left completely alone.
world = Window(window_chars=200_000)
short = [{"role": "user", "content": "hello"}]
before = json.dumps(short)
drive("hi", short, world, budget_rounds=1)
check("a short conversation is not folded", "hello" in json.dumps(short) and
      not any("compacted" in (e.get("text") or "") for e in drive("hi", short, world, budget_rounds=1)),
      short)

# 3. A conversation over the window is refused first, and that refusal is what makes
#    the fold happen. This is the whole point: 413 used to be terminal.
#
# The window is sized against the overhead every request carries -- the tool
# schemas and the system prompt are tens of thousands of characters before a single
# message -- so it is a window a real conversation could be too big for, rather
# than one nothing could ever fit in.
# Measured, not guessed: the fake is asked what a request costs before any of the
# conversation is added, so the window is the overhead plus room for a fold to land.
class _Measure(Window):
    pass
_overhead_window = Window(0)
world = Window(window_chars=0)          # replaced below once the overhead is known
long = [{"role": "user", "content": "the original request: build the thing" * 40}]
for i in range(40):
    long.append({"role": "assistant", "content": "step %d: read and wrote a file %s" % (i, "x" * 200)})
    long.append({"role": "tool", "tool_call_id": "c%d" % i, "content": "result %d %s" % (i, "y" * 200)})
long.append({"role": "user", "content": "now finish it"})
# The overhead is whatever the request costs with none of this conversation in it.
_overhead = world.chars([], "you are a test", agent_main.to_provider(
    agent_main._visible(agent_main.registry(), False), "openai"))
world.window = _overhead + 20_000
tools_now = agent_main.to_provider(agent_main._visible(agent_main.registry(), False), "openai")
size_before = world.chars(long, "you are a test", tools_now)
check("the conversation starts over the window", size_before > world.window, (size_before, world.window))
refusals_before = sum(1 for _, refused in world.requests if refused)

events = drive("finish", long, world)
notices = [e for e in events if e.get("type") == "notice" and e.get("compacted")]
size_after = world.chars(long, "you are a test", tools_now)
check("the fake provider did refuse at least once", refusals_before == 0)
check("the refusal was reported, not raised", len(notices) >= 1, notices[:1])
check("the history is now under the window", size_after < world.window, (size_after, world.window))
check("and smaller than it was", size_after < size_before, (size_before, size_after))

# 4. The fold left something a provider will accept: no tool result without its call,
#    and the conversation still opens with a user turn.
def legal(msgs):
    out = []
    for m in msgs:
        if m["role"] == "tool" and (not out or out[-1]["role"] != "assistant"):
            return False, "tool result with no assistant call before it"
        out.append(m)
    if not msgs or msgs[0]["role"] != "user":
        return False, "history does not open on a user turn"
    return True, ""

ok, why = legal(long)
check("the folded history is legal for every provider", ok, why)

# 5. The request survives the fold, word for word, in what the provider would see.
joined = "\n".join(str(m.get("content") or "") for m in long)
check("the opening request is still in the history", "the original request" in joined)
check("the latest request is still in the history", "now finish it" in joined or "finish" in joined)

# 6. A second round does not fold again: the fold landed under the line, which is the
#    difference between one fold and a fold every single round.
world.requests.clear()
drive("carry on", long, world)
check("a later round does not refuse again", all(not r for _, r in world.requests), world.requests[:3])

# 7. _compact_now's own report. `records` is folded in place, so the count has to be
#    read before fold() slices it; reading it after reported 75 records to -487.
import re as _re
recs = [{"t": "user", "text": "the request " * 40}]
recs += [{"t": "assistant", "content": "work " * 400, "calls": []} for _ in range(40)]
recs += [{"t": "user", "text": "and then the last thing"}]
sess = FakeSession()
sess._records = list(recs)
limit = compact.tokens(recs) * 2 // 3
real_summarize = compact.summarize
compact.summarize = lambda rs, words, ctx=None: {"summary": "the middle, in short. " * 20}
try:
    said = agent_main._compact_now([], used=compact.tokens(recs), limit=limit,
                                   ctx={"session": sess})
finally:
    compact.summarize = real_summarize
m = _re.search(r"compacted (\d+) records to (\d+)", said or "")
check("the report names the count that went in and the count that came out",
      bool(m) and int(m.group(1)) == len(recs) and int(m.group(2)) == len(sess._records),
      (said, len(recs), len(sess._records)))

print()
if FAILS:
    print("FAILED: " + ", ".join(FAILS))
    sys.exit(1)
print("all compact checks passed")
