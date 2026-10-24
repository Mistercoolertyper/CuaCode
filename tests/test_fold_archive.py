#!/usr/bin/env python3
"""A fold archives what it replaced, and says so on the session.

The thing being pinned: compaction rewrites messages.jsonl. If that is a plain
overwrite then a fold silently destroys the conversation it folded -- and a bad
summary is unrecoverable. So the previous list is written whole to
`folds/NN.jsonl` before anything is replaced, and meta.json gains a `compactions`
entry naming that archive.

This runs against the real Session class on a temp directory, so it is the actual
write path and not a stub.

    python3 tests/test_fold_archive.py
"""
import json
import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

FAILS = []


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond else ""))
    if not cond: FAILS.append(name)


home = Path(tempfile.mkdtemp(prefix="cuacode-fold-"))
os.environ["CUACODE_HOME"] = str(home)

from handler.session.main import Session
from handler.session import store


def conversation(n):
    """n rounds of a conversation, in the record shape the worker writes."""
    recs = [{"t": "user", "ts": store.now_iso(), "text": "the original request"}]
    for i in range(n):
        recs.append({"t": "assistant", "ts": store.now_iso(), "content": "step %d" % i,
                     "calls": [{"name": "shell", "args": {"command": "ls"}}]})
        recs.append({"t": "tool", "ts": store.now_iso(), "name": "shell",
                     "result": {"result": {"stdout": "file%d" % i}}})
    return recs


sess = Session.create(provider="ollama", model="deepseek-v4.1-flash", effort_level="high")
sess.add_user("the original request")
for i in range(20):
    sess.add_assistant("step %d" % i, "", [{"name": "shell", "args": {"command": "ls"}}])
    sess.add_tool("shell", {"result": {"stdout": "file%d" % i}})
sess.commit()

before = sess.records()
check("there is a conversation to fold", len(before) > 30, len(before))
on_disk_before = store.read_jsonl(sess.dir / "messages.jsonl")
check("and it is on disk", len(on_disk_before) == len(before), len(on_disk_before))

# Fold: everything after the first record becomes one summary.
folded = [before[0], {"t": "compact", "ts": store.now_iso(), "text": "a summary of the middle",
                      "folded": len(before) - 2, "freed": 4000}]
sess.rewrite(folded, fold={"folded": len(before) - 2, "freed": 4000, "summary": "a summary of the middle"})

# 1. The new history is the folded one.
after = store.read_jsonl(sess.dir / "messages.jsonl")
check("messages.jsonl now holds the folded list", len(after) == 2, len(after))
check("and it contains the summary", after[1].get("t") == "compact", after[1].get("t"))

# 2. Nothing was destroyed: the archive holds every original record, in order.
archives = sorted(sess.folds_dir.glob("*.jsonl")) if sess.folds_dir.exists() else []
check("an archive was written", len(archives) == 1, archives)
saved = store.read_jsonl(archives[0]) if archives else []
check("the archive holds every record that was folded away", len(saved) == len(before),
      (len(saved), len(before)))
check("byte for byte, in the same order", saved == before,
      "the archive is not the original conversation")
check("including the opening request",
      any(r.get("text") == "the original request" for r in saved))

# 3. The session says so on its own meta, which is what a person can read.
meta = store.read_json(sess.dir / "meta.json")
check("meta records how many folds", meta.get("compacted") == 1, meta.get("compacted"))
entry = (meta.get("compactions") or [{}])[0]
check("with the counts and what it freed",
      entry.get("folded") == len(before) - 2 and entry.get("freed") == 4000, entry)
check("and where the original went", entry.get("archive") == str(archives[0].relative_to(sess.dir)),
      entry.get("archive"))
check("and how the conversation shrank",
      (entry.get("records_before"), entry.get("records_after")) == (len(before), 2), entry)

# 4. A second fold archives again rather than overwriting the first.
mid = sess.records()
sess.rewrite(mid[:1], fold={"folded": 1, "freed": 100, "summary": "second"})
archives2 = sorted(sess.folds_dir.glob("*.jsonl"))
check("a second fold gets its own archive", len(archives2) == 2, archives2)
meta2 = store.read_json(sess.dir / "meta.json")
check("and its own meta entry", len(meta2.get("compactions") or []) == 2, meta2.get("compactions"))
check("the new one is numbered", [e.get("n") for e in meta2["compactions"]] == [1, 2],
      [e.get("n") for e in meta2["compactions"]])

# 5. Opening the session after a fold still replays. The summary record is what
#    says so to the model, and a second fold that swallows it leaves only the
#    session's own meta as the record -- which is why the meta entry exists.
msgs = sess.messages()
check("the folded session still replays", len(msgs) >= 1, len(msgs))
meta3 = store.read_json(sess.dir / "meta.json")
check("the session still says it was folded, whatever the replay holds",
      meta3.get("compacted") == 2 and len(meta3.get("compactions") or []) == 2,
      meta3.get("compacted"))

# A single fold, on its own, does reach the model: the summary is a user turn
# wrapped in a tag, so a reopened conversation is told it is working on a summary.
one = Session.create(provider="ollama", model="m", effort_level="high")
one.add_user("first")
for i in range(3):
    one.add_assistant("a%d" % i, "", [])
    one.add_tool("shell", {"result": {}})
one.commit()
kept = one.records()
one.rewrite([kept[0], {"t": "compact", "ts": store.now_iso(), "text": "SUMMARY HERE"}],
            fold={"folded": 6, "freed": 900, "summary": "SUMMARY HERE"})
joined = json.dumps(one.messages())
check("a fold is visible to a reopened conversation", "SUMMARY HERE" in joined, joined[:300])
check("and is marked as a fold, not as something the person said",
      "<compacted>" in joined, joined[:300])

print()
if FAILS:
    print("FAILED: " + ", ".join(FAILS))
    sys.exit(1)
print("all fold archive checks passed")
