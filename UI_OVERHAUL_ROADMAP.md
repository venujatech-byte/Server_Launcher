# Modern Dark UI Overhaul

## Mandatory scope — SSH PC Servers tab only

Apply this roadmap only when the selected machine resolves to a configured SSH host **and** the active workspace tab is `SidebarTab::Servers`. The screenshot defines the scope: SSH device identity and actions at the top, remote server list and filter on the left, and the integrated SSH terminal on the right. A new listener confirmation may be launched from this page. Keep the shared machine rail, title bar, theme settings, and existing shared power dialogs as they are.

Evaluate both conditions at render time. For the shared header and tab strip, put changes behind that exact condition in both app variants. The top header and tab strip must render their current version on every other tab, on This PC, and on ADB devices. Use scoped egui styling inside the SSH Servers render path. Shared theme tokens, global widget visuals, and other pages are outside scope. Use surface contrast instead of adding decorative borders, and retain keyboard focus indicators.

## Execution Rules & Git Protocol

1. **STAY ON THE CURRENT BRANCH:**
   - **NEVER** create a new branch, switch branches, or run `git checkout -b`, `git switch`, or `git branch <new-name>`.
   - Verify the active branch via `git branch --show-current` and keep all work on that exact branch.
2. **ATOMIC GIT COMMITS ARE MANDATORY:**
   - Every single phase/task in the roadmap MUST end with its own atomic Git commit using Conventional Commits (e.g., `style(tokens): ...`, `refactor(header): ...`).
   - Specify the exact files to stage and the exact commit message for every step.
   - Never batch multiple phases into one commit, and always check `git status` before committing.
3. **ZERO REGRESSIONS / PRESERVE ALL LOGIC:**
   - Preserve 100% of existing SSH terminal bindings, port scanning, server start/stop/restart/kill/curl/info actions, state management, and API/IPC calls.
   - Work strictly within the repository's existing tech stack and styling system (CSS, SCSS, Tailwind, etc.). Do **not** introduce heavy new UI libraries or unnecessary packages.

Branch locked to `fix/ui-alignment`, restored after the UI reverts at `f6a5c09`. Run commands from the repository root. Preserve unstaged and staged user changes. If preexisting changes overlap a phase's files or there are staged changes from elsewhere, resolve ownership before committing. Do not reset or discard changes. Keep Linux and Windows in each phase's single commit. The two intentional changes to action availability are the requested Curl eligibility and guarded Stop/Kill; preserve the underlying handlers. Work locally. Do not push or run GitHub CI. `AGENTS.md` permits `cargo check` only for Rust compilation. Never run `cargo build`. This document is a plan and grants no permission to implement before the user approves.

## Codebase Map

