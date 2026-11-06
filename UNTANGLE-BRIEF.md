# Untangle this codebase, with one target in mind

You are working in a git worktree of CuaCode on branch `refactor/untangle`, cut from
`3aa875c`. Nobody is watching. Nothing goes to a remote: **do not push, do not open a
PR, do not touch `origin`.** Everything is local commits on this branch.

Work the `untangle-codebase` skill (`~/.cuacode/skills/untangle-codebase/SKILL.md`)
to the end. This file is the part the skill cannot know: what the system has to
become, and therefore what "better" means here.

## What CuaCode is, in two paragraphs

The Python side is the worker: an agent that talks to a model and owns the tools.
It reads line-delimited JSON commands on stdin and writes events on stdout. The Go
and Rust frontends are clients of that wire and draw; they decide nothing. The
worker's own state is `~/.cuacode/` (sessions, memory, skills, subagents, workflows,
MCP servers, hosts).

The wire is `main.py`'s IPC loop plus `rust/src/wire.rs`. `rust/fixtures/record.py`
re-records `replay.jsonl`, which is what holds the two halves to the same shapes.
Nothing else crosses the process boundary.

## The target: one agent, two doors

The work this refactor serves is turning CuaCode into something a program can drive,
not only a person. Two kinds of caller onto one core:

**A chat.** A person, interactive: steering mid-turn, permission prompts, a
transcript drawn on a screen, history, a composer.

**A program.** No screen and nobody to ask: structured events on a pipe, a run that
can be resumed or cancelled, an allowlist of tools, a hard ceiling on cost and time,
and a result that comes back as data rather than words.

The SDK is the second door made official: **a consumer in another repo, in another
language, that spawns the worker and drives a turn without importing anything from
this tree.** That is the whole contract. It is a process boundary, not an import.

So the question this refactor answers, for every seam: does this force a consumer
outside the repo to reach into `handler/`? If it does, the seam is in the wrong
place, and moving it is the work. `main.py` already is that door. The job is to
make it a stable, documented one, and to move what a consumer needs that currently
lives behind it.

## What to do first

1. Load and follow the skill. It asks you to choose two optional additions:
   - **Comments: no.** Comments in this tree are the design record, deliberately
     long, often the only place a decision is written down. Leave them. A comment
     goes only when the redesign deletes the thing it describes.
   - **Tests: yes, at the end.** Same rule as the skill states: a test that can
     only fail on a regression stays; a test that restates the implementation or
     cannot fail goes; one broken by an internals change is rewritten against
     behavior.
2. Keep a decision log at the worktree root as `UNTANGLE.md`: the problem list,
   then each fix above the problems it solves. Commit it as it grows. It is the
   thing a reviewer reads afterwards, and the thing that survives your context
   window being compacted.
3. Do not change behavior. Anything a user can observe today stays observable
   today, except where you find a real bug, in which case say so in the log.

## Scope

**In:** `main.py`, `handler/`, `tools/`, `integrations/`, and the wire
(`rust/src/wire.rs` only when a message shape genuinely has to move, plus
`rust/fixtures/` re-recorded with it).

**Out:** `rust/app`, `rust/gui`, `rust/fastrs`, `rust/quik` are clients. Do not
refactor them, do not restyle them, do not rename their crates. `rust/zeron` is a
vendored tree under another license; never edit it. Adding a dependency needs a
much better reason than convenience.

## Where the leverage is

Start where a consumer would have to reach deepest:

- `main.py` — the IPC loop, which is also the only public door there is today.
- `handler/agent/main.py` — `generate()`, the round loop every run goes through.
- `handler/config.py`, `handler/session/store.py` — module-level reads of files and
  globals that other modules depend on without saying so.
- `tools/` — a tool is a folder with a `Description.md`, `InputSchema.json` and a
  `run(args, ctx)`. Check what `ctx` actually owes and whether it is discoverable.
- `integrations/` — skills, subagents, workflows, memory, MCP, instructions. Each
  is a loader with its own conventions; the ones that differ for no reason are the
  untangle work.

Special cases are the point of the exercise: an `if` for one value, a parameter
that exists for one caller, a swallowed exception, a retry, a disabled test. Each
one names a requirement the design never accounted for. Collect them all before
fixing any of them, and look for the single change behind a group of them.

## The proof, at the end

Write a consumer that is **not** in this tree, in
`/Users/boaz/.cuacode/sessions/20261106-090710-e743/notebook/sdk-probe/`, and make
it drive one real turn against this worktree's `main.py` over the wire: spawn,
send a message, stream the events, stop. It must not import anything from the repo,
and it must not read files under `~/.cuacode` that a normal client could not read.

If it cannot be written that way, the gap it hit is the finding. Write it down
instead of hiding it.

## How to check yourself

- `cd rust && CUACODE_CWD= cargo test --workspace` (unset `CUACODE_CWD`, two view
  tests shorten paths against it and fail when it is set).
- `python3 tests/<name>.py` — the Python tests are scripts, not a framework. Do not
  add one.
- One real worker turn: `printf '%s\n' '{"type":"cmd","data":{"action":"chat",...}}'
  | python3 main.py` in this worktree. If the wire moved, `rust/fixtures/record.py`
  re-records the fixture the Rust tests hold the types to.
- The frontends still build: `cd rust && cargo build -p fastrs -p quik`. You did not
  change them, they just have to survive the wire.

## When you are done

1. `UNTANGLE.md` in its final state: every problem, every fix, and what is left
   deliberately.
2. Commit in small steps as you go, Conventional Commits, subject under 50 chars,
   the why in the body where the why is not obvious.
3. Write `UNTANGLE-REPORT.md` at the worktree root: what a consumer outside the repo
   can do now that it could not before, the special cases that remain and why each
   one is a real requirement, and anything you would have done with more time.
4. Stop. Do not merge, do not push, do not touch the other worktrees.
