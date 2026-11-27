# RDP/VNC Service Manager — implementation roadmap

This is an execution guide for implementing the service manager described in the accompanying request. Follow the stages in order, keep each commit small, and do not skip the safety gates. Scope is the Rust application under `server_launcher_rust/`; leave the Python editions unchanged.

## 1. Repository rules and starting point

- Stay on `vnc/rdp` (or the established `vnc-rdp` fallback). Never switch to or push/merge into `main`, `design`, or `fix/ui-alignment`; never force-push.
- Before each stage, inspect `git status --short --branch` and the relevant files/callers. Preserve unrelated working-tree changes.
- Use the repository's current compile gate: from `server_launcher_rust/`, run `cargo check --locked --all-targets`. Workspace instructions prohibit `cargo build`; do not run it. Run other checks only when the current workspace instructions and task explicitly permit them.
- Commit only after the applicable compile gate passes. Use one Conventional Commit per stage, with a 2–5-line body. Do not push unless separately authorized.
- Keep legacy `servers.json` readable. New serialized settings must be optional/defaulted; do not rename/remove existing keys. Do not edit Python files.
- The feature must stay idle until the RDP/VNC editor is opened or the user requests a scan/action. Do not add a polling loop.

## 2. Existing implementation to reuse

Read these files before editing their areas; use existing helpers instead of adding parallel systems.

| Need | Existing code |
|---|---|
| RDP/VNC editor state and tab rendering | `src/modals.rs`, `src/remote_desktop/editor.rs` |
| Current SSH listener scan and async repaint pattern | `src/remote_desktop/probe.rs` (`ServerDetection`) |
| Direct/tunneled protocol handshake checks | `src/remote_desktop/probe.rs` (`ProbeRequest`, `ProbeControl`, `run`) |
| SSH command construction, bounded output, cancellation | `src/remote.rs` |
| SSH local forwarding and credential handling | `src/remote_desktop/tunnel.rs` |
| RDP/VNC profile serialization/defaults | `src/remote_desktop/profile.rs`, `src/config.rs` |
| RDP/VNC credential-store access | `src/remote_desktop/credentials.rs` |
| Existing privileged service command path and sudo UI | `src/systemd.rs`, `src/modals.rs`, `src/app.rs` |
| Theme colors and status colors | `src/theme.rs` |

Current detection is a single raw `ss`/`ps` dump shown from modal state. Current `ProbeRequest` already verifies TCP and protocol handshake, but explicitly does not authenticate. Current SSH tunnel targets `127.0.0.1:<port>`; service detection must pass the selected listener address through the tunnel path when the target binds to a specific address. Inspect all callers before changing that API.

### Important existing behavior to audit before reuse

The app has a sudo dialog and a remote sudo password cache used by existing operations. The service manager must not silently inherit session caching if the user approves only one operation. Inspect how the cache is cleared and how the password reaches the child process. For this feature, keep the sudo password scoped to one approved operation, pass it only through stdin, and wipe/drop it immediately afterward. Never put it in host config, argv, environment variables, output, or setup logs. If the existing dialog/cache cannot provide that guarantee without broad regressions, add a narrowly scoped operation mode or stop at passwordless sudo and provide a terminal/manual path. Do not weaken the existing SSH credential flow.

## 3. Product and safety boundaries

1. **Scan is read-only.** It may inspect distro, packages, units, processes, listeners, and resource hints. It never installs, edits, enables, starts, stops, restarts, or changes firewall/security-group rules.
2. **Presence is not readiness.** A process or listener only establishes a partial status. Show `OK` only after the current client completes the protocol handshake. Login is a distinct optional check and is never implied by a handshake.
3. **Protocol data stays separate.** The RDP card shows xrdp-related checks only; the VNC card shows TigerVNC-related checks only. A shared inventory scan may collect both protocols once, then each tab projects its own result.
4. **Unsupported systems are manual-only.** Non-Linux, unknown init/package manager, unverified distro, unsupported service layout, missing sudo rights, and ambiguous process ownership must show the evidence and safe manual instructions. Do not guess a package, unit, config syntax, or command.
5. **Every mutation is reviewed.** Build an explicit action plan from closed enums and validated typed fields. Display the exact commands/effects before the user approves. No firewall opening is ever offered.
6. **Treat remote output as hostile.** Bound output size, escape it in UI, redact secrets, and never evaluate data returned by the server.

