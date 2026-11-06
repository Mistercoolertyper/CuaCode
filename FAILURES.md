# Failures, as found

The inventory is one script, run over `handler/`, `tools/`, `integrations/` and
`main.py`:

```bash
python3 /Users/boaz/.cuacode/sessions/20261106-090710-e743/notebook/fallback-scan/inventory.py
```

It lists broad `except` handlers that do not re-raise, `or`/`.get` defaults applied
to values that can be missing, `try` inside `try`, and error strings that name no
subject. **518 hits in 83 files** before this pass. Each is one line
below with the line it had before the pass, because the whole point is that a
reviewer can check nothing was dropped, and the pass itself moved lines.

Four verdicts, and one more that the scanner makes necessary:

- **must fail** -- the failure was invisible to whoever read the result. Changed.
- **dispatch** -- platform or provider branching: several implementations, one of
  them running. Not a fallback.
- **compatibility** -- a difference between providers, SDK generations or protocol
  eras, remembered. Load-bearing.
- **real degradation** -- the layer survives on purpose and now says what it lost.
- **not a fallback** -- the scanner flagged a line that is boolean logic or a
  function's own default. Said rather than forced into a wrong bin.

## What the syntax scan misses, and what was found by hand

The scan reads shapes, not intent, so it is over-inclusive in one direction and
under-inclusive in the other. Two gaps are worth naming:

- **Structural defaults are counted, not listed**: 566 `or {}` / `or ""` /
  `.get(k, [])` sites in the tree today. Most are the type's own empty, which is
  not a claim about anything. The brief's own headline example was one of them --
  `Envelope.from_dict` turning a malformed line into an empty command -- so the
  count is here and that site is fixed below under "found by hand".
- **A handler that raises on one path** is not reported at all, because the scan
  asks "does this handler ever re-raise". The MCP client's era fallback was exactly
  that: `except _Legacy` re-raises when the config says modern and silently tries
  the legacy handshake otherwise.

Found by hand, and changed:

- `handler/protocol.py:16` `Envelope.from_dict` -- `data` that is not an object became
  `{}`, so a consumer's wrong shape came back as `unknown action: None`; a line
  whose JSON was not an object raised out of the reader thread. It now refuses with
  the field, the value and the shape that was wanted.
- `integrations/mcp/client.py:_handshake` -- when the stateless probe said "legacy"
  and the legacy handshake then failed, the error was the legacy failure alone. It
  names both openings and both reasons now.
- `handler/worker.py:turn` -- `_compact_now`'s import of `handler.session.naming`, a
  module that never existed (above, at `handler/agent/main.py:243`).

## The lines

| verdict | status | lines |
|---|---|---|
| compatibility | left | 11 |
| dispatch | left | 78 |
| must fail | changed | 88 |
| not a fallback | left | 115 |
| real degradation | left | 226 |

### handler/agent/background.py

- `23` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`isinstance(v, (str, int, float, bool)) or v is None`</sub>
- `54` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`self.finished or time.monotonic()`</sub>

### handler/agent/compact.py

- `120` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`settings.get("model") or config.model_for(name, cfg)`</sub>
- `303` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`ratio(w) or 1.0`</sub>

### handler/agent/effort.py

- `68` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`ctx.get("max_tokens", 64000)`</sub>
- `73` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`ctx.get("max_tokens", 32000)`</sub>
- `155` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`FALLBACK.get(dialect, Spec("none"))`</sub>

### handler/agent/flow.py

- `13` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`os.cpu_count() or 4`</sub>
- `17` `except` -- **real degradation**, left: a fan-out item's failure comes back as None, which flow.py documents at the top (a rate limit costs only its item); the workflow's agent() logs it in the run's own log <sub>`except Exception as e: if on_error: try: on_error(e) except Exception: pass return None`</sub>
- `20` `except (bare)` -- **must fail**, changed: a fan-out's error callback that raises is reported alongside the item's own failure <sub>`except Exception: pass`</sub>

### handler/agent/inline.py

- `101` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`ALIASES.get(text[i + 1:j], text[i + 1:j])`</sub>

### handler/agent/interrupt.py

- `46` `except` -- **must fail**, changed: Call._run catches BaseException because a thread cannot raise into its caller; the error is now labelled with the tool or request it was running <sub>`except BaseException as e: # Shaped like every other failed call, since that is what it...`</sub>
- `49` `error-string` -- **must fail**, changed: Call._run's sentence names the label it was given: the tool, or the request to the provider <sub>`error=str(e) with no subject in {"error": str(e)}`</sub>
- `71` `except` -- **real degradation**, left: ctx_with builds a copy so the call can be cancelled; a ctx whose own class refuses the copy falls back to dict, and the call still runs -- failing it would break tools over a ctx shape the loop does not own <sub>`except Exception: out = dict(ctx or {})`</sub>
- `75` `except` -- **real degradation**, left: same function: the token is attached to the copy when the class allows an attribute, and the call proceeds without it otherwise <sub>`except Exception: return ctx`</sub>
- `110` `except (bare)` -- **must fail**, changed: a socket shutdown that fails is reported: the producer thread may sit in a read until its own timeout <sub>`except Exception: pass`</sub>
- `138` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`stop or (lambda: False)`</sub>
- `146` `except` -- **compatibility**, left: the stream thread hands its exception back as a queue item; the caller (_open, then the round loop) is the one that decides whether it is transient, and it does <sub>`except BaseException as e: q.put(("error", e))`</sub>
- `166` `except` -- **must fail**, changed: a peer peek that raises is reported: the mail would wait for the round's own end <sub>`except Exception: news = None`</sub>
- `203` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`stop or (lambda: False)`</sub>

### handler/agent/main.py

