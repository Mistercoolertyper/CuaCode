# Failures that say what happened

Same worktree, same branch. This is the report for `FALLBACKS-BRIEF.md`; the
per-line ledger is `FAILURES.md`, which lists all 518 pre-pass hits with the
verdict and the reason, including the ones left alone.

The one rule was: a fallback that hides a failure is the problem, a fallback that
reports it is fine. 88 hits changed. `handler/faults.py` is the new piece almost
all of them go through: one function, `note(subject, error, next_move)`, sending
one deduplicated sentence to stderr and returning it so a caller with somewhere
better to put it uses the same words.

## 1. What fails loudly now that failed quietly

**The wire refuses a shape it cannot read.** `Envelope.from_dict` was
`raw.get("type",""), raw.get("id",""), raw.get("data",{}) or {}`, and `data` that
was not an object was answered `unknown action: None`, which blames the caller's
spelling for the caller's shape. A line whose JSON was not an object at all
raised out of the reader thread; a line that was not JSON was dropped by a bare
`except JSONDecodeError: pass`. From the other end of the pipe, a worker that had
stopped listening and one that had died looked the same. All three are now an
error envelope naming the line, the field, the value and the shape that was
wanted. Unknown *types* and unknown *fields* still parse: that is the frontend
compatibility the wire is built on and is not the same thing as unreadable.

**A turn that lost a block says so.** The recall block, the project docs block and
the skill a message names each sat in `except Exception` with the value set to
`""`. None may fail a turn, and still none may, but a turn that ran without the
project's own notes is a different answer and now says so in the transcript.

**The toolbox menu says when it could not read the folder.** Five tools put their
list in their own description. Each caught every exception from its loader and
returned `{}`, so `describe()` told the model "No skills are installed" (and four
more like it): a statement of fact about the machine that was false, and the
model acted on it by not trying. The loader's reason now comes back with the list.

**The registry's hooks report.** A tool whose `describe`/`schema` raises keeps the
row it had, and says it is stale. A `brief` that raises still sends the whole
result, and says so. `dispatch` names the tool in the error the model reads.

**The loaders name the file they skip.** "One broken skill must not hide the
rest" was right, and silence was not: a skill with a bad header vanished, a
memory that could not be read counted as a fact that does not exist, and a
malformed agent file was simply never offered. Each is named once, with the
folder and the reason.

**A fold with no turn around it now works.** `_compact_now`'s documented fallback
imported `handler.session.naming`, a module that has never existed. The
ImportError was eaten by the same `except: pass`, so `/compact` outside a turn did
nothing, quietly, since the fallback was written.

**The MCP handshake says both failures.** Probing the stateless protocol and
falling back to `initialize` is a real compatibility layer. When both openings
failed, only the legacy one's error reached the caller, so the reason the probe
gave (usually the informative one) was lost. It names both.

**A lost output is not an empty one.** `tools/shell`'s drain thread returned
`("", "")` if `communicate()` raised, so a command that ran and printed something
came back as a command that printed nothing. The reason now goes where the output
would have been. Same shape for a screenshot that could not park a window, and for
a turn that died (`str(e)` alone became "the turn: ... - the round was rewound and
the conversation is whole").

**Numbers that lied.** A round whose spend could not be filed left the usage
report under-counting; a todo list that could not be parsed meant the plan was
never mentioned again; a context window that could not be obtained was cached as
zero for the life of the process. All three said nothing and reported the wrong
number; all three now report, and the window is retried instead of remembered.

## 2. The messages, and why these are better

The rule: subject, value, next move. Lowercase, no "Error:" prefix, one sentence,
no exclamation marks.

| before | after |
|---|---|
| `unknown action: None` | `stdin line 2: cmd envelope data must be a JSON object, got a str - the fields this worker reads live under "data"` |
| (nothing) | `stdin line 4 is not JSON: Expecting value at column 1 - one JSON object per line` |
| `str(e)` from a handler | `context.report: 'NoneType' object has no attribute 'get'` |
| `No skills are installed.` | `the skill list in ~/.cuacode/skills could not be read: <error> - fix or remove the file it names` |
| (nothing, turn continued) | `recall: the mailbox is a directory, not a file - this turn carries no recalled memories` |
| `{"error": "[Errno 13] Permission denied: '/b'"}` | `moving /a to /b: [Errno 13] Permission denied: '/b'` |
| `initialize: server exited during startup` | `demo: neither protocol answered - stateless 2026-07-28: no answer to server/discover in 3s; legacy 2025-06-18: initialize: server exited during startup` |
| `reconcile failed` | `the reconciler returned nothing for 6 reads; the question went unanswered` |

What makes them better is not length, it is that each one names a thing the
reader can act on: the line number they sent, the folder to look in, the two
paths of the move, both protocol revisions. The old ones named the exception and
left the reader to guess which of a dozen moving parts produced it.

## 3. What I deliberately left alone, and why

`FAILURES.md` has the reason per line. In groups:

- **Platform dispatch (78 lines).** `tools/app_open/_open_*`, `app_list/_list_*`,
  `screenshot/_capture_*`, `scroll/_scroll_*`, `_window.py`, `_appkit.py`,
  `_pointer.py`, `screenshot/_session.py`. Each module is imported only on its own
  OS, and an OS API that is absent on an older build is the same answer as support
  absent. `_pointer.py`'s `None` is a third state on purpose ("cannot ask" is not
  "asked, and no"), and `_window.py`'s by-path imports cannot be made type-safe
  without inventing a backend interface for one platform.
