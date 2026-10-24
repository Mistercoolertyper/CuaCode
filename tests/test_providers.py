#!/usr/bin/env python3
"""Where a provider's models come from, including the URL an entry may name instead.

An entry reads its own catalog by default (/v1/models, /api/tags). When that endpoint is
missing or answers in a shape nobody can read, the entry names the URL that does, and
these are the shapes models_at has to survive.

    python3 tests/test_providers.py
"""

import json, os, sys, threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from handler.agent.providers.base import models_at
from handler.agent.providers.openai import OpenAI

FAILS = []


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond and detail else ""))
    if not cond: FAILS.append(name)


PAYLOADS = {}


class Served(BaseHTTPRequestHandler):
    def do_GET(self):
        self._send(PAYLOADS.get(self.path))

    def do_POST(self):
        # The show endpoint answers a body: {"model": ...}, and answers about it. A chat
        # endpoint at /v1 answers 401 to anything, which is what a missing key looks like.
        if self.path.endswith("/chat/completions"):
            self._send({"error": {"message": "Unauthorized"}}, 401)
            return
        self._send(PAYLOADS.get(self.path))

    def _send(self, body, status=200):
        if body is None:
            self.send_response(404); self.end_headers(); return
        raw = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def log_message(self, *a): pass


srv = HTTPServer(("127.0.0.1", 0), Served)
threading.Thread(target=srv.serve_forever, daemon=True).start()
base = f"http://127.0.0.1:{srv.server_port}"

PAYLOADS["/data"] = {"data": [{"id": "b"}, {"id": "a"}]}
PAYLOADS["/models"] = {"models": [{"name": "m1", "model": "m1"}, {"name": "m2"}]}
PAYLOADS["/bare"] = ["z", "y"]
PAYLOADS["/junk"] = {"error": "what is this"}
PAYLOADS["/empty"] = {"data": []}
PAYLOADS["/rows"] = {"data": [{"model": "x"}, {"nothing": 1}, "plain", 7]}
PAYLOADS["/show"] = {"capabilities": ["completion", "thinking", "tools", "vision"],
                     "model_info": {"deepseek_v41.context_length": 1048576},
                     "thinking": {"values": [False, "low", "high", "max"], "default": "high"}}
PAYLOADS["/shownum"] = {"parameters": "num_ctx 8192\ntemperature 0.8",
                        "model_info": {"x.context_length": 1048576}}
PAYLOADS["/shownothing"] = {"capabilities": "not a list"}

check("a `data` list of ids", models_at(base + "/data") == ["a", "b"])
check("a `models` list of names", models_at(base + "/models") == ["m1", "m2"])
check("a bare list", models_at(base + "/bare") == ["y", "z"])
check("a row that names nothing is skipped", models_at(base + "/rows") == ["plain", "x"])
check("something that is not a catalog is no answer", models_at(base + "/junk") is None)
check("an empty catalog is no answer", models_at(base + "/empty") is None)
check("a URL that 404s is no answer", models_at(base + "/nope") is None)
check("a host that is not listening is no answer", models_at("http://127.0.0.1:9/x") is None)

# The entry names it, and nothing else changes when it does not.
check("an openai entry reads the named URL", OpenAI("m", "https://host/v1", "HOST_KEY",
                                                    models_url=base + "/data").models() == ["a", "b"])
check("an openai entry with none reads its own", OpenAI("m", "https://host/v1", "HOST_KEY").models_url is None)

# The entry can hand over a shortlist instead of one name: the highest score is the default,
# the list leads with the shortlist and the host's own catalog follows it.
scored = OpenAI({"small": 4, "big": 9, "tie": 9}, "https://host/v1", "HOST_KEY", models_url=base + "/data")
check("the top score is the default", scored.default_model == "big", scored.default_model)
check("the shortlist leads, best first", scored.models()[:3] == ["big", "tie", "small"], scored.models())
check("and the host's catalog follows it", scored.models()[3:] == ["a", "b"], scored.models())
check("a host that will not say still ranks",
      OpenAI({"x": 1}, "https://host/v1", "K", models_url=base + "/nope").models() == ["x"])
