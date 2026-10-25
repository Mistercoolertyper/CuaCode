# cuacode-rs

CuaCode's frontend half, in Rust: three frontends over one worker, which is the
Python process in the parent directory. The shared part is a port of the Go
side's `core/protocol`, `core/session` and `core/runner`. No frontend is
built into it, and it draws nothing.

## Where things are

```
rust/
  src/wire.rs          the envelope and its events, as they cross
  src/worker.rs        find python, find main.py, spawn, stream, reap
  src/session.rs       the conversation's state, folded from events
  src/transcript.rs    the conversation as rows: the model a UI draws
  fixtures/            a recorded stream, and the script that records one
  tests/               the fixture and the fold, held against the types

  fastrs/ fastrs       the terminal frontend, built to the Figma page 2 design
        main.rs        keys, the frame, the turn's clock
        view.rs        what a row looks like in cells
        menu.rs        the `/`, `$` and `@` menus
  quik/   quik         the compact terminal frontend, starting from fastrs's code
                       and drawn more tightly: one line a step, no air, a thought
                       named by a small model; the design is not settled yet
  app/   cuacode       the app: projects and sessions, the conversation,
                       and the composer (GPUI, superset's look)
        main.rs        the window, the keys, the four composer menus, the cache
        state.rs       the state root, read
        view.rs        what the shell is made of
        theme.rs       the palette and the numbers
        motion.rs      the hover clock, the pulse, the arrival curve
        agent.rs       the worker: one process, driven for liveness
        app.sh         build it as a real macOS bundle
```

## Run them

```bash
./run.sh fastrs          # terminal
./run.sh quik            # terminal, the compact one (`--print`/`--frame` preview it)
./run.sh cuacode         # the app
./build.sh fastrs        # just build one
./rust/app/app.sh        # the app as a real macOS bundle
```

Those scripts are POSIX, and the frontends are not. On Windows, build and run
the crate from `rust/` instead: `cargo run --release -p cuacode-fastrs --bin
fastrs`. The binary finds `../main.py` by walking up from where it is run, so
nothing has to be exported, and `CUACODE_WORKER` overrides it if it is somewhere
else.

Both take `--fixture <file>` to replay a recorded stream with no worker, no model
and no network, and `--home <dir>` to run against a throwaway state root.
`fastrs --print --fixture <file>` folds a stream and prints the conversation as
text, the only way to see what a TUI draws without a terminal.

A launch can carry the message to open with. `fastrs fix the parser` starts a
conversation and sends those words the moment the worker says it is up, and one
argument that names a file is that file's contents, so a script can hand a
session something to do:

```bash
cd ~/somewhere && cuacode-rs fastrs prompt.txt
```

The words are typed into the field and sent through the same path a person's
are, so the turn is drawn on screen the way a typed one is; nothing about it is
a separate headless road. Only fastrs has it: its send path is the one a launch
can reach without a second implementation of sending, tested by nothing.

While it is running, `/` opens the palette. It is the worker's own command table
with `help` and `permissions` in front of it, so what it offers is what the
worker routes rather than a second list kept here. Typing after the slash narrows
it, so the same field that sends a message also finds a verb. fastrs reaches a
call with ctrl-o, which lists the calls, or with a double click on its row. Either
way the whole record costs a round trip on purpose: the wire carries the arguments
and a summary of the result, and the worker's session records hold the rest.

A palette is asked of the worker, and the worker answers `command.list`,
`skill.list` and `fs.list` on its reader thread rather than in its main loop,
because that loop is inside a turn until the turn ends. So the menus fill during
a run. `model.list` is the one exception: it asks the provider over the network,
and a handler that can block for seconds would hold up the thread cancel arrives
on, so it waits.

## The path an event takes

One line of stdout becomes a row on the screen in four steps, each in its own
file because each answers a different question:

```
stdout line ->  wire::parse        what did the worker say?
                session::apply     what is true of the conversation now?
                transcript::apply  what does the conversation hold?
                view::pieces       what does that row look like?
```

