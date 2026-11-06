"""The wire: every command a frontend can send, and what the worker does about it.

One table, and it is the protocol. It used to be a 500-line if/elif in main.py
with a second, shorter list beside it naming which commands answer on the reader
thread -- and the two had already drifted: `dir.list` was in one and not the
other, so whether it worked mid-run was decided by which list you read. Here a
command is one entry with its handler, a sentence, the fields it reads, and
whether it answers mid-run. `python3 main.py --wire` prints it, so the document
and the code cannot disagree.

The split that makes this readable is the other half of the change: a handler
does the wire half (read the envelope, reply to it) and `handler/worker.py` does
the conversation half. Nothing here owns a turn, and nothing there knows an id.
"""
import os, sys, time
from dataclasses import dataclass
from typing import Callable, Tuple

from handler import config, context, peers, usage
from handler.agent import inline, providers
from handler.agent.background import JOBS
from tools.loader import registry
from handler.session import store
from handler.worker import terminal_ctx
from integrations.memory import naming
from tools.loader import wire_result


# ---- the menu payloads ----
# Read on the reader thread while a turn is running, so they read what is loaded
# and never write: a menu that emptied itself for the length of a run is the bug
# these exist for.

def dir_listing(here: str, path: str = "") -> dict:
    """One directory's subdirectories, for the `/cwd` menu.

    Anywhere on **this** machine, unlike `fs_listing`: the point of `/cwd` is
    leaving where the conversation stands, so a path may be absolute, start with
    `~`, or step out with `..`. Only directories are rows, because only a
    directory can be an answer, and `.`-prefixed ones stay out the way they do
    in every menu here.

    `dir` is the absolute path actually listed, because that is what the
    `use this directory` row writes: the worker expanded `~`, and a frontend
    expanding it again would be a second idea of what a path is. `error` is why
    nothing is listed, so a menu can say "no such directory" instead of drawing
    an empty frame that looks like an empty folder.
    """
    want = os.path.expanduser((path or "").strip())
    if not os.path.isabs(want):
        want = os.path.join(here or os.getcwd(), want)
    want = os.path.normpath(want)
    parent = os.path.dirname(want)
    out = {"dir": want, "parent": "" if parent == want else parent,
           "entries": [], "error": ""}
    try:
        names = os.listdir(want)
    except OSError as e:
        out["error"] = e.strerror or str(e)
        return out
    entries = [{"name": n, "path": os.path.join(want, n), "dir": True}
               for n in names
               if not n.startswith(".") and os.path.isdir(os.path.join(want, n))]
    entries.sort(key=lambda e: e["name"].lower())
    out["entries"] = entries[:200]
    return out


def fs_listing(root: str, sub: str = "") -> dict:
    """One directory for a frontend's `@` menu: relative to the session's own
    directory and never above it, so `..` and an absolute path come back empty.
    Hidden entries stay out, and directories sort first.
    """
    base = os.path.realpath(root or os.getcwd())
    rel = (sub or "").strip().rstrip("/")
    if rel.startswith("/") or ".." in rel.split("/"):
        return {"dir": rel, "entries": []}
    target = os.path.realpath(os.path.join(base, rel))
    if target != base and not target.startswith(base + os.sep):
        return {"dir": rel, "entries": []}
    try:
        names = os.listdir(target)
    except OSError:
        return {"dir": rel, "entries": []}
    entries = []
    for n in names:
        if n.startswith("."):
            continue
        entries.append({"name": n, "dir": os.path.isdir(os.path.join(target, n)),
                        "path": (rel + "/" + n) if rel else n})
    entries.sort(key=lambda e: (not e["dir"], e["name"].lower()))
    return {"dir": rel, "entries": entries[:200]}


