# Untangling CuaCode

Branch `refactor/untangle`, cut from `3aa875c`. Local commits only.

The target, from the brief: one agent behind two doors. A **chat** (a person,
steering, permission prompts, a transcript) and a **program** (no screen, nobody
to ask, structured events on a pipe, resumable, an allowlist of tools, a ceiling
on cost and time, and a result that comes back as data). The SDK is the second
door made official: a consumer in another repo, another language, that spawns the
worker and drives a turn without importing anything.

So every seam gets one question: **does this force a consumer outside the repo to
reach into `handler/`?** If it does, the seam is in the wrong place.

Part 1 is the problem list, as found, before anything changed. Part 2 is the
fixes, each written above the problems it solves. Part 3 is what is left on
purpose.

---

## Part 1 — the problems

### The door a consumer has to use

- `main.py:643-1262` is the whole wire: one `while True` with a 500-line
  `if/elif` over fifty actions. There is no list of the actions anywhere else, so
  a consumer discovers the protocol by reading the worker's source. Nothing can
  check a doc against it because there is nothing to check against.
- `main.py:282-283` registers six actions to answer on the reader thread, and
  `main.py:731` re-lists five of them for the main loop -- a second copy that has
  already drifted: `dir.list` is in the first list and missing from the second, so
  which actions work mid-run is decided by two lists that disagree.
- `main.py:1077-1099` assembles the turn inline, inside the `chat` branch: fifteen
  keyword arguments built from six module globals. Nothing outside main.py can run
  a turn, and the arguments a program would care about (`allow`, and a budget) are
  either unexposed (`allow`) or do not exist.
- There is no allowlist on the wire. `generate(allow=...)` exists
  (`handler/agent/main.py:461`) and only the subagent loop uses it.
- There is no ceiling on a run: no rounds, no seconds, no tokens. A program that
  spawns the worker gets a run that ends when the model decides.
- There is no result as data. A finished turn is a `done` envelope carrying
  `msg_count` and token counts (`main.py:1167-1169`); the answer itself is only
  the `content` tokens a consumer has had to concatenate itself. Nothing says
  *why* a run ended -- done, cancelled, budget, provider failure -- in one field.

### Module-level state other modules depend on without saying so

- `main.py:18` `SETTINGS`, `:76` `sess`, `:78` `messages`, `:72` `ctx`, `:119`
  `LAST_TURN`, `:347` `pending_note`, `:108` `PERM_MODE`. `sess` is rebound in
  three places (`:873`, `:895`), `messages` in six. Every function below the loop
  reads or writes them, so the order of the branches is a hidden precondition.
- `main.py:136` `context_fields` asks `config.context_window` about
  `SETTINGS["provider"]`/`SETTINGS["model"]`, while `settings_fields` (`:95`) asks
  about `sess.provider`/`sess.meta["model"]`. After `session.load` those are
  different conversations, so `/context` reports the account's window for someone
  else's model. This is a bug, not a style complaint.
- `main.py:347` `pending_note` is written in the `chat` branch and in the chunk
  loop, and read on the next turn. A cancel note that nothing consumed is carried
  into whatever message is typed next, including one that belongs to another
  session after `session.new` -- it is not cleared there.
- `handler/agent/main.py:471` writes `ctx["emit"]` onto the shared worker dict and
  never removes it (only `fold_now` pops it, `main.py:423`), so after the first
  turn every later consumer of `ctx` sees a stale outbox bound to a generator that
  has finished. Harmless today only because `outbox.append` on a dead list is a
  no-op.

### Copies of logic and values

- `home()/subdir/mkdir`, seven times, none taking a root:
  `integrations/skills/loader.py:55-58`, `integrations/subagents/loader.py:21-24`,
  `integrations/mcp/loader.py:19-22`, `integrations/memory/loader.py:45-48` and
  `:50-53`, `integrations/mcp/client.py:57-61`,
  `handler/agent/workflow.py:37-40`.
- "the bundled directory", two incompatible spellings: `Path(__file__).parent`
  (`integrations/skills/loader.py:14`, `integrations/subagents/loader.py:18`,
  `integrations/mcp/loader.py:15`) and
  `Path(__file__).resolve().parents[2] / "integrations" / ...`
  (`handler/agent/workflow.py:23`), which only agree because of where the two files
  happen to live.