- `43` `except` -- **compatibility**, left: a thread cannot raise into its caller, so _opened returns ("err", e); the round loop reads it, retries what is transient and reports the rest <sub>`except BaseException as e: return "err", e`</sub>
- `109` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (TypeError, ValueError): pass`</sub>
- `114` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (TypeError, ValueError, AttributeError): pass`</sub>
- `121` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`_retry_after(e) or min(RETRY_BASE * (2 ** attempt), RETRY_CAP)`</sub>
- `141` `except (bare)` -- **must fail**, changed: _preview still returns None (the question must not become a failed call) and says why the question has no preview <sub>`except Exception: return None`</sub>
- `152` `except` -- **must fail**, changed: _needs_ask still fails closed to asking, and says the safety check is why <sub>`except Exception: return True`</sub>
- `166` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`b.get("name") or "?"`</sub>
- `243` `except (bare)` -- **must fail**, changed: `from handler.session import naming` has never existed: the fold's documented fallback to the live session never ran, and the ImportError was the thing being swallowed <sub>`except Exception: pass`</sub>
- `257` `except (bare)` -- **must fail**, changed: a fold's start event that cannot be delivered is reported: the screen would otherwise look like a hang <sub>`except Exception: pass`</sub>
- `265` `except (bare)` -- **must fail**, changed: a fold's done event that cannot be delivered is reported: the frontend would spin forever <sub>`except Exception: pass`</sub>
- `295` `except (bare)` -- **must fail**, changed: _todo_note returns the failure sentence instead of "", so a plan that cannot be read is visible in the round <sub>`except Exception: return ""`</sub>
- `347` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`r.get("from_name", "?")`</sub>
- `357` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`a.get("name") or "image"`</sub>
- `376` `except` -- **must fail**, changed: _skill_note puts the failure where the skill block would have been, so the model is told the block is missing <sub>`except Exception: block = ""`</sub>
- `401` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`think_end or began`</sub>
- `420` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`settings.get("provider", "ollama")`</sub>
- `421` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`provider_obj or providers.get(provider)`</sub>
- `422` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`settings.get("model") or p.default_model`</sub>
- `434` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`settings.get("vision", getattr(p, "vision", True))`</sub>
- `439` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`cancelled or (lambda: False)`</sub>
- `473` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`int((usage or {}).get("in") or 0) or measured`</sub>
- `491` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`settings.get("keep_images", images.DEFAULT_KEEP)`</sub>
- `620` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except interrupt.Stopped: # The read was still waiting when the cancel arrived, and the...`</sub>
- `629` `nested-try` -- **must fail**, changed: the stream-close inside the drop handler now reports; the nesting is the drop path itself <sub>`try: stream.close() except Exception: pass`</sub>
- `630` `except (bare)` -- **must fail**, changed: same site: reported instead of pass <sub>`except Exception: pass`</sub>
- `641` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`think_end or began`</sub>
- `644` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`thinking or content`</sub>
- `672` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`thinking or content`</sub>
- `775` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`sent.get("name", "?")`</sub>
- `803` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`got_mail or (peers.drain(peer_sid) if peer_sid else [])`</sub>

### handler/agent/providers/anthropic.py

- `33` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`self._key or "not-needed"`</sub>
- `101` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`blocks or [{"type": "text", "text": content or thinking or "(no output)"}]`</sub>
- `102` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`content or thinking or "(no output)"`</sub>
- `113` `except (bare)` -- **compatibility**, left: the provider adapters parse three SDK generations; a shape that is not there is None and the caller assembles from what is <sub>`except Exception: return None`</sub>
- `127` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`b.get("type", "tool_use")`</sub>

### handler/agent/providers/base.py

- `125` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except json.JSONDecodeError: return None`</sub>
- `136` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`i.get("name") or "image"`</sub>
- `190` `except (bare)` -- **compatibility**, left: same: reading a field an older SDK does not have <sub>`except Exception: return None`</sub>
- `218` `except (bare)` -- **compatibility**, left: same: a usage frame in a dialect that does not carry that counter <sub>`except Exception: return {}`</sub>
- `241` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`(host or "").strip() or "127.0.0.1:11434"`</sub>
- `245` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`u.hostname or "127.0.0.1"`</sub>
- `246` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`u.port or (443 if u.scheme == "https" else 11434)`</sub>
- `248` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return False`</sub>

### handler/agent/providers/ollama.py

- `19` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: pass`</sub>

### handler/agent/providers/openai.py

- `52` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`self._key or "not-needed"`</sub>
- `76` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`self.base_url or 'this endpoint'`</sub>
- `99` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`think or content`</sub>
- `115` `except` -- **compatibility**, left: same: the finish reason is optional in the stream <sub>`except Exception: found = None`</sub>
- `136` `except (bare)` -- **compatibility**, left: same: an optional frame <sub>`except Exception: return None`</sub>
- `164` `except (bare)` -- **compatibility**, left: same: a count the frame did not carry is 0, which is what 'absent' means here <sub>`except Exception: return 0`</sub>
- `203` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except json.JSONDecodeError: args = {}`</sub>

### handler/agent/stepname.py

- `144` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`call.get("name") or "?"`</sub>
- `170` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`asked[:400] or '(not known)'`</sub>
- `171` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`above or '(none)'`</sub>
- `172` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`think or '(nothing)'`</sub>
- `173` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`ran or '(nothing)'`</sub>
- `191` `except (bare)` -- **must fail**, changed: a step the namer cannot label is reported once, so a broken namer is not mistaken for a quiet one <sub>`except Exception: pass`</sub>

### handler/agent/subagent.py

- `96` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`spec.effort or inherited or config.default_effort()`</sub>
- `104` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`spec.provider or active["provider"]`</sub>
- `109` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`spec.model or (active.get("model") if provider == active["provider"] else entry.get("mo...`</sub>
- `115` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`model or config.model_for(provider)`</sub>
- `126` `except (bare)` -- **must fail**, changed: a subagent's progress callback that raises is reported; the run continues and its result still comes back <sub>`except Exception: pass`</sub>
- `157` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`chunk.get("content") or content`</sub>
- `216` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`more or content`</sub>
- `217` `except` -- **must fail**, changed: an agent failure names the agent: two can fail with the same provider text <sub>`except Exception as e: return {"error": str(e), "stopped": "error", "rounds": 0}`</sub>
- `218` `error-string` -- **must fail**, changed: same, in the value the caller reads <sub>`error=str(e) with no subject in {"error": str(e), "stopped": "error", "rounds": 0}`</sub>

### handler/agent/workflow.py

- `40` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`ns.get("DESCRIPTION") or (doc.split("\n")[0] if doc else f"workflow from {path.name}")`</sub>
- `54` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`ns.get("NAME") or path.stem`</sub>
- `69` `except (bare)` -- **must fail**, changed: a workflow log line that cannot be emitted is reported; it is still in the workflow's own log <sub>`except Exception: pass`</sub>
- `102` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except SyntaxError: return None`</sub>
- `109` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (ValueError, SyntaxError): continue`</sub>
- `145` `except` -- **real degradation**, left: a workflow file that will not load returns an error naming the file and the exception; nothing is hidden <sub>`except Exception as e: return {"error": f"{path.name} failed to load: {e}"}`</sub>
- `149` `except` -- **real degradation**, left: a workflow that raises returns an error naming the workflow, with its log and how many agents ran <sub>`except Exception as e: return {"error": f"{name} failed: {e}", "log": sink, "agents": c...`</sub>

