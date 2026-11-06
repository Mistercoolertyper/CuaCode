"""CuaCode's worker: line-delimited JSON in, line-delimited JSON out.

That is the whole program. The agent is `handler/worker.py`, the protocol is
`handler/commands.py`, and this file exists to choose between two argv forms,
start the loop, and refuse one flag spelling that would otherwise look like it
worked. A frontend, a script, or an SDK in another repo drives this process over
the pipe and nothing else: `WIRE.md` says what the messages are, and `--wire`
prints the commands this build answers, straight off the table that serves them.

Exit codes: 0 when the pipe closes or `stop` arrives, 2 for a flag that was
removed.
"""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))

from handler.commands import serve, wire_doc
from handler.protocol import IPC
from handler.worker import Worker

argv = sys.argv[1:]

# `--detach <file>` was the way before `--detached`: one message, run with nobody
# attached. A frontend from before this flag would start this worker with the old
# one, it would ignore the message, read the /dev/null it was handed, see end of
# file and exit -- a run that looks sent and never happens. Stderr is where that
# frontend's exit report reads from, because stdout here is the wire.
if "--detach" in argv:
    print("main.py --detach <file> is gone: a box's worker is hosted by tmux and started "
          "with --detached (README, Boxes)", file=sys.stderr)
    sys.exit(2)

# What this build answers, printed instead of serving.
if "--wire" in argv:
    print(json.dumps(wire_doc(), indent=2))
    sys.exit(0)

serve(Worker(IPC(), detached="--detached" in argv))