- "bundled, then user, user wins", four copies: `integrations/skills/loader.py:87-94`,
  `integrations/subagents/loader.py:56-62`, `integrations/mcp/loader.py:47-48`,
  `handler/agent/workflow.py:125-130`.
- The skip rule, four spellings: `startswith(("_", "."))`
  (`skills/loader.py:90`), `startswith("_")` (`workflow.py:128`), a dotted-path
  check (`memory/loader.py:123`), and nothing at all
  (`subagents/loader.py:57` -- so a `.DS_Store` or `_draft.md` in
  `~/.cuacode/subagents` is loaded as an agent).
- `unknown X: {name!r} (have ...)`, four copies: `skills/loader.py:106`,
  `subagents/loader.py:67`, `mcp/loader.py:68`, `workflow.py:149`.
- `describe_image/main.py:36-41` `_images()` is a copy of
  `handler/agent/providers/base.py:157-163`.
- "ctx cwd if it is a directory, else home", three times:
  `tools/file/_common.py:27-29`, `tools/shell/main.py:21-30`,
  `handler/environment.py:105`.
- `tools/_safety/paths.py:46-52` `scratch_dir()` re-derives
  `Session.notebook` (`handler/session/main.py:69-74`) with its own fallback, and
  the comment says it mirrors it.
- `tools/file/_undo.py:19` repeats the same `~/.cuacode` fallback rule.
- `handler/agent/main.py:159-166` `_preview` and `:168-177` `_needs_ask` are two
  hooks asked of a tool with the same "a broken hook must fail the safe way" rule;
  `tools/loader.py:110-111` and `handler/agent/interrupt.py:43-51` then both turn a
  raised exception into `{"error": str(e)}`.

### A module reading another's internals

- `handler/session/main.py:307-340` reads the file tool's edit gate out of
  `sys.modules["_common"]` -- a *top-level* module that only exists because
  `tools/file/main.py:3` inserted its own directory on `sys.path`. `:313` calls
  `load_tools()` for the side effect of importing it, which re-execs all 28 tool
  `main.py` files. Rename `_common`, move the folder, or load the session without
  the tools, and `read_files` is silently empty forever: `meta.json` then says a
  session read no files, and the gate lets the next turn edit unseen content.
- `integrations/instructions/loader.py:103-104` reaches into the same
  `sys.modules["_common"]` for the same set, and returns `set()` when the import
  order happens to be different -- so "files already read" is suppressed or not
  depending on which module was imported first.
- `main.py:65-66` imports `tools.screenshot._capture_windows.active_app_name`: the
  worker depends on a tool's private platform module to fill in `frontmost_app`.
- `handler/context.py:177` imports `handler.agent.main.registry, _visible`: a
  readout module reaching into the loop's private helpers.
- `handler/agent/main.py:721` `any(c.name == "todo" ...)`, `:792`
  `call.name == "roster"`: the round loop knows two tools by name.
- `tools/todo/state.py:43` imports `handler.agent.subagent.depth` while
  `handler/agent/main.py:318` imports `tools.todo.state`: a cycle across the
  handler/tool boundary.
- `tools/_safety/rules.py:21` imports `handler.config` while
  `integrations/permissions/decider.py:52` imports `tools._safety.rules`.
- `handler/agent/main.py:266` `from handler.session import naming` -- there is no
  such module. The `except Exception: pass` swallows the ImportError, so the
  documented fallback for "a fold outside a turn" has never worked. A real bug
  wearing a fallback.
- `integrations/permissions/decider.py:16` imports config at module scope while
  `integrations/skills/loader.py:45-48` imports it late with a comment explaining
  the cycle it must avoid. Two conclusions about the same cycle in the same tree.

### Tool knowledge living outside the tools

- `main.py:1220-1244` decides how much of a tool's result crosses the wire with
  three hardcoded name lists: `("screenshot", "photos")` becomes a count,
  `("WebFetch", "agent", "workflow", "skill", "describe_image")` gets its body
  stripped, `"shell"` gets `stdout`/`stderr` stripped. A new image-returning tool
  ships its megabytes to every frontend until someone remembers this line.
- `main.py:196` `shell_safety.destructive(...) if name == "shell" else ""`: bypass
  mode's "irreversible" test is a name list of one, in the worker, about a tool.
- `tools/describe_image/main.py:57,59` hardcodes `"screenshot"` and `"photos"`.
- `main.py:71` `frontmost_app` probing is a Windows-only branch inside the generic
  context builder.

