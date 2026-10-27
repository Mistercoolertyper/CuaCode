"""Stopping something that is already running. dispatch() blocks and the loop sits inside it,
so the handler runs on its own thread and the loop watches flags. Cancelled rewinds the round
past the call, which is what makes a missing result fine; detached keeps the thread and its box
for the background registry. A tool that ignores the token in ctx is abandoned rather than
stopped, which is why these threads are daemons: a socket read must never block app exit.
"""
import queue, socket, sys, threading

POLL = 0.02          # how often the flags are read while a call is in flight


class Token:
    """The cancel a running tool can see, handed to handlers through ctx["cancel"]. Looking at
    it is optional: a handler that never does still works, it just cannot finish early.
    """
    __slots__ = ("_event",)

    def __init__(self): self._event = threading.Event()
    def cancel(self): self._event.set()
    def cancelled(self) -> bool: return self._event.is_set()
    def wait(self, timeout=None) -> bool: return self._event.wait(timeout)

    # No __bool__ on purpose: truthy-when-cancelled would make `token or Token()` swap a
    # live token for a fresh one, truthy-when-present would make `if token:` a cancel test.


class Call:
    """One handler running on its own thread, plus the box it writes to. The box
    outlives the wait: whoever backgrounds the call keeps this object and reads the result
    whenever it lands, long after the loop that started it moved on.
    """

    def __init__(self, fn, token: "Token" = None):
        # `token is None`, never `token or ...`: a fresh token here would be unknown to
        # the ctx the handler reads, so the loop would cancel what nobody is watching.
        self.token = Token() if token is None else token
        self._fn = fn
        # done is written last, after result, so a reader that sees done sees
        # a result too. Nothing else touches these, which is why no lock.
        self.box: dict = {}
        self.thread = threading.Thread(target=self._run, daemon=True)

    def _run(self):
        try:
            self.box["result"] = self._fn()
        except BaseException as e:
            # Shaped like every other failed call, since that is what it is from the
            # model's side, and a traceback here would lose the job to nobody.
            self.box["result"] = {"error": str(e)}
        finally:
            self.box["done"] = True

    def start(self) -> "Call":
        self.thread.start()
        return self

    @property
    def done(self) -> bool: return bool(self.box.get("done"))

    @property
    def result(self): return self.box.get("result")


def ctx_with(ctx, token: Token):
    """A per-call copy of ctx carrying this call's cancel token, in ctx's own class since tools read
    its properties. Not a mutation: a background job holding the shared ctx would watch the next call's
    token appear and stop with it. "cancel" is for tools, "cancelled" for a nested loop's stop predicate;
    per call, so a backgrounded subagent keeps running after a later cancel.
    """
    try: out = type(ctx)(ctx) if ctx is not None else {}
    except Exception: out = dict(ctx or {})
    try:
        out["cancel"] = token
        out["cancelled"] = token.cancelled
    except Exception: return ctx
    return out


class Stopped(Exception):
    """Raised by stream() when the consumer's stop() fired mid-read. Not an error: the
    caller is already on its cancel path, and this is only what gets the iteration out of
    the way so that path runs.
    """


def abort(source) -> None:
    """Make a read blocked on this stream return, now.

    Closing an SDK stream is not enough: it returns at once and leaves the read exactly
    where it was, waiting on a socket nobody will ever write to again. The read is waiting
    on the socket, so the socket is what is shut down. Every step is best-effort: this runs
    while unwinding a cancel, and the stream may not have a connection under it at all.

    ponytail: httpx's transport only, which is what every SDK here uses. A transport that
    does not expose a network_stream leaves the read to the client's own timeout.
    """
    try:
        response = getattr(source, "response", None)
        network = ((getattr(response, "extensions", None) or {}).get("network_stream")
                   if response is not None else None)
        sock = network.get_extra_info("socket") if network is not None else None
        if sock is not None:
            sock.shutdown(socket.SHUT_RDWR)
            # Windows does not reliably wake a blocking recv() on shutdown() alone,
            # especially through an SSL wrapper. Closing the handle is what makes
            # the producer thread return there; elsewhere the graceful shutdown
            # is enough and lets an in-flight write finish.
            if sys.platform == "win32":
                sock.close()
    except Exception:
        pass


