"""The `mcp` tool: reach the MCP servers registered on this machine.

Deliberately one tool rather than one tool per MCP tool: a server can expose
thirty tools with thirty schemas, and folding those into the model's own tool
list would put every argument of every server in context for the whole
conversation. describe() lists the servers without starting anything, `load`
spends context on one server's tool schemas, and `call` runs a tool. A server
process starts on first use and is kept for the rest of the session.
"""
from integrations.mcp import client, loader
from tools import _window
from handler import faults
import re

MAX_TOOLS_SHOWN = 60


def _servers() -> tuple[dict, str]:
    # Re-read every turn so a server registered a moment ago is usable in the
    # next one. The reason comes back with the list: a config that cannot be
    # parsed is not a machine with no servers, and the model reads that second
    # answer as fact.
    try:
        return loader.load_servers(), ""
    except Exception as e:                                      # noqa: BLE001
        return {}, faults.note(f"the MCP server config in {loader.user_config()} could not be read", e,
                               "fix or remove the file it names")


def describe(body: str) -> str:
    found, problem = _servers()
    if problem: return f"{body}\n\n{problem}"
    if not found:
        return (body + "\n\nNo MCP servers are registered, so this tool has nothing to reach. "
                f"They are registered in {loader.user_config()}.")
    live = client.connected()
    lines = []
    for name, cfg in sorted(found.items()):
        note = cfg.get("description") or "no description"
        lines.append(f"- {name}: {note}" + ("  [loaded]" if name in live else ""))
    return (f"{body}\n\nRegistered MCP servers:\n" + "\n".join(lines) +
            "\n\nCall load on one to see the tools it offers before calling them.")


def schema() -> dict:
    names = sorted(_servers()[0])
    server = {"type": "string", "description": "Which registered MCP server."}
    if names:
        server["enum"] = names
    return {"properties": {
                "action": {"type": "string", "enum": ["list", "load", "call", "stop"],
                           "description": "list: what is registered. load: start a server and "
                                          "return its tools and their schemas. call: run one of "
                                          "those tools. stop: shut a server down."},
                "server": server,
                "tool": {"type": "string",
                         "description": "For call: the tool name, exactly as load reported it."},
                "arguments": {"type": "object",
                              "description": "For call: arguments matching that tool's schema."},
                "session": {"type": "string",
                             "description": "For call: an MCP session id or handle to route this "
                                            "call to. Omit to use the current run's session -- a "
                                            "subagent run gets its own automatically."}},
            "required": ["action"]}


def _listing() -> dict:
    found, problem = _servers()
    live = client.connected()
    out = {"servers": [{"name": name,
                        "description": cfg.get("description", ""),
                        "command": " ".join([str(cfg.get("command", ""))] +
                                            [str(a) for a in (cfg.get("args") or [])]).strip(),
                        "loaded": name in live}
                       for name, cfg in sorted(found.items())],
           "config": str(loader.user_config())}
    # An empty list and an unreadable config are different answers, and a picker
    # drawing the first one for the second says "you have no servers" as fact.
    if problem: out["error"] = problem
    return out


def _load(name: str) -> dict:
    conn = client.pool(name, loader.get(name))
    tools = conn.list_tools()
    shown = tools[:MAX_TOOLS_SHOWN]
    out = {"server": name,
           "server_info": conn.server_info,
           "protocol": conn.protocol,
           "era": conn.era,
           "tools": [{"name": t.get("name"),
                      "description": t.get("description", ""),
                      "input_schema": t.get("inputSchema") or {}}
                     for t in shown]}
    if getattr(conn, "instructions", ""):
        out["instructions"] = conn.instructions
    if len(tools) > len(shown):
        # Said out loud rather than silently dropped: a truncated list that
        # looks complete is how a model concludes a tool does not exist.
        out["truncated"] = f"showing {len(shown)} of {len(tools)} tools"
    return out