## 4. Target data flow

```text
RDP/VNC tab opens or Scan again
  -> async SSH inventory scan (one request; cache by host/settings for <=60s)
  -> parse versioned SLD/1 records into a protocol-neutral inventory
  -> project RDP or VNC ServerStatus for that tab
  -> async existing ProbeRequest for TCP + protocol handshake
  -> UI shows independent checks and a precise status

User clicks a mutating action
  -> pure ActionPlan built from ServerStatus + typed options
  -> approval dialog displays every step/effect/command
  -> optional operation-scoped sudo prompt only if privilege check requires it
  -> execute approved steps asynchronously, emitting bounded progress events
  -> rescan + existing protocol handshake probe
  -> update profile port/display only when automatic detection/action justifies it
  -> record redacted local change log and offer Undo for app-owned changes
```

Use one worker request at a time per host. Return events over `std::sync::mpsc`; poll receivers without blocking the egui thread and call `ctx.request_repaint()` only after an event or bounded progress update. Keep egui types out of inventory and plan models.

## 5. Milestones

Each milestone is a separate green commit. Do not combine scan, mutation, and UI work into a single commit.

### Stage 0 — Baseline and source verification

**Read:** `probe.rs`, `tunnel.rs`, `editor.rs`, `modals.rs`, `profile.rs`, `config.rs`, `credentials.rs`, `systemd.rs`, `theme.rs`, and current `docs/remote-desktop/DESIGN.md` / `TESTING.md`.

**Do:** confirm branch/clean status; map the RDP/VNC modal call sites and all tunnel callers. Confirm the sudo password cache lifecycle. Identify the exact distros/packages/units that can be verified from authoritative distro docs or disposable hosts. Record unknowns as manual-only; do not fill in facts from memory.

**Done when:** this roadmap and a source list are reviewed; no code changed. Begin implementation only after the maintainer asks to execute the roadmap.

### Stage 1 — Service-manager design record and supported-platform matrix

**Files:** `docs/remote-desktop/SERVICE_MANAGER.md` (new).

**Record:** state meanings; `SLD/1` output contract; verified distro/package/unit/config facts and source links; manual-only cases; sudo decision; no-firewall rule; Python config compatibility note; risks and test matrix. Mark a distro automatable only after package name, service unit, startup/config requirements, and rollback path are verified. In particular, do not assume xrdp listen-address syntax or TigerVNC systemd unit mapping.

**Gate:** doc review only. Do not claim a distro is runtime-verified based only on documentation.

### Stage 2 — Typed inventory and parser fixtures (read-only)

**Files:** add the smallest module structure needed under `src/remote_desktop/setup/` (for example `mod.rs`, `model.rs`, `detect.rs`); add Rust fixtures/tests beside the parser or in the existing test layout.

**Implement:** typed, egui-free structures for `Distro`, `ServiceState`, `Exposure`, listener, VNC display, check result, and per-protocol status. Parse a versioned line protocol (`SLD/1 key=value`) with bounded field sizes and explicit unknown values. Preserve raw output separately for troubleshooting, but do not parse arbitrary raw lines in UI code.

**Script output should include:** OS-release identifiers, init/systemd availability, SSH user/uid and sudo capability, package manager, verified package/binary versions, unit active/enabled/failure tail, listener address/port/process/pid/owner when available, xrdp configured port, VNC display/port/localhost flags, desktop session presence, resource hints, and read-only firewall hints. Missing `users:(...)` process fields are normal and must produce `owner/process unknown`, not a failed scan.

**Fixtures:** include the supplied `ss` example (3389 without process, Xtigervnc at 5901), wildcard IPv4/IPv6, `*`, localhost, specific bind, multiple VNC displays, no systemd, missing `ss`, malformed/truncated records, and unknown keys/version. Ensure `:1` maps to 5901 only when the detected server/display data supports that mapping.

**Done when:** parser behavior is deterministic, bounded, and covered by fixtures; no remote mutation exists in this stage.

### Stage 3 — Async read-only scan integration

**Files:** `setup/detect.rs`, `probe.rs`, `modals.rs`; optionally a narrow `SshRunner` seam if needed to test scan orchestration without the network.

