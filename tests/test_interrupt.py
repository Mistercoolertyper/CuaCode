#!/usr/bin/env python3
"""A turn that has stopped streaming can still be ended.

The bug this pins: generate() iterated the provider's stream on the worker's own thread, so
a cancel was only seen between chunks. A link that goes away (a wifi handover, moving between
two points in a building, a closed lid) sends nothing and does not close, so the main thread
sat in a blocking read and escape did nothing at all. Not ignored: unreachable.

Two mechanisms, doing two different jobs, asserted separately because it is tempting to think
the socket shutdown is the one that matters. It is not:

  - putting the producer on a thread and polling is what ends the turn. Measured: with
    abort() neutered the worker still stops in 0.13s, which is the negative control below.
  - abort() is what returns the abandoned producer from the read it is parked in, so a
    cancelled turn does not hold a connection to the provider until the client's timeout.

    python3 tests/test_interrupt.py
"""

import sys, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from handler.agent import interrupt

FAILS = []


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond and detail else ""))
    if not cond: FAILS.append(name)


def stall():
    """An endpoint that answers 200 and then says nothing more, ever.

    That is the shape a dead link has: the socket is alive, nothing arrives, nothing closes.
    """
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def do_POST(self):
            self.rfile.read(int(self.headers.get("content-length") or 0))
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.send_header("transfer-encoding", "chunked")
            self.end_headers()
            line = (b'data: {"id":"1","object":"chat.completion.chunk","created":0,'
                    b'"model":"m","choices":[{"index":0,"delta":{"content":"hi"}}]}\n\n')
            self.wfile.write(b"%x\r\n%s\r\n" % (len(line), line))
            self.wfile.flush()
            while True: time.sleep(3600)

        def log_message(self, *a): pass

    srv = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    srv.daemon_threads = True
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, f"http://127.0.0.1:{srv.server_address[1]}/v1"


def opened(url):
    from openai import OpenAI
    c = OpenAI(api_key="x", base_url=url, max_retries=0)
    return c.chat.completions.create(model="m",
                                     messages=[{"role": "user", "content": "hi"}], stream=True)


def stop_after(secs):
    """A stop() that flips once, the way a cancel arriving mid-read does."""
    flag = threading.Event()
    threading.Timer(secs, flag.set).start()
    return flag.is_set


def run_reader(stream, out, stop=None):
    """Where the loop sits: one thread draining the stream, errors kept for the caller."""
    def go():
        try:
            for d in interrupt.stream(stream, stop=stop): out.append(d)
        except BaseException as e:
            out.append(e)
    t = threading.Thread(target=go, daemon=True)
    t.start()
    return t


# 1. A read with nothing in it, ended by stop(). This is the fix: before it, the iteration
#    sat in the read and stop() was never looked at again.
srv, url = stall()
stream = opened(url)
first = next(stream)
check("the first chunk arrives before the stall", first.choices[0].delta.content == "hi")

got, t0, stopped = [], time.monotonic(), None
try:
    for d in interrupt.stream(stream, stop=stop_after(0.3)):
        got.append(d)
except interrupt.Stopped:
    stopped = time.monotonic() - t0
elapsed = time.monotonic() - t0
check("a stalled read ends on stop()", stopped is not None, f"no stop after {elapsed:.2f}s")
check("and promptly, not on the client's 600s timeout", stopped is not None and stopped < 2.0,
      f"{elapsed:.2f}s")
check("nothing was invented to fill the gap", got == [], got)
srv.shutdown()

# 2. The negative control, and the reason this test exists. Neutering abort() must NOT stop
#    the turn ending: it is the polling that does that, and the whole fix rests on it. If
#    this ever fails, the fix has been replaced by something that only works because the
#    socket happens to be shut, which is a different and much more fragile thing.
srv, url = stall()
stream = opened(url)
next(stream)
out = []
real_abort, interrupt.abort = interrupt.abort, lambda source: None
try:
    t = run_reader(stream, out, stop=stop_after(0.3))
    t.join(3.0)
    ended = any(isinstance(x, interrupt.Stopped) for x in out)
finally:
    interrupt.abort = real_abort
check("with the socket shutdown disabled, stop() still ends the turn", ended,
      f"polling alone was not enough: {out}")
srv.shutdown()

# 3. abort() does its own job: without it the producer stays parked in the read, which is a
#    connection to the provider held open by a turn nobody is watching any more.
srv, url = stall()
stream = opened(url)
next(stream)
out = []
t = run_reader(stream, out)
time.sleep(0.5)
interrupt.abort(stream)
t.join(3.0)
check("abort() alone returns a reader that had nothing to read", not t.is_alive(),
      "still parked in the read after abort()")
srv.shutdown()

# 4. A link that really dropped is still reported as dropped, not as a stop. The socket
#    shutdown looks like a network error from inside the read, and that distinction decides
#    whether the round closes as cancelled or as connection-lost. The producer's error is
#    only called a stop when stop() actually fired.
srv, url = stall()
stream = opened(url)
next(stream)
out = []
t = run_reader(stream, out, stop=lambda: False)
time.sleep(0.5)
interrupt.abort(stream)              # nobody asked for this: it is the link going away
t.join(3.0)
check("a drop nobody asked for comes back as the link's own error",
      out and isinstance(out[-1], BaseException), out)
check("and not as a stop", out and not isinstance(out[-1], interrupt.Stopped), out)
srv.shutdown()

print()
if FAILS:
    print(f"{len(FAILS)} failed: " + ", ".join(FAILS))
    sys.exit(1)
print("all checks passed")
