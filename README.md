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

Server Launcher comes out-of-the-box with 7 handcrafted color palettes:
- **Catppuccin Mocha** 🌿 (Pastel Dark)
- **Catppuccin Latte** ☕ (Pastel Light)
- **Dark (Midnight)** 🌙 (Modern Slate Dark - Default)
- **Light Mode** ☀️ (Modern Slate Light)
- **Dracula** 🧛 (Vibrant Dark)
- **Nord** ❄️ (Arctic Frosty Dark)
- **Tokyo Night** 🌌 (Neon Dark)

---

## ✨ Features at a Glance

### 🖥️ Native Process Management
- **One-Click Lifecycle**: Start, stop, and restart individual servers or batch-control everything with **Start All** and **Stop All**.
- **Auto-Restart on Crash**: Per-server crash guard option that monitors exit codes and automatically restarts failed services after a short delay.
- **Port-Aware Status Detection**: Monitors process sockets and marks services as `Running` once their configured TCP port is bound and listening.
- **Pre-Stop Command Hooks**: Run graceful shutdown or data backup scripts before process termination (e.g., `firebase emulators:export ./data`).

### 🌐 Remote SSH Machines Manager
- **Multi-Host Left Navigation**: Seamlessly switch between **This PC** and any number of remote SSH servers.
- **Remote Port Discovery**: Inspect listening processes on remote Linux servers via SSH queries (`ss` / `lsof` / `netstat`).
- **Flexible Authentication**: Connect with SSH config aliases, private keys (`~/.ssh/id_rsa`, `~/.ssh/id_ed25519`), or interactive password prompts.
- **External Remote Terminals**: Launch native terminal sessions directly into remote servers with working directory preloaded.

### 📊 System Resource & Port Monitoring
- **Live Performance Metrics**: Real-time per-process **CPU %** and **RAM (MB)** utilization tracking.
- **Listening Port Scanner**: Live overview of all active TCP/UDP ports on your machine, distinguishing launcher-managed servers from unmanaged background services.
- **One-Click Import**: Found an unmanaged daemon listening on port 8080 or 3000? Import it directly into the Launcher with prefilled command and directory paths.

### 📟 Interactive Terminal & Shell Execution
- **Interactive Inline Terminal**: Inspect stdout/stderr streams or send arbitrary commands (`ps`, `git status`, `npm install`, `curl`) directly inside the card view.
- **External Terminal Launcher**: One-click opening of desktop terminals (`gnome-terminal`, `alacritty`, `kitty`, `konsole`, `xterm`, `cmd.exe`, `powershell`) initialized in the server's working directory.
- **Built-in Slash Commands**: Type `/start`, `/stop`, `/restart`, `/status`, `/clear`, or run custom action buttons from the interactive command bar.

### 🎨 Customization & Developer Experience
- **Drag-to-Reorder Cards**: Effortlessly reorder service cards using the drag handle (`⠿`) with persistent order preservation in `servers.json`.
- **Quick Launch Command Palette (`Ctrl+P`)**: Fuzzy-search commands, toggle servers, jump to tabs, or switch themes via keyboard.
- **Live Log Search (`Ctrl+F`)**: Real-time filtering and highlight for console logs with severity color coding (`stdout`, `stderr`, `stdin`, `launcher`, `warnings`).
- **One-Click IDE & Explorer Integration**:
  - 📁 **Open Folder**: Open working directory in native file manager (`xdg-open` / File Explorer).
  - 💻 **Open in VS Code**: Launch the project directory directly in Visual Studio Code (`code .`).
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
│       ├── scanner.rs             # Local port scanner & sysinfo monitor
│       ├── remote.rs              # Remote SSH connector & port inspector
│       ├── modals.rs              # Add/edit dialogs, SSH modal, Command Palette
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
| `theme` | `string` | Selected theme ID (`dark`, `catppuccin_mocha`, `catppuccin_latte`, `light`, `dracula`, `nord`, `tokyo_night`) |
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
| `Ctrl + P` | Open Quick Launch Command Palette |
| `Ctrl + F` | Search & filter console logs / server overview |
| `Enter` | Submit command in interactive command bar or inline terminal |
| `Escape` | Dismiss modals, command palette, or autocomplete suggestions |
| `⠿ Drag` | Drag card up or down to reorder server list |
| `/` | Trigger slash command autocomplete in server log views |

---

## 🛡️ License

This project is licensed under the MIT License - feel free to use, modify, and distribute for personal and commercial projects.
