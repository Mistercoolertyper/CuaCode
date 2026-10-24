#!/usr/bin/env python3
"""Write a synthetic session for the Rust fixture, so the recording stops carrying
a live one's secrets, serial numbers and paths.

    python3 make_session.py <home-dir>

Makes <home-dir>/sessions/20260101-090000-fx01/{messages.jsonl,meta.json}, in the
shape handler/session/store.py writes: user / recall / assistant / tool / usage
records. The conversation is a made-up project, and every path in it is made up
too, so nothing here is a fact about anyone.

Hand-buildable because the fixture only has to exercise the wire: record.py boots
the real worker against this session and the worker does the rest.
"""

import datetime
import json
import sys
from pathlib import Path

SID = "20260101-090000-fx01"
HOME = Path(sys.argv[1]).expanduser().resolve()
DST = HOME / "sessions" / SID
ROOT = "/Users/someone/work/lumen"

README = """# lumen

A small HTTP server that serves a directory, with a request log.

## Run

```bash
python3 -m lumen --port 8080 --root ./public
```

## Tests

```bash
python3 -m pytest tests/ -q
```

Log lines are UTC, one per request.
"""

ROUTER_V1 = """ROUTES = []

def route(method, pattern):
    def wrap(fn):
        ROUTES.append((method, pattern, fn))
        return fn
    return wrap

def match(method, path):
    for m, pattern, fn in ROUTES:
        if m == method and pattern == path:
            return fn
    return None
"""

ROUTER_V2 = """import re

ROUTES = []

def route(method, pattern):
    rx = re.compile('^' + re.sub(r'<(\\w+)>', r'(?P<\\1>[^/]+)', pattern) + '$')
    def wrap(fn):
        ROUTES.append((method, rx, fn))
        return fn
    return wrap

def match(method, path):
    for m, rx, fn in ROUTES:
        hit = m == method and rx.fullmatch(path)
        if hit:
            return fn, hit.groupdict()
    return None, {}
"""

APP = """import http.server, socketserver
from lumen.router import match

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        path = self.path.split('?')[0]
        fn, args = match('GET', path)
        if fn is None:
            self.send_error(404)
            return
        body = fn(**args)
        self.send_response(200)
        self.end_headers()
        self.wfile.write(body.encode())
"""

LOG_V1 = """import datetime

def line(method, path, code, ms):
    now = datetime.datetime.now().strftime('%H:%M:%S')
    return f'{now} {method} {path} {code} {ms:.1f}ms'
"""

LOG_V2 = """import datetime

def line(method, path, code, ms):
    now = datetime.datetime.now(datetime.timezone.utc).strftime('%H:%M:%SZ')
    return f'{now} {method} {path} {code} {ms:.1f}ms'
"""

TEST_ROUTER_V1 = """from lumen.router import route, match

def test_a_literal_path_matches():
    @route('GET', '/ping')
    def ping(): return 'pong'
    assert match('GET', '/ping') is ping

def test_an_unknown_path_misses():
    assert match('GET', '/nope') is None
"""

TEST_ROUTER_V2 = """from lumen.router import route, match

def test_a_literal_path_matches():
    @route('GET', '/ping')
    def ping(): return 'pong'
    assert match('GET', '/ping')[1] == {{}}

def test_a_segment_arrives_by_name():
    @route('GET', '/item/<id>')
    def item(): return 'x'
    assert match('GET', '/item/42')[1] == {{'id': '42'}}

def test_an_unknown_path_misses():
    assert match('GET', '/nope')[0] is None
"""

TEST_LOG = """from lumen.log import line

def test_a_log_line_is_utc():
    out = line('GET', '/ping', 200, 1.0)
    assert out.endswith('Z GET /ping 200 1.0ms')
"""

