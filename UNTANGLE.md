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

### main.py is a door, not an agent

Three files instead of one, split along the line the brief is about:

- `main.py` (1248 lines -> 37): choose between the argv forms, `--wire`, `serve`.
- `handler/worker.py` (new): `Worker` owns the conversation -- the session, its
  messages, the provider settings, the permission mode, the gates, the inline
  commands, the fold, and `turn()`. No envelope, no id, no loop.
- `handler/commands.py` (new): `TABLE`, one `Command` per action with its handler,
  a sentence, the fields it reads and whether it answers mid-run. Plus `serve`,
  which is the loop, and the eight menu readers `direct` commands need.

The one contract worth naming: **a turn is a generator of `(envelope type,
payload)`**. `w.turn(text)` produces the events; `commands.chat` pairs each with
the envelope that asked. That is what makes the second door possible at all -- a
caller with no frontend gets the same turn, and does not have to own an `ipc` to
run one.

- the fifty-branch `if/elif` is one dict, so a new command is one entry and
  nothing else in the file changes.
- `ipc.direct` is registered *from* the table (`if c.direct`), so the list that
  answers mid-run and the list that answers the loop cannot disagree -- they were
  two lists, and `dir.list` was in one and not the other.
- an action that is not in the table now answers `status: error, unknown action`
  instead of being dropped in silence. A consumer's typo used to cost an
  afternoon of staring at a pipe that never spoke.
- an exception inside a handler is answered as `status: error` instead of killing
  the worker. Half the branches had their own `try`, and the other half were a
  crash waiting for a bad argument.
- six module globals (`SETTINGS`, `sess`, `messages`, `ctx`, `LAST_TURN`,
  `PERM_MODE`, `pending_note`) are fields on one object, so "which conversation is
  this?" has one answer.
- `context_fields` now asks about the session's model, not `SETTINGS`'s: after
  `session.load` those are two different conversations, so `/context` reported the
  account's window for a conversation that was not the account's.
- `pending_note` is cleared by `new_session` and `load_session`. It was carried
  into the next message typed, including one in a different conversation.
- `config.MODES` is the one list of the four permission modes; the wire had its
  own copy of the same four names.
- `handler/protocol.py`: `poll_one` (no caller) and the example loop under
  `__main__` (a protocol nothing implements) are gone.

### The second door: a policy on the turn, and a result as data

`chat` takes an optional `policy`, and every terminator carries a `result`.

```json
{"action":"chat","text":"…","policy":{"tools":["file"],"max_rounds":8,"max_seconds":120}}
```

- `tools` -> `generate(allow=...)`, which existed and had no door. A name that is
  not installed is refused before the turn starts: an agent with a typo'd toolbox
  would otherwise spend a turn reporting the model's failure.
- `max_rounds` -> counted at the `round` chunk, which is yielded *before* the
  request, so stopping there spends nothing. The turn ends as `done` with
  `stop: "max_rounds"` and a resumable history.
- `max_seconds` -> a deadline inside the `cancelled` callable the loop already
  polls everywhere (between chunks, between calls, inside the retry waits). A
  deadline is a cancel, so it needed no new machinery; the terminator says which
  of the two it was.
- `result` -> `{text, stop, messages, session}` on done / cancelled / error, and
  on the pre-turn refusals. `stop` is `done`, `cancelled`, `max_rounds`,
  `deadline`, or `error`.

The three fields a program needs that a person does not are exactly the three
that had no door: an allowlist, a ceiling, and an answer that is not text.

`WIRE.md` is the contract, and `main.py --wire` prints the commands from the
command table so the two cannot drift apart. README's Layout and a new
"Driving it from a program" section point at both.

### config.json is read once per change, not once per question

`handler/config.py:load` caches on `(path, mtime, size)`; `save` clears it. This
was tens of reads and parses of the same file per turn -- `settings()`,
`always_names()`, and every `entry()` lookup inside `context_window`,
`can_see` and `params_for`. A hand-edit is still picked up on the next stat, so
the "config is re-read every turn" guarantee is unchanged.

Left deliberately: `_spelled`'s `if key == "permission"`. It is not a mistake
wearing an `if` -- `permission`'s object has a third field (`provider`, for the
model that decides in auto mode) where `vision` and `steps` have two. Tabling the
three shapes instead was longer and less readable than one branch that says so.

### The toolbox owns the toolbox

`registry()` and `visible()` moved from `handler/agent/main.py` to
`tools/loader.py`.

- `handler/context.py:177` imported both from the loop to price the tool schemas
  for `/context`: a readout (stable) depending on the round loop (volatile) for
  an answer about the tool system (stable). The direction of that import was the
  only reason it existed.
- `handler/agent/main.py` loses `_tools_dir`, `_registry`, `_needs_vision` and
  `_visible` -- the loop no longer owns a global that is also the question "what
  is installed".
- `tests/test_compact.py` reached into the loop's `_visible` to build a provider
  fixture. It asks `tools.loader` now: the fold is what that file is about.

### The gate asks the tool

`Tool.irreversible(args) -> str` is new, and shell declares it
(`tools/shell/main.py`, pointing at the same `_safety.shell.destructive` it used).

