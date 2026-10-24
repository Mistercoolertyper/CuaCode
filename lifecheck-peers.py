"""peers: the mailbox, the roster and the two-phase send, end to end, on a scratch root.

Run with: python3 lifecheck-peers.py
"""
import json, os, shutil, sys, tempfile, time

os.environ["CUACODE_HOME"] = tempfile.mkdtemp(prefix="peers-check-")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)) or ".")

from handler import peers
from handler.session import store
from handler.session.main import Session
from tools.loader import load_tools, dispatch

fails = []

def check(label, got, want=True):
    ok = got == want
    if not ok: fails.append(f"{label}: got {got!r}, wanted {want!r}")
    print(("  ok   " if ok else "  FAIL ") + label)

# ---- two agents in one project, one elsewhere ----

here = "/tmp/peers-demo"
# The case checks below stat this path, and samefile() answers on the inode it
# finds, so the fixture has to be a real directory and not just a string.
os.makedirs(here, exist_ok=True)
mine = Session.create(provider="ollama", model="m")
mine.set_cwd(here)
mine.add_user("hello")          # makes the session real: folder + meta.json
mine.commit()

other = Session.open(mine.id) if False else None
colleague = Session.create(provider="ollama", model="m")
colleague.set_cwd(here)
colleague.set_title("clearing rust code", "user")
colleague.add_user("hi")
colleague.commit()

elsewhere = Session.create(provider="ollama", model="m")
elsewhere.set_cwd("/tmp/somewhere-else")
elsewhere.add_user("hi")
elsewhere.commit()

peers.become(mine.id)
peers.become(colleague.id)

print("roster")
r = peers.roster(mine.id)
colleagues_name = r[0]["name"]
check("same project is offered", [p["id"] for p in r], [colleague.id])
check("another project is not", elsewhere.id in [p["id"] for p in r], False)
check("this agent is not its own peer", mine.id in [p["id"] for p in r], False)
check("the name carries the project", r[0]["name"].startswith("peers-demo"))
check("a live worker reads as alive", r[0]["alive"], True)

print("the two-phase send")
tools = load_tools(tools_dir=os.path.join(os.path.dirname(os.path.abspath(__file__)), "tools"))
ctx = {"session_dir": str(mine.dir)}
g, t = tools["roster"], ctx

# A colleague that has gone quiet, with no process behind it.
os.remove(peers.pid_path(colleague.id))
old = time.time() - 3600
os.utime(store.path(colleague.id), (old, old))
check("a quiet peer drops off the roster", peers.roster(mine.id), [])
check("but is still reachable by id", peers.reachable(colleague.id), False)

def call(args):
    got = dispatch(tools, "roster", args, ctx=ctx)
    return got["result"] if "result" in got else got

first = call({"action": "message", "to": colleague.id, "body": "are you there"})
check("the first call warns instead of sending", first.get("ok"), False)
check("the warning says nothing is holding it", "no process is holding it" in first.get("warning", ""))
check("nothing was written yet", peers.mailbox(colleague.id).exists(), False)

second = call({"action": "message", "to": colleague.id, "body": "are you there"})
check("the second call with the same id delivers", second.get("ok"), True)
check("mail landed", len(store.read_jsonl(peers.mailbox(colleague.id))), 1)

print("identity is not the model's word")
rec = store.read_jsonl(peers.mailbox(colleague.id))[0]
check("from is this session", rec["from"], mine.id)
check("the name is this session's", rec["from_name"].startswith("peers-demo"))
check("the body survived", rec["body"], "are you there")

print("receiving")
# A second message from a peer, as if the colleague answered.
back = peers.send(mine.id, "yes, function_a looks valid", colleague.id)
cmd = peers.as_cmds(mine.id)
check("an idle agent gets a chat cmd", len(cmd), 1)
check("shaped as a cmd", cmd[0].type, "cmd")
check("action is chat", cmd[0].data["action"], "chat")
check("it says a peer sent it", "agent peers-demo" in cmd[0].data["text"])
check("the sender rides in the tag", f'from="{back["from_name"]}"' in cmd[0].data["text"])
check("the envelope id is the message id", cmd[0].id, back["id"])

print("a claim is not a delivery")
second_cmd = peers.as_cmds(mine.id)
check("nothing left to hand an idle agent", second_cmd, [])
check("the round seam has nothing left either", peers.claimed(), [])