# (thinking, content, [(name, args, result)])
STEPS = [
    ("", "I'll start by reading the project docs and getting oriented.",
     [("file", {"action": "read", "path": "README.md"},
       {"result": {"path": f"{ROOT}/README.md", "content": README}}),
      ("shell", {"command": "git log --oneline -12"},
       {"result": {"stdout": "9f2c1ab add range requests\n41d0e5a split the router out of app.py\n"
                              "8b7a3c2 fix the log line's timezone\n10c4d9e first commit\n",
                   "code": 0}})]),

    ("The README is thin on the router. Let me look at the code before changing anything.",
     "Reading the router and the app that wires it up.",
     [("shell", {"command": "ls -la lumen tests"},
       {"result": {"stdout": "lumen/\n  __init__.py\n  __main__.py\n  app.py\n  router.py\n"
                              "  log.py\ntests/\n  test_router.py\n  test_log.py\n", "code": 0}}),
      ("file", {"action": "read", "path": "lumen/router.py"},
       {"result": {"path": f"{ROOT}/lumen/router.py", "content": ROUTER_V1}}),
      ("file", {"action": "read", "path": "tests/test_router.py"},
       {"result": {"path": f"{ROOT}/tests/test_router.py", "content": TEST_ROUTER_V1}})]),

    ("match() compares with ==, so there is no way to say /item/<id>. That is the thing to fix.",
     "The router needs pattern segments. Let me see how the app calls it first.",
     [("shell", {"command": "grep -rn 'match(' lumen/ --include='*.py'"},
       {"result": {"stdout": "lumen/app.py:24:        fn = match(method, path)\n"
                              "lumen/app.py:25:        if fn is None:\n", "code": 0}}),
      ("file", {"action": "read", "path": "lumen/app.py"},
       {"result": {"path": f"{ROOT}/lumen/app.py", "content": APP}}),
      ("shell", {"command": "python3 -m pytest tests/ -q"},
       {"result": {"stdout": "..\n2 passed in 0.04s\n", "code": 0}})]),

    ("A clean baseline. Now the change: patterns with named segments, and the handler taking them.",
     "Adding segment matching, keeping the literal case working.",
     [("shell", {"command": "python3 -c \"import re; print(re.fullmatch(r'/item/(?P<id>[^/]+)',"
                            " '/item/42').groupdict())\""},
       {"result": {"stdout": "{'id': '42'}\n", "code": 0}}),
      ("file", {"action": "write", "path": "lumen/router.py"},
       {"result": {"path": f"{ROOT}/lumen/router.py", "bytes": len(ROUTER_V2)}})]),

    ("", "The router now compiles a regex per pattern. Let me hold it to the tests, with one for a segment.",
     [("file", {"action": "write", "path": "tests/test_router.py"},
       {"result": {"path": f"{ROOT}/tests/test_router.py", "bytes": len(TEST_ROUTER_V2)}}),
      ("shell", {"command": "python3 -m pytest tests/ -q"},
       {"result": {"stdout": "....\n4 passed in 0.06s\n", "code": 0}})]),

    ("", "Green. The app still calls fn() with no arguments, so a route with a segment would "
         "raise. Passing the matched segments through.",
     [("file", {"action": "edit", "path": "lumen/app.py"},
       {"result": {"path": f"{ROOT}/lumen/app.py", "changed": 3}}),
      ("shell", {"command": "python3 -m pytest tests/ -q"},
       {"result": {"stdout": "....\n4 passed in 0.05s\n", "code": 0}})]),

    ("", "The log writes local time and the README says UTC. Small, but it makes a log "
         "useless later. Let me check what it does today.",
     [("file", {"action": "read", "path": "lumen/log.py"},
       {"result": {"path": f"{ROOT}/lumen/log.py", "content": LOG_V1}}),
      ("shell", {"command": "grep -rn 'utc\\|UTC\\|timezone' README.md lumen/"},
       {"result": {"stdout": "README.md:18:Log lines are UTC, one per request.\n", "code": 0}}),
      ("file", {"action": "edit", "path": "lumen/log.py"},
       {"result": {"path": f"{ROOT}/lumen/log.py", "changed": 2}}),
      ("shell", {"command": "python3 -m pytest tests/test_log.py -q"},
       {"result": {"stdout": "F\n1 failed\n\ntest_log.py:9: AssertionError: "
                              "assert '08:15:00' == '08:15:00Z'\n", "code": 1}})]),

    ("The test pins the old shape. It should pin the new one, so the fix is in the test.",
     "Updating the assertion to the shape the README promised all along.",
     [("file", {"action": "write", "path": "tests/test_log.py"},
       {"result": {"path": f"{ROOT}/tests/test_log.py", "bytes": len(TEST_LOG)}}),
      ("shell", {"command": "python3 -m pytest tests/ -q"},
       {"result": {"stdout": "......\n6 passed in 0.07s\n", "code": 0}})]),

    ("", "One more thing while I am here: a request for a file that does not exist should "
         "say so in the log too. Checking how a 404 is logged today.",
     [("shell", {"command": "grep -rn 'send_error\\|log_message' lumen/"},
       {"result": {"stdout": "lumen/app.py:27:            self.send_error(404)\n", "code": 0}}),
      ("file", {"action": "read", "path": "lumen/log.py"},
       {"result": {"path": f"{ROOT}/lumen/log.py", "content": LOG_V2}}),
      ("file", {"action": "edit", "path": "lumen/app.py"},
       {"result": {"path": f"{ROOT}/lumen/app.py", "changed": 4}}),
      ("shell", {"command": "python3 -m pytest tests/ -q"},
       {"result": {"stdout": "......\n6 passed in 0.05s\n", "code": 0}})]),
]

# The one result big enough that the worker abbreviates it rather than ship the
# text, which is the branch the fixture has to cover too.
BIG = "\n".join(f"-rw-r--r--  1 someone staff {1000 + i * 37:7d} Jan  1 09:{i % 60:02d} "
                f"asset-{i:03d}.{['png', 'css', 'js', 'svg'][i % 4]}" for i in range(140))


