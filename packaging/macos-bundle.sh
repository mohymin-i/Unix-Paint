#!/bin/sh
# Build Paint.app for macOS (run on a Mac with Rust and the Xcode command line tools).
set -e
cd "$(dirname "$0")/.."
cargo build --release
APP=target/Paint.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/unix-paint "$APP/Contents/MacOS/unix-paint"
./target/release/unix-paint --export-icon target/icon.png
ICONSET=target/Paint.iconset
mkdir -p "$ICONSET"
for s in 16 32 128 256; do
  sips -z $s $s target/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2)); sips -z $d $d target/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Paint.icns"
VERSION=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Paint</string>
  <key>CFBundleDisplayName</key><string>Paint</string>
  <key>CFBundleIdentifier</key><string>org.unixpaint.Paint</string>
  <key>CFBundleExecutable</key><string>unix-paint</string>
  <key>CFBundleIconFile</key><string>Paint</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>CFBundleDocumentTypes</key><array><dict>
    <key>CFBundleTypeRole</key><string>Editor</string>
    <key>LSItemContentTypes</key><array>
      <string>public.png</string><string>public.jpeg</string><string>com.microsoft.bmp</string>
      <string>com.compuserve.gif</string><string>public.tiff</string><string>org.webmproject.webp</string>
    </array>
  </dict></array>
</dict></plist>
PLIST
echo "Built $APP"
