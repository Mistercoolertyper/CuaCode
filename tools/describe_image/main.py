"""Eyes for a model that has none.

The agent loop withholds image-returning tools from a provider that cannot accept
images, which leaves a blind model no way to answer "what does that error say". So
the image goes to a sighted provider from the same config, run as a subagent with the
picture in its first message; only the looking is delegated.

Images are fetched by dispatching the existing tools against the full registry rather
than the filtered one the model sees: screenshot is hidden from this model and still
has to run for it.
"""
from handler.agent.subagent import AgentSpec, run as run_agent

SYSTEM = """You are looking at an image on behalf of a model that cannot see it.
It asked one question. Answer that question.

- Transcribe text exactly. Error messages, values, labels, filenames, code -
  wrong by one character is wrong, and the model reading you has no way to
  check.
- Say what is actually there. Not what an interface like this usually shows,
  and not what the question seems to hope for.
- If the image does not answer the question, say so and say what it does show
  instead. That is a useful answer; a plausible invention is not.
- Do not give coordinates unless asked. They are read off a picture and they
  are not accurate enough to click."""

SCHEMA = {"properties": {
    "description": {"type": "string",
                    "description": "The answer to the question, as briefly as it can be answered."},
    "text": {"type": "string",
             "description": "Text visible in the image, verbatim, where it bears on the question. Empty if none does."},
    "answers": {"type": "boolean",
                "description": "False if the image does not actually answer what was asked."}},
    "required": ["description", "answers"]}

def _images(result: dict) -> list:
    """Both shapes the image tools produce, same as the providers do."""
    data = (result or {}).get("result")
    if not isinstance(data, dict): return []
    one, many = data.get("image_base64"), data.get("images") or []
    return many if many else ([one] if one else [])

def run(args: dict, ctx) -> dict:
    from handler.agent.main import registry
    from tools.loader import dispatch
    from handler import config

    candidates = config.lookers()
    if not candidates:
        return {"error": "no vision-capable provider is configured with a key, "
                         "so there is nothing that can look at this. The user names one "
                         "under \"vision\" in config.json"}

    source = (args.get("source") or "screen").strip()
    reg = registry()
    if source.lower() == "screen":
        got = dispatch(reg, "screenshot", {}, ctx=ctx)
    else:
        got = dispatch(reg, "photos", {"sources": [source], "max_size": 1400}, ctx=ctx)
    if got.get("error"): return {"error": f"could not load the image: {got['error']}"}
    images = _images(got)
    if not images: return {"error": f"no image came back for {source!r}"}

    # Down the list rather than at the first name: the vision flag is per provider while
    # vision is a property of the model, so a provider that looks sighted on paper can
    # still be pointed at a text-only model, which is only discoverable by asking it. A
    # refusal is written down so no later call picks it again.
    tried = []
    for provider, model in candidates:
        used = model or config.model_for(provider)
        r = run_agent(AgentSpec(name="eyes", tools=[], effort="low", max_rounds=3,
                                system=SYSTEM, schema=SCHEMA, provider=provider,
                                model=model or None),
                      f"Question: {args['question']}", ctx=ctx, images=images)
        if not r.get("error"):
            out = dict(r["output"])
            # Named, because this is another model's account and the caller should weigh
            # it as one, and because a picture of the screen went to it.
            out["provider"] = f"{provider}/{used}"
            return out
        tried.append(f"{provider}/{used}: {r['error']}")
        # Every candidate gets a turn regardless of how the last one failed: a local
        # server that is not running says nothing about a hosted one. Only an outright
        # refusal to accept images is worth writing down.
        if config.is_image_rejection(r["error"]):
            config.learn_blind(provider, used)

    return {"error": "nothing could look at this image. " + "; ".join(tried)}