check("one name is still one name", OpenAI("solo", "https://host/v1", "K").model_scores == {})

# A key pasted out of a provider's docs carries the scheme the SDK adds itself.
stripped = OpenAI("m", "https://host/v1", "SOME_KEY")
stripped.setup("Bearer abc123")
check("a `Bearer ` prefix is stripped once", stripped._key == "abc123", stripped._key)

# The stored shape is not the record. A record holds names and args, and the wire shape is
# rebuilt for whichever provider is being talked to now.
from handler.session import replay

def session(extra):
    rec = {"t": "assistant", "ts": "2026-10-08T20:00:01+00:00", "p": "ollama", "thinking": "",
           "content": "", "calls": [{"name": "shell", "args": {"command": "date"}}], **extra}
    return [{"t": "user", "ts": "2026-10-08T20:00:00+00:00", "text": "hi"}, rec,
            {"t": "tool", "ts": "2026-10-08T20:00:02+00:00", "name": "shell",
             "result": {"result": {"stdout": "x"}}}]

# Written by the old ollama, which stored the request shape it sent over /api/chat.
old = replay.to_messages(session({"native": [{"function": {"name": "shell",
                                                            "arguments": {"command": "date"}}}]}),
                         "ollama")
call = old[1]["tool_calls"][0]
check("a stored request shape is ignored", call.get("type") == "function" and "id" in call, call)
check("and its result answers the rebuilt id", old[2].get("tool_call_id") == call["id"], old[2])

# And one written now, which stores no shape at all, replays to the same messages.
new = replay.to_messages(session({}), "ollama")
check("a record with no stored shape replays the same", new[1]["tool_calls"] == old[1]["tool_calls"],
      new[1]["tool_calls"])

# show_url: what one model is, for a dialect whose own catalog says nothing about it.
shown = OpenAI("m", "https://host/v1", "K", show_url=base + "/show")
check("show_url answers what the model can do", shown.capabilities("m") == ["completion", "thinking",
                                                                            "tools", "vision"])
check("and the window its catalog never states", shown.context_window("m") == 1048576)
check("a num_ctx wins over the trained-for length",
      OpenAI("m", "https://host/v1", "K", show_url=base + "/shownum").context_window("m") == 8192)
check("a host that will not say answers nothing",
      OpenAI("m", "https://host/v1", "K", show_url=base + "/nope").capabilities("m") is None)
check("and a window of 0",
      OpenAI("m", "https://host/v1", "K", show_url=base + "/nope").context_window("m") == 0)
check("an answer in the wrong shape is no answer either",
      OpenAI("m", "https://host/v1", "K", show_url=base + "/shownothing").capabilities("m") is None)
check("without one, a plain host is still not asked", OpenAI("m", "https://host/v1", "K").capabilities("m") is None)

# A key that only ollama's own file has, which is how a signed-in box is configured nowhere at
# all. The reading stays in ollama.py on the entry's own class, so the generic class never
# learns about any one host's file.
import tempfile
import inspect
from handler.agent.providers import ollama as ollama_mod
from handler.agent.providers.ollama import OllamaOpenAI

keys = Path(tempfile.mkdtemp()) / "keys"
keys.write_text("Personal\tabc123\nsecond\tzzz\n")
check("the first line of the signin file is the key", ollama_mod._signed_in_key(str(keys)) == "abc123")
check("a missing signin file is no key, not a crash", ollama_mod._signed_in_key(str(keys) + ".nope") == "")
check("and the generic class takes no key file at all",
      "key_file" not in inspect.signature(OpenAI.__init__).parameters, inspect.signature(OpenAI.__init__))

