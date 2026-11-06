"""One worker: the conversation it is holding, and the one turn that runs on it.

Everything a turn needs is here, and nothing about the wire is. `main.py` reads
envelopes and hands them to `handler/commands.py`, which calls these methods.
That split is the whole point: a turn is `worker.turn(text)` yielding the events
it produced, so a caller with no frontend -- a test, or a consumer in another
repo driving the process -- gets the same turn a person gets, and can say what it
wants about the run (which tools, how much it may spend) without reaching in here.

Before, both halves were in main.py and the turn was assembled inline out of six
module-level names (`SETTINGS`, `sess`, `messages`, `ctx`, `LAST_TURN`,
`PERM_MODE`), three of which were rebound from five different branches. Which
conversation a function was talking about therefore depended on which branch had
run last, and nothing said so.
"""
import os, time

from handler import config, context, environment, gate, peers, usage, version
from handler.agent import compact, inline, stepname
from handler.agent.main import generate, _compact_now
from tools.loader import registry
from handler.agent import providers
from handler.session import store
from handler.session.main import Session
from integrations.instructions import loader as instructions
from integrations.memory import loader as memory, naming, recall
from integrations.skills import loader as skills
from integrations.permissions import decider


class Ctx(dict):
    """Envelope fields, plus the attribute spellings some tools read off it."""

    self_identity = property(lambda s: s.get("frontmost_app"))
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
    # The directory, when the frontend sent none. A frontend on another machine has
    # no path to send: the wire may cross to a box, where its cwd names nothing, so
    # it leaves the field out. The worker is standing in a directory either way, and
    # this is that one -- a session that cannot say where it happened is a session
    # recall and app-scoped memory cannot find again.
    if not info.get("cwd"):
        info["cwd"] = os.getcwd()
    if os.name == "nt" and not info.get("frontmost_app"):
        try:
            from tools.screenshot._capture_windows import active_app_name
            info["frontmost_app"] = active_app_name()
        except Exception:
            pass
    return Ctx(info)


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


