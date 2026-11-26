# Remote Desktop design

## Repository baseline

Work starts on `vnc/rdp` from `fix/ui-alignment`. The native viewer is scoped
to `server_launcher_rust`. The separate Windows package mirrors much of the
launcher but will need its own integration and native validation before it can
ship the viewer. The Python launchers share the host store. Keep the current
keys and omit unset protocol profiles when saving.

The SSH header in `app.rs` has a GUI menu with separate RDP and VNC connection
actions and profile-settings buttons. `modals.rs` owns the tabbed host editor
and command palette, `config.rs` owns host JSON, and `main.rs` dispatches the
existing separate Android Cockpit mode before opening the main window.
`remote.rs` provides SSH arguments and the port inspector. Reuse the active
`ThemePalette` from `theme.rs`. The baseline repository has rustfmt drift, so
check new files individually and avoid repository-wide formatting churn.

## Product references

[Remmina](https://remmina.org/remmina-features/) provides per-protocol profiles,
SSH tunneling, clipboard controls and viewer scaling. Its
[profile options](https://remmina.gitlab.io/remminadoc.gitlab.io/md__builds__remmina_remmina_ci__remmina_wiki__remmina__config__file__options.html)
show why connection settings belong in protocol-specific tabs. Use the
host's existing SSH identity in both tabs and keep fit, native size and
stretch controls in the session. Give the user a Send keys menu for shortcuts
the platform intercepts. Treat drive sharing as an explicit permission.

[Chrome Remote Desktop](https://support.google.com/chrome/answer/1649523) starts
from a host list and opens directly once a device is set up. Google documents
encrypted sessions and a disconnect control available from session options.
Apply the same low-friction launch to the existing host header. Use a toolbar
that can be pinned and show connection health, clipboard activity and
recording in the viewer. Keep authentication and certificate decisions in the
viewer. Headless hosts need a read-only diagnosis before suggesting setup.

## Client library audit

| Candidate | Verified surface | Integration required |
| --- | --- | --- |
| [IronRDP 0.17.0](https://docs.rs/crate/ironrdp/0.17.0/features) and [upstream viewer](https://github.com/Devolutions/IronRDP) | Connector/session features and protocol libraries for NLA, clipboard, audio, device redirection and display control | Pin matching crate versions. Verify the viewer's TLS upgrade and certificate validation, and test display control and each channel independently. RDCleanPath is a different protocol from Microsoft RD Gateway. File clipboard support in the web client does not establish support in the native clipboard adapter. |
| [vnc-rs 0.6.0](https://docs.rs/vnc-rs/0.6.0/vnc/) | Async connector, input events, full/incremental refresh, cursor events and desktop layout types. The example negotiates Raw, CopyRect, ZRLE and Tight. | Verify decoder bounds, authentication methods and desktop resize against real servers. The public cut-text event is Latin-1. Treat classic VNC as unencrypted and gate direct public connections. |

Both candidates offer MIT-compatible licensing. Verify the full locked
dependency tree before packaging. Rust 1.99 is installed in the development
workspace. Check the release workflow's compiler separately before choosing
the final versions.

## Boundaries and verification

- Keep a protocol profile distinct from credentials. The existing credential
  helper uses Secret Service on Linux and DPAPI on Windows. Preserve SSH's
  current lookup binding. Use protocol-specific target bindings for new
  secrets. Surface lookup failures in the viewer.
- Identify sessions by host ID and protocol. Start the same executable in
  session mode with those identifiers only. Use a bounded control channel,
  send Close on parent exit, and treat EOF as teardown. Prove tunnel lifetime
  after unexpected viewer termination before enabling forwarding.
- Keep at most 32 validated framebuffer damage regions per update, merging
  overlaps and falling back to one full-frame upload at the cap. This avoids
  uploading large empty areas between distant screen changes. A worker wakes
  the UI on frame and control changes. Never poll at a fixed refresh rate when
  nothing changes. Keep recording work outside the viewer update path.
- Gate VNC transport, RDP certificate trust and redirection before sending
  credentials or local data. Use the configured SSH identity for loopback
  forwards and preserve the remote name for TLS validation.
- Baseline launch, resize/input and failure recovery come before optional
  protocol channel wiring. Exercise X11 and Wayland modifier handling and use
  an explicit Send keys action where the desktop reserves shortcuts.

No connection, platform interoperability, or performance measurements have
been completed. Record results here only after a runnable viewer is measured.

## Direct connection checks

The host editor checks TCP reachability and the initial
[RDP security negotiation](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/b2975bdc-6d56-49ee-9c57-f2ff3a0b6817)
or [RFB version and security offer](https://www.rfc-editor.org/rfc/rfc6143.html).
The probe stops before authentication and does not load passwords. Its success
status means that the protocol endpoint responded. VNC checks run on the direct
route only when explicitly configured. Tunnel and gateway checks require their
respective transport implementations. The system DNS resolver runs on the
probe worker and controls resolution time independently of the socket deadline.

## VNC transport and current limits

The live RFB implementation validates display dimensions and rectangle lengths
before allocating and requests incremental updates. It supports Raw, CopyRect,
and ZRLE rectangles (raw, solid, packed-palette, and RLE tiles), classic VNC
password authentication, SSH stdio forwarding on Unix and Windows, keyboard
and pointer input, clipboard text, and server-supported resolution changes.
Quality selects 16-bit or 32-bit pixels. Direct VNC to a
public or unknown address requires an explicit warning confirmation. The
configured encoding priority is applied for the supported encodings; Tight is
shown as unsupported and skipped. Username-based authentication and remote
cursor shapes are unsupported. ZRLE uses one bounded persistent zlib stream per
connection. Classic RFB
clipboard text is Latin-1. Only the earlier loopback Raw handshake/rendering
smoke test is recorded; current end-to-end input, tunnel, authentication, and
encoding interoperability need real-server validation.

Classic VNC password negotiation uses the [RFB security handshake](https://www.rfc-editor.org/rfc/rfc6143.html#section-7.2.2)
and [RustCrypto DES 0.8.1](https://docs.rs/des/0.8.1/des/) with its zeroize feature.
DES is used only for this legacy challenge response. A direct session itself is
unencrypted, so public or unknown endpoints trigger a confirmation before use.
A configured password requires VNC authentication and rules out unauthenticated
negotiation. Username-based security modes need a separate implementation.

## SSH stream transport

On Unix, OpenSSH stdio forwarding (`ssh -W 127.0.0.1:PORT`) runs over a private
Unix socket pair, avoiding a local listening port. On Windows, OpenSSH forwards
to a loopback TCP port chosen from the OS ephemeral range; the viewer owns the
child in a kill-on-close job object. Both paths use the host's key, agent, and
host-key preferences. Unix password authentication uses an inherited
descriptor with sshpass. Windows background tunnels require a key or agent;
saved SSH passwords are not passed to hidden processes. The full Windows app
cross-check could not run here because MinGW is missing, but the Windows tunnel
module passed an isolated target `cargo check`. End-to-end tunnel behavior still
needs validation on a Windows machine and real VNC/RDP servers.

## VNC input

Click the remote canvas to capture keyboard input. On focus loss or a click
outside the canvas, the viewer releases tracked keys, modifiers and buttons.
The VNC worker omits its input channel in view-only mode. A bounded queue
disconnects on overflow so a key release cannot silently disappear. Pointer
coordinates follow the displayed canvas and are clamped during a drag.
Classic RFB forwards left, middle, and right buttons plus both scroll axes;
side buttons have no distinct classic RFB event and are ignored. RDP forwards
the two extended side buttons with MouseEventEx. IME, host-key
shortcuts and platform keyboard capture are pending. Input mapping and focus
release were tested in-process. Synthetic desktop input did not reach the
loopback peer in the X11 smoke test, so end-to-end input behavior remains
unverified.

## Clipboard sync

The clipboard worker reads local content only when the configured direction
allows it and the connected viewer is focused. Clipboard access stays off the
egui thread. Remote text and images are written to the local clipboard and
fingerprinted to prevent sending them back. Text over 1 MiB and images over
16 MiB are skipped. Native clipboard initialization retries once per second if
the OS clipboard is temporarily unavailable. RDP supports Unicode text and
bounded DIB images; classic VNC supports Latin-1 text only. File clipboard
formats are not implemented. End-to-end clipboard behavior still needs testing
against real RDP and VNC servers and on Wayland, X11, and Windows.

## Native RDP transport

The viewer uses the blocking adapter matching [IronRDP 0.17](https://docs.rs/crate/ironrdp/0.17.0)
inside its own process. The worker negotiates NLA by default and supports an
explicit TLS selection. It verifies CA trust and the original host name before
CredSSP or graphical logon, including when SSH supplies the underlying byte
stream. The Rustls provider is chosen explicitly and TLS resumption is disabled.
RD Gateway,
Kerberos/KDC requests, and legacy unencrypted RDP are not accepted. IronRDP holds
its own copy of the password during connection setup. The launcher zeroizes
its own working copy and does not log connector errors or credentials.

Initial dimensions are validated before allocating the decoded framebuffer.
Damage from the graphics stage feeds the common partial texture renderer.
Input and display control are wired separately from graphics. When enabled, the
profile requests two side-by-side remote monitors whose combined width and height
come from the selected resolution. The viewer shows them in one combined window
and disables fit-to-window resize to preserve the layout. IronRDP exposes the
server's aggregate display-area limit but not its monitor-count limit, so the
server may still reject the two-monitor request. Audio, microphone, drive
redirection, and RD Gateway remain unsupported.
The editor keeps audio, microphone, and drive-sharing profile values for
compatibility, but presents those controls as unsupported and read-only until
their channels are implemented.
IronRDP and its blocking adapter are available under MIT or Apache-2.0.

The optional live FPS value counts received display-update batches per second
from the RDP/VNC workers. It is not the remote compositor's native capture rate.
The viewer Stats menu also shows the most recent one-second protocol receive
rate separately from the session-wide average.

RDP keyboard and pointer events use IronRDP fast-path input on the worker that
owns the TLS connection. The worker drains a bounded queue between reads.
A 10 ms read timeout lets input reach an otherwise quiet connection without
periodic UI repaint requests. The current mapping covers the common PC
scancodes, five RDP pointer buttons, and both scroll axes. RDP scancode mapping
uses egui's physical key when available, while VNC keeps logical keysyms. IME
and a full interactive RDP server test remain pending.

Unknown-issuer RDP certificates can now be explicitly trusted from the
viewer after comparing the SHA-256 fingerprint. Validity dates and TLS
handshake signatures are checked for all certificates. System-trusted
certificates continue to use the platform trust store. Always trust records
the leaf fingerprint after TLS succeeds and before credentials are sent. The
store is scoped to the saved config, host, port, and connection route. A
changed saved fingerprint blocks the connection even if the new certificate
is signed by a trusted CA. The Remote Desktop settings tab lists saved pins by
host and lets the user remove one; the next connection then shows the trust
prompt again.

## Screenshot capture

The Screenshot button copies the latest remote framebuffer and writes it as
an opaque RGB PNG on a worker thread. Fit and Fill change the display only.
By default files are saved to the system Pictures folder under `ServerLauncher`.
The writer publishes a completed PNG without overwriting an existing file.
The viewer reports the result with open-file and show-folder actions.
The user can choose a screenshot folder in Remote Desktop settings; leaving it
unset uses Pictures/ServerLauncher. Screenshot clipboard copies and host-key
can be enabled as a viewer default or toggled per session. Host-key shortcuts
remain pending.

Recordings use FFmpeg H.264 at 30 fps when available and fall back to a PNG
sequence when FFmpeg is missing. The recording destination and frame rate
(15, 24, 30, or 60 FPS) are configurable in Remote Desktop settings, with
Pictures/ServerLauncher and 30 FPS as defaults. Users can select MP4 (H.264)
or a PNG sequence directly; the PNG fallback remains available when FFmpeg is
missing.


In Fit Window mode, RDP sends a single-monitor display request after the
window has settled for 180 ms. Requests are bounded by local framebuffer
limits and the server's advertised monitor area. Server reactivation updates
the share ID and graphics decoder and allocates a validated framebuffer for
the new size, restoring the negotiated bulk decompressor for fast-path updates.
The RDP profile exposes Smooth/Fast texture filtering, Incremental/Full-frame
painting, and Auto/Best/Good/Fast bitmap quality. Auto retains manual color-depth
selection; Best requests 32-bit without lossy bitmap flags, while Good and Fast
request 24-bit and 16-bit and allow dynamic color fidelity/subsampling. These
capabilities allow the server to adapt; they do not guarantee a specific frame
rate or speedup. The viewer's live FPS counts received display-update batches,
not the server's capture FPS or local rendered FPS.
Dimension normalization has unit coverage. A live resize against Windows
RDP or xrdp still needs verification.
