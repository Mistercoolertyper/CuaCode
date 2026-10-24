#!/usr/bin/env bash
# Run one frontend straight from source.
#   ./run.sh            list frontends
#   ./run.sh fastrs     run the terminal frontend
#   ./run.sh fastrs     run the rust crate's fastrs, building it if it is stale
set -euo pipefail

launch_dir="$PWD"      # where you ran this from, before either cd below
cd "$(dirname "$0")"

# The Rust frontends are the [[bin]] targets of the crates under rust/. Read
# from the manifests rather than listed here too, so adding one is a single edit.
rust_frontends() {
    # One "dir:bin" per frontend: the crate directory relative to the repo
    # root, and the binary that crate builds. Read from the manifests rather
    # than listed here, so adding a frontend is a single file.
    local m d b
    for m in rust/Cargo.toml rust/*/Cargo.toml; do
        [ -f "$m" ] || continue
        d="${m%/Cargo.toml}"
        for b in $(awk '/^\[\[bin\]\]/{f=1;next} /^\[/{f=0} f && /^name/{print $3}' "$m" | tr -d '"'); do
            echo "$d:$b"
        done
    done
}

# The crate a frontend belongs to, or nothing.
rust_bin_dir() {
    # No early `exit` in the awk: it would close the pipe while rust_frontends
    # still had a line to write, SIGPIPE on that write, and `pipefail` plus
    # `set -e` would then end the script silently before it runs anything.
    rust_frontends | awk -F: -v want="$1" '$2 == want && !seen { print $1; seen = 1 }'
}

# Tripwire for a fresh clone: the worker's dependencies must exist before any
# frontend is started, and a missing install should announce itself at the
# front door instead of as a stack trace from inside the worker. One import
# checks, install.sh is the only thing that fixes it, and the sentinel keeps
# the check off every later launch (touching requirements.txt brings it back).
sentinel="bin/.deps-ok"
if [ ! -f "$sentinel" ] || [ requirements.txt -nt "$sentinel" ]; then
    if ! "${CUACODE_PYTHON:-python3}" -c 'import openai' >/dev/null 2>&1; then
        ./install.sh || exit 1
    fi
    mkdir -p bin && touch "$sentinel"
fi

if [ $# -eq 0 ]; then
    echo "usage: ./run.sh <frontend> [args...]   anything else goes to the frontend"
    echo "       ./run.sh <frontend> --resume     carry on with an earlier conversation"
    echo "       ./run.sh --usage [--days N] [--json]"
    echo "frontends:"
    rust_frontends | awk -F: '{ printf "  %s  (%s)\n", $2, $1 }'

    exit 1
fi

name="$1"
shift

# Not a frontend: a question about the app rather than a conversation with it, so it
# never starts a session of its own to answer. usage.py owns this, not the worker.
if [ "$name" = "--usage" ] || [ "$name" = "usage" ]; then
    py="${CUACODE_PYTHON:-}"
    for cand in venv/bin/python3 venv/bin/python .venv/bin/python3 .venv/bin/python; do
        [ -n "$py" ] && break
        [ -x "$cand" ] && py="$cand"
    done
    exec "${py:-python3}" -m handler.usage "$@"
fi

# Frontends locate main.py by walking up from cwd; pin it so the worker is
# always this checkout's, whatever directory you launched from.
if [ -n "${CUACODE_WORKER:-}" ] && [ "${CUACODE_WORKER:-}" != "$PWD/main.py" ]; then
    # An inherited value wins, which is right for a frontend run from a shell
    # that was itself started inside another checkout -- and a silent trap in
    # a tree whose whole point is its own Python side. Say it rather than
    # quietly driving somebody else's worker.
    echo "note: driving the worker at $CUACODE_WORKER, not this checkout's." >&2
    echo "      unset CUACODE_WORKER to use $PWD/main.py" >&2
fi
export CUACODE_WORKER="${CUACODE_WORKER:-$PWD/main.py}"

# A frontend exec'd from the tree would report the tree as the agent's cwd on
# every dev launch. Report the directory you were actually standing in instead.
export CUACODE_CWD="${CUACODE_CWD:-$launch_dir}"

crate="$(rust_bin_dir "$name")"
if [ -n "$crate" ]; then
    # Built on demand and exec'd from cargo's own output tree, so there is no
    # second copy in bin/ to go stale. Rebuilt only when the crate moved past
    # the binary: a cargo run with nothing to do still prints a line, and that
    # line lands on top of a fullscreen UI.
    bin="rust/target/release/$name"
    if [ ! -x "$bin" ] || [ -n "$(find "$crate/src" "$crate/Cargo.toml" -newer "$bin" -print -quit)" ]; then
        (cd "$crate" && cargo build --release) >&2
    fi
    exec "$bin" "$@"
fi

echo "error: no such frontend: $name" >&2
echo "       ./run.sh with no argument lists the ones in rust/" >&2
exit 1
