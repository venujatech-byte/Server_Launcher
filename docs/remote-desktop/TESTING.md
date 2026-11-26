# Remote Desktop verification guide

This guide records what can be checked locally and what requires real RDP/VNC
servers. A successful compile is not proof that a protocol session works.

## Local checks

Run these from `server_launcher_rust/`:

```sh
cargo fmt --check
cargo check --offline --all-targets
```

The repository currently has formatting differences outside the remote desktop
code. When checking formatting, run `rustfmt --check --edition 2021` on only the
Rust files changed in the task rather than formatting the whole repository.
The project workspace instructions restrict agent compilation to `cargo check`;
unit and integration tests must be run by a maintainer in an environment where
test execution is approved.

## Manual connection matrix

For each test, record the app revision, client OS/display backend, server OS,
protocol, route (direct or SSH tunnel), negotiated display size, result, and
relevant server/client logs with credentials and clipboard contents removed.

| Client | Server | Protocol | Check |
| --- | --- | --- | --- |
| Linux X11 | Windows 10/11 or Windows Server | RDP | NLA, login, full desktop, pointer, keyboard, resize, disconnect |
| Linux Wayland | Windows 10/11 or Windows Server | RDP | Repeat core input and resize checks; note compositor-specific limits |
| Windows | Windows 10/11 or Windows Server | RDP | NLA, login, full desktop, pointer, keyboard, resize, disconnect |
| Linux X11 | Linux xrdp | RDP | Direct and SSH-tunneled sessions, authentication errors, resize |
| Linux Wayland | Linux xrdp | RDP | Repeat core rendering and input checks |
| Windows | Linux xrdp | RDP | Direct and SSH-tunneled sessions, rendering and input |
| Linux X11 | TigerVNC or x11vnc | VNC | RFB 3.3/3.7/3.8, auth, Tight/Raw/CopyRect/ZRLE, cursor, clipboard |
| Linux Wayland | TigerVNC or x11vnc | VNC | Repeat core rendering and input checks |
| Windows | TigerVNC or x11vnc | VNC | Direct and SSH-tunneled sessions, rendering and input |
| Linux/Windows | macOS Screen Sharing | VNC | Authentication, clipboard limitations, cursor and resize |

Do not expose a server port just to run this matrix. Use an existing private
network or the host's configured SSH tunnel.

### Endpoint handshake smoke check (2026-11-26)

On the authorized Ubuntu 24.04.5 SSH test PC, xrdp 0.9.24 answered an X.224
connection request and selected TLS. TigerVNC 1.13.1 returned an RFB 3.8
banner and offered VeNCrypt plus classic VNC password authentication. These
checks stopped before authentication and did not launch this app's viewer, so
they confirm reachable protocol endpoints only, not a working desktop session.
The checked app revision was `8ec3a0d`.

## RDP checks

1. Open the RDP and VNC editor tabs and confirm the SSH-side server scan finds
   listeners on TCP 3389 and 5900–5910, reports common server processes, and
   gives read-only setup guidance when none are found. Confirm it does not
   install packages or alter remote services.
2. Connect with security set to Auto and verify the server selects NLA when
   available, or encrypted TLS when NLA is unavailable. Verify the certificate
   and host name before credentials are sent. Check that invalid credentials
   produce a readable failure and no credential data in logs.
3. Verify full desktop rendering and local pointer shape, then type text and
   test left/right/middle plus side mouse buttons and both wheel axes.
   Confirm that a continuously changing desktop stays live over EGFX, and
   check that the viewer's FPS counter changes with received display updates.
   With Keyboard enabled by default, type while the viewer is focused without
   clicking the remote canvas first. On Linux/X11, confirm Super and Alt+Tab
   reach the remote. On Wayland with a reachable `DISPLAY`, confirm the viewer
   uses XWayland and test those shortcuts there. On Windows or Wayland without
   a reachable XWayland display, check whether the OS reserves those shortcuts;
   use Send keys when it does. Turn Keyboard off and confirm input stays local.
   On Linux/X11, verify Right Ctrl+F, +S, +R, +D, and +K, then disable the host
   key in Settings → Remote Desktop and confirm those chords are forwarded
   normally. Confirm Right Ctrl alone and unknown Right Ctrl combinations are
   forwarded normally.
   Enable View only and confirm the display remains live while keyboard,
   pointer, and outgoing clipboard input are suppressed.
4. Resize the viewer with dynamic resolution enabled. Check for correct aspect
   ratio and no stale pixels after the server changes resolution.
5. Compare Incremental and Full-frame repaint. They should show the same remote
   frame; Full-frame repaint may use more client CPU and does not change server
   capture FPS, codec negotiation, or network latency.
6. Connect with the 256 colors (8 bpp) profile and confirm a palette update
   renders correctly. To check AVC, configure a supported Cisco OpenH264
   library in Settings → Remote Desktop, then try GFX AVC420 and GFX AVC444
   against a server that supports them. Clear the library path and confirm the
   session falls back without advertising AVC. Confirm other profiles continue
   to negotiate EGFX.
7. Test clipboard directions separately with Unicode text and an image under
   the configured size cap. Confirm received content is not echoed back. Also
   test a clipboard source that advertises DIBV5 before Unicode text but rejects
   or returns invalid DIBV5 data; the client should still receive the text
   offered in that same clipboard update.
   Change the image cap in Settings → Remote Desktop and verify images larger
   than the selected 1–64 MiB limit are not sent or accepted.
8. On RDP, use **Share files**, choose local files, and paste them on the remote
   desktop. Include a file larger than 1 MiB to exercise multiple range
   requests. Confirm changing the offered file list does not change an
   in-progress paste. File clipboard transfer is local-to-remote only.