def replay(w, env):
    """Re-send a reopened conversation as the same events the live run sent, so a
    frontend redraws it with the code it already has. Nothing here carries a
    `status`: a replay is not a run, and a clock for it would time the redraw.
    """
    p = providers.get(w.sess.provider)
    tools_seen = 0
    for r in w.sess.records():
        t = r.get("t")
        if t == "user":
            # A record the worker marked as a note is runtime text it put in front of the
            # model, not a line the person typed: drawn as the notice it was, or a
            # reopened conversation puts words in their mouth. The tag in the text is
            # for the records written before that mark existed -- a turn carrying a
            # runtime block was never typed by a person.
            text = r.get("text") or ""
            if r.get("note") or "<todo_status>" in text:
                w.ipc.reply(env, "token", {"state": "notice", "token": text})
                continue
            # Names, never payloads: shipping the base64 back puts megabytes on the wire.
            data = {"state": "user", "token": r.get("text", "")}
            if r.get("images"):
                data["images"] = [{"name": i.get("name") or "image"} for i in r["images"]]
            w.ipc.reply(env, "token", data)
        elif t == "recall":
            # Shown, not hidden: runtime text put in front of the model is the user's to see.
            w.ipc.reply(env, "token", {"state": "notice", "token": r.get("text", "")})
        elif t == "assistant":
            if r.get("thinking"):
                w.ipc.reply(env, "token", {"state": "thinking", "token": r["thinking"]})
            if r.get("content"):
                w.ipc.reply(env, "token", {"state": "content", "token": r["content"]})
            if r.get("calls"):
                w.ipc.reply(env, "token", {"state": "tool_calls", "token": p.native_calls(r["calls"])})
        elif t == "tool":
            name = r.get("name", "")
            w.ipc.reply(env, "token", {
                "state": "tool_output", "token": name,
                # The same brief a live turn sent, or reopening a conversation draws a
                # build log the turn itself had reduced to a byte count.
                "result": wire_result(registry(), name, r.get("result", {})),
                "index": tools_seen})
            tools_seen += 1


# ---- handlers ----
# (worker, envelope) -> None, replying on the envelope it was given.

def _terminal(w, env):
    w.ipc.reply(env, "status", {"terminal": w.ipc.terminal_info})
    w.ipc.terminal_info = env.data
    w.ctx = terminal_ctx(w.ipc.terminal_info)
    # The boot session predates this envelope, so its frontend and cwd are stamped
    # here; neither writes, so no folder is born yet.
    w.sess.set_frontend(w.ctx.get("term_program") or "")
    w.sess.set_cwd(w.ctx.get("cwd") or "")
    w.ctx["session"] = w.sess


def _chat(w, env):
    data = env.data or {}
    # A peer message that took this door reads as one from the moment the turn is
    # built, not after: it has no user record, so the peer record is the only thing
    # the model's turn is filed under.
    for kind, payload in w.turn(data.get("text", ""),
                                peer_tag=data.get("peer") or None,
                                attached=data.get("images") or [],
                                policy=data.get("policy") or None):
        w.ipc.reply(env, kind, payload)


def _stop(w, env):
    # Background work first: the threads are daemons, but a shell job holds a real
    # process group that outlives us unless it is killed.
    JOBS.kill_all()
    w.sess.commit()
    w.drop_empty()
    w.ipc.reply(env, "status", {"state": "stopped"})
    sys.exit(0)


def _cancel(w, env):
    # The run itself was already stopped by the flag IPC set on the reader thread;
    # this is only the acknowledgement.
    w.ipc.reply(env, "status", {"state": "cancel_ack"})


def _background(w, env):
    # Same shape as cancel: the flag was set on the reader thread while this loop
    # was inside generate(), so the call has already moved.
    w.ipc.reply(env, "status", {"state": "background_ack"})


def _background_list(w, env):
    w.ipc.reply(env, "jobs", {"jobs": [j.brief() for j in JOBS.list(env.data.get("state") or "all")]})


def _background_kill(w, env):
    jid = env.data.get("job", "")
    w.ipc.reply(env, "jobs", {"killed": JOBS.kill(jid), "job": jid,
                              "jobs": [j.brief() for j in JOBS.list()]})


def _permission_mode(w, env):
    mode = (env.data.get("mode") or "ask").lower()
    if mode not in config.MODES: mode = "ask"
    # A detached worker keeps `off`: a gate here would wait for a person who is not
    # attached and cannot be, which is the hang the flag exists to prevent. The reply
    # carries what was actually set, so no screen shows a mode that nothing honours.
    if w.detached: mode = "off"
    w.mode = mode
    w.ipc.reply(env, "status", {"state": "permission", "mode": mode})