### Surplus: no reason of its own to change

- `handler/protocol.py:187-189` `poll_one` -- no caller.
- `handler/protocol.py:191-207` -- an example loop under `__main__` that models a
  protocol nothing uses.
- `integrations/skills/loader.py:81` `refresh: bool = True` -- never referenced in
  the body; it reads as cache control and is not.
- `integrations/subagents/loader.py:40`
  `tools=tools if tools is not None or "tools" not in meta else None` -- both
  branches yield `tools`, so the no-op contradicts its own comment.
- `integrations/mcp/loader.py:45` `include_disabled` -- no caller passes it.
- `integrations/memory/loader.py:42` `apps()` -- no caller.
- `tools/ask_user/main.py:21` `TIMEOUT = 1800` -- dead; the wait is `timeout=None`.
- `tools/file/Description.md`, `app_open`, `mouse_move`, `photos`: an `input:` key
  in the frontmatter that nothing reads (`tools/loader.py:71` uses the JSON schema).
- `main.py:31-34` `--detach <file>` -- a compat refusal for a flag that no
  frontend can still be sending. Kept: the comment's reason (a run that looks sent
  and never happens) is a real failure mode and the check costs three lines.

### Special cases, each one a requirement the design never accounted for

- `main.py:700` and `:108` both force `off` for a detached worker: two places that
  must agree about one rule.
- `main.py:700` `if mode not in ("ask", "auto", "bypass", "off"): mode = "ask"`
  duplicates `config.permission_mode`'s own validation.
- `handler/config.py:61` `if key == "permission"` inside the loop over `_SIDES`:
  one of three settings spelled differently, handled by an `if`.
- `handler/config.py:265-278` `_ask_caps` -- "only ollama can" answer
  capabilities, expressed as `hasattr(p, "capabilities")`.
- `integrations/mcp/client.py:315-316` rewrites the literal strings `"python"` and
  `"python3"` to `sys.executable`.
- `integrations/mcp/client.py:31` hardcodes `"version": "1.0.0"` in `CLIENT_INFO`
  while `handler/version.py` exists to compute the real stamp.
- `integrations/instructions/loader.py:154` a hardcoded word list
  (`repo`, `project`, `codebase`, ...) that decides whether the second docs pointer
  fires.
- `integrations/instructions/loader.py:167-179` `turn_no` counts *calls to
  `docs_block`*, and `MIN_GAP` compares against a counter `forget()` resets, so a
  resumed session gets two more pointers at the same content.
- `handler/agent/main.py:538` `MAX_FOLDS = 4` defined inside the round's inner
  `while True`.
- `handler/agent/main.py:801` `time.sleep(0.15)` between tool calls in a round:
  a pacing sleep that nothing documents as a rate limit.
- `tools/wait/main.py:4-6` returns the uncapped `seconds` while sleeping
  `min(seconds, 10)`: the result says a wait happened that did not.
- `tools/photos/main.py:31,44` raises while every other tool returns
  `{"error"}`; `tools/roster/main.py:62,67` returns `{"ok": False}` which is
  neither.
- `tools/screenshot/main.py:33` `getattr(ctx, "session_dir")` and four siblings
  (`app_open:13`, `shell:170`, `mcp:166`, `file/_undo.py:18`) rely on `Ctx`'s
  attribute properties; a plain dict ctx silently yields `None`, so the archive,
  the window-parking and the self-identity quietly stop happening instead of
  failing.
- `handler/agent/subagent.py:44` `tools: list = field(default_factory=list)` with
  the comment "[] is genuinely no tools", while the file loader turns an absent
  `tools:` into `None` = every tool. Two meanings for the same absence.

### The wire, as a contract

- `rust/fixtures/replay.jsonl` holds the event shapes, `rust/src/wire.rs` types
  them, and nothing holds the *command* shapes. `tests/test_worker_commands.py:66`
  documents by comment that a bare `chat` envelope is silently dropped and only
  `{"action": "chat"}` works -- the kind of thing a protocol document exists to
  say once.
- `handler/protocol.py:49-50` `Envelope.from_dict` defaults a malformed line to
  empty strings and an empty dict, so a consumer that sends `{"type":"cmd"}` with
  no `data` gets a command with no action rather than an error.
