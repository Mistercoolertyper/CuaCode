"""What every conversation has cost, added up.

A round's cost is stamped onto the assistant record it belongs to, which is the
only place it is unambiguous: one round, one model, one set of counts, and it
travels with the conversation when that is moved or reopened. commit() then rolls
the records into meta.json, so a question about a hundred sessions reads a hundred
small json files rather than a hundred transcripts full of screenshots.

A round another loop ran -- a subagent, a page digest, the permission gate -- is stamped the
same way onto a `usage` record beside the assistant ones. It is billed whether or not this
conversation's own loop ran it, and a total that quietly left it out read as a discount.

Nothing here estimates. Every figure is what a provider reported for a round, and a
provider that reports nothing contributes nothing. The one exception is thinking
tokens on providers that bill them without itemising them, marked wherever shown.
"""
import json, threading
from collections import defaultdict

from handler import context
from handler.session import store

# Side rounds, waiting for the loop to take them. A subagent bills on its own thread and
# meta.json has one writer, so nothing files these directly: the loop absorbs them between
# turns, the same path a finished name takes.
_queued, _queued_lock = [], threading.Lock()

# What a stamped round counts. Short keys: written once per assistant record, and a
# session can hold hundreds of them.
COUNTS = ("in", "out", "cache", "think", "secs")


def stamp(fields: dict, provider: str, model: str) -> dict:
    """One round's cost, in the shape it is stored in.

    Empty when the provider reported nothing, and an empty dict is never written:
    no usage means "not measured", not a round that cost zero.
    """
    out = {}
    for src, dst in (("in_tokens", "in"), ("out_tokens", "out"), ("cached_tokens", "cache"),
                     ("thinking_tokens", "think")):
        if n := fields.get(src): out[dst] = n
    if not out: return {}
    if secs := fields.get("gen_secs"): out["secs"] = round(secs, 3)
    if fields.get("thinking_est"): out["est"] = True
    if model: out["model"] = model
    if provider: out["provider"] = provider
    return out


def record(source: str, chunk: dict, provider: str) -> dict:
    """One side round's cost -- a subagent, a page digest, the permission gate.

    The chunk generate() yielded is the shape a main-loop round yields, so the same
    turn_fields and stamp it, and a side call lands in the report like any other; src is
    what ran it, the one thing that still tells a fan-out from a page fetch once the numbers
    are added up. Queued rather than filed: a side round is billed on its own thread and
    meta.json has one writer, so the loop takes these between turns, the way it does a
    finished name.
    """
    cost = stamp(context.turn_fields(chunk), provider, chunk.get("model") or "")
    if not cost: return {}
    cost["src"] = source or "side"
    with _queued_lock: _queued.append(cost)
    return cost


def absorb(session) -> int:
    """File every queued side round under the conversation that is live now, and say how
    many there were.

    Live-now rather than started-under because the alternative is worse: a backgrounded
    subagent outlives the turn that started it, and the session it began under may be closed
    and swapped out by the time its last round lands. Billed either way, so the choice is
    only about which conversation's report shows it.
    """
    with _queued_lock:
        queued, _queued[:] = list(_queued), []
    for cost in queued:
        try: session.add_spend(cost)
        except Exception: pass
    return len(queued)


def _add(into: dict, u: dict, peak: bool = True, src: str = None):
    """Fold one stamped round into a bucket.

    peak only for the conversation's own rounds: a subagent's prompt is its own window,
    and one let into the peak would claim a window this conversation never held. src folds
    a side round a second time, filed under what ran it, so where the money went is a
    reading rather than a search.
    """
    for k in COUNTS:
        if n := u.get(k): into[k] = round(into.get(k, 0) + n, 3)
    into["rounds"] = into.get("rounds", 0) + 1
    # Peak kept beside the running total because the two answer different questions:
    # a prompt is re-sent in full every round, so summing is what the provider bills
    # while the peak is how big the conversation actually got.
    if peak: into["peak"] = max(into.get("peak", 0), u.get("in") or 0)
    if src is not None: _add(into.setdefault("srcs", {}).setdefault(src, {}), u, peak=False)
    # One estimated round makes the bucket's thinking figure an estimate, carried
    # with the number so the tilde survives to the report.
    if u.get("est"): into["est"] = True