def _detail(w, env):
    """One call in full, arguments and result, read from the records. Images stay
    as blob refs: the point is to read what happened, not to move a megabyte."""
    try: want = int((env.data or {}).get("index", -1))
    except (TypeError, ValueError): want = -1
    w.ipc.reply(env, "detail", tool_detail(w.sess, want))


def _context_report(w, env):
    # Read off the prompt, the tool schemas and the history in memory: the readout
    # itself costs no tokens.
    try:
        w.ipc.reply(env, "context", context.report(w.sess, w.settings, w.ctx,
                                                   messages=w.messages, last=w.last_turn))
    except Exception as e:
        w.ipc.reply(env, "status", {"state": "error", "error": str(e)})


def _usage_report(w, env):
    # The same rollup --usage prints. Read from the meta files, so a hundred
    # conversations cost a hundred small json reads.
    try: days = int(env.data.get("days") or 0)
    except (TypeError, ValueError): days = 0
    rep = usage.rollup(days)
    # This conversation's own, including the rounds not yet committed.
    rep["session"] = {"id": w.sess.id, **(usage.of_records(w.sess.records()) or {})}
    w.ipc.reply(env, "usage", rep)


def _commands(w, env):
    w.ipc.reply(env, "commands", {"commands": inline.listing()})


def _skills(w, env):
    from integrations.skills import loader as skills
    w.ipc.reply(env, "skills", {"skills": skills.listing("user")})


def _session_root(w):
    return w.sess.meta.get("cwd") or w.ctx.get("cwd") or os.getcwd()


def _files(w, env):
    w.ipc.reply(env, "files", fs_listing(_session_root(w), env.data.get("dir", "")))


def _dirs(w, env):
    # `/cwd`'s menu: the directories anywhere on this machine, because choosing one
    # is leaving the directory the conversation is in.
    w.ipc.reply(env, "dirs", dir_listing(_session_root(w), env.data.get("dir", "")))


def _providers(w, env):
    w.ipc.reply(env, "providers", _providers_payload())


def _providers_payload() -> dict:
    return {"providers": config.listing(),
            "vision": config.vision_helper()[0], "vision_model": config.vision_helper()[1],
            "sighted": config.sighted()}


def _sessions(w, env):
    w.ipc.reply(env, "sessions", {"sessions": store.list_sessions(), "active": w.sess.id})


# ---- projects: directories the user promoted, persisted in projects.json ----

def projects_data(**extra) -> dict:
    """The tree's project payload, one shape on every projects reply: the promoted
    dirs, the pin order, the custom names and icons."""
    names, icons = store.list_project_details()
    return {"projects": store.list_projects(), "pinned": store.list_pinned(),
            "names": names, "icons": icons, **extra}


def _project_list(w, env):
    w.ipc.reply(env, "projects", projects_data())


def _project_add(w, env):
    d = store.add_project(env.data.get("dir", ""))
    w.ipc.reply(env, "projects", projects_data(added=d))


def _project_rename(w, env):
    # Cosmetics only: the group key stays the basename.
    store.rename_project(env.data.get("dir", ""), env.data.get("name", ""), env.data.get("icon", ""))
    w.ipc.reply(env, "projects", projects_data())


def _project_remove(w, env):
    gone = store.remove_project(env.data.get("dir", ""))
    w.ipc.reply(env, "projects", projects_data(removed=gone))


def _project_pin(w, env):
    store.pin_project(env.data.get("name", ""), bool(env.data.get("on", True)))
    w.ipc.reply(env, "projects", projects_data())


def _project_move(w, env):
    # One slot up or down inside the section the group sits in: pin order for
    # pinned groups, promotion order for the rest.
    try: delta = int(env.data.get("delta") or 0)
    except (TypeError, ValueError): delta = 0
    store.move_project(env.data.get("name", ""), env.data.get("dir", ""), delta)
    w.ipc.reply(env, "projects", projects_data())


# ---- providers ----

def _provider_use(w, env):
    w.settings = config.settings(config.use(env.data.get("name", "")))
    # Records are canonical, so the history rebuilds in the new dialect and the
    # model comes with the provider.
    w.sess.set_provider(w.settings["provider"])
    w.sess.set_model(w.settings.get("model", ""))
    w.messages = w.sess.messages()
    w.ipc.reply(env, "status", {"state": "provider", **w.settings_fields(),
                                "msg_count": len(w.messages)})