A frontend that wants to draw something else replaces the last step and nothing
before it. `--print` replaces that step too, which is why it cannot drift from
what the terminal shows.

## The fixture is the point

`fixtures/replay.jsonl` is one session's entire replay as real stdout, recorded by
`fixtures/record.py` against a throwaway `CUACODE_HOME` seeded with a copy of that
session (blobs included, because `session.load` calls `messages()`, which rehydrates
images). It is what lets a frontend be built and tested with no worker, no model, no
network and no API key.

It is committed, so what it is recorded *from* matters: use a session with nothing
personal in it. The one here is synthetic (`fixtures/make_session.py`, a made-up
project under `/Users/someone`), because the recording copies a session's content out
verbatim, secrets and paths included, and hand-editing it afterwards proves nothing.

```bash
python3 fixtures/record.py ../main.py <session-id>   # record a stream
cargo test --workspace                              # hold the types to it
rust/target/release/fastrs --print --fixture fixtures/replay.jsonl
```

The reasoning behind that exact test command, and the rest of the traps, are in
`AGENTS.md`. It is worth the two minutes before the first change.

## The app

`cuacode` (crate `cuacode-app`, `rust/app/`) is the app this tree is built
towards: one window, several projects, and the sessions standing in each.

```text
+------------+-------------------------------------+-------------+
| rail       |  top bar                            |             |
|  sessions  +-------------------------------------+  session    |
|  projects  |  the conversation                   |             |
|  sessions  +-------------------------------------+  plan       |
|  footer    |  the composer                       |             |
+------------+-------------------------------------+-------------+
```

**The rail** is the product: `Sessions`, then a heading per promoted project with
its sessions under it, then `No directory` for the sessions whose directory was
never promoted. A project folds; the row the pointer is on lights up; the
selected one wears a wash of the foreground.

**The conversation** is read out of the session's own record and folded, which is
the difference between a transcript and a log. Between one answer and the next,
every call and its result is **one row**: `5 calls · read, edit`, which opens on a
press to show each step and what it answered. A failing step is marked. The
model's reasoning is the run's own note, and it does not stack.

**The composer** is one card at the foot of the column, and it is where the next
run is decided:

| chip | what it sets | when |
|---|---|---|
| project | where the session stands | only until the session has started |
| provider | which account answers | always |
| model | which model on that account | always |
| permissions | `ask`, `auto`, `bypass`, `off` | always |

The location chip locks the moment a session is selected, and wears a lock rather
than a chevron so it reads as settled instead of broken. That is the worker's own
rule: it only takes a `cwd` before the first record.

**Where it reads.** Nothing is driven over IPC for the shell itself. The window
reads the state root directly, exactly as `handler/session/store.py` writes it:
`projects.json`, `sessions/*/meta.json`, a session's `messages.jsonl` (from the
end), its `todo.json`, and `config.json`. Plus two questions only the machine can
answer, each asked once and never in a frame: `git -C <dir>` for the changed
files, and the worker's own provider registry through one `python3 -c` at boot.
The worker is driven for liveness only (`agent.rs`): busy/stop, errors, and
permission asks. The disk stays the transcript.

**Why it is fast.** The render path does not touch the disk. Files are read on a
clock keyed on a `(length, mtime)` stamp, the transcript is read from **the end**
(the last 256KB, with the partial line it landed in dropped), the fold runs once
per read rather than per frame, and the clock naps at 33ms while something is
moving and 250ms when nothing is. The first pass read the transcript inside
`render`, and on a 2.8MB session that was 10.6ms of parsing per frame, 0.32s of
CPU for every second of hovering.

**Where the look comes from.** Superset's (`https://superset.sh`, ELv2), read out
of their own source rather than eyeballed off screenshots: `ember.ts` for the
palette, `globals.css` for the washes, their `PromptInput` for the composer, and
their rail and sidebar for the two columns.