def of_records(records: list[dict]) -> dict:
    """A session's own total, rebuilt from its records.

    Recomputed on every commit rather than incremented: a rewound round takes its cost
    with it, and a counter that only went up would keep charging for turns the
    conversation no longer holds.
    """
    total, models, days = {}, defaultdict(dict), defaultdict(dict)
    for rec in records:
        t = rec.get("t")
        # An assistant round is one this conversation ran; a usage record is one it paid
        # for and did not run. Both are cost, so both fold in, and only the first can move
        # the peak.
        if t not in ("assistant", "usage"): continue
        u = rec.get("u") or {}
        if not u: continue
        mine = t == "assistant"
        src = None if mine else str(u.get("src") or "side")
        _add(total, u, peak=mine, src=src)
        key = " ".join(x for x in (u.get("provider", ""), u.get("model", "")) if x) or "unknown"
        _add(models[key], u, peak=mine, src=src)
        # Filed under the date the round happened, not the session's last-touched
        # date: a conversation carried across three days spent its tokens on three
        # days, and filing them all under the last one makes every long session a spike.
        if ts := rec.get("ts"): _add(days[ts[:10]], u, peak=mine, src=src)
    if not total: return {}
    return {**total, "models": dict(models), "days": dict(days)}


def _blank() -> dict:
    return {"in": 0, "out": 0, "cache": 0, "think": 0, "secs": 0.0, "rounds": 0, "peak": 0}


def rollup(days: int = 0, metas: list[dict] = None) -> dict:
    """Every session's total, added together.

    days limits it to the last N days by the date the tokens were spent, which is why
    the per-day buckets exist: filtering whole sessions by their last-touched date
    would drop the older half of a conversation still going.
    """
    metas = store.list_sessions() if metas is None else metas
    cutoff = _cutoff(days)

    total, models, per_day, sessions = _blank(), defaultdict(_blank), {}, []
    counted, unmeasured = 0, 0
    for meta in metas:
        u = _repaired(meta)
        if not u:
            # A session from before this existed, or one whose provider never reported
            # usage. Counted separately: a total that quietly omitted it would read as
            # those sessions costing nothing.
            unmeasured += 1
            continue
        kept, kept_days = _blank(), (u.get("days") or {})
        if cutoff:
            for day, spent in kept_days.items():
                if day >= cutoff: _merge(kept, spent)
        else:
            _merge(kept, u)
        if not kept["rounds"]:
            continue

        counted += 1
        _merge(total, kept)
        # Per-model and per-day are kept as two separate breakdowns: the crossing lives
        # in the records and a rollup is not worth reopening transcripts for. With a day
        # filter, a session counts toward a model only when all of it is inside the
        # window; half a session overstates its model.
        whole = kept["rounds"] == u.get("rounds")
        for name, spent in (u.get("models") or {}).items():
            if not cutoff or whole: _merge(models[name], spent)
        for day, spent in kept_days.items():
            if cutoff and day < cutoff: continue
            _merge(per_day.setdefault(day, _blank()), spent)
        sessions.append({"id": meta.get("id", ""), "title": meta.get("title", ""),
                         "updated": meta.get("updated", ""), **kept})

    # Ranked by context, not billed volume: the sum of re-sent prompts is what the
    # provider charged, and it reads as "how much I used" when it is really "how many
    # times the same window went up". The peak is how big the conversation got.
    sessions.sort(key=lambda s: s.get("peak") or 0, reverse=True)
    return {
        "sessions": counted, "unmeasured": unmeasured, "since": cutoff or _first_day(per_day),
        "total": total,
        "models": [{"name": k, **v} for k, v in sorted(models.items(), key=lambda kv: -(kv[1]["in"] + kv[1]["out"]))],
        "days": [{"day": d, **v} for d, v in sorted(per_day.items())],
        "top": sessions[:8],
    }


