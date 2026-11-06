#!/usr/bin/env python3
"""A skill named in a message typed into a running turn has to load.

The bug this pins: a message typed while a turn was running took the steer seam in
generate(), which built the note itself and never asked the skill loader. `$demo` in
that message read as plain prose, while the same words on a turn of their own loaded
the skill -- so the feature worked or not depending on whether something happened to
be generating.

The seam is the thing under test, not the loader: generate() is driven for real, with
a provider fake enough to answer two rounds, and what is asserted is the message the
provider is handed on the round after the typed words arrived.

    python3 tests/test_steer_skill.py
"""
import json
import os
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

# Before anything reads it: skills are found under CUACODE_HOME, and this test
# installs one where the loader looks rather than depending on what is on disk.
_home = tempfile.mkdtemp(prefix="cuacode-steer-skills-")
os.environ["CUACODE_HOME"] = _home
_skill = Path(_home) / "skills" / "demo"
_skill.mkdir(parents=True)
(_skill / "SKILL.md").write_text("---\nname: demo\ndescription: for the test\n---\n\n"
                                 "DEMO BODY: say the word before doing anything else.\n")

from handler.agent import main as agent_main
from handler.agent import providers as registry

FAILS = []


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond and detail else ""))
    if not cond: FAILS.append(name)


class Delta:
    """One streamed frame: a reply to say nothing, or a call to keep the loop going."""

    def __init__(self, calls=(), content=""):
        self.raw = {}
        self.thinking = ""
        self.content = content
        self.tool_calls = list(calls)


class ToolCall:
    def __init__(self, id, name, args):
        self.id, self.name, self.args = id, name, args


class FakeStream:
    """An iterable with close(), which the loop calls on the way out."""

    def __init__(self, items): self.items = list(items)
    def __iter__(self): return iter(self.items)
    def close(self): pass


class FakeProvider:
    """Just enough of the provider interface for generate()."""
    name = "openai"                # the dialect, so translation and effort both resolve
    default_model = "fake-1"
    vision = False

    def setup(self, key=None): pass
    def user_message(self, text, images=None): return {"role": "user", "content": text}
    def assistant_message(self, thinking, content, calls):
        return {"role": "assistant", "content": content,
                "tool_calls": [{"id": c.id, "type": "function",
                                "function": {"name": c.name, "arguments": json.dumps(c.args)}}
                               for c in calls]}
    def result_messages(self, pairs):
        return [{"role": "tool", "tool_call_id": c.id, "content": json.dumps(r)} for c, r in pairs]
    def native_calls(self, calls): return calls
    # Deltas carry complete calls by contract, so there is nothing to parse here.
    def parse_calls(self, calls): return calls
    def context_window(self, model): return 0


registry.PROVIDERS["openai"] = FakeProvider()


def drive(typed):
    """One turn: a round that calls a tool (the seam), then a round that ends it.

    Returns every event and the message list the provider was handed, so the
    assertions read what the model actually saw rather than what was intended.
    """
    events, seen, rounds = [], [], {"n": 0}

    def fake_opened(p, provider, model, msgs, tools_, system, params):
        seen.append(json.loads(json.dumps(msgs)))
        rounds["n"] += 1
        if rounds["n"] == 1:
            return ("ok", (FakeStream([Delta(calls=[ToolCall("c1", "nosuchtool", {})])]), None))
        return ("ok", (FakeStream([]), None))

    real = agent_main._opened
    agent_main._opened = fake_opened
    try:
        for chunk in agent_main.generate(
                messages=[{"role": "user", "content": "start"}],
                settings={"provider": "openai", "model": "fake-1"},
                system="you are a test", cancelled=lambda: False,
                provider_obj=FakeProvider(),
                # What the reader thread would have collected while the round ran.
                steer=lambda: typed):
            events.append(chunk)
    finally:
        agent_main._opened = real
    return events, seen


def said(msgs):
    """Every user message the provider was handed, as text."""
    return "\n\n".join(m.get("content") or "" for m in msgs if m.get("role") == "user")


# 1. `$name` typed mid-turn loads the skill, and rides the same message as the words:
#    a second user message in a row is a 400 on more than one provider.
events, seen = drive([{"text": "meanwhile, $demo please", "images": []}])
sent = said(seen[-1])
check("the typed words reach the model", "meanwhile, $demo please" in sent)
check("the skill body loads beside them", "DEMO BODY" in sent)
check("both halves are one message",
      len([m for m in seen[-1] if "DEMO BODY" in json.dumps(m)]) == 1
      and any("meanwhile" in json.dumps(m) and "DEMO BODY" in json.dumps(m) for m in seen[-1]))
check("the frontend is told the skill loaded",
      any("DEMO BODY" in (e.get("show") or "") for e in events if e.get("type") == "notice"))

# 2. The opening-slash spelling works the same way; it is what older frontends write.
_, seen = drive([{"text": "/demo and now the other thing", "images": []}])
check("/name mid-turn loads too", "DEMO BODY" in said(seen[-1]))

# 3. A message naming nothing costs nothing: no block, and the words still arrive.
_, seen = drive([{"text": "no skill named here", "images": []}])
check("a message with no skill stays prose",
      "no skill named here" in said(seen[-1]) and "DEMO BODY" not in said(seen[-1]))

# 4. A name that is not installed is prose too, not a failed turn.
_, seen = drive([{"text": "$nosuchskill hello", "images": []}])
check("an unknown $name does nothing", "$nosuchskill hello" in said(seen[-1])
      and "skill loaded" not in said(seen[-1]))

print()
if FAILS:
    print("FAILED:", ", ".join(FAILS))
    sys.exit(1)
print("all checks passed")
