"""Skills: instructions loaded on demand instead of carried all the time.

A skill is a folder with a SKILL.md; only its name and one-line description are in context by default, so fifty skills cost fifty lines rather than fifty documents, and the body is read when asked for, with anything else in the folder left on disk and pointed at by path.
A house style or a safety rule is only useful already in force, and asking for it in AGENTS.md does not work reliably: the agent has to notice, decide and spend a tool call, exactly on the turn it forgets.
Those are marked `always: true` and their bodies go into the system prompt at startup, whole, every conversation, costing their full length on every request. That trade is deliberate, and made for a handful of skills rather than fifty.
"""
import re
from dataclasses import dataclass
from pathlib import Path

from tools.loader import parse_frontmatter
from handler.session import store

_BUNDLED = Path(__file__).parent
MAX_BODY = 20_000        # a skill that does not fit is a skill that should link out

# What every always-on skill may cost between them, in characters: a ceiling, not a cap on how many may be marked.
# Bodies past it are dropped and named in the block, so the reason reaches the model rather than only a log nobody reads.
MAX_ALWAYS = 40_000

@dataclass
class Skill:
    name: str
    description: str
    path: Path
    body: str
    files: list
    # Who may reach for it, both by default: disable-model-invocation keeps it out of the skill tool's list and
    # disable-user-invocation out of the /palette. Both off leaves a skill nothing can load, so that case is dropped.
    model_ok: bool = True
    user_ok: bool = True
    # In the system prompt from startup, not loaded on demand: `always: true` in the skill's own frontmatter, or naming it in the config's always_skills.
    # The config key forces on a bundled skill without editing a file that the next update overwrites.
    always: bool = False

def _flag(meta: dict, *names) -> bool:
    """A frontmatter bool, written with dashes or underscores, either way."""
    for n in names:
        if n in meta: return str(meta[n]).strip().lower() in ("true", "yes", "1", "on")
    return False

def always_names() -> set:
    """Skill names the config forces on, whatever their frontmatter says.

    The import is late because config reaches into the provider registry and skills are loaded from
    inside it. A missing or malformed key means none forced, never an error."""
    try:
        from handler import config
        names = config.load().get("always_skills") or []
        return {str(n).strip() for n in names if str(n).strip()}
    except Exception:
        return set()


def user_dir() -> Path:
    d = store.home() / "skills"
    d.mkdir(parents=True, exist_ok=True)
    return d

def _skill(folder: Path):
    f = folder / "SKILL.md"
    if not f.exists(): return None
    meta, body = parse_frontmatter(f.read_text())
    if len(body) > MAX_BODY:
        body = body[:MAX_BODY] + f"\n\n[truncated: SKILL.md is over {MAX_BODY} characters]"
    model_ok = not _flag(meta, "disable-model-invocation", "disable_model_invocation")
    user_ok = not _flag(meta, "disable-user-invocation", "disable_user_invocation")
    if not model_ok and not user_ok:
        return None                       # nothing could ever load it; do not pretend it is installed
    return Skill(model_ok=model_ok, user_ok=user_ok,
                 always=_flag(meta, "always", "always-load", "always_load",
                              "auto-load", "auto_load"),
                 name=meta.get("name") or folder.name,
                 description=meta.get("description", ""),
                 path=folder, body=body,
                 # Listed, not read: what is worth loading is the skill's own decision,
                 # which it states in the body.
                 files=sorted(str(p.relative_to(folder)) for p in folder.rglob("*")
                              if p.is_file() and p.name != "SKILL.md"))

def load_skills(refresh: bool = True, scope: str = None) -> dict:
    """Every installed skill, or only the ones `scope` is allowed to load.

    scope is "model" (the skill tool), "user" (a frontend's slash palette), or None for everything, since counting for /context does not care who may invoke what.
    An always-on skill is in neither scope: its body is already in the system prompt, so offering it would pay for it twice. A call with no scope still has it."""
    out = {}
    for d in (_BUNDLED, user_dir()):          # user second: it overwrites
        if not d.exists(): continue
        for folder in sorted(p for p in d.iterdir() if p.is_dir()):
            if folder.name.startswith(("_", ".")): continue
            try: s = _skill(folder)
            except Exception: continue        # one broken skill must not hide the rest
            if not s: continue
            out[s.name] = s
    for name in always_names() & set(out):
        out[name].always = True
    if scope in ("model", "user"):
        out = {n: s for n, s in out.items()
               if not s.always and (s.model_ok if scope == "model" else s.user_ok)}
    return out


