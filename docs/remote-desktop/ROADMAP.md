# RDP and VNC audit and implementation roadmap

Baseline `vnc/rdp` at `d8b64bb`. Scope is the native Rust edition, its packaging, and remote desktop documentation. Assume one operator with two simultaneous viewers, including an active 1080p desktop on a high-latency connection. Use `TESTING.md` for the platform matrix and `DESIGN.md` for decisions already made.

## Execution status — 2026-11-27

At the start of this status update, local `HEAD` was `e14e31a`, two commits ahead of `origin/vnc/rdp` (`4754177`). The authorized SSH test PC is at `4754177`, which includes all code changes but not the latest documentation-only commits. `cargo check --locked --all-targets` passed on that Linux PC. The local Linux check also passed at `4754177`. Repository instructions allow `cargo check` only. The new regression tests compile but have not been run. A local Windows GNU cross-check could not start because `x86_64-w64-mingw32-gcc` is missing; the release workflow's Windows MSVC check remains unrun. No GUI, protocol interoperability, Windows, CI, release, or performance result is verified for these commits.

| Stage | Evidence at `4754177` | Remaining work |
| --- | --- | --- |
| 1. Baseline | Source checked for the cursor setting. The local pointer setting is covered by a regression test. Linux `cargo check --locked --all-targets` passed locally and on the SSH test PC at `4754177`. | Run credential persistence, cursor/input, protocol connection and notification checks on current builds. |
| 2. Rendering and performance | RDP damage conversion falls back to one full-frame update after 32 disjoint regions. Regression case added and compiled. | Reproduce stale regions on a real server. Compare full and changed-region uploads and record real latency and resource measurements. |
| 3. Clipboard and files | Validate local clipboard image size before copying bytes. Include dimensions in image loop-prevention fingerprints. | Run both clipboard directions, clipboard fallback, and file transfer tests on supported desktops and servers. |
| 4. Recorder and lifecycle | FFmpeg writes use a one-frame queue and a supervisor with a two-second stop grace period. Frame buffers are reused when available. Unix stuck-child regression test added and compiled. | Execute the regression test where permitted. Test normal/forced recording shutdown, output integrity, launcher close, tunnel cleanup, and Windows process behavior. |
| 5. Interoperability | Test PC has no listener on TCP 3389 or 5900–5910; it is not currently an RDP/VNC target. | Run the RDP, VNC, gateway, certificate, audio, microphone, drive, multi-monitor, credential and Windows/Linux matrix in `TESTING.md` against a verified target. |
| 6. Release | Local Windows GNU cross-check is unavailable without MinGW. No current Windows MSVC CI or package evidence. | Run authorized Windows CI and package checks on the current revision. Record results and check the packaged viewer process. |
| 7. Follow-up | Direct VNC TLS, extended VNC clipboard and native Wayland shortcut capture remain unsupported or unverified. | Work these only after the core protocol and release gates pass. |

### Test PC listener probe — 2026-11-27

The authorized SSH test PC answered a read-only TCP listener check. No listener was found on TCP 3389 or 5900–5910. The process list showed `xrdp-sesman`, an Xorg `:10` session, and `xrdp-chansrv`, but no RDP listener process; this host is not currently a usable RDP/VNC interoperability target. No services or files were changed. Use a verified xrdp/VNC server before running stage 5 protocol checks.

## Start here

1. Run `git status --short --branch` and `git log -8 --oneline`. Stay on the remote desktop branch. At audit time `recording.rs` had an existing unstaged diff. Read it before editing that file and preserve it unless the owner has incorporated it.
2. Complete stages in order. Each stage names the relevant files under `server_launcher_rust/src/remote_desktop/` unless a different path is given. First find the named functions with `rg`, then read their callers. Prefer one fix and one focused regression check per commit. For verification-only tasks, commit the evidence in the docs.
3. Compile with `cargo check --locked --all-targets` from `server_launcher_rust`. Follow current workspace instructions for any additional commands. The existing Windows and Linux check workflow is `.github/workflows/remote-desktop-check.yml`. Preserve baseline formatting and warnings. Stage only files for the current task, inspect the staged diff, and use a Conventional Commit with a short body. Honor the user's current push authorization and protected branch rules.
4. Mark each check as **source inspected**, **compile checked**, **runtime passed**, **runtime failed**, or **blocked**. Log the commit, client OS and compositor, server and version, selected profile, route, observed behavior, and exact missing prerequisite if blocked. Use disposable data and authorized test hosts. Ask the owner for the remaining GUI checks after automated and available live checks finish.
5. Keep legacy config readable, preserve Python keys, and leave Android Cockpit alone. Keep credential bindings, prompt boundaries, and SSH host-key verification intact. Do not automate server installation or firewall changes.

