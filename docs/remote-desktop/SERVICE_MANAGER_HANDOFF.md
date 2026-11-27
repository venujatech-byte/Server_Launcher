# Service Manager Roadmap — Agent Handoff

Use this handoff with [`SERVICE_MANAGER_ROADMAP.md`](SERVICE_MANAGER_ROADMAP.md). The roadmap remains the source of requirements; this file records the current implementation state and the next work to do.

## Repository state

- Branch: `vnc/rdp`
- Latest code change: `2c33446 feat(setup): add approved xrdp undo`.
- Latest Rust changes passed `cargo check --locked --all-targets` on the SSH PC in temporary worktrees.
- SSH build/check PC: use the connection details already provided by the user; do not copy its address, username, or credentials into repository files. Its clone is `~/coding/Server_Launcher`.
- The user has asked that checks run on that SSH PC and that changes be pushed and pulled there.

## Non-negotiable workspace rules

Read the repository `AGENTS.md` before coding. It requires **`cargo check` only** and explicitly forbids `cargo build`. This overrides the user's later request for a release build and binary copy; do not run a release build or copy a binary unless the repository instructions change.

For Rust changes, apply the patch to a temporary worktree on the SSH PC and run:

```sh
cd server_launcher_rust
cargo check --locked --all-targets
```

Do not run `cargo test` under the current `AGENTS.md` restriction. `--all-targets` type-checks test code but does not execute tests. Do not edit the Python editions. Stay on `vnc/rdp`, make one conventional commit per logical change, and do not force-push. The user has already authorized pushing this feature branch and pulling it on the SSH PC. After the final implementation is complete, ask the user to perform the requested manual checks in the app.

## Work completed and evidence

The following changes are on `vnc/rdp` and passed `cargo check --locked --all-targets` on the SSH PC using a temporary worktree:

- `e6c2b2d` and `8c6927f`: pinned viewer toolbar occupies a top panel; the panel sizes from its contents instead of using a tight fixed height.
- `1260f5d`, `165d69b`, `4955b1e`: service sudo is scoped to the approved operation; approval stays visible while running; cancellation after a final step is reported accurately.
- `92df98c`: successful start/restart/install triggers one post-action scan and may apply a detected port once.
- `5f637ae`: common service-action failures receive clearer messages.
- `dd79c28`: the legacy SSH host compatibility test now exercises `servers.json` file load → save → reload and verifies old SSH fields survive while omitted RDP/VNC profiles stay omitted.
- `6e12801`: VNC port selection prefers a detected TigerVNC display and can use its port when listener inspection is missing; it avoids preferring an unrelated listener over that display.
- `8f2adfd`: pure readiness derivation covers install/service/reachability/handshake states and drives the service-card summary.
- `3f8471e`: scan-triggered RDP/VNC handshakes use the matching listener's parsed bind address through SSH; wildcard and loopback listeners map to `127.0.0.1`, and tunnel defaults for regular sessions remain loopback.
- `8586999`: manual tunneled Test Connection runs a short read-only listener scan and uses the matching bind address, with the existing loopback fallback if no address is found.
- `65040af`: live RDP/VNC sessions scan once before opening an SSH tunnel, target the matching listener address, and fall back to loopback when no usable bind is detected.
- `e435b4a`: parser fixtures cover IPv6 wildcard, `*`, specific VNC bind, multiple displays, and truncated records.
- `64f11b2`: service cards use a theme-colored outline and status pill.
- `a525af7`: pending and unconfirmed service states use the warning color.
- `5b40df1`: host-wide OS, sudo, desktop, firewall, and resource details are collapsed by default in the service card.
- `d906063`: generated RDP verification commands use the selected profile port.
- `8d29e80`: planner tests cover the fixed commands for xrdp start, stop, restart, and enable-at-boot.
- `055c751`: first SSH inventory scans show a themed placeholder while keeping existing results visible during refresh.
- `63d8d97`: manual command groups offer a Copy all button.
- `9b72659`: exposure fixtures distinguish specific addresses from the localhost alias.
- `5869e1f`: SSH, installation, and service-state checks render in a compact three-column checklist.
- `203cf1e`: a source-level regression check keeps the `ss`, `netstat`, and `/proc` listener fallback branches present.
- `408a386`: successful and partially completed service actions write a bounded, private local history file with host identity and completed step IDs; it excludes credentials and remote output.
- `2c33446`: the service card shows recent host-matched actions and offers an approved xrdp restore only when the saved and current systemd states are known; package removal and VNC undo are excluded.
- `8a1eba7` and `c47f26f`: README and service-manager notes describe the current limits and checked evidence.

Tests were not executed. UI changes were not visually runtime-tested. The SSH-PC compiler check is evidence of compilation only; it does not verify remote xrdp/VNC behavior or execute detector fallback branches.

## Current implementation boundary