def stream(source, stop=None, first=None, poll: float = POLL, peek=None, every: int = 16):
    """The items of a blocking producer, on a thread, with stop() read while nothing arrives.

    generate() iterated the provider's stream directly, which parked the worker inside a
    blocking read: a cancel was only noticed between chunks, and a link that went away sends
    nothing rather than closing, so `stop()` was never reached and the turn could not be
    ended at all. The producer moves to its own thread and the caller polls, which is what
    makes the wait abandonable -- the same shape a tool call already has.

    abort() is the other half and a different job. Stopping gets the caller out at once, but
    the producer is still in the read it was in, and the socket under it has to be shut or
    that thread sits there until the client's own timeout. Both matter: one ends the turn,
    the other ends the connection the turn left open.

    peek() is asked on the same idle stretch, and what it finds is yielded as a Notice: a
    waiting peer message, which is a thing to say and not a reason to end the read. That is
    the difference from stop() and the whole point of it. The items are asked for in batches
    of `every` so the peeking costs nothing on a stream that is answering normally: a poll
    per frame is a poll per token on a fast model, and the idle stretch is the only place
    anything can have changed. A peek that raises is nothing to say: a mailbox fault is not
    the link's fault, and this is not the place it takes a turn down.

    Raises Stopped if stop() fires, and the producer's own exception otherwise.
    """
    stop = stop or (lambda: False)
    q: queue.Queue = queue.Queue()
    done = threading.Event()

    def produce():
        try:
            if first is not None: q.put(("item", first))
            for item in source: q.put(("item", item))
        except BaseException as e:
            q.put(("error", e))
        else:
            q.put(("end", None))
        finally:
            done.set()

    thread = threading.Thread(target=produce, name="cuacode-stream", daemon=True)
    thread.start()
    try:
        idle = 0
        while True:
            try:
                kind, payload = q.get(timeout=poll)
                idle = 0
            except queue.Empty:
                if stop(): raise Stopped
                idle += 1
                if peek is not None and idle % every == 0:
                    try: news = peek()
                    except Exception: news = None
                    if news: yield Notice(news)
                continue
            if kind == "item":
                yield payload
            elif kind == "error":
                # A cancel that shut the socket arrives here as the read's own error. It is
                # the stop it was, and saying otherwise would report a link that dropped.
                if stop(): raise Stopped from payload
                raise payload
            else:
                return
    finally:
        if not done.is_set():
            # Left early, so nothing is reading it. Shutting the socket is what returns the
            # producer from the read it is parked in, rather than leaving it there until the
            # client's timeout: a cancelled turn should not keep a connection to a provider.
            abort(source)
            thread.join(poll * 5)


class Notice:
    """Something the loop was told while it waited, that is not a provider frame.

    A wrapper rather than an item of the same shape, because the caller unpacks provider
    deltas field by field and a notice carrying `peek` would either read as an empty delta
    or be dropped by whoever looked first. `what` is for the caller; it is not drawn here.
    """

    def __init__(self, what): self.what = what


def run(fn, stop=None, detach=None, token: Token = None, poll: float = POLL) -> tuple[str, object]:
    """Run fn on a thread, watching both flags. Returns ("done", result), or ("cancelled" or
    "detached", call) with the still-running Call. stop cancels the token so a cooperative tool
    can stop; detach does not, the work continues. detach is cleared on entry, a press from before.
    """
    stop = stop or (lambda: False)
    if detach is not None: detach.clear()
    call = Call(fn, token).start()
    while True:
        # join returns the moment the thread ends, so the poll interval is a
        # ceiling on how long a flag goes unnoticed, not on how long a call takes.
        call.thread.join(poll)
        if call.done: return "done", call.result
        if stop():
            call.token.cancel()
            return "cancelled", call
        if detach is not None and detach.is_set():
            detach.clear()
            return "detached", call
