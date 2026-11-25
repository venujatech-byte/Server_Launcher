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

- [x] **2. Verify command cancellation and cleanup on exit.** Inspect `app/ssh/terminal.rs::run_remote_ssh_command`, `cancel_remote_commands`, and both projects' app close path. Keep per-host isolation, the four-command limit, and all three inactivity modes. Confirm that the tracked local child is reaped on cancel and normal exit, and trace the sshpass wrapper and output reader handles separately. Apply a lifecycle fix only where the check demonstrates a gap. Use a benign local child fixture or a test host you control. Cover a second host continuing while the first is canceled, and a new submission after cancellation. Preserve external interactive SSH sessions. Commit verified changes as `fix(ssh): clean up owned terminal jobs on exit`. Stage the touched `app/ssh/terminal.rs`, close-path and process-helper files in both projects plus the audit Markdown files. The lifecycle test is compile-checked in both crates but not executed because this repository requires `cargo check` only; native runtime child-reaping remains unverified.

- [x] **3. Harden launcher process record writes.** Inspected `service.rs::pid_file`, `read_process_record`, and `write_identity` in both projects, plus process record removal and external-console bootstrap paths. PID paths now reject empty, traversal, separator, control, and Windows-reserved keys. Structured JSON records are written to an owner-only same-directory temporary file, synced, and atomically replaced; legacy numeric PID files remain readable and the external-console marker retains its no-clobber behavior. Attachment still requires the existing command/cwd or exact process identity checks. Source regression tests cover invalid keys, valid legacy decoding/migration, and an abandoned partial staging file leaving the old marker intact. Both `cargo check --tests` commands compiled the regression tests; tests were not executed under the repository's cargo-check-only rule. Native Windows ACL behavior and crash/power-loss behavior remain runtime-unverified. Commit `fix(servers): protect persisted process identity records`.

- [x] **4. Review privileged Systemd writes.** Local and remote unit create/edit/delete paths now validate their Systemd configuration roots: system units are restricted to `/etc/systemd/system`, and user units to the user config subtree. Vendor directories and traversal are rejected. Local edits reject symlink destinations and verify canonical parent containment; writes stage content and atomically replace the target so failed staging preserves prior content. Remote edits also stage in the destination directory before replacing, and new remote units use no-clobber linking. User-scope write/delete failures no longer invoke sudo; system-scope writes keep the existing password prompt and stdin transport. Windows Services handling is unchanged. Regression source covers allowed/denied roots, outside-file preservation, and local symlink rejection. Both test targets compile but tests are not executed. Native sudo, remote-host, and Windows runtime verification remain outstanding. Commit `fix(systemd): constrain privileged unit writes`.

- [x] **5. Measure before further performance edits.** Recorded all five desktop scenarios, repeated Overview scrolling, and confirmed the release Top Apps baseline across two 30-second runs at 1-second refresh. See APP_AUDIT.md for samples and limitations.

- [x] **6. Fix one measured bottleneck per commit.** Commit `f074a68` skips task enumeration in local Linux Top Apps. Two post-change RPM runs averaged 4.13–4.17% CPU versus 9.60–9.80% before. The repeat syscall trace found zero explicit task-path accesses. See APP_AUDIT.md for measurement details and remaining platform limits.

### Stage 6 candidate: Linux Top Apps process enumeration

The release Top Apps baseline at a 1-second refresh is 9.60–9.80% of one core. The filesystem trace points to thread enumeration. Sysinfo 0.34 introduced `ProcessRefreshKind::without_tasks()`. See the [upstream migration guide](https://docs.rs/crate/sysinfo/0.34.2/source/migration_guide.md).

1. Implemented the 0.34.2 migration in both desktop crates. Search all `sysinfo` call sites and preserve each refresh's dead-process removal behavior. Keep the Linux Top Apps switch scoped to the dedicated process refresh in `src/sensors.rs::collect_with_top_apps`. Preserve TCP cadence, PID start markers, and CPU, RAM and disk metrics.
2. Compile both crates with the roadmap checks and verify the Windows target through a native runner when authorized. Record package source revision and build settings for both Linux comparison binaries.
3. Repeat two 30-second Top Apps runs per build at 1-second refresh with comparable process counts. Use a separate syscall trace to verify the task enumeration reduction. Report CPU and RSS from the untraced runs. Keep Stage 6 open until the comparison is available.

## Deferred decisions

Release authenticity is still an audit finding. Independent signing requires a separately managed trust root and an explicit release-policy decision. Preserve the completed updater and packaging as requested. Android Cockpit remains deferred.

## Handoff to Luna

Start with Stage 1. Before each edit, search the named symbols and confirm every caller. After each stage, report the commit, exact checks that ran, and any runtime behavior still unverified. Continue with the next independent stage.
