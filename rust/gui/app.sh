#!/usr/bin/env bash
# Cuacode.app: the window as a real macOS application.
#
#   ./app.sh              rebuild the bundle (no compile) and leave it
#   ./app.sh --build      compile first, then refresh the bundle
#   ./app.sh --open       ...and open it
#   ./app.sh --release    use (and with --build, build) the release binary
#
# The bundle's executable is a symlink to `rust/target/<profile>/cuacode`, not a
# copy: the application IS the latest build, so nothing has to remember to
# reinstall it after a compile. What a bundle buys over running the binary
# directly is what macOS keys off CFBundleName — the menu bar's name
# (otherwise the path it was exec'd as), a Dock identity, and one instance.
set -euo pipefail
cd "$(dirname "$0")"

NAME="Cuacode"
BUNDLE_NAME="$NAME.app"
DEST="${CUACODE_DEST:-$HOME/Applications}"
PROFILE="debug"
BUILD=0
OPEN=0
for arg in "$@"; do
    case "$arg" in
        --build) BUILD=1 ;;
        --open) OPEN=1 ;;
        --release) PROFILE="release" ;;
        *) printf 'unknown argument: %s\n' "$arg" >&2; exit 2 ;;
    esac
done

BIN="$(cd ../target/$PROFILE && pwd)/cuacode"

if [ "$BUILD" = 1 ]; then
    printf '\033[1m==>\033[0m cargo build -p cuacode-gui (%s)\n' "$PROFILE"
    if [ "$PROFILE" = release ]; then
        cargo build -p cuacode-gui --release
    else
        cargo build -p cuacode-gui
    fi
fi
[ -x "$BIN" ] || { printf 'error: no binary at %s (run with --build)\n' "$BIN" >&2; exit 1; }

BUNDLE="$DEST/$BUNDLE_NAME"
printf '\033[1m==>\033[0m %s -> %s\n' "$BUNDLE" "$BIN"
rm -r "$BUNDLE" 2>/dev/null || true
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
ln -s "$BIN" "$BUNDLE/Contents/MacOS/cuacode"

VERSION="$(grep -m1 '^version' "$PWD/Cargo.toml" | cut -d'"' -f2)"
cat > "$BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$NAME</string>
    <key>CFBundleDisplayName</key><string>$NAME</string>
    <key>CFBundleExecutable</key><string>cuacode</string>
    <key>CFBundleIdentifier</key><string>sh.cuacode.gui</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST
printf 'APPL????' > "$BUNDLE/Contents/PkgInfo"

if [ "$OPEN" = 1 ]; then
    open "$BUNDLE"
fi
