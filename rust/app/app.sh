#!/usr/bin/env bash
# Build the app as a real macOS bundle: one you can double-click, that shows up in
# the Dock under its own name, gets a menu bar, and cannot run twice.
#
#   ./app.sh                 build the bundle into ~/Applications
#   ./app.sh --open          build it and open it
#
# Why a bundle at all: a bare binary is not an app to macOS. It has no
# CFBundleName, so the menu bar shows the path it was exec'd as; it cannot own a
# menu bar properly; it has no Dock identity; and nothing stops a second copy
# running beside the first.
#
# What this app does NOT need, unlike a frontend that runs workers: a login
# environment, or a recorded path back to the Python side. It reads
# `~/.cuacode` itself and runs no process.
set -euo pipefail
cd "$(dirname "$0")"        # rust/app, where this script lives
TREE_ROOT="$(cd ../.. && pwd)"   # the tree that holds main.py, baked into the wrapper

NAME="cuacode"
BUNDLE="cuacode.app"
DEST="${CUACODE_DEST:-$HOME/Applications}"
BIN="../target/release/$NAME"

say() { printf '\033[1m==>\033[0m %s\n' "$1"; }
die() { printf '\033[31merror:\033[0m %s\n' "$1" >&2; exit 1; }

[ -x "$BIN" ] || die "no binary at $BIN -- run ./build.sh cuacode first"

say "building $BUNDLE"
rm -rf "$BUNDLE"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
# The real binary keeps its own name; the wrapper below is what
# CFBundleExecutable names and what execs this.
cp "$BIN" "$BUNDLE/Contents/MacOS/$NAME-bin"

# One instance, ever. The lock file is the real check, so two launches racing
# each other cannot both win; macOS apps are single instances by convention, and
# a second launch is a request to look at the window that already exists.
cat > "$BUNDLE/Contents/MacOS/$NAME" <<'WRAP'
#!/bin/zsh
set -u
HERE="${0:A:h}"
LAUNCH="$HERE/cuacode-bin"

LOCK="${TMPDIR:-/tmp}/cuacode-app-$USER.lock"
if [ -f "$LOCK" ]; then
    OLD="$(cat "$LOCK" 2>/dev/null || true)"
    if [ -n "$OLD" ] && kill -0 "$OLD" 2>/dev/null; then
        osascript -e 'tell application "System Events" to set frontmost of first process whose unix id is '"$OLD"' to true' 2>/dev/null || true
        exit 0
    fi
fi
echo $$ > "$LOCK"

# The interpreter the worker runs on, pinned here rather than left to PATH.
#
# This wrapper is the one piece of the app that runs *from the user's own shell*,
# so it is the last place that can see the python the user actually installed: a
# bundle launched by Finder or `open` inherits launchd's environment, whose PATH
# is `/usr/bin:/bin:/usr/sbin:/sbin`, and `/usr/bin/python3` is the system's stub
# with none of the worker's dependencies.
#
# So it is asked of the **login shell**, and not of PATH. Reading PATH here looks
# equivalent and is not: started from a terminal it finds the user's python, and
# started from Finder it finds the stub, which is the same wrapper answering two
# different questions depending on who launched it. The stub gets through
# argparse and dies at the first real import - `tools/loader.py`'s bare
# `import yaml` - and the app reports it as "No module named 'yaml'", which reads
# as a broken worker rather than as a launcher that handed it the wrong python.
# A login shell reads the user's own profile and can see the one they installed.
if [ -z "${CUACODE_PYTHON:-}" ]; then
    if [ -x /bin/zsh ]; then
        PY="$(/bin/zsh -lic 'command -v python3' 2>/dev/null || true)"
    elif [ -x /bin/bash ]; then
        PY="$(/bin/bash -lc 'command -v python3' 2>/dev/null || true)"
    fi
    # A shell that answered nothing is not a machine without python: fall back to
    # PATH, so this is the worse answer and not a missing one.
    if [ -z "${PY:-}" ]; then
        PY="$(command -v python3 2>/dev/null || true)"
    fi
    if [ -n "$PY" ] && [ -x "$PY" ]; then
        CUACODE_PYTHON="$PY"
        export CUACODE_PYTHON
        # ...and its directory on PATH, because the worker shells out to the
        # tools' own programs and resolves them the same way.
        PATH="$(dirname "$PY"):$PATH"
        export PATH
    fi
fi

# The worker, beside the tree this app was built from: a bundle carries no
# python of its own, and the crate lives at rust/app under the root that holds
# main.py. Baked in here rather than walked to at runtime, because the bundle is
# copied to ~/Applications and has no way back to its source from there.
TREE="__TREE__"
if [ -f "$TREE/main.py" ] && [ -z "${CUACODE_WORKER:-}" ]; then
    CUACODE_WORKER="$TREE/main.py"
    export CUACODE_WORKER
fi

exec "$LAUNCH" "$@"
WRAP
chmod +x "$BUNDLE/Contents/MacOS/$NAME" "$BUNDLE/Contents/MacOS/$NAME-bin"
# The wrapper is written from a quoted heredoc, so `__TREE__` survived verbatim:
# this is what fills it in, with the tree the bundle was built from.
sed -i '' "s|__TREE__|$TREE_ROOT|" "$BUNDLE/Contents/MacOS/$NAME"

# CFBundleName is what the menu bar and the Dock read. `cuacode` and not the
# crate's name: this is the app the user opens.
cat > "$BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>cuacode</string>
    <key>CFBundleDisplayName</key><string>CuaCode</string>
    <key>CFBundleExecutable</key><string>cuacode</string>
    <key>CFBundleIdentifier</key><string>sh.cuacode.app</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSRequiresAquaSystemAppearance</key><false/>
</dict>
</plist>
PLIST

# Ad-hoc signed, because an unsigned bundle is a bundle macOS is suspicious of.
# Library validation off is what lets a bundle-less gpui link work from here.
codesign --force --sign - --timestamp=none "$BUNDLE" 2>/dev/null \
    || say "note: codesign failed; the app will still run from the terminal"

mkdir -p "$DEST"
rm -rf "$DEST/$BUNDLE"
cp -R "$BUNDLE" "$DEST/$BUNDLE"
say "installed $DEST/$BUNDLE"

if [ "${1:-}" = "--open" ]; then
    say "opening"
    open "$DEST/$BUNDLE"
fi
