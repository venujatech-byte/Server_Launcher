# Remote Desktop design

## Repository baseline

Work starts on `vnc/rdp` from `fix/ui-alignment`. The native viewer is scoped
to `server_launcher_rust`. The Windows release workflow packages this same
Rust app so RDP/VNC are included in the distributed Windows build. The separate
`server_launcher_windows` crate remains outside this feature. The Python
launchers share the host store. Keep the current keys and omit unset protocol
profiles when saving.

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
  secrets, including the normalized RD Gateway endpoint in RDP bindings.
  Changing a gateway requires saving a new credential; legacy unscoped
  credentials are not retried for gateway profiles. Surface lookup failures in
  the viewer.
- Identify sessions by host ID and protocol. Start the same executable in
  session mode with those identifiers only. Use a bounded control channel,
  send Close on parent exit, and treat EOF as teardown. Prove tunnel lifetime
  after unexpected viewer termination before enabling forwarding.
- Persist each viewer's native window geometry separately by host ID and
  protocol so opening a different session cannot reuse another session's size.
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

Compile checks on 2026-11-26 passed for Linux (`cargo check --locked
--all-targets` on the SSH test PC) and Windows x86_64 MSVC (`cargo check
--locked --all-targets --target x86_64-pc-windows-msvc` in GitHub Actions) at
`8862062`. The Linux test PC pulled that commit from `vnc/rdp`; Windows CI run
[45858003619](https://github.com/venujatech-byte/Server_Launcher/actions/runs/45858003619)
also checked all targets. These checks do not prove that either viewer connects
or that clipboard behavior works. No connection, platform interoperability, or
performance measurements have been completed. Record those results here only
after a runnable viewer is measured.

## Display profiles and input controls

RDP profiles expose Automatic, GFX RFX, GFX RFX Progressive, GFX AVC420,
GFX AVC444, legacy RemoteFX, 32/24-bit true color, 16/15-bit high color, and
256-color (8 bpp) bitmap mode. AVC profiles use the optional OpenH264 decoder
loaded from Settings → Remote Desktop. The app does not bundle this library;
the loader accepts supported Cisco release hashes. If no path is configured,
IronRDP does not advertise AVC and the server can use another supported codec.
The 8 bpp profile disables EGFX so the server uses the palette-aware legacy
bitmap path. The EGFX path supports
IronRDP's ClearCodec and RFX decoders. Explicit GFX RFX profile choices limit
the advertised capability version. GFX AVC420 advertises AVC420-capable
capabilities; GFX AVC444 advertises AVC444 and AVC420 capability sets, each
with lower-version fallbacks. Automatic lets the server choose among paths
supported by the configured decoder. The legacy RemoteFX
bitmap codec is advertised but cannot be forced: the server selects the codec
during negotiation. Older `quality` and `color_depth` fields remain intact and
are used when no explicit `color_profile` is saved.
RDP and VNC can disable fractional wheel accumulation; RDP can also swap the
primary and secondary pointer buttons for left-handed use. These are local
input/display settings, not protocol performance modes. The VNC editor groups
the existing profile options into Advanced, Behavior, and SSH Tunnel sections.
New VNC profiles request Tight, ZRLE, CopyRect, and Raw encodings. Tight
supports fill, copy, palette, gradient, JPEG, and PNG subencodings with bounded
rectangle sizes and four persistent zlib streams. The profile can turn remote cursor display off; by default,
the viewer requests RichCursor or XCursor shapes plus PointerPos updates and
draws the cursor over the scaled framebuffer.
Remmina's server-side local-input lock is not exposed, and bell messages are
ignored. Tight gradient decoding currently requires the negotiated 24-bit color
format; a 16-bit gradient rectangle is rejected with a clear error.

## Direct connection checks

The host editor checks TCP reachability and the initial
[RDP security negotiation](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/b2975bdc-6d56-49ee-9c57-f2ff3a0b6817)
or [RFB version and security offer](https://www.rfc-editor.org/rfc/rfc6143.html).
Direct and SSH-tunneled probes stop before desktop authentication and do not
load desktop passwords. Gateway probes use the saved RDP credential only for
gateway sign-in, then stop after the RDP security handshake; they do not log in
to the desktop. A successful check means the protocol endpoint responded.
Checks use the configured direct, SSH tunnel, or RD Gateway route. The system
DNS resolver runs on direct probes; SSH and gateway routes resolve through their
configured transport.

Opening either protocol tab also starts a bounded, read-only SSH scan for
listeners on TCP 3389 and 5900–5910 and common Linux RDP/VNC server processes.
It reuses the host's SSH key, agent, host-key policy, and saved SSH password
path. The scan reports setup guidance when no service is found; it never
installs packages or changes the remote machine.

## VNC transport and current limits

The live RFB implementation validates display dimensions and rectangle lengths
before allocating and requests incremental updates. It supports Tight (fill,
copy, palette, gradient, JPEG and PNG), Raw, CopyRect, and ZRLE rectangles (raw,
solid, packed-palette, and RLE tiles), classic VNC
password authentication, SSH stdio forwarding on Unix and Windows, keyboard
and pointer input, clipboard text, and server-supported resolution changes.
Fast quality selects RGB565; Auto follows the configured color depth. When Tight
is enabled, Best/Good/Fast also send the Tight compression and JPEG quality
pseudo-encodings; Auto leaves those choices to the server. Servers may ignore
these optional hints. Direct VNC to a public or unknown address requires an
explicit warning confirmation. The configured encoding priority is applied for
the supported encodings.
Username-based authentication is unsupported.
Rich and X cursor shapes are drawn locally when the server supports either
cursor encoding and a PointerPos extension. ZRLE uses one bounded persistent zlib stream per
connection. Classic RFB clipboard text is Latin-1; outgoing Unicode text and
images outside that support show a notice in the viewer. Only the earlier
loopback Raw handshake/rendering smoke test is recorded; current end-to-end
input, tunnel, authentication, and encoding interoperability need real-server
validation.

The VNC TCP user timeout profile is applied with `TCP_USER_TIMEOUT` only for
direct Linux TCP connections. Zero leaves the OS default; SSH-tunneled and
non-Linux connections ignore the setting. It bounds time spent retrying
unacknowledged data and does not disconnect an otherwise idle session.

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
descriptor with sshpass. Windows password authentication uses OpenSSH askpass;
the hidden helper loads the saved credential from the OS credential store and
writes it only to the SSH process's askpass pipe. The password is not passed in
arguments or environment variables. Windows tunnels with password-only SSH
profiles still need runtime validation on Windows and real VNC/RDP servers.

## Viewer input

Clicking the remote canvas captures pointer input and enables keyboard input.
The Keyboard toolbar toggle also enables keyboard input without a canvas click
while the viewer window is focused. On focus loss or a click outside the
canvas, the viewer releases tracked keys, modifiers and buttons. The keyboard
toggle remains enabled across focus changes so native capture can resume when
the viewer regains focus.
The VNC worker omits its input channel in view-only mode. A bounded queue
disconnects on overflow so a key release cannot silently disappear. Pointer
coordinates follow the displayed canvas and are clamped during a drag.
Classic RFB forwards left, middle, and right buttons plus both scroll axes;
side buttons have no distinct classic RFB event and are ignored. RDP forwards
the two extended side buttons with MouseEventEx. The viewer forwards committed
non-ASCII egui IME text where the platform backend provides commit events;
preedit candidates are not sent. Linux egui-winit 0.29 drops IME events, so
Linux composition still depends on text events delivered by its keyboard
backend. Host-key shortcut prefixes remain pending; native keyboard grabbing
currently supports Linux/X11 only.
Input mapping and focus release were tested in-process. Synthetic desktop input
did not reach the loopback peer in the X11 smoke test, so end-to-end input
behavior remains unverified.

## Clipboard sync

The clipboard worker reads local content only when the configured direction
allows it and the connected viewer is focused. Clipboard access stays off the
egui thread. Remote text and images are written to the local clipboard and
fingerprinted to prevent sending them back. Text over 1 MiB is skipped; the
RDP image limit is configurable from 1 to 64 MiB (16 MiB by default) and applies
in both directions. Native clipboard initialization retries once per second if
the OS clipboard is temporarily unavailable. Linux uses Wayland data-control
when available and falls back to X11 when Wayland clipboard initialization
fails. Remote clipboard values remain queued when a local clipboard write fails
and are retried while the viewer is focused; a newer remote value takes priority
over an older retry. RDP supports Unicode text and bounded DIB images; classic
VNC supports Latin-1 text only.
File clipboard formats are not implemented. End-to-end clipboard behavior still
needs testing against real RDP and VNC servers and on Wayland, X11, and Windows.

## Native RDP transport

The viewer uses the blocking adapter matching [IronRDP 0.17](https://docs.rs/crate/ironrdp/0.17.0)
inside its own process. Auto advertises NLA and encrypted TLS; explicit NLA-only
and TLS-only selections remain available. The worker verifies CA trust and the original host name before
CredSSP or graphical logon, including when SSH supplies the underlying byte
stream. The Rustls provider is chosen explicitly and TLS resumption is disabled.
Kerberos/KDC requests and legacy unencrypted RDP are not accepted. IronRDP holds
its own copy of the password during connection setup. The launcher zeroizes
its own working copy and does not log connector errors or credentials.

Initial dimensions are validated before allocating the decoded framebuffer.
Damage from the graphics stage feeds the common partial texture renderer.
The client requests the server pointer and composites its shape into the
software framebuffer so the remote cursor follows pointer movement in the
viewer. This adds cursor pixels to framebuffer damage; it does not change the
server's display-update rate.
Input and display control are wired separately from graphics. When enabled, the
profile requests two side-by-side remote monitors whose combined width and height
come from the selected resolution. The viewer shows them in one combined window
and disables fit-to-window resize to preserve the layout. IronRDP exposes the
server's aggregate display-area limit but not its monitor-count limit, so the
server may still reject the two-monitor request. RDP playback uses the native
IronRDP audio backend when enabled. Microphone capture is opt-in and registers
IronRDP's AUDIO_INPUT dynamic channel only for profiles that enable it; capture
starts only when the remote requests the channel. Outbound audio uses a bounded
queue to the RDP worker. The implementation has passed the SSH PC cargo check
but still needs live server and input-device testing. RDP drive redirection now
shares the configured local folders, defaults each new share to read-only, and
requires confirmation before adding a share or granting remote write access.
Windows uses IronRDP's native backend;
Linux and macOS route each share to an isolated root, reject write requests for
read-only shares, and rely on IronRDP's path resolver to prevent traversal and
symlink escape. Drive access and read-only enforcement still need real-server
validation. RD Gateway uses the pinned IronRDP MS-TSGU transport over HTTPS and
the saved RDP username/password for gateway authentication. Gateway TLS uses
strict system-root validation; SSH tunneling cannot be combined with this route.
This path still needs a live gateway test. RDP administrative sessions request
session ID 0 through GCC cluster data. The VNC username field remains visible
but unsupported by the classic VNC authentication implementation.
IronRDP and its blocking adapter are available under MIT or Apache-2.0.

The RDP viewer now registers IronRDP's EGFX dynamic channel before advertising
`SUPPORT_DYN_VC_GFX_PROTOCOL`. IronRDP composites supported EGFX updates into
the same persistent framebuffer as legacy bitmap updates, so both paths feed
the existing incremental upload and live display-update counter. The client
uses IronRDP's built-in ClearCodec/RFX decoders and can load its OpenH264
decoder from an official supported Cisco library selected in Remote Desktop
settings. Without that library the client does not advertise AVC and the
server can fall back to supported codec sets. Smooth/Fast filtering and
incremental/full-frame painting remain local rendering choices and cannot
create additional server frames.
Direct RDP and the Windows tunnel socket already enable TCP_NODELAY.

The screenshot reference is a live desktop framebuffer: received remote
updates are decoded and composited into one persistent image. Full-frame local
repainting redraws that image but does not change the server capture rate. The
coordinated IronRDP stack update and EGFX registration follow the upstream
implementation ([IronRDP issue #1464](https://github.com/Devolutions/IronRDP/issues/1464)).

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

The viewer toolbar's Keyboard toggle routes key events while the viewer window
is focused, without requiring a click inside the remote canvas. On Linux/X11 it
also grabs keyboard input for that focused viewer, so system shortcuts such as
Super and Alt+Tab can reach the remote. The grab is released while focus is
away and resumes when the viewer regains focus; it also ends when capture is
disabled or the session closes. Wayland and Windows use egui capture;
system-reserved shortcuts may stay local there, and Send keys remains the fallback.

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
unset uses Pictures/ServerLauncher. Screenshot clipboard copying can be enabled
as a viewer default or toggled per session. Host-key shortcut prefixes are not
implemented; the Send keys menu remains the fallback for other platforms.

Recordings use FFmpeg H.264 (MP4) or VP9 (WebM) at 30 fps when available and
fall back to a PNG sequence when FFmpeg is missing. The recording destination
and frame rate (15, 24, 30, or 60 FPS) are configurable in Remote Desktop settings, with
Pictures/ServerLauncher and 30 FPS as defaults. Users can select a specific
FFmpeg executable or use `ffmpeg` from `PATH`. Users can select MP4 (H.264) or a
PNG sequence directly; the PNG fallback remains available when FFmpeg is
missing.
Remote Desktop settings also seed quality, clipboard direction, scaling, and
reconnect defaults into newly created protocol profiles; saved host profiles
retain their own values.


In Fit Window mode, RDP sends a single-monitor display request after the
window has settled for 180 ms. Requests are bounded by local framebuffer
limits and the server's advertised monitor area. Server reactivation updates
the share ID and graphics decoder and allocates a validated framebuffer for
the new size. IronRDP's active-stage builder restores fast-path decompression
state during reactivation. The RDP profile exposes Smooth/Fast texture
filtering, Incremental/Full-frame
repainting, and Auto/Best/Good/Fast bitmap quality. Full-frame repaint is a
client-side compatibility mode: it redraws the complete image after each
received update, but does not alter server rendering or network behavior. Auto retains manual color-depth
selection; Best requests 32-bit without lossy bitmap flags, while Good and Fast
request 24-bit and 16-bit and allow dynamic color fidelity/subsampling. These
capabilities allow the server to adapt; they do not guarantee a specific frame
rate or speedup. The viewer's live FPS counts received display-update batches,
not the server's capture FPS or local rendered FPS.
Dimension normalization has unit coverage. A live resize against Windows
RDP or xrdp still needs verification.