## Audit summary

| Area | Current state | Finish in |
| --- | --- | --- |
| GUI menu, editor, palette, profiles, OS credentials, protocol probes and SSH listener scan | Wired. Latest real credential-save and restart matrix outstanding. Probes check reachability and negotiation. | 1, 5 |
| RDP TLS/NLA, EGFX, bitmap, resize, keyboard and pointer | Wired. User reported stale display regions and local Super capture. Cursor rendering is disabled in the connection settings and has a compile-checked regression test. Live confirmation is outstanding. | 1, 2 |
| VNC Raw, Tight, CopyRect, ZRLE, resize, clipboard and SSH transport | Wired. Real-server encoding/interoperability matrix incomplete. Direct VNC TLS and extended clipboard are absent. | 1, 5, 7 |
| RDP audio, microphone, drive redirection, multi-monitor and RD Gateway | Code exists in `rdp.rs` and `rdp_drives.rs`. Live validation is outstanding. | 5 |
| Clipboard and files | RDP Unicode, DIB and explicit file share/download paths exist. Classic VNC uses Latin-1. Rich HTML, always-sync preference and broader native clipboard integration remain follow-ups. | 3, 5, 7 |
| Viewer lifecycle, trust, screenshots and recording | Wired. Recorder shutdown is bounded in code and has a compile-checked Unix regression test. Certificate rotation, recorder runtime behavior and crash cleanup remain unverified. | 4, 5 |
| Release and performance | Prior Linux and Windows compile results are documented. Comparable performance numbers and current package/runtime results are missing. FFmpeg is already recommended in Linux packaging. | 2, 6 |

## Stage 1 — Establish a reproducible baseline

- Start with the owner's xrdp and VNC targets or an already authorized test setup. Record exact client revision on both ends and avoid using a stale binary for comparison. Open each protocol through the GUI menu, enter a disposable password, save, close the launcher, reopen the editor, and test reconnect. Exercise changing the endpoint and make sure the previous credential is not reused. If the store is unavailable, check that the error is visible and the saved config does not claim a password was stored. Follow `credentials.rs` into the host editor save handlers before changing behavior.
- Reproduce the RDP pointer report with Fit, native-size scrolling, Fill, fullscreen, window resize and view-only. In `viewer.rs`, inspect the canvas code around `remote-desktop-cursor` and the pointer mode in `rdp.rs`. Expected result is one pointer over the visible remote image and the normal host pointer over toolbars and outside the viewer. If it fails only when scrolled, derive a visible rectangle with `screen.intersect(ui.clip_rect())` and use the existing canvas response or clipped rectangle for hover. Test disconnected and loading overlays so neither hides the host pointer.
- Reproduce Super and Alt+Tab on X11, GNOME XWayland, native Wayland and Windows. Follow `x11_keyboard.rs`, `windows_keyboard.rs`, and `viewer.rs`. Capture the OS grab result and whether the viewer forwarded the chord using event counts or status flags, never log actual keystrokes. Where native capture is denied, show the real capture state on the Keyboard toggle and keep Send keys available. Do not repeatedly retry grabs every frame. Check held Ctrl and mouse buttons across focus loss, View only and disconnect. Preserve Windows secure-attention handling.
- Run direct and tunneled probes with a known reachable endpoint and a closed port. Ensure protocol readiness says what was verified rather than claiming desktop authentication. Compare modal error text and launcher notifications with the actual failure stage. Use the existing notification path for launcher errors if any still change header height.

## Stage 2 — Fix stale RDP regions and measure latency

Files `rdp.rs`, `framebuffer.rs`, `viewer.rs`. First establish whether the fault is decoded pixels, damage accounting, or upload.

