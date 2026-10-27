import sys, os, time
sys.path.insert(0, os.path.dirname(__file__))

from handler.protocol import IPC
from handler.agent.main import generate, _compact_now
from handler.agent import compact, inline, providers
from handler.agent.background import JOBS
from handler.session import store
from handler.session.main import Session
from handler import config, context, environment, peers, usage, version
from integrations.memory import loader as memory, naming, recall
from handler.agent import stepname
from integrations.instructions import loader as instructions
from integrations.skills import loader as skills
from integrations.permissions import decider
from tools._safety import shell as shell_safety

SETTINGS = config.settings()

ipc = IPC()


class Ctx(dict):
    """Envelope fields, plus the attribute spellings some tools read off it."""

    self_identity = property(lambda s: s.get("frontmost_app"))
    self_handle = property(lambda s: s.get("frontmost_hwnd"))
    session_dir = property(lambda s: s.get("session_dir"))
    cwd = property(lambda s: s.get("cwd"))


def terminal_ctx(data) -> Ctx:
    """Terminal facts, with the foreground app filled in on Windows.

    The Rust frontend can only probe this cheaply on macOS; the worker already
    has pywin32 and can name the terminal it is speaking to. Without a name,
    screenshot cannot park the agent's own window and app_open cannot find it
    when the two swap places.
    """
    info = dict(data or {})
    if os.name == "nt":
        if not info.get("frontmost_app"):
            try:
                from tools.screenshot._capture_windows import active_app_name
                info["frontmost_app"] = active_app_name()
            except Exception:
                pass
        # The name cannot distinguish two terminals of the same app. The window
        # in front at boot can: this worker was started by the terminal the user
        # launched, and that window is the one parking must move.
        if not info.get("frontmost_hwnd"):
            try:
                from tools.screenshot._capture_windows import foreground_window
                info["frontmost_hwnd"] = foreground_window()
            except Exception:
                pass
    return Ctx(info)


ctx = terminal_ctx(ipc.terminal_info)

# A session exists from boot: its folder is born with the first record and an
# unused one is dropped on exit. Effort starts at the account default.
sess = Session.create(provider=SETTINGS["provider"], model=SETTINGS.get("model", ""),
                      effort_level=config.default_effort())
messages = sess.messages()
# Set once, at boot, and not per turn: /compact runs at the prompt with no turn in
# flight, and a fold that could only find the session mid-turn would answer "nothing
# to fold" to the one command that exists to fold on demand. `sess` is rebound on a
# resume, so this is refreshed there too.
ctx["session"] = sess
# The pid a peer checks to tell a slow agent from a dead one. meta.json's `updated`
# only moves when a round commits, so without this an agent ten minutes into a build
# is indistinguishable from one that was killed.
peers.become(sess.id)


def settings_fields(sess) -> dict:
    """Who is answering, how hard it thinks, how much window it has. Read off the
    session, not the config: a conversation reopened from disk answers with its own
    provider and model.
    """
    out = {"provider": sess.provider, "model": sess.meta.get("model", ""),
           "effort": sess.effort}
    # No window beats a window somebody invented.
    if window := config.context_window(sess.provider, sess.meta.get("model", "")):
        out["context_max"] = window
    return out


# The ready line carries the session id, so a frontend can tie its live row to
# the record committed later, plus the mode and settings for the header.
ipc.send("status", {"state": "ready", "session_id": sess.id,
                    "mode": config.permission_mode(), **settings_fields(sess)})
# Tools run in this process, so the memory tool retitles the object the loop
# holds, not the file underneath it.
naming.set_live(sess)

# ask | auto | bypass | off, for calls a tool declares need asking; a frontend
# re-declares it once it is up.
PERM_MODE = config.permission_mode()

# The last round's cost, read by /context: one round, not a running total.
LAST_TURN = {}


def turn_fields(chunk: dict) -> dict:
    """What the round that just generated cost, remembered for /context."""
    fields = context.turn_fields(chunk)
    LAST_TURN.clear()
    LAST_TURN.update({"input": (chunk.get("usage") or {}).get("input", 0), **fields})
    return fields


def context_fields(usage: dict) -> dict:
    """The context readout. Spent tokens only, and the window only when the
    model's size is actually known."""
    used = (usage or {}).get("input", 0) + (usage or {}).get("output", 0)
    if not used: return {}
    out = {"context_used": used}
    if window := config.context_window(SETTINGS["provider"], SETTINGS.get("model", "")):
        out["context_max"] = window
        out["context_left"] = max(window - used, 0)
    return out


def ask_user(question: str, options: list, detail: str = "", n: int = 1, of: int = 1) -> dict:
    """Put a question to the user and wait, without ending the turn.

    The same envelope a permission uses, with `question`, `options` and `detail`
    added. A frontend that only knows the binary prompt ignores the extras and
    answers allow/deny, which reads as yes to the first option -- so an old build
    asks a worse question instead of drawing nothing and hanging.
    """
    options = [str(o) for o in (options or [])]
    # `kind` says what this is, rather than letting a frontend infer it from the
    # name: a permission can be raised for a tool called anything, and inferring
    # "this is a question" from a tool name is the kind of guess that draws the
    # wrong screen for one of the two cases. The absence of `kind` means permission.
    payload = {"name": "ask_user", "kind": "question", "question": question,
               "options": options, "n": n, "of": of,
               "args": {"question": question, "options": options}}
    if detail: payload["detail"] = detail
    # No timeout, deliberately, and the same as a permission asks. A question the
    # person walked away from is still a question when they come back: expiring it
    # would either fail the call under them or, worse, let the model decide what
    # they would have said. `stop` is what ends the wait, and the only thing that
    # should -- a cancel, or the frontend going away.
    reply = ipc.call("permission", payload, timeout=None, stop=ipc.cancelled)
    data = (reply.data or {}) if reply else {}
    # Three answer shapes, newest first: a pick, a typed answer, and the plain
    # allow/deny an unmigrated frontend sends back.
    if data.get("choice"): return {"choice": str(data["choice"])}
    if (data.get("answer") or "").strip(): return {"answer": str(data["answer"]).strip()}
    if reply is None: return {"error": "no answer: cancelled, or the frontend never showed it"}
    if not data.get("allow") and (data.get("allow") is False): return {"error": "declined"}
    return {"choice": options[0]} if options else {"answer": "yes"}


