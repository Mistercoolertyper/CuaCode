# This repo

CuaCode is one worker and several interchangeable frontends over it. The worker
is Python at the root (`main.py`, `handler/`, `tools/`) and is the actual agent.
The frontends are Rust, under `rust/`, and talk to that same worker over
line-delimited JSON on stdin/stdout.

## Rules

- The Rust half has its own standing orders. Read `rust/AGENTS.md` before
  touching anything under `rust/`. It is short, and it is not optional.
- Run a frontend with `./run.sh <name>` from this directory. Do not launch a
  binary from an agent shell: a TUI needs a real terminal, and a GUI wants a
  person in front of it. Both write their state to `~/.cuacode` unless
  `CUACODE_HOME` says otherwise.
- `CUACODE_WORKER` is inherited when it is already set in your shell, so an
  agent session can silently drive another checkout's worker. `run.sh` warns
  when it is about to; read the warning rather than assuming which tree ran.
- `build.sh` and `run.sh` serve both halves and read the frontends out of the
  cargo manifests, so adding one is a crate with a `[[bin]]` and nothing else.
  `install.sh` is reachable only from `run.sh`'s dependency tripwire.
- `rust/target/` is gigabytes of build output. Do not search it, and do not add
  it to anything. `cargo clean --profile dev` is how to get the room back.

## The Go frontends are gone

`go/` held the previous frontend generation (`deck`, `bridge`, `Pane`) and was
removed in favour of `rust/`. Its history is still in this log; the code is not
on this branch. Nothing at the root should reference `go/` again.
