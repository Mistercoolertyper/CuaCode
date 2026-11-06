# The wire

One process. JSON objects in on stdin, one per line; JSON objects out on stdout,
one per line; diagnostics on stderr. Nothing else crosses the boundary: no
sockets, no files a client has to read, no import of anything in this tree.

That is the contract for both doors. A chat is a person sending `chat` and
watching the events arrive. A program is the same thing with nobody at the
keyboard: it sends `chat` with a `policy`, and reads the last event as its
result.

```bash
python3 main.py            # serve
python3 main.py --detached # serve, permission mode forced to "off" (a hosted box)
python3 main.py --wire     # print the commands this build answers, and exit
```

`--wire` reads the table in `handler/commands.py` that actually serves the
commands, so it cannot describe one that is not there. This file is the part
that table cannot say: the order things happen in, and what a client must answer.

## The envelope

Every line either way is:

```json
{"type": "cmd", "id": "a", "data": {...}}
```

- `type` — what kind of message this is. A `cmd` is a request; `token` and
  `status` are the worker's events; `permission` is the worker asking, and the
  answer carries the same type back. A type the worker does not know is ignored.
- `id` — the client's correlation id. Every event answering a command repeats
  it, so a client can tell one turn's events from the next one's. The worker
  generates ids for events it sends on its own (a title that finished, a
  `permission` request).
- `data` — the payload. Missing or non-object `data` reads as `{}`.

The first line out is always:

```json
{"type":"status","data":{"state":"ready","session_id":"20261106-103000-ab12",
  "mode":"ask","provider":"ollama","model":"deepseek-v4.1-flash","effort":"max",
  "context_max":1048576}}
```

Wait for it before sending anything. `state: ready` is not repeated.

## A command

```json
{"type":"cmd","id":"1","data":{"action":"chat","text":"hello"}}
```

`action` picks the handler. `python3 main.py --wire` lists all of them with the
`data` fields each one reads and whether it answers while a turn is running.
There are four groups:

- **a turn** — `chat`, plus `cancel`, `background`, `permission.mode`.
- **the conversation** — `session.*`, `project.*`.
- **settings** — `provider.*`, `model.list`, `vision.use`.
- **the menus** — `command.list`, `skill.list`, `fs.list`, `dir.list`,
  `provider.list`, `session.list`, `tool.detail`, `context.report`,
  `usage.report`. These are `direct`: the loop is inside a turn for its whole
  length, so anything a person needs to draw *during* one is answered on the
  reader thread instead. `--wire` prints `answers_mid_run` for each.

An action that is not in the table is answered, not ignored:

```json
{"type":"status","id":"1","data":{"state":"error","error":"unknown action: 'chta'"}}
```

An exception inside a handler comes back the same way, and the worker keeps
running.

## A turn

Send `chat`. What comes back, in this order:

1. `status` — `{"type":"chat_received","status":"..."}` for a `chat` that is
   about to generate. A blocking `/command` instead gets its notice and then
   `{"status":"idle"}`, and no turn happens.
2. `token` events, streamed:
   - `{"state":"notice"}` — a `/command`'s answer, or a runtime block put in
     front of the model. `token` is the text.
   - `{"state":"user"}` — only on a replay, never on a live turn: the client
     drew the user's own line when it sent it.
   - `{"state":"thinking"}`, `{"state":"content"}` — `token` is the text, and
     consecutive ones append. `thinking` is what the model reasoned, when the
     provider reports it.
   - `{"state":"tool_calls"}` — `token` is an array, in the provider's own
     dialect. Do not parse it; it is for drawing.
   - `{"state":"tool_output"}` — `token` is the tool's name, `index` is the
     ordinal of the call in this conversation (what `tool.detail` takes), and
     `result` is what a row says about it. The full result is in the
     conversation, not on the wire: a tool declares its own `brief`.
   - `{"state":"background"}` — a call moved to a job. `token` is the job id.
   - `{"state":"peer"}` / `{"state":"peer_waiting"}` — another agent spoke,
     or a message is queued behind the round in flight.
   - `{"state":"step_name"}` — the worker's small model named a step of the run.
3. `status` events, alongside:
   - `{"state":"usage"}` — the round's counts. Absent counters stay absent.
   - `{"state":"rate"}` — a live estimate while the model is generating.
   - `{"state":"retry"}` — the request failed transiently and is being retried.
   - `{"state":"compacting"}` — a fold is running, `stage` is `start` or `done`.
   - `{"state":"session_title"}` — the conversation was named or renamed.
4. Exactly one terminator:
   - `{"state":"done"}` — the model finished. `msg_count` is the history length.
   - `{"state":"cancelled"}` — the turn was stopped (`cancel`, or a budget).
   - `{"state":"error"}` — the request failed or the turn raised.

All three carry `result`:

```json
{"state":"done","token":"done","status":"done","msg_count":12,
 "result":{"text":"…the model's last message…","stop":"done",
           "messages":12,"session":"20261106-103000-ab12"}}
```

