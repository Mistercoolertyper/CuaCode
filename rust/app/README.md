# cuacode

The base of an agent orchestrator: one window, several projects, and the
sessions standing in each. Built in Rust on GPUI, from the same tree as the
terminal frontends.

```bash
./run.sh cuacode                  # run it from source
rust/app/app.sh --open            # build a real .app and open it
```

It is bin `cuacode` (crate `cuacode-app`), and it is the app this tree is being
built towards. `fastrs` draws one conversation in a terminal; this one is the
shell an orchestrator needs before any of that: where your work is, what you
were last doing, and one row that decides how the next run is made.

## What is here

```
+------------+-------------------------------------+-------------+
| rail       |  top bar                            |             |
|  sessions  +-------------------------------------+  session    |
|  projects  |  the conversation                   |             |
|  sessions  +-------------------------------------+  plan       |
|  footer    |  the composer                       |             |
+------------+-------------------------------------+-------------+
```

**The rail** is the product: `Sessions`, then a heading per promoted project
with its sessions under it, then `No directory` for the sessions whose directory
was never promoted. A session row is its title, where it stands, and how long ago
you last used it. A project folds; the row the pointer is on lights up; the
selected one wears a wash of the foreground.

**The conversation** is read out of the session's own record and folded, which is
the difference between a transcript and a log. Between one answer and the next,
every call and its result is **one row**: `5 calls · read, edit`, which opens on a
press to show each step and what it answered. A failing step is marked. The
model's reasoning is the run's own note, and it does not stack.

**The sidebar** is superset's: `Changes` and `Files` tabs, the branch, and the
working tree in their three categories (Staged, Changes, Untracked) with a status
mark and `+n -n` per file. A file opens its diff in the same column. Under all of
it, the session's own **Plan**, which is the worker's `todo.json` and the one part
of this column that is about the work rather than about the checkout.

**The composer** is one card at the foot of the column, and it is where the next
run is decided:

| chip | what it sets | when |
|---|---|---|
| project | where the session stands | only until the session has started |
| provider | which account answers | always |
| model | which model on that account | always |
| permissions | `ask`, `auto`, `bypass`, `off` | always |

The location chip locks the moment a session is selected, and it wears a lock
rather than a chevron so it reads as settled instead of broken. That is the same
rule the worker enforces: it only takes a `cwd` before the first record, because
a directory decides where every one of a session's tools already ran.

## Where it reads

Nothing is driven over IPC. The window reads the state root itself, exactly as
`handler/session/store.py` writes it:

```text
~/.cuacode/projects.json          what you promoted, in your order, with pins
~/.cuacode/sessions/<id>/meta.json   id, title, cwd, provider, model, updated
~/.cuacode/sessions/<id>/messages.jsonl   the conversation, read from the end
~/.cuacode/sessions/<id>/todo.json    the session's own plan
~/.cuacode/config.json            which providers exist, and the active one
```

Plus two questions only the machine can answer, each asked once and never in a
frame: `git -C <dir>` for the changed files (every 4s), and the worker's own
provider registry through one `python3 -c` at boot (77ms, no network, no key).

`CUACODE_HOME` moves the root. No worker, no process, no socket: the worker
answers `project.list` and `session.list` by reading these same two files, so
starting a process to hear it read a JSON file would buy nothing but a process.
It also means the window draws its rail when the worker is broken, which is
exactly when you want to see it.

## Why it is fast

The render path does not touch the disk.

An earlier pass read the transcript inside `render`, and `render` runs thirty
times a second while a hover is travelling. On a 2.8MB session that was **10.6ms
of parsing per frame, 0.32s of CPU for every second of hovering**, plus a file
open each time. It showed up as a window that stuttered under the pointer.

Now:

- Files are read on a clock, keyed on a stamp of `(length, mtime)`. One read is
  **1.6ms**, once, and only when the file actually moved.