| Area | Verified paths and entry points |
|---|---|
| Stack and startup | `server_launcher_rust/Cargo.toml`, `server_launcher_windows/Cargo.toml` declare Rust with eframe/egui 0.29 (locked egui 0.29.1). Corresponding `src/main.rs` files configure the native viewport (800×500 minimum). Windows uses native decorations. Linux uses the custom title bar in `LauncherApp::update`. Tauri code exists for the Android Cockpit handoff. This SSH workspace has no DOM, CSS, Tailwind, frontend package scripts, or standalone IPC styling layer. |
| Themes and icons | `server_launcher_rust/src/theme.rs` and `server_launcher_windows/src/theme.rs`: `ThemeMode::palette`, `ThemePalette::apply_accent`, `ThemePalette::egui_visuals`. Each `src/app.rs` has `LauncherApp::palette` for the configured accent. Each `src/lucide.rs` has glyph constants and `setup_lucide_font`, which installs bundled `assets/fonts/inter_sans.ttf` as the proportional face and leaves egui's monospace stack in place. |
| Header | The two `src/app.rs` files, `impl eframe::App for LauncherApp::update`, `TopBottomPanel::top("top_header")` (Linux around 3870, Windows around 3707). Identity uses the selected `SshRemoteHost`, `remote_connection_status`, and `remote_connecting`. The existing timed message convention is `import_status_msg: (message, is_error, Instant)`; remote power dispatch also writes that field. No common toast component is needed. |
| Rail | Same two app files: `SidePanel::left("machines_nav_bar")`, `render_machines_nav_bar` (Linux 2505, Windows 2406), `MACHINE_RAIL_WIDTH`, local/SSH/ADB branches, context menus and Add/Import utilities. `src/config.rs::SshRemoteHost` in both crates has id, name, host, port, user and connection config. There is no configured OS/provider icon field. |
| Workspace and server list | Same two app files: `render_remote_pc_dashboard`, `workspace_sidebar_width`, `workspace_sidebar_body`, `render_workspace_header`, `render_sidebar_tabs_bar`, `SidebarTab`, `render_remote_servers_left_panel` (Linux 2901, Windows 2802). `src/remote.rs::{RemoteListener, RemoteStoppedServer, clean_service_name, build_remote_stop_cmd, build_remote_start_cmd, build_remote_restart_cmd, guess_remote_start_cmd}` in both crates. |
| Terminal | Same two app files: `render_remote_servers_right_panel` (Linux 3310, Windows 3211), `get_remote_logs`, `clear_remote_terminal_logs`, `run_remote_ssh_command`, `remote_terminal_inputs`. The Servers terminal is distinct from the Cmds tab's `remote_terminals` and from `launch_external_ssh_terminal`. |
| Dialogs and menus | `server_launcher_rust/src/modals.rs` and `server_launcher_windows/src/modals.rs`: `render_modal_titlebar`, `render_ssh_host_modal`, `render_remote_start_server_modal`, `render_remote_reboot_modal`, `render_remote_shutdown_modal`, and their existing state structs. Calls and action dispatch live in `LauncherApp::update`. Settings uses `render_settings_modal` in each app file. Use egui 0.29's `menu_button` / `close_menu`, `Response::context_menu` for existing rail actions, and `Area` / `Window` primitives for dialogs. There is no egui `Modal` widget in this version. |
| Verification | `server_launcher_windows/verify.ps1` wraps native `cargo check --locked --tests --target x86_64-pc-windows-msvc`, also the command recorded in `.github/workflows/build-and-publish.yml`. No separate project lint script is configured for this desktop UI. Use the compiler diagnostics and `git diff --check` as the required local checks. Avoid a whole-file formatter pass over the existing source. |

**Check C (required for every phase):**

```bash
git branch --show-current
git status --short --branch
git diff --check
cargo check --manifest-path server_launcher_rust/Cargo.toml --locked --offline --tests
cargo check --manifest-path server_launcher_windows/Cargo.toml --locked --offline --tests
```

Run from the corresponding host when necessary. The second Cargo command on Linux checks the Windows crate's host configuration, not its Windows target. Also use the native Windows check when available. From the repo root in Windows PowerShell, run `./server_launcher_windows/verify.ps1`. Report a missing toolchain/cache or unavailable native check explicitly. Cargo check compiles test code without running it. Phase 6 lists native visual checks using an approved executable that contains the changes, if available. Never claim runtime coverage from type checking or from an older binary.

## Exact Design Token Reference

In Phase 1 define a page-local palette in both app files using the existing palette as the starting point. Apply the surface tokens only in Dark, within the gated SSH Servers view. Preserve custom accent preferences. Leave the shared theme files untouched.