`stop` is the reason the turn is over, said by the only side that knows:
`done`, `cancelled`, `max_rounds`, `deadline`, `error`. A client that reads
`result.text` and `result.stop` has the whole answer; a client that draws a
transcript reads the tokens and ignores it.

## The policy: what a program says about a run

Every field is optional, and all of them are additive — a client that sends
just `text` gets an ordinary interactive turn.

```json
{"type":"cmd","id":"7","data":{"action":"chat","text":"summarise the diff",
  "policy":{"tools":["file","shell"],"max_rounds":8,"max_seconds":120}}}
```

- `tools` — the allowlist. Only these tools are offered to the model and only
  these may run. Absent means every installed tool. A name that is not
  installed is refused before the turn starts, as an `error` naming it, because
  an agent with a typo'd toolbox would otherwise spend a turn reporting the
  model's failure instead of the caller's.
- `max_rounds` — a ceiling on model calls. A round that has not opened is not
  counted, so stopping here spends nothing; the turn ends with
  `stop: "max_rounds"` and the history whole. One round is one request, so
  `1` means "no tool rounds".
- `max_seconds` — a ceiling on the wall clock, from the moment the turn starts.
  It is enforced as a cancel, at the same points a cancel is checked: between
  streamed chunks, between tool calls, and inside the retry waits. The turn ends
  with `stop: "deadline"` and whatever it produced so far is kept.

Both ceilings leave a conversation that can be resumed: send another `chat`, or
`session.load` it later. Nothing is rewound.

`text` also carries the inline commands: `/provider fr` switches provider and
ends the turn with `status: idle` and no generation; `/usage` answers and the
rest of the message still goes. `command.list` is the list.

## Permissions: the one request that blocks

In mode `ask` (and `auto`, when the gate flags a call), a tool that needs
asking sends:

```json
{"type":"permission","id":"<worker-generated>","data":{
  "name":"shell","kind":"permission","args":{"command":"rm -rf build"},
  "preview":{...},"note":"why the gate flagged it"}}
```

and **waits, with no timeout**, until an envelope with the same `id` arrives:

```json
{"type":"permission","id":"<same>","data":{"allow":true}}
```

Nothing else happens while it waits. `cancel` ends the wait (`allow` reads as
false). `ask_user` is the same envelope with `kind: "question"`, plus
`question`, `options`, `detail`, `n`, `of`; the answer is `{"choice": "…"}` or
`{"answer": "…"}`, and a plain `{"allow": true}` reads as the first option.

A program with nobody to ask has three answers, in order of preference:

1. `permission.mode` → `off` before the turn: the gate asks nothing at all.
2. `permission.mode` → `bypass`: ordinary calls proceed; the irreversible ones
   still block, which a program must then answer or cancel.
3. Answer every `permission` envelope as it arrives, with a policy of its own.

`--detached` forces `off` and cannot be talked out of it: a worker hosted on a
box has no person attached by definition, and a gate there is a hang.

## Starting, stopping, and the conversation

- `session.new` — a fresh conversation. The reply is `status` with
  `state: "session"`, plus the new `session_id` and the header fields, then a
  `session_title` with the (empty) name.
- `session.load` with `id` — status first, then the whole conversation replayed
  as the same `token` events a live turn sent (so a client draws it with code it
  already has), then `{"state":"done"}`.
- `session.list` — every conversation's `meta.json`, newest first. Safe mid-run.
- `session.delete` / `session.archive` / `session.pin` / `session.rename` /
  `session.effort` / `session.cwd`.
- `stop` — kill background jobs, commit the session, exit 0. Closing stdin does
  the same: a client that dies takes its worker with it, rather than leaving one
  generating into a pipe nobody reads.

Exit codes: `0` for `stop` or stdin closing, `2` for the removed
`--detach <file>` flag.

## What the wire does not carry

- **Blobs.** An image's bytes live in `messages`, and on disk under the session's
  `blobs/`. A `tool_output` for an image says `{"n": 1}`; a replay names the
  attachment and never sends its bytes.
- **Whole tool results.** A tool declares how much of its result a row needs
  (`Tool.brief`); the full one is in the conversation. `tool.detail` is how a
  client asks for one in full, and it is capped at 20k characters per string.
- **The provider's raw frames.** `state: "model"` names the model that answered.
  Everything else is already folded.

## How this stays true

Two things hold the shapes: `handler/commands.py` is the only place a command is
implemented, and `rust/fixtures/replay.jsonl` is a recording of a real turn that
`cargo test` parses through `rust/src/wire.rs`. A field added to an event is
additive by construction (a frontend reads the keys it knows); a field renamed
shows up as a fixture test failing, which is the point.

To re-record after a deliberate change to the shapes:

```bash
cd rust && python3 fixtures/record.py ../main.py <session-id>
```
