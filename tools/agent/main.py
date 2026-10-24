"""The `agent` tool: run one installed subagent.

Which subagents exist is read off this machine at load time, through describe()
and schema(), so the model is offered the agents actually installed.
"""
from integrations.subagents import loader as agents
from handler.agent import subagent

def _installed() -> dict:
    # Called once per turn to notice a subagent file written a moment ago. Two
    # directory globs are cheaper than a round trip that finds out it was missed.
    try: return agents.load_agents(refresh=True)
    except Exception: return {}

def describe(body: str) -> str:
    found = _installed()
    if not found:
        return body + "\n\nNo subagents are installed, so this tool has nothing to run."
    lines = "\n".join(f"- {name}: {spec.description or 'no description'}"
                      for name, spec in sorted(found.items()))
    return f"{body}\n\nInstalled subagents:\n{lines}"

def schema() -> dict:
    found = sorted(_installed())
    agent = {"type": "string", "description": "Which subagent to run."}
    # Providers reject an empty enum, and describe() has already said the tool is
    # unusable with nothing installed.
    if found: agent["enum"] = found
    return {"properties": {
                "agent": agent,
                "prompt": {"type": "string",
                           "description": "The whole job, self-contained. The subagent sees nothing else."}},
            "required": ["agent", "prompt"]}

def run(args: dict, ctx) -> dict:
    # Depth is enforced in subagent.run() too; refusing here means the reason
    # comes back in words instead of as an empty result.
    if subagent.depth() > subagent.MAX_DEPTH:
        return {"error": "subagents cannot start further subagents"}
    try: spec = agents.get(args["agent"])
    except ValueError as e: return {"error": str(e)}
    return subagent.run(spec, args["prompt"], ctx=ctx)
