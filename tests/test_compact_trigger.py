#!/usr/bin/env python3
"""Where a conversation folds when one model's window is not like the others.

`compact` is the global default; `model_params[<model>].compact` is that model's own
answer, so a 300k model behind a bridge that publishes no window need not fold at the
180k default meant for a smaller one.

    python3 tests/test_compact_trigger.py
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from handler import config
from handler.agent import compact

FAILS = []
MODEL = "deepseek-v4.1-flash-free"
SETTINGS = {"provider": "opencode", "model": MODEL}


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  -> {detail}" if not cond and detail else ""))
    if not cond: FAILS.append(name)


cfg = {
    "compact": 180000.0,
    "providers": {"opencode": {"model_params": {
        MODEL: {"context_window": 300000, "compact": 295000}}}},
}
check("the model's own count wins", compact.trigger(SETTINGS, cfg) == 295000,
      compact.trigger(SETTINGS, cfg))
check("another model keeps the global count",
      compact.trigger({"provider": "openai", "model": "gpt-x"}, cfg) == 180000)
check("the model's own window is reported",
      config.context_window("opencode", MODEL, cfg) == 300000)
check("neither local key is ever bound for the wire",
      "compact" not in config.params_for("opencode", MODEL, cfg)
      and "context_window" not in config.params_for("opencode", MODEL, cfg))

# A window with no count still folds at the ratio that window deserves.
window_only = {"providers": {"opencode": {"model_params": {MODEL: {"context_window": 300000}}}}}
check("no count falls back to the window's ratio",
      compact.trigger(SETTINGS, window_only) == int(300000 * compact.ratio(300000)),
      compact.trigger(SETTINGS, window_only))

# Zero means never, and a per-model zero must not be read as "unset".
off = {"compact": 180000.0, "providers": {"opencode": {"model_params": {MODEL: {"compact": 0}}}}}
check("a per-model zero turns folding off", compact.trigger(SETTINGS, off) == 0)

# An unset model resolves through the provider, so a per-model count still applies.
resolved = {"providers": {"opencode": {
    "model": MODEL, "model_params": {MODEL: {"compact": 295000}}}}}
check("an unset model still finds its own count",
      compact.trigger({"provider": "opencode"}, resolved) == 295000)

print()
print("failed:", len(FAILS))
raise SystemExit(1 if FAILS else 0)