1. Test an identical xrdp session with changed-region and full-frame uploads. Use a scrolling page, a window moved across the desktop, and a resize mid-update. Record negotiated codec/profile and whether the same bad pixels occur in a native screenshot. If full-frame and screenshot are also wrong, inspect IronRDP active-stage output and decoder surface updates before changing texture code.
2. Add temporary aggregate counters at the decoder output, damage accumulator, and texture uploader. Count update batches, damage rectangles and pixels, texture uploads, and maximum input queue delay. Log at most once per second only when requested. Do not log pixels or input payloads. Keep received update rate separate from actual uploaded frame rate.
3. If partial uploads are at fault, extend the tests beside `Framebuffer::take_updates`. Apply its updates to an initially blank reference image, then compare against `snapshot()`. Cover distant disjoint regions, overlapping copies, right and bottom edges, padded stride, many rectangles beyond the damage cap, and a dimension change. On resize the texture must be rebuilt before applying patches for the new size. Match the decoder's rectangle endpoint semantics to the exclusive Rust slice boundary exactly once.
4. If decode and upload are correct but input waits on busy streams, measure existing queue drain timing before changing it. Give reliable key/button transitions a bounded turn between bounded batches of inbound work. Coalesce only unsent pointer motion, preserving its ordering around clicks, wheels and key/button releases. Retain existing overflow handling for reliable input. Keep idle repaint event-driven.
5. Compare the same host, resolution, codec when possible, route, and workload against the reference client. Record idle CPU/RAM, active CPU/RAM, received updates/s, upload rate, bandwidth, and input-to-visible-response latency. Measure static desktop, scrolling and video. Include 30 minutes at active 1080p and one higher-resolution case. Report actual values and the setup in `DESIGN.md`.

## Stage 3 — Make clipboard failures diagnosable and verify data boundaries

Files `clipboard_sync.rs`, `rdp_clipboard.rs`, `clipboard_image.rs`, `viewer.rs`, and `vnc.rs`.

- Use unique disposable clipboard values for each test. Test each direction independently, then Both and Off. Include unfocused viewer, View only, reconnect, Unicode and DIB for RDP, and Latin-1 for VNC. Verify the desktop clipboard when pasted into a real application. A UI notice is insufficient.
- Trace each boundary independently when a copy fails. Local OS read → worker pending slot → protocol announcement → remote data request is the outbound path. Inbound reverses those steps. Record result categories and sizes only. If startup suppresses a fresh value, distinguish initial baseline sampling from a user copy after focus is acquired. Preserve direction and focus checks.
- Audit finding in `clipboard_sync.rs` near `get_image()`: local pixel data is copied into a Vec before size/shape validation. Validate dimensions with checked multiplication, compare the result to the borrowed byte length and `max_image_bytes.saturating_sub(124)`, then construct the owned image. Reuse already owned bytes where possible. Verify rejection at one byte over the cap and overflow dimensions. The OS library can still allocate its initial buffer, so describe the cap accurately.
- Verify alternate-format fallback when a remote advertises DIBV5 and Unicode but fails the first request. Keep pending offer state scoped to the current connection and offer. If a later offer arrives, an earlier failed request must not overwrite its value.
- Validate RDP file ranges and destination handling using a file larger than 1 MiB, multiple names, a conflicting destination, and interrupted transfer. Confirm existing local files remain intact and partial outputs are cleaned up. Keep flat regular-file semantics unless recursive transfer is explicitly implemented. Use the existing SSH upload path for viewer drops and verify exact remote destination, progress and cancel.

## Stage 4 — Fix recorder shutdown and verify process cleanup

Files `recording.rs`, `viewer.rs`, `session.rs`, `tunnel.rs`.

- Audit finding in `record()`: the worker can block on `pipe.write_all` when FFmpeg stops consuming data and on `child.wait` when it stops exiting. The stop flag cannot interrupt those waits. Preserve the current encoder/fallback options. Give the process one supervisor responsible for waiting and killing, through a bounded control channel or shared stop state. Move pipe writes to the worker owning stdin. Keep at most one pending frame and retain the last frame for CFR. On normal stop close stdin and allow a bounded trailer flush. On expiry terminate and reap the encoder, report failure, and preserve only media verified usable. Test with a controlled child that stops draining its pipe. Confirm UI and remote input stay responsive.
- Compare the viewer exit/finalization deadline with the parent session's two-second forced termination in `Sessions::open`. Explicitly test normal close during recording, launcher close, and viewer termination. Keep the normal shutdown bounded and coordinate any changed timeout between parent and child. Verify the SSH child is gone on Linux and Windows. Check FFmpeg output after abnormal exit before claiming it is recoverable.
- Verify screenshots at native remote size in Fill and Fit, odd-sized video frames, pause/resume, resolution changes during recording, and fallback when FFmpeg is missing. Distinguish missing executable from encoder rejection. Validate PNG fallback against a constrained destination and document its disk growth.

