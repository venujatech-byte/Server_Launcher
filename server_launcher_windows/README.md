# Server Launcher for Windows

Rust Windows port of `../server_launcher_rust`, preserving its egui interface and Android Cockpit. The Linux source is unchanged. This folder replaces the previous Python implementation; it does not require Python.

## Requirements and verification

Use Windows 10/11 x64, Rust's MSVC toolchain, Visual Studio C++ Build Tools and Windows SDK. Android Cockpit needs the Microsoft Edge WebView2 runtime. Run `./verify.ps1` from PowerShell to type-check the app and its tests. This checkout intentionally uses `cargo check`, never `cargo build`; no Windows executable is included.

Install `adb`, `fastboot`, `scrcpy`, `ssh` and development server runtimes separately as needed and put them on PATH. Existing saved commands are not translated automatically.

## Windows behavior

- Local terminal commands use Windows PowerShell without loading profiles, with UTF-8 captured output, CRLF input and Windows prompts. Built-in `cd` persists the working directory and accepts quoted paths. Each submission starts a fresh shell, so variables and shell settings do not persist. Use an external console for interactive programs; the embedded console uses pipes rather than ConPTY.
- Stop terminates the owned Windows process tree. Background terminal commands and scripts hide console windows; external terminal sessions open their own console.
- New local scripts use PowerShell; remote Linux scripts retain Bash. Saved Bash scripts still need Bash installed or manual conversion. Normal execution respects the machine's PowerShell execution policy.
- Sensors use sysinfo's Windows CPU, memory, pagefile, volume, network and process counters. Hardware inventory uses cached CIM queries, at most every 30 seconds, with an eight-second timeout. Windows performance counters provide aggregate disk throughput and drive-letter volume rates when available. Temperatures and supported GPU core readings require the LibreHardwareMonitor WMI provider; GPU readings are associated only with an unambiguous matching device. SMART and per-process network counters are unavailable. The animated system power model is labeled as an estimate. Per-process CPU can exceed 100% across multiple cores.
- Local services use Windows Service Control Manager via PowerShell, with start/stop/restart and automatic/disabled startup. Changes may require launching the application elevated. User scope, systemd unit creation, drop-ins and unit file editing apply only to remote Linux hosts. Service logs show matching events from the newest 1,000 Service Control Manager System events.
- SSH hosts retain Linux shell, sensor and systemd behavior.

## Scope and remaining Linux tools

This is a focused port of terminal, scripts, sensors and services. Other inherited panels, including NetworkManager Wi-Fi, BlueZ Bluetooth, Linux process inspection, and vendor/Linux power controls, have not been ported to Windows. Core server and Android capabilities retain their existing platform branches. No compatibility is claimed for Linux-only saved commands or scripts.

Configuration keeps the original `servers.json` search behavior (current folder, then parent folder). Back up that file before editing shared server definitions.

## Validation limits

See the implementation report for checks completed in this environment. A successful Linux check does not validate Windows-only branches. Native Windows compilation, interactive terminal behavior, service permissions and sensor availability must also be verified on a Windows machine.