def _provider_set(w, env):
    data = env.data
    name = data.get("name", "")
    config.update(name, model=data.get("model"), key=data.get("api_key"),
                  vision=data.get("vision"), params=data.get("params"),
                  effort_map=data.get("effort_map"), model_params=data.get("model_params"))
    if name == w.settings.get("provider"):
        w.settings = config.settings()
        if data.get("model"): w.sess.set_model(data["model"])
        # A model swap can turn vision off, and a history holding screenshots fails
        # every request after. Records keep the blobs.
        w.messages = w.sess.messages()
        w.ipc.reply(env, "status", w.settings_fields())
    # Reply from listing(), never the request: echoing env.data puts the key in the log.
    w.ipc.reply(env, "providers", _providers_payload())


def _model_list(w, env):
    # Asked of the provider, not kept in a table here: the catalogs rotate. A
    # provider that will not answer says so.
    name = env.data.get("name") or w.settings["provider"]
    p = providers.new(name)
    p.setup(config.api_key(name))
    found = p.models() if hasattr(p, "models") else None
    known = config.entry(name)
    w.ipc.reply(env, "models", {
        "provider": name, "models": found or [],
        "listed": found is not None,
        "active": config.model_for(name),
        # What is already known, so the picker can mark a blind model without asking
        # the provider once per row.
        "blind": sorted({m for m, caps in (known.get("model_caps") or {}).items()
                         if "vision" not in caps} | set(known.get("blind_models") or []))})


def _vision_use(w, env):
    # A role, not a provider setting: set here rather than through provider.set.
    # Validated on the spot, or it fails later in a tool call.
    config.set_vision_helper(env.data.get("name", ""), env.data.get("model"))
    w.ipc.reply(env, "providers", _providers_payload())


# ---- sessions ----

def _session_fields(w) -> dict:
    return {"state": "session", "session_id": w.sess.id, **w.settings_fields(),
            "msg_count": len(w.messages)}


def _session_effort(w, env):
    # Session state, not provider state: the depth belongs to this conversation and
    # travels with it on reload.
    effort = env.data.get("effort", "")
    # Refused rather than approximated: every other rung lands on the nearest thing
    # the model has, but "off" has to mean off.
    if reason := config.effort_block(effort):
        w.ipc.reply(env, "status", {"state": "error", "error": reason})
        return
    w.sess.set_effort(effort)
    # ...and remembered as the level the next conversation opens on.
    config.set_default_effort(w.sess.effort)
    w.ipc.reply(env, "status", {"state": "effort", "effort": w.sess.effort,
                                "session_id": w.sess.id})


def _session_cwd(w, env):
    # Where the next conversation stands, said as a notice because it is the answer
    # to a question someone asked by typing a directory into a picker.
    notice = w.set_cwd_here(env.data.get("path") or "")
    w.ipc.reply(env, "token", {"state": "notice", "token": notice})


def _session_new(w, env):
    w.new_session()
    w.ipc.reply(env, "status", _session_fields(w))
    # Saying the empty name is the point: the frontend is showing the one that just went.
    w.announce_title(env)


def _session_load(w, env):
    w.load_session(env.data.get("id", ""))
    # Status first, so a frontend can clear what it was showing before the replayed
    # conversation starts arriving.
    w.ipc.reply(env, "status", _session_fields(w))
    w.announce_title(env)
    replay(w, env)
    w.ipc.reply(env, "token", {"state": "done", "token": "done", "status": "done",
                               "msg_count": len(w.messages)})


def _session_delete(w, env):
    sid = env.data.get("id", "")
    w.ipc.reply(env, "status", {"state": "deleted" if store.delete(sid) else "error",
                                "session_id": sid})


def _session_archive(w, env):
    # The trash can, not the delete: out of every listing, back by moving the folder in.
    sid = env.data.get("id", "")
    w.ipc.reply(env, "status", {"state": "archived" if store.archive(sid) else "error",
                                "session_id": sid})


