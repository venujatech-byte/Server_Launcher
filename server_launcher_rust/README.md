# Server Launcher (Rust GUI)

A fast, lightweight, cross-platform developer server launcher GUI written in Rust with **egui / eframe**.

Runs natively on both **Linux** and **Windows** with near-zero idle CPU and ~15 MB RAM usage.

---

## Features

- **Cross-Platform Process Management**: Clean subprocess spawning, streaming, and tree termination on both Linux and Windows.
- **Server Groups**: Tag and group servers (Frontend, Backend, DB, etc.) with group-level start/stop.
- **Real-time Log Viewer**: Thread-safe buffered log streaming with error/warning syntax highlighting.
- **Log Search & Filter (`Ctrl+F`)**: Substring search with instant highlighting, match counter, and line filtering.
- **Quick Launch Palette (`Ctrl+P`)**: Fast fuzzy/substring palette to start/stop servers and trigger actions.
- **Interactive Terminal Input (`❯`)**: Send commands directly to the running server's `stdin`.
- **Port & Resource Polling**: Automatic TCP port detection, plus CPU & memory usage per server.
- **Shared Configuration**: Uses the same `servers.json` as the Python launchers.

---

## Prerequisites

Install Rust if not already installed:

### Linux / macOS:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

On Fedora/RHEL for native windowing dependencies:
```bash
sudo dnf install libxcb-devel libxkbcommon-devel
```
On Ubuntu/Debian:
```bash
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev
```

### Windows:
Download and run the installer from:
[https://rustup.rs/](https://rustup.rs/)

---

## Build & Run

### 1. Run Development Build:
```bash
cd server_launcher_rust
cargo run
```

### 2. Build Optimized Release Binary:
```bash
cargo build --release
```
The compiled standalone binary will be located at:
- **Linux**: `server_launcher_rust/target/release/server_launcher`
- **Windows**: `server_launcher_rust/target/release/server_launcher.exe`

You can copy this single executable anywhere!

---

## Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| **Ctrl + P** | Open Quick Launch Palette |
| **Ctrl + F** | Open / Toggle Log Search Bar |
| **Ctrl + A** | Start all servers |
| **Ctrl + X** | Stop all servers |
| **Escape** | Close active dialog or search bar |