Their dark theme is warm, and that is the whole character of it: a brown-black
`#151110`, greys that lean warm, a warm off-white `#eae8e6` that is never white,
and one hot orange `#e07850` for what you can act on. Two of their rules are kept
exactly: a row is marked by a low-opacity wash of the **foreground**, so a lit row
tracks its own text; and the accent means "you can act on this" and nothing else.

One place this window departs from theirs: **nothing here is translucent.** Their
window is a vibrancy material with translucent planes over it. This one is a
page, and every surface is opaque, for two reasons: `--background` there is an
opaque `#151110` and a translucent one over a bright desktop is not that colour,
and `theme.rs` can then check every tone against a contrast target instead of
against a wallpaper. Those checks are tests, not comments.

The dependency is one line:

```toml
gpui-kit = "0.7.0"
```

`gpui-kit` is a facade that pins and re-exports the matching `gpui-pre` release,
`gpui-base` and `gpui-component`. Two things worth knowing before touching it:

- `gpui` on crates.io (0.2.2) is a different, older line from the `gpui-pre`
  snapshots the component library builds against: `gpui-component` 0.7.0 pins
  `gpui-pre =0.3.7` exactly. Mixing the two fails to resolve.
- All of it is Apache-2.0. The GPL that makes Zed's UI crates dangerous to copy
  is in Zed's own crates, not in the framework or in these components.

The alternative is writing the text field by hand: GPUI ships no single-line
input, and its own `examples/input.rs` is 784 lines of it (IME, UTF-16 selection,
shaped lines, clipboard). The component library exists to spare that.

**Files.** `state.rs` reads the root and owns no drawing; `view.rs` draws and owns
no state; `theme.rs` is the palette and the numbers, with the contrast checks as
tests; `motion.rs` is the hover clock and the pulse; `main.rs` is the window, the
keys, the four composer menus and the cache; `agent.rs` drives one worker.
`app.sh` builds it as a real macOS bundle into `~/Applications`.

## fastrs

`fastrs` is the terminal frontend, built to a design rather than grown from the
one before it: a `#282c34` page, three lines of header, the conversation, the turn's
own readout, and an input fenced by two rules. It sets the terminal's own
background to the page colour, so a cell nothing draws is still the page.

The things in it that are decisions rather than transcription:

- Each row is led by a mark that says what it is and what became of it, in a shape
  rather than only in a colour: a reasoning row and the answer keep the dot, a call
  that ran gets a tick or a cross, and a call that never reported keeps a dim dot.
  The colour repeats the shape instead of carrying it, because four colours down a
  column is four things to tell apart from memory. The mockup's `⏺` is a glyph
  terminals paint as an emoji, so the dot is `●`: a filled circle with text
  presentation.
- A thought folds and a call does not. ctrl-t unfolds or folds every thought at
  once, so a long transcript is heads with the reasoning put away; a double click
  folds the one thought under the pointer, and that beats the global until the key
  is pressed again. A call is not part of that: it always shows what came back,
  because a column of calls saying nothing about their results is a list you have
  to open one line at a time to read. A double click on a call opens it in the
  panel instead. Thoughts sit tight folded and are given air when open. escape
  stops the round in flight (ctrl-k does the same, and with nothing running escape
  empties what has been typed). The click needs the terminal to report the mouse,
  which costs its own drag-to-select. Nothing on screen says any of it.
- A call reads as a call: `● write(crates/fastrs/src/main.rs)`, not a clipped
  `path=…`. The row keeps the arguments exactly as the wire sent them, so the head
  names what the call did -- the tool's own action where it has one, which is why
  `file(action=write)` is `write(...)` -- and puts the whole of the argument that
  says what it was in the brackets. Nothing is clipped to a fixed width: the head
  wraps like any other row. Under the head is what came back, one line, always
  there, with the pair the head already carries left out of it: a row that names
  the file twice has room for nothing else. The whole of the input -- a file's
  content, an edit's old and new as a code block with its own edges -- is not on
  the row at all: it is the panel behind a double click or ctrl-o, which draws the
  arguments and the result as the documents they are rather than as
  pretty-printed JSON.
