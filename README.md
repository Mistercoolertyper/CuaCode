# CuaCode

An agent that runs on your computer and actually uses it. It reads and writes
files, runs commands, opens apps, clicks, types, takes screenshots, reads pages,
and talks to whatever model you point it at.

One Python process does the work, called the worker. The things you look at are
separate programs that draw what the worker reports: a terminal UI, or a window.

> [!WARNING]
> Still a work in progress. Not a virus, just not my problem if you mess up your
> computer. Read a permission prompt before you allow it.

## What it can do

`file` `shell` `app_open` `app_list` `screenshot` `click` `type_text` `key`
`mouse_move` `scroll` `photos` `describe_image` `WebFetch` `WebSearch` `mcp`
`skill` `todo` `memory` `agent` `workflow` `roster` `background` `wait` `ask_user`

The computer-use ones have a real implementation per operating system, picked
when the call runs, so nothing about Windows is loaded on a Mac.

It also remembers. Anything it learns that stays true goes into `~/.cuacode/memory`
as one small file, and comes back next time it matters.

## Requirements

- Python 3.10 or newer
- Rust, only if you want to build or run a frontend

## Install

```bash
./install.sh
```

That finds an interpreter, makes a venv, installs the Python deps, builds every
frontend, and puts a `cuacode` command on your PATH. Re-running is safe. On
Windows, use `install.ps1` and read [CuaCode on Windows](WINDOWS.md) first.

Then:

```bash
cuacode            # the terminal UI (quik)
cuacode cuacode    # the GUI window
cuacode --list     # what this tree can run
cuacode doctor     # what this build is, and how far behind origin it is
cuacode --usage    # what your conversations have cost
```

`./run.sh <frontend>` does the same without installing the command. Building is
`./build.sh`.

## First run

Nothing is configured out of the box. The first launch writes
`~/.cuacode/config.json` as a fill-in-the-blank template:

```json
{
  "active": "ollama",
  "vision": "",
  "effort": "",
  "always_skills": [],
  "providers": {
    "ollama":    {"model": "", "api_key": "", "params": {}},
    "anthropic": {"model": "", "api_key": "", "params": {}}
  }
}
```

Fill in one provider and set `active` to its name, or pick both from inside a
frontend with `/provider`. The file is re-read every turn, so an edit lands on
your next message with no restart. Keys sit in it in plaintext, chmod 0600.

`vision` names the provider that looks at images if your main model cannot, and
`effort` is the level a new conversation starts on.

| Provider | Endpoint | Key |
| --- | --- | --- |
| `ollama` | ollama.com | `OLLAMA_API_KEY`, or `ollama signin` |
| `anthropic` | api.anthropic.com | `ANTHROPIC_API_KEY` |
| `openai` | api.openai.com | `OPENAI_API_KEY` |
| `openrouter` | openrouter.ai | `OPENROUTER_API_KEY` |
| `nvidia` | integrate.api.nvidia.com | `NVIDIA_API_KEY` |
| `deepseek` | api.deepseek.com | `DEEPSEEK_API_KEY` |
| `together` | api.together.xyz | `TOGETHER_API_KEY` |
| `qubrain` | qubrain.org | `QB_API_KEY` |

## Permissions

Anything irreversible has to be approved before it runs. A frontend can set how
that goes:

- `ask` puts the call to you, showing the arguments and, for a write, the patch.
- `auto` sends the calls that would prompt to a small model first. A yes runs; a
  no comes to you anyway, with the reason, so the gate filters rather than decides.
- `bypass` asks nothing except for calls that wipe something.
- `off` runs whatever the tools ask for.

`permission` in `config.json` names the mode and which model does the filtering.
The gate fails closed: if it errors, you get asked.

## Frontends

| Name | What you get |
| --- | --- |
| `quik` | Terminal, compact. One row per thing, and the one I use. |
| `fastrs` | Terminal, larger. Tool calls fold into one row per run; `Ctrl+O` opens it. |
| `cuacode` | GUI window. A rail of projects with their sessions, the conversation, a composer. |

`./run.sh` with no argument gives you `quik`. Every frontend is documented in
`rust/README.md`.

## Boxes

A box is another machine that runs a worker of its own over ssh. Add one to
`~/.cuacode/hosts.json` and it shows up in the picker. Its sessions, memory,
skills and keys are that machine's, nothing is synced, and its worker lives in a
tmux pane there so closing the window ends your view and not the work.

## Extending it

Files, re-read every turn. Drop them in `~/.cuacode/` and they are yours; the
ones under `integrations/` ship with the app and yours win on a name clash.

| What | Where yours goes | What it is |
| --- | --- | --- |
| subagent | `subagents/*.md` | one model run with its own prompt, tools and output schema |
| workflow | `workflows/*.py` | a script running several of them in a fixed order |
| skill | `skills/<name>/SKILL.md` | instructions loaded only when needed, by the agent or by `$name` |
| MCP server | `mcp/servers.json` | tools this codebase did not write |

A tool is a folder under `tools/` with a `Description.md`, an `InputSchema.json`
and a `main.py` exporting `run(args, ctx)`. Nothing registers it. The
`writing-tools` skill walks through that, `writing-subagents` and
`writing-workflows` do the other two.

## Where things live

```
~/.cuacode/config.json      providers, keys, models, permission mode
~/.cuacode/AGENTS.md        your standing instructions, in every conversation
~/.cuacode/sessions/        one directory per conversation
~/.cuacode/memory/          what it has learned about you, one fact per file
~/.cuacode/subagents/ workflows/ skills/ mcp/    the four extension points
```

`CUACODE_HOME` moves the whole root.

## If you are writing code against it

- [`WIRE.md`](WIRE.md) is the protocol: every command, every event, and what a
  client that is not in this tree can rely on. `python3 main.py --wire` prints the
  command table this build answers.
- `rust/README.md` is the frontend half, `rust/AGENTS.md` its standing orders.
- `AGENTS.md` at the root is for agents working in this repo, not for people.

## License

Free to use, copy, modify and distribute, and not to be sold. Keep the notice,
credit the author, and do not put it inside anything you charge for. The whole
thing is four paragraphs in [LICENSE](LICENSE).
