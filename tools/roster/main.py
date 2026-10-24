"""The `roster` tool: talk to another live agent.

Identity is not an argument. The sender's name, directory and session id all come
off the session this call is standing in, because a `from` field would be the
model's word for who it is and the whole point of naming the sender is that the
recipient can believe it.

The one bit of state here is `_warned`: the two-phase send. A first call to an
agent that has gone quiet answers with a warning instead of sending, so reaching
one is deliberate. A module is loaded once per process and a process is one
session, so a module-level set is exactly the lifetime this needs.
"""
import os

from handler import peers

_warned: set[str] = set()


def _me(ctx) -> str:
    """This session's id, off the envelope the worker already put in ctx."""
    d = (ctx or {}).get("session_dir") if isinstance(ctx, dict) else None
    return os.path.basename(str(d)) if d else ""


def run(args: dict, ctx) -> dict:
    sid = _me(ctx)
    if not sid:
        return {"error": "no session is open"}

    action = args.get("action")

    if action == "init":
        return {"me": peers.brief(sid), "peers": peers.roster(sid)}

    if action == "message":
        body = str(args.get("body") or "").strip()
        if not body:
            return {"error": "body required"}
        if len(body) > peers.MAX_BODY:
            return {"error": f"body is {len(body)} characters; the limit is {peers.MAX_BODY}"}

        to = str(args.get("to") or "").strip()
        if not to:
            targets = peers.roster(sid)
            if not targets:
                return {"error": "no other agents are in this project right now; "
                                 "`init` lists who is reachable, and an id from earlier still works"}
            return {"ok": True, "sent": [_send(sid, t["id"], body, t["name"]) for t in targets]}

        who = peers.target(to)
        if who is None:
            return {"error": f"no agent matches {to!r}; call init for the roster, or use an exact session id"}
        tid = str(who.get("id"))

        # Two two-phase gates, both keyed on the same warning set: reaching an agent the
        # roster left off, and going past the exchange limit. Neither is a refusal, so the
        # same call again delivers -- which is what makes reaching a quiet agent, or
        # pushing through a cap, cost a deliberate second look rather than a retry loop.
        if not peers.reachable(tid) and tid not in _warned:
            _warned.add(tid)
            return {"ok": False, "warning": peers.quiet_note(tid, who)}

        n = peers.exchange(sid, tid)
        if n >= peers.EXCHANGE_CAP and tid not in _warned:
            _warned.add(tid)
            return {"ok": False, "warning": peers.exchange_note(tid, n)}

        _warned.discard(tid)
        return {"ok": True, "sent": [_send(sid, tid, body, peers.name(tid, who))]}


def _send(sid: str, to: str, body: str, who: str) -> dict:
    """One delivery, described by who it is going TO.

    `who` is the recipient's name and not the sender's: it is what the row saying
    "you to <name>" is drawn from, and the sender's own name there reads backwards.
    """
    rec = peers.send(to, body, sid)
    return {"to": to, "name": who, "id": rec["id"]}