## Stage 5 — Run interop, transport and credential checks

1. **RDP routes and trust** — Match `probe.rs` and `rdp.rs` across direct, SSH and a trusted RD Gateway. Use `trust.rs` to verify Trust once, Always trust, pinned certificate mismatch, invalid dates and cancellation. A mismatch must block login. Verify the connection route changes the credential/pin binding where expected. Keep legacy unencrypted RDP unavailable.
2. **Audio and devices** — In `rdp.rs`, exercise RDPSND and microphone separately, with the microphone opt-in. Test device denied and audio-device removal. In `rdp_drives.rs`, share a disposable folder and test read-only, read/write and paths outside the granted folder. Verify sharing ends on disconnect and unselected folders stay inaccessible. Verify two-monitor coordinates and single-monitor fallback. Fix only failures with evidence.
3. **VNC transport and codecs** — Run existing VNC test cases and real server tests from `TESTING.md`. Include a successful SSH-only username/password session when the server offers the matching auth method. Check whether the selected color depth and encoding explain any rendering errors. Verify rejected resize preserves the old framebuffer and focus loss releases keys. Keep fail-closed authentication behavior and the warning for direct unencrypted connections.
4. **Windows** — Verify quiet SSH child spawning, stored-password tunnel authentication, hook scope, viewer focus, and cleanup using a real Windows build. On Linux cover both X11 and Wayland. Each failure gets the smallest patch in its owning module and one regression check.

## Stage 6 — Release readiness

- Review existing README and design/test docs against measured behavior. Update supported modes, setup, troubleshooting, data-sharing controls and keyboard limitations. Keep the short README section and link to the full matrix. Confirm `tauri.conf.json` still lists FFmpeg as a recommendation and the update manifest and artifact formats are unchanged.
- Run current branch CI on Linux and Windows where authorized. Record run ID and revision. Inspect generated package content from an authorized release build and verify the packaged launcher can start the native viewer process. Check optional native libraries and dependency licenses against the locked versions.
- Final gate is evidence for foreground input, both protocols, password persistence, clipboard directions, transfers, resize, certificate trust, reconnect, tunnel cleanup, recording and performance. Unavailable platforms stay marked blocked. Give the owner a short remaining GUI checklist with the build revision.

## Stage 7 — Follow-up work after core verification

- **Native Wayland keyboard capture** — Add a capability indicator first. Investigate compositor shortcut inhibition for the viewer surface and the pinned windowing APIs. Only add backend-specific integration if a supported interface can request and release inhibition with viewer focus. Keep Send keys on unsupported compositors and add a release path for focus loss.
- **Encrypted direct VNC** — Keep the existing SSH route as the default secure option. Evaluate a maintained TLS-capable backend against TigerVNC and macOS. Require certificate/name validation and explicit trust before auth. Keep the existing VNC behavior until the new route passes positive and negative interoperability checks. No automatic insecure fallback.
- **Clipboard extensions** — Verify negotiated support before adding HTML or extended VNC clipboard. Extend existing clipboard content types only when the local backend and remote channel can both carry the format. Reuse direction checks, size bounds, echo suppression and transient write retry. Prefer Unicode text fallback when extended formats fail. Treat always-sync as explicit opt-in and keep focused-only sync as default.
- **Viewer quality of life** — Add measured connection health only where the protocol supplies a meaningful signal. Distinguish encode/update rate from displayed frames. Verify existing geometry persistence, toolbar auto-hide, keyboard shortcut settings and clip activity first. Keep optional changes separate from rendering fixes.
- **Optional devices and capture** — Keep smart cards, printers, USB, gamepads, session audio recording and GIF fallback optional. PNG sequence already supplies a fallback. Verify library support and user permission controls before exposing any additional device option.

## Audit limits

The execution status above is based on the source and compile output at the listed revision. No runtime protocol tests, CI, package checks, or performance measurements were run for this roadmap execution. Prior results in `DESIGN.md` and `TESTING.md` apply only to their recorded revisions. The audit excludes Python, the separate Windows crate, Android Cockpit, and unrelated launcher features. No dependencies were added.
