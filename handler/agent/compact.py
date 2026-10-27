"""Folding a long conversation down before the window runs out.

The prompt is re-sent every round, so a conversation that grows past the window
does not degrade, it stops: the provider answers 413 and the loop treats that as
final. This keeps the head, keeps the tail, and replaces the middle with a summary
written by a small model.

## Where the trigger sits

A ratio, not a number, because the same conversation belongs in a 256k window and a
1M one. But a single ratio cannot serve both. At 70% of 1M there is 300k of slack,
which means every round re-sends half a million cached tokens before anything is
folded. So the ratio steps down as the window grows:

    256k or under  -> 70%      (179k of room left, one more round of tool output)
    1M or over     -> 40%      (600k of room left, same headroom, bigger window)
    linear between, clamped outside

Chosen, not tuned. `compact` in config.json overrides it, and `compact: 0` turns it
off. /compress sets that key.

## What folding down to

Folding to exactly the trigger is a bug with a delay on it: the next round is over
the line again and folds again. So a fold targets FOLD_TO of the trigger, which
leaves real room to work in.

## What survives

- The head, verbatim. The opening of a conversation is where the request that
  framed everything lives.
- The tail, verbatim. Recent work in progress, and the tool results the next
  decision is read off.
- The current request, verbatim, even when it is folded. A long single turn puts
  the user's message at the beginning and then hundreds of rounds of tool work
  after it; refusing to fold past it would make compaction useless exactly when it
  is needed, and folding it away without replacement would lose the thing the
  whole turn is for. So the last user turn inside the folded range is carried on
  the summary record as `request` and replayed word for word after it.

## What a fold may never do

Cut a tool result away from the assistant call that produced it. A tool message
with no call in front of it is a 400 on every provider. So the boundary is moved
backwards, past whole assistant/tool groups, until the kept side starts on
something that can legally open a round.

The plan is pure -- records in, two numbers in, cut points out -- because the
arithmetic is the part that breaks, and it is testable without a provider.
"""
import json

from handler import context
from handler.session import store

HEAD_CHARS = 6000          # the opening, kept verbatim, until the target is small
HEAD_SHARE = 0.10          # of the target, when that is the smaller of the two
HEAD_MIN = 1500
TAIL_SHARE = 0.30          # of the target, kept verbatim
TAIL_MIN = 3000
FOLD_TO = 0.60             # of the trigger: fold down to here, not to the line
MIN_FOLD_RECORDS = 3       # a fold of two records is not worth a model call
MIN_SAVING_CHARS = 4000    # nor one that frees almost nothing

SUMMARY_PROMPT = """You compress a working conversation so an agent can carry on inside a smaller window.

You are given the middle of a session. Everything before it and after it is kept
verbatim and you will not see it, so write only what belongs in the gap.

Write the replacement so the agent can continue as if it had read the original:

- What was asked for, in the user's own terms, when that is still live.
- What was decided and why, including options rejected and the reason.
- What was built or changed, by path. Exact file paths, function names, commands,
  identifiers, versions and numbers, verbatim. Paraphrase loses the thing that
  makes the note useful.
- What is still open, broken, or unverified. This is the part an agent most needs
  and most often loses.
- Anything that would be actively wrong to repeat: a failed approach, a dead end,
  a command that does not work on this machine.

Rules:
- No preamble. No "this conversation covered". Start with the content.
- Facts, not narration. "port 8080 is taken by an old dev server" beats "we
  discussed the port situation".
- Never invent a detail to fill a gap. If the middle does not say, leave it out.
- Keep it under {budget} words. Shorter is better when nothing is lost.
- Write in the language the conversation is in."""

SUMMARY_SCHEMA = {"properties": {
    "summary": {"type": "string",
                "description": "The replacement for the folded middle. Facts, paths, decisions, open items."}},
    "required": ["summary"]}


