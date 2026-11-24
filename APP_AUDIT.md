# Server Launcher code map and audit

This report covers the Linux and Windows desktop launchers, their shared updater, and the Android Cockpit boundary. Findings are from source inspection. Severity describes the impact and conditions visible in the code, without claiming a reproduced exploit. CPU and memory findings identify work the code performs and need runtime measurements before assigning a cost.

## Where to make changes

Both desktop crates use the same paths within `src`. The paths below are relative to `server_launcher_rust/src` or `server_launcher_windows/src`. The split moves existing renderers without redesigning their layouts or rewriting their command handlers. `app.rs` still owns application state, startup, frame dispatch, shared helpers and layout tests.

| Page or control | File | Backend or shared logic |
|---|---|---|
| Workspace tabs and machine rail | `app/workspace.rs` | `app.rs`, `config.rs` |
| Servers groups and cards, screenshot 1 | `app/servers.rs` | `service.rs`, `config.rs` |
| Overview and other local listeners, screenshot 2 | `app/listeners.rs` | `scanner.rs`, `procmgr.rs` |
| Selected server tabs, output and composer, screenshot 3 | `app/server_detail.rs` | `service.rs` |
| Scripts | `app/scripts.rs` | `scripts.rs` |
| Saved commands and interactive terminal | `app/commands.rs` | `commands.rs`, `service.rs` |
| Systemd status, Files & Drop-ins, Dependencies | `app/systemd.rs` | `systemd.rs`, Windows `windows_services.rs` |
| Settings, About and sudo prompt | `app/settings.rs` | `theme.rs`, `autostart.rs`, root `shared/updater.rs` |
| SSH workspace and connection or scan coordination | `app/ssh.rs`, `app/ssh/connection.rs` | `remote.rs`, `config.rs` |
| SSH listener list and terminal | `app/ssh/listeners.rs`, `app/ssh/terminal.rs` | `remote.rs`, `service.rs` |
| Sensors navigation and routing | `app/sensors.rs` | `sensors.rs`, platform and SSH collectors |
| Sensors overview | `app/sensors/overview.rs` | `sensors.rs` |
| Top Apps and process dialogs | `app/sensors/processes.rs` | `sensors.rs`, `procmgr.rs` |
| CPU, RAM and GPU | `app/sensors/cpu.rs`, `ram.rs`, `gpu.rs` under the same directory | `sensors.rs`, `power.rs` |
| Storage, Wi-Fi and Data Usage | `app/sensors/storage.rs`, `wifi.rs`, `data_usage.rs` | `sensors.rs`, `wifi.rs` |
| Power, Bluetooth and Temperature | `app/sensors/power.rs`, `bluetooth.rs`, `temperature.rs` | `power.rs`, `bluetooth.rs`, `sensors.rs` |
| Server, script, command, host and confirmation dialogs | `modals.rs` | Main dispatch in `app.rs` |
| Android device picker and Cockpit handoff | `adb_view.rs` | `adb.rs`, `tauri_app.rs`, separate frontend at each crate’s `web/app.js` |

The Sensors split is for display code. Hardware collection remains in its existing platform modules. The Android Cockpit already runs as a separate Tauri frontend and was not moved in this pass.

## Security and reliability findings