### handler/catalog.py

- `82` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`sorted(have) or 'none installed'`</sub>

### handler/commands.py

- `51` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`here or os.getcwd()`</sub>
- `54` `error-string` -- **real degradation**, left: an empty `error` field is how this menu says there is none; it is filled from e.strerror when there is one <sub>`error=empty message in {"dir": want, "parent": "" if parent == want else parent, "entri...`</sub>
- `58` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError as e: out["error"] = e.strerror or str(e) return out`</sub>
- `59` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`e.strerror or str(e)`</sub>
- `74` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`root or os.getcwd()`</sub>
- `76` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`rel.startswith("/") or ".." in rel.split("/")`</sub>
- `83` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return {"dir": rel, "entries": []}`</sub>
- `111` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`r.get("note") or "<todo_status>" in text`</sub>
- `117` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`i.get("name") or "image"`</sub>
- `189` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`env.data.get("state") or "all"`</sub>
- `199` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`env.data.get("mode") or "ask"`</sub>
- `212` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`(env.data or {}).get("index", -1)`</sub>
- `213` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (TypeError, ValueError): want = -1`</sub>
- `223` `except` -- **must fail**, changed: a /context failure is answered as `context.report: <error>` <sub>`except Exception as e: w.ipc.reply(env, "status", {"state": "error", "error": str(e)})`</sub>
- `224` `error-string` -- **must fail**, changed: same: the readout is named in the message <sub>`error=str(e) with no subject in {"state": "error", "error": str(e)}`</sub>
- `231` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (TypeError, ValueError): days = 0`</sub>
- `248` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`w.sess.meta.get("cwd") or w.ctx.get("cwd") or os.getcwd()`</sub>
- `314` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (TypeError, ValueError): delta = 0`</sub>
- `352` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`env.data.get("name") or w.settings["provider"]`</sub>
- `558` `except` -- **must fail**, changed: _answer names the action it ran, so a consumer with a dozen commands in flight knows which one raised <sub>`except Exception as e: w.ipc.reply(env, "status", {"state": "error", "error": str(e)})`</sub>
- `559` `error-string` -- **must fail**, changed: same <sub>`error=str(e) with no subject in {"state": "error", "error": str(e)}`</sub>
- `572` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`ipc.eof() or os.getppid() == 1`</sub>

### handler/config.py

- `71` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`value or "ask"`</sub>
- `89` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`added or more`</sub>
- `113` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: stamp = None`</sub>
- `121` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`added or respelled`</sub>
- `139` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`cfg or load()`</sub>
- `144` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`cfg or load()`</sub>
- `145` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`cfg.get("active") or "ollama"`</sub>
- `153` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`out.get("model") or model_for(name, cfg)`</sub>
- `160` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`cfg or load()`</sub>
- `199` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`cfg or load()`</sub>
- `201` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`p.get("mode") or "ask"`</sub>
- `203` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`p or "ask"`</sub>
- `211` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`cfg or load()`</sub>
- `253` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`entry(name, cfg).get("model") or getattr(providers.get(name), "default_model", "")`</sub>
- `323` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`model or model_for(name, cfg)`</sub>
- `335` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`model or model_for(name, cfg)`</sub>
- `345` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`cfg or load()`</sub>
- `346` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`name or cfg.get("active") or "ollama"`</sub>
- `347` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`model or model_for(name, cfg)`</sub>
- `352` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`model or name`</sub>
- `377` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`cfg or load()`</sub>
- `390` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`cfg or load()`</sub>
- `410` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`model or model_for(name)`</sub>
- `420` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`cfg or load()`</sub>
- `422` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`cfg.get("active") or "ollama"`</sub>
- `462` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`model or model_for(name, cfg)`</sub>
- `477` `except` -- **must fail**, changed: a context window that cannot be obtained is no longer cached as zero for the life of the process, and is reported <sub>`except Exception: found = 0`</sub>
- `546` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`cfg or load()`</sub>
- `547` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`cfg.get("active") or "ollama"`</sub>
- `551` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`e.get("model") or p.default_model`</sub>
- `560` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`e.get("vision", getattr(p, "vision", True))`</sub>

### handler/context.py

- `74` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`think_chars or reply_chars`</sub>
- `137` `except (bare)` -- **must fail**, changed: a count that fails is reported, so the missing /context row has a reason <sub>`except Exception: return 0`</sub>
- `203` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`settings.get("vision", getattr(p, "vision", True))`</sub>
- `240` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`LABELS.get(part["key"], part["key"])`</sub>

### handler/environment.py

- `21` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`settings.get("vision", default)`</sub>
- `26` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`found[0][1] or config.model_for(found[0][0])`</sub>

### handler/gate.py

- `57` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: pass`</sub>
- `69` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (OSError, KeyError, TypeError): return False`</sub>
- `83` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: continue`</sub>

### handler/peers.py

- `66` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: pass`</sub>
- `84` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (OSError, ValueError): return False`</sub>
- `90` `except (bare)` -- **real degradation**, left: a pid file that cannot be read or parsed is no pid; the caller is asking whether a worker is alive, and the answer is no <sub>`except Exception: return False`</sub>
- `94` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ProcessLookupError: return False`</sub>
- `96` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except PermissionError: return True`</sub>
- `105` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return 0.0`</sub>
- `122` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`meta or _meta(sid)`</sub>
- `125` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`os.path.basename(cwd.rstrip("/")) or "~"`</sub>
- `180` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return False`</sub>
- `228` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return 0`</sub>
- `293` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return []`</sub>
- `297` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: pass`</sub>
- `323` `except (bare)` -- **must fail**, changed: the subagent depth guard that cannot run is reported: it is what keeps an agent out of its parent's mailbox <sub>`except Exception: pass`</sub>
- `345` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`rec.get("from", "?")`</sub>
- `345` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`rec.get("from_name", "?")`</sub>
- `356` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`rec.get('from', '?')`</sub>
- `356` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`rec.get('from_name', '?')`</sub>

### handler/protocol.py

- `74` `error-string` -- **must fail**, changed: an exception inside a wire handler is answered as `<action>: <error>`, and the reader thread's direct handlers the same <sub>`error=str(e) with no subject in {"state": "error", "error": str(e)}`</sub>
- `74` `except` -- **must fail**, changed: same site: the reply names the action that raised; two handlers can raise the same text <sub>`except Exception as e: self.send("status", {"state": "error", "error": str(e)}, id_=env...`</sub>
- `80` `except-narrow (bare)` -- **must fail**, changed: a line that is not JSON is answered with an error naming the line and column, on the sender's id, instead of being dropped <sub>`except json.JSONDecodeError: pass`</sub>
- `125` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`text.strip() or images`</sub>
- `136` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`self._cancel.is_set() or self._eof.is_set()`</sub>
- `140` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`id_ or str(uuid.uuid4())`</sub>
- `146` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except BrokenPipeError: # The frontend is gone and nobody is left to read this. Flagged...`</sub>