def ask_permission(name: str, args: dict, preview: dict = None, note: str = None) -> bool:
    """Put one call to the user. timeout=None keeps the question open as long as
    they need; a cancel ends the run it belonged to. preview is what the call
    would do; note is why the auto gate flagged it."""
    payload = {"name": name, "kind": "permission", "args": args}
    if preview: payload["preview"] = preview
    if note: payload["note"] = note
    reply = ipc.call("permission", payload, timeout=None, stop=ipc.cancelled)
    return bool(reply and (reply.data or {}).get("allow"))


def auto_decide(tool, name: str, args: dict, preview: dict, tail: str, ctx) -> tuple:
    """The auto gate: a configured small model decides. Returns (allow, reason)."""
    return decider.decide(ctx, tool, name, args, preview, tail)


def bypass_gate(name: str, args: dict, preview: dict = None, note: str = None) -> bool:
    """Bypass mode's gate: ordinary calls answer themselves, the irreversible
    ones still go to the user. Only shell can do the irreversible thing in one
    word; everything else that writes is bounded by what it writes to, and the
    file tool already puts what it deletes in the Trash."""
    why = shell_safety.destructive(str(args.get("command") or "")) if name == "shell" else ""
    if not why: return True
    return ask_permission(name, args, preview, note or f"bypass keeps these: {why}")


def tool_ordinal(session) -> int:
    """How many tool results the conversation already holds: the index the next
    one is filed under."""
    return sum(1 for r in session.records() if r.get("t") == "tool")


DETAIL_CHARS = 20000


def _shrink(value, limit: int = DETAIL_CHARS):
    """Cap the strings in a stored payload. The detail view exists because the
    wire strips output, but a 5MB build log still should not cross as one line."""
    if isinstance(value, str):
        if len(value) <= limit: return value
        return value[:limit] + f"\n... [{len(value) - limit} more characters]"
    if isinstance(value, dict): return {k: _shrink(v, limit) for k, v in value.items()}
    if isinstance(value, list): return [_shrink(v, limit) for v in value]
    return value


def tool_detail(session, index: int) -> dict:
    """One call in full, arguments and result, read from the records. Images stay
    as blob refs: the point is to read what happened, not to move a megabyte."""
    calls, results = [], []
    for r in session.records():
        if r.get("t") == "assistant": calls.extend(r.get("calls") or [])
        elif r.get("t") == "tool": results.append(r)
    if not 0 <= index < len(results):
        # Not "error": that key means a failed envelope everywhere else, and a
        # call that was rewound away is absent, not broken.
        return {"index": index, "total": len(results), "unavailable": "no such call"}
    rec = results[index]
    # Positional, since every call yields one tool record in order, but checked
    # by name so a drifted history shows no arguments rather than another call's.
    call = calls[index] if index < len(calls) and calls[index].get("name") == rec.get("name") else {}
    return {"index": index, "total": len(results), "name": rec.get("name", ""), "ts": rec.get("ts", ""),
            "args": _shrink(call.get("args") or {}), "result": _shrink(rec.get("result") or {})}


def _detail_now(env):
    """tool.detail answered on the reader thread: the main loop is inside
    generate() for a whole turn, which for "what did that call return" is an
    hour too late. Reads only, so it is safe beside a run."""
    try: want = int((env.data or {}).get("index", -1))
    except (TypeError, ValueError): want = -1
    ipc.reply(env, "detail", tool_detail(sess, want))


ipc.direct["tool.detail"] = _detail_now


def _list_now(env):
    """The `/`, `$` and `@` menus, on the reader thread because queued for the main loop
    they came up empty for the whole length of a run. Everything here reads what is already
    loaded and never writes. model.list asks the provider over the network, so it stays out.
    """
    data = env.data or {}
    action = data.get("action")
    try:
        if action == "command.list":
            ipc.reply(env, "commands", {"commands": inline.listing()})
        elif action == "skill.list":
            ipc.reply(env, "skills", {"skills": skills.listing("user")})
        elif action == "fs.list":
            root = sess.meta.get("cwd") or ctx.get("cwd") or os.getcwd()
            ipc.reply(env, "files", fs_listing(root, data.get("dir", "")))
        elif action == "provider.list":
            ipc.reply(env, "providers", {"providers": config.listing(),
                                         "vision": config.vision_helper()[0], "vision_model": config.vision_helper()[1],
                                         "sighted": config.sighted()})
        elif action == "session.list":
            ipc.reply(env, "sessions", {"sessions": store.list_sessions(), "active": sess.id})
    except Exception as e:
        ipc.reply(env, "status", {"state": "error", "error": str(e)})


for _action in ("command.list", "skill.list", "fs.list", "provider.list", "session.list"):
    ipc.direct[_action] = _list_now


def replay(session, env):
    """Re-send a reopened conversation as the same events the live run sent, so a
    frontend redraws it with the code it already has. Nothing here carries a
    `status`: a replay is not a run, and a clock for it would time the redraw.
    """
    p = providers.get(session.provider)
    tools_seen = 0
    for r in session.records():
        t = r.get("t")
        if t == "user":
            # A record the worker marked as a note is runtime text it put in front of the
            # model, not a line the person typed: drawn as the notice it was, or a
            # reopened conversation puts words in their mouth. The tag in the text is
            # for the records written before that mark existed -- a turn carrying a
            # runtime block was never typed by a person.
            text = r.get("text") or ""
            if r.get("note") or "<todo_status>" in text:
                ipc.reply(env, "token", {"state": "notice", "token": text})
                continue
            # Names, never payloads: shipping the base64 back puts megabytes on the wire.
            data = {"state": "user", "token": r.get("text", "")}
            if r.get("images"):
                data["images"] = [{"name": i.get("name") or "image"} for i in r["images"]]
            ipc.reply(env, "token", data)
        elif t == "recall":
            # Shown, not hidden: runtime text put in front of the model is the user's to see.
            ipc.reply(env, "token", {"state": "notice", "token": r.get("text", "")})
        elif t == "assistant":
            if r.get("thinking"):
                ipc.reply(env, "token", {"state": "thinking", "token": r["thinking"]})
            if r.get("content"):
                ipc.reply(env, "token", {"state": "content", "token": r["content"]})
            if r.get("calls"):
                ipc.reply(env, "token", {"state": "tool_calls", "token": p.native_calls(r["calls"])})
        elif t == "tool":
            ipc.reply(env, "token", {"state": "tool_output", "token": r.get("name", ""),
                                     "result": r.get("result", {}), "index": tools_seen})
            tools_seen += 1