- A path is shortened rather than cut off: relative to the directory the
  conversation is in, `~` for home, and a long one's middle dropped between its
  first part and its last two, so a file two folders down in
  `~/.cuacode/sessions/…/notebook` reads as `write(~/…/notebook/SESSION_BREAKDOWN.md)`.
  The end of a path is the part worth reading and the way there is what an edge
  clips off; the panel behind a call has it in full.
- A row keeps the spaces it was given. The wrap takes the one space a break falls
  on and nothing else, so a code line's indentation and a box's padding are still
  there after the row has had them. Collapsing either was how the first code block
  came out ragged, and a row that silently eats the indent is a row lying about
  what it holds.
- A call's verb is the one coloured thing on its head, and the argument in the
  brackets is the row's own colour -- except for the way to a file, which is
  fainter than the file at the end of it. A column of `photos(...)`, `shell(...)`,
  `write(...)` is then readable at a glance; a column of white lines is not, and
  that is what "this screen has no colour" was looking at.
- A diff is read by its colours, so it is drawn in them. What a call changed comes
  back as a unified diff, and the panel gives it a block of its own: the line that
  went in `failed` red, the line that came in `done` green, the file names and hunk
  markers dim because they are what the change is against rather than the change,
  and the lines kept for context fainter still. The permission prompt's `change:`
  section is the same diff in the same colours, because that is the one place a
  person is asked to read it before saying yes.
- ctrl-o lists the calls this conversation has made, newest first, in the menu a
  `/`, `$` or `@` trigger opens: type to filter, `↑↓` to move, enter opens the one
  under the cursor, escape closes it. Every row is the call's own row read back --
  the same mark in the colour of what became of it, the same verb in the call
  colour, the same argument -- and the title says how many the filter left, because
  the window shows eight whatever the list holds. That list is how a keyboard
  reaches a call that is not the newest, and the only other way to one is a double
  click on its row. Opening it fills the panel from the row at once -- the wire
  carries the arguments whole and only the result as a summary -- and asks the
  worker for its own record, which the worker answers off its reader thread so it
  works mid-run. The panel draws the arguments and the result as the documents they
  are rather than as pretty-printed JSON, and it scrolls to a clamped end.
- A call the worker is blocked on arrives as a panel too, not as a clipped line on
  the field: why the auto gate flagged it, the call as the model wrote it, and what
  that call says it would do. The panel covers the conversation and not the field,
  because the yes or no is typed on that line and a panel that hid the answer would
  be asking for one it hid.
- A row's detail hangs from a `└` at the same column as the row's dot, with the
  text two cells in, so a wrapped line uses the full width and its continuations
  land under the text rather than under the glyph. The corner is `#a5a5a5`.
- The turn's readout rides at the right end of the field line: a spinner while it
  works, a green dot once it is done, with the worker's token count and the
  elapsed time beside it. It is on the line you type on because a round in flight
  is the only thing that moves on its own, and a lone line above the rules reads
  as one more row of the transcript. The wire carries no clock, so the seconds are
  this frontend's own stopwatch, which is why they are tenths rather than a figure
  the worker sent. The readout is absent until this frontend has sent something,
  and the blink and the spinner are spaced off the clock rather than off the loop,
  which any mouse movement makes come round early.
- The model's own text is markdown and is drawn as most of it: bold, italic,
  both, strikethrough, `code`, links (with the address kept, because a terminal
  cannot follow one otherwise), headings, bullets, task lists, quotes, nested
  quotes, fences, rules and tables, with the markers gone. Code and links have the
  palette's two colours that are not the design's grey; a quote gets a bar per
  level, a task a box, a table its columns lined up, and a rule the width of its
  row. An image is drawn as its alt text and address, which is all a row can do with
  a picture. Anything else is drawn as written, because a row that quietly dropped
  something would be lying about the answer: no nested lists, no footnotes, no HTML.