def ratio(window: int) -> float:
    """Where in the window this size starts folding. 0 when it never should."""
    if window <= 0: return 0.0
    if window <= 256_000: return 0.70
    if window >= 1_000_000: return 0.40
    return 0.70 - 0.30 * ((window - 256_000) / (1_000_000 - 256_000))


def trigger(settings: dict, cfg: dict = None) -> int:
    """The token count at which this conversation folds, or 0 to never.

    `compact` in config.json wins when it is a number, and wins with 0 by turning
    folding off. Read before the window, because a count is a count: a provider
    that will not say how big its window is can still be given one, and a
    conversation with a written-down limit folds on it -- the alternative is that
    an unrecognized model has no way to fold on purpose before it hits the wall.
    A window nobody knows with no override means no trigger: guessing the size of
    the window is guessing when to throw away the user's context.
    """
    from handler import config
    override = (cfg if cfg is not None else config.load()).get("compact")
    fixed = isinstance(override, (int, float)) and not isinstance(override, bool)
    if fixed and override <= 0: return 0
    if fixed and override > 1: return int(override)      # a token count
    window = config.context_window(settings.get("provider", ""), settings.get("model", ""), cfg)
    if not window: return 0
    r = override if fixed else ratio(window)            # a share of the window
    return int(window * r)


def _chars(rec: dict) -> int:
    return len(json.dumps(rec, default=str))


def _safe_cut(records: list, cut: int) -> int:
    """Move a cut point backwards until the kept side can open a round.

    records[:cut] is folded and records[cut:] is kept, so what has to be true is
    about records[cut], not records[cut - 1]: if it is a tool result, the assistant
    call that produced it is on the folded side and the kept side opens with a tool
    message, which every provider rejects.
    """
    while 0 < cut < len(records) and records[cut].get("t") == "tool":
        cut -= 1
    return cut


def _safe_start(records: list, start: int, end: int) -> int:
    """Move the fold's opening forward until the *folded* side is a legal round too.

    The other half of the same problem, and the one that is easy to miss. If the
    fold begins on a tool record, the assistant call that produced it is folded away
    while its result is kept -- an orphaned result, which is illegal in the other
    direction. So the fold may not start on a tool result; it moves forward to the
    end of that group, and then back to a legal point.
    """
    while start < end and records[start].get("t") == "tool":
        start += 1
    # Now it may sit on an assistant whose calls are all folded with it, which is
    # fine, or on a user turn, which is the cleanest place. Never past end.
    return min(start, end)


def _ends(target_tokens: int) -> tuple[int, int]:
    """How many characters the head and the tail may keep, for a fold targeting this size.

    Both scale with the target, because keeping a fixed 6k head and 4k tail is a
    fold that cannot land under a 20k trigger: the two ends alone are most of the
    window and the next round folds again immediately. The mins still apply to a
    big target, where they are what stops a fold throwing away the opening of the
    conversation to save a few hundred characters.
    """
    room = max(target_tokens, 1) * context.CHARS_PER_TOKEN
    head = max(HEAD_MIN, min(HEAD_CHARS, int(room * HEAD_SHARE)))
    tail = max(TAIL_MIN, int(room * TAIL_SHARE))
    # The guarantee, which outranks the floors: after the fold the head, the tail
    # and a summary have to fit in the target with room to spare, or this fold is
    # only a prelude to the next one.
    budget = int(room * 0.80)
    if head + tail > budget:
        scale = budget / (head + tail)
        head, tail = int(head * scale), int(tail * scale)
    return head, tail


