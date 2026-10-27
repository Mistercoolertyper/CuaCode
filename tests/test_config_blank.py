"""The config file a person opens: every option, and nothing reinterpreted.

`load()` writes the whole template on first run and fills a file that already
exists, so a setting nobody knew about does not stay invisible. What it must never
do is change what a value means, which is what the second check is about.
"""
import json
import os
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
os.environ["CUACODE_HOME"] = tempfile.mkdtemp(prefix="config-check-")

from handler import config


def test_a_fresh_file_shows_every_option():
    cfg = config.load()
    for key in ("active", "effort", "vision", "steps", "permission", "compact",
                "always_skills", "providers"):
        assert key in cfg, f"a fresh config does not show {key}"
    assert cfg["permission"]["mode"] == "ask"
    assert cfg["steps"] == {"provider": "", "model": ""}
    assert cfg["compact"] is None
    assert set(cfg["providers"]) == set(config.providers.PROVIDERS)
    for name, entry in cfg["providers"].items():
        assert entry == {"model": "", "api_key": "", "params": {}, "model_params": {}}, name
        # Never the eye flag: it is read instead of the model's own capabilities, so a
        # seeded true would claim a blind model can see.
        assert "vision" not in entry, name
    assert json.load(open(config.path())) == cfg, "the file is not what load() returns"


def test_an_existing_file_keeps_its_meaning():
    cfg = {"active": "anthropic", "permission": "bypass",
           "providers": {"anthropic": {"model": "claude", "api_key": "k"}}}
    json.dump(cfg, open(config.path(), "w"))
    got = config.load()
    assert got["active"] == "anthropic"
    assert config.permission_mode(got) == "bypass", "a bare mode was reinterpreted"
    assert got["permission"] == {"mode": "bypass", "provider": "", "model": ""}
    assert got["providers"]["anthropic"]["model"] == "claude", "a set value was overwritten"
    assert got["providers"]["anthropic"]["params"] == {}, "an entry did not gain its blanks"
    assert config.steps_helper(got) == config.vision_helper(got), "steps stopped falling back"
    # Settled: a second load writes nothing, or every read is a write.
    stamp = os.path.getmtime(config.path())
    config.load()
    assert os.path.getmtime(config.path()) == stamp


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)
