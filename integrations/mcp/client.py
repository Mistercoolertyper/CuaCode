"""A Model Context Protocol client: JSON-RPC 2.0, both eras of the protocol.

Stdio is newline-delimited JSON both ways. Legacy servers (2025-11-25 and earlier) open with an `initialize` handshake and then a session, named on HTTP by a server `Mcp-Session-Id` header; modern servers are stateless, every request carrying its own protocol version, client identity and capabilities in `_meta`.
The era is picked once at startup, probing `server/discover` and falling back to `initialize` on anything not a recognised modern answer, with `"protocol"` in the config forcing the choice.
Connections are kept rather than made per call, since starting a server costs an interpreter launch: `pool()` returns a live one or starts it, and exit tears everything down.
"""
from __future__ import annotations

import atexit
import base64
import json
import os
import re
import signal
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from collections import deque
from contextvars import ContextVar
from pathlib import Path

from handler import faults
from handler.session import store

PROTOCOL = "2025-06-18"             # legacy era: the version `initialize` asks for
MODERN = "2026-07-28"               # modern era: the version the probe asks for
MODERN_VERSIONS = ("2026-07-28",)   # every stateless revision this client speaks
LEGACY_VERSIONS = ("2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05")
CLIENT_INFO = {"name": "cuacode", "version": "1.0.0"}

META = "io.modelcontextprotocol/"   # the reserved `_meta` key prefix
# HeaderMismatch, MissingRequiredClientCapability, UnsupportedProtocolVersion,
# which a legacy server cannot produce: one means the server is modern.
MODERN_ERRORS = {-32020, -32021, -32022}

START_TIMEOUT = 20          # interpreter launch + handshake
PROBE_TIMEOUT = 3           # server/discover, which a legacy server may never answer
CALL_TIMEOUT = 60           # a tool that drives an app can be slow; a hung one must not be forever
STDERR_KEEP = 40            # lines, kept only to explain a server that died
WEDGE_PCT = 90.0            # of one core, for a server with nothing asked of it
WEDGE_SEC = 60.0            # pinned that long is a server that will not answer again
WATCH_SEC = 15.0            # how often that is sampled


# ---- a server must not outlive the worker that started it -------------------
#
# `close_all` is an `atexit` hook, and `atexit` does not run for a worker that was
# killed rather than asked to leave. A lightpanda server orphaned that way on
# 2026-10-24 held a whole core for a week, spinning in its own idle loop with
# nobody asking it anything. Two things below keep that from repeating: a pidfile
# says whose server a process is, so the next worker can end an orphan without
# touching a session that is still running, and the watcher ends a server that is
# burning a core with nothing asked of it.

def _run_dir() -> Path:
    """Where the pid of each running server is kept: beside its config."""
    d = store.home() / "mcp" / "run"
    d.mkdir(parents=True, exist_ok=True)
    return d


def _pidfile(name: str) -> Path:
    return _run_dir() / (re.sub(r"[^\w.-]", "_", name) + ".pid")


def _alive(pid: int) -> bool:
    """Whether a process exists. A signalling check, not a proof: a recycled pid
    reads as alive, which is why a pidfile is never believed on the pid alone."""
    if pid == os.getpid():
        return True
    if os.name == "nt":
        try:
            import psutil
            return bool(psutil.pid_exists(pid))
        except Exception:                                       # noqa: BLE001
            return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def _command(pid: int) -> str:
    """A process's command line, or empty when there is nothing to read."""
    try:
        return subprocess.run(["ps", "-o", "command=", "-p", str(pid)],
                              capture_output=True, text=True, timeout=5).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return ""


def _cputime(pid: int) -> float | None:
    """CPU seconds a process has used, off `ps`'s own clock. None when it is gone."""
    try:
        out = subprocess.run(["ps", "-o", "time=", "-p", str(pid)],
                             capture_output=True, text=True, timeout=5).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return None
    if not out:
        return None
    days, sep, clock = out.partition("-")
    if not sep:
        days, clock = "", out            # no `-`: mm:ss.cc, and no days at all
    total = (int(days) if days else 0) * 86400.0
    for part in clock.split(":"):
        total = total * 60 + float(part)
    return total


def _end(pid: int):
    """Ask, then insist: a wedged server is not going to read a polite message."""
    for sig in (signal.SIGTERM, getattr(signal, "SIGKILL", signal.SIGTERM)):
        try:
            os.kill(pid, sig)
        except OSError:
            return
        for _ in range(25):
            if not _alive(pid):
                return
            time.sleep(0.04)


