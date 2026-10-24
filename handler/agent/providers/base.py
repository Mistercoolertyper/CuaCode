"""Shared plumbing for the provider dialects: chunk normalization, usage
extraction, tool-call reassembly, message shapers, and the local-daemon probe."""

import json, socket
from dataclasses import dataclass, field
from urllib.parse import urlparse

# How every SDK client here is built. The SDKs default to 600s on every phase and retry
# twice inside the call, and the loop above makes four attempts of its own: a link that went
# away is silent rather than closed, so ten minutes of a spinner was the least of it. A read
# with nothing in it is the shape a dead link has, and the numbers below are where that is
# called dead. `max_retries` is 0 because retries belong to the loop, which is the only side
# that can say out loud that one is happening -- a silent one inside a request the user just
# stopped is a thread that keeps talking to a provider after they walked away.
# ponytail: fixed, not configurable. Add a `timeout` key beside context_window (config.LOCAL_KEYS
# + local_param) the day some provider needs longer than a slow first token.
TIMEOUT = {"connect": 15.0, "read": 300.0, "write": 60.0, "pool": 15.0, "retries": 0}

def client_kwargs() -> dict:
    """The timeout and retry policy for an SDK client, for every dialect's _client()."""
    import httpx
    t = TIMEOUT
    return {"timeout": httpx.Timeout(t["read"], connect=t["connect"],
                                     write=t["write"], pool=t["pool"]),
            "max_retries": t["retries"]}

def _segments(system) -> list[str]:
    """The system prompt as its parts: the instructions, then what is true about this machine,
    then whatever else. Kept apart because the dialects disagree about what a system prompt is:
    two take messages and can carry several, anthropic takes a list-of-blocks parameter.
    """
    if not system: return []
    if isinstance(system, str): system = [system]
    return [s for s in (str(x).strip() for x in system) if s]

def _int(obj, *names):
    """The first of these fields that is present and a number. Provider SDKs
    return objects, compatible servers return dicts, and both shapes turn up on
    the same wire format."""
    for n in names:
        v = obj.get(n) if isinstance(obj, dict) else getattr(obj, n, None)
        if isinstance(v, int): return v
    return None

def _sub(obj, *names):
    """A nested holder off a usage object, whichever of these names it uses."""
    for n in names:
        v = obj.get(n) if isinstance(obj, dict) else getattr(obj, n, None)
        if v is not None: return v
    return None

def usage_of(raw) -> dict:
    """Token counts off one streamed frame. Only what that frame carried, since no provider puts
    both halves in one place: openai on a usage trailer, anthropic
    on message_start (prompt) and message_delta (reply), and the caller merges. "input" is the whole
    prompt, and "cached" the part of it the provider already held: anthropic states it beside
    input_tokens, which excludes cached prompt entirely, so both counters are added back in. That
    makes cached a subset of input everywhere, which is the only way the same report can read all
    three. "reasoning" only where the provider splits it out, which anthropic, billing thinking
    inside output_tokens, never does.
    """
    if raw is None: return {}
    out = {}

    # openai and anthropic both hang a usage object somewhere. message_start
    # keeps it one level down, on the message. _sub rather than getattr because
    # this frame is a dict as often as it is an SDK object.
    for holder in (raw, _sub(raw, "message")):
        u = holder.get("usage") if isinstance(holder, dict) else getattr(holder, "usage", None)
        if u is None: continue
        if (n := _int(u, "prompt_tokens", "input_tokens")) is not None:
            out["input"] = n + (_int(u, "cache_read_input_tokens") or 0) \
                             + (_int(u, "cache_creation_input_tokens") or 0)
        if (n := _int(u, "completion_tokens", "output_tokens")) is not None:
            out["output"] = n
        # Where the cache hit is written: anthropic states it at the top level, the openai
        # dialect on the prompt's details. One number, two spellings, and the one the
        # openai SDK hands over is a plain field on a nested model, not a nested dict.
        pdet = _sub(u, "prompt_tokens_details", "input_tokens_details")
        if pdet is not None and (n := _int(pdet, "cached_tokens")) is not None:
            out["cached"] = n
        elif (n := _int(u, "cache_read_input_tokens")) is not None:
            out["cached"] = n
        det = _sub(u, "completion_tokens_details", "output_tokens_details")
        if det is not None and (n := _int(det, "reasoning_tokens")) is not None:
            out["reasoning"] = n
    return out

@dataclass
class Delta:
    """One normalized streaming chunk. tool_calls carries only complete calls: arguments
    arrive as partial JSON and accumulate inside stream(), which emits here once a call closes.
    raw keeps the untouched provider frame.
    """
    thinking: str = ""
    content: str = ""
    tool_calls: list = field(default_factory=list)
    raw: object = None

class CallAssembler:
    """Reassembles tool calls whose arguments arrive as JSON fragments. Anthropic and openai
    both stream them that way, so it lives here rather than in two copies. close() returns a
    neutral {id, name, args} for the provider to shape.
    """
    def __init__(self): self._open = {}

    def start(self, key, name: str = "", call_id: str = ""):
        """Idempotent and merging: openai sends id and name on the first fragment
        for an index, but not every compatible server does, so this can be called
        on each fragment with whatever that one has."""
        c = self._open.setdefault(key, {"id": "", "name": "", "buf": ""})
        if name: c["name"] = name
        if call_id: c["id"] = call_id

    def feed(self, key, fragment: str):
        c = self._open.get(key)
        if c: c["buf"] += fragment

    def close(self, key) -> dict | None:
        """The finished call, or None if its arguments never parsed: a truncated
        stream must not surface as a call with empty args."""
        c = self._open.pop(key, None)
        if not c: return None
        try: args = json.loads(c["buf"]) if c["buf"].strip() else {}
        except json.JSONDecodeError: return None
        return {"id": c["id"], "name": c["name"], "args": args}

    def close_all(self) -> list[dict]:
        return [c for c in (self.close(k) for k in list(self._open)) if c]