class Worker:
    """One process's agent. One conversation at a time, swapped by session.new/load."""

    def __init__(self, ipc, detached: bool = False):
        self.ipc = ipc
        # `main.py --detached` is a worker **hosted** on a machine by something that
        # outlives any frontend (tmux, through `Host::hosted`). Nothing is attached to
        # it by definition - that is what detach means - so it may never stop to ask a
        # person. `bypass` is not enough: its whole rule is to stop for the
        # irreversible calls and put those to a person, and that person is exactly who
        # is not there.
        self.detached = detached
        self.ctx = terminal_ctx(ipc.terminal_info)
        self.settings = config.settings()
        # A session exists from boot: its folder is born with the first record and an
        # unused one is dropped on exit. Effort starts at the account default.
        self.sess = Session.create(provider=self.settings["provider"],
                                   model=self.settings.get("model", ""),
                                   effort_level=config.default_effort())
        self.messages = self.sess.messages()
        # Set at boot and on every swap: /compact runs at the prompt with no turn in
        # flight, and a fold that could only find the session mid-turn would answer
        # "nothing to fold" to the one command that exists to fold on demand.
        self.ctx["session"] = self.sess
        # ask | auto | bypass | off, for calls a tool declares need asking; a frontend
        # re-declares it once it is up.
        self.mode = "off" if detached else config.permission_mode()
        # The last round's cost, read by /context: one round, not a running total.
        self.last_turn = {}
        # Set when a turn is cancelled, spent on the next one. It has to travel between
        # turns: it is a user message, and the user is about to send one of their own.
        self.pending_note = ""
        # The last title sent, so the same one is not sent twice. None rather than "",
        # because "no name yet" is worth saying once: after a swap the title is stale.
        self._last_title = None
        # The pid a peer checks to tell a slow agent from a dead one. meta.json's
        # `updated` only moves when a round commits, so without this an agent ten
        # minutes into a build is indistinguishable from one that was killed.
        peers.become(self.sess.id)
        # Tools run in this process, so the memory tool retitles the object the loop
        # holds, not the file underneath it.
        naming.set_live(self.sess)
        # The ready line carries the session id, so a frontend can tie its live row to
        # the record committed later, plus the mode and settings for the header.
        ipc.send("status", {"state": "ready", "session_id": self.sess.id,
                            "mode": self.mode, **self.settings_fields()})

    # ---- what the header says ----

    def settings_fields(self) -> dict:
        """Who is answering, how hard it thinks, how much window it has. Read off the
        session, not the config: a conversation reopened from disk answers with its own
        provider and model.
        """
        out = {"provider": self.sess.provider, "model": self.sess.meta.get("model", ""),
               "effort": self.sess.effort}
        # No window beats a window somebody invented.
        if window := config.context_window(self.sess.provider, self.sess.meta.get("model", "")):
            out["context_max"] = window
        return out

    def remember_turn(self, chunk: dict) -> dict:
        """What the round that just generated cost, remembered for /context."""
        fields = context.turn_fields(chunk)
        self.last_turn.clear()
        self.last_turn.update({"input": (chunk.get("usage") or {}).get("input", 0), **fields})
        return fields

    def context_fields(self, use: dict) -> dict:
        """The context readout. Spent tokens only, and the window only when the
        model's size is actually known.

        Asked about the session's own pair, not the account's: after a reload those
        are two different conversations, and the worker used to answer with whatever
        model the config named.
        """
        use = use or {}
        used = use.get("input", 0) + use.get("output", 0)
        if not used: return {}
        out = {"context_used": used}
        if window := config.context_window(self.sess.provider, self.sess.meta.get("model", "")):
            out["context_max"] = window
            out["context_left"] = max(window - used, 0)
        return out

    def announce_title(self, env=None):
        """Tell the frontend what this conversation is called, if that changed.
        Replied to when there is an envelope to answer, sent unprompted when the
        namer finishes on its own thread."""
        title = self.sess.meta.get("title", "")
        if title == self._last_title: return
        self._last_title = title
        data = {"state": "session_title", "title": title,
                "source": self.sess.meta.get("title_source", ""), "session_id": self.sess.id}
        if env is not None: self.ipc.reply(env, "status", data)
        else: self.ipc.send("status", data)

    def drop_empty(self):
        """A session that recorded nothing leaves nothing behind. Anything with a
        record, even just the user's message from a run that never answered, stays."""
        if not self.sess._records:
            store.delete(self.sess.id)

    # ---- who answers a permission ----

    def ask_user(self, question: str, options: list, detail: str = "", n: int = 1, of: int = 1) -> dict:
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
        reply = self.ipc.call("permission", payload, timeout=None, stop=self.ipc.cancelled)
        data = (reply.data or {}) if reply else {}
        # Three answer shapes, newest first: a pick, a typed answer, and the plain
        # allow/deny an unmigrated frontend sends back.
        if data.get("choice"): return {"choice": str(data["choice"])}
        if (data.get("answer") or "").strip(): return {"answer": str(data["answer"]).strip()}
        if reply is None: return {"error": "no answer: cancelled, or the frontend never showed it"}
        if data.get("allow") is False: return {"error": "declined"}
        return {"choice": options[0]} if options else {"answer": "yes"}

    def ask_permission(self, tool, name: str, args: dict, preview: dict = None,
                       note: str = None) -> bool:
        """Put one call to the user. timeout=None keeps the question open as long as
        they need; a cancel ends the run it belonged to. preview is what the call
        would do; note is why the auto gate flagged it.

        Takes the tool as well as its name, the same as `auto_decide`: the two are
        asked the same question about the same call, and a gate that could not see
        the tool had to be told which tools were dangerous from outside.
        """
        payload = {"name": name, "kind": "permission", "args": args}
        if preview: payload["preview"] = preview
        if note: payload["note"] = note
        reply = self.ipc.call("permission", payload, timeout=None, stop=self.ipc.cancelled)
        return bool(reply and (reply.data or {}).get("allow"))

    def auto_decide(self, tool, name: str, args: dict, preview: dict, tail: str, ctx) -> tuple:
        """The auto gate: a configured small model decides. Returns (allow, reason)."""
        return decider.decide(ctx, tool, name, args, preview, tail)

    def bypass_gate(self, tool, name: str, args: dict, preview: dict = None,
                    note: str = None) -> bool:
        """Bypass mode's gate: ordinary calls answer themselves, the irreversible
        ones still go to the user.

        Which calls those are is the tool's own answer (`Tool.irreversible`), not a
        list of tool names kept here: this said `if name == "shell"`, so a second
        tool that could wipe something would have been waved through by a gate
        written to catch exactly that.
        """
        try:
            why = tool.irreversible(args) if tool is not None and tool.irreversible else ""
        except Exception as e:
            # A tool that cannot answer the question is asked about. The other
            # direction is a gate that fails open exactly when the tool is confused
            # about what it is about to do.
            why = f"the tool could not say ({e})"
        if not why: return True
        return self.ask_permission(tool, name, args, preview, note or f"bypass keeps these: {why}")

    def gate(self):
        """Who a call that needs asking goes to, for the mode in force.

        Returned rather than applied, so a mode with nobody to ask answers None and
        the run proceeds instead of hanging on a question nothing will ever answer.
        """
        if self.mode in ("ask", "auto"): return self.ask_permission
        if self.mode == "bypass": return self.bypass_gate
        return None

    # ---- the conversation's place on disk ----

    def set_cwd_here(self, arg: str) -> str:
        """Where this conversation stands, and the one place that decides it.

        Before the first record only: the worker stamps the directory on the session
        and every tool the run calls runs there, so moving it under a live conversation
        is not a setting, it is a lie about where the work has been happening. Checked
        against **this** process's filesystem, which on a box is that box's: a path that
        does not exist where the work happens is not a location.

        Every answer says which of the three it is: where it stands (and that it can
        still move), where it now stands (and that this conversation starts there),
        or why it cannot move and what to do instead. A path is read against the
        directory the conversation stands in, not against the worker's own launch
        directory, because that is the directory the person is looking at.

        One function for both doors - the `/cwd` command and the `session.cwd` a picker
        sends - because two spellings of "set the directory" is how one of them forgets
        that a conversation that has started cannot move.
        """
        here = self.sess.meta.get("cwd") or self.ctx.get("cwd") or os.getcwd()
        started = bool(self.sess.records())
        if not arg:
            if started:
                return f"cwd: {here} - this conversation has started, so it is fixed"
            return f"cwd: {here}"
        # `~` first: it is the only spelling that does not depend on where the
        # process was launched, and a relative path is a path from the conversation's
        # own directory rather than from wherever the worker happened to start.
        want = os.path.abspath(os.path.join(here, os.path.expanduser(arg)))
        if not os.path.isdir(want):
            return f"cwd: {arg} is not a directory on this machine"
        if started:
            return (f"cwd: {here} - this conversation has started, so it is fixed; "
                    "open a new session to work somewhere else")
        self.sess.set_cwd(want)
        self.ctx["cwd"] = want
        return f"cwd: {want} - this conversation starts there"

    # ---- the inline /commands ----

    def inline(self, text: str) -> tuple:
        """Act on the /commands in a chat message.

        `(text still to send, notices to show)`. The text is None when a blocking
        command (provider, model, vision, effort) ran: its result is the whole
        answer and the turn stops. A read-only command's notice is shown and the
        rest of the message still goes to the model.
        """
        res = inline.parse(text)
        names = res.blocking or res.parallel
        notices = []
        for name, arg in names:
            try: notice = self.run_inline(name, arg)
            except ValueError as e: notice = f"{name}: {e}"
            if notice: notices.append(notice)
        return (None if res.blocking else res.text), notices

    def run_inline(self, name, arg) -> str:
        """Run one inline command, returning the notice line to show the user."""
        if name == "provider":
            if not arg:
                return f"provider: {self.settings['provider']} - {self.settings.get('model', 'no model')}"
            self.settings = config.settings(config.use(arg))
            self.sess.set_provider(self.settings["provider"])
            self.messages = self.sess.messages()
            return f"provider -> {self.settings['provider']} - {self.settings.get('model', 'no model')}"
        if name == "model":
            if not arg:
                return f"model: {self.settings.get('model', 'no model')} on {self.settings['provider']}"
            config.update(self.settings["provider"], model=arg)
            self.settings = config.settings()
            self.sess.set_model(arg)
            # A model swap can turn vision off; rebuild history or the next request
            # fails on the screenshots it still holds.
            self.messages = self.sess.messages()
            return f"model -> {arg}"
        if name == "vision":
            if not arg:
                return f"vision: {config.vision_helper()[0] or 'default - whichever can see'}"
            config.set_vision_helper(arg)
            return f"vision -> {arg}"
        if name == "effort":
            if not arg:
                return f"effort: {self.sess.effort}"
            if reason := config.effort_block(arg):
                return f"effort: {reason}"
            self.sess.set_effort(arg)
            config.set_default_effort(self.sess.effort)
            return f"effort -> {self.sess.effort}"
        if name == "cwd":
            return self.set_cwd_here(arg)
        if name == "compact":
            # Three jobs, one word. With no argument it folds now; with a size it sets
            # the limit the conversation folds at; with a share it sets that instead,
            # because a limit in tokens means something different on a 256k model than
            # on a 1M one and both spellings are honest about what they are.
            if not arg:
                return self.fold_now()
            want = arg.strip().lower()
            if want in ("off", "never", "none"):
                config.set_compact(0)
                return "compact: off - the conversation will not fold on its own"
            if want in ("auto", "default", "ratio"):
                config.set_compact(None)
                window = config.context_window(self.settings["provider"], self.settings.get("model", ""))
                limit = compact.trigger({**self.settings, "effort": self.sess.effort})
                if not limit:
                    return "compact: automatic, but this model's window is unknown"
                # Say the actual rule rather than "automatic": on this window it is a
                # number, and a number is what someone can check against /context.
                share = round(compact.ratio(window) * 100) if window else 0
                return f"compact -> automatic: {share}% of the window ({limit:,} tokens here)"
            n = _size(want)
            if not n: return f"compact: {arg!r} is not a size (400k, 250000, 40%)"
            config.set_compact(n)
            limit = compact.trigger({**self.settings, "effort": self.sess.effort})
            # A count is a count; a share is a share. Formatted from what was typed, so
            # "400k" is not echoed back as "400,000.0".
            said = f"over {int(n):,} tokens" if n > 1 else f"at {round(n * 100)}% of the window"
            return f"compact -> {said} ({limit:,} tokens here)" if limit else f"compact -> {said}"
        if name == "context":
            r = context.report(self.sess, self.settings, self.ctx, messages=self.messages,
                               last=self.last_turn)
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

    def fold_now(self) -> str:
        """Fold the conversation down this instant and say what it bought.

        A forced fold ignores the trigger: the point of typing `/compact` is to stop
        waiting on a limit, possibly the one just moved. It goes through the same
        `_compact_now` the round loop uses, so there is one implementation of a fold
        and not a second one here that drifts from it -- which is what happened first
        time, with this function quietly keeping its own copy of the arithmetic.
        """
        # Treating the conversation as over the line: the target is half of what is in
        # hand, so the fold aims below it and leaves room rather than landing on it.
        here = compact.tokens(self.messages)
        if not here:
            return "compact: nothing here worth folding yet"
        # The one place the emit goes in for a fold with no round around it.
        self.ctx["emit"] = self._fold_progress
        try:
            said = _compact_now(self.messages, used=here, limit=max(here // 2, 1), ctx=self.ctx)
        finally:
            self.ctx.pop("emit", None)
            # The line has to come down again whether or not the fold worked.
            self.ipc.send("status", {"state": "compacting", "stage": "done"})
        return said or "compact: nothing here worth folding yet"

    def _fold_progress(self, ev: dict):
        """A fold's own events, for a fold that is not running inside a round.

        `/compact` runs on the main loop with no generate() around it, so there is no
        outbox to drain and nothing would reach the screen while a model call runs.
        Sent straight out instead, and the state is set to "running" first: a status
        line is only drawn while the frontend thinks a turn is in flight, so without
        that the fold would be happening invisibly.
        """
        stage = ev.get("stage", "")
        self.ipc.send("status", {"state": "compacting", "stage": stage,
                                 "folded": ev.get("folded", 0), "freed": ev.get("freed", 0),
                                 # A frontend that only reads `status` still knows a turn is
                                 # in flight, so it keeps its status line up.
                                 "status": "running" if stage == "start" else ""})

    # ---- sessions ----

    def new_session(self, effort: str = "") -> None:
        """A fresh conversation, and everything that has to follow the swap."""
        recall.forget(self.sess.id)
        instructions.forget(self.sess.id)
        # Nothing from the last conversation is read in this one. load_session gets
        # the same treatment from gate.restore, which clears before it seeds.
        gate.reset()
        self.last_turn.clear()
        # Committed before it goes: a backgrounded subagent holds this session's bound
        # add_spend, and whatever it has billed so far has nowhere else to land.
        self.sess.commit()
        self.drop_empty()
        self.sess = Session.create(provider=self.settings["provider"],
                                   model=self.settings.get("model", ""),
                                   effort_level=effort or self.sess.effort,
                                   frontend=self.ctx.get("term_program") or "")
        self.messages = self.sess.messages()
        self.ctx["session"] = self.sess
        naming.set_live(self.sess)
        peers.become(self.sess.id)
        self.pending_note = ""

    def load_session(self, sid: str) -> None:
        """Open a conversation from disk and put it behind everything."""
        loaded = Session.open(sid)
        # The edit gate follows the conversation: a file it read before the reload is
        # still read, unless it moved on disk meanwhile.
        gate.restore(loaded.meta.get("read_files") or [])
        self.last_turn.clear()
        # Same reason as new_session: spend still arriving from a backgrounded
        # subagent is this session's, and this is its last door out.
        self.sess.commit()
        self.drop_empty()
        self.sess, self.messages = loaded, loaded.messages()
        self.ctx["session"] = self.sess
        naming.set_live(self.sess)
        # The pid follows the session: a peer has to stop believing a worker
        # is holding the conversation it just navigated away from.
        peers.become(self.sess.id)
        self.pending_note = ""

    # ---- the turn ----

    def _result_of(self, stop: str, since: int = 0) -> dict:
        """The turn as data: what the model last said, why it ended, how big it got.

        Every frontend ignores this, and a program on the other end of the pipe
        needs it: concatenating content tokens and inferring a stop reason from
        which ones arrived is not a result. `stop` is one of `done`, `cancelled`,
        `max_rounds`, `deadline`, `error` -- the reason this turn is over, said by
        the only side that knows.
        """
        text = ""
        for r in reversed(self.sess.records()[since:]):
            if r.get("t") == "assistant":
                text = r.get("content") or ""
                break
        return {"text": text, "stop": stop, "messages": len(self.messages),
                "session": self.sess.id}

    def turn(self, text: str, peer_tag: dict = None, attached: list = None,
             policy: dict = None):
        """Run one turn, yielding `(envelope type, payload)` for what it produced.

        Events rather than envelopes: this has no id to answer on, and a caller with
        no frontend has nothing to answer. `commands.chat` pairs each one with the
        envelope that asked for it.

        `policy` is the second door's half of a turn, and every part of it is
        optional: `tools` is the allowlist (absent means everything installed),
        `max_rounds` a ceiling on model calls, `max_seconds` a ceiling on the wall
        clock. The two ceilings end the run the way a cancel does -- closed off and
        resumable -- and the last event says which one it was, because a program
        cannot read that off a stream of text.
        """
        policy = policy or {}
        allow = policy.get("tools")
        try: max_rounds = int(policy.get("max_rounds") or 0)
        except (TypeError, ValueError): max_rounds = 0
        try: max_seconds = float(policy.get("max_seconds") or 0)
        except (TypeError, ValueError): max_seconds = 0
        deadline = time.monotonic() + max_seconds if max_seconds > 0 else 0.0
        stopped = "done"
        # Where this turn starts in the record list: the result is what this turn
        # said, and after a failed request that is nothing -- not the last thing
        # the conversation said an hour ago.
        since = len(self.sess.records())

        def outcome(stop: str) -> dict:
            """This turn's result. Named apart from `result`, which the tool branch
            binds to a tool's own result: one assignment in a function makes the name
            local for all of it."""
            return self._result_of(stop, since)
        # Config is re-read every turn: the user edits it by hand, and a model changed
        # behind our back must not survive until restart.
        before = (self.settings.get("vision", True), self.settings.get("model", ""))
        self.settings = config.settings()
        now = (self.settings.get("vision", True), self.settings.get("model", ""))
        if now != before:
            # The history was rendered for the old answer, so it is rebuilt before
            # anything is sent.
            self.sess.set_model(now[1])
            self.messages = self.sess.messages()
        # A blocking inline command is the whole answer and stops the turn; a
        # read-only one is shown and the rest still goes. Neither reaches the model.
        text, notices = self.inline(text)
        for notice in notices:
            # No run state claimed: nothing is generating, so a frontend timing this
            # would be timing a loop that does not exist.
            yield "token", {"state": "notice", "token": notice}
        if text is None or not text.strip():
            # Nothing left to send, and nothing is generating or about to: that is the
            # whole answer. Run state goes in `status`, not `state` (what a payload
            # is). A read-only command typed on its own (`/version`) lands here, and
            # sending the empty remnant to the model was a turn nobody asked for.
            yield "status", {"status": "idle", "result": outcome("idle"),
                             **self.settings_fields()}
            return
        # An allowlist naming a tool that is not installed is a typo, not a request
        # for a smaller toolbox: an agent with no tools would spend a turn finding
        # out, and report it as the model's failure rather than the caller's.
        if allow is not None:
            known = set(registry())
            missing = [t for t in allow if t not in known]
            if missing:
                said = f"unknown tool(s): {', '.join(missing)}"
                # In `result.text` as well as in the event: a program reads the
                # result, and an empty one beside a reason it never saw is the exact
                # shape of a bug report that says "it returned nothing".
                yield "token", {"state": "error", "status": "error", "token": said,
                                "result": {**outcome("error"), "text": said}}
                return
        # A model that cannot see is told, not sent an image it will 400 on, and the
        # attachment is dropped from the record, or it returns in the history forever.
        attached = [a for a in (attached or []) if isinstance(a, dict) and a.get("b64")]
        if attached and not config.can_see(self.settings["provider"]):
            names = ", ".join(a.get("name") or "image" for a in attached)
            yield "token", {"state": "notice", "status": "running",
                            "token": f"{len(attached)} image(s) not sent ({names}): "
                                     f"{self.settings.get('model') or self.settings['provider']} cannot see images"}
            attached = []
        # Asked before the record is written, because "is this the opening message"
        # stops being answerable the moment it is one.
        opening = not any(r.get("t") == "user" for r in self.sess.records())
        # Built by the provider adapter, the same call replay makes: a user turn with
        # a picture on it is three different shapes.
        turn = providers.get(self.sess.provider).user_message(text, [a["b64"] for a in attached])
        # The names ride in the message itself rather than as a record: derived from
        # the attachments, so replay rebuilds the same line.
        providers.append_user_text(turn, providers.attachment_note(attached))
        self.messages.append(turn)
        # A peer's words are the peer's record and are not drawn as the person's line;
        # anything else here was typed by the person, so it is theirs.
        if not peer_tag:
            self.sess.add_user(text, attached)
        # Set inside add_user, said now: the loop does not come back around until the
        # turn ends, and the title would be for the wrong thing.
        self.announce_title()
        # Where the turn is happening, set before recall runs and before the tool
        # descriptions are rebuilt, so the memory index is this project's.
        self.sess.set_cwd(self.ctx.get("cwd") or "")
        memory.set_cwd(self.ctx.get("cwd") or "")
        memory.set_apps([self.ctx.get("frontmost_app") or ""])
        # Notes ride beside the user's message, not as a second one: two in a row is a
        # 400 on anthropic. None of these may fail a turn.
        try:
            notes = recall.block(text, sid=self.sess.id, path=self.ctx.get("cwd") or "",
                                 apps=[self.ctx.get("frontmost_app") or ""])
        except Exception:
            notes = []
        # Project documentation: names and sizes only, at most twice in a conversation.
        try:
            docs = instructions.docs_block(text, sid=self.sess.id, path=self.ctx.get("cwd") or "",
                                           first=opening)
        except Exception:
            docs = ""
        # A message that opens with /<skill> loads that skill's instructions.
        try:
            skill_block = skills.invocation(text)
        except Exception:
            skill_block = ""
        # A peer message that started this turn: recorded as a peer's words rather than
        # as the person's, and drawn with the buddy mark. One place for both paths,
        # because a wake turn takes the same door a typed message does.
        if peer_tag:
            self.sess.add_peer(peer_tag.get("from", ""), peer_tag.get("name", ""),
                               peer_tag.get("from_cwd", ""), peer_tag.get("text", ""))
            shown = "peer message from %s\n%s" % (peer_tag.get("name", "?"), peer_tag.get("body", ""))
            yield "token", {"state": "peer", "token": shown, "status": "running"}
        # The block itself still rides beside the message for the model; on a peer turn
        # the peer state already said all of it, and drawing it twice is a row nobody
        # can read.
        for extra in (self.pending_note, *notes, docs, skill_block):
            if not extra: continue
            self.sess.add_recall(extra)
            providers.append_user_text(self.messages[-1], extra)
            yield "token", {"state": "notice", "token": extra, "status": "running"}
        self.pending_note = ""
        # The rewind floor for this turn. commit() resets it to 0, so a failure
        # before the first round was reached would have rewound past the user's own
        # message: gone from the session, still in `messages`, and the next turn would
        # treat the conversation as unopened.
        self.sess.round_start()
        yield "status", {"type": "chat_received"}
        # Peer mail rides in as a chat envelope. The tag says who wrote it and what the
        # model was given, since the turn's own text carries the lead-in (which nobody
        # typed) rather than the block the provider saw. The record and the row are
        # already done above, so this only answers the mid-turn case.
        peer_from = (peer_tag or {}).get("from")
        # This turn's session goes in here: the turn is the only place a tool reads it.
        self.ctx["session_dir"] = str(self.sess.dir)
        # How hard this conversation is thinking, for anything that starts a model of
        # its own from inside a tool call.
        self.ctx["effort"] = self.sess.effort
        # The door ask_user opens. Per turn, not at boot, because it closes over this
        # process's ipc and nothing else; passed through ctx because a tool handler
        # gets ctx and cannot import the worker it is running inside.
        self.ctx["ask_user"] = self.ask_user
        # The session itself, so the round loop can fold the conversation it is about
        # to send. An object, not an import: generate() is also the subagent loop,
        # which runs for callers with no session at all.
        self.ctx["session"] = self.sess
        ipc = self.ipc
        ipc.begin_run()
        round_mark = len(self.messages)
        # Held until the assistant record it belongs to is written and cleared per
        # round, so a round the provider never reported on carries nothing.
        round_cost = {}
        rounds = 0

        def out_of_time() -> bool:
            return bool(deadline) and time.monotonic() > deadline

        def stop_now() -> bool:
            """Called all over generate(), so a deadline is a cancel that costs
            nothing to add: the loop already checks this between chunks, between
            calls and inside its waits."""
            return ipc.cancelled() or out_of_time()

        # `None` until the loop object exists: everything from here to the first
        # chunk is inside the try, because beginning a run and then raising out of
        # argument evaluation would leave the run open for the rest of the process.
        gen = None
        try:
            gen = generate(API_KEY=config.api_key(self.settings["provider"]),
                           messages=self.messages, ctx=self.ctx,
                           settings={**self.settings, "effort": self.sess.effort},
                           # One segment each: the shipped prefix stays stable, so the
                           # cache survives; machine facts last.
                           system=[s for s in (self.sess.system, instructions.user_block(),
                                               skills.always_block(),
                                               environment.block(self.ctx, self.settings, self.sess)) if s],
                           cancelled=stop_now,
                           # A callable, not a list: generate() asks at the one point a
                           # user message is legal.
                           steer=ipc.take_steer,
                           # Which mailbox the round seam drains. Not `ctx["session_dir"]`,
                           # which a subagent shares: a subagent is not the agent peers
                           # are talking to.
                           peer_sid=self.sess.id,
                           # The event itself: generate() clears it as it consumes it,
                           # one press one call.
                           detach=ipc.background,
                           # The allowlist a program asked for, or everything installed.
                           allow=allow,
                           # Who the calls that need asking go to, per mode.
                           ask=self.gate(),
                           auto=self.auto_decide if self.mode == "auto" else None)
            for chunk in gen:
                typ = chunk.get("type")
                if typ == "round":
                    # Yielded before the request is made, so stopping here spends
                    # nothing. A budget is in rounds because that is what costs money,
                    # and a round that never opened is not one.
                    rounds += 1
                    if max_rounds and rounds > max_rounds:
                        stopped = "max_rounds"
                        break
                    # generate() takes its own rollback mark right before yielding
                    # this, so the two stay in step.
                    round_mark = len(self.messages)
                    round_cost = {}
                    # Committed before the mark is taken: pending state is consistent
                    # here, so a force-kill loses only this round.
                    self.sess.commit()
                    self.sess.round_start()
                elif typ == "assistant":
                    self.sess.add_assistant(chunk.get("thinking", ""), chunk.get("content", ""),
                                            chunk.get("tool_calls") or [], usage=round_cost)
                    # A step just closed: its thought and the calls it made are the
                    # whole input a small model needs to name it, and the calls are
                    # about to run, which is the room the naming has.
                    stepname.maybe_name(ipc.send, chunk.get("thinking", ""),
                                        chunk.get("tool_calls") or [], text)
                elif typ == "cancelled":
                    # No rewind: the loop closed the round off and every tool_call has
                    # an interrupted result, so messages is resumable.
                    self.messages = chunk.get("messages", self.messages)
                    self.sess.commit()
                    self.pending_note = chunk.get("note", "")
                    stopped = "deadline" if out_of_time() and not ipc.cancelled() else "cancelled"
                    yield "token", {"state": "cancelled", "token": "cancelled",
                                    "status": "cancelled", "msg_count": len(self.messages),
                                    "result": outcome(stopped)}
                elif typ == "failed":
                    # A dropped connection, handled like a cancel: the round was closed
                    # off, so what streamed is kept.
                    self.messages = chunk.get("messages", self.messages)
                    self.sess.commit()
                    self.pending_note = chunk.get("note", "")
                    stopped = "error"
                    yield "token", {"state": "error", "token": chunk.get("error", ""),
                                    "status": "error", "kept": True,
                                    "msg_count": len(self.messages),
                                    "result": outcome("error")}
                elif typ == "compacting":
                    # A fold is a model call: the screen would otherwise sit on
                    # "thinking" for however long the summarizer takes, with nothing
                    # saying what the wait is for. Its own state, so the frontend can
                    # draw it quietly and differently.
                    yield "status", {"state": "compacting", "stage": chunk.get("stage", ""),
                                     "folded": chunk.get("folded", 0), "freed": chunk.get("freed", 0)}
                elif typ == "retry":
                    # No "status" key, same as rate: a frontend must not be knocked off
                    # thinking or tooling by it.
                    yield "status", {"state": "retry", "attempt": chunk.get("attempt", 0),
                                     "of": chunk.get("of", 0), "secs": chunk.get("secs", 0),
                                     "error": chunk.get("error", "")}
                elif typ == "rate":
                    yield "status", {"state": "rate", **context.rate_fields(chunk)}
                elif typ == "usage":
                    # Its own status rather than a field on the next token: a tool loop
                    # can run for minutes without one.
                    fields = self.remember_turn(chunk)
                    # Kept for the assistant record about to be written: the counts
                    # arrive one yield before the turn they describe.
                    round_cost = usage.stamp(fields, self.settings["provider"],
                                             self.settings.get("model", ""))
                    out = {**self.context_fields(chunk.get("usage")), **fields}
                    if out:
                        yield "status", {"state": "usage", **out}
                elif typ == "done":
                    self.messages = chunk.get("messages", self.messages)
                    self.sess.commit()
                    # After the commit, so the namer reads a turn count that has
                    # happened. At most three times per session, own thread.
                    naming.maybe_start(self.sess)
                    yield "token", {"state": "done", "token": "done", "status": "done",
                                    "msg_count": len(self.messages),
                                    "result": outcome("done"),
                                    **self.context_fields(chunk.get("usage")), **self.remember_turn(chunk)}
                elif typ == "tool_calls":
                    yield "token", {"state": "tool_calls", "token": chunk.get("text"), "status": "tooling"}
                elif typ == "background":
                    # Its own state rather than folded into tool_output: one closes a
                    # row, this one says it will not close for a while.
                    job = chunk.get("job", {})
                    yield "token", {"state": "background", "token": job.get("job", ""),
                                    "result": job, "status": "tooling"}
                elif typ == "peer":
                    # Only a message that arrived is recorded. One that went out is not:
                    # the call that sent it is already on the record, and a second entry
                    # would double it. `peer_in` tells the two apart -- the other name on
                    # a sent row is the recipient's, so it cannot.
                    if chunk.get("peer_in"):
                        # From the chunk: a message that landed mid-turn has no
                        # `peer_tag` at all (a person started the turn), so reading
                        # the body from the tag recorded the peer's words as empty --
                        # and on a peer-started turn it recorded the *starting*
                        # message a second time under the wrong sender.
                        self.sess.add_peer(chunk.get("from_peer", ""), chunk.get("peer_name", ""),
                                           chunk.get("from_cwd", ""), chunk.get("body", ""))
                    shown = chunk.get("text", "")
                    if shown:
                        yield "token", {"state": "peer", "token": shown, "status": "running"}
                elif typ == "peer_waiting":
                    # A message that has landed but has not been handed to the model
                    # yet: the round it arrived in is still running, and the words
                    # themselves go in at that round's own seam. So there is nothing to
                    # record and nothing to draw as a line of the conversation -- the
                    # record happens once, when the message is delivered, and this only
                    # says the round that is running has something waiting behind it.
                    yield "token", {"state": "peer_waiting", "status": "tooling",
                                    "from_peer": chunk.get("from_peer", ""),
                                    "peer_name": chunk.get("peer_name", ""),
                                    "result": {"n": chunk.get("n", 1)}}
                elif typ == "notice":
                    # Recorded by the peer branch above, or as the user's turn below --
                    # marked as a note, so reopening the conversation draws runtime text
                    # as the notice it was and not as something the person typed.
                    if not peer_from:
                        self.sess.add_user(chunk.get("text", ""), chunk.get("images") or None,
                                           note=True)
                    # Only the runtime half goes to the frontend: it drew the user's
                    # line when they typed it.
                    shown = chunk.get("show", chunk.get("text", ""))
                    if shown:
                        yield "token", {"state": "notice", "token": shown, "status": "running"}
                elif typ == "tool_output":
                    result = chunk.get("result", {})
                    name = chunk.get("name")
                    # Taken before the record is added: this is the index the call is
                    # filed under, and what tool.detail is asked for.
                    idx = sum(1 for r in self.sess.records() if r.get("t") == "tool")
                    self.sess.add_tool(name, result)
                    # The row gets the tool's own brief; the body it leaves out is in
                    # the record and in `messages`, where the model reads it.
                    yield "token", {"state": "tool_output", "token": name,
                                    "result": chunk.get("wire", result),
                                    "index": idx, "status": "tooling"}
                else:
                    yield "token", {"state": typ, "token": chunk.get("text"), "status": "running"}
        except Exception as e:
            # A turn that dies mid-round leaves an assistant message whose tool_calls
            # have no results, which the provider rejects. Rewind both.
            del self.messages[round_mark:]
            self.sess.rewind()
            self.sess.commit()
            stopped = "error"
            yield "token", {"state": "error", "token": str(e), "status": "error",
                            "result": outcome("error")}
        finally:
            # Closed rather than dropped: a round budget stops in the middle of the
            # generator, and what ran on the way out (a stream's own cleanup, a
            # provider socket) has to run here and not at collection.
            # Its own guard: a generator whose cleanup raises must not be what decides
            # whether `end_run` happens, or a client waits on a turn that is over.
            if gen is not None:
                try: gen.close()
                except Exception: pass
            # generate() puts its own outbox here; left behind, the next subagent asked
            # to report progress would append into a list nobody reads again.
            self.ctx.pop("emit", None)
            # Anything typed during the turn that the loop never reached goes back on
            # the inbox and is answered as an ordinary next message.
            ipc.end_run()
        if stopped == "max_rounds":
            # Said as a done, with the reason: the rounds that ran are whole and the
            # conversation is resumable, so this is a ceiling reached, not a failure.
            # Emitted after the cleanup rather than inside it: a generator may not
            # yield while it is being closed.
            yield "token", {"state": "done", "token": "done", "status": "done",
                            "msg_count": len(self.messages),
                            "result": outcome("max_rounds"),
                            **self.settings_fields()}
