"""Inline commands: `/command arg` typed in or beside a message.

A message can carry a command in the middle of it ("explain X and then
/provider fr"), and the worker routes it instead of sending it to the model.
A token is only a command when the `/` starts a word and the name is one we
know; anything else is prose and goes to the model untouched.

Two classes. Blocking (`provider`, `model`, `vision`, `effort`) must run
before the request goes out, because the request would otherwise be built
against the setting the command changes; the result is shown and generation
waits for the next message. Parallel (`context`, `usage`) are read-only, and
the rest of the message still generates.

Dispatch lives in main.py, which holds the session and the config; this only
decides what in the text is a command and what a command needs.
"""
from dataclasses import dataclass

# The commands, in the order a menu should list them: name, description, and
# whether it must run before the request goes out. One not in BLOCKING is
# parallel by definition, and one not in this table is not a command at all,
# it is a path, a URL, or prose with a slash in it. The description is what the
# frontend's `/` menu shows, so a command added here reaches the parser and the
# menu in the same edit.
COMMANDS = (
    ("provider", "which provider answers, from here on", True),
    ("model",    "which model that provider runs", True),
    ("vision",   "which provider looks at pictures", True),
    ("effort",   "how hard it thinks: off low medium high max", True),
    ("cwd",      "where the next conversation stands, before it starts", True),
    ("compact",  "fold the conversation now, or set the limit it folds at", True),
    ("context",  "how much of the window the conversation fills", False),
    ("usage",    "what this and every other session has cost", False),
    ("version",  "which build of cuacode this worker is running", False),
)

# The old spelling, kept working and kept out of the menu. A command someone has
# in their fingers should go on working; a menu listing two names for one thing is
# a menu asking a question that has no answer.
ALIASES = {"compress": "compact"}

_ALL = frozenset(name for name, _, _ in COMMANDS) | frozenset(ALIASES)
_BLOCKING = frozenset(name for name, _, blocking in COMMANDS if blocking) | frozenset(ALIASES)


def listing() -> list:
    """Every inline command, for a frontend drawing a `/` menu."""
    return [{"name": name, "description": desc} for name, desc, _ in COMMANDS]


@dataclass
class Result:
    blocking: list          # [(name, arg)] must run before the model starts
    parallel: list         # [(name, arg)] can ride beside generation
    text: str              # the message minus its command tokens


def parse(text: str) -> Result:
    """Pull `/command arg` occurrences out of `text`.

    A command is a `/` that opens a word followed by a known name and an
    optional single-token argument; every other slash is left alone. The
    residue, prose and any unknown `/word`, comes back as `text`, so a parallel
    command leaves its message intact for the model.
    """
    blocking, parallel, out = [], [], []
    i, n = 0, len(text or "")
    while i < n:
        c = text[i]
        if c == "/" and (i == 0 or text[i - 1].isspace()) and i + 1 < n and text[i + 1].isalpha():
            # The name is the longest run of letters after the slash; a word
            # glued to it (providerfr) is not a command, so the next char must
            # be a break.
            j = i + 1
            while j < n and text[j].isalpha():
                j += 1
            name = ALIASES.get(text[i + 1:j], text[i + 1:j])
            if name in _ALL and (j >= n or text[j].isspace()):
                k = j
                arg = ""
                # Only blocking commands take an argument (/provider fr);
                # context and usage read nothing, so anything after is prose.
                if name in _BLOCKING:
                    while k < n and text[k].isspace():
                        k += 1
                    if k < n and (text[k].isalnum() or text[k] in "._:"):
                        k2 = k
                        while k2 < n and not text[k2].isspace():
                            k2 += 1
                        arg = text[k:k2]
                        k = k2
                (blocking if name in _BLOCKING else parallel).append((name, arg))
                i = k
                continue
        out.append(c)
        i += 1
    return Result(blocking=blocking, parallel=parallel, text="".join(out))


if __name__ == "__main__":
    # One runnable check: the smallest thing that fails if the parser breaks.
    def names(res):
        return [(n, a) for n, a in res.blocking] + [(n, a) for n, a in res.parallel]

    # The menu is not a second copy of the table: every command the parser
    # knows is one the frontend can list.
    assert {c["name"] for c in listing()} == _ALL
    assert all(c["description"] for c in listing())

    r = parse("explain X then /provider fr now")
    assert names(r) == [("provider", "fr")] and "fr" not in r.text
    assert names(parse("/context")) == [("context", "")]
    assert names(parse("/usage plus rest")) == [("usage", "")] and "rest" in parse("/usage plus rest").text
    # A slash glued to a word, a path, or an unknown name is prose.
    assert names(parse("a /providerfr /etc/passwd /nope")) == []
    assert parse("").text == ""
    print("inline parse ok")