- The transcript is read from **the end**: the last 256KB, with the partial line
  the seek lands in dropped. A ten-hour session costs the same as a short one.
- The fold runs once per read, not per frame.
- The clock naps at 33ms while something is moving and **250ms when nothing is**.

The wheel gets a `ScrollHandle`, which is what gives a scroll offset somewhere to
live between frames; without one a scroll is re-derived from the pointer every
frame and the page fights the cursor.

## Where the look comes from

Superset's (`https://superset.sh`, ELv2), read out of their own source rather
than eyeballed off screenshots:

```text
packages/shared/src/themes/built-in/ember.ts          the palette, as literals
apps/desktop/src/renderer/globals.css                 the same, plus the washes
packages/chat-ui/src/components/PromptInput/          the composer
apps/desktop/src/renderer/screens/main/components/     the rail, the sidebar
```

Their dark theme is warm, and that is the whole character of it: a brown-black
`#151110`, greys that lean warm, a warm off-white `#eae8e6` that is never white,
and one hot orange `#e07850` for what you can act on. A neutral grey-blue window
is what everyone else ships.

Two of their rules are load-bearing and are kept exactly:

- A row is marked by a low-opacity wash of the **foreground** (`--fill-hover`,
  `--fill-selected`), not by a grey fill, so a lit row tracks its own text.
- The accent means "you can act on this" and nothing else. `ask`/`warning` and
  `error` are states and are the only other hues.

One place this window departs from theirs: **nothing here is translucent.**
Superset's window is a vibrancy material with translucent planes over it. This
one is a page, and every surface is opaque, for two reasons: `--background` there
is an opaque `#151110` and a translucent one over a bright desktop is not that
colour, and `theme.rs` can then check every tone against a contrast target
instead of against a wallpaper. Those checks are tests, not comments.

`theme.rs` carries their literals and the file they came from. `fleet/theme.rs`
carries the same palette and is its own contract. Neither may drift.

## Files

- `state.rs` - the state root, read: the projects, the sessions, a transcript's
  tail, the fold, and what a picker may offer. No drawing.
- `view.rs` - what the shell is made of. No state.
- `theme.rs` - the palette and the numbers, with the contrast checks as tests.
- `motion.rs` - the hover clock and the pulse.
- `main.rs` - the window, the keys, the menus, and the cache.
- `app.sh` - build it as a real macOS bundle into `~/Applications`.

Tests are the crate's own (`cargo test -p cuacode-app`) and cover the parts with
logic in them: a session grouping under the right project, the tail read keeping
whole rows, the cache stamp moving when a file does, a run folding to one row,
and a session id that cannot climb out of the sessions directory. The palette's
contrast targets are tests too.

## Where a fact comes from

Every list and every number has one owner, and none of them is duplicated here:

| fact | owner |
|---|---|
| which providers exist, their keys and shortlists | the worker's registry, asked once |
| which models to offer | this account's session records, then the registry |
| the permission modes | `handler/config.py`'s `ask \| auto \| bypass \| off` |
| whether a session's location may change | the worker's own rule: only before its first record |
| what changed in a directory | `git`, on a 4s clock |
| a project's mark | `projects.json`'s `icons`, in the worker's three forms |

## Not done

- **No agent half.** Nothing is sent to a worker. The composer takes the message
  and the four settings and prints them; `App::send` is the one function that
  changes when a worker is wired in.
- No session rename or delete, no attachments, no syntax highlighting in the
  conversation's code spans, no keyboard navigation of the rail.
- The `Files` tab under the sidebar's plan is a placeholder.
- A project's uploaded image icon (`img:data:`) falls back to the letter: this
  window has no image pipeline, which is their own fallback too.
- A diff is drawn from the first 400 lines and says so.
- The composer's field is single-line.
- Nothing is committed: this tree's branch is `lab` with no upstream, and
  everything meant to ship goes to `CuaCode-core` by hand.