**Implement:** replace raw-dump-as-primary UI with one cached inventory per host key and a protocol-specific projection. Start the scan on opening the protocol tab, once per modal session; cache successful and failed results for at most 60 seconds. `Scan again` invalidates the cache. Invalidate on host, port, username, identity, or SSH credential changes. Use the existing SSH command/timeout/output-limit helpers. Do not poll in the background.

**Probe:** after a listener/port is known, run the existing RDP X.224 or VNC RFB handshake asynchronously using the selected profile route. Report handshake separately from optional authentication. Update tunnel routing so loopback/wildcard uses `127.0.0.1`, while a verified specific non-loopback bind uses that address. Preserve all existing tunnel callers with the current default behavior.

**Done when:** RDP and VNC cards show only their own service; scan never changes the host; process owner may be unknown; `OK` requires handshake success.

### Stage 4 — Status derivation and focused status tests

**Files:** `setup/model.rs` or a small `status.rs`; parser tests.

**Implement pure state derivation:**

| Evidence | Derived state |
|---|---|
| SSH scan failed/not configured | Can't check; point to SSH test |
| Unsupported OS/init or incomplete evidence | Limited detection / Unknown with reason |
| No verified installed binary/package | Not installed, unless evidence is incomplete (then Unknown) |
| Installed; unit inactive and no matching process | Installed, not running |
| Failed unit | Failed plus capped log tail |
| Matching process/listener without active unit | Running, manual process |
| Active unit or matching process and listener, but handshake failed | Running, not reachable + exact failed stage |
| Handshake succeeded | OK (login not tested) |
| Optional login succeeds/fails | Explicit login result, separate from handshake |

Exposure derives from listeners; do not infer process owner from port alone. Keep source evidence alongside summary for tooltips/raw output.

**Done when:** one focused test covers every row plus ambiguous/missing evidence; no UI state is needed to test derivation.

### Stage 5 — Service cards, status dots, and manual guidance shell

**Files:** `remote_desktop/editor.rs`, `modals.rs`, `theme.rs` only if an existing token is truly missing.

**Implement:** reusable visual card layout with protocol-specific heading, fixed-height checking placeholder, status chip/accent from existing palette, compact check rows, scan button, hidden-by-default raw output, Copy action, and collapsed manual instruction sections. Add status dots to the RDP/VNC tab labels from the same model. Add VNC display selector only when more than one verified display exists. Show manual-process ownership note when unit management does not own it. Avoid duplicating status logic in widgets.

**Done when:** no raw `ss` dump dominates the card; no horizontal overflow; all themes use palette tokens; keyboard focus order works.

### Stage 6 — Manual instruction generator (verified targets only)

**Files:** `setup/manual.rs`, `SERVICE_MANAGER.md`, service card UI.

**Implement:** generate numbered start/install/verify guidance only for documented distro families. Commands are copyable and parameterized from validated settings, with no hidden execution. Include cloud firewall/security-group note and SSH-tunnel guidance. Add “I did it manually, scan again.” Unknown distro gets explicit unknown/manual status, not a guessed command.

**Done when:** generated text has snapshot/expectation tests for each verified distro/protocol and no unverified package/service names.

### Stage 7 — Pure action plan and validation (still no execution)

**Files:** `setup/plan.rs`, `setup/model.rs`, tests.

**Implement:** closed `Intent` enum (`Start`, `Stop`, `Restart`, `InstallAndStart`, `EnableAtBoot`, `RestrictToLocalhost`, optional desktop install); typed options; `ActionPlan` with fixed step ids, description, exact display command, privilege requirement, optional/recommended flag, and undo note. Select plans only from a verified distro capability table. Reject malformed username, port, display, geometry, VNC target user, and control/newline characters. Never build commands by interpolating arbitrary UI text; pass validated values as quoted positional arguments.

**Dry-run:** plan construction only; package-size estimates are optional and must be labeled unavailable unless supported by a verified, bounded package-manager simulation. Do not invoke dry-run package tools from the egui thread.

**Tests:** hostile usernames/paths (`;`, quotes, `$()`, backticks, newline), valid boundary ports/displays, and each intent/capability combination.

**Done when:** plan is deterministic and pure; no SSH command can run from the planner.

### Stage 8 — Approval and async progress UI (no privileged executor yet)

