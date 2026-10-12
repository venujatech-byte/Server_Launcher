# Server Launcher (Windows Edition)

A high-performance Windows desktop control panel for local developer services, native **Windows Services (Service Control Manager)** management, real-time **hardware sensors & telemetry**, and an integrated **Windows PowerShell terminal** with one-click **UAC Administrator elevation**.

Designed using the **Nocturnal Command (Material 3 Dark)** aesthetic with deep obsidian surfaces, electric violet accents, telemetry cyan badges, and pill-shaped action controls.

---

## ⚡ Highlights & Windows-Optimized Features

### 1. 🛡️ Windows Administrator & UAC Elevation
* **Automatic Privilege Detection**: Identifies whether running under standard user or elevated Administrator credentials.
* **One-Click Relaunch as Admin**: Elevates seamlessly via Windows UAC prompt (`runas` verb).
* **Elevated Action Runner**: Individual PowerShell diagnostic commands, scripts, or service commands can be executed with elevated privileges even without restarting the app.

### 2. ⚙️ Windows Services Manager (SCM)
* **Real-time Service Monitoring**: Lists all installed Windows services (`Win32_Service` / `Get-Service`) with Display Name, Service Name, Status (Running / Stopped / Paused), Startup Type (Automatic / Manual / Disabled), Log On account, and PID.
* **Instant Controls**: Start, Stop, Pause, Resume, and Restart services with feedback.
* **Startup Type Configuration**: Change startup types between `Automatic`, `Manual`, and `Disabled` on the fly.
* **Service Details Inspector**: Inspect service binary executable paths, descriptions, and dependent services.
* **Register New Windows Services**: Register new Windows background services via `sc.exe create` directly from the UI.

### 3. 📊 Hardware Sensors & Resource Telemetry
* **Live Overview Header**: Real-time display of CPU Brand, Logical Threads, Core Clocks, Total RAM, and Storage Throughput Speed.
* **Two Subtabs**:
  * **`⏱ System Sensors`**:
    * **CPU**: Processor model, physical cores, logical threads, core clock frequencies, global usage %.
    * **Memory**: Physical RAM (Used / Total / %) and Pagefile Swap Commit load.
    * **Disks**: Logical drives (C:, D:, etc.) with Total size, Free space, % used, and live disk I/O rates.
    * **GPU**: Active display adapter name, dedicated VRAM, and driver version.
    * **Battery & Power**: AC mains online state, charge percentage, and battery status.
  * **`⚡ Top Apps`**:
    * Dedicated monitor for top resource-consuming processes.
    * **Permanent Column Widths**: Column widths never jump or resize when processes appear, sort, or update.
    * **Deferred Collection**: Only scans and updates processes when navigated to the Top Apps tab.

### 4. ⚡ Interactive Windows PowerShell & Diagnostic Presets
* **PowerShell REPL Console**: Interactive PowerShell terminal with real-time stdout/stderr streaming.
* **Elevated Toggle**: Execute commands inside an elevated PowerShell session.
* **Preset Diagnostic & Administrative Commands Catalog**:
  * **Network**: Flush DNS (`Clear-DnsClientCache`), Show TCP Port Listeners (`Get-NetTCPConnection`), IP Config (`ipconfig /all`), Reset Winsock (`netsh winsock reset`).
  * **System Repair**: System File Checker (`sfc /scannow`), DISM Component Store Health Check (`DISM /Online /Cleanup-Image /ScanHealth`), Clean Temp Files.
  * **Services**: Restart Print Spooler, Restart Windows Update.
  * **Developer**: Shutdown WSL instances, Tool version checker (Node, Python, Git, Rust, Docker).

### 5. 🚀 Dev Server Launcher & Port Collision Resolver
* **Process Management**: Start, stop, and restart local servers defined in `servers.json`.
* **Live Monospace Logs**: Real-time log viewer with auto-scroll and ANSI color stripping.
* **Port Conflict Resolver**: Identifies active TCP listeners via `Get-NetTCPConnection` / `netstat -ano` with one-click process termination (`taskkill`).

---

## 🚀 Getting Started on Windows

### Prerequisites
* Windows 10 / Windows 11 / Windows Server (64-bit or 32-bit).
* Python 3.8+ (Standard library only — **no pip dependencies required**).

### Quick Launch
1. **Normal Launch**:
   ```cmd
   python server_launcher_windows.py
   ```
2. **Launch with Administrator Privileges**:
   Double-click `run_as_admin.bat` or run:
   ```cmd
   run_as_admin.bat
   ```

---

## 📦 Packaging into a Standalone `.exe` (Optional)

To bundle the application into a single executable that runs without Python installed:

```cmd
pip install pyinstaller
pyinstaller --noconfirm --onedir --windowed --name "ServerLauncherWindows" --add-data "servers.json;." --add-data "powershell_presets.json;." server_launcher_windows.py
```

The compiled standalone executable will be located in `dist\ServerLauncherWindows\ServerLauncherWindows.exe`.