### handler/session/main.py

- `44` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`self.meta.get("provider") or "ollama"`</sub>

### handler/session/store.py

- `10` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`os.environ.get(HOME_ENV) or "~/.cuacode"`</sub>
- `40` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`sid.startswith(".") or "/" in sid or "\\" in sid or "\x00" in sid`</sub>
- `56` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (OSError, json.JSONDecodeError): return {}`</sub>
- `66` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except json.JSONDecodeError: continue`</sub>
- `228` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`c.get("name", "?")`</sub>
- `243` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`meta.get("id", sid)`</sub>
- `263` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return False`</sub>

### handler/usage.py

- `63` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`source or "side"`</sub>
- `81` `except (bare)` -- **must fail**, changed: a round whose spend cannot be filed is reported: the usage report under-counts otherwise <sub>`except Exception: pass`</sub>
- `123` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`u.get("src") or "side"`</sub>
- `125` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`" ".join(x for x in (u.get("provider", ""), u.get("model", "")) if x) or "unknown"`</sub>
- `188` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`cutoff or _first_day(per_day)`</sub>
- `208` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError: return u`</sub>
- `327` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`max(d["in"] + d["out"] for d in recent) or 1`</sub>
- `362` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (IndexError, ValueError): print("usage: handler/usage.py [--days N] [--json]") r...`</sub>

### handler/version.py

- `27` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (OSError, subprocess.TimeoutExpired): return None`</sub>

### handler/worker.py