| Priority | Evidence and impact | Proposed fix |
|---|---|---|
| High | `config.rs::SshRemoteHost` serializes its password and `ConfigFile::save_to_file` writes the config as JSON. `app.rs::persist_config` includes the SSH host list. Anyone with read access to that file or its backups can read saved SSH passwords. | Store credentials in the OS credential store, migrate old passwords with visible success or failure, and never include them in exports. Use restrictive permissions and atomic writes for configuration. |
| High | `remote.rs::launch_external_ssh_terminal` and `launch_external_ssh_command` build shell scripts from host fields. The Linux script is written under a predictable temp filename before its permissions are modified. The Windows command path assembles a shell string. This risks exposing credentials or treating imported host fields as executable shell syntax. | Use argument arrays where the terminal supports them, validate SSH destinations, and use secure temporary files where a wrapper is required. Keep secrets out of wrapper contents, handle write errors and clean up after launch. |
| High | `remote.rs::reboot_remote_device` and `shutdown_remote_device` interpolate the sudo password into the remote command string. It can be visible where process arguments or command auditing are recorded. | Pass credentials over stdin to the remote sudo command. Keep the displayed and logged command free of secrets. |
| Medium | `modals.rs::SshHostModalState` displays strict host key and agent toggles, but `config.rs::SshRemoteHost` has no corresponding fields. `remote.rs` uses `StrictHostKeyChecking=accept-new` for connection checks and commands. Users cannot enforce the stricter behavior from that checkbox. | Persist and honor per-host policy consistently, including external terminals. Prefer explicit host key verification when a user requests strict mode. |
| Medium | `app/ssh/listeners.rs` confirms SSH Stop/Kill against a cached listener snapshot. The identity match catches UI changes but does not prove the remote process still has that PID when the command runs. Review local Overview actions for the same gap. The Top Apps path already has identity checks to compare against. | Validate process start identity near mutation. Use safe service management where identity cannot be confirmed, and keep the SSH lockout confirmation. |
| Medium | `app.rs::run_remote_ssh_command` moved to `app/ssh/terminal.rs`. It starts an independent SSH child and two reader threads for each submission and has no per-host command limit, application deadline, or cancel control there. SSH liveness settings do not end remote commands that are still running. | Bound concurrent commands per host and track the child so Cancel or host removal can terminate it. Preserve interactive sessions that explicitly require longer lifetimes. |
| Medium | The updater verifies SHA-256 from the same release channel as the asset before starting an installer. A digest detects corruption, but is not a separate trust root if release credentials and metadata are compromised. Linux then requests package manager privileges. | Sign release manifests or enforce platform signing with a separately trusted identity. Preserve Microsoft Store’s independent update channel. |
| Medium | `ConfigFile::save_to_file` overwrites the existing JSON directly, and `app.rs::persist_config` discards its error. A failed write can lose changes or leave a partial config without UI feedback. | Write a sibling temporary file, atomically replace the config, and surface failures. Cover malformed config recovery and preservation of the existing file. |
| Review | Android Cockpit exposes multiple device operations through `tauri_app.rs::adb_action` and renders device data in `web/app.js` templates. The local asset CSP and escaping helpers are present. This audit did not establish a renderer injection or privileged command bypass. | Review every dynamic HTML interpolation and action handler against an explicit operation allowlist. Validate device and path at the Rust boundary. Include device names, messages, filenames, and stale device selection in fixture coverage. |
| Review | Systemd already validates unit names and drop-in names, and has source tests covering shell and path rejection. It still crosses a privileged file boundary through `write_privileged_unit_content`. | Add adversarial path and symlink test coverage when a Linux service sandbox is available. Keep Windows service capabilities explicitly separate. |

Scripts and saved commands intentionally run shell code selected by the user. Treat imported scripts, saved commands, and autostart configuration as executable content. Show the full target and command before enabling automation for a newly imported record.

## CPU and memory audit

| Priority | Evidence | Change to measure |
|---|---|---|
| High | `app/ssh/terminal.rs::render_remote_servers_right_panel` builds a layout job for each retained log entry while holding the log mutex. Producers need the same lock. | Cache rendered or measured entries by revision and viewport width. Paint visible rows without holding the writer lock. Measure typing latency and log throughput during a burst. |
| High | `app/server_detail.rs::render_server_log_content` deep-clones the log deque into a vector every render even though visible rows are virtualized. Searches also normalize strings while rendering. | Cache an immutable snapshot until the log changes, and cache matching indexes by query and revision. Preserve wrapping, selection and copy output. |
| High | `service.rs::Service::new` reserves 2,000 log entries for every configured server, including those that never run. Log entry count caps exist but are not uniform byte budgets across all subprocess output paths. | Allocate log storage on demand. Measure idle memory with many configured services and enforce a shared byte budget so unusually long output cannot dominate memory. |
| Medium | SSH listener rendering clones the entire listener snapshot and then clones filtered rows. Stopped rows are pruned in the rendering path. `app/ssh/connection.rs` has a second delayed scan path in addition to the normal scan gate. | Publish immutable snapshots by generation, filter by reference or index, coalesce scans per host and discard stale results. |
| Medium | The Commands and Systemd lists already virtualize rows. Commands still scans the catalog and rebuilds filtered indexes on rendered frames. Systemd logs and dependency results need their own payload limits. | Cache results by query, host, scope and catalog revision. Retain the existing fixed-height virtualization. |
| Medium | `sensors.rs` refreshes process lists and has per-PID reads. Top Apps, hardware inventory, GPU, Wi-Fi and Bluetooth have different collection costs. Parts of the collectors already gate expensive work by selection and cadence. | Log elapsed sample time and sample size by collector. Compare Overview and Top Apps at the same interval before changing refresh policy. Keep slow inventory and fast utilization sampling separate. |
| Medium | Android screen preview schedules screenshots every three seconds on the active screen tab in `web/app.js`. Device media paths hold image data and rebuild some DOM trees. The desktop opens a separate Cockpit process. | Measure combined parent and WebView memory. Prevent overlapping captures on slow ADB links and verify caches evict on device or tab changes. |
| Medium | Host-scoped maps in `LauncherApp` can retain logs, inputs and snapshots after host deletion. The removal code in `app/workspace.rs` removes the host record but does not clear all related maps there. | Give host removal a single cleanup path and prevent late workers from recreating deleted entries. Measure memory after repeated add/connect/remove cycles. |

