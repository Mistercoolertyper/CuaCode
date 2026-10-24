#!/usr/bin/env python3
"""What the usage report promises, checked against the shapes the providers actually send.

The bug this pins: a round's cached input never reached the report, so "billed input"
was every prompt at full price and the prompt cache looked like it did not exist. Three
gates dropped it, one per provider SDK, and all three are checked here along with the
store and the report that read nothing of it.

    python3 tests/test_usage.py
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from handler.agent.providers.base import usage_of
from handler import usage as u

FAILS = []


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond and detail else ""))
    if not cond: FAILS.append(name)


# 1. one round's counts, in each dialect's own spelling. ollama.com runs over the openai
#    schema, so its frame is the openai one below; the ollama SDK's own counters are gone
#    with the native dialect.
check("openai frames", usage_of({"usage": {"prompt_tokens": 16076, "completion_tokens": 5,
                                           "prompt_tokens_details": {"cached_tokens": 15946}}})
      == {"input": 16076, "output": 5, "cached": 15946})
check("anthropic frames", usage_of({"message": {"usage": {"input_tokens": 130, "output_tokens": 5,
                                                         "cache_read_input_tokens": 15946}}})
      == {"input": 16076, "output": 5, "cached": 15946})

# 3. a round stores it, a session adds it up, the report prints it.
round_cost = u.stamp({"in_tokens": 16076, "out_tokens": 5, "cached_tokens": 15946,
                      "thinking_tokens": 3, "gen_secs": 2.0}, "ollama", "deepseek-v4.1-flash")
check("a round stamps the cache count", round_cost.get("cache") == 15946, round_cost)
rec = {"t": "assistant", "ts": "2026-10-08T20:00:00+00:00", "u": round_cost}
session = u.of_records([rec, rec])
check("a session totals it", (session.get("in"), session.get("cache")) == (32152, 31892), session)

rep = {"sessions": 1, "unmeasured": 0, "since": "2026-10-08", "total": {**u._blank(), **session},
       "models": [{"name": "ollama deepseek-v4.1-flash", **session}], "days": [], "top": []}
text = u.render(rep)
check("the report shows the cached share", "cached input" in text and "99% of the prompt" in text, text)
check("and repeats it per model", "(99% cached)" in text, text)

# Rounds recorded before the count existed say nothing rather than claiming none were cached.
old = u.render({**rep, "total": {**u._blank(), "in": 100, "out": 1, "rounds": 1}, "models": []})
check("an unmeasured round claims no cache either way", "cached" not in old, old)

# 4. A round another loop ran -- a subagent, a page digest, the permission gate -- is billed
#    the same and used to leave no trace: generate() yields its usage chunk and nothing
#    downstream of subagent._drive kept it.
side = u.record("digest", {"type": "usage", "usage": {"input": 8000, "output": 50, "cached": 7000},
                          "model": "deepseek-v4.1-flash", "secs": 1.5,
                          "thinking_chars": 40, "reply_chars": 60}, "ollama")
check("a side round stamps like any other", (side.get("in"), side.get("out"), side.get("src")) == (8000, 50, "digest"), side)

# Its own thread, so it lands in the queue the loop drains rather than on a session directly.
class Fake:
    def __init__(self): self.got = []
    def add_spend(self, cost): self.got.append(cost)
handed = Fake()
check("the loop takes what it queued", u.absorb(handed) == 1 and handed.got[0]["src"] == "digest", handed.got)

rec = {"t": "usage", "ts": "2026-10-08T20:00:00+00:00", "u": side}
both = u.of_records([{"t": "assistant", "ts": "2026-10-08T20:00:00+00:00", "u": round_cost}, rec])
check("a session counts it", both["rounds"] == 2 and both["in"] == 24076, both)
check("under what ran it", both["srcs"]["digest"]["in"] == 8000, both.get("srcs"))
# Its prompt is its own window: letting it move the peak would claim a window this
# conversation never held.
check("and not as this conversation's context", both["peak"] == 16076, both)
both_text = u.render({**rep, "total": {**u._blank(), **both}})
check("the report says where the side rounds went", "side calls" in both_text and "digest" in both_text, both_text)

# Rollups merge them, or a day-filtered answer would lose what the fan-out cost.
rolled = u.rollup(metas=[{"id": "a", "usage": both}])
check("a rollup of many sessions keeps them", rolled["total"]["srcs"]["digest"]["rounds"] == 1, rolled["total"])

print()
if FAILS:
    print(f"{len(FAILS)} failed: " + ", ".join(FAILS))
    sys.exit(1)
print("all checks passed")
