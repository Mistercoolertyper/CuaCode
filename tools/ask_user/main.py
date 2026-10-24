"""Ask the user a question and wait for the answer.

The reason this is a tool and not prose: a model that writes "shall I proceed with A
or B?" into its reply has ended its turn, so the answer costs a whole new round trip
and the machine sits idle in between. A tool call keeps the turn open, and the
frontend knows the difference between a question that blocks and text that does not.

It rides the envelope `permission` already uses. That is deliberate: it is the one
channel every frontend already implements, it already blocks the worker until a
person answers, and it already cancels correctly. What is added is `options` and
`question`; a frontend that does not know them draws the binary prompt it always
did, and a yes means the first option, so nothing hangs on an old build.

The answer comes back as either {"choice": "<one of the options>"} from a frontend
that renders the list, or {"allow": true|false} from one that does not.
"""
import json

# Long enough for someone to read and answer, short enough that a frontend which
# never renders the question does not wedge the turn forever.
TIMEOUT = 1800


def _ask(ctx, question: str, options: list, detail: str, n: int, of: int) -> dict:
    fn = ctx.get("ask_user") if hasattr(ctx, "get") else None
    if not callable(fn):
        return {"error": "this frontend cannot show a question; ask in your reply instead"}
    # n and of are 1-based and of is the total, so the screen can say "2 of 3"
    # rather than making someone wonder how many more there are.
    return fn(question, options, detail, n=n, of=of)


def run(args: dict, ctx) -> dict:
    questions = args.get("questions")
    if not isinstance(questions, list) or not questions:
        return {"error": "questions must be a non-empty list"}

    out = []
    for i, q in enumerate(questions):
        if isinstance(q, str):
            q = {"question": q}
        if not isinstance(q, dict):
            return {"error": f"question {i} is neither a string nor an object"}
        text = (q.get("question") or "").strip()
        if not text:
            return {"error": f"question {i} has no text"}
        options = [str(o) for o in (q.get("options") or []) if str(o).strip()]
        detail = (q.get("detail") or "").strip()
        got = _ask(ctx, text, options, detail, n=i + 1, of=len(questions))
        if "error" in got:
            return {**got, "answered": out}
        out.append({"question": text, **got})

    return {"answered": out}


def check():
    """The answer shape, without a frontend."""
    class Ctx(dict):
        pass

    def fake(question, options, detail, n=1, of=1):
        # A frontend that renders options replies with the chosen one.
        return {"choice": options[0]} if options else {"answer": "typed"}

    ctx = Ctx(ask_user=fake)
    r = run({"questions": [{"question": "which?", "options": ["a", "b"]}]}, ctx)
    assert r["answered"][0]["choice"] == "a", r

    # A plain string question still works, and free text comes back.
    r = run({"questions": ["why?"]}, Ctx(ask_user=lambda q, o, d, n=1, of=1: {"answer": "because"}))
    assert r["answered"][0]["answer"] == "because", r

    # No channel is a sentence, not an exception.
    r = run({"questions": ["why?"]}, {})
    assert "error" in r
    print("ask_user: ok")


if __name__ == "__main__":
    check()
