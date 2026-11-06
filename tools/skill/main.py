"""The `skill` tool: load one skill's instructions.

The list is read off disk every turn, so a skill written now loads next turn.
"""
from integrations.skills import loader as skills
from tools.loader import brief_body
from handler import faults

# The instructions are the answer and they are in the conversation; the row says
# how much text they were.
brief = brief_body("instructions")

def _installed() -> tuple[dict, str]:
    # scope="model": a skill marked `disable-model-invocation` is the user's to
    # reach for with /<name>, and is not offered here at all. The second half is
    # why the list is empty, because "no skills are installed" and "the skills
    # folder could not be read" are different answers and the model acts on the
    # first one as fact.
    try: return skills.load_skills(scope="model"), ""
    except Exception as e:
        return {}, faults.note(f"the skill list in {skills.user_dir()} could not be read", e,
                                "fix or remove the file it names")

def describe(body: str) -> str:
    found, problem = _installed()
    if problem: return f"{body}\n\n{problem}"
    if not found:
        return body + "\n\nNo skills are installed."
    lines = "\n".join(f"- {name}: {s.description or 'no description'}"
                      for name, s in sorted(found.items()))
    return f"{body}\n\nAvailable skills:\n{lines}"

def schema() -> dict:
    names = sorted(_installed()[0])
    which = {"type": "string", "description": "Which skill to load."}
    if names: which["enum"] = names
    return {"properties": {"skill": which}, "required": ["skill"]}

def run(args: dict, ctx) -> dict:
    try: s = skills.get(args["skill"], scope="model")
    except ValueError as e: return {"error": str(e)}
    out = {"name": s.name, "instructions": s.body, "dir": str(s.path)}
    if s.files: out["files"] = s.files
    return out
