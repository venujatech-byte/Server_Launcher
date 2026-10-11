# 🚀 Server Launcher

[![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg)](https://www.rust-lang.org/)
[![egui](https://img.shields.io/badge/GUI-egui%200.29-blue.svg)](https://github.com/emilk/egui)
[![Python](https://img.shields.io/badge/Python-3.10+-yellow.svg)](https://www.python.org/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20Windows%20%7C%20macOS-lightgrey.svg)]()

**Server Launcher** is a modern, high-performance developer dashboard and process control center for managing local microservices, web servers, background daemons, and remote SSH hosts.

Available in a native GPU-accelerated **Rust Desktop application**, a lightweight zero-dependency **Python Desktop GUI**, and a terminal-based **Python TUI**.

---

## 📸 Screenshots & Themes

Server Launcher comes out-of-the-box with 8 handcrafted color palettes, spearheaded by the sleek **Nocturnal Command** design system:
- **Nocturnal Command** 🌌 (Material 3 Dark / Nocturnal Obsidian — Default)
- **Dark (Midnight)** 🌙 (Modern Slate Dark)
- **Catppuccin Mocha** 🌿 (Pastel Dark)
- **Catppuccin Latte** ☕ (Pastel Light)
- **Light Mode** ☀️ (Modern Slate Light)
- **Dracula** 🧛 (Vibrant Dark)
- **Nord** ❄️ (Arctic Frosty Dark)
- **Tokyo Night** 🌆 (Neon Dark)

### 🌌 Nocturnal Command Design System
Engineered for modern developer cockpits, server orchestrators, and technical tools:
- **Obsidian Luminance Tiers**: Elevation expressed through calibrated dark surface luminance (`#13131a`, `#1b1b22`, `#1f1f26`) rather than blurry drop shadows.
- **Electric Accents**: Electric Violet (`#c4c0ff`), Telemetry Cyan (`#70d5e4`), and Mint (`#97f6ca`) highlight active processes, ports, and metrics.
- **Pill Primitives (`rounded-full`)**: Smooth pill action buttons, pill search bar, and Material 3 status chips (`● Running`, `○ Stopped`).
- **Recessed Terminal Wells**: Deep obsidian containers (`#0a0a0f`) for monospace command previews (`$ <command>`) with live runtime status tags.

---

## ✨ Features at a Glance

### 🖥️ Native Process Management
- **⚙️ Settings & Preferences Modal (`Ctrl+,`)**: Centralized preferences window accessible via the top-bar `⚙` icon button or `Ctrl+,`:
  - **🎨 Theme Picker & Quick Pills**: Choose from all 8 themes with live preview and instant switching.
  - **🚀 System Startup Autostart**: Toggle launch on system boot/login with status chip pills (`● ENABLED` / `○ DISABLED`).
  - **📦 Server Configuration Management**: One-click `📥 Import servers.json` and `📤 Export servers.json` file dialogs for effortless backup and restore.
  - **⌨ Keyboard Shortcuts Guide**: Instant cheatsheet for `Ctrl+P`/`Ctrl+K`, `Ctrl+,`, `Ctrl+A`, `Ctrl+X`, and `Ctrl+F`.
  - **ℹ️ System Diagnostics**: Quick overview of host IP, managed instance count, and active config path.
- **✨ Streamlined Borderless Top Bar**: Minimalist, spacious header layout eliminating overcrowding: includes instance search pill bar, batch controls (`▶ Start All`, `⏹ Stop All`), Quick Launch (`⚡ Quick Launch Ctrl+P`), `+ Add Server`, and `⚙` Settings.
- **🚀 Per-Server Launch Autostart**: Configure individual servers with the `🚀 Autostart` checkbox to start running immediately whenever Server Launcher is opened.
- **Auto-Restart on Crash**: Per-server crash guard option that monitors exit codes and automatically restarts failed services after a short delay.
- **Port-Aware Status Detection**: Monitors process sockets and marks services as `Running` once their configured TCP port is bound and listening.
- **Pre-Stop Command Hooks**: Run graceful shutdown or data backup scripts before process termination (e.g., `firebase emulators:export ./data`).

### 🌐 Remote SSH Machines Manager
- **Multi-Host Left Navigation**: Seamlessly switch between **This PC** and any number of remote SSH servers.
- **Remote Port Discovery & Management**: Inspect listening processes on remote Linux servers via SSH queries (`ss` / `lsof` / `netstat`), filter by port/name, inspect process details (`ps`), test endpoints (`curl`), and terminate stuck processes (`kill`) with one click.
- **Flexible Authentication**: Connect with SSH config aliases, private keys (`~/.ssh/id_rsa`, `~/.ssh/id_ed25519`), or interactive password prompts.
- **External Remote Terminals**: Launch native terminal sessions directly into remote servers with working directory preloaded.

### 📊 System Resource & Port Monitoring
- **Live Performance Metrics**: Real-time per-process **CPU %** and **RAM (MB)** utilization tracking with cached lookup for zero CPU scroll overhead.
- **Listening Port Scanner**: Live overview of all active TCP/UDP ports on your machine, distinguishing launcher-managed servers from unmanaged background services.
- **One-Click Import**: Found an unmanaged daemon listening on port 8080 or 3000? Import it directly into the Launcher with prefilled command and directory paths.

### 📟 Interactive Terminal & Shell Execution
- **Interactive Inline Terminal**: Inspect stdout/stderr streams or send arbitrary commands (`ps`, `git status`, `npm install`, `curl`) directly inside the card view.
- **High-Performance Virtualized Logs**: Smooth scrolling with `show_rows` virtualization, distinct scroll persistence, and optional auto-scroll toggle (`📜 Auto-scroll`) eliminating frame drops and jitter.
- **External Terminal Launcher**: One-click opening of desktop terminals (`gnome-terminal`, `alacritty`, `kitty`, `konsole`, `xterm`, `cmd.exe`, `powershell`) initialized in the server's working directory.
- **Built-in Slash Commands**: Type `/start`, `/stop`, `/restart`, `/status`, `/clear`, or run custom action buttons from the interactive command bar.

### ⚙️ SystemD Services Manager & Sudo Authentication
- **Local & Remote SystemD Control**: Inspect all units across User (`--user`) and System scopes with live state badges (`Active`, `Inactive`, `Failed`).
- **Interactive Lifecycle Actions**: Start, stop, restart, enable, disable, and reload daemon (`systemctl daemon-reload`) directly from the GUI.
- **🔐 Secure Sudo Password Popup**: When controlling or creating local system-level services requiring root privileges, a password modal automatically prompts for authentication.
- **Session Sudo Caching**: Optionally remember sudo credentials for the current session, with an inline badge `🔑 Sudo [✕]` to clear or forget them at any time.
- **Built-in Unit Creator**: GUI modal to define, install, and immediately enable new systemd service units (`.service`) into user config or `/etc/systemd/system/`.
- **Integrated Journalctl Log Stream**: View the last 100 journal log lines per unit with configurable auto-refresh rates (Off, 1s, 2s, 5s, 10s, 30s, 60s) and automatic root fallback.

### 🌡️ Hardware Telemetry & Power Sensors
- **Comprehensive Hardware Metrics**: Live monitoring of CPU frequency/speed, CPU package power, GPU utilization, GPU core clock, GPU power draw, and VRAM.
- **Battery & Power Supply Analytics**: Real-time battery percentage, charging state, instantaneous power draw/discharge wattage, voltage, and battery health.
- **Component Temperatures & Fans**: Categorized sensor breakdown for CPU cores, GPU dies, NVMe storage drives, motherboard thermal zones, and cooling fan RPMs.
- **Remote Telemetry via SSH**: Fetch hardware sensor snapshots from remote servers with asynchronous, non-blocking polling.

### 📜 Custom Scripts & ⚡ Saved Commands
- **Script Runner**: Manage and trigger automation scripts with live execution output tracking across local and remote SSH machines.
- **Quick Command Vault**: Store frequently used developer shell commands categorized by tags with one-click terminal execution.

### 🎨 Customization & Developer Experience
- **Instant Click Responsiveness**: Fully responsive cards with context menu reordering (`▲ Move Up` / `▼ Move Down`) and zero drag-interception or missed clicks.
- **Quick Launch Command Palette (`Ctrl+P`)**: Fuzzy-search commands, toggle servers, jump to tabs, or switch themes via keyboard.
- **Live Log Search (`Ctrl+F`)**: Real-time filtering and highlight for console logs with severity color coding (`stdout`, `stderr`, `stdin`, `launcher`, `warnings`).
- **One-Click IDE, Explorer & Console Integration**:
  - 📁 **Open Folder**: Open working directory in native file manager (`xdg-open` / File Explorer).
  - 💻 **Open in VS Code**: Launch the project directory directly in Visual Studio Code (`code .`).
  - 🖥️ **Open in Console**: Launch an external desktop terminal emulator (`ptyxis`, `gnome-terminal`, `alacritty`, `cmd.exe`, etc.) initialized in the server's working directory.
  - 🔗 **Browser Links**: Quick-open web dashboards, API docs, or localhost endpoints in your default browser.

---

## 📂 Project Structure

```text
Server_Launcher/
├── server_launcher_rust/          # Native Rust + egui 0.29 Desktop Application
│   ├── Cargo.toml                 # Dependencies & package configuration
│   └── src/
│       ├── main.rs                # Application entrypoint & runtime setup
│       ├── app.rs                 # Primary GUI layouts, panels, and viewports
│       ├── theme.rs               # 7-palette theme engine & visuals
│       ├── service.rs             # Subprocess lifecycle, log pipes & metrics
│       ├── systemd.rs             # SystemD unit controller, journalctl & unit creation
│       ├── sensors.rs             # Hardware sensors collector (CPU/GPU power, battery, temps)
│       ├── scripts.rs             # Custom script manager & execution runner
│       ├── commands.rs            # Saved command vault & integrated terminal sessions
│       ├── scanner.rs             # Local port scanner & sysinfo monitor
│       ├── remote.rs              # Remote SSH connector & port inspector
│       ├── modals.rs              # Add/edit dialogs, SSH modal, Command Palette
│       ├── autostart.rs           # System boot/login autostart (XDG & Windows)
│       └── config.rs              # Configuration schema & JSON persistence
│
├── server_launcher_linux.py       # Python Tkinter Desktop GUI (Linux optimized)
├── server_launcher.py             # Python Tkinter Desktop GUI (Cross-platform)
├── server_launcher_tui.py         # Terminal User Interface edition
├── install_desktop_icon.sh        # Linux desktop launcher installer (.desktop)
├── servers.json                   # Saved server configurations & theme settings
└── assets/                        # Application icons & branding assets
```

---

## 🛠️ Getting Started

### 1. Rust Native Edition (Recommended)

The Rust edition delivers high performance, low memory footprint, and GPU-accelerated smooth rendering.

#### Prerequisites
- Rust 1.75+ & Cargo installed ([rustup.rs](https://rustup.rs)):
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- **Linux Packages** (for windowing & clipboard support):
  - **Ubuntu / Debian**:
    ```bash
    sudo apt update
    sudo apt install -y pkg-config libgl1-mesa-dev libx11-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
    ```
  - **Fedora / RHEL**:
    ```bash
    sudo dnf install -y pkgconfig mesa-libGL-devel libX11-devel
    ```
  - **Arch Linux**:
    ```bash
    sudo pacman -S --needed pkgconf mesa libx11
    ```

#### Build and Run
```bash
cd server_launcher_rust
cargo run --release
```

#### Running Tests
```bash
cargo test
```

---

### 2. Python Desktop Edition

A zero-dependency option that runs using Python's standard library.

#### Prerequisites
- Python 3.10+
- Tkinter system libraries:
  - **Ubuntu/Debian**: `sudo apt install python3-tk`
  - **Fedora**: `sudo dnf install python3-tkinter`
  - **Windows / macOS**: Included by default in official Python installers.

#### Run Desktop GUI
```bash
# Standard cross-platform desktop GUI
python3 server_launcher.py

# Linux-optimized GUI
python3 server_launcher_linux.py
```

---

### 3. Python Terminal TUI Edition

Ideal for headless servers, SSH sessions, or lightweight terminal setups:

```bash
python3 server_launcher_tui.py
```

---

## 🐧 Linux Desktop Integration

To install Server Launcher as a native Linux desktop application with system launcher search:

```bash
chmod +x install_desktop_icon.sh
./install_desktop_icon.sh
```

This generates a standard `.desktop` entry in `~/.local/share/applications/` and links the application icon from `assets/`.

---

## ⚙️ Configuration Format (`servers.json`)

All configurations are human-readable and automatically persisted to `servers.json`:

```json
{
  "theme": "catppuccin_mocha",
  "ssh_hosts": [
    {
      "id": "prod-vps",
      "name": "Production VPS",
      "host": "192.168.1.100",
      "port": 22,
      "user": "ubuntu",
      "identity_file": "~/.ssh/id_ed25519",
      "password": null
    }
  ],
  "servers": [
    {
      "key": "web-api",
      "name": "FastAPI Backend",
      "group": "Backend",
      "cwd": "/home/user/projects/web-api",
      "command": "python -m uvicorn main:app --port 8000",
      "stop_command": "",
      "port": 8000,
      "auto_restart": true,
      "env": {
        "ENV": "development",
        "DEBUG": "true"
      },
      "actions": [
        {
          "name": "Run Migrations",
          "command": "alembic upgrade head"
        }
      ],
      "links": [
        {
          "name": "Swagger Docs",
          "url": "http://localhost:8000/docs"
        }
      ]
    }
  ]
}
```

### Configuration Fields
| Field | Type | Description |
| :--- | :--- | :--- |
| `theme` | `string` | Selected theme ID (`nocturnal_command`, `dark`, `catppuccin_mocha`, `catppuccin_latte`, `light`, `dracula`, `nord`, `tokyo_night`) |
| `name` | `string` | Display name of the service |
| `group` | `string` | Category badge for grouping related microservices |
| `cwd` | `string` | Working directory where the start/action commands execute |
| `command` | `string` | Shell command used to launch the service |
| `stop_command` | `string` | Optional cleanup command executed before termination |
| `port` | `number` | TCP port monitored to confirm successful startup |
| `auto_restart` | `boolean` | Automatically restart the process if it crashes or exits unexpectedly |
| `env` | `object` | Key-value dictionary of environment variables |
| `actions` | `array` | Custom action buttons runnable in the service directory |
| `links` | `array` | Quick-access URLs opened in your default web browser |

---

## ⌨️ Shortcuts & Navigation

| Keybinding | Action |
| :--- | :--- |
| `Ctrl + P` / `Ctrl + K` | Open Quick Launch Command Palette & Instance Search |
| `Ctrl + F` | Search & filter console logs / server overview |
| `Enter` | Submit command in interactive command bar, terminal, or authentication modal |
| `Escape` | Dismiss modals, sudo password prompt, command palette, or autocomplete suggestions |
| `⠿ Drag` | Drag card up or down to reorder server list |
| `/` | Trigger slash command autocomplete in server log views |

---

## 🛡️ License

This project is licensed under the MIT License - feel free to use, modify, and distribute for personal and commercial projects.