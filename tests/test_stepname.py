"""What the step namer is given, and what the todo note's row says.

Both are pure strings, so this runs the functions rather than a model: a name is
only as good as the call it was handed, and the failure that mattered was `shell`
arriving with no command on it at all.
"""
import json
import os
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from handler.agent import stepname


def test_heads_keeps_the_object_of_a_call():
    # A long command used to be dropped for being long, which left a bare `shell`.
    cmd = "cd /Users/x/work/thing && cargo check -p cuacode-quik 2>&1 | tail -20"
    assert stepname.heads([{"name": "shell", "args": {"command": cmd, "timeout": 30}}]) == (
        "shell(command=cargo check -p cuacode-quik 2>&1 | tail -20)")

    # The action is the verb, not the object: `file(read)` named nothing.
    got = stepname.heads([{"name": "file",
                           "args": {"action": "edit", "path": "/a/b/src/view.rs", "edits": "x" * 400}}])
    assert got == "file(path=src/view.rs, action=edit)", got
    assert "xxxx" not in got, "a body is not what the step was for"

    assert stepname.heads([{"name": "app_list", "args": {}}]) == "app_list"
    assert stepname.heads([{"name": "memory", "args": {
        "action": "write", "name": "quik-polish", "body": "y" * 900,
        "description": "the shimmer rules"}}]) == (
        "memory(name=quik-polish, description=the shimmer rules, action=write)")


def test_excerpt_keeps_the_decision_at_the_end():
    thought = "Let me look at the loader. " + ("filler " * 400) + "so the fix is the wrap guard."
    got = stepname.excerpt(thought)
    assert got.startswith("Let me look at the loader.")
    assert got.endswith("so the fix is the wrap guard."), got[-40:]
    assert "[...]" in got
    assert len(got) < len(thought) / 2


def test_clean_is_one_head_line():
    assert stepname.clean('  "Tracing the router."  ') == "Tracing the router"
    long = stepname.clean("Sizing the three frontends against the same wrap " + "x" * 40)
    assert long.endswith("\u2026") and len(long) <= stepname.MAX_NAME


def test_the_todo_note_is_a_short_single_line():
    from handler.agent.main import _todo_note
    d = tempfile.mkdtemp()
    list_ = {"items": [
        {"id": 1, "text": "read the loader", "state": "done"},
        {"id": 2, "text": "Improve the namer's input (arg choice, thought head+tail, recent names)",
         "state": "in_progress"},
        {"id": 3, "text": "verify", "state": "pending"},
    ], "next_id": 4, "updated": ""}
    with open(os.path.join(d, "todo.json"), "w") as f:
        json.dump(list_, f)
    note = _todo_note({"session_dir": d})
    head, block = note.split("\n\n", 1)
    assert head == "todo: 1/3 done, 2 open, in progress: Improve the namer's input (arg choice\u2026", head
    assert len(head) <= 80, "the head is a row, not a paragraph"
    assert "<todo_status>" in block and "</todo_status>" in block


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)
