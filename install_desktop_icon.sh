#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ICON_SRC="$SCRIPT_DIR/assets/icon.png"

# Target directories
ICON_DIR_512="$HOME/.local/share/icons/hicolor/512x512/apps"
ICON_DIR_BASE="$HOME/.local/share/icons"
APPS_DIR="$HOME/.local/share/applications"

mkdir -p "$ICON_DIR_512" "$ICON_DIR_BASE" "$APPS_DIR"

if [ -f "$ICON_SRC" ]; then
    cp "$ICON_SRC" "$ICON_DIR_512/server_launcher.png"
    cp "$ICON_SRC" "$ICON_DIR_BASE/server_launcher.png"
    echo "✓ Installed application icon to $ICON_DIR_512/server_launcher.png and $ICON_DIR_BASE/server_launcher.png"
fi

# Detect release or debug binary
BIN_PATH="$SCRIPT_DIR/server_launcher_rust/target/release/server_launcher"
if [ ! -f "$BIN_PATH" ]; then
    BIN_PATH="$SCRIPT_DIR/server_launcher_rust/target/debug/server_launcher"
fi

cat <<EOF > "$APPS_DIR/server_launcher.desktop"
[Desktop Entry]
Version=1.0
Type=Application
Name=Server Launcher
Comment=Development Server Launcher & Monitor
Exec="$BIN_PATH"
Icon=server_launcher
Terminal=false
Categories=Development;Utility;
StartupWMClass=server_launcher
StartupNotify=true
EOF

chmod +x "$APPS_DIR/server_launcher.desktop"
echo "✓ Installed desktop entry to $APPS_DIR/server_launcher.desktop"

# Refresh desktop database and icon cache if available
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APPS_DIR" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
fi

echo "✓ Application icon setup complete."