**Files:** `modals.rs`, `app.rs` only for wiring, plus a small operation state in `setup/`.

**Implement:** Action Plan dialog with exact commands, affected packages/files/units/ports, backups, warnings, Cancel default, and Run disabled for invalid required values. On Run, transition to progress state; show step markers and bounded output tail. Closing while a mutation runs is blocked, with explicit cancel. Cancellation occurs between safe steps, not in the middle of an irreversible package manager command. Add notifications for completion/failure through the existing notification path.

**Done when:** dialog can display a fake plan/progress sequence without running a remote mutation.

### Stage 9 — Privilege decision and safe command execution

**Files:** `setup/sudo.rs`, `setup/exec.rs`, existing `systemd.rs` only if a reusable narrow helper is needed.

**First:** read-only check whether remote user is root, passwordless sudo works, sudo is missing, or rights are unavailable. Do not prompt preemptively.

**Execution:** use the existing SSH transport, with strict argument/script handling, bounded streamed output, per-step timeout, and cancellation at step boundaries. If remote sudo password is required, present a feature-scoped masked prompt after the exact action plan is approved. Validate with one cheap privileged command; allow at most three attempts. Password must be in-memory only for that operation, stdin only, zeroized/dropped immediately, never session-cached or saved. Ensure sudo and script payload do not compete for stdin: deliver a private temp script separately, execute with password as stdin, then remove temp script on success/failure/cancel. If these properties cannot be guaranteed, stop and show terminal/manual instructions.

**Progress:** parse only the app's own `##SL` markers; treat ordinary remote output as text. Report partial completion exactly. Add per-step timeout and output caps. Never continue with steps the user did not approve.

**Done when:** fake-runner tests prove event order, failure, cancellation, truncation, and no password in command construction/output; source audit confirms no secret logging.

### Stage 10 — Start/stop/restart and enable-at-boot

**Files:** `setup/scripts/` only for verified shell scripts, `setup/exec.rs`, plan tests, service UI.

**Implement:** systemd actions only for verified unit names and hosts with systemd. For manual VNC, offer restart only after a second confirmation names the PID and owner; never kill an ambiguous process. Back up every edited file before edits. Keep start/stop distinct from enable-at-boot. After each operation, re-run the read-only scan and protocol handshake automatically.

**Done when:** restart/start/stop/enable actions are individually plan-gated, idempotent where possible, and their post-state comes from scan+handshake rather than command exit alone.

### Stage 11 — Install-and-start for verified distributions

**Files:** `setup/scripts/install_xrdp.sh`, `install_tigervnc.sh`, `manual.rs`, docs and tests.

**Implement one verified distro family at a time.** Stage apt/dnf update/install steps separately and let the approval dialog list each one. Handle package locks with a timeout, noninteractive mode, package-not-found, no network, and low disk/RAM as explicit outcomes. xrdp needs both expected units and a usable desktop session; desktop installation is a separate optional approved step. Warn on root login. TigerVNC needs a chosen non-root user, an existing or explicitly approved desktop startup script, and a VNC password. Password setup must avoid argv and shell history. Classic VNC password truncation/unencrypted transport warnings must be explicit; localhost-only plus SSH tunnel is the default where supported.

Do not support automated install on Arch for xrdp or any unverified distro. Provide manual steps only. Never claim the package operation succeeded until the rescan and handshake succeed.

### Stage 12 — Port/display assignment and safe bind restriction

**Files:** `setup/ports.rs`, `setup/scripts/restrict_*.sh`, `profile.rs`, `config.rs`, `editor.rs`, tests.

**Port rules:**

- Running xrdp: prefer the actual xrdp listener. Fresh install: use 3389 unless occupied by a different process; choose next free only after verifying the config syntax for installed xrdp version and building a backup-first plan.
- TigerVNC: derive 5900 + display only when matching process/socket evidence identifies the display. Pick the lowest verified unused display from `:1`; include listeners and X sockets in the check. Prefer configured user ownership; otherwise show choices.
- Add `#[serde(default)]` optional auto/manual indicators and VNC display/user/resolution only after confirming the current profile serialization tests. Manual edits clear auto provenance. Do not silently overwrite a user-edited port.
- If detected and configured ports differ, show an Apply button. Auto-apply only after an app-run install/start action, and emit a notification.