| Role | Existing palette assignments |
|---|---|
| Canvas and device rail | `bg_app`, `bg_header`, `bg_main`, `bg_machines_nav`: `Color32::from_rgb(0x0B, 0x0D, 0x12)` |
| Panel | `bg_sidebar`, `bg_subtle`: `Color32::from_rgb(0x11, 0x14, 0x1B)` |
| Card, input, neutral control | `bg_card`, `bg_input`, `bg_button`: `Color32::from_rgb(0x17, 0x1B, 0x24)` |
| Hover | `bg_card_hover`, `bg_button_hover`: `Color32::from_rgb(0x1F, 0x25, 0x32)` |
| Terminal well | `terminal_bg`: `Color32::from_rgb(0x0D, 0x10, 0x17)` |
| Outlines | Use `Stroke::NONE` for decorative borders on this page. Use a 1-point accent outline for keyboard and editor focus. |
| Accent | `accent`: `Color32::from_rgb(0x3B, 0x82, 0xF6)`; `accent_hover`: `Color32::from_rgb(0x25, 0x63, 0xEB)`; `accent_subtle`, `bg_card_active`: `Color32::from_rgba_unmultiplied(0x3B, 0x82, 0xF6, 36)`; `accent_light`: `Color32::from_rgb(0x93, 0xC5, 0xFD)`. |
| Semantic | `success` = `#10B981`, `warning` = `#F59E0B`, `danger` = `#EF4444` (each via `from_rgb`). For soft status fills use the matching color with alpha 31 via `from_rgba_unmultiplied`. Reserve solid red in this workspace for final confirmation. Retain actual log severity distinctions. |
| Text | `text_primary` = `#F1F5F9`, `text_secondary` = `#CBD5E1`, `text_muted` = `#94A3B8`. On default blue/red filled controls use `#0B0D12`; on hover blue `#2563EB` use white. Existing user accent overrides should keep their computed foreground. |
| Dimensions and type | Units are egui logical points. Controls 28 high, compact icon targets 28×28, radius 8, spacing 6/8/12, 1-point dividers, 6-point scrollbars. Proportional sizes 18 device heading, 14 panel heading, 13 service name/control, 11–12 metadata. Target weights 600 heading/name, 500 control, 400 metadata. Use current proportional font with `RichText::strong()` for emphasis and normal style for controls/metadata. egui 0.29 has no CSS font weights or font feature declaration, so verify legibility rather than adding font assets. Use `FontId::monospace` for identifiers, port/protocol, terminal commands and output. Monospace gives equal-width digits. |

For local styling, use `ui.scope` and `ui.visuals_mut()` for widget idle, hovered, and active states. Prefer real `Button` and `TextEdit` widgets, with `Response::has_focus()` for a 1-point focus outline. Apply slim bars within the rail, remote listener list, and output viewport by scoping `ui.spacing_mut().scroll`: start with `egui::style::ScrollStyle::solid()`, set `bar_width = 6.0`, `bar_inner_margin = 10.0` for listener cards (4 for other areas), and use palette-compatible muted inactive/hover contrast. Do not modify global spacing, font registration, or the source of user accent preferences.

## Phased Implementation Checklist

Each checkbox is one task and one commit. Complete the work and checks under that checkbox before moving to the next.

### Phase 1 — Page-local dark tokens (A)

- [x] Define the SSH Servers palette. Both local crate checks passed.
  - **Target files:** `server_launcher_rust/src/app.rs`, `server_launcher_windows/src/app.rs`.
  - **Exact changes:** Add an SSH Servers palette helper that starts from `LauncherApp::palette()`. Apply the token table to the page-local copy for Dark, keeping the configured accent when present. Use it in the two SSH Servers panel renderers. Later phases use it in the gated SSH header and tab strip. Keep the rail on the shared palette.
  - **Verification:** Run Check C. Review all call sites for the scope condition.
  - **Commit:**
    ```bash
    git add -- UI_OVERHAUL_ROADMAP.md server_launcher_rust/src/app.rs server_launcher_windows/src/app.rs
    git commit -m "style(tokens): define the unified dark palette"
    ```

### Phase 2 — Remote device header and power confirmation (B)