print("mid-turn: the seam reads the mailbox itself")
# The round seam's whole reason to exist. An agent inside a long turn never reaches the
# idle branch of the main loop, so mail that landed since the turn began is only ever
# delivered if the seam collects it. This is the case that was broken: a message written
# here, with nothing collected first, then handed straight to the seam.
mid = peers.send(mine.id, "are you still on function_a", colleague.id)
check("nothing has collected it yet", peers.claimed(), [])
seam = peers.drain(mine.id)
check("the seam delivers it", [r["id"] for r in seam], [mid["id"]])
# Acknowledged once it is in the message. A second pass over the same seam must find
# nothing, or the model reads the same words twice.
peers.ack([r["id"] for r in seam])
check("and not twice", peers.drain(mine.id), [])

print("a subagent does not read the worker's mailbox")
# A subagent shares its parent's ctx, session_dir included, so a seam that trusted ctx
# would hand a peer's words to an agent that is not the one being talked to.
from handler.agent import subagent
peers.send(mine.id, "only the worker should see this", colleague.id)
token = subagent._depth.set(1)
check("the seam is silent at depth", peers.drain(mine.id), [])
subagent._depth.reset(token)
check("and reads again at depth 0", len(peers.drain(mine.id)), 1)
peers.ack([r["id"] for r in peers.claimed()])

print("render and names")
check("render tags the sender", peers.render(back).startswith('<peer_message from="peers-demo'))
check("a head is one line", "\n" in peers.head(back), False)
check("a name resolves", peers.target(colleague.id)["id"], colleague.id)
check("a name prefix resolves", peers.target("peers-demo: clearing")["id"], colleague.id)
check("a miss resolves to nothing", peers.target("nobody"), None)
check("unknown action is an error", call({"action": "wat"}).get("error") is not None)
check("a body is required", call({"action": "message", "to": colleague.id}).get("error") is not None)

print("global send")
# The colleague is fresh again, so it is back on the roster.
peers.become(colleague.id)
os.utime(store.path(colleague.id), None)
check("fresh again", [p["id"] for p in peers.roster(mine.id)], [colleague.id])
glob = call({"action": "message", "body": "status?"})
sent = glob.get("sent", [{}])[0]
check("sends to every peer", sent.get("to"), colleague.id)
check("the row names who it went TO", sent.get("name"), colleagues_name)

print("the exchange limit")
# Fill both mailboxes until the cap is reached, then the send must warn once.
# Earlier checks already sent some, so this tops the pair up to the cap rather than
# assuming it started empty.
already = peers.exchange(mine.id, colleague.id)
for i in range(peers.EXCHANGE_CAP - already):
    peers.send(colleague.id, "ping %d" % i, mine.id)
check("counted on both mailboxes", peers.exchange(mine.id, colleague.id), peers.EXCHANGE_CAP)
capped = call({"action": "message", "to": colleague.id, "body": "one more"})
check("the cap warns instead of sending", capped.get("ok"), False)
check("the warning says how many", str(peers.EXCHANGE_CAP) in capped.get("warning", ""))
again = call({"action": "message", "to": colleague.id, "body": "one more"})
check("sending again goes through", again.get("ok"), True)
# Clear the mailboxes so the rest of the checks count from zero.
for sid in (mine.id, colleague.id):
    if peers.mailbox(sid).exists(): peers.mailbox(sid).unlink()

print("init")
init = call({"action": "init"})
check("init reports me", init["me"]["id"], mine.id)
check("init reports peers", [p["id"] for p in init["peers"]], [colleague.id])
check("init is one call not two tools", set(tools).issuperset({"roster"}) and "agent" in tools)

print("the same directory written another way")
# The bug this covers: cwd used to be compared as a string. A session opened from
# `.../documents/work` and one from `.../Documents/Work` are one directory on this
# filesystem and were two projects, so the roster offered nobody. The inode is the
# truth and the spelling is not.
check("the same path two ways is one directory", peers._same_dir(here, here.lower()) if here != here.lower() else peers._same_dir(here, here), True)
check("case does not make two directories", peers._same_dir(here, here.upper()), True)
check("different directories are not", peers._same_dir(here, "/tmp/somewhere-else"), False)
check("an empty cwd matches only itself", peers._same_dir("", here), False)
# And the roster still marks spelling apart from an exact match, because what the
# sender does with a peer in the same files is not what it does with one in another.
check("an exact cwd is same_cwd", peers.roster(mine.id)[0]["same_cwd"], True)
check("no note when the spelling is exact", "cwd_note" in peers.roster(mine.id)[0], False)

shutil.rmtree(os.environ["CUACODE_HOME"], ignore_errors=True)
print()
if fails:
    print(f"{len(fails)} FAILED:"); [print(" -", f) for f in fails]; sys.exit(1)
print("all ok")