def sweep() -> list[str]:
    """End every server whose worker is gone, and name the ones it ended.

The recorded command line has to match before anything is killed: a pid is not
proof of identity, and ending whatever inherited a recycled one would be worse
than leaving a single orphan."""
    ended = []
    for f in sorted(_run_dir().glob("*.pid")):
        try:
            rec = json.loads(f.read_text() or "{}")
        except (OSError, ValueError):
            rec = {}
        pid, owner = rec.get("pid"), rec.get("owner")
        if not (isinstance(owner, int) and _alive(owner)):
            argv0 = str(rec.get("argv0") or "")
            mark = (argv0, Path(argv0).name if argv0 else "")
            if (isinstance(pid, int) and _alive(pid)
                    and (not argv0 or any(m and m in _command(pid) for m in mark))):
                _end(pid)
                ended.append(f"{rec.get('name') or f.stem} (pid {pid})")
        f.unlink(missing_ok=True)
    if ended:
        print(f"[mcp] ended {len(ended)} server(s) left behind by a worker that is gone: "
              f"{', '.join(ended)}", file=sys.stderr)
    return ended


class MCPError(RuntimeError):
    """Anything that stops a call: server missing, dead, slow, or protocol-level.

    `code` and `data` are the JSON-RPC error's own when the server sent one. `status` is the HTTP status: its presence is
    how the probe tells "the server answered and said no" from "nothing answered yet"."""

    def __init__(self, message: str, code: int | None = None,
                 data: dict | None = None, status: int | None = None):
        super().__init__(message)
        self.code = code
        self.data = data if isinstance(data, dict) else {}
        self.status = status


class _Legacy(Exception):
    """Internal: the probe decided this server predates the stateless era."""


# HTTP request metadata (modern era): a router need not parse the body, and a
# mismatch is rejected, so everything below keeps the mirror exact.

_TCHAR = re.compile(r"^[-!#$%&'*+.^_`|~0-9A-Za-z]+$")   # RFC 9110 field-name token
_SENTINEL = re.compile(r"^=\?base64\?.*\?=$", re.S)
_PRIMITIVE = {"string", "integer", "boolean"}           # `number` is not permitted
_SCHEMA_MAPS = ("properties", "patternProperties", "$defs",
                "definitions", "dependentSchemas")
_INSTANCE_KEYS = ("enum", "const", "default", "examples")


def _header_value(value) -> str:
    """A parameter value as a header value, Base64-wrapped when it cannot be
    carried as plain visible ASCII."""
    if isinstance(value, bool):
        text = "true" if value else "false"
    else:
        text = str(value)
    safe = (text == text.strip()
            and all("\x20" <= ch <= "\x7e" for ch in text)
            and not _SENTINEL.match(text))
    if safe:
        return text
    return "=?base64?" + base64.b64encode(text.encode("utf-8")).decode("ascii") + "?="


def _mcp_name(method: str, params: dict) -> str | None:
    """The body field the `Mcp-Name` header mirrors, for the methods that have one."""
    if method in ("tools/call", "prompts/get"):
        value = params.get("name")
    elif method == "resources/read":
        value = params.get("uri")
    else:
        return None
    return None if value is None else str(value)


def _annotated(schema: dict, path: tuple, out: list):
    """Every `x-mcp-header` reachable from the schema root through `properties`,
    the only place the spec allows one."""
    props = schema.get("properties")
    if not isinstance(props, dict):
        return
    for key, sub in props.items():
        if not isinstance(sub, dict):
            continue
        if "x-mcp-header" in sub:
            out.append(((*path, key), sub))
        _annotated(sub, (*path, key), out)


def _anywhere(node, out: list, is_schema: bool = True):
    """Every `x-mcp-header` in the schema at all, reachable or not: an unreachable
    one invalidates the tool, so the two walks are compared, not the first trusted."""
    if isinstance(node, dict):
        if is_schema and "x-mcp-header" in node:
            out.append(id(node))
        for key, value in node.items():
            if key in _INSTANCE_KEYS:
                continue                            # instance values, not schemas
            if key in _SCHEMA_MAPS:
                if isinstance(value, dict):
                    for sub in value.values():
                        _anywhere(sub, out, True)
                continue
            _anywhere(value, out, is_schema)
    elif isinstance(node, list):
        for value in node:
            _anywhere(value, out, is_schema)


