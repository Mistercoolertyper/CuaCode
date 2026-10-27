# CuaCode

CLI agent for computer use and more.
> [!WARNING]
> This is still a work in progress, use at your own risk (not a virus, just not my problem if you mess up your computer lol).

## Requirements

- [Python](https://www.python.org/) 3.10 or newer
- [Rust](https://rustup.rs/) - only to build or run a frontend

## Install

One script does the whole thing: finds an interpreter, makes a venv, installs
the dependencies, builds every frontend.

```bash
./install.sh
```

Flags: `--no-build` for the Python side only, `--no-venv` to install into the
interpreter as found. Re-running is safe - an existing `venv/` is reused.

Then run one:

```bash
./run.sh fastrs   # terminal frontend, built on first run
./run.sh cuacode  # the GUI app
./run.sh          # list the frontends you have
./run.sh --usage  # what every conversation has cost, no session started
```

Building on its own is `./build.sh`, optionally with one frontend's name, or
`--keep-going` to report failures at the end rather than stopping at the first.
Both frontends come out of `rust/`; `app.sh` in `rust/app` builds the GUI as a
real macOS bundle.

### Doing it by hand

```bash
python3 -m venv venv
./venv/bin/pip install -r requirements.txt
./build.sh
```

The interpreter is found at spawn time by layout, not by activation: there is
no need to `activate` anything to run the app. A `venv/` or `.venv/` next to
`main.py` wins over PATH, and `CUACODE_PYTHON` wins over both.

### Platform notes

- **macOS** - `pyobjc-framework-Quartz`, installed for you. Computer use needs
  Accessibility and Screen Recording permission for your terminal (System
  Settings → Privacy & Security), or clicks land nowhere and screenshots come
  back black.
- **Linux** - needs `xdotool` on PATH for clicks, keys and window titles
  (`apt install xdotool` / `pacman -S xdotool`). X11; Wayland is not handled.
- **Windows** - native Win32 computer use, no WSL or VM required. `pywin32` and
  `psutil` are installed for you. Setup and build notes are in
  [CuaCode on Windows](WINDOWS.md). If a frontend starts but nothing ever
  happens, see [Worker discovery](#worker-discovery) - a zero-byte
  `python3.exe` from the Microsoft Store is the usual reason.

## First run

Nothing is configured out of the box. The first launch writes
`~/.cuacode/config.json` (on Windows, `%USERPROFILE%\.cuacode\config.json`) as a
fill-in-the-blank template with a blank entry per registered provider:

```json
{
  "active": "ollama",
  "vision":   "",
  "effort":   "",
  "always_skills": [],
  "providers": {
    "anthropic": {"model": "", "api_key": "", "params": {}},
    "ollama":    {"model": "", "api_key": "", "params": {}}
  }
}
```

Fill in `model` and `api_key` for the one you want and set `active` to its name,
or pick both from inside a frontend with `/provider` - the file is written either
way. `vision` picks the provider that looks at images (see Vision), `effort`
sets the level a new conversation starts on, `always_skills` is the short list
of skills held in the system prompt. `steps` names the model that labels a step
of work for a compact frontend, spelled like `vision`; unset, it borrows the
`vision` model rather than the conversation's own. It is re-read every turn, so an edit lands
on the next message without a restart. It holds API keys in plaintext and is
chmod 0600 on every write (a no-op on Windows, where the file's protection is
the profile directory's).

Everything else the agent keeps lives beside it:

```
~/.cuacode/config.json      providers, keys, models, your permission lists
~/.cuacode/AGENTS.md        your standing instructions, in every conversation
~/.cuacode/sessions/        one directory per conversation
~/.cuacode/memory/          what the agent has learned about you, one fact per file
~/.cuacode/subagents/*.md   yours; loaded next to the ones that ship
~/.cuacode/workflows/*.py
~/.cuacode/skills/<name>/
~/.cuacode/mcp/servers.json
```

`CUACODE_HOME` moves the whole root.

## Providers

| Name | Default endpoint | Key read from |
| --- | --- | --- |
| `ollama` | ollama.com | `OLLAMA_API_KEY`, or `ollama signin` |
| `anthropic` | api.anthropic.com | `ANTHROPIC_API_KEY` |
| `openai` | api.openai.com | `OPENAI_API_KEY` |
| `openrouter` | openrouter.ai | `OPENROUTER_API_KEY` |
| `nvidia` | integrate.api.nvidia.com | `NVIDIA_API_KEY` |
| `deepseek` | api.deepseek.com | `DEEPSEEK_API_KEY` |
| `together` | api.together.xyz | `TOGETHER_API_KEY` |
| `qubrain` | qubrain.org | `QB_API_KEY` |

The environment variable wins over the key in `config.json`, so a shell can
override a stored key without editing anything. Anything OpenAI-compatible is
one line in `handler/agent/providers/__init__.py` - default model, base URL,
key variable - and a local server (ollama local, litellm, LM Studio) joins the
registry the same way.

`ollama` means ollama.com, not the daemon on your machine, and it speaks the
openai schema - there is no native ollama dialect in here - with the model picker
listing that account's catalog rather than `ollama list`. It needs no key in the
file: `ollama signin` through the desktop app leaves one in `~/.ollama/keys`,
which is read when nothing else is set.

**Vision is the thing that decides what the agent can do.** A model that cannot
see is never handed the screenshot tools - offered them it would call them, and
the endpoint rejects the image with a 400 that costs the whole turn - so on a
text-only model you get a capable CLI agent and no computer use. Capability is
asked of the provider where it can be, learned from a refusal where it cannot,
and remembered per model. A blind model can borrow eyes: set `vision` to another
configured provider - a name, or `{"provider": ..., "model": ...}` to pin the
model that does the looking - and `describe_image` routes screenshots through
it; `/vision` does this from the palette.

Thinking effort is per conversation, not per account: `off`, `low`, `medium`,
`high`, `max`, set with `/effort` and stored with the session. Each provider's
own dialect for it - `reasoning_effort`, thinking budgets - is
translated from that one ladder.

## Version and updates

There is no version number to bump. A build's id is the tree it came from:
`<commits>.<short sha>`, stamped by `rust/build.rs` and computed the same way by
the worker (`handler/version.py`). `fastrs` puts it in its header, the GUI puts
it at the right end of the top bar, and `fastrs --version` prints it alone.

Both frontends ask the remote once at startup, on a thread, whether the tree has
moved past them. When it has, fastrs adds a yellow line under its header - `you
are behind 3 updates` - and the GUI's chip turns amber, both naming the command
that does something about it:

```bash
cuacode doctor              # the build, the tree, the branch, and how far behind
cuacode doctor --upgrade    # git pull --ff-only, then ./build.sh; restart after
```

`/version` typed in a conversation answers with the worker's own stamp, which is
not always the frontend's: both read the tree, and only one of them is rebuilt
when the other's source moves.

This works in a checkout you made yourself, not only in the one it was written
in. The comparison is against the remote's **default branch** and the first
remote by any name (`origin` preferred), so a fork on a feature branch, a tag
checked out, a detached head and a plain clone all get the same answer. The
common case costs one `ls-remote`: no objects, no fetch, no refs written, and
nothing at all on the frame path - a fetch is only paid for when there is a
number to count. Build it from a tarball with no `.git` and it still builds: the
id falls back to `unknown` and the update line stays silent instead of guessing.

## Layout

The Python agent is the worker; the Rust side is one or more interchangeable
frontends talking to it over line-delimited JSON on stdin/stdout.

```
main.py, handler/, tools/     the Python worker (the actual agent)
integrations/                 subagents, workflows, skills, MCP servers
rust/src/wire.rs              the wire format: the envelope, its events, its fields
rust/src/worker.rs            finds python and main.py, spawns, reads, reaps
rust/src/session.rs           frontend-agnostic state, folded from events
rust/src/transcript.rs        events folded into rows: the model a UI draws
rust/fastrs/                  the terminal frontend
rust/quik/                    the compact terminal frontend
rust/app/                     the GUI frontend (GPUI)
rust/target/release/          build output, one binary per frontend
```

Nothing in the shared half knows a frontend exists, and no frontend knows about
another: the terminal one must not link a UI framework and the GUI must not link
a terminal library. Adding one never touches the others.

### Tools

`WebFetch` `WebSearch` `agent` `app_list` `app_open` `background`
`click` `describe_image` `file` `key` `mcp` `memory` `mouse_move`
`photos` `screenshot` `scroll` `shell` `skill` `todo` `type_text`
`wait` `workflow`

The pointer-and-keyboard ones (`click`, `key`, `scroll`, `type_text`,
`screenshot`, `app_open`, `app_list`) have a per-OS implementation behind one
`main.py`, picked at call time - nothing OS-specific is imported until it runs.

### Frontends

| Name | What it is |
| --- | --- |
| `fastrs` | Terminal. One conversation as an action tape rather than a chat log: the model's prose is the unmarked baseline, and what you asked, what it thought and what it did to the machine are what catch the eye. Tool calls fold into one row per run - `5 calls · read, edit` - which opens on a press. `/` opens the worker's own command table (with `help` and `permissions` in front), `$` the skill menu, `@` a file picker. `Ctrl+O` opens the call under the cursor in full, `Ctrl+V` attaches a picture, `Esc` stops the run, `Shift+Enter` (or `Alt+Enter`, or `Ctrl+J`) puts a newline in the message. `/help` lists every key; there is no key row drawn in the corner of the frame. |
| `quik` | Terminal, compact. Fastrs's code with the design still to be settled: thoughts and calls compressed, the header and the frame cut down. |
| `cuacode` | GUI window. A rail of promoted projects and the sessions standing in each, the conversation, and a composer that sets provider, model, permissions and location. It reads the state root directly, so the rail draws with no worker up. A run is one row, the same fold fastrs uses. `rust/app/app.sh` builds it as a real macOS bundle so it owns a Dock identity and a menu bar. |

All three are documented at length in `rust/README.md`, which is the page for the
frontend half.

### Adding a frontend

A frontend is a crate under `rust/` with a `[[bin]]`, built on the shared half
(`wire`, `worker`, `session`, `transcript`). Nothing registers it: `build.sh` and
`run.sh` read the cargo manifests and pick up the new binary by its `[[bin]]
name`, so `./run.sh <name>` works the moment the crate exists.

```rust
let mut sess = Session::start(home, cwd)?;   // spawns the worker, reads its lines
sess.chat("hello", &[])?;                    // user input in
sess.cancel()?;                              // abandon the run in flight, worker stays up
sess.background()?;                          // push the running tool call into the background
sess.command("session.list", json!({}))?;    // any of the worker's other commands
for ev in sess.drain() { /* fold it */ }
```

The other commands are the worker's own table: `session.list`, `session.new`,
`session.load`, `session.rename`, `session.delete`, `session.archive`,
`session.effort`, `provider.list`, `provider.use`, `provider.set`,
`model.list`, `vision.use`, `permission.mode`, `tool.detail`,
`background.list`, `background.kill`, `skill.list`, `context.report`,
`usage.report`, `project.list`, `project.add`. A reply comes back as an event
under the id it was sent with.

`rust/fastrs` is the smallest complete example and `rust/app` the other extreme,
a window over the same types. `rust/AGENTS.md` has the rules that keep them
apart, and `rust/README.md` explains the fold each of them draws from.

### Cancelling

`Cancel` lands wherever the run currently is: while the request is opening,
between streamed chunks, between tool calls, and inside one. A tool call is
watched from another thread, so a ninety-second `shell` is stopped rather than
waited out - the tool is handed a cancel token and decides what stopping means
for it (`shell` kills its process group), and one that ignores it is abandoned
rather than waited for. A click that has already fired still cannot be taken
back. The worker rewinds the conversation past the partial turn either way, so
no assistant message is left holding tool calls that were never answered.
Completed earlier rounds of the same run are kept.

### Background tool calls

`Background` is the opposite of `Cancel`: the work is wanted, the waiting is
not. The call in flight keeps running under a job id, the agent is handed that
id where it expected a result, and the turn carries on. Nothing is stopped.

It applies to the call in flight and nothing else - sent while the worker is
thinking rather than calling a tool it is discarded, not saved up for whatever
runs next, so offer the key only while the UI shows a tool running.

The agent can also start one itself: a tool declaring `backgroundable: true` in
its frontmatter (`shell`, `agent`, `workflow`, `WebFetch` and `WebSearch`) gets a `background`
boolean added to its schema, and the loop routes the call to a job instead of
waiting on it. Either way it is the same job, and the agent collects the result
through the `background` tool. When one finishes, a one-line notice naming the
job - not the result - is put into the conversation after the next round of
tool results.

For a frontend drawing a jobs panel:

```rust
sess.command("background.list", json!({}))?;                  // -> {"type":"jobs", ...}
sess.command("background.kill", json!({"job": "bg_3"}))?;
```

Killing is cooperative, for the same reason cancelling is: a tool that watches
its token stops, and one that does not runs to the end with its result
discarded. The reply says which of the two happened rather than claiming
success either way.

### Reading a tool call in full

A `tool_output` event carries a summary, not the result: a screenshot is a
count, a shell command is a byte total, a fetched page is a character total. The
content is in the conversation, and a frontend drawing one row should not have
to swallow a build log to do it.

What the row cannot show is asked for by index instead. Every `tool_output`
carries the `index` its result was filed under, and:

```go
sess.Command("tool.detail", map[string]any{"index": 4})   // -> {"type":"detail", ...}
```

comes back with `{index, total, name, ts, args, result}` - the arguments the
model actually sent and the whole stored result, strings capped at 20k
characters each and images left as `{"$blob": ...}` refs rather than moving a
megabyte of base64. A call that has been rewound away answers
`{"unavailable": "no such call"}` rather than somebody else's result. `fastrs`
draws this on `Ctrl+O`; a frontend that never asks sees exactly what it saw
before.

### Tool permissions

A tool declares `require_permissions: true` in its `Description.md` frontmatter
(`file`, `shell`, `WebFetch`, `WebSearch`, `mcp` and `workflow` do). Before
running one, the worker asks the frontend and
**blocks until it answers** - there is no timeout, so a question left up
overnight is still answered in the morning.

Only frontends that have said they answer are ever asked, so this changes
nothing for a frontend that ignores it:

```go
sess.Command("permission.mode", map[string]any{"mode": "ask"})   // opt in
```

The worker then sends `{"type": "permission", "id": ..., "data": {"name":
..., "args": {...}}}` and waits for `sess.Reply(id, "permission",
map[string]any{"allow": true})`. A refusal comes back to the model as a normal
failed call (`denied by the user`) rather than a missing one, so the turn stays
valid.

The mode is the frontend's to set, and there are four: `ask` (the user
decides), `auto`, `bypass`, and `off`. In `auto` the calls that would prompt go to a
small model instead, and `bypass` asks nothing either but still puts the irreversible
calls to the user. `permission` in config.json names it, spelled like
`vision` - a provider, and optionally a model, blank to inherit whichever the
conversation runs on:

```json
"permission": {"mode": "auto", "provider": "ollama", "model": "gemma4:31b"}
```

The gate runs at low effort with no tools of its own and answers a strict
yes/no plus one line of reason. A yes runs. A no is not the last word: the
call is put to the user through the ordinary prompt, with the reason drawn
above the patch it is a comment on (`flagged: ...`) - the gate is a net for
what deserves a look, not the decider. Only a frontend that cannot be asked
takes the refusal itself (`denied by the permission model: <reason>`), so a
headless run stays safe. The gate fails closed: an error counts as a flag, and
the person still decides. `off` runs everything the tools ask for without a
word.

The prompt shows what is actually being allowed rather than a summary of it.
Both frontends draw the call's arguments in full, and where the tool supplies a
`preview` - `file` does, for a write or an edit - it draws that instead: the
summary line and then the patch itself, with line numbers off the hunk headers,
a marked gutter beside every line that changes and the code syntax-coloured
(Go, Python, JS/TS, Rust, C-family, shell, JSON/YAML, Markdown). A file being
created has no patch to diff against, so its content is drawn as one all-new
hunk. Long blocks are cut to keep the choices on screen and open with `Ctrl+O`.

**Not every call is asked about.** `require_permissions` is a property of the
tool, and per tool is too coarse: `file` reads far more often than it writes,
and most shell commands only look at the machine. A tool that requires
permission may define `safe(args, ctx) -> bool`
in its `main.py` and answer for the specific call; the prompt is skipped when
it says yes. It can only ever take a prompt away - a tool without
`require_permissions` was never asked about either way - and it fails closed:
no hook, an exception, anything not understood, and the question is asked as
before.

`file` calls itself safe for `read`, `ls`, `glob` and `grep`, and only outside
the paths whose contents are the thing being protected (`~/.ssh`, `.env`,
keychains, `*.pem`, the agent's own config). `shell` parses the command the way
the shell will - separators, quoting, newlines, substitutions - and calls it
safe only when every segment is a command on a read-only list in a form that
cannot write: `date && whoami && ls -la | grep a` runs without a prompt,
`git log` does, `git push` does not, and `find -delete`, `sed -i`, `sort -o`
and `awk 'BEGIN{system(...)}'` are all recognised as the write they are.
Anything the parser cannot fully account for is asked about.

The lists live in `tools/_safety/`. Personal additions go in
`~/.cuacode/config.json`, so nothing about one machine is checked in:

```json
"permissions": {
  "shell_allow":     ["kubectl", "terraform"],
  "shell_deny":      ["ps"],
  "sensitive_paths": ["/work/vault"]
}
```

### Worker discovery

Frontends find `main.py` by walking up from the working directory, then from
the executable. Override with `CUACODE_WORKER=/path/to/main.py`.

The interpreter is found in this order: `CUACODE_PYTHON`, then a `venv/` or
`.venv/` next to `main.py` (`bin/python3`, or `Scripts\python.exe` on Windows),
then PATH. On Windows a zero-byte candidate is skipped, because `python3.exe`
there is usually the Microsoft Store App Execution Alias - a stub that prints to
stderr and exits, which used to look exactly like the app hanging. If a frontend
draws its chrome and nothing else ever happens, run the worker by hand:

```
venv/bin/python3 main.py          # windows: venv\Scripts\python.exe main.py
```

It should print one `{"state": "ready"}` line and then wait on stdin. Anything
else is the error the frontend was not showing you.

## Extending it

Four ways that need no rebuild and no restart - files are re-read every turn, so
one written mid-conversation is usable in the next. The ones under
`integrations/` ship with the app; the ones under `~/.cuacode/` are yours, and a
name collision goes to yours.

| | Ships in | Yours in | What it is |
| --- | --- | --- | --- |
| subagent | `integrations/subagents/*.md` | `~/.cuacode/subagents/` | one model run with its own prompt, tools and output schema |
| workflow | `integrations/workflows/*.py` | `~/.cuacode/workflows/` | a script running several of them in a fixed order |
| skill | `integrations/skills/<name>/` | `~/.cuacode/skills/<name>/` | instructions loaded only when needed, by the agent or by `$name` |
| MCP server | `integrations/mcp/servers.json` | `~/.cuacode/mcp/servers.json` | tools this codebase did not write |

`integrations/README.md` has the formats. Nothing MCP is registered by default.

A skill is loadable two ways: the agent calls the `skill` tool, or you type
`$name` in the message - a `$` that starts a word opens the skill menu in the
input, and choosing one (or just sending the `$name` as typed) loads the
instructions beside that message. Its frontmatter can close either door -
`disable-model-invocation: true` keeps it out of the tool's list,
`disable-user-invocation: true` keeps it out of the menu. Setting both leaves a
skill nothing can load, so it is dropped.

### Adding a tool

A folder under `tools/` with three files - `Description.md` (frontmatter plus
the prose the model reads), `InputSchema.json`, and `main.py` exporting
`run(args, ctx)`. It is loaded on the next start, with no registration
anywhere; a folder starting with `_` is a library, not a tool. Optional hooks:
`describe()`/`schema()` for a tool whose options only exist at runtime,
`preview()` for one that can show what a call would do, `safe()` to answer for
a specific call. The `writing-tools` skill walks through it.
