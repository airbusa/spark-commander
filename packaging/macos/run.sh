#!/bin/sh
# cargo's runner on macOS (.cargo/config.toml): `cargo run` starts the app
# from a bundle beside it, so the Dock names it as the release, not after the
# executable. Other programs and tests run as they are. Each new build is
# signed with this Mac's development certificate, where it has one: the
# keychain's "Always Allow" and the privacy permissions then hold across
# builds, as they hold an app's identifier and certificate; the linker's ad
# hoc signature is a new one with each build, which they ask for again.
case "$(basename "$1")" in
sparkcommander) ;;
*) exec "$@" ;;
esac
exe=$1
shift
app="$(dirname "$exe")/Spark Commander.app"
contents="$app/Contents"
mkdir -p "$contents/MacOS" "$contents/Resources"
# The one there stays, signed, until a new build.
if [ ! -e "$contents/MacOS/sparkcommander" ] || [ "$exe" -nt "$contents/MacOS/sparkcommander" ]; then
    ln -f "$exe" "$contents/MacOS/sparkcommander"
fi
cp "$(dirname "$0")/SparkCommander.icns" "$contents/Resources/"
# Its own identifier: Launch Services keeps it apart from an installed release.
cat > "$contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDisplayName</key>
    <string>Spark Commander</string>
    <key>CFBundleExecutable</key>
    <string>sparkcommander</string>
    <key>CFBundleIconFile</key>
    <string>SparkCommander</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.airbusa.sparkcommander.dev</string>
    <key>CFBundleName</key>
    <string>Spark Commander</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
EOF
if codesign -dv "$contents/MacOS/sparkcommander" 2>&1 | grep -q '^Signature=adhoc'; then
    identity=$(security find-identity -v -p codesigning | awk -F'"' '/Apple Development|Developer ID Application/ { print $2; exit }')
    [ -z "$identity" ] || codesign --force --sign "$identity" "$app" 2>/dev/null
fi
exec "$contents/MacOS/sparkcommander" "$@"
