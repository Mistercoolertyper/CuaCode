import ast, json
from pathlib import Path

try: import yaml
except ImportError: yaml = None
try: import tomllib
except ImportError: tomllib = None

def check(path: Path, text: str) -> str | None:
    """A syntax error caught now instead of the next time something runs the file.

    Cheap, because it is only a parse of text already in memory, and worth it: the
    alternative is the agent finding a stray bracket several tool calls later with
    no idea which edit left it there. The write still stands, because rolling back
    would leave the agent editing against content no longer on disk.
    """
    suf = path.suffix.lower()
    try:
        if suf in (".py", ".pyi"): ast.parse(text, filename=str(path))
        elif suf == ".json": json.loads(text)
        elif suf in (".yaml", ".yml") and yaml: yaml.safe_load(text)
        elif suf == ".toml" and tomllib: tomllib.loads(text)
        else: return None
    except SyntaxError as e:
        return f"line {e.lineno}: {e.msg}"
    except Exception as e:
        return f"{type(e).__name__}: {e}"
    return None
