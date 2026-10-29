# quik

One conversation in a terminal. `rust/AGENTS.md` governs the tree; this file
governs `rust/quik/` and wins where the two disagree.

It exists because the screen was being redesigned faster than it was being
settled. An unsettled screen is one that every agent redesigns again, and that is
how a frontend grows five versions of the same row and stops being a product.

## What quik is

- The frontend the work goes into. `fastrs` is frozen: it is the same code, older
  and wider, and an edit there is an edit to a copy. A fix that must reach both
  goes in the shared half (`rust/src/`), never typed twice.
- A view of one conversation. Rows come from `cuacode::transcript`, state from
  `cuacode::session`, commands and skills from the worker. quik folds nothing,
  parses nothing, and names no tool.
- A terminal program, not a page. It has cells, a fixed grid, one font chosen by
  the person, and no way to draw a rounded corner. Everything below follows from
  that.

## The law of the screen

Nine rules. A change that breaks one is reverted, not argued with.

1. **Density is the design.** The conversation is why the window is open. A row
   gets its own line and nothing else: no blank line between rows of one kind, one
   blank line before a turn and none inside it. In a 40-row pane with ten turns, at
   least thirty rows are conversation. The current frame runs at six rows in
   twenty-four, and that is the single biggest reason it reads as generated.
2. **A mark is a state, so a mark that never changes is not drawn.** A dot on every
   row is a bullet list. `●` on all of them, and the eye has learned to skip the
   column, which means the one row that needed it is skipped too.
3. **Nothing on screen is an instrument reading.** `content:26B`, `v142.a0e3f78`,
   `4.2k tok`, a byte count under a call, a commit hash in the header. If the
   reader cannot act on it and it is not the conversation, it is a measurement of
   the program, and it belongs in a log. A result that is only a body says nothing
   at all: `read(path)` already said what was read.
4. **There is no header.** What a title bar would hold is already on the screen
   twice over: the model is under the field, one line from the key that sends to
   it, and the directory is the last row of the frame. The build id is `--version`.
   The one line that may appear above the conversation is the update notice, and
   only on the day origin has a commit this tree does not.
5. **One glyph family.** See below. Three glyph families in one frame is why the
   type looks wrong even when the font is right.
6. **Three levels of ink, not seven.** Text, quiet, and the one live thing in
   accent. A row that is dimmer than another row must be dimmer for a reason
   somebody can say out loud.
7. **The palette is ember's, and the accent is one colour.** A tone that is not in
   `ink` is a bug. The mint `#7ec699` is ember's terminal green on paper, but it is
   cold against a warm page and it is the green every generated page is: finished
   work is quiet, not green. `ink::done()` becomes the text tone and the accent
   stays the only saturated thing on screen.
8. **A placeholder is not a manual.** `Ask anything, or /help` teaches the program
   on the first line a person sees. The empty state says what this is in a word, or
   nothing.
9. **Motion says one thing.** One thing moves at a time: the run in flight. A
   shimmer that travels over a peer row, a pulse, a sweep and a blink in the same
   frame is a demo reel. `SHIMMER_STEP_MS` and `SHIMMER_MS` are the only clock in
   the conversation; anything else that animates needs a reason that is not "it
   looked flat".

## The rows

One shape per `Row`, and nothing draws a row another way.

| Row | Shape |
| --- | --- |
| `User` | the only surfaced block: rail at the left, `bar` behind it, the text on it, no lead mark |
| `Content` | unmarked prose at the body column, a blank line above it and none below. That one line is what says a run has finished |
| `Thinking` | one quiet line, the mark only while it is being written, the text dim, folded to its head when the step is closed |
| `Tool` | mark, `name(arg)` in one line, result folded under it only when opened. No byte counts, no durations |
| `Notice` | the head only, quiet, no mark. A payload is for the model |
| `Peer` | buddy mark in the peer colour, body hanging under it, opens on a press |
| `Error` | cross mark in `failed`, the message, and nothing folded |

A step is one row: a thought, its calls, the calls' results. It opens on a press
and it is the only fold. A new shape means a new `Row` variant in
`src/transcript.rs` first, and a frontend cannot invent one.

## The frame

Top to bottom: header, conversation, composer, status row. Nothing else, and no
banner, key row, welcome line or border around the pane.

When the pane cannot hold all four, they go in this order: the status row, then the
header, then the composer's padding, then the metadata under the message. The
message being typed and one row of it are never given up.

The composer is one block: a heavy rail down the left, the message on the raised
surface one cell inside it, and one line under the message carrying who will answer
it, how full the window is, and what the round is doing. No air row inside it, no
foot under it, no `▀` pedestal and no `❯` before the field: the rail is the mark, and
every one of those was a row or a glyph saying it again.

There is no status row. What it used to count lives on the composer's own meta line,
and the one key it ever needed to say, `esc stop`, rides there too and only while a
round is in flight.