def attachment_note(images: list) -> str:
    """What the user called the files they attached. The name is all this adds, and "login-error.png"
    is often the most useful thing said about a picture nobody described. Same line the mid-run
    path uses (agent/main.py _steer_note), so an attachment reads the same wherever it arrived.
    """
    names = [i.get("name") or "image" for i in (images or [])]
    return "[attached: " + ", ".join(names) + "]" if names else ""

def append_user_text(msg: dict, extra: str) -> None:
    """Fold a runtime block into a user turn, in place. Recall, the docs notice, a skill's instructions
    and the interrupt note ride on the user's own message, since two user messages in a row is a 400 on
    anthropic. Content is a string in one dialect and blocks in the other two, so the text has to find
    the text block; an images-only turn grows one in front, an empty block being a 400 of its own.
    """
    if not extra: return
    content = msg.get("content")
    if content is None or isinstance(content, str):
        msg["content"] = content + "\n\n" + extra if content else extra
        return
    for block in content:
        if isinstance(block, dict) and block.get("type") == "text":
            block["text"] = block["text"] + "\n\n" + extra if block.get("text") else extra
            return
    content.insert(0, {"type": "text", "text": extra})

def _images(data) -> list:
    """Both shapes the tools produce: screenshot returns image_base64, photos returns an
    images list.
    """
    if not isinstance(data, dict): return []
    img, images = data.get("image_base64"), data.get("images") or []
    if img and not images: images = [img]
    return images

def _media_type(b64: str) -> str:
    """Read the format off the bytes rather than declaring one: screenshot returns PNG and
    photos JPEG, and a label that disagrees with the bytes is rejected by some models and
    quietly mis-decoded by others. Base64 is deterministic at the front, so the magic shows.
    """
    if b64.startswith("iVBORw0KGgo"): return "image/png"
    if b64.startswith("R0lGOD"): return "image/gif"
    if b64.startswith("UklGR"): return "image/webp"
    return "image/jpeg"

def models_at(url: str, key: str = "") -> list | None:
    """Model ids off a URL that is not this dialect's own catalog, or None.

    Some endpoints answer their own catalog badly or not at all while serving the same list
    somewhere else, so a provider entry may name the URL that does. Three shapes cover what has
    been seen: a bare list, or a list under `data` or `models`, each row an id, a name, a model
    field, or the string itself. None rather than [] on anything else, so a URL that is down or
    talking about something else reads as "no answer" and not as an empty account.
    """
    try:
        import httpx
        r = httpx.get(url, timeout=10, headers={"Authorization": f"Bearer {key}"} if key else None)
        r.raise_for_status()
        payload = r.json()
    except Exception:
        return None
    if isinstance(payload, dict):
        payload = payload.get("data") or payload.get("models") or []
    if not isinstance(payload, list): return None
    ids = []
    for row in payload:
        if isinstance(row, str): ids.append(row)
        elif isinstance(row, dict):
            name = next((row[k] for k in ("id", "name", "model")
                         if isinstance(row.get(k), str) and row[k]), "")
            if name: ids.append(name)
    return sorted(set(ids)) or None

def show_at(url: str, model: str, key: str = "") -> dict:
    """What a host says about one model, or {} when it will not say.

    The body posted is ollama's /api/show shape, and an entry names this URL the way it names
    models_url: for a dialect that cannot answer a question only the host can. Its catalog says
    nothing about the window and nothing about what the model can do. An empty dict is "no
    answer", so a host that is down leaves every question it was asked still open.
    """
    try:
        import httpx
        r = httpx.post(url, json={"model": model}, timeout=10,
                       headers={"Authorization": f"Bearer {key}"} if key else None)
        r.raise_for_status()
        body = r.json()
    except Exception:
        return {}
    return body if isinstance(body, dict) else {}

def context_of(info: dict) -> int:
    """The window off a /api/show body, or 0.

    A num_ctx in the parameters is what the server actually allocates, and it wins over the
    trained-for length; with neither set, the default is smaller still and unreported.
    """
    for line in str(info.get("parameters") or "").splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[0] == "num_ctx" and parts[1].isdigit(): return int(parts[1])
    for name, value in (info.get("model_info") or {}).items():
        if name.endswith("context_length") and isinstance(value, int): return value
    return 0

def _daemon_up(host: str | None, timeout: float = 0.5) -> bool:
    """Whether anything is listening at this address, for local servers speaking a dialect over a
    base_url (lm studio, llama.cpp, vllm), where installed is not running and a configured entry with
    nothing behind it must not be offered as reachable. Accepts host, host:port or a full url, an empty
    one meaning the usual ollama port. Only a TCP connect is checked, the guarded case being no listener.
    """
    host = (host or "").strip() or "127.0.0.1:11434"
    if "://" not in host: host = f"http://{host}"
    u = urlparse(host)
    try:
        with socket.create_connection((u.hostname or "127.0.0.1",
                                       u.port or (443 if u.scheme == "https" else 11434)), timeout):
            return True
    except OSError: return False