For a repeatable baseline, record 30 seconds each for idle Servers, scrolling Overview, a busy terminal, Sensors Overview, and Top Apps. Use the same window size, log volume and refresh interval before and after changes. Record CPU time over the interval, RSS on Linux or working set and private bytes on Windows, and frame time or input delay. Include child SSH and Cockpit processes separately. No active launcher process was observed during this audit, so there are no measured before-and-after CPU or RAM claims.

## Cleanup candidates

1. **Legacy Android egui Cockpit.** `adb_view.rs::render_adb_cockpit_view` has no call site in the inspected Rust source. The current desktop routes through `render_adb_tauri_handoff`. Trace shared mirror and device helpers separately before deleting the old view and its state.
2. **Silent host options.** Fix the strict-host-key and agent settings or remove the controls. A visible control that silently does nothing is worse than an explicit unsupported state.
3. **Suppressed dead code.** Review the isolated `allow(dead_code)` items in `remote.rs`, `app.rs`, and `modals.rs` before deletion. Keep target-specific entry points that a Linux-only check cannot see.
4. **Duplicated platform UI.** Both crates now have the same module boundaries. Compare each pair before sharing files. Start with byte-identical pages and leave service, process and sensor collectors platform-specific.

## Suggested work order

1. Protect SSH passwords and centralize SSH argument and host-key handling. Verify configuration migration with dummy credentials.
2. Stop retaining removed host data, add command lifecycle limits, and remove unnecessary log allocations. Benchmark each change separately.
3. Fix process identity checks and audit privileged systemd and ADB operations against disposable fixtures.
4. Remove proven dead view code and consolidate identical UI modules only after Linux and native Windows builds agree.

The moved Rust modules were checked with `cargo check --locked --offline --tests` in both crates on Linux. This compiles test code but does not run it. Native Windows, UI rendering, dependency advisory scans, and runtime profiling were outside the verification performed here. The report does not claim a clean bill of security.

## Remediation progress

- `f7de58e` stages configuration writes before replacing the previous file and reports save failures. External SSH launches no longer put passwords in temporary scripts or interpolate the destination into a Windows command shell. Remote power actions send sudo credentials through SSH stdin. Stopped services allocate log storage on demand.
- `5048742` saves and enforces the per-host host-key and agent options. Existing records retain their previous defaults.
- `4b85f44` shares immutable log text across snapshots and moves SSH terminal layout out of the log lock. The terminal still lays out all retained rows. A runtime profile is needed before assigning CPU or memory savings.

Android Cockpit is deferred by request. The desktop now migrates legacy SSH passwords on startup using Linux Secret Service or Windows DPAPI. JSON retains a credential reference or protected blob, and migration failure leaves the previous file in place. Existing backups made before migration are outside the app’s control. Remaining work also includes SSH command lifecycle and host removal cleanup, trusted update signing, and runtime profiling. External password authentication now prompts in the terminal instead of copying the saved credential into a launcher script.

Local Overview now requires a captured process start identity for Stop. On Linux, Restart verifies fresh launch details before stopping the process and refuses to relaunch while the process or port is still active. Windows currently exposes verified Stop only in Other processes.

SSH Servers Stop and Kill now use the existing guarded process signaling backend. The scan captures Linux process start markers before collecting sockets, and the SSH lockout confirmation compares those markers. Listener rows without an observable PID and start marker have Stop and Kill disabled. Restart now verifies the listener against the systemd service’s main PID and process start marker. It excludes SSH listeners and uses noninteractive sudo on the remote host. Unused batch Stop and Restart builders were removed.

SSH launch and retry now share one command builder. Service operands are validated, labels and working paths are quoted as data, and retries retain the configured working directory. A failed explicit directory change aborts the launch. Background process startup remains asynchronous.

Both SSH host deletion entry points now share cleanup for connection status, listener scans, saved stopped rows, terminal state, search and cached sudo input. Delayed and immediate scans share one per-host gate, and an in-flight listener scan or connection test cannot publish after deletion. Other remote workers and SSH subprocess lifetimes still need review.

SSH connection probes and port scans now have 10-second and 20-second subprocess deadlines after spawn. Expired probes release their worker slots, and Unix cleanup also terminates the local SSH wrapper process group. Interactive SSH commands retain their existing lifecycle.

The Add/Edit SSH Host dialog now runs Test Connection on a background thread. The form shows pending feedback, accepts Cancel or Escape, and discards results from closed dialogs. The test uses the existing SSH probe deadline.

SSH terminal output now lays out selectable labels only for visible rows. Explicit line breaks are preserved, and long lines scroll horizontally. The retained log limit is unchanged. CPU and memory impact still need runtime measurement.

SSH Servers now shares each completed listener scan as an immutable snapshot. Search borrows rows, and successful scans prune stopped entries by port. The renderer still hides a stopped entry while the same port appears in its current listener snapshot.

Local server logs now reuse a single active-view snapshot and cache matching row indexes until the log or filter changes. Display and retained log limits are unchanged. Runtime CPU and memory impact remain unmeasured.