def _session_pin(w, env):
    # The reply is a fresh sessions list, so the frontend re-stamps its pins.
    sid = env.data.get("id", "")
    on = bool(env.data.get("on", True))
    if sid == w.sess.id: w.sess.set_pinned(on)
    else: store.pin_session(sid, on)
    w.ipc.reply(env, "sessions", {"sessions": store.list_sessions(), "active": w.sess.id})


def _session_rename(w, env):
    # A name set by hand is stamped `user`, which is what stops the namer renaming it
    # back later: the auto title never overwrites a chosen one.
    sid = env.data.get("id", "")
    title = (env.data.get("title") or "").strip()
    if not title: raise ValueError("a name needs something in it")
    if sid == w.sess.id:
        w.sess.set_title(title, "user")
        w.announce_title(env)
    else:
        title = store.rename_session(sid, title)
        w.ipc.reply(env, "status", {"state": "session_title", "title": title,
                                    "source": "user", "session_id": sid})


def _shrink(value, limit: int = 20_000):
    """Cap the strings in a stored payload. The detail view exists because the
    wire strips output, but a 5MB build log still should not cross as one line."""
    if isinstance(value, str):
        if len(value) <= limit: return value
        return value[:limit] + f"\n... [{len(value) - limit} more characters]"
    if isinstance(value, dict): return {k: _shrink(v, limit) for k, v in value.items()}
    if isinstance(value, list): return [_shrink(v, limit) for v in value]
    return value


def tool_detail(session, index: int) -> dict:
    """One call in full, arguments and result, read from the records."""
    calls, results = [], []
    for r in session.records():
        if r.get("t") == "assistant": calls.extend(r.get("calls") or [])
        elif r.get("t") == "tool": results.append(r)
    if not 0 <= index < len(results):
        # Not "error": that key means a failed envelope everywhere else, and a call
        # that was rewound away is absent, not broken.
        return {"index": index, "total": len(results), "unavailable": "no such call"}
    rec = results[index]
    # Positional, since every call yields one tool record in order, but checked by
    # name so a drifted history shows no arguments rather than another call's.
    call = calls[index] if index < len(calls) and calls[index].get("name") == rec.get("name") else {}
    return {"index": index, "total": len(results), "name": rec.get("name", ""), "ts": rec.get("ts", ""),
            "args": _shrink(call.get("args") or {}), "result": _shrink(rec.get("result") or {})}


@dataclass(frozen=True)
class Command:
    """One action on the wire.

    `direct` means answered on the reader thread: the main loop is inside
    `generate()` for a whole turn, so a command a person needs while one runs has
    to answer there. A direct handler is read-only and quick -- it must never wait
    on a model, a lock, or a subprocess.
    """
    handler: Callable
    summary: str
    direct: bool = False
    fields: Tuple[str, ...] = ()


TABLE = {
    "chat": Command(_chat, "send a message and run one turn",
                    fields=("text", "images", "peer", "policy")),
    "stop": Command(_stop, "kill background work, commit, and exit"),
    "cancel": Command(_cancel, "stop the run in flight (the flag is set on the reader thread)"),
    "background": Command(_background, "move the call in flight to a background job"),
    "background.list": Command(_background_list, "every job, or those in `state`", fields=("state",)),
    "background.kill": Command(_background_kill, "end one job", fields=("job",)),
    "permission.mode": Command(_permission_mode, "ask | auto | bypass | off", fields=("mode",)),
    "tool.detail": Command(_detail, "one call in full: args and result", direct=True, fields=("index",)),
    "context.report": Command(_context_report, "what the context window is full of"),
    "usage.report": Command(_usage_report, "tokens and rounds over `days`", fields=("days",)),
    "command.list": Command(_commands, "the inline /commands", direct=True),
    "skill.list": Command(_skills, "installed skills the user may load", direct=True),
    "fs.list": Command(_files, "one directory for the @ menu", direct=True, fields=("dir",)),
    "dir.list": Command(_dirs, "one directory for the /cwd menu", direct=True, fields=("dir",)),
    "provider.list": Command(_providers, "providers, their models and whether they can see", direct=True),
    "model.list": Command(_model_list, "what a provider says it offers", fields=("name",)),
    "provider.use": Command(_provider_use, "point the conversation at another provider", fields=("name",)),
    "provider.set": Command(_provider_set, "write a provider's settings",
                            fields=("name", "model", "api_key", "vision", "params", "effort_map", "model_params")),
    "vision.use": Command(_vision_use, "name the provider that looks at pictures", fields=("name", "model")),
    "session.list": Command(_sessions, "every conversation, newest first", direct=True),
    "session.new": Command(_session_new, "start a fresh conversation"),
    "session.load": Command(_session_load, "reopen one, then replay it", fields=("id",)),
    "session.delete": Command(_session_delete, "remove a conversation's folder", fields=("id",)),
    "session.archive": Command(_session_archive, "move one out of every listing", fields=("id",)),
    "session.pin": Command(_session_pin, "pin or unpin one", fields=("id", "on")),
    "session.rename": Command(_session_rename, "name one by hand", fields=("id", "title")),
    "session.effort": Command(_session_effort, "how hard this conversation thinks", fields=("effort",)),
    "session.cwd": Command(_session_cwd, "where the next conversation stands", fields=("path",)),
    "project.list": Command(_project_list, "promoted directories, pins, names and icons"),
    "project.add": Command(_project_add, "promote a directory", fields=("dir",)),
    "project.rename": Command(_project_rename, "a display name and icon", fields=("dir", "name", "icon")),
    "project.remove": Command(_project_remove, "demote one", fields=("dir",)),
    "project.pin": Command(_project_pin, "pin a group", fields=("name", "on")),
    "project.move": Command(_project_move, "move a group one slot", fields=("name", "dir", "delta")),
}