def main():
    recs = []
    # A real stamp, not an arithmetic trick: the shape these records carry is ISO, and
    # anything that walks past an hour has to roll over rather than print 09:61:00.
    base = datetime.datetime(2026, 1, 1, 9, 0, 0)
    step = [0]

    def stamp():
        step[0] += 1
        return (base + datetime.timedelta(seconds=step[0] * 3)).strftime("%Y-%m-%dT%H:%M:%S+00:00")

    recs.append({"t": "user", "ts": stamp(),
                 "text": "the router only matches literal paths and the log is in the wrong "
                         "timezone. fix both, and add a test for a path with an id in it."})
    recs.append({"t": "recall", "ts": stamp(),
                 "text": "recall: 1 memory may bear on this\n\n<recall-memories>\n"
                         "Memories that your tool list does *not* carry here -- they belong to "
                         "another project or to an app that is not on screen, so this is the only "
                         "mention of them you will get. Matched by word overlap on the name and "
                         "the line, which is a guess. Nothing has been read. `memory` with action "
                         "`load` opens one; ignore the rest silently -- do not mention them.\n"
                         "- `lumen-router-patterns` [projects/lumen-a1b2c3] - the router matches "
                         "literals only; segments were deferred on purpose\n</recall-memories>"})

    def add_assistant(thinking, content, calls):
        recs.append({"t": "assistant", "ts": stamp(), "p": "openai", "thinking": thinking,
                     "content": content, "calls": [{"name": n, "args": a} for n, a, _ in calls],
                           "u": {"in": 9000 + step[0] * 11, "out": 40 + step[0],
                                 "cache": 8000 + step[0] * 10,
                                 "secs": 1.2, "est": True, "model": "small-model",
                                 "provider": "openai"}})
        for n, _a, r in calls:
            recs.append({"t": "tool", "ts": stamp(), "name": n, "result": r})

    for thinking, content, calls in STEPS:
        add_assistant(thinking, content, calls)

    # Pad to a real conversation's shape: the transcript test wants >100 calls, and a
    # fixture that barely covers the wire is a weak tripwire. A real session is mostly
    # reads and greps, with a thought before the call, a second user turn now and then,
    # and a recall block riding the turns that came after a memory write.
    i, turn = 0, 0
    while sum(1 for r in recs if r["t"] == "tool") < 118:
        i += 1
        if i % 9 == 5:
            turn += 1
            recs.append({"t": "user", "ts": stamp(),
                         "text": f"ok, now do module_{i:02d} the same way."})
            recs.append({"t": "recall", "ts": stamp(),
                         "text": "recall: 1 memory outside this scope\n\n<recall-memories>\n"
                                  "Not from the user. The runtime matched words against things "
                                  "already stored and listed what looked close - pointers only, "
                                  "nothing loaded, no judgement about relevance.\n"
                                  f"- `lumen-module-{i:02d}` [projects/lumen-a1b2c3] - what "
                                  "module shape the other files use\n</recall-memories>"})
        add_assistant(f"module_{i:02d} is the same shape as the last one, so it is a quick read.",
                      f"Checking module_{i:02d}.py.",
                      [("file", {"action": "read", "path": f"lumen/module_{i:02d}.py"},
                        {"result": {"path": f"{ROOT}/lumen/module_{i:02d}.py",
                                    "content": f"def f_{i}():\n    return {i}\n"}}
                        if i % 4 else
                        {"result": {"path": f"{ROOT}/lumen/module_{i:02d}.py", "listing": BIG}})])

    # One usage record per assistant round, the way the live loop writes them.
    for r in [r for r in recs if r["t"] == "assistant"]:
        recs.append({"t": "usage", "ts": stamp(), "u": dict(r["u"])})
    recs.sort(key=lambda r: r["ts"])

    DST.mkdir(parents=True, exist_ok=True)
    with open(DST / "messages.jsonl", "w") as f:
        for r in recs:
            f.write(json.dumps(r) + "\n")

    kinds = {}
    for r in recs:
        kinds[r["t"]] = kinds.get(r["t"], 0) + 1
    turns = kinds.get("user", 0)
    meta = {"id": SID, "created": "2026-01-01T09:00:00+00:00",
            "updated": "2026-01-01T09:30:00+00:00", "provider": "openai", "origin": "openai",
            "model": "small-model", "effort": "low", "frontend": "fixture",
            "title": "lumen: segment routes and a UTC log", "title_source": "auto",
            "title_attempts": 1, "cwd": ROOT, "records": len(recs), "turns": turns}
    (DST / "meta.json").write_text(json.dumps(meta, indent=2) + "\n")

    print(f"wrote {DST}/  {len(recs)} records, {turns} turns")
    print("  " + "  ".join(f"{k}={v}" for k, v in sorted(kinds.items())))
    print(f"  size={(DST / 'messages.jsonl').stat().st_size // 1024} KiB")
    print(f"  tools={kinds.get('tool', 0)}  (the fixture's test wants >100)")


if __name__ == "__main__":
    main()