- `handler/worker.py:bypass_gate` said `if name == "shell"`, so a second tool that
  could wipe something would have been waved through by the gate written to catch
  exactly that. The gate now takes the tool object, which is also what `auto`
  already took: two gates answering the same question about the same call had two
  different signatures.

### Smaller ones, in one pass

- `tools/describe_image/main.py` had its own copy of the image unwrapper;
  `providers/base._images` is now public `images_of` and shared.
- `integrations/permissions/decider.py` imported config at module scope while
  `skills/loader.py` imports it late with a comment explaining the cycle. Both late.
- `handler/worker.py` clears `ctx["emit"]` when a turn ends: `generate()` writes
  its own outbox there and never removed it, so a subagent reporting progress
  after a completed turn appended into a list nobody reads again.
- the four `input:` blocks in `tools/*/Description.md` that restated
  `InputSchema.json` -- which is what the loader actually hands the model -- are
  gone. Two statements of one contract: a field added to the JSON and not the
  markdown was invisible, and one removed from the markdown was still offered.

### The regression this pass caused and caught

While splitting the turn, the rule "a `/command` that leaves no text to send
answers `idle` and does not generate" was lost: `/version` typed on its own sent
an empty message to the model. `tests/test_worker_commands.py` caught it -- the
nine command checks went from 4 seconds to 248, because each one was starting a
real generation. Restored in `handler/worker.py:turn`.

### Four a reviewer found in the turn

An independent read of `handler/worker.py` (one file, one brief) found four
defects. Three were inherited from the old `main.py` and one was mine:

- **a peer's words were recorded empty.** The `peer` branch built the record from
  the *turn's* `peer_tag`, not from the chunk. A message that lands mid-turn has no
  tag at all (a person started the turn), so it was written as `from=None,
  text=""`; on a peer-started turn it recorded the message that started the turn
  again, under the wrong sender. `generate()` now puts `body` and `from_cwd` on the
  chunk, and the record is made from those.
- **`begin_run()` sat outside the `try`.** `generate(...)`'s arguments are
  evaluated eagerly (`config.api_key`, `instructions.user_block`, `environment.block`),
  so an exception there left the run open for the life of the process: every later
  `chat` would be steered into a list nobody drains, and no terminator was sent.
  The whole turn is inside the try now, with `gen = None` until it exists.
- **the rewind floor was the last commit, not the user's message.** `commit()`
  resets the session's mark to 0, so a failure before the first round chunk
  rewound past the user's own record -- gone from `meta.json`, still in the
  provider history, and the next turn would call the conversation unopened and
  re-inject the docs block. `turn()` takes a `round_start()` immediately after the
  user record is written.
- **`gen.close()` could skip the rest of the cleanup.** A generator whose own
  cleanup raises would have skipped `end_run()` and the `emit` pop, and with them
  the `max_rounds` terminator. It is inside its own guard now.

Plus the smaller one the reviewer noted: the idle answer carries a `result`
(`stop: "idle"`), so a client driving `turn()` directly has something to read,
the same as the unknown-tool refusal.

### Six more, from two more reads

A second pass over the tool loader, the file tool and the shell safety answer, and
a third over the catalog, the five loaders and the config cache. Every one of these
was inherited except the last, and two are the kind that only show up once
something else has already gone wrong.

- **The gate outlived its conversation.** `read_files` and `seen` are
  process-wide, and `new_session()` swapped the session without touching them, so
  a file read in the conversation you left let a write through in the one you
  opened -- a fresh conversation that never saw that content, which is the case the
  gate exists for. `gate.reset()` is new and both session swaps call it;
  `gate.restore` clears before it seeds.
- **`rm -rf build 2>/dev/null` walked through bypass mode.** `destructive()` parsed
  each line with `_segments`, which returns None for punctuation it does not model,
  and the loop read `(_segments(line) or [])` as "nothing here". A redirect
  anywhere on the line disarmed it. An unreadable line that names `rm`, `dd`,
  `mkfs*` or `diskutil` is a reason to ask now, and harmless redirects still pass.
  The same hole was there when the check was called from a tool name; it was the
  replacement that made it matter.
- **`read` reported a cut line as a whole file.** A one-line file bigger than the
  read budget came back `truncated: false`, so the file was marked read and the
  next `write` destroyed the rest of the line. It says `truncated: true` now.
- **`bypass_gate` called `Tool.irreversible` bare.** A hook that raised took the
  turn down, in the one mode where no prompt catches anything. A tool that cannot
  answer is asked about.
- **`brief_body(*keys)` measured only `output` and `instructions`.** Every current
  caller uses one of those two, so nothing was broken, and the docstring promised a
  tool could name its own body -- which would have dropped the body from the row
  with nothing in its place. It measures whichever key it dropped.
- **The config cache could pin a stale file** (mine). The stamp was taken a second
  time *after* the read, so a `save()` from another thread in between cached the
  old content under the new stamp and served it until the file was touched again.
  It is stamped with the value taken before the read now, and `load()` returns a
  copy: the cache made a caller's mutation stick where a fresh parse had thrown it
  away.
- Also: the docs pointer compared `abspath` against the gate's `resolve`d paths, so
  on macOS (`/tmp` -> `/private/tmp`) an already-read README was pointed at again on
  the next turn. Both sides resolve.
