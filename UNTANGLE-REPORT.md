# Untangle: the report

Branch `refactor/untangle`, cut from `3aa875c`. Everything below is local; nothing
was pushed, no branch was merged, `origin` was not touched.

`UNTANGLE.md` is the decision log: the problem list as found, and each fix above
the problems it answers. This file is the shorter question: what is different
from outside the tree, what is still odd on purpose, and what I would do with
more time.

---

## 1. What a consumer outside this repo can do now

It can drive one turn without importing anything, and it can say what kind of run
it wants. That is the whole of it, and it was not possible before.

| | before | now |
| --- | --- | --- |
| run a turn from another process | possible, over the wire | same wire, one documented contract |
| which tools a run may use | nothing; every installed tool | `policy.tools`, refused if a name is wrong |
| a ceiling on a run | nothing | `policy.max_rounds`, `policy.max_seconds` |
| the answer as data | concatenate `content` tokens and guess why the run ended | `result: {text, stop, messages, session}` on the terminator |
| find out what the protocol is | read `main.py`'s 500-line `if/elif` | `WIRE.md`, plus `main.py --wire` generated from the command table |
| a typo in a command | silently dropped | `status: error, unknown action: 'chta'` |
| an exception in a handler | killed the worker | `status: error`, the loop keeps serving |
| a tool's result on the wire | three hardcoded name lists decided what crossed | the tool declares its own row (`Tool.brief`); a live turn and a replay agree |

The proof is a consumer that is not in this tree:
`~/.cuacode/sessions/20261106-090710-e743/notebook/sdk-probe/`. A client plus a
probe, standard library only, 21 checks against the real worker:

```
python3 probe.py /path/to/worktree/main.py
```

It spawns `main.py` against a scratch `CUACODE_HOME` (a fresh temp dir per run),
never opens a state-root file, asks for `tools: ["shell"]`, runs a real command,
reads the result, refuses a second call at the permission gate, puts a round
ceiling on a third, lists and reloads the conversation over the wire, and stops
the process.

**The gap it found, in order of how much it mattered:** an allowlist and a
ceiling had no door at all (`generate()` already took `allow`; only the subagent
loop ever passed it). A refusal put its reason in the event but left
`result.text` empty, which is the exact shape of "it returned nothing", so the
refusal fills both now. And `/version` typed on its own started an empty turn.

## 2. What moved

The brief asked one question of every seam: does this force a consumer outside
the repo to reach into `handler/`? Where the answer was yes, the seam moved.

```
main.py                    1248 lines -> 37     argv, --wire, serve
handler/worker.py          new                  the agent: one conversation, its turn
handler/commands.py        new                  the wire: a table of 34 commands, and the loop
handler/catalog.py         new                  where a kind's files live, which count, who wins
handler/gate.py            new                  which files this process has read
```

- **A turn is an object.** `Worker.turn(text, policy=...)` yields
  `(envelope type, payload)`; `commands.chat` pairs each with the envelope that
  asked. Six module globals became fields, so "which conversation is this?" has
  one answer. That is the seam that made the policy possible: a caller with no
  frontend gets the same turn a person gets.
- **The wire is a table.** `ipc.direct` is registered *from* it, so the list that
  answers mid-run and the list that answers the loop cannot disagree -- they did,
  about `dir.list`.
- **One convention for file-backed integrations.** Skills, subagents, memory, MCP
  and workflows each spelled out the same three steps their own way. Four skip
  rules meant `_draft.md` in `~/.cuacode/subagents` was loaded as an agent.
- **The edit gate has an owner.** It lived inside the file tool's helper, and two
  other modules read it out of `sys.modules["_common"]` -- one of them after
  calling `load_tools()` purely to make the import happen.
- **The toolbox owns the toolbox.** `registry()`/`visible()` moved out of the
  round loop, which had made a readout depend on the most volatile module in the
  tree.

## 3. Special cases that remain, and why each is real

Not excuses: each is a requirement the design has no smaller way to express. If I
am wrong about one of them, it is the first place to look.

- **`tools/_safety/paths.py:scratch_dir` mirrors `Session.notebook`.** The file
  tool has to answer "is this path the user's scratch space?" without importing
  the session, and it has to answer it when there is no session at all. The
  fallback (`~/.cuacode/notebook`) is that case. It is a permission rule, so a
  wrong deduplication would either start prompting for scratch writes or stop
  enforcing the exemption.
- **`describe_image` names `screenshot` and `photos`.** Dispatching those two is
  its job: they are the only image sources, and it runs them against the *full*
  registry because a blind model is not offered them. A "declared image source"
  flag would be one more field for two tools.
- **`tools/todo/state.py` imports `handler.agent.subagent.depth`, and
  `handler/agent/main.py` imports `tools.todo.state`.** A cycle, and the depth
  has to be a `ContextVar`: a parallel workflow hands every agent the same `ctx`,
  so a depth field in ctx would report one agent's nesting for another's.
- **A few tool names survive in the round loop.** `c.name == "todo"` resets the
  stale-plan counter, and only the loop knows how many rounds have gone by.
  `call.name == "roster"` draws a peer row, and a tool returns a dict, not a
  conversation event.
- **"ctx cwd, else home" appears three times** (`file/_common._base`,
  `shell/main._start_dir`, `environment.block`). They answer three different
  questions -- where a relative path resolves with no ctx, where the shell starts,
  and what the prompt says -- with three different fallbacks. Merging them would
  change one of the three.