- The mockup's 20px did not come across: a terminal has one size, and the frame is
  the same at 80 columns as at 200.
- The box is a small editor rather than a line that only grows at its end. The
  caret moves (a word at a time for ctrl or alt, the ends of the line for the
  command key), delete takes the character it is standing on, and shift-enter
  puts a new line in where the caret is rather than at the end of the message.
  A long line wraps inside the box rather than running off the right edge, on
  words, and a chip moves whole to the next row instead of being cut in half. The
  box grows with the message to eight rows and then scrolls to the caret, because
  the conversation is what the screen is for. Up, down, home and end
  belong to the message while it is more than one line and to the conversation
  when it is not. Shift-enter is bound in both encodings a terminal may put it
  in, because which one arrives is the terminal's business and not the key's: a
  terminal that reports shifted keys sends the shifted enter, and one that has the
  key mapped sends escape and return, which is what VS Code's terminal does.
  `fastrs --keys` prints what your terminal actually sends, for when a key does
  not do what it should. ctrl-j is bound with it as the backup, because raw mode
  makes 0x0A a key of its own, so it is a new line in a terminal that reports
  neither of the others.
- Under the box there is a status line: the state, what the context window is
  filling with, the rate, the turns, and the permission mode. Every figure on it
  is one the worker sent, and it is absent entirely when the worker has said
  nothing, because a line that is only there to be there is the thing this
  screen keeps off.
- Two verbs are the frontend's own, because the worker has a command for
  neither. `/help` opens the key list as a panel, and `/permissions` opens the
  three modes with the one in use marked, or sets the one it is given. They are
  read in the send path, so neither reaches the model as prose about a key list
  or a command that does not exist.

`fastrs --print --fixture <f>` is the way to see the transcript with no terminal,
and the tests in `view.rs` hold the dot rule, folding, and the width of the bar
under what you typed. `fastrs --keys` is the other end of it: it prints what the
terminal sends, byte by byte, for when a key does not do what it should.

The `/` palette and the session picker are both there now, the picker through
`--resume`. The permission prompt is there, on the field line, because the worker
blocks until it is answered, with the whole question in the panel above it: the
reason, the call, and what it would do.

A launch carries its message in the same field a person types into, and sends
it the moment the worker says it is up. That is `fastrs fix the parser` or
`fastrs prompt.txt`, above; with a message there is no question about which
session to open, because a launch handed something to do should be doing it.

## Where fastrs came from

fastrs replaces the terminal frontend that preceded it. That one was Go,
bubbletea with a cell buffer of its own - no markdown crate, no syntax crate - so
its renderer was its own too. What is here is a port of the shape and the
behaviour, not of those lines.

Ported: the frame (a feed, a status bar, an input row), the `/` palette, a panel
for anything you read, the session picker, the permission prompt.

Deliberately different: no syntax highlighting in fenced code beyond the shallow
one-line highlighter, and the provider and model verbs switch rather than only
reading out, because the `/` menu is the worker's own table. The earlier
frontend drew a row of keys in the corner of every frame and printed a greeting
at startup; neither is here, and `/help` is the one door. Its conversation was
boxed into a column, and this one takes the whole window. What that frontend kept
in a panel along the side lives behind `/` here, so no panel costs the
conversation its width.

## Not done

- fastrs highlights fenced code (comments, strings, keywords, numbers), one line at
  a time with no state carried between lines and no language read
- the app's conversation has no syntax highlighting in its code spans, and its
  model chip is display-only: a picker it has no list for would be pretending
- the rail renames a session (the worker's `session.rename`), but has no delete
  affordance and does not rename, re-icon or pin a project: those are the
  worker's verbs and the rail only reads them
- the app's field is single-line on purpose; mid-turn its send circle is the
  Stop, and sends the same `cancel` a terminal's escape sends
- the sidebar's `Files` tab is a placeholder, and a diff is drawn from its first
  400 lines
- neither frontend has been driven against a live model