**Exposure:** offer localhost restriction only if the syntax and service behavior are verified for this host. Explain direct connections will stop working. Require SSH tunnel or explicit acknowledgement. Never edit host/cloud firewall rules. For VNC, use `-localhost yes` only with the verified server invocation/config path. For xrdp, if the installed config syntax is unknown, leave manual-only.

### Stage 13 — Change log and conservative Undo

**Files:** setup state/log module; use existing application state-directory helpers.

Store a bounded, redacted local record outside `servers.json`: host id, timestamp, verified distro, app-installed packages, files changed plus backup path, and units started/enabled. Do not store passwords or raw unbounded logs. Undo only reverses app-owned service/config changes; do not uninstall packages by default. Show the exact undo plan and require approval again. If backup is missing or host identity changed, disable Undo and explain why.

### Stage 14 — Failure diagnosis and polish

Map known, evidenced failures to concise messages: SSH auth/timeout, no listener/refused, timeout/firewall hint, wrong protocol, unsupported handshake/security type, auth rejected, apt lock, package unavailable, no sudo rights, missing desktop/black screen, port collision, SELinux denial, cancellation, and partial completion. Keep raw details expandable. Do not label timeout as firewall certainty; phrase it as a possible network/security-group block.

Check three theme variants (Nocturnal Command, Light, Catppuccin Latte) and narrow window sizes. Ensure all actions have tooltips, accessible labels, predictable keyboard focus, and no layout jump while scanning.

### Stage 15 — Compatibility, docs, and final gates

**Files:** `config.rs` tests, `SERVICE_MANAGER.md`, `TESTING.md`, `README.md`.

- Add an old-config deserialization → save → reload compatibility check, including omitted new fields and existing `ssh_hosts` keys.
- Read the Python implementations read-only to determine whether they preserve unknown Rust fields or drop them when rewriting `servers.json`; document the observed behavior, do not edit Python.
- Update manual test matrix with OS/distro/version, client route, service/unit, expected status, observed result, and whether it was docs-only, compile-checked, or runtime-tested.
- Follow repository rules for compile/check/commit. Do not report runtime success from parser fixtures or `cargo check`.

## 6. Per-commit checklist

For every implementation commit:

1. Inspect `git status --short --branch`; read all callers of touched APIs.
2. Make one logical change and its focused fixture/test updates.
3. Run `cargo fmt --check` only if permitted by current workspace rules; the mandatory repository compile gate is `cargo check --locked --all-targets` (never `cargo build`).
4. Review `git diff --check` and the staged diff; confirm no secrets, Python edits, unrelated files, or generated artifacts.
5. Commit with Conventional Commit and a short body stating the change and why.
6. Record evidence honestly: source-inspected, compile-checked, unit-tested (only if tests were permitted and run), or runtime-tested against a named authorized host.

## 7. Required tests and acceptance checklist

Automated checks should cover parser fixtures, each state transition, unknown/missing evidence, port/display selection, tunnel target, strict option validation/quoting, plan capability selection, config backward compatibility, fake-runner progress/failure/cancel, and script output expectations. Run only those test commands allowed by repository instructions.

Manual checks on an authorized disposable/test host:

- xrdp active at 3389, handshake passes through tunnel, wildcard bind warning shown.
- TigerVNC manually running at `:1`/5901, process may lack a unit, owner/display shown, port suggestion correct.
- Missing service, installed-stopped service, failed unit, no systemd, unknown distro, missing sudo, passwordless sudo, wrong sudo password, root SSH user, no desktop, low disk/RAM, port collision, VNC `:2`, and cancellation.
- Every mutation has explicit approval, exact effect list, backups where applicable, bounded progress, post-action rescan and handshake. Failure reports completed steps and manual next steps.
- No firewall/security-group changes, no password in config/log/argv/env, no silent changes, and no duplicate periodic scans.

Acceptance is per protocol: only that protocol's service is shown; read-only scans never mutate; unknown systems remain manual-only; `OK` requires a successful handshake; profile ports are not silently replaced; and all supported writes are explicitly approved and auditable.

## 8. Scope deliberately held back

Do not add automatic firewall/security-group edits, guessed distro support, unattended install, package uninstall as Undo, generic remote command editing, background polling, or a new credential store. These are outside the requested safe service manager and increase risk without helping its core workflow.