def _repaired(meta: dict) -> dict:
    """A session's rollup, recomputed from its transcript if it predates the peak.

    A rollup written before the peak was recorded has no peak, and adding rollups
    together cannot produce one because the number only exists in the records. So a
    stale meta is recomputed once from its own transcript and written back. Bounded
    work: it happens once per session that predates the field.
    """
    u = meta.get("usage") or {}
    if not u or not u.get("rounds") or u.get("peak"): return u
    sid = meta.get("id") or ""
    try: d = store.path(sid)
    except ValueError: return u
    records = store.read_jsonl(d / "messages.jsonl")
    if not records: return u
    fresh = of_records(records)
    # Only when the recompute agrees about what it is recomputing: a transcript moved
    # or trimmed under its meta is left alone rather than rewritten to disagree with
    # the number the session reported.
    if fresh.get("rounds") != u.get("rounds") or not fresh.get("peak"): return u
    store.write_json(d / "meta.json", {**meta, "usage": fresh})
    return fresh


def _merge(into: dict, u: dict):
    for k in ("in", "out", "cache", "think", "rounds"):
        into[k] = into.get(k, 0) + (u.get(k) or 0)
    into["secs"] = round(into.get("secs", 0.0) + (u.get("secs") or 0), 3)
    into["peak"] = max(into.get("peak", 0), u.get("peak") or 0)
    if u.get("est"): into["est"] = True
    # The side rounds ride along in their own buckets, so an answer about a window of
    # days can still say what the subagents inside it cost.
    for name, spent in (u.get("srcs") or {}).items():
        _merge(into.setdefault("srcs", {}).setdefault(name, {}), spent)


def _cutoff(days: int) -> str:
    if not days or days <= 0: return ""
    from datetime import datetime, timedelta, timezone
    # UTC, because that is what a record's `ts` is stamped in and what the
    # per-day buckets are keyed by. `date.today()` is the LOCAL date, which is
    # already tomorrow in +03:00 while every record still says today, so a
    # local cutoff makes `--days 1` report an empty window for the last three
    # hours of every evening and makes the newest day look like it is missing.
    return (datetime.now(timezone.utc).date() - timedelta(days=days - 1)).isoformat()


def _first_day(per_day: dict) -> str:
    return min(per_day) if per_day else ""


def tps(u: dict) -> float:
    secs = u.get("secs") or 0
    return round((u.get("out") or 0) / secs, 1) if secs >= 1 else 0.0


# ---------------------------------------------------------------------------
# the terminal report


def _n(v: int) -> str:
    if v >= 1_000_000: return f"{v / 1e6:.2f}M"
    if v >= 10_000: return f"{v // 1000}k"
    if v >= 1_000: return f"{v / 1000:.1f}k"
    return str(v)


def _dur(secs: float) -> str:
    s = int(secs)
    if s >= 3600: return f"{s // 3600}h {s % 3600 // 60:02d}m"
    if s >= 60: return f"{s // 60}m {s % 60:02d}s"
    return f"{s}s"