def _header_params(schema) -> list[tuple[tuple, str]] | None:
    """`(property path, header name)` for one tool, or None if the tool breaks
    the rules and must be dropped from the list rather than called wrongly."""
    if not isinstance(schema, dict):
        return []
    found: list = []
    _annotated(schema, (), found)
    everywhere: list = []
    _anywhere(schema, everywhere)
    if len(everywhere) != len(found):
        return None                                 # annotated somewhere unreachable
    out, seen = [], set()
    for path, sub in found:
        name = sub.get("x-mcp-header")
        if not isinstance(name, str) or not name or not _TCHAR.match(name):
            return None
        if name.lower() in seen:
            return None                             # names are case-insensitively unique
        if sub.get("type") not in _PRIMITIVE:
            return None
        seen.add(name.lower())
        out.append((path, name))
    return out


def _at_path(arguments: dict, path: tuple):
    node = arguments
    for key in path:
        if not isinstance(node, dict) or key not in node:
            return None
        node = node[key]
    return node


class Connection:
    """One running MCP server, and the conversation with it."""

    def __init__(self, name: str, config: dict):
        self.name = name
        self.config = config
        self.proc: subprocess.Popen | None = None
        self.server_info: dict = {}
        self.capabilities: dict = {}
        self.instructions: str = ""
        self.protocol: str = ""
        self.era: str = ""                    # "modern" or "legacy", decided at startup
        self._tools: list | None = None
        self._tools_at = 0.0
        self._tools_ttl: float | None = None  # seconds, from the server's own ttlMs
        self._headers: dict[str, list] = {}   # tool name -> its x-mcp-header mirrors
        self._next_id = 0
        self._pending: dict[int, dict] = {}
        self._events: dict[int, threading.Event] = {}
        self._lock = threading.Lock()
        self._busy = 0                        # requests in flight: its own reason to work
        self._stderr = deque(maxlen=STDERR_KEEP)
        self.transport = config.get("transport", "stdio")
        self.url = config.get("url")
        self._session_id: str | None = None   # server-assigned on initialize (legacy HTTP)

    # -- lifecycle ---------------------------------------------------------

    def alive(self) -> bool:
        return self.proc is not None and self.proc.poll() is None

    def _argv(self) -> list[str]:
        command = self.config.get("command")
        if not command:
            raise MCPError(f"{self.name}: no command in its config")
        # `python3` in a config is whatever the shell would find, a coin toss with
        # several installs; the running interpreter is one we know exists.
        if command in ("python", "python3"):
            command = sys.executable
        args = [os.path.expanduser(str(a)) for a in (self.config.get("args") or [])]
        return [command, *args]

    def start(self):
        if self.alive():
            return
        env = {**os.environ, **{str(k): str(v) for k, v in (self.config.get("env") or {}).items()}}
        cwd = self.config.get("cwd")
        argv = self._argv()
        try:
            self.proc = subprocess.Popen(
                argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                text=True, bufsize=1, env=env,
                cwd=os.path.expanduser(cwd) if cwd else None)
        except FileNotFoundError:
            raise MCPError(f"{self.name}: cannot run {argv[0]!r}")
        except OSError as exc:
            raise MCPError(f"{self.name}: {exc}")

        threading.Thread(target=self._read_stderr, daemon=True).start()
        if self.transport != "http":
            threading.Thread(target=self._read_stdout, daemon=True).start()
        try:
            self._handshake()
        except MCPError:
            self.close()
            raise
        self._stamp()
        threading.Thread(target=self._watch, args=(self.proc,), daemon=True).start()

    def _stamp(self):
        """Write down whose server this is, so a later worker can tell an orphan
        from a session that is still using it."""
        rec = {"name": self.name, "pid": self.proc.pid, "owner": os.getpid(),
               "argv0": self._argv()[0]}
        try:
            _pidfile(self.name).write_text(json.dumps(rec))
        except OSError:
            pass

    def _unstamp(self, pid: int):
        path = _pidfile(self.name)
        try:
            if json.loads(path.read_text() or "{}").get("pid") == pid:
                path.unlink()
        except (OSError, ValueError):
            pass

    def _watch(self, proc):
        """End a server that is burning a core with nothing asked of it.

        Not a health check: a call in flight is its own reason to be busy, and a page
        being fetched takes all of one core honestly. This is for the other case, a
        server at 100% with an empty inbox, which is what lightpanda was doing in its
        own idle loop for a week. Ending it costs one restart on the next call, which
        `pool()` does without being asked."""
        prev = _cputime(proc.pid)
        pinned = 0.0
        while prev is not None and proc.poll() is None:
            time.sleep(WATCH_SEC)
            if self._busy:
                prev, pinned = _cputime(proc.pid), 0.0
                continue
            now = _cputime(proc.pid)
            if now is None:
                return
            used, prev = now - prev, now
            pinned = pinned + WATCH_SEC if used >= WEDGE_PCT / 100 * WATCH_SEC else 0.0
            if pinned >= WEDGE_SEC:
                self._stderr.append(f"[killed: {WEDGE_PCT:.0f}% CPU for {WEDGE_SEC:.0f}s idle]")
                print(f"[mcp] {self.name}: pinned at {WEDGE_PCT:.0f}% of a core for "
                      f"{WEDGE_SEC:.0f}s with nothing asked of it; ended it", file=sys.stderr)
                try:
                    proc.kill()
                except OSError:
                    pass
                return

    def _handshake(self):
        """Which era this server speaks, decided once and kept for its lifetime.

        `protocol` in the config forces it, by era or by revision: `legacy` skips a probe the server will
        never answer, and `modern` refuses the fallback rather than quietly speaking an old protocol to it."""
        want = str(self.config.get("protocol") or "auto").lower()
        version = MODERN
        if want in MODERN_VERSIONS:
            version, want = want, "modern"
        elif want in LEGACY_VERSIONS:
            want = "legacy"
        if want == "legacy":
            self._legacy_handshake()
            return
        try:
            self._discover(version, set())
        except _Legacy as fallback:
            reason = fallback.args[0] if fallback.args else "no answer"
            if want == "modern":
                raise MCPError(f"{self.name}: configured for the stateless protocol, but "
                               f"server/discover did not answer as a {version} server would "
                               f"({reason})")
            try:
                self._legacy_handshake()
            except Exception as old:
                # Both openings were tried, so both reasons go in the sentence. The
                # legacy error on its own reads as the only thing that was attempted,
                # and when a server is misconfigured it is usually the probe -- the
                # one that never got an answer -- that says why.
                raise MCPError(f"{self.name}: neither protocol answered - stateless "
                               f"{version}: {reason}; legacy {PROTOCOL}: {old}") from old

    def _discover(self, version: str, tried: set):
        """The modern opening: ask what the server speaks, and speak it. Anything
        but a DiscoverResult or a recognised modern error makes it `_Legacy`."""
        tried.add(version)
        deadline = time.time() + START_TIMEOUT
        while True:
            if not self.alive():
                raise MCPError(f"{self.name}: server exited during startup{self._why()}")
            try:
                result = self.request("server/discover", {},
                                      timeout=PROBE_TIMEOUT, version=version)
                break
            except MCPError as exc:
                if exc.code in MODERN_ERRORS:
                    return self._unsupported(exc, version, tried)
                # No answer at all from an HTTP endpoint is a server still coming
                # up, not a verdict; anything the server said says legacy.
                if (self.transport == "http" and exc.status is None
                        and exc.code is None and time.time() < deadline):
                    time.sleep(0.2)
                    continue
                raise _Legacy(str(exc))

        versions = [v for v in (result.get("supportedVersions") or []) if isinstance(v, str)]
        chosen = next((v for v in MODERN_VERSIONS if v in versions), None)
        if chosen is None:
            if not versions:
                chosen = version            # answered the probe but named nothing: take its word
            elif any(v in LEGACY_VERSIONS for v in versions):
                raise _Legacy(f"speaks only {', '.join(versions)}")
            else:
                raise MCPError(f"{self.name}: server speaks {', '.join(versions)}; "
                               f"CuaCode speaks {MODERN} and {PROTOCOL}")
        self.era = "modern"
        self.protocol = chosen
        self.capabilities = result.get("capabilities") or {}
        self.instructions = result.get("instructions") or ""
        self.server_info = (result.get("_meta") or {}).get(f"{META}serverInfo") or {}

    def _unsupported(self, exc: MCPError, version: str, tried: set):
        """A modern error came back. The server is modern; the version was wrong."""
        if exc.code != -32022:
            raise exc                       # our request was malformed, not our version
        supported = [v for v in (exc.data.get("supported") or []) if isinstance(v, str)]
        chosen = next((v for v in MODERN_VERSIONS if v in supported and v not in tried), None)
        if chosen:
            return self._discover(chosen, tried)
        if any(v in LEGACY_VERSIONS for v in supported):
            raise _Legacy(f"rejected {version}, offers {', '.join(supported)}")
        raise MCPError(f"{self.name}: server rejected {version} and offers "
                       f"{', '.join(supported) or 'nothing this client speaks'}")

    def _legacy_handshake(self):
        """The legacy opening: one `initialize`, one `notifications/initialized`,
        then a session for the rest of the connection's life."""
        if self.transport == "http":
            deadline = time.time() + START_TIMEOUT
            while True:
                if not self.alive():
                    raise MCPError(f"{self.name}: server exited during startup{self._why()}")
                try:
                    hello = self._http_request("initialize", {
                        "protocolVersion": PROTOCOL,
                        "capabilities": {},
                        "clientInfo": CLIENT_INFO,
                    }, timeout=5, version=PROTOCOL)
                    break
                except MCPError as exc:
                    # Retry only while nothing has answered; a server that
                    # replied has given its answer.
                    if exc.status is not None or exc.code is not None or time.time() > deadline:
                        raise
                    time.sleep(0.2)
        else:
            hello = self.request("initialize", {
                "protocolVersion": PROTOCOL,
                "capabilities": {},
                "clientInfo": CLIENT_INFO,
            }, timeout=START_TIMEOUT, version=PROTOCOL)

        self.era = "legacy"
        self.server_info = hello.get("serverInfo") or {}
        self.capabilities = hello.get("capabilities") or {}
        self.protocol = hello.get("protocolVersion") or ""
        self.instructions = hello.get("instructions") or ""
        if self.transport == "http":
            self._http_request("notifications/initialized", {})
        else:
            self._notify("notifications/initialized", {})

    def close(self):
        proc, self.proc = self.proc, None
        self._tools = None
        self._tools_ttl = None
        self._headers = {}
        self._session_id = None
        if not proc:
            return
        self._unstamp(proc.pid)
        try:
            if proc.stdin:
                proc.stdin.close()
            proc.wait(timeout=3)
        except Exception as e:                                  # noqa: BLE001
            proc.kill()
            try:
                proc.wait(timeout=3)            # a killed child still needs reaping
            except Exception as stuck:          # noqa: BLE001
                faults.note(f"reaping MCP server {self.name} (pid {proc.pid})", stuck,
                            f"it would not exit on its own either: {e}")

    # -- wire --------------------------------------------------------------

    def _read_stdout(self):
        """One reader thread, so a notification arriving between a request and
        its response cannot be mistaken for that response."""
        proc = self.proc
        try:
            for line in proc.stdout:
                line = line.strip()
                if not line:
                    continue
                try:
                    message = json.loads(line)
                except json.JSONDecodeError:
                    self._stderr.append(f"[non-JSON on stdout] {line[:200]}")
                    continue
                mid = message.get("id")
                if message.get("method") and mid is not None:
                    # The server asking us for something (sampling, roots), which
                    # modern servers do inside a result. Refuse: no capabilities.
                    self._send({"jsonrpc": "2.0", "id": mid,
                                "error": {"code": -32601, "message": "client has no such capability"}})
                    continue
                if mid is None:
                    continue                                    # server notification: nothing to route
                with self._lock:
                    self._pending[mid] = message
                    event = self._events.get(mid)
                if event:
                    event.set()
        except Exception as e:                                  # noqa: BLE001
            # This thread is the only reader. A call made after it dies waits on an
            # answer nothing is reading, and says "the server did not answer" -- so
            # the reason it stopped reading belongs here, once.
            faults.note(f"reading MCP server {self.name}", e,
                        "every later call to it is unanswered")
        finally:
            # Whatever is still waiting will never be answered now.
            with self._lock:
                events = list(self._events.values())
            for event in events:
                event.set()

    def _read_stderr(self):
        proc = self.proc
        try:
            for line in proc.stderr:
                self._stderr.append(line.rstrip())
        except Exception as e:                                  # noqa: BLE001
            faults.note(f"collecting MCP server {self.name}'s stderr", e,
                        "why it exits will be missing from the error")

    def _send(self, message: dict):
        proc = self.proc
        if not proc or proc.poll() is not None:
            raise MCPError(f"{self.name}: server is not running{self._why()}")
        try:
            proc.stdin.write(json.dumps(message) + "\n")
            proc.stdin.flush()
        except (BrokenPipeError, ValueError):
            raise MCPError(f"{self.name}: server closed its input{self._why()}")

    def _notify(self, method: str, params: dict):
        self._send({"jsonrpc": "2.0", "method": method, "params": params})

    def _why(self) -> str:
        """The server's own last words, when it left any."""
        tail = [l for l in self._stderr if l.strip()]
        return f" -- stderr: {' | '.join(tail[-3:])}" if tail else ""

    def _new_id(self) -> int:
        with self._lock:
            self._next_id += 1
            return self._next_id

    def _version(self, version: str | None) -> tuple[str, bool]:
        """The version a request declares, and whether that is a stateless one."""
        chosen = version or self.protocol or PROTOCOL
        return chosen, chosen in MODERN_VERSIONS

    def _with_meta(self, params: dict, version: str) -> dict:
        """The per-request protocol fields a stateless server has instead of a
        handshake. Required on every request: one without them is malformed."""
        meta = dict(params.get("_meta") or {})
        meta.setdefault(f"{META}protocolVersion", version)
        meta.setdefault(f"{META}clientInfo", CLIENT_INFO)
        meta.setdefault(f"{META}clientCapabilities", {})
        return {**params, "_meta": meta}

    def _result(self, message: dict, method: str) -> dict:
        if "error" in message:
            err = message.get("error") or {}
            raise MCPError(f"{self.name}: {err.get('message', 'error')} (code {err.get('code')})",
                           code=err.get("code"), data=err.get("data"))
        result = message.get("result") or {}
        # Absent means "complete": that is what an older server's results mean,
        # and the spec says to read them that way.
        kind = result.get("resultType", "complete")
        if kind != "complete":
            raise MCPError(f"{self.name}: {method} came back as {kind!r} -- the server wants "
                           "more input mid-call, which this client does not carry out")
        return result

    def _read_sse(self, resp, want_id: int, method: str) -> dict:
        """An SSE response stream, read down to the response for this request.

        A modern server may answer any request this way, sending progress and log notifications first;
        everything that is not the answer is dropped."""
        answer = None
        data: list[str] = []

        def flush():
            if not data:
                return None
            body = "\n".join(data)
            data.clear()
            try:
                return json.loads(body)
            except json.JSONDecodeError:
                self._stderr.append(f"[non-JSON in SSE] {body[:200]}")
                return None

        for raw in resp:
            line = raw.decode("utf-8", "replace").rstrip("\r\n")
            if not line:
                message = flush()
                if message is not None and message.get("id") == want_id:
                    answer = message
                    break
                continue
            if line.startswith(":"):                            # comment: a keep-alive
                continue
            field, _, value = line.partition(":")
            if value.startswith(" "):
                value = value[1:]
            if field == "data":
                data.append(value)
        if answer is None:
            message = flush()
            if message is not None and message.get("id") == want_id:
                answer = message
        if answer is None:
            raise MCPError(f"{self.name}: the response stream for {method} closed "
                           f"before it answered{self._why()}")
        return answer

    def _http_request(self, method: str, params: dict, timeout: float = CALL_TIMEOUT,
                      session: str | None = None, version: str | None = None,
                      headers: dict | None = None) -> dict:
        version, modern = self._version(version)
        mid = self._new_id()
        if modern:
            params = self._with_meta(params, version)
        body = json.dumps({"jsonrpc": "2.0", "id": mid,
                           "method": method, "params": params}).encode()

        head = {"Content-Type": "application/json",
                "Accept": "application/json, text/event-stream"}
        if modern:
            # These mirror the body, and the server rejects the request with
            # HeaderMismatch if they do not.
            head["MCP-Protocol-Version"] = version
            head["Mcp-Method"] = method
            name = _mcp_name(method, params)
            if name is not None:
                head["Mcp-Name"] = _header_value(name)
            head.update(headers or {})
        else:
            if self.protocol:       # required from 2025-06-18 on, once negotiated
                head["MCP-Protocol-Version"] = self.protocol
            sid = session or self._session_id
            if sid:
                head["Mcp-Session-Id"] = sid

        req = urllib.request.Request(self.url, data=body, headers=head)
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                if not modern and not self._session_id:
                    self._session_id = resp.headers.get("Mcp-Session-Id")
                if resp.status == 202:
                    return {}                   # a notification: nothing to answer
                ctype = (resp.headers.get("Content-Type") or "").split(";")[0].strip().lower()
                if ctype == "text/event-stream":
                    message = self._read_sse(resp, mid, method)
                else:
                    raw = resp.read()
                    if not raw or not raw.strip():
                        return {}
                    message = json.loads(raw)
        except urllib.error.HTTPError as exc:
            raise self._http_error(exc)
        except OSError as exc:
            raise MCPError(f"{self.name}: {exc}")
        except json.JSONDecodeError as exc:
            raise MCPError(f"{self.name}: unreadable response to {method}: {exc}")
        return self._result(message, method)

    def _http_error(self, exc: urllib.error.HTTPError) -> MCPError:
        """An HTTP failure, with the JSON-RPC error inside it when there is one.

        A modern server answers an unsupported version, a missing capability or a header mismatch with 400
        and a JSON-RPC error; telling that from a legacy 400 is the probe."""
        try:
            raw = exc.read()
        except Exception:                                       # noqa: BLE001
            raw = b""
        try:
            message = json.loads(raw)
        except (json.JSONDecodeError, ValueError):
            message = None
        if isinstance(message, dict) and isinstance(message.get("error"), dict):
            err = message["error"]
            return MCPError(f"{self.name}: {err.get('message', 'error')} (code {err.get('code')})",
                            code=err.get("code"), data=err.get("data"), status=exc.code)
        return MCPError(f"{self.name}: HTTP {exc.code}: {raw[:200]}", status=exc.code)

    def _stdio_request(self, method: str, params: dict, timeout: float,
                       version: str | None) -> dict:
        version, modern = self._version(version)
        if modern:
            params = self._with_meta(params, version)
        with self._lock:
            self._next_id += 1
            mid = self._next_id
            event = threading.Event()
            self._events[mid] = event
        try:
            self._send({"jsonrpc": "2.0", "id": mid, "method": method, "params": params})
            if not event.wait(timeout):
                raise MCPError(f"{self.name}: {method} timed out after {timeout}s")
            with self._lock:
                message = self._pending.pop(mid, None)
            if message is None:
                raise MCPError(f"{self.name}: server died during {method}{self._why()}")
            return self._result(message, method)
        finally:
            with self._lock:
                self._events.pop(mid, None)
                self._pending.pop(mid, None)

    def request(self, method: str, params: dict, timeout: float = CALL_TIMEOUT,
                session: str | None = None, version: str | None = None,
                headers: dict | None = None) -> dict:
        # Counted, so the watcher can tell a server working for us from one working
        # for nobody.
        self._busy += 1
        try:
            if self.transport == "http":
                return self._http_request(method, params, timeout=timeout, session=session,
                                          version=version, headers=headers)
            return self._stdio_request(method, params, timeout=timeout, version=version)
        finally:
            self._busy -= 1

    # -- MCP -------------------------------------------------------------

    def _fresh(self) -> bool:
        if self._tools is None:
            return False
        if self._tools_ttl is None:
            return True
        return (time.time() - self._tools_at) < self._tools_ttl

    def list_tools(self, refresh: bool = False) -> list:
        if not refresh and self._fresh():
            return self._tools
        self.start()
        tools, cursor, ttl = [], None, None
        while True:
            page = self.request("tools/list", {"cursor": cursor} if cursor else {})
            tools.extend(page.get("tools") or [])
            ttl = page.get("ttlMs")
            cursor = page.get("nextCursor")
            if not cursor:
                break
        self._tools = self._mirrors(tools)
        self._tools_at = time.time()
        # ttlMs is the server's own freshness hint. Without one the list is kept
        # for the life of the connection.
        self._tools_ttl = ttl / 1000.0 if isinstance(ttl, (int, float)) and ttl > 0 else None
        return self._tools

    def _mirrors(self, tools: list) -> list:
        """Each tool's `x-mcp-header` mirrors, dropping any whose annotations break
        the rules: one bad schema must not take the others down or be called wrongly."""
        self._headers = {}
        if self.era != "modern" or self.transport != "http":
            return tools
        kept = []
        for tool in tools:
            name = tool.get("name")
            params = _header_params(tool.get("inputSchema"))
            if params is None:
                print(f"[mcp] {self.name}: dropping tool {name!r} -- invalid x-mcp-header "
                      "annotation in its input schema", file=sys.stderr)
                continue
            if params:
                self._headers[name] = params
            kept.append(tool)
        return kept

    def _param_headers(self, tool: str, arguments: dict) -> dict:
        out = {}
        for path, header in self._headers.get(tool, ()):
            value = _at_path(arguments, path)
            if value is None:
                continue                    # absent or null: the header is omitted
            out[f"Mcp-Param-{header}"] = _header_value(value)
        return out

    def call(self, tool: str, arguments: dict | None = None, timeout: float = CALL_TIMEOUT,
             session: str | None = None) -> dict:
        self.start()
        arguments = arguments or {}
        headers = None
        if self.era == "modern" and self.transport == "http":
            if self._tools is None:
                self.list_tools()           # the mirrors live in the tool schemas
            headers = self._param_headers(tool, arguments)
        result = self.request("tools/call", {"name": tool, "arguments": arguments},
                              timeout=timeout, session=session, headers=headers)
        return _flatten(result)

    def has_tool(self, name: str) -> bool:
        try:
            return any(t.get("name") == name for t in self.list_tools())
        except MCPError:
            return False

    def tool_schema(self, name: str) -> dict:
        for tool in (self._tools or ()):
            if tool.get("name") == name:
                return tool.get("inputSchema") or {}
        return {}


