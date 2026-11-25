# Remote Desktop design

## Repository baseline

Work starts on `vnc/rdp` from `fix/ui-alignment`. The native viewer is scoped
to `server_launcher_rust`. The separate Windows package mirrors much of the
launcher but will need its own integration and native validation before it can
ship the viewer. The Python launchers share the host store. Keep the current
keys and omit unset protocol profiles when saving.

The current UI already has a disabled GUI control in the SSH header in
`app.rs`. `modals.rs` owns the host editor and command palette, `config.rs`
owns host JSON, and `main.rs` dispatches the existing separate Android Cockpit
mode before opening the main window. `remote.rs` provides SSH arguments and
the port inspector. Reuse the active `ThemePalette` from `theme.rs`. The
baseline repository has rustfmt drift, so check new files individually and
avoid repository-wide formatting churn.

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
- Coalesce damage into bounded, validated framebuffer regions. Retain the
  latest state even when intermediate notifications drop. A worker wakes the
  UI on frame and control changes. Never poll at a fixed refresh rate when
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

## VNC viewer milestone

The first live viewer transport is a bounded Raw RFB implementation based on
[RFC 6143](https://www.rfc-editor.org/rfc/rfc6143.html). It validates display
dimensions and rectangle lengths before allocating and asks for incremental
updates after the first frame. This establishes the native rendering and
worker-to-viewer path without depending on compressed decoders yet. The
loopback smoke test verified the RFB handshake, Raw frame rendering and
incremental request on X11. Production VNC authentication, secure tunneling,
input, and additional encodings still need implementation. This milestone
accepts only an explicitly configured direct private endpoint and refuses
connections that would ignore configured credentials.

Classic VNC password negotiation uses the [RFB security handshake](https://www.rfc-editor.org/rfc/rfc6143.html#section-7.2.2)
and [RustCrypto DES 0.8.1](https://docs.rs/des/0.8.1/des/) with its zeroize feature.
DES is used only for this legacy challenge response. The session itself is
unencrypted on a direct connection, and public endpoints remain blocked.
A configured password requires VNC authentication and rules out unauthenticated
negotiation. Username-based security modes need a separate implementation.

## SSH stream transport

VNC uses OpenSSH stdio forwarding (`ssh -W 127.0.0.1:PORT`) over a private
Unix socket pair. This reaches the same remote loopback service as a local
forward, without binding a local listening port or racing another process
for that port. It uses the host's key, agent, and host-key preferences. On
Unix, password authentication uses an inherited descriptor with sshpass.
The viewer worker owns the child and reaps it when the stream ends. Linux
also requests a parent-death signal for the direct child. Forced termination
of the password wrapper's descendants has not been verified. Windows and
editor connection tests do not yet use this transport.

## VNC input

Click the remote canvas to capture keyboard input. On focus loss or a click
outside the canvas, the viewer releases tracked keys, modifiers and buttons.
The VNC worker omits its input channel in view-only mode. A bounded queue
disconnects on overflow so a key release cannot silently disappear. Pointer
coordinates follow the displayed canvas and are clamped during a drag.
Classic RFB currently forwards the primary three buttons and both scroll
axes. Extra buttons, IME, host-key shortcuts and platform keyboard capture
are pending. Input mapping and focus release were tested in-process.
Synthetic desktop input did not reach the loopback peer in the X11 smoke
test, so end-to-end input behavior remains unverified.

## Native RDP transport

The viewer uses the blocking adapter matching [IronRDP 0.17](https://docs.rs/crate/ironrdp/0.17.0)
inside its own process. The worker negotiates NLA by default and supports an
explicit TLS selection. It verifies CA trust and the original host name before
CredSSP or graphical logon, including when SSH supplies the underlying byte
stream. The Rustls provider is chosen explicitly and TLS resumption is disabled.
Certificate exceptions and per-host fingerprint prompts are pending. RD Gateway,
Kerberos/KDC requests, and legacy unencrypted RDP are not accepted. IronRDP holds
its own copy of the password during connection setup. The launcher zeroizes
its own working copy and does not log connector errors or credentials.

Initial dimensions are validated before allocating the decoded framebuffer.
Damage from the graphics stage feeds the common partial texture renderer.
Reactivation, input, display control, and redirection channels are separate work.
IronRDP and its blocking adapter are available under MIT or Apache-2.0.

RDP keyboard and pointer events use IronRDP fast-path input on the worker that
owns the TLS connection. The worker drains a bounded queue between reads.
A 30 ms read timeout lets input reach an otherwise quiet connection without
periodic UI repaint requests. The current mapping covers the common PC
scancodes, three pointer buttons, and both scroll axes. Physical keyboard
layout mapping, IME, extra buttons, and a full interactive RDP server test
remain pending.