- `main.py:674` `if env.type != "cmd": continue` -- an envelope of a type the
  worker does not know is dropped without a word. That is correct for a stray
  `terminal`, and wrong for a consumer's typo.

---

## Part 2 — the fixes

### One convention for every file-backed integration

`handler/catalog.py` is new: `bundled(kind)`, `user_dir(kind)`, `draft(path)`,
`layered_files(kind, pattern)`, `layered_dirs(kind)`, `unknown(kind, name, have)`.
The five loaders call it and keep only their own parsing.

- home+subdir+mkdir: seven copies -> `catalog.user_dir`
  - `integrations/skills/loader.py:55-58`, `integrations/subagents/loader.py:21-24`,
    `integrations/mcp/loader.py:19-22`, `integrations/memory/loader.py:45-48` and
    `:50-53`, `integrations/mcp/client.py:57-61`, `handler/agent/workflow.py:37-40`
- the bundled directory, two spellings -> `catalog.bundled`
  - `Path(__file__).parent` in three loaders, `parents[2]/"integrations"` in the
    workflow one; they agreed by luck of layout
- bundled-then-user, four merge loops -> `catalog.layered_files` / `layered_dirs`
- the skip rule, four spellings -> `catalog.draft`, and subagents gain the rule it
  never had (a `_draft.md` there used to load as an agent and be offered to the model)
- `unknown X (have ...)`, four copies -> `catalog.unknown`

Deleted, because nothing could ever change them:

- `skills.load_skills(refresh=True)`, a parameter never read in the body
- `subagents._spec`'s `tools=tools if tools is not None or "tools" not in meta else None`,
  a no-op whose comment claimed the opposite of what it did
- `memory.apps()`, no caller
- `mcp.load_servers(include_disabled)`, no caller

Bug fixed on the way: `subagent.run` turned `tools=None` into `allow=[]`, so an
agent file saying `tools: "*"` got **no** tools, and the dataclass's own "None
means unrestricted" was unreachable. `run()` now passes `None` through.

### The edit gate has an owner instead of a `sys.modules` lookup

`handler/gate.py` is new: `read_files`, `seen`, `mark`, `forget`, `moved`,
`same_as_recorded`, `snapshot`, `restore`. It is process state with three
readers, so it lives where all three can import it by name.

- `handler/session/main.py:307-340` read it out of `sys.modules["_common"]` and
  called `load_tools()` purely to import all 28 tools so the module would exist.
  That is gone, with `_read_files`, `_unchanged` and `_seed_read_files`:
  `restore_tool_state` becomes `gate.restore(meta["read_files"])` and a commit
  calls `gate.snapshot()`.
- `integrations/instructions/loader.py:96-104` reached into the same module and
  silently returned `set()` when the import order differed. Now
  `gate.read_files`.
- `tools/file/_common.py` keeps `record`/`stale` (what reading means) and drops
  the two module-level containers.
- the "discard and pop" pair, written out twice, is `gate.forget`; the five-line
  rename fixup in `_fs_ops.py:61-67` is `gate.moved`.

### A tool says what its own row is

`Tool.brief` is a new field: a callable taking the dispatch result and returning
what a frontend should be told about it. `tools/loader.py` has `brief_body(*keys)`
for the five tools whose result carries a document, and `wire_result(registry,
name, result)` is the one place that asks.

- `main.py:1220-1244`, three hardcoded name lists, becomes one call: `("screenshot",
  "photos")` -> `{"n": ...}`, six body-carrying tools -> strip the body, `"shell"`
  -> strip the output. Each is now declared by the tool: `screenshot/main.py`,
  `photos/main.py`, `shell/main.py`, `WebFetch/main.py`, `agent/main.py`,
  `workflow/main.py`, `skill/main.py`. A tool that declares nothing sends its
  result whole, which is what the small ones want.
- `tools/describe_image` was in the body list with no body to strip -- the list
  was a guess about a set of names, not a statement about any one tool. It has no
  brief now, because its result is already one answer and a provider name.
- the live turn and the replay now agree. `main.py:320-322` sent the stored result
  whole while a live turn sent the brief, so reopening a conversation redrew a
  build log the turn itself had reduced to a byte count.

Bug fixed on the way: the image branch read `result.get("count")` on the
*envelope* rather than the photos result, so a three-photo call drew `n 1`.
`photos.brief` reads the count where photos puts it.
