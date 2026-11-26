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
- **✨ Streamlined Borderless Top Bar**: Minimalist, spacious header layout eliminating overcrowding: includes enlarged branding logo, instance search pill bar, batch controls (`▶ Start All`, `⏹ Stop All`), Quick Launch (`⚡ Quick Launch Ctrl+P`), `+ Add Server`, and `⚙` Settings.
- **🚀 Per-Server Launch Autostart**: Configure individual servers with the `autostart` checkbox in the left sidebar and overview cards to start running immediately whenever Server Launcher opens.
- **🖥️ Run in External Terminal Toggle**: Per-server `[✓] run in external terminal` checkbox in both overview cards and left sidebar containers to launch services into a native desktop terminal window upon start.
- **Auto-Restart on Crash**: Per-server crash guard option that monitors exit codes and automatically restarts failed services after a short delay.
- **Port-Aware Status Detection**: Monitors process sockets and marks services as `Running` once their configured TCP port is bound and listening.
- **Pre-Stop Command Hooks**: Run graceful shutdown or data backup scripts before process termination (e.g., `firebase emulators:export ./data`).

### 🌐 Remote SSH Machines Manager
- **Multi-Host Left Navigation**: Seamlessly switch between **This PC** and any number of remote SSH servers.
- **Remote Port Discovery & Management**: Inspect listening processes on remote Linux servers via SSH queries (`ss` / `lsof` / `netstat`), filter by port/name, inspect process details (`ps`), test endpoints (`curl`), and terminate stuck processes (`kill`) with one click.
- **Flexible Authentication**: Connect with SSH config aliases, private keys (`~/.ssh/id_rsa`, `~/.ssh/id_ed25519`), or interactive password prompts.
- **External Remote Terminals**: Launch native terminal sessions directly into remote servers with working directory preloaded.
- **Remote Power**: The SSH host header groups **Shut Down** and **Restart** under **Power**. Both actions require confirmation.
- **Quiet background SSH on Windows**: Connection checks, commands run inside the app, and file transfers run without opening console windows. The **Terminal** button opens an interactive SSH window.
- **Remote Files**: Browse folders, transfer files, and manage files on the selected SSH host.
- **Remote Apps & Packages**: Browse and filter installed packages. **Uninstall** opens the host's package manager in an SSH terminal for supported packages. Refresh the Apps tab after the package manager finishes.

#### Remote Desktop (RDP and VNC)

The Rust desktop app can open an RDP or VNC viewer from an SSH host's **GUI**
menu. Configure each protocol with the gear button beside its menu item. RDP
credentials and VNC passwords are stored in the operating system credential
store; protocol passwords are not saved in `servers.json`.

- RDP supports NLA by default, explicit TLS, certificate fingerprint prompts,
  keyboard and mouse input, text and bounded image clipboard sync, screenshots,
  recording, local audio playback, and dynamic resize when the server supports
  Display Control. Saved certificate fingerprints can be reviewed or removed in
  Settings → Remote Desktop. The RDP profile can request two side-by-side remote
  displays using the selected combined resolution, and it composites the remote
  pointer into the displayed framebuffer. Display controls include
  smooth/fast texture filtering, incremental/full-frame repainting, Automatic and RemoteFX
  profiles, 32/24-bit true color, 16/15-bit high color, left-handed mouse mapping, and an
  option to disable smooth scrolling. The profiles include GFX AVC420 and
  AVC444. To use them, select a supported Cisco OpenH264 library in Settings →
  Remote Desktop; the app does not bundle that library. Without it, AVC is not
  advertised and the server can use another supported codec. The 256-color
  (8 bpp) profile uses legacy bitmap updates. The server negotiates the codec;
  these options do not guarantee a specific frame rate or speedup.