def plan(records: list, used: int, limit: int) -> dict | None:
    """What to fold, or None. Pure: records and two token counts in, a plan out.

    `limit` is the trigger. The fold targets FOLD_TO of it, so the next round is
    not over the line again the moment this one lands.
    """
    if limit <= 0 or used <= limit: return None
    target = int(limit * FOLD_TO)
    head, tail = _ends(target)

    # Head: whole records until the head budget is covered.
    taken, start = 0, 0
    while start < len(records) and taken < head:
        taken += _chars(records[start]); start += 1

    # Tail: whole records from the end until the tail budget is covered.
    taken, end = 0, len(records)
    while end > start and taken < tail:
        end -= 1; taken += _chars(records[end])
    end = _safe_cut(records, end)
    # Both edges, or the fold creates the exact shape it is supposed to avoid: an
    # assistant call on one side of the cut and its result on the other.
    start = _safe_start(records, start, end)

    if end - start < MIN_FOLD_RECORDS: return None
    freed = sum(_chars(r) for r in records[start:end])
    if freed < MIN_SAVING_CHARS: return None
    # The request still in force, if the fold swallowed it. Carried verbatim on
    # the summary so a turn's own opening survives being folded.
    request = ""
    for r in reversed(records[start:end]):
        if r.get("t") == "user" and (r.get("text") or "").strip():
            request = r["text"]; break
    return {"start": start, "end": end, "freed": freed, "folded": end - start,
            "request": request, "words": max(120, freed // 16),
            "target": target, "head": head, "tail": tail}


def _render(records: list) -> str:
    """The folded middle as text for the summarizer. Records, not wire messages:
    they carry results and names the dialect shape drops."""
    out = []
    for r in records:
        t = r.get("t")
        if t == "user": out.append("USER: " + (r.get("text") or ""))
        elif t == "assistant":
            said = (r.get("content") or "").strip()
            calls = r.get("calls") or []
            if said: out.append("AGENT: " + said)
            for c in calls:
                out.append("  CALL %s %s" % (c.get("name"), json.dumps(c.get("args") or {}, default=str)[:800]))
        elif t == "tool":
            res = json.dumps(r.get("result") or {}, default=str)
            out.append("  RESULT %s: %s" % (r.get("name"), res[:1200]))
        elif t == "compact":
            out.append("EARLIER SUMMARY: " + (r.get("text") or ""))
    return "\n".join(out)


def summarize(records: list, words: int, ctx=None) -> dict:
    """One small model call over the folded middle. {summary} or {error}."""
    from handler.agent.subagent import AgentSpec, run as run_agent
    spec = AgentSpec(name="compactor", tools=[], effort="low", max_rounds=2,
                     system=SUMMARY_PROMPT.replace("{budget}", str(words)),
                     schema=SUMMARY_SCHEMA)
    # Capped: the summarizer's own window is smaller than the conversation's, and a
    # fold that cannot be read is worse than no fold.
    out = run_agent(spec, _render(records)[:300_000], ctx=ctx)
    if "error" in out: return {"error": out["error"]}
    text = ((out.get("output") or {}).get("summary") or "").strip()
    return {"summary": text} if text else {"error": "the summarizer returned nothing"}


def fold(records: list, steps: dict) -> list:
    """records with the planned slice replaced by one compact record. In place."""
    rec = {"t": "compact", "ts": store.now_iso(), "text": steps["summary"],
           "folded": steps["folded"], "freed": steps["freed"]}
    if steps.get("request"): rec["request"] = steps["request"]
    records[steps["start"]:steps["end"]] = [rec]
    return records


def tokens(records: list) -> int:
    """What this costs in the window. Records or wire messages, either will do:
    both are json, and the difference between the two shapes is a few hundred
    characters of key names against a window measured in hundreds of thousands.
    """
    return context.tokens_for_chars(sum(len(json.dumps(r, default=str)) for r in records))


def check():
    """The arithmetic that breaks, none of it needing a provider."""
    # The curve: exact at the ends, monotone in between, clamped outside.
    assert ratio(256_000) == 0.70
    assert ratio(1_000_000) == 0.40
    assert ratio(0) == 0.0
    assert ratio(100_000) == 0.70 and ratio(2_000_000) == 0.40
    assert 0.40 < ratio(628_000) < 0.70

    # A written-down count is honoured even when the provider will not say how big
    # its window is, and 0 still means never. Neither branch may reach the
    # provider, which is why a name no registry entry has is the test case.
    unknown = {"provider": "no-such-provider", "model": "unknown-1"}
    assert trigger(unknown, {"compact": 400_000}) == 400_000
    assert trigger(unknown, {"compact": 0}) == 0
    assert trigger(unknown, {"compact": 0.4}) == 0        # a share of nothing
    assert trigger(unknown, {}) == 0
    prev = 1.0
    for w in (50_000, 256_000, 400_000, 700_000, 1_000_000, 3_000_000):
        r = ratio(w) or 1.0
        assert r <= prev + 1e-9, (w, r, prev)
        prev = r

    # A cut never leaves the kept side starting on a tool result.
    recs = [{"t": "assistant", "content": "", "calls": [{"name": "shell", "args": {}}]},
            {"t": "tool", "name": "shell", "result": {}},
            {"t": "user", "text": "next"}]
    assert _safe_cut(recs, 1) == 0, _safe_cut(recs, 1)
    # Cutting at 2 folds the assistant and its tool result together and keeps the
    # user turn: a legal pair on both sides, so it stands.
    assert _safe_cut(recs, 2) == 2
    assert _safe_cut(recs, 0) == 0
    assert _safe_cut(recs, 3) == 3           # end of list is always safe

    # Under the trigger: nothing planned.
    assert plan(recs, 10, 100) is None
    # Over it, but nothing worth folding: still nothing.
    assert plan([{"t": "user", "text": "hi"}], 500, 100) is None

    # A real fold, sized like a real conversation rather than like a toy: records
    # of a few hundred characters and a trigger in the thousands.
    big = ([{"t": "user", "text": "the request " * 60}] +
           [{"t": "assistant", "content": "work " * 400, "calls": []} for _ in range(200)] +
           [{"t": "user", "text": "now do the next thing"}])
    limit = tokens(big) * 2 // 3
    before = tokens(big)
    assert before > limit
    p = plan(big, used=before, limit=limit)
    assert p, "expected a plan at this size"
    after = before - context.tokens_for_chars(p["freed"]) + context.tokens_for_chars(400)
    assert p["start"] > 0 and p["end"] <= len(big), p
    # Kept side opens on something legal.
    assert big[p["end"]].get("t") != "tool", p
    # It folds below the trigger rather than down to it, or the next round folds again.
    assert after < limit * FOLD_TO, (after, limit, p)

    # A later request that the fold swallows is carried on the record, not lost.
    # Pattern of a long turn: an opening, then a second request, then the work it
    # set off, then the tail. The head keeps the opening; the second request is in
    # the middle and has to come back with the summary.
    turn = ([{"t": "user", "text": "opening ask" * 40}] +
            [{"t": "assistant", "content": "filler " * 300, "calls": []} for _ in range(60)] +
            [{"t": "user", "text": "SECOND REQUEST: rename the field"}] +
            [{"t": "assistant", "content": "working " * 300, "calls": []} for _ in range(60)] +
            [{"t": "assistant", "content": "latest work", "calls": []}])
    q = plan(turn, used=tokens(turn), limit=tokens(turn) * 2 // 3)
    assert q, "expected a plan"
    assert q["request"].startswith("SECOND REQUEST"), q["request"][:40]

    # Folding is in place and shrinks the list by exactly what it replaced.
    n = len(big)
    steps = dict(p, summary="a summary")
    fold(big, steps)
    assert len(big) == n - p["folded"] + 1, (len(big), n, p["folded"])
    assert big[p["start"]].get("t") == "compact"

    # The carried request survives the fold itself, not just the plan.
    n2 = len(turn)
    fold(turn, dict(q, summary="s"))
    assert len(turn) == n2 - q["folded"] + 1
    assert turn[q["start"]].get("request", "").startswith("SECOND REQUEST")
    print("compact: ok")


if __name__ == "__main__":
    check()
