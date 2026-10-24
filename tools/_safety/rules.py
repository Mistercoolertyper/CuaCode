"""The user's own additions to what may run without being asked about.

Read from ~/.cuacode/config.json rather than hardcoded here: what one person
considers routine on their own machine is not something the repo can know, and a
checked-in list would be either too permissive for everyone or too narrow to be
useful. The shape:

    "permissions": {
      "shell_allow":     ["kubectl", "terraform"],
      "shell_deny":      ["ps"],
      "sensitive_paths": ["/work/vault"]
    }

Re-read on every call instead of cached, so editing the file takes effect on the
next tool call rather than the next launch; it is one small JSON read on a path
that already spawns a process.
"""

def rules() -> dict:
    try:
        from handler import config
        return (config.load().get("permissions") or {})
    except Exception:
        # A tool asked whether a call is safe must never fail loudly: the caller
        # reads a False as "ask the user", which is the right answer when the rules
        # themselves cannot be read.
        return {}

def extra(key: str) -> set:
    v = rules().get(key) or []
    return {str(x) for x in v} if isinstance(v, (list, tuple, set)) else set()
