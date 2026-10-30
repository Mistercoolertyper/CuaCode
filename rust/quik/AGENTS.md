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

1. **Air is part of the row.** One blank line above a turn and above an answer, and
   none inside one: the conversation is read in turns, and a frame with no air in it
   is a log. The air is the first thing a short pane gives up, after the status row.
2. **A mark says what the row is.** `●` on a thought or an answer, `❯` on the line
   you typed, `└` on a call's result folding under its head. The mark is the row's
   kind, not its state: a live row is told apart by the text moving in it, and one
   glyph that changes meaning twice a second is a glyph nobody can read.
3. **A cost says what the work cost.** `0.3s · stdout:29B · +1` under a call, and the
   token figure and rate under a step. They come from the wire and nowhere else; a
   row that reports a number the worker did not send is a lie with three decimals.
4. **There is no header.** What a title bar would hold is on the opening screen: the
   mark, who would answer (model, provider, effort), and along the floor the
   directory, the permission mode and the build. On a conversation the same facts are
   on the status row. A title bar drawn above every frame said them twice and pushed
   the conversation down a row it did not have.
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
| `User` | `❯` and the text on the raised surface, and no rail: a rail down it made the record of what was said look like a second composer |
| `Content` | `●` and the prose at the body column. This is the baseline the rest is read against |
| `Thinking` | `●` and one quiet line, the text dim, folded to its head when the step is closed, with the step's own cost under it |
| `Tool` | `●`, `name(arg)` in one line, the fold under it when it is opened: a single call says nothing under its head, a run says what it ran |
| `Notice` | the head only, quiet, no mark. A payload is for the model |
| `Peer` | buddy mark in the peer colour, body hanging under it, opens on a press |
| `Error` | cross mark in `failed`, the message, and nothing folded |

A step is one row: a thought, its calls, the calls' results. It opens on a press
and it is the only fold. A new shape means a new `Row` variant in
`src/transcript.rs` first, and a frontend cannot invent one.

## The frame

Top to bottom: an optional update notice, the conversation, the box, the status row.
Nothing else, and no banner, key row, welcome line or border around the pane.

When the pane cannot hold all of it, they go in this order: the status row, the
notice, then the box's two rules. The message being typed and one row of it are
never given up.

The box is a rule, the message and a rule: `❯` is the only mark in it, and the
message keeps the whole width of the pane. A rail down its left, a foot under it and
a `▀` pedestal under that were three marks saying it was an input. The round's own
readout rides the right end of the message's first row, because that is the row the
person is looking at.

The status row: the context gauge and the rate on the left, the state or a fold, the
permission mode and the one key that is live on the right.

A menu draws over the tail of the conversation, above the field, as plain rows: no
border, no title on an edge, no footer of keys. The selected row is the one with a
`┃` beside it. It is as wide as its content wants, up to 68 cells, because a menu as
wide as the pane puts a row's hint half a screen from the word it explains. A
permission or a question is a panel above the field with a scroll of its own.

The opening screen is the mark: the word in opencode's own font
(`pantheon-org/opencode-font`, seven rows by four columns per letter, published as
grids), with `cua` in the muted tones and `code` in the bright ones and every letter
cut in the font's two bands - rows above `MARK_BAND` in the half's bright tone, the
rest one rung down. `MARK_GLYPHS` holds the letter grids, not shapes drawn to match
them. Then who would answer, then the box, and the floor line pinned to the bottom
edge. It is centred as one block and it is the only screen that shows the build.
Under `MARK_MIN` columns the art is replaced by the word.

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
- The marks the rows are drawn with: `●` a thought or an answer, `❯` the prompt,
  `▸` an opened fold, `└` a result under its head, `✓`/`✗` a passing and a failing
  call, `☑`/`☐` a plan's two states, `◆` a fold in progress, `○` an idle peer, `↑↓`
  in a picker's keys, `×` a removed attachment.

A new glyph from a family not on that list needs a line here and a reason, because
it is drawn from a fallback with a different advance width, stroke weight and
vertical centring: four families in one frame is why the type looks wrong even when
the font is right.

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
   has a fixture. Nothing asserts a string that a person is meant to read, because
   that test fails on the next copy edit and teaches nobody anything.
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

Measured on 2026-10-30.

| Thing | Now | Rule |
| --- | --- | --- |
| `main.rs` | 6358 | 1500, and a commit touching it comes out net negative |
| `view.rs` | 4867 | 1800, same |
| `App` fields | 42 | a new field names the one it replaces |
| `impl App` | 2437 lines, 91 methods | it draws, it keys, it feeds and it performs. Split |
| `draw` | 250 lines | 80. It is four functions: frame, box, menu, panel |
| `key` | 245 lines | 80, after `keys.rs` is the table |
| `view::step` | 297 lines, 11 parameters | 80, and 6 parameters. This is the fold, and it is the one thing that must be readable |
| `feed`, `perform`, `has_choices` | 162, 161, 163 | 80 |
| rows in a 24-row frame | 22 | 22 |
| glyph families | 1 | 1 |

## Traps

- `--frame` needs `--fixture`; a path that does not exist draws the empty app
  instead of failing, so an empty frame proves nothing about the conversation.
- `--colour` prints the cells as letters with a legend, and a `Reset` foreground
  in that legend is the harness not setting `PAGE`, not the app painting nothing.
- The fixture is a recording. `python3 fixtures/record.py ../main.py <id>`
  re-records it; never hand-edit one.
- `CUACODE_WORKER` is inherited. A frame from `run.sh` may have come from another
  checkout.
