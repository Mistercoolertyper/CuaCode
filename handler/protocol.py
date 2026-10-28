import json, queue, sys, threading, time, uuid
from dataclasses import dataclass
from typing import Callable, Optional


@dataclass
class Envelope:
    type: str
    id: str
    data: dict

    def to_dict(self) -> dict: return {"type": self.type, "id": self.id, "data": self.data}

    @classmethod
    def from_dict(cls, raw: dict) -> "Envelope":
        return cls(type=raw.get("type", ""), id=raw.get("id", ""), data=raw.get("data", {}) or {})


class IPC:
    def __init__(self, script: Optional[list] = None):
        self.inbox: queue.Queue[Envelope] = queue.Queue()
        self._lock = threading.Lock()
        self._pending: dict[str, Callable[[Envelope], None]] = {}
        self.terminal_info: dict = {}
        self._cancel = threading.Event()
        # A scripted run: these envelopes are handed to the loop before anything
        # else, and there is no stdin to read because there is nothing on the other
        # end of it. `main.py --detach` is the whole of the caller: one message from
        # a file, run with nobody attached.
        self.detached = script is not None
        self._script: list[Envelope] = [Envelope.from_dict(s) for s in (script or [])]
        # Handed straight to generate(), which clears it as it consumes it:
        # one press moves one call.
        self.background = threading.Event()
        # Set when stdin hits EOF, meaning the frontend that owned the pipe is gone:
        # the main loop checks it, so a dead frontend leaves no orphan worker.
        self._eof = threading.Event()
        # True while a turn is running, which is what tells a chat envelope
        # whether it is the next message or a mid-turn one.
        self._running = threading.Event()
        # Messages typed while a turn was in flight, oldest first. Envelopes,
        # not strings: an unspoken one goes back to the inbox as a normal turn.
        self._steer: list[Envelope] = []
        self._steer_lock = threading.Lock()
        # Commands answered on the reader thread, by action, since the loop sits inside
        # generate() for a whole turn: handlers must be read-only and quick.
        self.direct: dict[str, Callable[[Envelope], None]] = {}
        # No reader thread for a scripted run: `for line in sys.stdin` on a detached
        # process reads /dev/null, hits EOF, and the loop's own check would shut it
        # down before the message it was started for ever ran.
        if not self.detached:
            threading.Thread(target=self._read_loop, daemon=True).start()

    def _read_loop(self):
        for line in sys.stdin:
            line = line.strip()
            if not line: continue
            try:
                env = Envelope.from_dict(json.loads(line))
                
                # Set on the reader thread, not through the main loop, so it is readable mid-run.
                if env.type == "terminal": self.terminal_info = env.data

                # Flagged on the reader thread: the main loop is blocked inside generate()
                # for the whole run, and the envelope is still queued below for it.
                action = (env.data or {}).get("action") if env.type == "cmd" else None
                if action == "cancel": self._cancel.set()
                # Same reason: anything meant to reach a call already in flight
                # has to be flagged from the reader thread.
                elif action == "background": self.background.set()
                # A message typed mid-turn, not queued for the main loop, which would
                # answer it as a second turn; the loop drains it between calls.
                elif action == "chat" and self._running.is_set():
                    with self._steer_lock: self._steer.append(env)
                    # Shaped like chat_received: an acknowledgement, not a state, since a
                    # `state` field would be folded into the snapshot and move a frontend.
                    self.send("status", {"type": "chat_queued"}, id_=env.id)
                    continue

                # Answered here, not queued, since the point of these is to work mid-run.
                # A raising handler must not strand every later envelope, cancel included.
                if action and (fn := self.direct.get(action)):
                    try: fn(env)
                    except Exception as e: self.send("status", {"state": "error", "error": str(e)}, id_=env.id)
                    continue

                cb = self._pending.pop(env.id, None)
                if cb: cb(env)
                else: self.inbox.put(env)
            except json.JSONDecodeError: pass
        # stdin closed: the frontend that spawned us is gone. Signal the main
        # loop so it shuts down instead of running on as an orphan.
        self._eof.set()

    def eof(self) -> bool:
        """True once stdin has closed, meaning the frontend that spawned us is gone."""
        return self._eof.is_set()

    @property
    def running(self) -> bool:
        """Whether a turn is in flight. Read by the main loop, which must not hand an
        idle agent peer mail the round seam is already going to deliver."""
        return self._running.is_set()

    def begin_run(self):
        """Drop anything that arrived while idle so it cannot act on the next
        run. A background press with nothing running was aimed at nothing."""
        self._cancel.clear()
        self.background.clear()
        self._eof.clear()
        # Last, after the two clears: setting it earlier would hold back a chat
        # envelope belonging to the run that has not started, stranding it a turn.
        self._running.set()

    def end_run(self):
        """Close the turn and hand back whatever was typed during it.

        Anything the loop never got to goes on the inbox now, answered as an ordinary next turn: queued is a
        delay, not a place messages die. Safe to call twice."""
        self._running.clear()
        with self._steer_lock: left, self._steer = self._steer, []
        for env in left: self.inbox.put(env)

    def take_steer(self) -> list[dict]:
        """Everything typed since the last drain, oldest first, and clear.

        Handed to generate() as a callable so the loop asks at the one place a user message is legal for every provider,
        straight after a round's tool results. Messages, not strings: one with nothing but an attachment is still a message."""
        with self._steer_lock: got, self._steer = self._steer, []
        out = []
        for e in got:
            d = e.data or {}
            text = d.get("text") or ""
            images = [a for a in (d.get("images") or []) if isinstance(a, dict) and a.get("b64")]
            if text.strip() or images: out.append({"text": text, "images": images})
        return out

    def cancelled(self) -> bool:
        """True once a cancel has arrived for the run in flight, or the frontend is gone.

        EOF reads as a cancel on purpose: the reader thread sees a dead pipe at once, but the
        main loop is inside generate() and cannot look at it until the run ends. A laptop
        closing is exactly that, and without this the turn it was in the middle of keeps going
        with nobody to show it to.
        """
        return self._cancel.is_set() or self._eof.is_set()

    def send(self, type_: str, data: Optional[dict] = None, id_: Optional[str] = None) -> str:
        """Send an envelope. Returns the id (generated if not provided)."""
        id_ = id_ or str(uuid.uuid4())
        env = Envelope(type=type_, id=id_, data=data or {})
        with self._lock:
            try:
                sys.stdout.write(json.dumps(env.to_dict()) + "\n")
                sys.stdout.flush()
            except BrokenPipeError:
                # The frontend is gone and nobody is left to read this. Flagged like stdin
                # closing, so the run in flight stops and the main loop shuts the process
                # down instead of streaming a whole turn into a pipe with no reader.
                self._eof.set()
        return id_

    def reply(self, to: Envelope, type_: str, data: Optional[dict] = None) -> str:
        """Reply to an envelope, echoing its id back."""
        return self.send(type_, data, id_=to.id)

    def call(self, type_: str, data: Optional[dict] = None, timeout: Optional[float] = None,
             stop: Optional[Callable[[], bool]] = None) -> Optional[Envelope]:
        """Send and block until a response with the same id arrives.

        stop makes the wait abandonable: a permission prompt is asked with no timeout on purpose, but a cancel arriving while
        one is up must still end it. Giving up returns None, which callers already read as "no answer"."""
        event = threading.Event()
        result: list[Optional[Envelope]] = [None]

        def cb(env: Envelope):
            result[0] = env
            event.set()

        id_ = self.send(type_, data)
        self._pending[id_] = cb
        if stop is None:
            if event.wait(timeout=timeout): return result[0]
        else:
            deadline = None if timeout is None else time.monotonic() + timeout
            while not stop():
                if event.wait(timeout=0.05): return result[0]
                if deadline is not None and time.monotonic() >= deadline: break
        self._pending.pop(id_, None)
        return None

    def poll(self) -> list[Envelope]:
        cmds = []
        while not self.inbox.empty(): cmds.append(self.inbox.get())
        if self._script:
            cmds.extend(self._script)
            self._script.clear()
        elif self.detached and not cmds and not self._running.is_set():
            # A scripted run is over when the script is. The flag a closed stdin sets
            # is the same one, so the loop's own shutdown does the commit and the
            # empty-session drop, exactly as it does for a frontend that went away.
            self._eof.set()
        return cmds

    def poll_one(self, timeout: Optional[float] = None) -> Optional[Envelope]:
        try: return self.inbox.get(timeout=timeout)
        except queue.Empty: return None

# Example protocol: commands in from the other side, responses out.
if __name__ == "__main__":
    ipc = IPC()
    ipc.send("status", {"state": "startup"})
    while True:
        for env in ipc.poll():
            if env.type != "cmd":
                continue
            action = env.data.get("action")
            if action == "stop":
                sys.exit(0)
            if action == "say_hi":
                ipc.reply(env, "status", {"state": "running", "text": "hi!"})
            if action == "inject":
                ipc.reply(env, "status", {"state": "injected", "text": env.data.get("text")})
        ipc.send("action", {"action": "do_abc", "data": [1, 2, 3, 4]})
        time.sleep(1)