See [`SERVICE_MANAGER.md`](SERVICE_MANAGER.md) for package-source notes. In brief, the app has a read-only `SLD/1` inventory scan and RDP/VNC handshake probe, manual instructions for Debian/Ubuntu, and reviewed fixed-command xrdp actions for Debian/Ubuntu systemd hosts. Service changes run asynchronously and use operation-scoped sudo password handling. Bounded local history is visible, with approved xrdp state restore when prior/current systemd evidence is complete. TigerVNC mutations and unverified distro actions remain manual-only.

The latest readiness model distinguishes `Ready` (service state running and handshake succeeded), running without a tested handshake, running but unreachable, stopped, failed, not installed, limited detection, and unknown. A successful handshake alone does not imply the managed service is healthy when service evidence is absent.

## Remaining roadmap work

Do not claim the roadmap is complete until each required item is implemented and verified or explicitly documented as manual-only.

1. **Finish read-only scan/probe correctness (Stages 2–4).** Parser fixtures now include wildcard/IPv6 binds, `*`, specific binds, multiple VNC displays, and truncated records. A source-level regression check guards the `ss` → `netstat` → `/proc/net/tcp*` fallback chain, but it does not execute the shell script with mocked command availability. Keep scans read-only and on-demand; keep authentication/login distinct from handshake.
2. **Finish service-card details (Stage 5).** The card has a theme-derived status outline/pill, compact three-column SSH/install/service rows, collapsed host details, a first-scan placeholder, protocol-specific raw-output disclosure, manual-process details, and cached editor status dots. A VNC display selector appears only with multiple verified displays. Remaining: visual/keyboard checks across themes and narrow windows.
3. **Finish manual guidance (Stage 6).** Current Debian/Ubuntu command groups support per-command and Copy all actions, and RDP verification uses the selected port. Verify the remaining command details against authoritative sources before displaying them. Add other distro families only after package, unit, startup, and rollback facts are confirmed. Unknown or unsupported systems remain manual-only; do not generate guessed commands.
4. **Audit plans against the safety contract (Stage 7).** Plans currently use fixed static xrdp commands. Preserve the closed intent set, but complete explicit capability checks, evidence-based display/effect descriptions, and plan tests. Do not introduce user-controlled shell interpolation. Any future command argument must be validated and passed positionally.
5. **Complete action execution/progress (Stages 8–10).** The UI reports command-step boundaries, not live output/progress inside a package operation. Add bounded streaming only if it can reuse the SSH layer safely. Preserve operation-scoped sudo, exact approved commands, safe cancellation boundaries, bounded logs, and a post-action rescan. Test event order/failure/cancellation by type-checking fixtures; do not run remote mutations during coding checks.
6. **Install/start support (Stage 11).** xrdp install/start exists only for verified Debian/Ubuntu systemd hosts, and has not been runtime-tested by this app. Add lock timeout and explicit optional desktop setup only if exact commands and rollback are verified. TigerVNC start/install must remain manual-only until per-user service mapping and password provisioning can be made safe without argv, history, or config storage.
7. **Port assignment and bind restriction (Stage 12).** Port provenance is now explicit: auto-eligible profiles can accept a detected port, manual edits are preserved, and legacy profiles with unknown provenance keep their current port and receive an Apply suggestion. The behavior is compile-checked, not runtime-tested. Do not automate xrdp or VNC localhost binding until installed-version syntax, backups, rollback, tunnel routing, and behavior are verified. Never change firewall or cloud-security rules.
8. **Finish setup log and reviewed Undo (Stage 13).** A bounded local history file now records completed step IDs, prior service state, and host identity without passwords or remote output. The UI shows recent matching actions. Approved xrdp restore is available only with known prior/current systemd state; it does not uninstall packages. Remaining: verify log behavior and UX at runtime, and decide whether any additional inverse action can be made safe. VNC mutations remain manual-only.
9. **Complete diagnostics and UI polish (Stage 14).** Expand diagnosis only for observed evidence (SSH failure, refusal, timeout as a possible network/security-group block, wrong protocol, unsupported handshake/auth, package lock/unavailable, missing desktop, port conflict, SELinux, cancellation, partial completion). Keep raw output collapsed. Visual checks in Nocturnal Command, Light, Catppuccin Latte, and narrow windows remain undone.
10. **Complete documentation and final verification (Stage 15).** README and `SERVICE_MANAGER.md` now describe the current service-manager scope and compile-check evidence. Keep `TESTING.md` aligned with implementation limits and the manual matrix. Python persistence behavior is documented as potentially dropping Rust-only fields; keep Python files untouched. Record actual distro/version/client route and clearly distinguish compile-checked, test-executed, docs-verified, and runtime-tested evidence.

## Next recommended coding change

Continue with Stage 14 diagnosis/UI polish, then Stage 15 evidence and final checks. Keep all runtime-only checks clearly marked until the user can exercise them.

## Do not misreport

- No live xrdp or TigerVNC install/start/restart was performed.
- No RDP/VNC setup card has been runtime-tested against a real host.
- Unit tests have not been executed; compile checks type-checked their targets only.
- The service-manager roadmap is **incomplete**. TigerVNC automation, verified bind restrictions, setup history/Undo, full streamed progress, full distro coverage, and the manual UI matrix remain outstanding.