real = ollama_mod._signed_in_key
ollama_mod._signed_in_key = lambda: real(str(keys))
env_key = os.environ.pop("OLLAMA_API_KEY", None)
try:
    signed_in = OllamaOpenAI("m", "https://host/v1", "NOT_SET_ANYWHERE")
    signed_in.setup(None)
    check("the ollama entry is the one that finds it", signed_in._key == "abc123", signed_in._key)
finally:
    ollama_mod._signed_in_key = real
    if env_key is not None: os.environ["OLLAMA_API_KEY"] = env_key

# The native ollama dialect is gone: no ollama SDK import, no "ollama" tool-call parser or
# result formatter, no effort dialect for it. ollama.com is the openai class with its own key
# lookup, and that is the whole of what this provider is.
import handler.agent.providers as registry
from handler.agent import effort
from tools._parser.ToProvider import SCHEMA_ADAPTERS, RESULT_FORMATTERS

check("the ollama entry is the openai dialect", type(registry.PROVIDERS["ollama"]).__mro__[1] is OpenAI)
check("it says so", registry.PROVIDERS["ollama"].name == "openai")
check("the ollama SDK is not imported by the worker", "ollama" not in sys.modules)
check("no ollama schema adapter is left", "ollama" not in SCHEMA_ADAPTERS)
check("nor a result formatter", "ollama" not in RESULT_FORMATTERS)
check("nor an effort dialect", "ollama_bool" not in effort.SHAPES and "ollama" not in effort.FALLBACK)
check("and its rungs still resolve through the openai shape",
      effort.resolve("ollama", "openai", "deepseek-v4.1-flash", "max") == {"reasoning_effort": "max"})

# And a 401 sent with no key at all says so, instead of repeating the server's "Unauthorized".
keyless = OpenAI("m", base + "/v1", "NOT_SET_ANYWHERE")
try:
    next(iter(keyless.stream("m", [keyless.user_message("hi")], [], params={"max_tokens": 1})), None)
    check("a 401 with no key explains itself", False, "no exception raised")
except RuntimeError as e:
    check("a 401 with no key explains itself", "no API key was sent" in str(e), str(e))
except Exception as e:
    check("a 401 with no key explains itself", False, f"{type(e).__name__}: {e}")

# A round that produced neither prose nor a call is still a turn on the wire, and null
# content is a 400 on servers stricter than openai's own.
blank = OpenAI("m", "https://host/v1", "K")
check("a silent assistant turn sends an empty string, not null",
      blank.assistant_message("", "", [])["content"] == "")
check("and so does one that only called a tool",
      blank.assistant_message("", "", [{"id": "call_0"}])["content"] == "",
      blank.assistant_message("", "", [{"id": "call_0"}]))

# The effort knob on ollama's openai schema: the levels this host actually reads, measured
# against the live endpoint. low/medium/high are all the model's default, so the rungs are
# the model's own and "max" has to reach the endpoint as max.
from handler.agent import effort

def rung(level):
    return effort.resolve("ollama", "openai", "deepseek-v4.1-flash", level)

check("max is sent as max, not as the default", rung("max") == {"reasoning_effort": "max"}, rung("max"))
check("off is sent as none, which is the only thing that stops it",
      rung("off") == {"reasoning_effort": "none"}, rung("off"))
check("low stays low", rung("low") == {"reasoning_effort": "low"}, rung("low"))
check("the rungs are the host's own four, with no medium to send",
      [rung(l)["reasoning_effort"] for l in effort.LADDER] == ["none", "low", "high", "high", "max"],
      [rung(l) for l in effort.LADDER])
check("and an off that cannot be reached is not claimed",
      effort.reachable("ollama", "openai", "deepseek-v4.1-flash", "off"))

srv.shutdown()

print()
if FAILS:
    print(f"{len(FAILS)} failed: " + ", ".join(FAILS))
    sys.exit(1)
print("all checks passed")