- **Provider and protocol compatibility.** Five provider adapters parse three SDK
  generations each; a field that is not there is `None` and the caller assembles
  from what is. `anthropic.py` synthesizes a `(no output)` text block because the
  endpoint rejects empty content. The MCP client's two eras are the same kind of
  thing. These are load-bearing, and the brief says so.
- **The wire's forward compatibility.** An unknown `type` is still ignored, and an
  unknown field is still carried through. `rust/src/wire.rs` is built on it.
- **Precedence and documented defaults (103 lines).** `cfg or load()`, `env or
  file`, ollama's `127.0.0.1:11434`, `$SHELL or /bin/sh`, `0o644`, and the `"?"`
  and `"image"` placeholders a row draws. A default whose replacement value is
  readable at the call site is not a value standing in for a missing one.
- **Narrow handlers (111 lines).** `except (TypeError, ValueError)` around one
  conversion, `except JSONDecodeError` around one parse, `except BrokenPipeError`
  around one write that flags the run to stop. The type list is the statement.
- **Retries that name their condition.** `_transient()` decides from status code,
  exception class and message, and says so on the wire (`state: retry`, attempt,
  seconds, error). The 413 fold retry only repeats while the fold freed
  something. Neither exists so a bug does not show.
- **The designed safe defaults.** `worker.bypass_gate` asks about a tool that
  cannot say whether it is irreversible; `_needs_ask` fails closed to asking;
  `_safety/rules.py` reads an unreadable rule set as "no extra allowances"; the
  auto gate returns `(False, "permission gate failed: ...")`. Every one fails
  toward the person, not away from them, and every one now says it happened.
- **A peer that is not there is not a failure.** `roster`'s `{"ok": False}` and
  `photos` raising with its per-source errors are documented answers, and
  `wait`'s 10-second cap says what it actually waited.
- **`except BaseException` at a thread boundary.** Two places hand an exception
  back as a value because a thread cannot raise into its caller; both are read
  by the loop, which decides whether it is transient. The MCP one now carries a
  label.

## 4. Checks

One new test per group, all in the tree's own style (a script, no framework):

- `tests/test_protocol.py`: six bad shapes raise `ValueError` naming the field and
  the value; unknown type, unknown field, missing `data` and `null` still parse;
  against a live worker, three bad lines get three answers, the shape error
  answers on its own id, and a good command still works afterwards.
- `tests/test_turn_blocks.py`: a fold outside a turn reads the live session; three
  failing blocks produce three notices, each naming its block, the error, and
  what the turn goes without.
- `tests/test_menu_failures.py`: each of the five menus, with its loader broken,
  says it could not be read and does not claim there are none; the schema stays a
  schema.
- `tests/test_mcp_handshake.py`: both openings failing names both reasons and both
  revisions; `protocol: modern` still refuses the fallback; `protocol: legacy`
  never probes.

The full suite is green (16 scripts), and the SDK probe from outside the repo
passes against `main.py`: `python3 .../sdk-probe/probe.py main.py`.

## 5. What is not done

- `generate()` is still 431 lines with two loops. The brief listed it as a place to
  look, not to split, and its fallbacks are the ones this pass fixed. Splitting it
  is the next pass's job, not a side effect of this one.
- `integrations/mcp/client.py` is still 957 lines with two handshakes. The dual
  failure now says both reasons; the file itself is unchanged in shape.
- The scanner is syntax-only. It over-reports boolean `or` (115 lines say so) and
  under-reports two shapes that matter: structural empties (566 counted, not
  listed, one of which was the brief's own example) and a handler that raises on
  one path. Both gaps are written down in `FAILURES.md`, with what was found by
  hand in spite of them.