# The last name sent, so the same one is not sent twice. None rather than "",
# because "no name yet" is worth saying once: after a swap the title is stale.
_last_title = None


def announce_title(env=None):
    """Tell the frontend what this conversation is called, if that changed.
    Replied to when there is an envelope to answer, sent unprompted when the
    namer finishes on its own thread."""
    global _last_title
    title = sess.meta.get("title", "")
    if title == _last_title: return
    _last_title = title
    data = {"state": "session_title", "title": title,
            "source": sess.meta.get("title_source", ""), "session_id": sess.id}
    if env is not None: ipc.reply(env, "status", data)
    else: ipc.send("status", data)


# Set when a turn is cancelled, spent on the next one. It has to travel between
# turns: it is a user message, and the user is about to send one of their own.
pending_note = ""


def projects_data(**extra) -> dict:
    """The tree's project payload, one shape on every projects reply: the
    promoted dirs, the pin order, the custom names and icons."""
    names, icons = store.list_project_details()
    return {"projects": store.list_projects(), "pinned": store.list_pinned(),
            "names": names, "icons": icons, **extra}


def drop_empty(s):
    """A session that recorded nothing leaves nothing behind. Anything with a
    record, even just the user's message from a run that never answered, stays."""
    if not s._records:
        store.delete(s.id)


def _size(text: str) -> float:
    """`400k`, `250000`, `40%` -> a number. 0 when it is none of those.

    A share comes back under 1 and a count comes back over it, which is the one
    distinction the caller needs: one field means both, and which it means comes
    off its size rather than from a second spelling of it.
    """
    s = (text or "").strip().lower().replace("_", "").replace(",", "")
    try:
        if s.endswith("%"):
            v = float(s[:-1]) / 100
            return v if 0 < v < 1 else 0
        mult = 1
        if s.endswith("k"): mult, s = 1_000, s[:-1]
        elif s.endswith("m"): mult, s = 1_000_000, s[:-1]
        v = float(s) * mult
        return v if v >= 1 else 0
    except ValueError:
        return 0


def fold_progress(ev: dict):
    """A fold's own events, for a fold that is not running inside a round.

    `/compact` runs on the main loop with no generate() around it, so there is no
    outbox to drain and nothing would reach the screen while a model call runs.
    Sent straight out instead, and the state is set to "running" first: a status
    line is only drawn while the frontend thinks a turn is in flight, so without
    that the fold would be happening invisibly.
    """
    stage = ev.get("stage", "")
    ipc.send("status", {"state": "compacting", "stage": stage,
                        "folded": ev.get("folded", 0), "freed": ev.get("freed", 0),
                        # A frontend that only reads `status` still knows a turn is
                        # in flight, so it keeps its status line up.
                        "status": "running" if stage == "start" else ""})


