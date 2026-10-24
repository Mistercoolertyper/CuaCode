import json, yaml, importlib.util
from pathlib import Path
from dataclasses import dataclass
from typing import Callable

from tools._parser.Validate import check

@dataclass
class Tool:
    name: str
    description: str
    input_schema: dict
    output_schema: dict
    active: bool
    handler: Callable
    require_permissions: bool
    # Set only for tools that answer describe()/schema() at runtime, so they can
    # be re-asked without re-executing every main.py. body is the Description.md
    # text describe() was handed.
    module: object = None
    body: str = ""
    # Offered only to a model that cannot see, since standing in for eyes is its
    # whole job.
    blind_only: bool = False
    # Withheld from a model that cannot see. A coordinate is read off a
    # screenshot; with none, the call is a guess that lands somewhere real and
    # cannot be taken back.
    needs_sight: bool = False
    # May be started and left to finish on its own. Declared per tool because it
    # is only true of the slow ones that return an answer (a build, a fetch, a
    # subagent), and meaningless on a click, whose value is having happened by
    # the next call.
    backgroundable: bool = False
    # What this call would do, asked before the user is asked to allow it. Only
    # defined by a tool that can say something the arguments do not (file draws
    # the diff an edit would produce); the loop shows the arguments alone for
    # every tool that does not.
    preview: Callable = None
    # Per-call danger, asked of the tool itself: require_permissions is
    # tool-wide, but most tools are only sometimes dangerous (a file read, a
    # harmless command). safe() says which; without it every call is asked.
    safe: Callable = None

def parse_frontmatter(text: str) -> tuple[dict, str]:
    if not text.startswith("---"): return {}, text.strip()
    _, meta_block, body = text.split("---", 2)
    return yaml.safe_load(meta_block) or {}, body.strip()

def _load_main(path: Path):
    spec = importlib.util.spec_from_file_location(f"tools.{path.parent.name}.main", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod

def load_tools(tools_dir="tools") -> dict[str, Tool]:
    registry = {}
    for folder in sorted(Path(tools_dir).iterdir()):
        if not folder.is_dir() or folder.name.startswith("_"): continue
        desc_f, schema_f, main_f = folder / "Description.md", folder / "InputSchema.json", folder / "main.py"
        missing = [f.name for f in (desc_f, schema_f, main_f) if not f.exists()]
        if missing: raise RuntimeError(f"{folder.name}: missing {missing}")
        meta, body = parse_frontmatter(desc_f.read_text(encoding="utf-8"))
        mod = _load_main(main_f)
        if not hasattr(mod, "run"): raise RuntimeError(f"{folder.name}: main.py has no run()")
        # Options that only exist at runtime (installed subagents, workflows)
        # cannot be written into a file months earlier. describe() and schema()
        # let main.py answer instead; a tool with neither loads as before.
        registry[folder.name] = Tool(
            name=meta.get("name", folder.name),
            description=mod.describe(body) if hasattr(mod, "describe") else body,
            input_schema=mod.schema() if hasattr(mod, "schema") else json.loads(schema_f.read_text(encoding="utf-8")),
            output_schema=meta.get("output", {}),
            active=meta.get("active", True),
            require_permissions=meta.get("require_permissions", False),
            blind_only=meta.get("blind_only", False),
            needs_sight=meta.get("needs_sight", False),
            backgroundable=meta.get("backgroundable", False),
            handler=mod.run,
            preview=getattr(mod, "preview", None),
            safe=getattr(mod, "safe", None),
            module=mod if (hasattr(mod, "describe") or hasattr(mod, "schema")) else None,
            body=body,
        )
    return registry

def refresh_dynamic(registry: dict[str, Tool]) -> dict[str, Tool]:
    """Re-ask the self-describing tools what they offer.

    The registry is built once per process, and an agent that writes a new
    subagent file has to run it in the same conversation. Only the tools that
    answer at runtime are asked again, so no other main.py is re-executed. A
    hook that raises leaves the previous value in place.
    """
    for t in registry.values():
        if t.module is None: continue
        try:
            if hasattr(t.module, "describe"): t.description = t.module.describe(t.body)
            if hasattr(t.module, "schema"): t.input_schema = t.module.schema()
        except Exception: pass
    return registry

def dispatch(registry: dict[str, Tool], name: str, args: dict, ctx=None) -> dict:
    tool = registry.get(name)
    if not tool: return {"error": f"unknown tool: {name}"}
    # Checked against the schema before the handler, so a missing field comes
    # back as a sentence instead of a KeyError. Defaults stay unfilled: the
    # handler's own args.get() owns what absent means.
    args, errs = check(args or {}, tool.input_schema)
    if errs: return {"error": "; ".join(errs)}
    try: return {"result": tool.handler(args, ctx)}
    except Exception as e: return {"error": str(e)}
