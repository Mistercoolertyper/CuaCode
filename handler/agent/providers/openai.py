"""openai chat-completions dialect: everything speaking that wire format."""

import json, os

from handler.agent.images import PinnedUser
from handler.agent.providers.base import (Delta, CallAssembler, _segments, _images, _media_type,
                                           models_at, show_at, context_of, client_kwargs)
from tools._parser.FromProvider import ToolCall

class OpenAI:
    """The openai chat-completions dialect. `name` is the schema dialect, not the registry
    key: everything speaking this wire format (openrouter, groq, deepseek, together, vllm,
    lm studio) is this class with a different base_url, registered under its own key.
    """
    name = "openai"

    def __init__(self, default_model="gpt-5.6-sol", base_url=None, key_env="OPENAI_API_KEY", vision=True,
                 models_url=None, show_url=None):
        # default_model is one name, or a dict of names to scores: a shortlist the entry ranks for
        # itself, best first, whose top entry is the model a round uses when settings name none.
        # The dict is read, never written.
        self.model_scores = dict(default_model) if isinstance(default_model, dict) else {}
        if self.model_scores: default_model = rank(self.model_scores)[0]
        self.default_model, self.base_url, self.key_env = default_model, base_url, key_env
        self.vision = vision
        # Where to read the model list when this endpoint's own /v1/models cannot be read:
        # named per provider entry, since it is a fact about the host, not about the user.
        self.models_url = models_url
        # And where to ask what one model is: its window and what it can do, for a host whose
        # dialect endpoint answers neither. /api/show is the one on ollama; a POST of {"model": m}.
        self.show_url = show_url
        self._key, self._c = "", None

    def setup(self, api_key: str | None):
        # The key often arrives as `Bearer x` straight from a provider's docs header, and the SDK
        # adds the scheme itself: `Bearer Bearer x` is a 401 that reads as a bad key. Last is the
        # stored_key hook, if this provider has one at all: this class reads no key file, and a
        # host that keeps one on disk says so on its own entry's class.
        key = (api_key or os.environ.get(self.key_env)
               or getattr(self, "stored_key", lambda: "")() or "").strip()
        if key.lower().startswith("bearer "): key = key[7:].strip()
        if key != self._key: self._key, self._c = key, None

    def _client(self):
        if self._c is None:
            from openai import OpenAI as _SDK      # imported late: a process that never streams never pays for it
            # Local servers (lm studio, llama.cpp, vllm) want no key but the
            # SDK refuses to build without one.
            self._c = _SDK(api_key=self._key or "not-needed", base_url=self.base_url,
                           **client_kwargs())
        return self._c

    def stream(self, model: str, messages: list[dict], tools: list, system: str = "", params: dict = None):
        # One system message per segment: this dialect accepts several and keeps
        # them in order, so nothing has to be joined.
        msgs = [{"role": "system", "content": s} for s in _segments(system)] + messages
        kw = {"model": model, "messages": msgs, "stream": True}
        if tools: kw["tools"] = tools
        # Verbatim passthrough (max_tokens, temperature, reasoning_effort, top_p), keys are the
        # provider's own: whatever the host does not know is its business to reject, and
        # config.quirks records the key so it is not sent again.
        kw.update({k: v for k, v in (params or {}).items() if k not in kw})
        # Streams no usage unless asked, and the context readout has nothing to show without
        # it. A server without the flag gets one retry rather than losing the turn.
        kw.setdefault("stream_options", {"include_usage": True})
        try:
            stream = self._client().chat.completions.create(**kw)
        except Exception as e:
            # A refusal with nothing sent is not the server's opinion of the key: there was no
            # key to have an opinion about, and every provider words that as plain "Unauthorized".
            if getattr(e, "status_code", 0) in (401, 403) and not self._key:
                raise RuntimeError(
                    f"no API key was sent for {self.base_url or 'this endpoint'}: set one in the "
                    "provider's settings") from e
            if "stream_options" not in str(e): raise
            kw.pop("stream_options", None)
            stream = self._client().chat.completions.create(**kw)
        asm, order, last = CallAssembler(), [], None
        try:
            for chunk in stream:
                last = chunk
                if not chunk.choices:
                    yield Delta(raw=chunk)          # usage-only trailer on some servers
                    continue
                d = chunk.choices[0].delta
                for tc in getattr(d, "tool_calls", None) or []:
                    i = tc.index
                    if i not in order: order.append(i)
                    fn = getattr(tc, "function", None)
                    asm.start(i, getattr(fn, "name", "") or "", getattr(tc, "id", "") or "")
                    if fn and fn.arguments: asm.feed(i, fn.arguments)
                # openai's own models never return reasoning text, but several
                # compatible servers put it on one of these.
                think = getattr(d, "reasoning_content", None) or getattr(d, "reasoning", None) or ""
                content = getattr(d, "content", None) or ""
                if think or content: yield Delta(thinking=think, content=content, raw=chunk)
            # Arguments arrive as JSON fragments with no per-call stop event, so a
            # call can only be closed once the stream ends.
            done = [c for c in (asm.close(i) for i in order) if c]
            if done: yield Delta(tool_calls=[_openai_call(c) for c in done], raw=last)
        finally:
            stream.close()

    def models(self) -> list | None:
        """Whatever /v1/models returns, or the URL the entry names, ranked when it carries scores.
        Every server speaking this dialect has the endpoint, and what is behind it ranges from one
        local model to openrouter's several hundred, so the length is not this method's problem.
        """
        if self.models_url: found = models_at(self.models_url, self._key)
        else:
            try: found = sorted(m.id for m in self._client().models.list())
            except Exception: found = None
        if not self.model_scores: return found
        # Scored first, best first, then the rest of the host's list alphabetically: the shortlist
        # answers "which should I use", and the tail keeps a model that is not on it reachable. A
        # host that will not say leaves the shortlist, which is still an answer.
        return rank(self.model_scores) + [m for m in (found or []) if m not in self.model_scores]

    def capabilities(self, model: str) -> list | None:
        """What this model can do, when the endpoint publishes it, else None. Only openrouter
        does, carrying input_modalities per model; the other endpoints describe no modality, and the
        aggregators (groq, together, nvidia, lmstudio) answer only by rejecting an image once. None
        means "no answer", not "no vision": an unreachable catalog must not blind a sighted model.
        A show_url wins over that lookup: it is the host being asked directly.
        """
        if self.show_url:
            caps = show_at(self.show_url, model, self._key).get("capabilities")
            return list(caps) if isinstance(caps, list) else None
        if not self.base_url or "openrouter.ai" not in self.base_url: return None
        try:
            import httpx
            data = httpx.get("https://openrouter.ai/api/v1/models", timeout=10).json()["data"]
        except Exception: return None
        for m in data:
            if m.get("id") != model: continue
            arch, params = m.get("architecture") or {}, m.get("supported_parameters") or []
            caps = ["completion"]
            if "image" in (arch.get("input_modalities") or []): caps.append("vision")
            if "tools" in params: caps.append("tools")
            # Whether the effort knob does anything here: openrouter lists reasoning per model,
            # so this is the one openai-dialect endpoint that can be asked instead of guessed.
            if {"reasoning", "reasoning_effort", "include_reasoning"} & set(params):
                caps.append("thinking")
            return caps
        return None                     # unlisted model: not an answer either

    def context_window(self, model: str) -> int:
        """The model's window, when the server publishes one, else 0. /v1/models is a fixed schema
        on paper and a grab bag in practice: the direct openai endpoint returns four fields and none is
        the window, openrouter puts context_length on every row, and the self-hosted servers (vllm,
        sglang, lmstudio) report max_model_len. The SDK keeps unknown fields, so one call reaches them.
        A show_url is asked instead when the entry names one, for a host whose catalog describes no
        window at all -- ollama's /v1/models carries four fields and none of them is this.
        """
        if self.show_url: return context_of(show_at(self.show_url, model, self._key))
        # Short timeout, unlike every other call here: this one runs on the path that reports
        # what a round cost, and a dead catalog endpoint is not worth waiting on.
        try:
            c = self._client()
            m = (c.with_options(timeout=5.0) if hasattr(c, "with_options") else c).models.retrieve(model)
        except Exception: return 0
        extra = dict(getattr(m, "model_extra", None) or {})
        # openrouter states the catalog's number at the top level and the serving
        # provider's -- the one a request actually gets -- below it.
        for source in (extra.get("top_provider") or {}, extra):
            if not isinstance(source, dict): continue
            for key in ("context_length", "max_model_len", "max_context_length", "context_window"):
                n = source.get(key)
                if isinstance(n, int) and n > 0: return n
        return 0

    def assistant_message(self, thinking: str, content: str, tool_calls: list) -> dict:
        # Reasoning text is display-only here: chat-completions has no field to send it back in.
        # It still reaches disk, on the session record. Content is always a string, never null:
        # a round that said nothing is still a turn on the wire, and null is a 400 on servers
        # stricter than openai's own (ollama: "invalid message content type: <nil>"), while an
        # empty string is accepted by both.
        msg = {"role": "assistant", "content": content or ""}
        if tool_calls: msg["tool_calls"] = tool_calls
        return msg

    def user_message(self, text: str, images: list = None) -> dict:
        if not images: return {"role": "user", "content": text}
        # An empty text block is a 400 here and on anthropic, and an attachment with no words
        # is an ordinary thing to do, so the block is left out rather than empty.
        return PinnedUser(role="user", content=[
            *([{"type": "text", "text": text}] if text else []),
            *({"type": "image_url", "image_url": {"url": f"data:{_media_type(b)};base64,{b}"}} for b in images)])

    def parse_calls(self, native: list) -> list:
        out = []
        for i, c in enumerate(native):
            fn = c.get("function", {})
            args = fn.get("arguments")
            if isinstance(args, str):
                # Unlike ollama, arguments cross the wire as a JSON string.
                try: args = json.loads(args) if args.strip() else {}
                except json.JSONDecodeError: args = {}
            out.append(ToolCall(c.get("id") or f"call_{i}", fn.get("name", ""), args or {}))
        return out

    def result_messages(self, pairs: list) -> list[dict]:
        """One tool message per result, plus a user message for any images. A tool message
        cannot carry an image in this dialect, and json.dumps of a screenshot result would inline
        ~700KB of base64 as plain text the model cannot see as an image anyway.
        """
        out = []
        for call, result in pairs:
            data = (result or {}).get("result")
            images = _images(data)
            if not images:
                out.append({"role": "tool", "tool_call_id": call.id, "content": json.dumps(result)})
                continue
            meta = {k: v for k, v in data.items() if k not in ("image_base64", "images")}
            if not self.vision:
                # Replay only: a blind model is never offered an image-returning tool, but a
                # session recorded on a sighted one can still be reopened here, and the image 400s.
                out.append({"role": "tool", "tool_call_id": call.id, "content": json.dumps(
                    {"note": f"{len(images)} image(s) captured, not shown: this model cannot see images. "
                             "describe_image can have a sighted model answer one question about it",
                     **meta})})
                continue
            out.append({"role": "tool", "tool_call_id": call.id, "content": json.dumps(
                {"note": "images are attached as the next user message, not included here", **meta})})
            out.append({"role": "user", "content": [
                {"type": "text", "text": f"here are the {len(images)} photo(s) you requested"},
                *({"type": "image_url", "image_url": {"url": f"data:{_media_type(b)};base64,{b}"}} for b in images)]})
        return out

    def native_calls(self, calls: list[dict]) -> list[dict]:
        return [_openai_call({"id": c.get("id") or f"call_{i}", "name": c["name"], "args": c["args"]})
                for i, c in enumerate(calls)]

def _openai_call(c: dict) -> dict:
    return {"id": c["id"], "type": "function",
            "function": {"name": c["name"], "arguments": json.dumps(c["args"])}}

def rank(scores: dict) -> list:
    """Names by score, highest first, ties in the order they were written.

    Stable on purpose: two models the entry rates the same come back in the order of its own
    dict, so the list reads the way it was typed rather than the way the host sorts.
    """
    return sorted(scores, key=lambda m: -scores[m])