def fold_now() -> str:
    """Fold the conversation down this instant and say what it bought.

    A forced fold ignores the trigger: the point of typing `/compact` is to stop
    waiting on a limit, possibly the one just moved. It goes through the same
    `_compact_now` the round loop uses, so there is one implementation of a fold
    and not a second one here that drifts from it -- which is what happened first
    time, with this function quietly keeping its own copy of the arithmetic.
    """
    global messages
    # Treating the conversation as over the line: the target is half of what is in
    # hand, so the fold aims below it and leaves room rather than landing on it.
    here = compact.tokens(messages)
    if not here:
        return "compact: nothing here worth folding yet"
    # The one place the emit goes in for a fold with no round around it.
    ctx["emit"] = fold_progress
    try:
        said = _compact_now(messages, used=here, limit=max(here // 2, 1), ctx=ctx)
    finally:
        ctx.pop("emit", None)
        # The line has to come down again whether or not the fold worked.
        ipc.send("status", {"state": "compacting", "stage": "done"})
    return said or "compact: nothing here worth folding yet"


def inline_command(env, text: str):
    """Act on the inline /commands in a chat message. True when a blocking command
    (provider, model, vision, effort) ran: its result is the whole answer and the turn stops.
    Otherwise the text still to send, read-only commands stripped. Writes through the globals.
    """
    global SETTINGS, messages
    res = inline.parse(text)
    if res.blocking:
        for name, arg in res.blocking:
            try:
                notice = _run_inline(name, arg)
            except ValueError as e:
                notice = f"{name}: {e}"
            if notice:
                # No run state claimed: nothing is generating, so a frontend
                # timing this would be timing a loop that does not exist.
                ipc.reply(env, "token", {"state": "notice", "token": notice})
        return True
    for name, arg in res.parallel:
        try:
            notice = _run_inline(name, arg)
        except ValueError as e:
            notice = f"{name}: {e}"
        if notice:
            ipc.reply(env, "token", {"state": "notice", "token": notice})
    return res.text


def _run_inline(name, arg) -> str:
    """Run one inline command, returning the notice line to show the user."""
    global SETTINGS, messages
    if name == "provider":
        if not arg:
            return f"provider: {SETTINGS['provider']} - {SETTINGS.get('model', 'no model')}"
        SETTINGS = config.settings(config.use(arg))
        sess.set_provider(SETTINGS["provider"])
        messages = sess.messages()
        return f"provider -> {SETTINGS['provider']} - {SETTINGS.get('model', 'no model')}"
    if name == "model":
        if not arg:
            return f"model: {SETTINGS.get('model', 'no model')} on {SETTINGS['provider']}"
        config.update(SETTINGS["provider"], model=arg)
        SETTINGS = config.settings()
        sess.set_model(arg)
        # A model swap can turn vision off; rebuild history or the next request
        # fails on the screenshots it still holds.
        messages = sess.messages()
        return f"model -> {arg}"
    if name == "vision":
        if not arg:
            return f"vision: {config.vision_helper()[0] or 'default - whichever can see'}"
        config.set_vision_helper(arg)
        return f"vision -> {arg}"
    if name == "effort":
        if not arg:
            return f"effort: {sess.effort}"
        if reason := config.effort_block(arg):
            return f"effort: {reason}"
        sess.set_effort(arg)
        config.set_default_effort(sess.effort)
        return f"effort -> {sess.effort}"
    if name == "compact":
        # Three jobs, one word. With no argument it folds now; with a size it sets
        # the limit the conversation folds at; with a share it sets that instead,
        # because a limit in tokens means something different on a 256k model than
        # on a 1M one and both spellings are honest about what they are.
        if not arg:
            return fold_now()
        want = arg.strip().lower()
        if want in ("off", "never", "none"):
            config.set_compact(0)
            return "compact: off - the conversation will not fold on its own"
        if want in ("auto", "default", "ratio"):
            config.set_compact(None)
            window = config.context_window(SETTINGS["provider"], SETTINGS.get("model", ""))
            limit = compact.trigger({**SETTINGS, "effort": sess.effort})
            if not limit:
                return "compact: automatic, but this model's window is unknown"
            # Say the actual rule rather than "automatic": on this window it is a
            # number, and a number is what someone can check against /context.
            share = round(compact.ratio(window) * 100) if window else 0
            return f"compact -> automatic: {share}% of the window ({limit:,} tokens here)"
        n = _size(want)
        if not n: return f"compact: {arg!r} is not a size (400k, 250000, 40%)"
        config.set_compact(n)
        limit = compact.trigger({**SETTINGS, "effort": sess.effort})
        # A count is a count; a share is a share. Formatted from what was typed, so
        # "400k" is not echoed back as "400,000.0".
        said = f"over {int(n):,} tokens" if n > 1 else f"at {round(n * 100)}% of the window"
        return f"compact -> {said} ({limit:,} tokens here)" if limit else f"compact -> {said}"
    if name == "context":
        r = context.report(sess, SETTINGS, ctx, messages=messages, last=LAST_TURN)
        left = f" ({r['free']} left of {r['window']})" if r.get("window") else ""
        kind = "measured" if r.get("measured") else "estimated"
        return f"context: {r['used']} tokens filling the window{left} \u00b7 {kind}"
    if name == "usage":
        t = usage.rollup(0)
        tot = t.get("total") or {}
        # The cache share, when any round reported one: without it the input figure reads as
        # every prompt at full price, which is what made this command worth distrusting.
        cached = tot.get("cache") or 0
        share = f" \u00b7 {round(cached * 100 / tot['in'])}% cached" if cached and tot.get("in") else ""
        return (f"usage: {t.get('sessions', 0)} sessions \u00b7 {tot.get('rounds', 0)} rounds \u00b7 "
                f"{tot.get('in', 0)} in / {tot.get('out', 0)} out tokens{share}")
    if name == "version":
        # The build the worker is running out of, which is not always the one a
        # frontend was built from: both read the same tree, and only one of them
        # is rebuilt when the other's source moves.
        return f"version {version.stamp()} \u00b7 `cuacode doctor` compares it with origin"
    return None


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


while True:
    # The frontend is gone: pipe EOF or reparented to launchd. Mirrors `stop`:
    # background work killed first, then the session committed, empty dropped.
    if ipc.eof() or os.getppid() == 1:
        JOBS.kill_all()
        sess.commit()
        drop_empty(sess)
        sys.exit(0)
    # Names that finished while the loop was busy, applied here rather than on
    # the thread that produced them, so meta.json and stdout have one writer.
    for finished in naming.drain():
        naming.apply(finished, sess)
    announce_title()

    # Peer mail for an idle agent: nobody is at the prompt, so a message would sit in
    # the mailbox until the person pressed enter. Shaped like the cmd a frontend sends,
    # so it goes through the same door and becomes an ordinary turn. Skipped while a run
    # is in flight, where the round seam owns delivery instead.
    if not ipc.cancelled() and not ipc.running:
        for mail in peers.as_cmds(sess.id): ipc.inbox.put(mail)

    for env in ipc.poll():
        if env.type == "terminal":
            ipc.reply(env, "status", {"terminal": ipc.terminal_info})
            ipc.terminal_info = env.data
            ctx = terminal_ctx(ipc.terminal_info)
            # The boot session predates this envelope, so its frontend and cwd
            # are stamped here; neither writes, so no folder is born yet.
            sess.set_frontend(ctx.get("term_program") or "")
            sess.set_cwd(ctx.get("cwd") or "")
            continue
        if env.type != "cmd": continue
        action = env.data.get("action")
        if action == "stop":
            # Background work first: the threads are daemons, but a shell job
            # holds a real process group that outlives us unless it is killed.
            JOBS.kill_all()
            sess.commit()
            drop_empty(sess)
            ipc.reply(env, "status", {"state": "stopped"})
            sys.exit(0)
        elif action == "cancel":
            # The run itself was already stopped by the flag IPC set on the
            # reader thread; this is only the acknowledgement.
            ipc.reply(env, "status", {"state": "cancel_ack"})
        elif action == "background":
            # Same shape as cancel: the flag was set on the reader thread while
            # this loop was still inside generate(), so the call has already moved.
            ipc.reply(env, "status", {"state": "background_ack"})
        elif action == "background.list":
            ipc.reply(env, "jobs", {"jobs": [j.brief() for j in JOBS.list(env.data.get("state") or "all")]})
        elif action == "background.kill":
            jid = env.data.get("job", "")
            ipc.reply(env, "jobs", {"killed": JOBS.kill(jid), "job": jid,
                                    "jobs": [j.brief() for j in JOBS.list()]})
        elif action == "permission.mode":
            mode = (env.data.get("mode") or "ask").lower()
            if mode not in ("ask", "auto", "bypass", "off"): mode = "ask"
            PERM_MODE = mode
            ipc.reply(env, "status", {"state": "permission", "mode": mode})
        elif action == "tool.detail":
            # Normally answered on the reader thread; this catches one that
            # arrived before that.
            _detail_now(env)
        elif action == "context.report":
            # Read off the prompt, the tool schemas and the history in memory: the
            # readout itself costs no tokens.
            try:
                ipc.reply(env, "context", context.report(sess, SETTINGS, ctx,
                                                         messages=messages, last=LAST_TURN))
            except Exception as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})

        elif action == "usage.report":
            # The same rollup --usage prints. Read from the meta files, so a
            # hundred conversations cost a hundred small json reads.
            try: days = int(env.data.get("days") or 0)
            except (TypeError, ValueError): days = 0
            rep = usage.rollup(days)
            # This conversation's own, including the rounds not yet committed.
            rep["session"] = {"id": sess.id, **(usage.of_records(sess.records()) or {})}
            ipc.reply(env, "usage", rep)

        elif action in ("skill.list", "command.list", "fs.list", "session.list", "provider.list"):
            _list_now(env)

        # Project list and edits: directories the user promoted, persisted in
        # projects.json. Every reply carries the whole payload.
        elif action == "project.list":
            ipc.reply(env, "projects", projects_data())
        elif action == "project.add":
            try:
                d = store.add_project(env.data.get("dir", ""))
                ipc.reply(env, "projects", projects_data(added=d))
            except ValueError as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})
        elif action == "project.rename":
            # Cosmetics only: the group key stays the basename.
            try:
                store.rename_project(env.data.get("dir", ""),
                                     env.data.get("name", ""), env.data.get("icon", ""))
                ipc.reply(env, "projects", projects_data())
            except ValueError as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})
        elif action == "project.remove":
            try:
                gone = store.remove_project(env.data.get("dir", ""))
                ipc.reply(env, "projects", projects_data(removed=gone))
            except ValueError as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})
        elif action == "project.pin":
            try:
                on = bool(env.data.get("on", True))
                store.pin_project(env.data.get("name", ""), on)
                ipc.reply(env, "projects", projects_data())
            except ValueError as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})
        elif action == "project.move":
            # One slot up or down inside the section the group sits in: pin
            # order for pinned groups, promotion order for the rest.
            try:
                try: delta = int(env.data.get("delta") or 0)
                except (TypeError, ValueError): delta = 0
                store.move_project(env.data.get("name", ""), env.data.get("dir", ""), delta)
                ipc.reply(env, "projects", projects_data())
            except ValueError as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})

        elif action == "provider.use":
            try:
                SETTINGS = config.settings(config.use(env.data.get("name", "")))
                # Records are canonical, so the history rebuilds in the new
                # dialect and the model comes with the provider.
                sess.set_provider(SETTINGS["provider"])
                sess.set_model(SETTINGS.get("model", ""))
                messages = sess.messages()
                ipc.reply(env, "status", {"state": "provider", **settings_fields(sess),
                                          "msg_count": len(messages)})
            except ValueError as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})

        elif action == "provider.set":
            try:
                name = env.data.get("name", "")
                config.update(name, model=env.data.get("model"), key=env.data.get("api_key"),
                              vision=env.data.get("vision"), params=env.data.get("params"),
                              effort_map=env.data.get("effort_map"),
                              model_params=env.data.get("model_params"))
                if name == SETTINGS.get("provider"):
                    SETTINGS = config.settings()
                    if env.data.get("model"): sess.set_model(env.data["model"])
                    # A model swap can turn vision off, and a history holding
                    # screenshots fails every request after. Records keep the blobs.
                    messages = sess.messages()
                    ipc.reply(env, "status", settings_fields(sess))
                # Reply from listing(), never the request: echoing env.data puts the key in the log.
                ipc.reply(env, "providers", {"providers": config.listing(),
                                             "vision": config.vision_helper()[0], "vision_model": config.vision_helper()[1],
                                             "sighted": config.sighted()})
            except ValueError as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})

        elif action == "model.list":
            # Asked of the provider, not kept in a table here: the catalogs rotate. A
            # provider that will not answer says so.
            try:
                name = env.data.get("name") or SETTINGS["provider"]
                p = providers.new(name)
                p.setup(config.api_key(name))
                found = p.models() if hasattr(p, "models") else None
                known = config.entry(name)
                ipc.reply(env, "models", {
                    "provider": name, "models": found or [],
                    "listed": found is not None,
                    "active": config.model_for(name),
                    # What is already known, so the picker can mark a blind model
                    # without asking the provider once per row.
                    "blind": sorted({m for m, caps in (known.get("model_caps") or {}).items()
                                     if "vision" not in caps} | set(known.get("blind_models") or []))})
            except Exception as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})

        elif action == "vision.use":
            # A role, not a provider setting: set here rather than through
            # provider.set. Validated on the spot, or it fails later in a tool call.
            try:
                config.set_vision_helper(env.data.get("name", ""), env.data.get("model"))
                ipc.reply(env, "providers", {"providers": config.listing(),
                                             "vision": config.vision_helper()[0], "vision_model": config.vision_helper()[1],
                                             "sighted": config.sighted()})
            except ValueError as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})

        elif action == "session.effort":
            # Session state, not provider state: the depth belongs to this
            # conversation and travels with it on reload.
            try:
                # Refused rather than approximated: every other rung lands on the
                # nearest thing the model has, but "off" has to mean off.
                if reason := config.effort_block(env.data.get("effort", "")):
                    ipc.reply(env, "status", {"state": "error", "error": reason})
                else:
                    sess.set_effort(env.data.get("effort", ""))
                    # ...and remembered as the level the next conversation opens on.
                    config.set_default_effort(sess.effort)
                    ipc.reply(env, "status", {"state": "effort", "effort": sess.effort,
                                              "session_id": sess.id})
            except ValueError as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})

        elif action == "session.new":
            # The effort carries over: a fresh chat is not a request to go back
            # to whatever the provider does by default.
            recall.forget(sess.id)
            instructions.forget(sess.id)
            LAST_TURN.clear()
            # Committed before it goes: a backgrounded subagent holds this session's bound
            # add_spend, and whatever it has billed so far has nowhere else to land.
            sess.commit()
            drop_empty(sess)
            sess = Session.create(provider=SETTINGS["provider"], model=SETTINGS.get("model", ""),
                                  effort_level=sess.effort, frontend=ctx.get("term_program") or "")
            messages = sess.messages()
            # The boot-time assignment again: this is a different session object, and
            # a /compact after a resume must fold the one that is now live.
            ctx["session"] = sess
            naming.set_live(sess)
            peers.become(sess.id)
            ipc.reply(env, "status", {"state": "session", "session_id": sess.id,
                                      **settings_fields(sess), "msg_count": len(messages)})
            # Saying the empty name is the point: the frontend is showing the one
            # that just went.
            announce_title(env)
        elif action == "session.load":
            try:
                loaded = Session.open(env.data.get("id", ""))
                loaded.restore_tool_state()
                LAST_TURN.clear()
                # Same reason as session.new: spend still arriving from a backgrounded
                # subagent is this session's, and this is its last door out.
                sess.commit()
                drop_empty(sess)
                sess, messages = loaded, loaded.messages()
                naming.set_live(sess)
                # The pid follows the session: a peer has to stop believing a worker
                # is holding the conversation it just navigated away from.
                peers.become(sess.id)
                # Status first, so a frontend can clear what it was showing
                # before the replayed conversation starts arriving.
                ipc.reply(env, "status", {"state": "session", "session_id": sess.id,
                                          **settings_fields(sess), "msg_count": len(messages)})
                announce_title(env)
                replay(sess, env)
                ipc.reply(env, "token", {"state": "done", "token": "done", "status": "done",
                                         "msg_count": len(messages)})
            except (FileNotFoundError, ValueError, KeyError) as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})
        elif action == "session.delete":
            try:
                sid = env.data.get("id", "")
                ipc.reply(env, "status", {"state": "deleted" if store.delete(sid) else "error", "session_id": sid})
            except ValueError as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})
        elif action == "session.archive":
            # The trash can, not the delete: out of every listing, back by
            # moving the folder in.
            try:
                sid = env.data.get("id", "")
                ipc.reply(env, "status", {"state": "archived" if store.archive(sid) else "error", "session_id": sid})
            except ValueError as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})
        elif action == "session.pin":
            # The reply is a fresh sessions list, so the frontend re-stamps its pins.
            try:
                sid = env.data.get("id", "")
                on = bool(env.data.get("on", True))
                if sid == sess.id: sess.set_pinned(on)
                else: store.pin_session(sid, on)
                ipc.reply(env, "sessions", {"sessions": store.list_sessions(), "active": sess.id})
            except (ValueError, FileNotFoundError) as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})
        elif action == "session.rename":
            # A name set by hand is stamped `user`, which is what stops the namer
            # renaming it back later: the auto title never overwrites a chosen one.
            try:
                sid = env.data.get("id", "")
                title = (env.data.get("title") or "").strip()
                if not title: raise ValueError("a name needs something in it")
                if sid == sess.id:
                    sess.set_title(title, "user")
                    announce_title(env)
                else:
                    title = store.rename_session(sid, title)
                    ipc.reply(env, "status", {"state": "session_title", "title": title,
                                              "source": "user", "session_id": sid})
            except (ValueError, FileNotFoundError) as e:
                ipc.reply(env, "status", {"state": "error", "error": str(e)})
        elif action == "chat":
            # Config is re-read every turn: the user edits it by hand, and a
            # model changed behind our back must not survive until restart.
            before = (SETTINGS.get("vision", True), SETTINGS.get("model", ""))
            SETTINGS = config.settings()
            now = (SETTINGS.get("vision", True), SETTINGS.get("model", ""))
            if now != before:
                # The history was rendered for the old answer, so it is rebuilt
                # before anything is sent.
                sess.set_model(now[1])
                messages = sess.messages()
            text = env.data.get("text", "")
            # A blocking inline command is the whole answer and stops the turn; a
            # read-only one is shown and the rest still goes. Neither reaches the model.
            _inline = inline_command(env, text)
            if _inline is True or (isinstance(_inline, str) and not _inline.strip()):
                # Nothing is generating and nothing is about to, which only this side
                # knows. Run state goes in `status`, not `state` (what a payload is).
                ipc.reply(env, "status", {"status": "idle", **settings_fields(sess)})
                continue
            if isinstance(_inline, str):
                text = _inline
            # A peer message that took this door reads as one from the moment the turn is
            # built, not after: it has no user record, so the peer record below is the only
            # thing the model's turn is filed under.
            peer_tag = env.data.get("peer") or None
            # What the user attached: [{"name", "b64"}]. Only the payload reaches
            # the model; the filename is kept on the record for the frontends.
            attached = [a for a in (env.data.get("images") or [])
                        if isinstance(a, dict) and a.get("b64")]
            # A model that cannot see is told, not sent an image it will 400 on, and the
            # attachment is dropped from the record, or it returns in the history forever.
            if attached and not config.can_see(SETTINGS["provider"]):
                names = ", ".join(a.get("name") or "image" for a in attached)
                ipc.reply(env, "token", {"state": "notice", "status": "running",
                                         "token": f"{len(attached)} image(s) not sent ({names}): "
                                                  f"{SETTINGS.get('model') or SETTINGS['provider']} cannot see images"})
                attached = []
            # Asked before the record is written, because "is this the opening
            # message" stops being answerable the moment it is one.
            opening = not any(r.get("t") == "user" for r in sess.records())
            # Built by the provider adapter, the same call replay makes: a user
            # turn with a picture on it is three different shapes.
            turn = providers.get(sess.provider).user_message(text, [a["b64"] for a in attached])
            # The names ride in the message itself rather than as a record:
            # derived from the attachments, so replay rebuilds the same line.
            providers.append_user_text(turn, providers.attachment_note(attached))
            messages.append(turn)
            # A peer's words are the peer's record and are not drawn as the person's line;
            # anything else here was typed by the person, so it is theirs.
            if not peer_tag:
                sess.add_user(text, attached)
            # Set inside add_user, said now: the loop does not come back around
            # until the turn ends, and the title would be for the wrong thing.
            announce_title(env)
            # Where the turn is happening, set before recall runs and before the
            # tool descriptions are rebuilt, so the memory index is this project's.
            sess.set_cwd(ctx.get("cwd") or "")
            memory.set_cwd(ctx.get("cwd") or "")
            memory.set_apps([ctx.get("frontmost_app") or ""])
            # Notes ride beside the user's message, not as a second one: two in a
            # row is a 400 on anthropic. None of these may fail a turn.
            try:
                notes = recall.block(text, sid=sess.id, path=ctx.get("cwd") or "",
                                     apps=[ctx.get("frontmost_app") or ""])
            except Exception:
                notes = []
            # Project documentation: names and sizes only, at most twice in a
            # conversation.
            try:
                docs = instructions.docs_block(text, sid=sess.id, path=ctx.get("cwd") or "",
                                               first=opening)
            except Exception:
                docs = ""
            # A message that opens with /<skill> loads that skill's instructions.
            try:
                skill_block = skills.invocation(text)
            except Exception:
                skill_block = ""
            # The interrupt note rides the same rail and is spent as it is used:
            # repeated, it would read as a fresh stop each time.
            # A peer message that started this turn: recorded as a peer's words rather
            # than as the person's, and drawn with the buddy mark. One place for both
            # paths, because a wake turn takes the same door a typed message does.
            if peer_tag:
                sess.add_peer(peer_tag.get("from", ""), peer_tag.get("name", ""),
                              peer_tag.get("from_cwd", ""), peer_tag.get("text", ""))
                shown = "peer message from %s\n%s" % (peer_tag.get("name", "?"),
                                                      peer_tag.get("body", ""))
                ipc.reply(env, "token", {"state": "peer", "token": shown, "status": "running"})
            # The block itself still rides beside the message for the model; on a peer
            # turn the peer state already said all of it, and drawing it twice is a row
            # nobody can read.
            for extra in (pending_note, *notes, docs, skill_block):
                if not extra: continue
                sess.add_recall(extra)
                providers.append_user_text(messages[-1], extra)
                ipc.reply(env, "token", {"state": "notice", "token": extra, "status": "running"})
            pending_note = ""
            ipc.reply(env, "status", {"type": "chat_received"})
            # Peer mail rides in as a chat envelope. The tag says who wrote it and what
            # the model was given, since the turn's own text carries the lead-in (which
            # nobody typed) rather than the block the provider saw. The record and the row
            # are already done above, so this only answers the mid-turn case.
            peer_from = (peer_tag or {}).get("from")
            # This turn's session goes in here: there are three `sess =` sites and
            # a tool only reads it inside the run.
            ctx["session_dir"] = str(sess.dir)
            # How hard this conversation is thinking, for anything that starts a
            # model of its own from inside a tool call.
            ctx["effort"] = sess.effort
            # The door ask_user opens. Per turn, not at boot, because it closes over
            # this process's ipc and nothing else; passed through ctx because a tool
            # handler gets ctx and cannot import the worker it is running inside.
            ctx["ask_user"] = ask_user
            # The session itself, so the round loop can fold the conversation it is
            # about to send. An object, not an import: generate() is also the
            # subagent loop, which runs for callers with no session at all.
            ctx["session"] = sess
            ipc.begin_run()
            round_mark = len(messages)
            # Held until the assistant record it belongs to is written and cleared
            # per round, so a round the provider never reported on carries nothing.
            round_cost = {}
            try:
                # The provider half of the settings is account-wide and the
                # effort is this conversation's, so they are joined per turn.
                for chunk in generate(API_KEY=config.api_key(SETTINGS["provider"]), messages=messages,
                                      ctx=ctx, settings={**SETTINGS, "effort": sess.effort},
                                      # One segment each: the shipped prefix stays
                                      # stable, so the cache survives; machine facts last.
                                      system=[s for s in (sess.system, instructions.user_block(),
                                                          skills.always_block(),
                                                          environment.block(ctx, SETTINGS, sess)) if s],
                                      cancelled=ipc.cancelled,
                                      # A callable, not a list: generate() asks at
                                      # the one point a user message is legal.
                                      steer=ipc.take_steer,
                                      # Which mailbox the round seam drains. Not
                                      # `ctx["session_dir"]`, which a subagent shares:
                                      # a subagent is not the agent peers are talking to.
                                      peer_sid=sess.id,
                                      # The event itself: generate() clears it as
                                      # it consumes it, one press one call.
                                      detach=ipc.background,
                                      # Who the calls that need asking go to, per
                                      # mode.
                                      ask=(ask_permission if PERM_MODE in ("ask", "auto")
                                           else bypass_gate if PERM_MODE == "bypass" else None),
                                      auto=auto_decide if PERM_MODE == "auto" else None):
                    typ = chunk.get("type")
                    if typ == "round":
                        # generate() takes its own rollback mark right before
                        # yielding this, so the two stay in step.
                        round_mark = len(messages)
                        round_cost = {}
                        # Committed before the mark is taken: pending state is
                        # consistent here, so a force-kill loses only this round.
                        sess.commit()
                        sess.round_start()
                    elif typ == "assistant":
                        sess.add_assistant(chunk.get("thinking", ""), chunk.get("content", ""),
                                           chunk.get("tool_calls") or [], usage=round_cost)
                        # A step just closed: its thought and the calls it made are
                        # the whole input a small model needs to name it, and the
                        # calls are about to run, which is the room the naming has.
                        stepname.maybe_name(ipc.send, chunk.get("thinking", ""),
                                            chunk.get("tool_calls") or [], text)
                    elif typ == "cancelled":
                        # No rewind: the loop closed the round off and every
                        # tool_call has an interrupted result, so messages is resumable.
                        messages = chunk.get("messages", messages)
                        sess.commit()
                        pending_note = chunk.get("note", "")
                        ipc.reply(env, "token", {"state": "cancelled", "token": "cancelled", "status": "cancelled", "msg_count": len(messages)})
                    elif typ == "failed":
                        # A dropped connection, handled like a cancel: the round
                        # was closed off, so what streamed is kept.
                        messages = chunk.get("messages", messages)
                        sess.commit()
                        pending_note = chunk.get("note", "")
                        ipc.reply(env, "token", {"state": "error", "token": chunk.get("error", ""),
                                                 "status": "error", "kept": True,
                                                 "msg_count": len(messages)})
                    elif typ == "compacting":
                        # A fold is a model call: the screen would otherwise sit on
                        # "thinking" for however long the summarizer takes, with
                        # nothing saying what the wait is for. Its own state, so the
                        # frontend can draw it quietly and differently.
                        ipc.reply(env, "status", {"state": "compacting",
                                                  "stage": chunk.get("stage", ""),
                                                  "folded": chunk.get("folded", 0),
                                                  "freed": chunk.get("freed", 0)})
                    elif typ == "retry":
                        # No "status" key, same as rate: a frontend must not be
                        # knocked off thinking or tooling by it.
                        ipc.reply(env, "status", {"state": "retry", "attempt": chunk.get("attempt", 0),
                                                  "of": chunk.get("of", 0), "secs": chunk.get("secs", 0),
                                                  "error": chunk.get("error", "")})
                    elif typ == "rate":
                        ipc.reply(env, "status", {"state": "rate", **context.rate_fields(chunk)})
                    elif typ == "usage":
                        # Its own status rather than a field on the next token: a
                        # tool loop can run for minutes without one.
                        turn = turn_fields(chunk)
                        # Kept for the assistant record about to be written: the
                        # counts arrive one yield before the turn they describe.
                        round_cost = usage.stamp(turn, SETTINGS["provider"], SETTINGS.get("model", ""))
                        fields = {**context_fields(chunk.get("usage")), **turn}
                        if fields:
                            ipc.reply(env, "status", {"state": "usage", **fields})
                    elif typ == "done":
                        messages = chunk.get("messages", messages)
                        sess.commit()
                        # After the commit, so the namer reads a turn count that
                        # has happened. At most three times per session, own thread.
                        naming.maybe_start(sess)
                        ipc.reply(env, "token", {"state": "done", "token": "done", "status": "done",
                                                 "msg_count": len(messages),
                                                 **context_fields(chunk.get("usage")), **turn_fields(chunk)})
                    elif typ == "tool_calls":
                        ipc.reply(env, "token", {"state": "tool_calls", "token": chunk.get("text"), "status": "tooling"})
                    elif typ == "background":
                        # Its own state rather than folded into tool_output: one
                        # closes a row, this one says it will not close for a while.
                        job = chunk.get("job", {})
                        ipc.reply(env, "token", {"state": "background", "token": job.get("job", ""),
                                                 "result": job, "status": "tooling"})
                    elif typ == "peer":
                        # Only a message that arrived is recorded. One that went out is not:
                        # the call that sent it is already on the record, and a second entry
                        # would double it. `peer_in` tells the two apart -- the other name on
                        # a sent row is the recipient's, so it cannot.
                        if chunk.get("peer_in"):
                            sess.add_peer(peer_from, chunk.get("peer_name", ""),
                                          (peer_tag or {}).get("cwd", ""),
                                          (peer_tag or {}).get("text", ""))
                        shown = chunk.get("text", "")
                        if shown:
                            ipc.reply(env, "token", {"state": "peer", "token": shown, "status": "running"})
                    elif typ == "peer_waiting":
                        # A message that has landed but has not been handed to the model yet:
                        # the round it arrived in is still running, and the words themselves
                        # go in at that round's own seam. So there is nothing to record and
                        # nothing to draw as a line of the conversation -- the record happens
                        # once, when the message is delivered, and this only says the round
                        # that is running has something waiting behind it.
                        ipc.reply(env, "token", {"state": "peer_waiting", "status": "tooling",
                                                 "from_peer": chunk.get("from_peer", ""),
                                                 "peer_name": chunk.get("peer_name", ""),
                                                 "result": {"n": chunk.get("n", 1)}})
                    elif typ == "notice":
                        # Recorded by the peer branch above, or as the user's turn below --
                        # marked as a note, so reopening the conversation draws runtime
                        # text as the notice it was and not as something the person typed.
                        if not peer_from:
                            sess.add_user(chunk.get("text", ""), chunk.get("images") or None,
                                          note=True)
                        # Only the runtime half goes to the frontend: it drew the
                        # user's line when they typed it.
                        shown = chunk.get("show", chunk.get("text", ""))
                        if shown:
                            ipc.reply(env, "token", {"state": "notice", "token": shown, "status": "running"})
                    elif typ == "tool_output":
                        result = chunk.get("result", {})
                        name = chunk.get("name")
                        # Taken before the record is added: this is the index the
                        # call is filed under, and what tool.detail is asked for.
                        idx = tool_ordinal(sess)
                        sess.add_tool(name, result)
                        if name in ("screenshot", "photos"):
                            # Keep the IPC payload tiny - images live in messages, not the wire
                            count = result.get("count", 1)
                            ipc.reply(env, "token", {"state": "tool_output", "token": name, "result": {"n": count}, "index": idx, "status": "tooling"})
                        elif name in ("WebFetch", "agent", "workflow", "skill", "describe_image") and isinstance(result.get("result"), dict):
                            # The body stays out of the drawn row: tens of
                            # thousands of characters, and messages already hold it.
                            r = result["result"]
                            body = r.get("output")
                            brief = {k: v for k, v in r.items() if k not in ("output", "log", "text", "instructions", "description")}
                            if isinstance(r.get("instructions"), str): brief["chars"] = len(r["instructions"])
                            if isinstance(body, str): brief["chars"] = len(body)
                            elif isinstance(body, dict): brief["fields"] = sorted(body)[:8]
                            if isinstance(r.get("log"), list): brief["steps"] = len(r["log"])
                            ipc.reply(env, "token", {"state": "tool_output", "token": name,
                                                     "result": {"result": brief}, "index": idx, "status": "tooling"})
                        elif name == "shell" and isinstance(result.get("result"), dict):
                            # Same reason as images: command output belongs in the
                            # messages, not in a row a frontend has to swallow.
                            r = result["result"]
                            brief = {k: v for k, v in r.items() if k not in ("stdout", "stderr")}
                            brief["bytes"] = len(r.get("stdout", "")) + len(r.get("stderr", ""))
                            ipc.reply(env, "token", {"state": "tool_output", "token": name, "result": {"result": brief}, "index": idx, "status": "tooling"})
                        else:
                            ipc.reply(env, "token", {"state": "tool_output", "token": name, "result": result, "index": idx, "status": "tooling"})
                    else:
                        ipc.reply(env, "token", {"state": typ, "token": chunk.get("text"), "status": "running"})
            except Exception as e:
                # A turn that dies mid-round leaves an assistant message whose
                # tool_calls have no results, which the provider rejects. Rewind both.
                del messages[round_mark:]
                sess.rewind()
                sess.commit()
                ipc.reply(env, "token", {"state": "error", "token": str(e), "status": "error"})
            finally:
                # Anything typed during the turn that the loop never reached goes
                # back on the inbox and is answered as an ordinary next message.
                ipc.end_run()
    time.sleep(0.001)
