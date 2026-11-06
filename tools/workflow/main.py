"""The `workflow` tool: run one stored orchestration script.

The installed list is read off disk at load time by describe() and schema(), as
in the agent tool.
"""
from handler.agent import workflow as wf, subagent
from tools.loader import brief_body

# The output and the log are the run; the row says how big the first was and how
# many lines the second had.
brief = brief_body("output", "log")

def _listing() -> list:
    try: return wf.listing()
    except Exception: return []

def describe(body: str) -> str:
    found = _listing()
    if not found:
        return body + "\n\nNo workflows are installed, so this tool has nothing to run."
    lines = "\n".join(f"- {name}: {desc}" for name, desc in sorted(found))
    return f"{body}\n\nInstalled workflows:\n{lines}"

def schema() -> dict:
    names = sorted(name for name, _ in _listing())
    which = {"type": "string", "description": "Which stored workflow to run."}
    if names: which["enum"] = names
    return {"properties": {
                "workflow": which,
                "args": {"type": "object",
                         "description": "Input for the workflow, in whatever shape its description asks for."}},
            "required": ["workflow"]}

def run(args: dict, ctx) -> dict:
    # A workflow spawns agents, so a subagent running one is recursion.
    if subagent.depth() > 0:
        return {"error": "subagents cannot run workflows"}
    return wf.run(args["workflow"], args.get("args") or {}, ctx=ctx)