9. Test NLA-only and TLS-only explicitly against matching servers. Auto must
   accept either encrypted mode and must reject legacy unencrypted RDP.
   Also test Administrative session against a Windows account allowed to use
   it; confirm the default profile does not request session ID 0.
10. Use Test Connection through direct, SSH-tunneled, and gateway routes. The
   tunnel check must reach the RDP security negotiation over SSH. Connect
   through an RD Gateway whose certificate is trusted by the client.
   Confirm the RDP credentials are accepted for gateway sign-in, the gateway
   forwards to the configured RDP host and port, and an untrusted gateway
   certificate blocks the connection. Confirm the editor rejects combining
   RD Gateway with Tunnel via SSH. The probe checks the RDP handshake after
   gateway sign-in but does not log in to the desktop. Change the gateway host
   or port and confirm the old saved RDP password is not reused; equivalent
   default-port spellings should preserve the credential binding.
11. Start with the server unavailable, then make it available during the retry
   window and confirm the initial connection is retried. Confirm an
   auto-reconnecting session makes at most five attempts after a failure, then
   leaves the final error visible (or closes when Close on connection failure
   is enabled). Test a disconnect after a stable session as well, and close
   while input keys are held. Confirm all remote keys/buttons are released on
   focus loss and close.

The renderer accepts legacy bitmap updates and EGFX updates. AVC decoding is
available only when a supported Cisco OpenH264 library is configured. Without
one, confirm the server negotiates a supported ClearCodec/RFX path. Audio,
microphone, drive redirection, committed IME input, and multi-monitor behavior
have not been verified against real servers; do not mark them complete based on
UI controls or compile checks alone. RD Gateway, including trusted-certificate
handling and gateway authentication, still requires live-server validation.
Native keyboard grabbing is currently Linux/X11-only. Windows and Wayland rely
on egui input and cannot guarantee capture of OS-reserved shortcuts.

## VNC checks

1. Test no-auth and classic VNC password auth. Use passwords from 1 to 8 bytes;
   verify longer inputs are rejected with a clear message rather than silently
   truncated. Confirm unauthenticated negotiation is not used when a password
   is configured.
2. Verify Tight fill, copy, palette, gradient, JPEG, and PNG updates, plus
   Raw, CopyRect, and ZRLE. Include resize and an empty framebuffer. Confirm
   Raw/Cursor pseudo-rectangles do not get treated as framebuffer pixels.
   With Tight enabled, verify Best/Good/Fast send their compression and JPEG
   quality hints, while Auto leaves the server defaults unchanged.
3. Verify RichCursor and XCursor shapes, hotspot placement, pointer movement,
   and the Show remote cursor setting. Test a server that sends only one cursor
   shape encoding as well as a server that sends neither.
4. Test keyboard, pointer buttons, drag, and vertical/horizontal wheel. Check
   that view-only mode sends no input and focus loss releases held keys. Type a
   non-Latin-1 character into a remote text field and confirm keyboard Unicode
   input works independently of the Latin-1 clipboard limit.
5. Test clipboard text in both directions, including non-Latin-1 content and
   data above the limit. Classic RFB does not support image clipboard here.
6. Test direct public-address confirmation and SSH tunneling. Use Test
   Connection through the tunnel and confirm the RFB banner/security offer is
   reported. On Windows, verify a key- or agent-authenticated SSH profile
   and a saved-password-only profile both open the requested local forward.
   Confirm the askpass password is absent from process arguments and
   environment variables. Verify closing or crashing the viewer
   cleans up the tunnel process.
7. Enable Close on connection failure and verify an exhausted retry sequence
   closes the viewer. With the option disabled, the final error stays visible.
8. Request a supported and unsupported remote resolution; confirm rejection
   leaves the prior framebuffer usable.

Tight fill, copy, palette, gradient, JPEG and PNG decoding have unit coverage;
verify them against a live TigerVNC or x11vnc server before relying on every
server-specific Tight variant. Username-based VNC authentication, rich
clipboard, and direct VNC encryption are not currently implemented. Classic
VNC over a direct connection is unencrypted.

## Session and security checks

- Open two sessions and verify host/protocol pairs do not collide or duplicate.
- Relaunch the same host/protocol and verify the existing child is focused.
- Resize and reposition an RDP or VNC viewer, close it normally, and confirm
  that reopening the same host/protocol restores its geometry without affecting
  the other protocol's window.
- Close a viewer, terminate it unexpectedly, and close the launcher; verify
  children and SSH tunnels are cleaned up.
- Verify passwords are only read from the OS credential store, never command
  line arguments, environment variables, JSON config, logs, or crash messages.
- Verify trust-once is not persisted, always-trust is host/port scoped, and a
  changed certificate fingerprint blocks reconnection until reviewed.
- Verify clipboard and screen recording are visibly indicated and can be
  disabled. Never use real secrets in clipboard or recording tests.

## Performance measurements

No comparable real-server measurements are recorded yet. Measure the same host,
resolution, scene, network path, and duration for this app and a reference
client. Record idle viewer CPU/RAM, active CPU/RAM, display updates per second,
receive bandwidth, and input-to-visible-response latency. Test a static desktop,
scrolling text, and moving video separately. Report the app's FPS counter as
received display-update batches per second, not the server's capture FPS.

Run each active test for at least 30 minutes and check memory growth. Repeat
with the window focused, unfocused, and minimized. Keep raw measurements with
the test environment details; do not claim a performance improvement from a
single screenshot or FPS sample.
