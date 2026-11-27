# RDP/VNC service manager implementation notes

## Current boundary

The current implementation performs a read-only SSH inventory scan and a client-side protocol handshake. It shows copyable manual instructions for documented Debian and Ubuntu hosts. For verified Debian/Ubuntu xrdp systemd units, the RDP tab offers a review dialog for install/start, start, stop, restart, and enable-at-boot. The Run button executes only the displayed fixed command plan on a background worker, then rescans. Approved APT update/install steps run with `DEBIAN_FRONTEND=noninteractive` and wait up to 60 seconds for APT locks; package steps have a five-minute command timeout. Sudo passwords are held in zeroizing memory for this operation and sent only on SSH stdin; password-required calls use `sudo -k -S` so they do not rely on or refresh sudo's timestamp cache. A passwordless `sudo -n` check avoids sending an entered password when the account does not need one. A rejected password can be re-entered up to three times before the flow stops. Passwords are redacted from captured command output before it reaches the UI. Other distributions, TigerVNC mutations, listener configuration edits, and firewall changes remain manual-only. The action dialog reports each command boundary and bounded output summaries; cancellation waits for the current command to finish. It does not yet show byte/percentage progress within a package command.

The inventory script emits bounded `SLD/1` tab-separated records. Raw output remains available in the UI for troubleshooting. The RDP and VNC views filter their protocol records independently; shared host facts are shown in both.

## Documentation-verified package facts

| Platform | RDP | VNC | Automation status |
|---|---|---|---|
| Debian/Ubuntu | Ubuntu publishes `xrdp`; its Noble package includes `xrdp.service` and `xrdp-sesman.service`. Upstream documents port 3389 and the `apt install xrdp` package. | Ubuntu publishes `tigervnc-standalone-server`; TigerVNC documents `-localhost`, `-rfbport`, and the default `5900 + display` mapping. | xrdp install/start and systemd controls are available through a reviewed plan for detected Debian/Ubuntu systemd hosts. TigerVNC remains manual-only because per-user service mapping and password setup are not yet verified by this app. |
| Fedora/RHEL | Fedora publishes `xrdp`; upstream says non-Fedora RHEL derivatives need EPEL. | Fedora publishes `tigervnc-server`; unit and per-user mapping vary by release/configuration. | Manual instructions only for now. No EPEL enablement or package/service mutation is automated. |
| Arch, openSUSE, other/unknown | Not verified for this feature. | Not verified for this feature. | Manual-only; no guessed package names or units. |

Documentation does not equal runtime verification. No distro has yet been exercised by this application’s service manager. The UI must keep that distinction clear.

## Security and execution decisions

- Scans are read-only and start only when the RDP/VNC editor is shown or the user requests a rescan; results are cached briefly.
- The existing application has a session-level remote sudo password cache. Service actions do not reuse it. The review dialog uses an operation-scoped password with up to three attempts; cancellation is available between plan steps only.
- Do not start TigerVNC without a password. Do not place a VNC password in argv, shell history, logs, or `servers.json`.
- Never alter host firewalls or cloud security groups. Prefer a verified loopback listener and SSH tunnel; do not claim a listener is private when evidence is incomplete.
- Do not edit xrdp listen-address configuration until the installed version’s syntax and a backup/restore path are verified.
- Do not kill a manual VNC process unless a later action flow explicitly confirms its PID and owner.

## Backward compatibility

Rust keeps `ssh_hosts` and all existing keys; new protocol fields must remain serde-defaulted. A read-only inspection of the Python editions found that their current persistence writers serialize only their own server list (`{"servers": ...}` or a legacy list). If one of those editions rewrites the same file, it can drop Rust-only `ssh_hosts` and protocol settings. Python files are out of scope here; users should not edit the same config file from those editions until their persistence behavior is changed separately.

## Sources checked

- [xrdp upstream quick start and platform notes](https://github.com/neutrinolabs/xrdp) — package names, default port, and distribution notes.
- [xrdp upstream systemd unit template](https://github.com/neutrinolabs/xrdp/blob/devel/instfiles/xrdp.service.in) — unit dependency on `xrdp-sesman.service`.
- [Ubuntu Noble xrdp package file list](https://packages.ubuntu.com/noble/amd64/xrdp/filelist) — packaged unit files and configuration path.
- [Ubuntu package search for xrdp](https://packages.ubuntu.com/search?arch=any&keywords=xrdp&searchon=sourcenames&section=all&suite=all) — supported Ubuntu releases/package availability.
- [Ubuntu package search for TigerVNC standalone server](https://packages.ubuntu.com/search?keywords=tigervnc-standalone-server) — package availability.
- [Debian TigerVNC server manual](https://manpages.debian.org/unstable/tigervnc-standalone-server/vncserver.1.en.html) — `-rfbport`, `-localhost`, and display-to-port behavior.
- [TigerVNC upstream Xvnc manual](https://github.com/TigerVNC/tigervnc/blob/master/unix/xserver/hw/vnc/Xvnc.man) — port, security-type, and clipboard options.
- [TigerVNC upstream per-user systemd template](https://github.com/TigerVNC/tigervnc/blob/master/unix/vncserver/vncserver%40.service.in) — user mapping/configuration is required; do not assume a single unit instance.
- [Debian APT lock implementation](https://github.com/Debian/apt/blob/main/apt-pkg/deb/debsystem.cc) — confirms `DPkg::Lock::Timeout` is read by APT's dpkg lock path; current automation sets it to 60 seconds.
- [Fedora xrdp package](https://packages.fedoraproject.org/pkgs/xrdp/xrdp/) and [Fedora TigerVNC server package](https://packages.fedoraproject.org/pkgs/tigervnc/tigervnc-server/) — package availability only, not proof of install/start behavior on a target host.

## Validation record

- Parser fixtures cover the supplied 3389 listener without process metadata, a manual TigerVNC process on `:1`/5901, percent-escaped owner data, malformed/truncated records, duplicate singleton fields, IPv4/IPv6 wildcard binds, `*`, localhost, a specific address, multiple VNC displays, and unknown service state.
- Manual command generation is limited to `ID=debian` and `ID=ubuntu`; unknown distributions receive no guessed package/service commands. RDP listener verification uses the selected profile port. The VNC start command includes localhost binding and the selected numeric port.
- The service card presents a status-colored outline/pill, a first-scan placeholder, collapsed host-wide details, protocol-filtered raw output, and manual commands with per-command and Copy all controls. These UI changes have been compile-checked but not visually tested.
- Planner tests now cover the fixed command for each supported xrdp service intent. Test targets have only been type-checked; unit tests were not executed.
- The xrdp plan builder is limited to Debian/Ubuntu with systemd and both xrdp units detected. Restrict-to-localhost plans and all VNC service plans are rejected until their configuration/user-unit behavior is verified.
- Listener inspection tries `ss`, then `netstat`, then `/proc/net/tcp*`; IPv6 fallback entries without a reliable address decode are explicitly classified as unknown exposure.
- `cargo check --locked --all-targets` passed on the authorized SSH build PC after syncing the read-only inventory implementation.
- Parser and manual-command unit tests were type-checked as targets but not executed. No live service install/start or distro runtime test was performed.