def render(rep: dict) -> str:
    total = rep["total"]
    head = f"usage · {rep['sessions']} session" + ("" if rep["sessions"] == 1 else "s")
    if rep.get("since"): head += f" · since {rep['since']}"
    if rep.get("unmeasured"): head += f" · {rep['unmeasured']} with nothing measured"

    est = "~" if total.get("est") else ""
    # Known only for rounds recorded since the peak was; an older session says nothing
    # rather than claiming zero.
    peak = _n(total["peak"]) if total.get("peak") else "\u2013"
    # "context" comes first because it means the most: the largest window a session ever
    # held. "billed input" is the same window re-sent every round, and a sum that dwarfs
    # the peak invites the same misreading when it is the headline.
    lines = [head, "",
             f"  {'context':<13} {peak:>10}  largest single window held",
             f"  {'billed input':<13} {_n(total['in']):>10}  re-sent every round"]
    # A cached prompt is billed too, at a fraction of a fresh token, so it is a slice of the
    # line above rather than a discount off it. Rounds recorded before this was read say
    # nothing, which is why zero prints no row rather than claiming nothing was ever cached.
    if total.get("cache"):
        share = round(total["cache"] * 100 / total["in"]) if total["in"] else 0
        lines.append(f"  {'cached input':<13} {_n(total['cache']):>10}  {share}% of the prompt, served from cache")
    lines.append(f"  {'tokens out':<13} {_n(total['out']):>10}  of which thinking  {est}{_n(total['think'])}")
    # Counted above, and said again by whatever ran them: with a fan-out going these are
    # most of what was spent, and with no line here they cannot be seen at all.
    srcs = total.get("srcs") or {}
    if srcs:
        side = {k: sum(s.get(k) or 0 for s in srcs.values()) for k in ("in", "out", "rounds")}
        # One source named here, several broken out below: the summary already is the
        # breakdown when there is only one, and printing it twice says it once too often.
        only = f" ({next(iter(srcs))})" if len(srcs) == 1 else ""
        lines.append(f"  {'side calls':<13} {side['rounds']:>10}  rounds outside this loop{only}"
                     f"  in {_n(side['in'])} / out {_n(side['out'])}")
        if len(srcs) > 1:
            width = max(len(name) for name in srcs)
            for name, s in sorted(srcs.items(), key=lambda kv: -((kv[1].get('in') or 0) + (kv[1].get('out') or 0))):
                rounds = f"{s.get('rounds', 0)} round" + ("" if s.get("rounds") == 1 else "s")
                lines.append(f"    {name:<{width}}   in {_n(s.get('in', 0)):>7}   out {_n(s.get('out', 0)):>7}   {rounds}")
    lines += [f"  {'rounds':<13} {total['rounds']:>10}",
              f"  {'generating':<13} {_dur(total['secs']):>10}  {tps(total)} tok/s average"]

    if rep["models"]:
        lines += ["", "by model"]
        width = max(len(m["name"]) for m in rep["models"])
        # The cache share rides beside the input it belongs to, padded so that the columns
        # after it line up whether or not a model reported one.
        shares = [f" ({round(m['cache'] * 100 / m['in'])}% cached)" if m.get("cache") and m.get("in") else ""
                  for m in rep["models"]]
        sw = max(len(s) for s in shares)
        for m, share in zip(rep["models"], shares):
            rounds = f"{m['rounds']} round" + ("" if m["rounds"] == 1 else "s")
            think = ("~" if m.get("est") else "") + _n(m["think"])
            lines.append(f"  {m['name']:<{width}}   in {_n(m['in']):>7}{share:<{sw}}   out {_n(m['out']):>7}   "
                         f"think {think:>7}   {tps(m):>6} tok/s   {rounds}")

    if rep["days"]:
        recent = rep["days"][-14:]
        peak = max(d["in"] + d["out"] for d in recent) or 1
        lines += ["", "by day"]
        for d in recent:
            spent = d["in"] + d["out"]
            lines.append(f"  {d['day']}  {'█' * max(round(spent / peak * 24), 1):<24}  {_n(spent)}")

    if rep["top"]:
        # Context, rounds and billed volume together, ranked by peak because that is how
        # big each conversation got. The billed column adds up every round's prompt, which
        # goes up in full each round, so a conversation that never held more than 25k can
        # have been charged 472k over 24 of them.
        lines += ["", f"  {'largest context':<21}  {'':<40}  {'billed':>7}  {'rounds':>6}  {'peak':>7}"]
        for s in rep["top"]:
            # One showing nothing but its id is a session not yet named, which is the
            # truth about it.
            title = (s["title"] or "")[:40]
            # Unknown for sessions rolled up before the peak was recorded, which says
            # nothing rather than claiming zero.
            peak = _n(s["peak"]) if s.get("peak") else "-"
            lines.append(f"  {s['id']}  {title:<40}  {_n(s['in'] + s['out']):>7}  "
                         f"{s['rounds']:>6}  {peak:>7}")

    return "\n".join(lines)


def cli(argv: list[str]) -> int:
    """`python3 -m handler.usage [--days N] [--json]`, printed and gone.

    Its own entry point rather than a flag on the worker: the question is about the app
    rather than inside a conversation with it, and starting a session to answer it would
    add to the number being asked about.
    """
    days = 0
    if "--days" in argv:
        try: days = int(argv[argv.index("--days") + 1])
        except (IndexError, ValueError):
            print("usage: handler/usage.py [--days N] [--json]")
            return 2
    rep = rollup(days)
    print(json.dumps(rep, indent=2) if "--json" in argv else render(rep))
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(cli(sys.argv[1:]))