- [ ] Rebuild the SSH identity and group existing header actions.
  - **Target files:** `server_launcher_rust/src/app.rs`, `server_launcher_windows/src/app.rs`.
  - **Exact changes:** In `top_header`, operate only inside the SSH host branch while Servers is active. Show logo, left-aligned host name, textual Connecting/Online/Error/Idle badge with status dot, and a muted 11-point monospace SSH URI beneath. Retain status/error hover detail. Compose `ssh://user@host:port`, bracket raw IPv6 once, keep existing cwd in a separate hover detail and exclude it and credentials from the clipboard. Use `lucide::COPY` with `ctx.copy_text`; store a host-scoped temporary expiry in egui memory and request repaint after 1.5 s to restore the Copy label/tooltip after showing Copied. Truncate identity with full-value tooltip. Visually order neutral controls `Power ▾ | Scan Ports · Terminal · Reconnect | Edit · Settings`. Reserve control width before identity; at under 900 points usable header width put the action toolbar on a second line and let its bounded groups wrap. Avoid moving the non-SSH header or OS title bar.
  - **Bindings:** `Scan Ports → trigger_remote_scan` with existing scanning state; `Terminal → launch_external_ssh_terminal` with pop-out icon and “Open external SSH terminal” tooltip; `Reconnect → test_remote_host` then `trigger_remote_scan`; `Edit → ssh_host_modal.open_edit(host)`; Settings still sets `settings_modal_open`. Use `ui.menu_button` for Power; close it upon choosing an item and invoke only `remote_reboot_modal.open_for_host(host)` or `remote_shutdown_modal.open_for_host(host)`. Keep both modal return signatures and the `update` dispatch to `reboot_remote_device` / `shutdown_remote_device`, including existing password validation, visibility control, Enter, state clearing and status/repaint logic.
  - **Dialog treatment:** Reuse the existing shared restart and shutdown dialogs without restyling them.
  - **Edge cases:** Copy works with long/IPv6 hosts and while offline. Selecting Power or dismissing any dialog dispatches no command. Confirm runs once against the captured host. Preserve errors/password mask and cancel behavior. Validate a configured and unconfigured password without exposing it.
  - **Verification:** Run Check C. See Phase 6 for the native checks.
  - **Commit:**
    ```bash
    git add -- UI_OVERHAUL_ROADMAP.md server_launcher_rust/src/app.rs server_launcher_windows/src/app.rs
    git commit -m "refactor(header): group remote actions and confirm power commands"
    ```

### Phase 3 — Shared device rail (C)

Removed from implementation scope. The existing rail is shared across pages and requires no edit or commit.

### Phase 4 — SSH workspace tabs, search, cards and safety (D)

