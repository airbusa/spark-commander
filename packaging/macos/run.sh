#!/bin/sh
# cargo's runner on macOS (.cargo/config.toml): `cargo run` starts the app
# from a bundle beside it, so the Dock names it as the release, not after the
# executable. Other programs and tests run as they are.
case "$(basename "$1")" in
sparkcommander) ;;
*) exec "$@" ;;
esac
exe=$1
shift
contents="$(dirname "$exe")/Spark Commander.app/Contents"
mkdir -p "$contents/MacOS" "$contents/Resources"
ln -f "$exe" "$contents/MacOS/sparkcommander"
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
exec "$contents/MacOS/sparkcommander" "$@"