- **`tools/wait` caps at 10 seconds**, because the tool blocks the loop. It now
  reports what it waited as well as what it was asked.
- **`tools/photos` raises** where the other tools return `{"error"}`. The raise
  carries its per-source errors in the message and `dispatch` turns it into the
  same shape, so the model sees one thing either way.
- **`roster`'s `{"ok": False, "warning": ...}`** is a refusal that is not an
  error: a peer that is not there is not a failure.
- **`mcp/client.py` hardcodes `"version": "1.0.0"`** in its client info while
  `handler/version.py` exists. The handshake wants a semantic version; the build
  stamp is a git description (`162.7066534`), which is not one.
- **`mcp/client.py` rewrites `"python"`/`"python3"` to `sys.executable`.** A
  config that says `python3` has to get the interpreter with the dependencies.
- **The Windows `frontmost_app` probe imports a tool's private module**
  (`tools.screenshot._capture_windows`). A handler-level desktop module is more
  code for one platform and one function.
- **`_spelled`'s `if key == "permission"`.** `permission`'s object has a third
  field (`provider`, for the model that decides in auto mode) where `vision` and
  `steps` have two.
- **`handler/agent/main.py` sleeps 0.15s between tool calls in a round.** Pacing:
  it is what keeps a three-call round readable as it happens, and keeps a local
  server from being hit in a burst.
- **`costs.py` at the root** is imported by nothing and is not dead: it is a
  script a person runs (`python3 costs.py --days 7`).

## 4. The checks that ran

```
python3 tests/*.py                       13 scripts, all green
python3 lifecheck-projects.py            all green
python3 lifecheck-peers.py               all ok
python3 tests/test_worker_commands.py    the real worker over its wire, all green
sdk-probe/probe.py                       21 checks, all green, from outside the tree
```

Two tests changed, both because they rested on something that moved:
`tests/test_compact.py` reached into the loop's private `_visible` to build a
provider fixture (it asks `tools.loader` now; the fold is what that file is
about), and `tests/test_compact_trigger.py` computed its expected fold limit with
`compact.ratio()` -- the function under test. The number is written out now.

**The Rust half does not build in this worktree, and did not before this branch.**
`rust/Cargo.toml` lists `gui` as a member and `rust/gui` depends on
`../zeron/crates/ui`, which is not in the checkout (`git ls-files rust/zeron` is
empty), so cargo cannot resolve the workspace at all:

```
error: failed to load source for dependency `zeron-ui`
  ... failed to read `rust/zeron/crates/ui/Cargo.toml`: No such file or directory
```

I did not touch `rust/`, and I did not "fix" it by deleting a crate from another
tree's design. Instead I copied `rust/` to `/tmp` without the `gui` member and ran
the shared half there: **48 passed, 2 failed**, and both failures are the copy's
own missing context -- `version::the_checkout_is_found` ("no checkout above the
crate") and `worker::the_worker_is_found_by_env_or_by_walking_up` ("the Python
side is the sibling of this crate"). Every `wire`, `transcript`, `session` and
`code` test passed. No message shape moved, so `rust/fixtures/replay.jsonl` was
not re-recorded: the only change to an event is `result` added to a terminator,
which `Payload` already parses as `Option<Value>` and no frontend reads outside
`tool_output`.

## 5. With more time

- **`generate()` is still 873 lines with 16 keyword parameters**, and it is both
  the chat loop and the subagent loop. It is the last module that is really four:
  transport (retry, backoff, what counts as transient), the round (open, stream,
  assemble), the tool phase (gate, dispatch, close out), and the notes that ride
  the round boundary (steer, peer mail, todo, background jobs). Splitting it is
  the next real win, and the reason I stopped short is that it is the one file
  where a mistake is silent: the tests drive it through a fake provider and would
  pass a subtly different loop.
- **A budget in money, not rounds.** `max_rounds` and `max_seconds` are the two
  ceilings that need no provider cooperation. A token budget needs usage
  mid-round; `usage` arrives per round, which is exact enough. Worth doing, and
  worth doing after the split above.
- **`integrations/mcp/client.py` is 957 lines** and I did not untangle it. Its
  public surface is fine; inside, the legacy handshake and the modern one share
  almost nothing, and two sleeps (`0.2` in a start loop, `0.04` in `_end`) are
  waiting on a state they could be told about.
- **`skills` and `memory` re-read their corpora on every call.** `load_skills`
  re-reads every `SKILL.md` at least three times a turn. The config cache shows
  the shape of the fix (stat, cache, invalidate on write); it is mechanical, and
  it was not this brief.
- **A second language for the probe.** The point of the second door is "another
  repo, another language". I wrote it in Python because it had to run here; the
  honest next step is a 60-line Go or TypeScript client, which would test whether
  the contract really is language-neutral or just Python-shaped.
- **`tools/_window.py` imports platform backends by file path**, and
  `screenshot/_session.py` imports them by bare name because the folder was put
  on `sys.path`. It works, and the import machinery depends on call order in a
  way nothing states. That is the same class of bug as the `sys.modules` one I
  removed, one layer down.
- **Some long comments now describe a tree that moved.** They are the design
  record here and the brief said to leave them, so I left them; a pass over
  `handler/agent/main.py`'s docstrings after the next change to that file is worth
  the ten minutes.
