#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ICON_SRC="$SCRIPT_DIR/assets/icon.png"
ICON_96_SRC="$SCRIPT_DIR/assets/icon_96.png"

# Target directories
ICON_DIR_512="$HOME/.local/share/icons/hicolor/512x512/apps"
ICON_DIR_256="$HOME/.local/share/icons/hicolor/256x256/apps"
ICON_DIR_128="$HOME/.local/share/icons/hicolor/128x128/apps"
ICON_DIR_96="$HOME/.local/share/icons/hicolor/96x96/apps"
ICON_DIR_64="$HOME/.local/share/icons/hicolor/64x64/apps"
ICON_DIR_48="$HOME/.local/share/icons/hicolor/48x48/apps"
ICON_DIR_32="$HOME/.local/share/icons/hicolor/32x32/apps"
ICON_DIR_BASE="$HOME/.local/share/icons"
PIXMAPS_DIR="$HOME/.local/share/pixmaps"
APPS_DIR="$HOME/.local/share/applications"
DESKTOP_DIR="$HOME/Desktop"
LOCAL_BIN_DIR="$HOME/.local/bin"

mkdir -p "$ICON_DIR_512" "$ICON_DIR_256" "$ICON_DIR_128" "$ICON_DIR_96" "$ICON_DIR_64" "$ICON_DIR_48" "$ICON_DIR_32" "$ICON_DIR_BASE" "$PIXMAPS_DIR" "$APPS_DIR" "$LOCAL_BIN_DIR"

if [ -f "$ICON_SRC" ]; then
    cp "$ICON_SRC" "$ICON_DIR_512/server_launcher.png"
    cp "$ICON_SRC" "$ICON_DIR_256/server_launcher.png"
    cp "$ICON_SRC" "$ICON_DIR_128/server_launcher.png"
    cp "$ICON_SRC" "$ICON_DIR_64/server_launcher.png"
    cp "$ICON_SRC" "$ICON_DIR_48/server_launcher.png"
    cp "$ICON_SRC" "$ICON_DIR_32/server_launcher.png"
    cp "$ICON_SRC" "$ICON_DIR_BASE/server_launcher.png"
    cp "$ICON_SRC" "$PIXMAPS_DIR/server_launcher.png"
    echo "✓ Installed application icons to XDG hicolor, base icons, and pixmaps directories"
fi

if [ -f "$ICON_96_SRC" ]; then
    cp "$ICON_96_SRC" "$ICON_DIR_96/server_launcher.png"
fi

# Detect active binary or copy to ~/.local/bin/server_launcher
BIN_PATH=""
if [ -f "$LOCAL_BIN_DIR/server_launcher" ]; then
    BIN_PATH="$LOCAL_BIN_DIR/server_launcher"
elif [ -f "$SCRIPT_DIR/server_launcher_rust/target/release/server_launcher" ]; then
    BIN_PATH="$SCRIPT_DIR/server_launcher_rust/target/release/server_launcher"
elif [ -f "$SCRIPT_DIR/server_launcher_rust/target/debug/server_launcher" ]; then
    BIN_PATH="$SCRIPT_DIR/server_launcher_rust/target/debug/server_launcher"
else
    BIN_PATH="$LOCAL_BIN_DIR/server_launcher"
fi

PRIMARY_ICON="$ICON_DIR_512/server_launcher.png"

DESKTOP_ENTRY="[Desktop Entry]
Version=1.0
Type=Application
Name=Server Launcher
GenericName=Server Launcher & Android Cockpit
Comment=Development Server Launcher, ADB Android Cockpit & System Monitor
Exec=\"$BIN_PATH\" %u
Icon=$PRIMARY_ICON
Terminal=false
Categories=Development;Utility;System;
StartupWMClass=server_launcher
StartupNotify=true
Keywords=server;launcher;adb;android;scrcpy;terminal;ssh;docker;monitor;
X-GNOME-UsesNotifications=true"

# 1. Install in All Apps (~/.local/share/applications/)
echo "$DESKTOP_ENTRY" > "$APPS_DIR/server_launcher.desktop"
chmod +x "$APPS_DIR/server_launcher.desktop"
echo "✓ Installed desktop entry to $APPS_DIR/server_launcher.desktop (Visible in All Apps)"

# 2. Create Desktop Shortcut (~/Desktop/)
if [ -d "$DESKTOP_DIR" ]; then
    echo "$DESKTOP_ENTRY" > "$DESKTOP_DIR/server_launcher.desktop"
    chmod +x "$DESKTOP_DIR/server_launcher.desktop"
    if command -v gio >/dev/null 2>&1; then
        gio set "$DESKTOP_DIR/server_launcher.desktop" metadata::trusted true 2>/dev/null || true
    fi
    echo "✓ Created launcher shortcut on ~/Desktop/server_launcher.desktop"
fi

# Refresh desktop database and icon cache if available
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APPS_DIR" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
fi

echo "✓ Application icon and shortcut setup complete."

fix: hide the host's swap devices from memory-limited containers