- `59` `except (bare)` -- **must fail**, changed: the Windows foreground-app probe reports through faults.note: without the name, screenshot cannot park the worker's window <sub>`except Exception: pass`</sub>
- `81` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError: return 0`</sub>
- `254` `except` -- **real degradation**, left: bypass mode's gate: a tool that cannot say whether it is irreversible is asked about, and the reason says so <sub>`except Exception as e: # A tool that cannot answer the question is asked about. The oth...`</sub>
- `293` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`self.sess.meta.get("cwd") or self.ctx.get("cwd") or os.getcwd()`</sub>
- `323` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`res.blocking or res.parallel`</sub>
- `327` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError as e: notice = f"{name}: {e}"`</sub>
- `335` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`self.settings.get('model', 'no model')`</sub>
- `339` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`self.settings.get('model', 'no model')`</sub>
- `342` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`self.settings.get('model', 'no model')`</sub>
- `352` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`config.vision_helper()[0] or 'default - whichever can see'`</sub>
- `438` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`said or "compact: nothing here worth folding yet"`</sub>
- `472` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`effort or self.sess.effort`</sub>
- `536` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (TypeError, ValueError): max_rounds = 0`</sub>
- `538` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (TypeError, ValueError): max_seconds = 0`</sub>
- `587` `error-string` -- **real degradation**, left: `status` is the event's state, not a message: the sentence is `unknown tool(s): ...` in `token` and in `result.text` <sub>`status=generic message 'error' in {"state": "error", "status": "error", "token": said, ...`</sub>
- `594` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`a.get("name") or "image"`</sub>
- `597` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`self.settings.get('model') or self.settings['provider']`</sub>
- `626` `except` -- **must fail**, changed: recall failing yields a notice in the turn saying it carries no recalled memories <sub>`except Exception: notes = []`</sub>
- `632` `except` -- **must fail**, changed: the docs block failing yields a notice saying the model was not pointed at this repo's docs <sub>`except Exception: docs = ""`</sub>
- `637` `except` -- **must fail**, changed: the skill block failing yields a notice naming what did not load <sub>`except Exception: skill_block = ""`</sub>
- `645` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`peer_tag.get("name", "?")`</sub>
- `695` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`ipc.cancelled() or out_of_time()`</sub>
- `755` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`chunk.get("messages", self.messages)`</sub>
- `759` `error-string` -- **real degradation**, left: same shape: `status: cancelled` is the state, and the message is the token beside it <sub>`status=generic message 'cancelled' in {"state": "cancelled", "token": "cancelled", "sta...`</sub>
- `765` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`chunk.get("messages", self.messages)`</sub>
- `769` `error-string` -- **real degradation**, left: same shape: the message is the provider's own `chunk["error"]`, passed through unchanged <sub>`status=generic message 'error' in {"state": "error", "token": chunk.get("error", ""), "...`</sub>
- `800` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`chunk.get("messages", self.messages)`</sub>
- `843` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`chunk.get("n", 1)`</sub>
- `853` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`chunk.get("show", chunk.get("text", ""))`</sub>
- `866` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`chunk.get("wire", result)`</sub>
- `870` `except` -- **must fail**, changed: the turn's own failure row names the turn and says the round was rewound, instead of being str(e) alone <sub>`except Exception as e: # A turn that dies mid-round leaves an assistant message whose t...`</sub>
- `877` `error-string` -- **must fail**, changed: the turn's own failure row names the turn and says the round was rewound, instead of being str(e) alone <sub>`status=generic message 'error' in {"state": "error", "token": str(e), "status": "error"...`</sub>
- `887` `except (bare)` -- **must fail**, changed: a generator whose own cleanup raises is reported: a provider socket left open is a leak nobody sees <sub>`except Exception: pass`</sub>

### integrations/instructions/loader.py

- `70` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return ""`</sub>
- `74` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return ""`</sub>
- `123` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return []`</sub>
- `129` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: continue`</sub>
- `142` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`sid or "-"`</sub>
- `169` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`sid or "-"`</sub>
- `170` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`state.get("turn_no", -1)`</sub>

### integrations/mcp/client.py

- `77` `except (bare)` -- **dispatch**, left: the Windows liveness probe falls back to psutil because os.kill(pid, 0) means something else there; without psutil the answer is 'not alive', which is the safe direction <sub>`except Exception: # noqa: BLE001 return False`</sub>
- `81` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ProcessLookupError: return False`</sub>
- `83` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except PermissionError: return True`</sub>
- `93` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (OSError, subprocess.SubprocessError): return ""`</sub>
- `102` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (OSError, subprocess.SubprocessError): return None`</sub>
- `120` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return`</sub>
- `137` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`f.read_text() or "{}"`</sub>
- `138` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (OSError, ValueError): rec = {}`</sub>
- `147` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`rec.get('name') or f.stem`</sub>
- `300` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`config.get("transport", "stdio")`</sub>
- `354` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: pass`</sub>
- `360` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`path.read_text() or "{}"`</sub>
- `362` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (OSError, ValueError): pass`</sub>
- `391` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: pass`</sub>
- `400` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`self.config.get("protocol") or "auto"`</sub>
- `439` `error-string` -- **compatibility**, left: an internal signal, not a message: its text is carried into the sentence that names both openings <sub>`str(exc) with no subject: raise _Legacy(str(exc))`</sub>
- `468` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`', '.join(supported) or 'nothing this client speaks'`</sub>
- `521` `except` -- **must fail**, changed: a server that will not die is reported; the kill-then-reap path is where it happens <sub>`except Exception: # noqa: BLE001 proc.kill() try: proc.wait(timeout=3) # a killed child...`</sub>
- `523` `nested-try` -- **must fail**, changed: the reaping wait inside the kill: its failure is reported with the pid <sub>`try: proc.wait(timeout=3) # a killed child still needs reaping except Exception: # noqa...`</sub>
- `525` `except (bare)` -- **must fail**, changed: same site: reported instead of pass <sub>`except Exception: # noqa: BLE001 pass`</sub>
- `541` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except json.JSONDecodeError: self._stderr.append(f"[non-JSON on stdout] {line[:200]}") ...`</sub>
- `558` `except (bare)` -- **must fail**, changed: the stdout reader dying is reported: every later call to that server goes unanswered <sub>`except Exception: # noqa: BLE001 pass`</sub>
- `572` `except (bare)` -- **must fail**, changed: the stderr collector dying is reported: why the server exits would be missing from every error <sub>`except Exception: # noqa: BLE001 pass`</sub>
- `600` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`version or self.protocol or PROTOCOL`</sub>
- `615` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`err.get('message', 'error')`</sub>
- `620` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`result.get("resultType", "complete")`</sub>
- `641` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except json.JSONDecodeError: self._stderr.append(f"[non-JSON in SSE] {body[:200]}") ret...`</sub>
- `693` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`session or self._session_id`</sub>
- `727` `except` -- **real degradation**, left: an HTTP error body that cannot be read leaves the MCPError to report its status code alone, which it does <sub>`except Exception: # noqa: BLE001 raw = b""`</sub>
- `731` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (json.JSONDecodeError, ValueError): message = None`</sub>
- `735` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`err.get('message', 'error')`</sub>
- `849` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except MCPError: return False`</sub>
- `877` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (json.JSONDecodeError, ValueError): out["text"] = joined`</sub>
- `900` `except (bare)` -- **must fail**, changed: a sweep of orphaned servers that fails is reported: one left by a dead worker stays running <sub>`except Exception: # noqa: BLE001 pass`</sub>
- `935` `except (bare)` -- **must fail**, changed: a connection that will not close is reported: its process may outlive the worker <sub>`except Exception: # noqa: BLE001 pass`</sub>

### integrations/mcp/loader.py

- `34` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`path.read_text() or "{}"`</sub>
- `35` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (json.JSONDecodeError, OSError) as exc: print(f"[mcp] ignoring {path}: {exc}", f...`</sub>
- `42` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`data.get("mcpServers", data)`</sub>

### integrations/mcp/servers/spotify/server.py

- `101` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError: return 0.0`</sub>
- `239` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("step", 10)`</sub>
- `410` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except SpotifyError as exc: # A tool that failed is a result, not a protocol error: the...`</sub>
- `414` `except` -- **compatibility**, left: a vendored MCP server answers a failed tool call as an error result; that is the MCP contract, and it names the exception type <sub>`except Exception as exc: # noqa: BLE001 return _text_result(f"{type(exc).__name__}: {ex...`</sub>
- `452` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except json.JSONDecodeError as exc: _fail(None, -32700, f"parse error: {exc}") continue`</sub>
- `457` `except` -- **compatibility**, left: one bad request must not take a stdio server down, which would look like a dead pipe to the client; it answers and keeps reading <sub>`except Exception as exc: # noqa: BLE001 # One bad request must not take the server down...`</sub>
- `482` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (KeyboardInterrupt, BrokenPipeError): pass`</sub>

### integrations/memory/loader.py

- `37` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`_CWD or os.getcwd()`</sub>
- `56` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`path or cwd()`</sub>
- `57` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`re.sub(r"[^a-z0-9]+", "-", os.path.basename(p).lower()).strip("-") or "root"`</sub>
- `64` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`scope or "global"`</sub>
- `100` `except (bare)` -- **must fail**, changed: a memory that cannot be read is named, instead of counting as a fact that does not exist <sub>`except Exception: return None`</sub>
- `102` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`meta.get("name") or p.stem`</sub>
- `104` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`meta.get("scope") or "global"`</sub>
- `106` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`meta.get("type") or "project"`</sub>
- `107` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`meta.get("source") or "agent"`</sub>
- `189` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`session or (prior.session if prior else "")`</sub>
- `198` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: pass`</sub>
- `223` `except (bare)` -- **must fail**, changed: a use count that cannot be written is reported; the read that earned it still happened <sub>`except Exception: pass`</sub>

### integrations/memory/naming.py

- `83` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError: return len(PRECEDENCE)`</sub>
- `115` `except (bare)` -- **must fail**, changed: a namer failure is reported on stderr, which is not the turn it was afraid of reaching <sub>`except Exception: # A name is a nicety: a failure is not worth surfacing to the user an...`</sub>
- `144` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except queue.Empty: return out`</sub>

### integrations/memory/recall.py

- `73` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`w.get(t, 1.0)`</sub>
- `77` `except` -- **must fail**, changed: a date nothing can parse is named once, instead of sorting last as if it were an old memory <sub>`except Exception: return 999.0`</sub>
- `92` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`sid or "-"`</sub>
- `98` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`sid or "-"`</sub>
- `129` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`m.get("title") or m.get("cwd")`</sub>
- `171` `except (bare)` -- **must fail**, changed: the memory half of recall failing is reported <sub>`except Exception: pass`</sub>
- `176` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`(meta.get("title") or "").strip() or "untitled"`</sub>
- `181` `except (bare)` -- **must fail**, changed: the session half of recall failing is reported <sub>`except Exception: pass`</sub>

### integrations/permissions/decider.py

- `102` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`got.get("reason") or ("allowed" if allow else "denied")`</sub>
- `104` `except` -- **real degradation**, left: the auto gate fails closed to (False, 'permission gate failed: ...'), which puts the call to the user with the reason attached <sub>`except Exception as e: return False, "permission gate failed: %s" % e`</sub>

### integrations/skills/loader.py

- `50` `except (bare)` -- **must fail**, changed: losing always_skills is reported: a person's forced list otherwise disappears from the system prompt for no stated reason <sub>`except Exception: return set()`</sub>
- `70` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`meta.get("name") or folder.name`</sub>
- `86` `except (bare)` -- **must fail**, changed: a broken skill is skipped and named, with the folder it lives in <sub>`except Exception: continue`</sub>
- `133` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError: continue`</sub>

### integrations/subagents/loader.py

- `31` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`meta.get("name") or path.stem`</sub>
- `46` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`meta.get("max_rounds", 8)`</sub>
- `57` `except (bare)` -- **must fail**, changed: a malformed agent file is skipped and named <sub>`except Exception: continue`</sub>

### integrations/workflows/research.py

- `53` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`got[0].get("sources") or urls`</sub>
- `59` `or (expression)` -- **must fail**, changed: an empty reconcile is answered with what came back and how many reads it had, instead of the two words "reconcile failed" <sub>`out or {"error": "reconcile failed", "reads": got}`</sub>

### tools/WebFetch/main.py

- `61` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return False`</sub>
- `64` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError: continue`</sub>
- `65` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`ip.is_private or ip.is_loopback or ip.is_link_local or ip.is_reserved or ip.is_multicas...`</sub>
- `91` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`r.encoding or "utf-8"`</sub>
- `129` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("mode", "digest")`</sub>
- `139` `except` -- **must fail**, changed: a failed fetch is named with the URL it was fetching <sub>`except Exception as e: return {"error": str(e)}`</sub>
- `140` `error-string` -- **must fail**, changed: same <sub>`error=str(e) with no subject in {"error": str(e)}`</sub>
- `142` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`ctype.startswith("text/html") or ctype.startswith("application/xhtml") or not ctype`</sub>
- `144` `error-string` -- **must fail**, changed: same <sub>`error=str(e) with no subject in {"error": str(e)}`</sub>
- `144` `except` -- **must fail**, changed: a markdown failure names the page and the way out: ask for mode text <sub>`except Exception as e: return {"error": str(e)}`</sub>

### tools/WebSearch/main.py

- `28` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("max_results", 5)`</sub>
- `31` `except` -- **real degradation**, left: a search error names the search and the exception <sub>`except Exception as exc: return {"error": f"search failed: {exc}"}`</sub>

### tools/_appkit.py

- `25` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: # An older pyobjc without the constant, or a policy change refused; n...`</sub>

### tools/_parser/Validate.py

- `19` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`{str: "string", dict: "object", list: "array"}.get(type(v), type(v).__name__)`</sub>
- `51` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError: return v`</sub>
- `56` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (json.JSONDecodeError, ValueError): return v`</sub>

### tools/_pointer.py

- `35` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return None`</sub>
- `39` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return None`</sub>

### tools/_safety/paths.py

- `40` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (OSError, ValueError, RuntimeError): pass`</sub>
- `66` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (OSError, ValueError, RuntimeError): return False`</sub>

### tools/_safety/rules.py

- `23` `except (bare)` -- **must fail**, changed: still fails closed to "ask", and says the rules could not be read <sub>`except Exception: # A tool asked whether a call is safe must never fail loudly: the cal...`</sub>

### tools/_safety/shell.py

- `151` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError: return None`</sub>
- `202` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`rec or "r" in a[1:] or "R" in a[1:]`</sub>
- `203` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`force or "f" in a[1:]`</sub>

### tools/_window.py

- `51` `get (real value)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`{"Darwin": "_open_macos", "Windows": "_open_windows"}.get(OS, "_open_linux")`</sub>
- `69` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: # noqa: BLE001 return {}`</sub>
- `76` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: # noqa: BLE001 return False`</sub>
- `84` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: # noqa: BLE001 return False`</sub>
- `92` `except` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: # noqa: BLE001 return True`</sub>
- `124` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return False`</sub>
- `213` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: # noqa: BLE001 return ""`</sub>
- `236` `except` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: hwnd = None`</sub>
- `264` `or (expression)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`name or (handle if isinstance(handle, str) else str(handle))`</sub>

### tools/agent/main.py

- `17` `except (bare)` -- **must fail**, changed: the subagent menu says it could not be read instead of "no subagents are installed" <sub>`except Exception: return {}`</sub>
- `23` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`spec.description or 'no description'`</sub>
- `45` `error-string` -- **real degradation**, left: catalog.unknown's error names the agent and lists the ones that exist <sub>`error=str(e) with no subject in {"error": str(e)}`</sub>
- `45` `except-narrow` -- **real degradation**, left: catalog.unknown's error names the agent and lists the ones that exist <sub>`except ValueError as e: return {"error": str(e)}`</sub>

### tools/app_list/_list_linux.py

- `19` `get (real value)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`cfg.get("Desktop Entry", "Name", fallback=None)`</sub>
- `21` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: continue`</sub>

### tools/app_list/_list_macos.py

- `15` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: pass`</sub>

### tools/app_list/_list_windows.py

- `9` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: pass`</sub>
- `29` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: continue`</sub>
- `31` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: continue`</sub>

### tools/app_open/_open_linux.py

- `14` `except` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: subprocess.Popen(["xdg-open", app], **_quiet)`</sub>

### tools/app_open/_open_macos.py

- `40` `except` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: _AX = False`</sub>
- `57` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: pass`</sub>
- `70` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return {}`</sub>
- `77` `get (real value)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`w.get("kCGWindowLayer", 1)`</sub>
- `85` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: continue`</sub>
- `107` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return None`</sub>
- `117` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: continue`</sub>
- `142` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return None`</sub>
- `204` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return False`</sub>
- `248` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return None`</sub>
- `270` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return None`</sub>
- `324` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: pass`</sub>
- `329` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return None`</sub>
- `344` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return 0`</sub>
- `425` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: pass`</sub>
- `491` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return False`</sub>
- `510` `except` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: out = {}`</sub>
- `534` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return None`</sub>

### tools/app_open/_open_windows.py

- `75` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: pass`</sub>
- `96` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return 0`</sub>
- `107` `except` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: name = ""`</sub>
- `139` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return`</sub>
- `145` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return None`</sub>
- `167` `except` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: # `start` runs the app under a fresh shell rather than beside the TUI...`</sub>
- `171` `nested-try` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`try: subprocess.Popen(["cmd", "/c", "start", "", app], shell=False, stdin=subprocess.DE...`</sub>
- `175` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return None`</sub>
- `204` `nested-try` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`try: for tid in threads: if tid and tid != current: try: win32process.AttachThreadInput...`</sub>
- `210` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: pass`</sub>
- `217` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: pass`</sub>
- `219` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return False`</sub>
- `237` `except-narrow (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except (TypeError, ValueError): return False`</sub>
- `258` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: pass`</sub>
- `276` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return False`</sub>
- `290` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return`</sub>
- `295` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return`</sub>
- `299` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return {}`</sub>
- `324` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return None`</sub>
- `333` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return 0`</sub>
- `339` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return ""`</sub>
- `345` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return 0`</sub>
- `355` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: pass`</sub>
- `405` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return`</sub>
- `419` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return []`</sub>
- `448` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return 0`</sub>
- `454` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return 0`</sub>
- `504` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return False`</sub>
- `511` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return False`</sub>

### tools/background/main.py

- `40` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("state") or "all"`</sub>
- `69` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`args.get("timeout") or DEFAULT_WAIT`</sub>

### tools/click/main.py

- `17` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("button", "left")`</sub>
- `18` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("clicks", 1)`</sub>

### tools/describe_image/main.py

- `48` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("source") or "screen"`</sub>
- `64` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`model or config.model_for(provider)`</sub>

### tools/file/_common.py

- `58` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (UnicodeDecodeError, UnicodeError): # Readable but not writable: re-encoding the...`</sub>
- `76` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`meta.get("encoding", "utf-8")`</sub>
- `76` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`meta.get("newline", "\n")`</sub>
- `83` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`meta.get("mode") or 0o644`</sub>
- `106` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError as e: return f"cannot stat {path}: {e}"`</sub>

### tools/file/_fs_ops.py

- `15` `error-string` -- **must fail**, changed: same <sub>`error=str(e) with no subject in {"error": str(e)}`</sub>
- `15` `except-narrow` -- **must fail**, changed: a listing failure names the directory it was listing <sub>`except OSError as e: return {"error": str(e)}`</sub>
- `26` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return {"error": f"{path} is a directory with contents -- " "delete its...`</sub>
- `31` `error-string` -- **must fail**, changed: same <sub>`error=str(e) with no subject in {"error": str(e)}`</sub>
- `31` `except-narrow` -- **must fail**, changed: reading a file before deleting it names both the path and the operation <sub>`except OSError as e: return {"error": str(e)}`</sub>
- `39` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (OSError, shutil.Error): where = None`</sub>
- `42` `error-string` -- **must fail**, changed: same <sub>`error=str(e) with no subject in {"error": str(e)}`</sub>
- `42` `except-narrow` -- **must fail**, changed: deleting names the path <sub>`except OSError as e: return {"error": str(e)}`</sub>
- `57` `error-string` -- **must fail**, changed: same <sub>`error=str(e) with no subject in {"error": str(e)}`</sub>
- `57` `except-narrow` -- **must fail**, changed: moving names both paths <sub>`except (OSError, shutil.Error) as e: return {"error": str(e)}`</sub>
- `68` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return {"name": p.name, "type": "unknown"}`</sub>

### tools/file/_io_ops.py

- `20` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`args.get("end", total)`</sub>
- `20` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("start", 1)`</sub>

### tools/file/_preview.py

- `17` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get('end', 'end')`</sub>
- `35` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return {"summary": f"overwrite {path}"}`</sub>
- `42` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return {"summary": f"edit {path}"}`</sub>

### tools/file/_search.py

- `53` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (OSError, subprocess.SubprocessError): return None`</sub>
- `62` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError: continue`</sub>
- `80` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except re.error as e: return {"error": f"bad regex: {e}"}`</sub>
- `82` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("include") or "*"`</sub>
- `91` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: continue`</sub>
- `97` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`truncated or idx < len(files) - 1`</sub>
- `112` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`truncated or idx < len(files) - 1`</sub>
- `132` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError: return False`</sub>
- `137` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return 0.0`</sub>
- `141` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return False`</sub>
- `146` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return False`</sub>

### tools/file/_undo.py

- `26` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: return []`</sub>
- `30` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError: continue`</sub>
- `51` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: pass`</sub>
- `85` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError as err: return {"error": f"undo failed: {err}"}`</sub>

### tools/file/_verify.py

- `5` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ImportError: yaml = None`</sub>
- `7` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ImportError: tomllib = None`</sub>
- `24` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except SyntaxError as e: return f"line {e.lineno}: {e.msg}"`</sub>
- `26` `except` -- **real degradation**, left: a verification that raises returns `Type: message`, which is what the file tool shows as the reason an edit could not be confirmed <sub>`except Exception as e: return f"{type(e).__name__}: {e}"`</sub>

### tools/file/main.py

- `34` `error-string` -- **must fail**, changed: same <sub>`error=str(e) with no subject in {"error": str(e)}`</sub>
- `34` `except-narrow` -- **must fail**, changed: a refused path names the action and tells the caller what a path may be <sub>`except ValueError as e: return {"error": str(e)}`</sub>
- `53` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError: return False`</sub>
- `71` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (ValueError, OSError): return None`</sub>

### tools/key/_key_linux.py

- `10` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`_ALIASES.get(p, p)`</sub>

### tools/loader.py

- `71` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`keys or ("output", "log", "text", "instructions", "description")`</sub>
- `113` `get (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`meta.get("name", folder.name)`</sub>
- `127` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`hasattr(mod, "describe") or hasattr(mod, "schema")`</sub>
- `145` `except (bare)` -- **must fail**, changed: a tool whose describe/schema raises keeps the row it had and is reported <sub>`except Exception: pass`</sub>
- `167` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`tools_dir or Path(__file__).parent`</sub>
- `196` `except` -- **must fail**, changed: a brief that raises still sends the whole result, and is reported <sub>`except Exception: return result`</sub>
- `207` `error-string` -- **must fail**, changed: same <sub>`error=str(e) with no subject in {"error": str(e)}`</sub>
- `207` `except` -- **must fail**, changed: the tool's name is in the error the model reads <sub>`except Exception as e: return {"error": str(e)}`</sub>

### tools/mcp/main.py

- `22` `except (bare)` -- **must fail**, changed: an unreadable server config is reported instead of reading as "no servers registered" <sub>`except Exception: # noqa: BLE001 return {}`</sub>
- `34` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`cfg.get("description") or "no description"`</sub>
- `114` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`cfg.get("session_tool") or "session_new"`</sub>
- `115` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`cfg.get("session_argument") or "session"`</sub>
- `179` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("server") or "?"`</sub>
- `210` `except-narrow` -- **must fail**, changed: an unknown server answer names the server <sub>`except ValueError as exc: # unknown server return {"error": str(exc)}`</sub>
- `211` `error-string` -- **must fail**, changed: same <sub>`error=str(exc) with no subject in {"error": str(exc)}`</sub>
- `212` `except-narrow` -- **must fail**, changed: an MCP error answer names the server <sub>`except client.MCPError as exc: return {"error": str(exc)}`</sub>
- `213` `error-string` -- **must fail**, changed: same <sub>`error=str(exc) with no subject in {"error": str(exc)}`</sub>

### tools/memory/main.py

- `19` `except (bare)` -- **must fail**, changed: an unreadable memory index is said in the tool description, not drawn as "nothing remembered yet" <sub>`except Exception: return ""`</sub>
- `62` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("type") or "project"`</sub>
- `63` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("scope") or "global"`</sub>
- `64` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("source") or "agent"`</sub>
- `82` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except ValueError as e: return {"error": str(e)}`</sub>
- `83` `error-string` -- **real degradation**, left: the ValueError comes from catalog.unknown, which names the memory and lists what exists <sub>`error=str(e) with no subject in {"error": str(e)}`</sub>

### tools/photos/main.py

- `44` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("max_size", 1920)`</sub>
- `52` `except` -- **real degradation**, left: a source that fails is kept in the result's own `errors` list with the source beside it, and an all-failed call raises with them <sub>`except Exception as e: results["errors"].append({"source": src, "error": str(e)})`</sub>
- `53` `error-string` -- **real degradation**, left: the same fact, in the per-source dict: `source` and `error` are two keys of one entry <sub>`error=str(e) with no subject in {"source": src, "error": str(e)}`</sub>

### tools/screenshot/_capture_linux.py

- `7` `or (real value)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`r.stdout.strip().replace(" ", "_")[:40] or "unknown"`</sub>
- `8` `except` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return "unknown"`</sub>

### tools/screenshot/_capture_macos.py

- `15` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: pass`</sub>
- `21` `or (real value)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`r.stdout.strip().replace(" ", "_") or "unknown"`</sub>
- `22` `except` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return "unknown"`</sub>

### tools/screenshot/_capture_windows.py

- `7` `except` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return "unknown"`</sub>
- `25` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: continue`</sub>

### tools/screenshot/_grid.py

- `10` `except` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: font = ImageFont.load_default()`</sub>

### tools/screenshot/_session.py

- `28` `except (bare)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`except Exception: return None`</sub>

### tools/screenshot/main.py

- `36` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("grid_size") or 100`</sub>
- `51` `except` -- **must fail**, changed: parking the worker's own window is reported: it cannot park itself and the capture is taken anyway <sub>`except Exception: self_snapped = False self_raised = False`</sub>
- `62` `except (bare)` -- **must fail**, changed: parking the apps that appeared is reported: a window opened during the capture stays where it landed <sub>`except Exception: pass`</sub>
- `74` `except` -- **must fail**, changed: parking the window in front is reported: the capture does not say which app is frontmost <sub>`except Exception: active_app = ""`</sub>
- `99` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("zoom") or 2`</sub>

### tools/scroll/_scroll_linux.py

- `10` `or (real value)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`abs(int(dy / 20)) or 1`</sub>
- `14` `or (real value)` -- **dispatch**, left: platform backend, imported only on its own OS: an OS API that is absent on an older build is the same answer as support absent, and there is no one on another platform to report it to <sub>`abs(int(dx / 20)) or 1`</sub>

### tools/shell/main.py

- `74` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`os.environ.get("SHELL") or "/bin/sh"`</sub>
- `104` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (ProcessLookupError, PermissionError, OSError, subprocess.TimeoutExpired): proc....`</sub>
- `125` `except` -- **must fail**, changed: a command whose output cannot be read says so where the output would have been, instead of returning two empty strings <sub>`except Exception: box["io"] = ("", "")`</sub>
- `142` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`box.get("io") or ("", "")`</sub>
- `169` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`args.get("timeout") or DEFAULT_TIMEOUT`</sub>
- `199` `or (expression)` -- **not a fallback**, left: the scanner is deliberately over-inclusive: this is boolean logic, or a default that is a function's own documented argument, not a value standing in for a missing one <sub>`cut_out or cut_err`</sub>

### tools/skill/main.py

- `16` `except (bare)` -- **must fail**, changed: the skill menu says it could not be read instead of "no skills are installed" <sub>`except Exception: return {}`</sub>
- `22` `or (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`s.description or 'no description'`</sub>
- `34` `error-string` -- **real degradation**, left: catalog.unknown's error names the skill and lists what exists <sub>`error=str(e) with no subject in {"error": str(e)}`</sub>
- `34` `except-narrow` -- **real degradation**, left: catalog.unknown's error names the skill and lists what exists <sub>`except ValueError as e: return {"error": str(e)}`</sub>

### tools/todo/state.py

- `45` `except (bare)` -- **must fail**, changed: the depth check failing is reported: 0 means "the main loop" <sub>`except Exception: return 0`</sub>
- `71` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (OSError, ValueError): # A missing file is the normal case -- most conversations...`</sub>
- `94` `except-narrow` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except OSError: _memory[_slot(ctx)] = data`</sub>
- `109` `except-narrow (bare)` -- **real degradation**, left: narrow: the named types are the statement of what can go wrong at this call, and the answer chosen there is the designed one -- the type list is what keeps it from being a catch-all <sub>`except (TypeError, ValueError): return None`</sub>
- `151` `except (bare)` -- **must fail**, changed: a todo list that cannot be read is reported; otherwise the plan is never nudged again and nobody knows <sub>`except Exception: return None`</sub>

### tools/type_text/main.py

- `15` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("delay", 0.0001)`</sub>

### tools/wait/main.py

- `10` `get (real value)` -- **real degradation**, left: a default whose replacement value is readable at the call site: a documented config default (the provider a fresh install uses, ollama's local endpoint), a placeholder a screen draws, a header an endpoint requires, a file mode -- not an answer standing in for a missing one <sub>`args.get("seconds", 1.0)`</sub>

### tools/workflow/main.py

- `15` `except (bare)` -- **must fail**, changed: the workflow menu says it could not be read instead of "no workflows are installed" <sub>`except Exception: return []`</sub>