- [ ] Restyle shared tabs and SSH server list, preserving action dispatch with the two requested guards.
  - **Target files:** `server_launcher_rust/src/app.rs`, `server_launcher_windows/src/app.rs`, `server_launcher_rust/src/modals.rs`, `server_launcher_windows/src/modals.rs`.
  - **Tabs/layout:** In `render_sidebar_tabs_bar`, gate the styling on both scope conditions and use the existing five equal allocations and 26-point row height. Recess the surface with `bg_subtle`, use subtle accent fill and a 2-point accent underline for selected, and keep existing compact label at smaller tab width. Keep `WORKSPACE_HEADER_HEIGHT`, `render_workspace_header`, `workspace_sidebar_body`, and the Sensors width override independent of selection styling. Keep Commands' local and remote focus branches. In `render_remote_pc_dashboard`, set the Servers left frame fill to `p.bg_sidebar`; keep the right match routing and avoid forcing right content below the tab header. Leave the automatic connect, port scan and autostart blocks in place.
  - **List:** In `render_remote_servers_left_panel`, align “Remote servers” and count left, `+ Start Server` right with primary fill, preserving `remote_start_server_modal.open_for_host(host)`. Render a 28-point borderless search field with a separate `lucide::SEARCH` glyph, muted “Filter remote servers...” hint, and a permanently reserved 28-point clear slot. Show a titled `lucide::X` button only for nonempty query; clear only the current host entry in `remote_search_query`, refocus it and filter in the same frame. Retain matching by port/proto/name/command/PID and stopped filters. Keep distinct scanning/no listeners/no matches states and host-scoped scroll id. Scope scroll settings before constructing the scroll area.
  - **Cards:** In both active and stopped loops put left-aligned truncated service name first (full-name tooltip), then `:port · PROTO` in 11-point monospace. Show dot plus Running or Stopped text at right with reserved width. Keep available PID/CPU/RAM/command details, preserve stopped-cache pruning and Dismiss by port. Use radius 8, 10-point inner margin, 6-point vertical gap, borderless cards and a slightly stronger hover fill using pointer-in-card checks that do not intercept clicks. Use neutral Restart, Stop, Info buttons with tooltips, and a neutral `⋯` `menu_button` exposing Kill and eligible Curl. Amber on Restart hover, red on Stop/Kill hover. Stopped rows retain Start, Restart, Dismiss and Info.
  - **Action dispatch:** Move the existing active Stop and Kill click bodies into a single private `LauncherApp` dispatch method taking action, host, listener and context. Stop still calls `build_remote_stop_cmd`, adds the existing `RemoteStoppedServer` metadata and guessed start command if absent, runs the command and schedules scan at 1500 ms. Kill still uses the original PID/fuser branch and schedules 1500 ms. Active Restart stays `build_remote_restart_cmd` plus 2000 ms. Stopped Start/Restart stay `build_remote_start_cmd` plus 1500 ms. Keep both Info branches and exact Curl command (`curl -I -s --connect-timeout 2 http://localhost:<port> | head -n 5`) for eligible listeners.
  - **Curl guard:** Require `l.proto.eq_ignore_ascii_case("tcp")` and plain HTTP evidence. Treat ports 80, 3000, 4173, 5173, 8000, 8008, 8080, 8081, 8888 as HTTP candidates; otherwise allow explicit HTTP labeling (ASCII-lowercase name equal to `http` or beginning `http /`) or a command containing `http://`. Reject 443, 8443, names equal to `https` or beginning `https /`, and commands with only an `https://` URL, because the preserved handler probes plain HTTP. Reject ports 22, 53, 68, 323 even when other hints match. Disable in the menu with tooltip “Available for HTTP over TCP”; the allowlist is a heuristic, and tooltip on the enabled action should say “Probe HTTP response”. No auto-probing. Apply predicate at dispatch too.
  - **SSH lockout guard:** Before any Stop cache mutation or Kill dispatch, divert to confirmation if listener port is 22 or current host's configured SSH port, or if ASCII-normalized service name contains the token `sshd` (`sshd.exe` and `sshd: listener` count), or normalized service name equals `ssh`. Add a transient `Option<PendingRemoteListenerAction>` to `LauncherApp` in both files and initialize to None. Store only host ID, listener identity (protocol, port, PID, name) and a two-variant Stop/Kill enum. Render confirmation from `update` using a dedicated foreground egui Area with a click-blocking backdrop, centered confirmation card, initial Cancel focus, and disabled underlying workspace controls until dismissal, explain SSH loss and name the host and port. Cancel, Escape, outside click, host change, missing host or changed/missing listener clear pending with no command. On explicit Confirm, `take()` the option first, confirm selected host ID, re-read the latest listener snapshot and require a unique exact identity match. Release the map lock before invoking the preserved dispatch helper. On stale selection report “Listener changed. Scan again before retrying.” in the dialog or remote header. Do not add a second SSH request or alter backend protocol. Route both the Stop button and overflow Kill through this intent gate, including keyboard activation.
  - **Edge cases:** Test pure Curl and guard predicates with a compact assertion or existing test module fixture only where existing coverage lacks these cases. Cover UDP, ordinary SSH on 22, custom SSH port, uppercase protocol/name, HTTPS-only, HTTP on a dev port, duplicate/changed listener identity and stale modal. Don't exercise real Stop/Kill on the controlling SSH daemon. Ensure Cancel mutates neither logs nor stopped-cache state.
  - **Verification:** Run Check C. Review every existing action against the original dispatch in the parent commit.
  - **Commit:**
    ```bash
    git add -- UI_OVERHAUL_ROADMAP.md server_launcher_rust/src/app.rs server_launcher_windows/src/app.rs server_launcher_rust/src/modals.rs server_launcher_windows/src/modals.rs
    git commit -m "refactor(remote-servers): simplify listener cards and guard SSH actions"
    ```

