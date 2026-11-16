# Port analysis and validation

## Analysis

The Linux source contains shared egui UI, Tauri Android Cockpit, server process management, SSH, and several Linux-specific control panels. The Windows copy retains that structure and its comments. Adaptations are limited to terminal/script execution, sensors, local service management, Windows GUI startup and package configuration.

Terminal callers share the PowerShell command path, hidden-console helper and process-tree termination. Saved remote scripts retain Bash defaults. Terminal completion no longer waits while holding the child mutex, so Stop can acquire it.

Sensor snapshot structs and consumers remain compatible. A Windows collector replaces the local `/proc`, `/sys`, hwmon, Linux CLI and estimated-power collection path. The interface bypasses Linux power controls and CPU power estimates for local Windows. Remote Linux queries retain their existing behavior.

Service API signatures remain stable. Local Windows calls dispatch to the Service Control Manager adapter, with quoted PowerShell service names and a fixed action allowlist. Local systemd editors and user scope are unavailable; SSH callers retain systemctl and journalctl.

## Checks

- `cargo check --tests --offline`: PASS on x86_64 Linux; no warnings or errors. Tests compiled, not executed.
- `node --check web/app.js`: PASS.
- JSON parse checks for Tauri configuration and font metadata: PASS.
- Final commit-range whitespace check: reports trailing whitespace and extra EOF blank lines inherited from the copied Linux source; these were preserved to avoid unrelated changes.
- Original `server_launcher_rust` working tree: unchanged.

- `cargo check --target x86_64-pc-windows-gnu --tests --offline`: PASS, zero errors, 15 warnings about platform-specific unused imports/variables/code. Uses `RC_x86_64_pc_windows_gnu=/tmp/windows-toolchain/resource-compiler`; MinGW windres was extracted into `/tmp` from Fedora's `mingw64-binutils` RPM and used the system C preprocessor. No system package installation succeeded or was needed.
- The cross-check caught and resolved the missing ICO Windows resource and a non-Linux battery-helper type-inference failure. Both Windows application and test code were type-checked; no Windows test executable was run.

## Limits

No `cargo build` was run and no executable was produced. Windows runtime, hardware readings, service privileges, external consoles, WebView2 and UI screenshots have not been tested on Windows. Run `verify.ps1` with a native MSVC Rust installation. Embedded terminal uses pipes, with a new shell per command. Optional hardware providers determine temperature availability. Unported Linux tools and unsupported telemetry fields are listed in README.md.

## Version history

The requested 50 commits consist of 35 copied source/config/asset files, 10 platform adaptation changes, Windows service adapter, verification script, copied cockpit checks, usage documentation, and this validation record. Intermediate copy commits are source import steps; the completed tree is the validated port. Existing deletions of the previous Python implementation are left unstaged.
