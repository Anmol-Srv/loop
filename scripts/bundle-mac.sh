#!/usr/bin/env bash
# Wraps the release binary in a real .app so it double-clicks, gets a dock
# icon, and behaves like a Mac application instead of a terminal program.
set -euo pipefail
cd "$(dirname "$0")/.."

NAME="Airtribe Control Plane"
APP="target/$NAME.app"

cargo build --release --features app --bin acp-app

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/acp-app "$APP/Contents/MacOS/acp-app"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>live.airtribe.controlplane</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>acp-app</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <!-- A window app, not a background agent. -->
  <key>LSUIElement</key><false/>
</dict>
</plist>
PLIST

# A generated icon beats the blank default and costs nothing.
ICONSET=$(mktemp -d)/icon.iconset
mkdir -p "$ICONSET"
python3 - "$ICONSET" <<'PY'
import subprocess, sys, pathlib
out = pathlib.Path(sys.argv[1])
svg = '''<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024">
<rect width="1024" height="1024" rx="228" fill="#1C1B19"/>
<circle cx="512" cy="512" r="250" fill="none" stroke="#2F5CE5" stroke-width="56"/>
<circle cx="512" cy="262" r="74" fill="#6A3DC4"/>
</svg>'''
src = out.parent / "icon.svg"
src.write_text(svg)
for size in (16, 32, 64, 128, 256, 512, 1024):
    for scale, suffix in ((1, ""), (2, "@2x")):
        px = size * scale
        if px > 1024:
            continue
        name = out / f"icon_{size}x{size}{suffix}.png"
        subprocess.run(["qlmanage", "-t", "-s", str(px), "-o", str(out), str(src)],
                       capture_output=True)
        produced = out / "icon.svg.png"
        if produced.exists():
            produced.rename(name)
PY
if command -v iconutil >/dev/null && ls "$ICONSET"/*.png >/dev/null 2>&1; then
  iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/icon.icns" 2>/dev/null || true
fi

# Ad-hoc signature: without it macOS quarantines the unsigned bundle and the
# keychain entry is scoped to an unstable identity.
codesign --force --deep --sign - "$APP" 2>/dev/null || \
  echo "  (codesign unavailable; the app still runs)"

echo
echo "built  $APP"
echo "run    open '$APP'"