A menu is rows and nothing else: no border, no title on the edge, no footer of keys.
It draws over the tail of the conversation, above the field, as wide as its widest
row, and the row under the cursor is the one with a rail beside it. A permission or a
question is a panel above the field with a scroll of its own.

## Glyphs, and the font

The font is the terminal's. The only thing quik controls is which glyphs it asks
for, and a glyph the terminal's text font does not carry is drawn from a fallback
with a different advance width, a different stroke weight and a different
vertical centring. That is why the rail, the pedestal and the prompt in one frame
look like three fonts: they are three fonts.

Allowed, and nothing else:

- ASCII, all of it.
- `U+2500..U+257F`, box drawing. Every cell of these is on the grid by
  definition, and it is the only family whose weight can be chosen per glyph.
- `U+2580..U+259F`, block elements, for a bar that is a bar.
- `·` and `…`, which every mono text font carries.

Banned: `❯ ▸ › ✓ ✗ ✦ ◆ ○ ● ☑ ☐ ↑ ↓ → ←`. They come from dingbats, geometric
shapes and arrows, which is three more families and at least one fallback each.

One weight per family per frame. A `┃` rail beside a `│` gutter beside a `╭╮`
corner is three weights of one family, and it reads as a font problem because it
is one.

## Where a thing lives

| File | Owns | Budget |
| --- | --- | --- |
| `view.rs` | `ink`, `PAGE`, the marks, `pieces`, `step`, `draw`, the rows | 1800 |
| `main.rs` | the window, `App`, the keys dispatch, the loop | 1500 |
| `menu.rs` | the `/`, `$`, `@` menus and the palette | 900 |
| `keys.rs` | the action table and `keys.json` | 800 |

A frontend owns no conversation logic and the shared half draws nothing. `ink` is
the only code that names a colour, `BODY_LEAD` and `LEAD` the only code that names
an indent, and a second copy of either is the bug.

## How a change lands

1. **One change.** One row, one panel, one key, one behaviour. A commit that
   touches four things cannot be looked at, and a screen that is only ever changed
   four things at a time is a screen nobody ever sees.
2. **Prove it with a frame.** Anything that draws runs
   `quik --frame --fixture ../fixtures/replay.jsonl 100x40` before and after, and
   the after goes in the commit body. `--colour` when a tone changed, `--open` when
   a fold did, `--keys` when a binding did. A claim about the screen that has no
   frame under it is a guess.
3. **A test for the fold, not for the pixels.** The row model has tests. The frame
   has the chrome harness instead:
   `cargo test -p cuacode-quik -- --ignored --nocapture show_the_chrome` prints the
   home screen, a conversation, the `/` menu and a typed message as they are laid
   out. Look at it before changing a frame and at the new one after. Nothing asserts
   a string a person is meant to read: that test fails on the next copy edit and
   teaches nobody anything.
4. **`cargo test --workspace` before the commit.** Not `cargo test`: it tests the
   root package and skips the frontends.
5. **A change that needs three files and a new field is a design problem.** Stop
   and say so in one line instead of threading a twelfth parameter through `step`
   or a forty-third field through `App`.
6. **A visual decision is written here or it does not exist.** The commit that
   changes what the screen looks like edits this file in the same commit, and says
   which line it changed. This is the rule that stops the loop: a decision that
   lives only in code gets relitigated by the next agent, and a decision that lives
   in prose gets argued with instead of re-derived.
7. **Deletion is a change.** Removing a mark, a row, a tone or a key is worth a
   commit and needs no justification. Adding one needs a line in this file.

## The debt

Measured on 2026-10-29, before the rules above existed.

| Thing | Now | Rule |
| --- | --- | --- |
| `main.rs` | 6368 | 1500, and a commit touching it comes out net negative |
| `view.rs` | 4881 | 1800, same |
| `App` fields | 42 | a new field names the one it replaces |
| `impl App` | 2437 lines, 91 methods | it draws, it keys, it feeds and it performs. Split |
| `draw` | 253 lines | 80. It is four functions: frame, composer, menu, panel |
| `key` | 245 lines | 80, after `keys.rs` is the table |
| `view::step` | 297 lines, 11 parameters | 80, and 6 parameters. This is the fold, and it is the one thing that must be readable |
| `feed`, `perform`, `has_choices` | 162, 161, 163 | 80 |
| rows in a 24-row frame | 22 of 30 non-blank, from 6 | 22 of 30 |
| glyph families | 1, box drawing and ASCII only | 1 |

## Traps

- `--frame` needs `--fixture`; a path that does not exist draws the empty app
  instead of failing, so an empty frame proves nothing about the conversation.
- `--colour` prints the cells as letters with a legend, and a `Reset` foreground
  in that legend is the harness not setting `PAGE`, not the app painting nothing.
- The fixture is a recording. `python3 fixtures/record.py ../main.py <id>`
  re-records it; never hand-edit one.
- `CUACODE_WORKER` is inherited. A frame from `run.sh` may have come from another
  checkout.