### Phase 5 — Remote SSH terminal (E)

- [ ] Restyle terminal and preserve per-host output and input behavior.
  - **Target files:** `server_launcher_rust/src/app.rs`, `server_launcher_windows/src/app.rs`.
  - **Exact changes:** In `render_remote_servers_right_panel`, use a `p.bg_sidebar` frame and title “Remote SSH Terminal”. Render chips in visual order `ss -tulpn`, `docker ps`, `uptime`, `free -m`, `df -h` in 11-point monospace with radius 8, existing tooltip meanings. Reserve a 76-point right-hand slot for `lucide::TRASH Clear` with an 8-point gap; chips wrap inside the remaining width and Clear stays top-right. Use `p.terminal_bg`  for the output well, 10-point padding and scoped 6-point scrollbars. Use `egui::text::LayoutJob` and `egui::TextFormat` with monospace 13 and `line_height: Some(19.5)`; wrap to the viewport content width and preserve log text, order and selectability. Map log kinds via `p.log_stdout`, `p.log_stderr`, `p.log_stdin`, `p.log_launcher`, `p.log_warning` (use text_secondary for stdin in this workspace if the theme's stdin treatment is a clashing extra accent). Retain host-scoped scroll ID, both-axis scrolling support and `stick_to_bottom(true)`.
  - **Input:** Reserve a single 40-point composer below the viewport before computing the scroll height. Fill with `p.bg_input`, radius 8. Use a bounded monospace prompt with absolute cap of 180 points or 30% of composer width, whichever is less, and a full-prompt tooltip. Make the input single-line and frameless with the remaining width after reserved Execute and gaps. Use muted “Enter SSH command…” hint. Draw focus-within outline around the composer when the input or Execute has keyboard focus. Use primary 28-point Execute, which stays in view as prompt truncates. Calculate output height once from actual remaining height after header, frame margins, spacing and composer reservation. Keep all dimensions nonnegative; test minimum window height.
  - **Bindings:** Chips call `run_remote_ssh_command(host, qcmd, Some(ctx.clone()))`. Clear calls only `clear_remote_terminal_logs(&host.id)`. Keep `get_remote_logs` and `remote_terminal_inputs.entry(host.id.clone())`, trim and reject whitespace, submit once on click or the existing Enter/lost-focus condition, clear after valid dispatch and restore input focus. Use separate scroll and editor IDs per host. Avoid any change to Cmds, command strings, remote cwd semantics, launch transport, or log lifecycle.
  - **Edge cases:** Long user/host, long log lines, huge output, scroll position during append, switch between hosts with drafts, Enter with empty input, click on Clear with an unsent draft and pending listener confirmation covering the workspace.
  - **Verification:** Run Check C.
  - **Commit:**
    ```bash
    git add -- UI_OVERHAUL_ROADMAP.md server_launcher_rust/src/app.rs server_launcher_windows/src/app.rs
    git commit -m "style(remote-terminal): unify toolbar viewport and command input"
    ```

### Phase 6 — Accessibility and verification (F)

- [ ] Complete the focus and responsive layout pass and record verification.
  - **Target files:** `server_launcher_rust/src/app.rs`, `server_launcher_windows/src/app.rs`, `server_launcher_rust/src/modals.rs`, `server_launcher_windows/src/modals.rs`, `UI_OVERHAUL_ROADMAP.md`.
  - **Exact changes:** Audit actual focus rings on each new header/tab/card/menu control and both editors. Paint missing 1-point accent outlines on the response rect via `Stroke::new(1.0, p.accent)`; avoid clipping at container edges. Add missing accessible names to custom and icon-only interactables, retain text with all semantic status colors and give disabled Curl a reason. Verify that open menus close on Escape and click outside, and select only on activation. In modal states, limit keyboard reachability to the enabled modal controls by disabling underlying UIs during that frame; restore focus on cancellation and ensure the closing click is not replayed. Audit IDs for host changes and repeated listeners. Fix any width/height overflow in the phase's widgets only. Record the matrix below and any remaining native verification limits in this document.
  - **Verification:** Run Check C and the native Windows check if available. Use a current executable when available, with an authorized disposable SSH host or fixture for actions. Verify each row below on Linux and native Windows where possible. Record a platform-specific limitation if unavailable. Confirm title-bar hit targets are correct at Windows 125–150% display scaling.
  - **Commit:** Use path-limited staging as below. If no source correction is required, Git stages only the updated roadmap.
    ```bash
    git add -- UI_OVERHAUL_ROADMAP.md server_launcher_rust/src/app.rs server_launcher_windows/src/app.rs server_launcher_rust/src/modals.rs server_launcher_windows/src/modals.rs
    git commit -m "fix(remote-ui): verify focus and responsive interactions"
    ```

| Native check | Expected result |
|---|---|
| 1366×768, minimum 800×500, wide desktop, 125–150% scale, Dark and alternate theme | No horizontal page clipping. Header actions and terminal Clear/Execute stay reachable. Long names/URI/prompt truncate with reveal. Scrollbar leaves card breathing room. |
| Switch Servers, Scripts, Cmds, SystemD, Sensors on local and SSH | Tabs remain aligned, existing tab focus behavior survives, and Sensors does not shift the shared header. Local and ADB context actions keep working. |
| Keyboard and pointer | Tab and Shift+Tab reach controls in render order; Enter/Space activate the focused control as appropriate. Cancel/Escape/outside click dispatch nothing in Power, overflow and guarded action dialogs. Clicks do not pass through modal backdrop. |
| Connection and search | Online, Connecting, Idle and Error stay distinct. Copy yields the URI alone. Reconnect, Scan, Edit, Settings and external terminal still route correctly. Search and clear affect only one host. No active listeners and no matches remain distinct. |
| Listener actions | Start/Dismiss and active/stopped Info and Restart bind to the same handlers. Eligible HTTP accepts Curl; UDP/SSH/HTTPS-only reject it. Guarded Stop/Kill require explicit confirmation. Verify dispatch on a disposable endpoint or by inspecting an isolated action boundary; do not stop the daemon carrying the session. |
| SSH terminal | Each chip dispatches its exact command. Enter and Execute dispatch once. Blank input dispatches nothing. Clear affects the selected host's logs alone. Host switching preserves drafts, and wrapping, scrollback and bottom-follow remain usable. |
| Power | Both menu items open the right existing dialog. Cancel and backdrop never execute. Verify confirm-to-dispatch mapping with an isolated stub or disposable host and preserve existing authorization/error reporting. |

## Progress Tracking Instructions

For every phase, run Check C and its specific checks, then change that phase's checkbox to `[x]` in this file. Record the checks actually performed beneath that phase; distinguish unavailable checks from passing ones. Stage the exact paths for that phase, check `git status --short --branch`, `git diff --cached --name-only` and `git diff --cached --check`, then use the specified message. After commit, verify status and that the branch is still `fix/ui-alignment`. The initially untracked roadmap belongs in the Phase 1 commit; later commits stage only their own progress update with their source changes. Keep separate commits even if all phases are executed in one session.

**Planning stop gate:** After this document is created, stop and ask for approval. Implementation starts only after the user confirms.