- VNC supports classic password authentication, Tight (fill, copy, palette,
  gradient, JPEG, and PNG), Raw, CopyRect, and ZRLE framebuffer updates,
  keyboard and mouse input, text clipboard sync, screenshots,
  recording, and server-supported resolution changes. Its TCP user timeout setting
  applies only to direct Linux connections; zero uses the OS default, and SSH tunnels
  ignore it. The profile can close the viewer after connection retries fail.
  Classic VNC is not
  encrypted; the viewer warns before a direct connection to public or unknown
  hosts. Use the SSH tunnel option on untrusted networks.
- Both viewers offer Fit, native-size, and Fill scaling, a live received display
  update-rate display, and a statistics menu. The FPS value counts update batches
  delivered by the remote server; it is not the server's capture rate or the
  local display's rendered frame rate. Local clipboard text is read only
  while the connected viewer window is focused. Classic VNC text is limited to
  Latin-1; VNC clipboard images and file contents are not supported. Smooth-scroll
  accumulation can be disabled in either protocol's profile.
- Screenshot and recording folders can be set separately under Settings →
  Remote Desktop. Recording supports MP4 (H.264), WebM (VP9), or PNG sequence,
  at 15, 24, 30, or 60 FPS, and can use an explicitly selected FFmpeg executable or
  `ffmpeg` from `PATH`. Quality, clipboard, scaling, and reconnect defaults
  apply to newly created RDP/VNC profiles; existing profiles keep their choices.
  Screenshot clipboard copying can be set as the viewer default.
  By default, captures are saved under Pictures/ServerLauncher. Linux DEB and
  RPM packages recommend FFmpeg for MP4 recording; it remains optional because
  recordings can fall back to PNG sequences.

RDP microphone capture is available as an opt-in profile setting. When enabled,
the remote PC can request audio from the local default input device while the
session is connected; it is off by default and has only been compile-checked,
not tested against a live RDP server or microphone. RDP drive redirection can
share selected local folders. Each share is read-only by default, and the app
asks before sharing a folder or allowing remote changes. Linux/macOS uses
separate filesystem roots and blocks write requests for read-only shares;
Windows uses IronRDP's native drive backend. This needs real-server validation.
RDP profiles can route through Microsoft RD Gateway over HTTPS. The gateway
uses the saved RDP username/password, requires a system-trusted gateway
certificate, and cannot be combined with Tunnel via SSH. This path still needs
live gateway validation. Right Ctrl host-key shortcuts (fullscreen, screenshot,
recording, disconnect, and Send keys) are available on Linux/X11. Focused
keyboard capture supports Linux/X11 and Windows; Windows reserves
Ctrl+Alt+Delete, which remains available from Send keys. Wayland without
XWayland cannot guarantee capture of system-reserved shortcuts. Keyboard-layout-specific IME is not implemented; committed
non-ASCII IME text is forwarded on platforms that provide egui commit events,
while Linux relies on its keyboard text events. Linux source builds
need the ALSA development package
(`libasound2-dev` on Ubuntu/Debian, `alsa-lib-devel` on Fedora/RHEL) for local
RDP audio playback. RDP dual-display layout is best-effort: the server must
support two monitors, and the viewer presents their combined desktop in one
window. VNC
username-based authentication is not implemented. VNC cursor
shapes can be toggled in the VNC profile; they are enabled by default and need
server support for RichCursor or XCursor shapes and PointerPos. SSH desktop tunnels support Unix
and Windows key/agent credentials. Windows password-only SSH tunnels use the
OS credential store through OpenSSH askpass and still need runtime validation.
Protocol interoperation and performance still
need testing against real servers on Windows, Linux X11, and Linux Wayland.

### Local Apps & Packages

Browse and filter the installed apps and packages on **This PC**. The list
and details panel display installed app icons when artwork is available.

