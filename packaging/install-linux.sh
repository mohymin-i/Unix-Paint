#!/bin/sh
# Install Paint for the current user (or system-wide with PREFIX=/usr/local, run as root).
set -e
cd "$(dirname "$0")/.."
PREFIX="${PREFIX:-$HOME/.local}"
cargo build --release
install -Dm755 target/release/unix-paint "$PREFIX/bin/unix-paint"
# Point the launcher at the installed binary so it works even when $PREFIX/bin isn't on PATH.
mkdir -p "$PREFIX/share/applications" "$PREFIX/share/icons/hicolor/256x256/apps"
sed "s|^Exec=unix-paint|Exec=\"$PREFIX/bin/unix-paint\"|" packaging/unix-paint.desktop > "$PREFIX/share/applications/unix-paint.desktop"
chmod 644 "$PREFIX/share/applications/unix-paint.desktop"
./target/release/unix-paint --export-icon "$PREFIX/share/icons/hicolor/256x256/apps/unix-paint.png"
if command -v update-desktop-database >/dev/null; then update-desktop-database "$PREFIX/share/applications" || true; fi
if command -v gtk-update-icon-cache >/dev/null; then gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" || true; fi
echo "Installed to $PREFIX (make sure $PREFIX/bin is on your PATH)."