def wire_doc() -> dict:
    """The protocol, read off the table that implements it."""
    return {"commands": [{"action": a, "summary": c.summary,
                          "reads": list(c.fields), "answers_mid_run": c.direct}
                         for a, c in sorted(TABLE.items())]}


def _answer(w, env, handler):
    """Run one handler. An exception becomes an error envelope rather than a dead
    worker: the loop serves every later command, and a consumer's typo gets an
    answer it can read instead of silence."""
    try:
        handler(w, env)
    except SystemExit:
        raise
    except Exception as e:
        w.ipc.reply(env, "status", {"state": "error", "error": str(e)})


def serve(w):
    """The loop: poll, answer, sleep. Returns when the frontend goes away."""
    ipc = w.ipc
    # Registered once, from the same table the loop dispatches from, so the two
    # cannot disagree about which commands work mid-run.
    ipc.direct = {a: (lambda env, fn=c.handler: _answer(w, env, fn))
                  for a, c in TABLE.items() if c.direct}
    while True:
        # The frontend is gone: pipe EOF or reparented to launchd. Mirrors `stop`:
        # background work killed first, then the session committed, empty dropped.
        if ipc.eof() or os.getppid() == 1:
            JOBS.kill_all()
            w.sess.commit()
            w.drop_empty()
            sys.exit(0)
        # Names that finished while the loop was busy, applied here rather than on
        # the thread that produced them, so meta.json and stdout have one writer.
        for finished in naming.drain():
            naming.apply(finished, w.sess)
        w.announce_title()
        # Peer mail for an idle agent: nobody is at the prompt, so a message would sit
        # in the mailbox until the person pressed enter. Shaped like the cmd a
        # frontend sends, so it goes through the same door and becomes an ordinary
        # turn. Skipped while a run is in flight, where the round seam owns delivery.
        if not ipc.cancelled() and not ipc.running:
            for mail in peers.as_cmds(w.sess.id): ipc.inbox.put(mail)
        for env in ipc.poll():
            if env.type == "terminal":
                _answer(w, env, _terminal)
                continue
            if env.type != "cmd": continue
            cmd = TABLE.get(env.data.get("action"))
            if cmd is None:
                # Said rather than swallowed: a consumer's typo costs one envelope to
                # find instead of an afternoon.
                ipc.reply(env, "status", {"state": "error",
                                          "error": f"unknown action: {env.data.get('action')!r}"})
                continue
            _answer(w, env, cmd.handler)
        # Fifty wakeups a second, not a thousand. Nothing a person does waits on this
        # loop: a turn runs inside `chat()` and a key arrives on the reader thread. At
        # 1ms this loop, which stats a peer mailbox and a session's own files on every
        # pass, cost 13% of a core with the machine sitting still.
        time.sleep(0.02)