### 📊 System Resource & Port Monitoring
- **Live Performance Metrics**: Real-time per-process **CPU %** and **RAM (MB)** utilization tracking with cached lookup for near-zero CPU scroll overhead.
- **Top Apps & Process Properties**: Inspect resource-heavy processes, view detailed command arguments, working directories, open files, memory maps, and property files with responsive non-blocking background inspection.
- **Listening Port Scanner**: Live overview of all active TCP/UDP ports on your machine, distinguishing launcher-managed servers from unmanaged background services.
- **One-Click Import**: Found an unmanaged daemon listening on port 8080 or 3000? Import it directly into the Launcher with prefilled command and directory paths.
- **⚡ Ultra-Low Idle Footprint**: Engineered with reactive frame rendering, lightweight timer tickers, and optimized release builds (`lto = "thin"`) for minimal CPU, GPU, and RAM consumption.

### 📟 Interactive Terminal & Shell Execution
- **Interactive Inline Terminal**: Inspect stdout/stderr streams or send arbitrary commands (`ps`, `git status`, `npm install`, `curl`) directly inside the card view.
- **High-Performance Virtualized Logs**: Smooth scrolling with `show_rows` virtualization, distinct scroll persistence, and optional auto-scroll toggle (`📜 Auto-scroll`) eliminating frame drops and jitter.
- **External Terminal Launcher**: One-click opening of desktop terminals (`gnome-terminal`, `ptyxis`, `alacritty`, `kitty`, `konsole`, `xterm`, `cmd.exe`, `powershell`) initialized in the server's working directory.
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
- **Protected Linux Sensors**: Use **Unlock hardware sensors** to request read-only access, then **Refresh protected sensors** to take another snapshot. Available readings depend on the hardware.
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
│       ├── remote_desktop/         # RDP/VNC profiles, viewers, input, clipboard, capture, tunnels
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
      "autostart": false,
      "auto_restart": true,
      "own_console": false,
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
| `autostart` | `boolean` | Automatically start this server when Server Launcher opens |
| `auto_restart` | `boolean` | Automatically restart the process if it crashes or exits unexpectedly |
| `own_console` | `boolean` | Launch service in an external desktop terminal window upon start |
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
## Desktop updates and release packages

In either Rust app, open **Settings → About → Check for updates**. GitHub builds
publish a versioned update manifest with package sizes and SHA-256 digests.
For older releases without a manifest, About uses GitHub-provided SHA-256
digests for recognized installers.
About selects the compatible package for your platform and installation.
Fedora and other supported RPM distributions use RPM, and Debian and Ubuntu
use DEB. Windows uses the detected installer type, and Microsoft Store
installations use the Store’s update page. Choose **Install update** to
download and verify the installer, then close the launcher through its
normal exit confirmation. Canceling that confirmation also cancels the
update. Linux package updates use the system package manager via Polkit.
AppImage updates replace the original file if its directory is writable. The updater uses the system `curl`
command, which the DEB and RPM declare as a dependency. AppImage and portable
Linux users need `curl` installed on the host.

To prepare GitHub release packages, run **Build and Publish Release** from
Actions. For an official version, push a tag in the form `v1.2.3`. The workflow
also publishes on `main` and `design`, using the run number as the patch
version. Runs on `fix/ui-alignment` produce the `server_launcher-release-bundle`
artifact for inspection, including the normalized packages and `latest.json`.
Windows artifacts include the NSIS setup EXE, MSI, portable ZIP, and MSIX.
The upload job waits for the Linux and Windows artifacts before publishing
`latest.json` alongside them.

For Microsoft Store submission, set the repository variables
`WINDOWS_STORE_IDENTITY`, `WINDOWS_STORE_PUBLISHER`, and
`WINDOWS_STORE_PUBLISHER_NAME` to the values in Partner Center, then download
the `server_launcher-windows-store` artifact. Without those values the
workflow uses a placeholder identity. The MSIX is unsigned for Store
submission, so Windows requires a trusted signature for local sideloading.


### Saved SSH credentials

The desktop launcher protects saved passwords using the Linux Secret Service
(`secret-tool` and a running keyring) or Windows DPAPI for the current user.
On first launch after upgrading, old plaintext passwords are migrated before
the config is replaced. If the credential store is unavailable, the old
config stays intact. Fix or unlock the keyring and restart to retry migration.
Backups made before migration may still contain passwords.
