# Desktop audit continuation for Luna

Baseline commit `0e6396a` on `fix/ui-alignment`. Read [APP_AUDIT.md](APP_AUDIT.md), including its remediation notes, before starting. Its original findings describe the earlier baseline. The remediation notes and current source take precedence.

## Execution rules

- Verify `git branch --show-current` and `git status --short`. Stay on `fix/ui-alignment`. Preserve unrelated edits and stage explicit paths.
- Work in order. Apply shared fixes to `server_launcher_rust/src/` and `server_launcher_windows/src/` where both implementations exist. Keep native platform behavior, configuration compatibility, and SSH action bindings.
- Keep Android Cockpit and the completed updater, packaging, and release workflows out of this sequence. Keep changes local. Leave GitHub CI and publishing idle.
- Use Ponytail. Reuse the current workers, guards, and UI. A small reproduction-free regression test or local fixture should cover each new state transition. Use only `cargo check` for Rust compilation.
- Run the following checks after each code step. They compile test targets on the current host. Record native runtime checks separately.

```sh
git diff --check
cargo check --manifest-path server_launcher_rust/Cargo.toml --locked --offline --tests
cargo check --manifest-path server_launcher_windows/Cargo.toml --locked --offline --tests
git status --short
```

Each stage below has one suggested commit. Add the verified result and any remaining limitation to `APP_AUDIT.md` in that commit. Mark the stage complete here only when its acceptance checks hold. Stop work on an individual stage when native evidence or a product decision is required, record what is missing, and continue independent stages.

## Planned fixes

- [x] **1. Finish SSH host-removal cleanup.** Start with `app/ssh/connection.rs::delete_ssh_host` and the host-scoped maps in `app.rs`. Trace each actual producer of remote Sensors, Bluetooth, and Systemd caches in `app/sensors.rs`, `app/sensors/bluetooth.rs`, `app/sensors/processes.rs`, `app/systemd.rs`, and any result handlers identified by `rg`. Use each cache's actual key or its existing key builder for composite keys. Clear per-host entries and ensure removal and late result publication agree on the same lifetime check. A result for a deleted or replaced host must be dropped even if the same identifier is later reused. Use an owned lifetime token only where the existing in-flight ownership cannot establish that. Keep local and other-host records intact. Verify with synthetic completion data after deletion and after reuse of the same host ID. Commit `fix(ssh): discard host-scoped results after removal`. Stage the touched producer and cleanup files in both projects plus the two audit Markdown files. The regression test was compile-checked in both crates; test execution is omitted because this repository requires `cargo check` only.

- [ ] **2. Verify command cancellation and cleanup on exit.** Inspect `app/ssh/terminal.rs::run_remote_ssh_command`, `cancel_remote_commands`, and both projects' app close path. Keep per-host isolation, the four-command limit, and all three inactivity modes. Confirm that the tracked local child is reaped on cancel and normal exit, and trace the sshpass wrapper and output reader handles separately. Apply a lifecycle fix only where the check demonstrates a gap. Use a benign local child fixture or a test host you control. Cover a second host continuing while the first is canceled, and a new submission after cancellation. Preserve external interactive SSH sessions. Commit verified changes as `fix(ssh): clean up owned terminal jobs on exit`. Stage the touched `app/ssh/terminal.rs`, close-path and process-helper files in both projects plus the audit Markdown files.

- [ ] **3. Harden launcher process record writes.** Inspect `service.rs::pid_file`, `read_process_record`, and `write_identity` in both projects. Trace all reads, writes and removals, including the external-console bootstrap and numeric legacy markers. Reuse the atomic-file pattern in `config.rs`. Use a validated single-component server key and private storage for new structured records. Preserve read compatibility for valid old records and continue requiring process identity evidence before attachment or signals. Verify invalid keys, an interrupted write preserving the previous record, and valid legacy record handling with temporary fixtures. Commit `fix(servers): protect persisted process identity records`. Stage both `src/service.rs` files, any directly required existing helper file, and the audit Markdown files.

- [ ] **4. Review privileged Systemd writes.** Inspect `systemd.rs::write_privileged_unit_content` and its callers in both projects. Verify that application-owned unit writes remain inside the intended unit locations and use the existing sudo prompt and password transport. Distinguish writable configuration overrides from vendor files. Cover valid targets, rejected paths outside the intended roots, preservation of existing contents on error, and destination link handling with harmless file fixtures. Keep separate Windows Services behavior. Native privileged verification requires a disposable Linux environment. Commit a verified fix as `fix(systemd): constrain privileged unit writes`. Stage the touched Systemd backend and UI files plus the audit Markdown files.

- [ ] **5. Measure before further performance edits.** Use an existing desktop binary or a separately available verified package. Follow the 30-second scenarios in APP_AUDIT.md for Servers, busy terminal, Sensors Overview, and Top Apps. Record CPU, RSS or private bytes, sample cadence, and child process measurements separately. Repeat comparable runs before reporting a change. If usable binaries or native access are missing, record the gap and keep this stage open. Commit available measurements as `docs(audit): record desktop runtime baseline`. Stage only the audit Markdown files.

- [ ] **6. Fix one measured bottleneck per commit.** If terminal layout dominates, start in `app/ssh/terminal.rs::render_remote_servers_right_panel`. Preserve wrapping, line starts, selection, and user-controlled vertical scrolling. If sensor collection dominates, use the matching stage in [SENSORS_AUDIT_ROADMAP.md](SENSORS_AUDIT_ROADMAP.md) and inspect the current `src/sensors.rs` plus the applicable sensor panel. Preserve collection cadence and tests for unavailable data. Compare the same baseline before claiming a gain. Use `perf(ssh): reduce wrapped terminal layout work` or `perf(sensors): reduce measured collection overhead` as appropriate. Stage only the affected feature files and audit Markdown files.

## Deferred decisions

Release authenticity is still an audit finding. Independent signing requires a separately managed trust root and an explicit release-policy decision. Preserve the completed updater and packaging as requested. Android Cockpit remains deferred.

## Handoff to Luna

Start with Stage 1. Before each edit, search the named symbols and confirm every caller. After each stage, report the commit, exact checks that ran, and any runtime behavior still unverified. Continue with the next independent stage.
