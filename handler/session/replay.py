from tools._parser.FromProvider import ToolCall
from handler.agent import providers
from handler import peers

def to_messages(records: list[dict], provider: str, vision: bool = None) -> list[dict]:
    """Records -> provider messages, by running the same adapters the live loop
    runs, so a reload cannot drift from what the agent actually sent.

    Tool results are flushed a round at a time rather than one per record, matching
    the loop: anthropic and gemini require every result from a round in a single
    message.

    The calls are rebuilt from the canonical record every time. A record holds names
    and args, not the shape some dialect wrapped them in: that shape belongs to the
    wire, the wire changes under a provider, and a session has to open after it does.
    """
    # Its own instance, never the shared singleton: result_messages re-inflates
    # stored images off p.vision, so vision has to be set from config here or the
    # class default fails the next request on a model that cannot see.
    from handler import config          # imported late: config imports providers
    p = providers.new(provider)
    p.vision = config.can_see(provider) if vision is None else vision
    msgs, calls, i, pending = [], [], 0, []

    def flush():
        nonlocal pending
        if pending:
            msgs.extend(p.result_messages(pending))
            pending = []

    for rec in records:
        t = rec.get("t")
        if t == "user":
            flush()
            attached = rec.get("images") or []
            turn = p.user_message(rec.get("text", ""), [i.get("b64", "") for i in attached])
            # Same helper and source as the live path, so a reopened turn is the one
            # that was sent.
            providers.append_user_text(turn, providers.attachment_note(attached))
            msgs.append(turn)
        elif t in ("recall", "peer"):
            # A runtime note or a peer's words ride the user turn they were attached
            # to (anthropic rejects two user messages in a row, and both are notes
            # about that turn); standalone if there is no user turn behind them.
            # A peer keeps its tag here, so reopening a conversation still says
            # these words were another agent's and not the person's.
            flush()
            text = (peers.render({"from": rec.get("from", ""), "from_name": rec.get("from_name", ""),
                                 "from_cwd": rec.get("from_cwd", ""), "ts": rec.get("ts", ""),
                                 "body": rec.get("text", "")})
                    if t == "peer" else rec.get("text", ""))
            if msgs and msgs[-1].get("role") == "user":
                # append_user_text rather than concatenation: a user turn with an
                # attachment holds its words in a content block, not a string, and the
                # note belongs in that block.
                providers.append_user_text(msgs[-1], text)
            else:
                msgs.append({"role": "user", "content": text})
        elif t == "compact":
            # A folded middle. It takes the place of the records it replaced, so it
            # has to open the conversation as a user turn: a session whose history
            # starts with an assistant message is rejected outright. Its own frame
            # says what it is, so nothing downstream mistakes it for something the
            # person typed.
            flush()
            text = ("<compacted>\nThe earlier part of this conversation was folded "
                    "down to this summary. It is what you know about it; say so "
                    "rather than assuming you still have the original.\n\n"
                    + (rec.get("text") or "") + "\n</compacted>")
            if msgs and msgs[-1].get("role") == "user":
                providers.append_user_text(msgs[-1], text)
            else:
                msgs.append({"role": "user", "content": text})
            # The request the fold swallowed comes back word for word, after the
            # summary and before the work: what it asked for is not a thing a
            # summary should be trusted to restate.
            if rec.get("request"):
                msgs.append({"role": "user", "content": rec["request"]})
        elif t == "assistant":
            flush()
            # Rebuilt from the canonical calls, which are names and args and nothing dialect
            # shaped. Ids are positional in every parser we have, so replaying the calls in
            # order regenerates the exact ids the tools ran under.
            native = p.native_calls(rec.get("calls", []))
            msgs.append(p.assistant_message(rec.get("thinking", ""), rec.get("content", ""), native))
            calls, i = p.parse_calls(native), 0
        elif t == "tool":
            # Call ids are positional in every parser we have, so replaying the calls
            # in order regenerates the exact ids the tools ran under.
            call = calls[i] if i < len(calls) else ToolCall(f"call_{i}", rec.get("name", ""), {})
            i += 1
            pending.append((call, rec.get("result", {})))
    flush()
    return msgs