def _session_id_from(res: dict) -> str | None:
    """Pull the id out of a session_new result, whatever shape it came back in."""
    data = res.get("data")
    if isinstance(data, dict):
        for k in ("id", "session_id", "name"):
            if data.get(k):
                return str(data[k])
    if isinstance(data, str) and data:
        return data
    text = res.get("text")
    if isinstance(text, str) and text.strip():
        m = re.match(r"session\s+(\S+)", text.strip())
        return m.group(1) if m else text.strip()
    return None


def _session_keys(cfg: dict) -> tuple[str, str]:
    """The tool that mints a session and the argument that carries it back.
    Only the stateless era needs the argument, and only its server knows its
    name, so both are config with a default."""
    return (cfg.get("session_tool") or "session_new",
            cfg.get("session_argument") or "session")


def _resolve_session(name: str, conn, cfg: dict, explicit: str | None) -> str | None:
    """Which session a call belongs to. An explicit id wins; otherwise a subagent
    run gets its own session, created lazily on first use, while the top-level
    conversation uses the server's default."""
    if explicit:
        return explicit
    maker, _ = _session_keys(cfg)
    if conn.era == "modern":
        # Stateless era: nothing to route unless the server offers the session
        # tool.
        if not conn.has_tool(maker):
            return None
    elif conn.transport != "http":
        return None
    scope = client.session_scope()
    if scope is None:
        return None
    sid = scope.get(name)
    if sid is None:
        res = conn.call(maker, {})
        sid = _session_id_from(res)
        if sid:
            scope[name] = sid
    return sid


def _with_handle(conn, cfg: dict, tool: str, arguments: dict, sid: str) -> dict:
    """The stateless era's answer to a session header: an ordinary argument, added
    only when the tool's own schema declares one."""
    _, argument = _session_keys(cfg)
    props = conn.tool_schema(tool).get("properties") or {}
    if argument not in props or arguments.get(argument) is not None:
        return arguments
    return {**arguments, argument: sid}


def _call(name: str, tool: str, arguments: dict, session: str | None, ctx) -> dict:
    cfg = loader.get(name)
    # A server may start a GUI of its own, a browser usually. Baseline before
    # the call so that window can be parked after, instead of sitting wherever
    # the app last remembered being, which is over the terminal.
    _window.baseline()
    conn = client.pool(name, cfg)
    sid = _resolve_session(name, conn, cfg, session)
    if sid and conn.era == "modern":
        arguments = _with_handle(conn, cfg, tool, arguments, sid)
        sid = None                          # there is no session header to route by
    result = conn.call(tool, arguments, session=sid)
    opened = _window.park_new(getattr(ctx, "self_identity", None))
    if opened:
        result["opened_apps"] = opened
    if result.pop("is_error", False):
        # The server's own explanation, so the model gets something it can act
        # on.
        return {"error": result.get("text") or result.get("data") or f"{name}.{tool} failed"}
    return {"server": name, "tool": tool, **result}


def preview(args: dict, ctx) -> dict:
    """What the user is being asked to allow, in one line."""
    action = args.get("action")
    server = args.get("server") or "?"
    if action == "call":
        return {"summary": f"MCP {server} -> {args.get('tool')}"}
    if action == "load":
        cfg = _servers()[0].get(server) or {}
        argv = " ".join([str(cfg.get("command", ""))] + [str(a) for a in (cfg.get("args") or [])])
        return {"summary": f"start MCP server {server}: {argv.strip()}"}
    return {"summary": f"MCP {action}"}


def run(args: dict, ctx) -> dict:
    action = args.get("action")
    server = args.get("server")

    if action == "list":
        return _listing()

    if action in ("load", "call", "stop") and not server:
        return {"error": f"server is required for {action}"}

    try:
        if action == "load":
            return _load(server)
        if action == "stop":
            return {"server": server, "stopped": client.close(server)}
        if action == "call":
            tool = args.get("tool")
            if not tool:
                return {"error": "tool is required for call"}
            return _call(server, tool, args.get("arguments") or {},
                         args.get("session"), ctx)
    except ValueError as exc:                                   # unknown server
        return {"error": f"MCP {server}: {exc}"}
    except client.MCPError as exc:
        return {"error": f"MCP {server}: {exc}"}

    return {"error": f"unknown action: {action!r}"}