def get(name: str, scope: str = None) -> Skill:
    found = load_skills(scope=scope)
    if name not in found:
        raise ValueError(f"unknown skill: {name!r} (have {sorted(found) or 'none installed'})")
    return found[name]

# --------------------------------------------------------------------------
# user invocation: $<name> typed in a frontend

def listing(scope: str = "user") -> list:
    """Name and description of every skill `scope` may load, for a picker."""
    return [{"name": s.name, "description": s.description}
            for _, s in sorted(load_skills(scope=scope).items())]

def invocation(text: str) -> str:
    """The skill block(s) a user message asked for, or "" when none is named.

    A skill is named with `$name` anywhere in the text, or `/<name>` opening the message, which is what
    older frontends wrote. Each body rides beside the message, and the user's own line is left alone."""
    names = []
    text = text or ""
    line = text.lstrip()
    if line.startswith("/") and len(line) > 1:
        if n := line[1:].split(None, 1)[0].strip():
            names.append(n)
    # $skill anywhere in the message: a "$" starting a word, then name characters. A
    # dollar amount ("$5") looks up a skill called "5", finds none, and stays prose.
    for m in re.finditer(r"(?<!\S)\$([\w-]+)", text):
        names.append(m.group(1))
    # One block per unique skill that actually exists, in the order asked.
    blocks, seen = [], set()
    for n in names:
        if n in seen:
            continue
        seen.add(n)
        try:
            s = get(n, scope="user")
        except ValueError:
            continue
        blocks.append(f"skill loaded: {s.name}\n\n"
                      f"<skill name=\"{s.name}\" dir=\"{s.path}\">\n"
                      "The user asked for this skill by name. These are its instructions;\n"
                      "follow them for the message it came with.\n\n"
                      f"{s.body}\n"
                      + (f"\nFiles next to it, on disk, unread: {', '.join(s.files)}\n" if s.files else "")
                      + "</skill>")
    return "\n\n".join(blocks)


# --------------------------------------------------------------------------
# always on: in the system prompt from startup

def always_on() -> list:
    """The skills marked always-on, in name order."""
    return [s for _, s in sorted(load_skills().items()) if s.always]


def always_block() -> str:
    """Their bodies as one system segment, or "" when none is marked.

    Built the same way every turn from the same files, so a provider caching the system prompt as a prefix keeps hitting it.
    Whole bodies, not the index, and what the budget does not fit is named rather than silently dropped."""
    loaded, dropped, used = [], [], 0
    for s in always_on():
        if used + len(s.body) > MAX_ALWAYS:
            dropped.append(s.name)
            continue
        used += len(s.body)
        loaded.append(f"<skill name=\"{s.name}\" dir=\"{s.path}\">\n{s.body}\n"
                      + (f"\nFiles next to it, on disk, unread: {', '.join(s.files)}\n" if s.files else "")
                      + "</skill>")
    if not loaded and not dropped: return ""
    cut = (f"\n[not loaded, no room left under {MAX_ALWAYS} characters: "
           f"{', '.join(dropped)}. Unmark one of the others or move detail into "
           "files beside it.]\n") if dropped else ""
    return ("<always_on_skills>\n"
            "These skills were marked always-on, so their instructions are here in full\n"
            "rather than waiting behind the skill tool. They are in force for every turn\n"
            "of this conversation without being asked for. Do not load them again -- they\n"
            "are not in the skill tool's list, and this is the same text it would return.\n\n"
            + "\n\n".join(loaded) + cut + "\n</always_on_skills>") if loaded else (
            "<always_on_skills>" + cut + "</always_on_skills>")