def _flatten(result: dict) -> dict:
    """MCP's content blocks, turned into something a tool result can hold.

    The text block holds JSON, and parsing it here gives the model an object instead of a string holding
    one."""
    blocks = result.get("content") or []
    texts = [b.get("text", "") for b in blocks if isinstance(b, dict) and b.get("type") == "text"]
    other = [b.get("type") for b in blocks if isinstance(b, dict) and b.get("type") != "text"]
    joined = "\n".join(t for t in texts if t)

    out = {"is_error": bool(result.get("isError"))}
    # One of data or text, never both: they are the same bytes twice, and a
    # tool result that says everything twice is paid for on every later turn.
    if result.get("structuredContent"):
        out["data"] = result["structuredContent"]
    elif joined:
        try:
            out["data"] = json.loads(joined)
        except (json.JSONDecodeError, ValueError):
            out["text"] = joined
    if other:
        out["other_content"] = other
    return out


# ----------------------------------- Pool ---------------------------------

_pool: dict[str, Connection] = {}
_pool_lock = threading.Lock()
_swept = False


def pool(name: str, config: dict) -> Connection:
    """The live connection for this server, started if it is not up yet. One whose
    config changed is replaced rather than reused, being a process running the
    old command."""
    global _swept
    if not _swept:
        _swept = True
        try:
            sweep()                     # once per worker: anything a dead one left
        except Exception as e:                                  # noqa: BLE001
            faults.note("sweeping orphaned MCP servers", e,
                        "one left by a dead worker stays running")
    with _pool_lock:
        conn = _pool.get(name)
        if conn and (not conn.alive() or conn.config != config):
            conn.close()
            conn = None
        if conn is None:
            conn = Connection(name, config)
            _pool[name] = conn
    conn.start()
    return conn


def connected() -> dict[str, Connection]:
    with _pool_lock:
        return {n: c for n, c in _pool.items() if c.alive()}


def close(name: str) -> bool:
    with _pool_lock:
        conn = _pool.pop(name, None)
    if not conn:
        return False
    conn.close()
    return True


def close_all():
    with _pool_lock:
        conns = list(_pool.values())
        _pool.clear()
    for conn in conns:
        try:
            conn.close()
        except Exception as e:                                  # noqa: BLE001
            faults.note(f"closing MCP server {conn.name}", e,
                        "its process may outlive the worker")


# Session scope. A ContextVar, not a connection field: parallel subagents each get a browser session of their own,
# the top-level conversation leaves None for the default, and the modern era mints a handle as an ordinary tool argument.

_mcp_sessions: ContextVar[dict | None] = ContextVar("mcp_sessions", default=None)


def session_scope() -> dict | None:
    return _mcp_sessions.get()


def set_session_scope(scope: dict):
    return _mcp_sessions.set(scope)


def reset_session_scope(token):
    _mcp_sessions.reset(token)


atexit.register(close_all)
