use super::{credentials::DesktopProtocol, profile::RdpSecurity};
use std::{
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

type DetectionResult = Result<String, String>;

#[derive(Default)]
pub(crate) struct ServerDetection {
    pending: Option<(String, std::sync::mpsc::Receiver<DetectionResult>)>,
    last: Option<(String, DetectionResult)>,
}

impl ServerDetection {
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        host: &crate::config::SshRemoteHost,
        timeout: Duration,
    ) {
        let key = format!("{}:{}@{}", host.user, host.port, host.host);
        if let Some((_, receiver)) = &self.pending {
            use std::sync::mpsc::TryRecvError;
            match receiver.try_recv() {
                Ok(result) => {
                    self.last = self.pending.take().map(|(key, _)| (key, result));
                }
                Err(TryRecvError::Disconnected) => {
                    self.last = self
                        .pending
                        .take()
                        .map(|(key, _)| (key, Err("The server scan stopped unexpectedly.".into())));
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        let can_scan = !host.host.trim().is_empty() && !host.user.trim().is_empty();
        let should_scan = can_scan
            && self.pending.is_none()
            && self
                .last
                .as_ref()
                .map_or(true, |(tested, _)| tested != &key);
        if should_scan {
            self.start(host.clone(), key.clone(), timeout, ui.ctx().clone());
        }

        ui.horizontal(|ui| {
            ui.strong("Remote desktop server detection");
            if self.pending.is_some() {
                ui.spinner();
                ui.label("Checking over SSH…");
            }
            if ui
                .add_enabled(
                    can_scan && self.pending.is_none(),
                    egui::Button::new("Scan again"),
                )
                .clicked()
            {
                self.start(host.clone(), key.clone(), timeout, ui.ctx().clone());
            }
        });
        if let Some((_, result)) = self.last.as_ref().filter(|(tested, _)| tested == &key) {
            match result {
                Ok(report) => {
                    ui.add(egui::Label::new(report.as_str()).selectable(true).wrap());
                }
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            }
        } else if !can_scan {
            ui.label("Enter the SSH host and username to scan.");
        }
    }

    fn start(
        &mut self,
        host: crate::config::SshRemoteHost,
        key: String,
        timeout: Duration,
        ctx: egui::Context,
    ) {
        let (sender, receiver) = std::sync::mpsc::channel();
        self.pending = Some((key, receiver));
        std::thread::spawn(move || {
            let _ = sender.send(detect_servers(&host, timeout));
            ctx.request_repaint();
        });
    }
}

fn detect_servers(host: &crate::config::SshRemoteHost, timeout: Duration) -> DetectionResult {
    let command = crate::remote::build_ssh_command(
        host,
        "if command -v ss >/dev/null 2>&1; then ss -ltnp 2>/dev/null | awk 'NR == 1 || /:3389([^0-9]|$)|:590[0-9]([^0-9]|$)|:5910([^0-9]|$)/'; else echo 'ss is unavailable'; fi; ps -eo comm= 2>/dev/null | grep -E '^(xrdp|Xvnc|x11vnc|Xtigervnc|vncserver)$' || true",
    );
    let output = crate::remote::ssh_probe_output(command, timeout)
        .map_err(|error| format!("SSH scan failed: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if error.is_empty() {
            format!("SSH scan exited with status {:?}.", output.status.code())
        } else {
            format!("SSH scan failed: {error}")
        });
    }
    Ok(format_detection_report(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn format_detection_report(report: &str) -> String {
    let matches = report
        .lines()
        .filter(|line| {
            [
                3389, 5900, 5901, 5902, 5903, 5904, 5905, 5906, 5907, 5908, 5909, 5910,
            ]
            .into_iter()
            .any(|port| contains_port(line, port))
                || matches!(
                    line.trim(),
                    "xrdp" | "Xvnc" | "x11vnc" | "Xtigervnc" | "vncserver"
                )
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        "No listener found on RDP 3389 or VNC 5900–5910. Linux setup examples (run on the remote host):\nFedora RDP: sudo dnf install xrdp && sudo systemctl enable --now xrdp\nUbuntu/Debian RDP: sudo apt install xrdp && sudo systemctl enable --now xrdp\nVNC: install TigerVNC, set its password, then start a display (for example: vncserver :1).\nA graphical desktop session is required. This scan did not change the host.".into()
    } else {
        format!("Detected remote desktop services:\n{}", matches.join("\n"))
    }
}

fn contains_port(line: &str, port: u16) -> bool {
    let needle = format!(":{port}");
    line.match_indices(&needle).any(|(start, _)| {
        line.as_bytes()
            .get(start + needle.len())
            .map_or(true, |byte| !byte.is_ascii_digit())
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProbeRequest {
    pub host: String,
    pub port: u16,
    pub protocol: DesktopProtocol,
    pub rdp_security: RdpSecurity,
    pub ssh_host: Option<crate::config::SshRemoteHost>,
    pub tunnel_via_ssh: bool,
    pub gateway: Option<String>,
}

pub(crate) fn run(request: &ProbeRequest, timeout: Duration) -> Result<String, String> {
    if request.tunnel_via_ssh
        && request
            .gateway
            .as_deref()
            .is_some_and(|gateway| !gateway.trim().is_empty())
    {
        return Err("RD Gateway and Tunnel via SSH cannot be used together.".into());
    }

    if request.tunnel_via_ssh {
        let ssh_host = request
            .ssh_host
            .as_ref()
            .ok_or_else(|| "SSH tunnel: host settings are unavailable.".to_owned())?;
        let mut stream = super::tunnel::open(ssh_host, request.port, timeout)
            .map_err(|error| format!("SSH tunnel: {error}"))?;
        return check_handshake(request, &mut *stream)
            .map(|info| format!("{info} through the SSH tunnel. Authentication was not tested."));
    }

    if let Some(gateway) = request
        .gateway
        .as_deref()
        .filter(|gateway| !gateway.trim().is_empty())
    {
        if request.protocol != DesktopProtocol::Rdp {
            return Err("RD Gateway can only be used for RDP.".into());
        }
        let host = request
            .ssh_host
            .as_ref()
            .ok_or_else(|| "RD Gateway: host settings are unavailable.".to_owned())?;
        let profile = host
            .rdp
            .as_ref()
            .ok_or_else(|| "RD Gateway: save the RDP profile before testing.".to_owned())?;
        let password = super::credentials::load(host, DesktopProtocol::Rdp)
            .map_err(|error| format!("RD Gateway credentials: {error}"))?
            .ok_or_else(|| "RD Gateway: save the RDP password before testing.".to_owned())?;
        let username = if profile.same_username_as_ssh {
            &host.user
        } else {
            &profile.username
        };
        let (mut stream, _) = super::rdp::open_gateway(
            gateway,
            &request.host,
            request.port,
            username,
            &profile.domain,
            &password,
        )
        .map_err(|error| format!("RD Gateway: {error}"))?;
        return check_handshake(request, &mut *stream)
            .map(|info| format!("{info} through RD Gateway. RDP authentication was not tested."));
    }

    let deadline = Instant::now() + timeout;
    let addresses = (request.host.as_str(), request.port)
        .to_socket_addrs()
        .map_err(|_| "DNS: could not resolve the host. Check the address in General.".to_owned())?;
    let mut connection = None;
    for address in addresses.take(16) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        if let Ok(socket) =
            TcpStream::connect_timeout(&address, remaining.min(Duration::from_secs(3)))
        {
            connection = Some(socket);
            break;
        }
    }
    let hint = match request.protocol {
        DesktopProtocol::Rdp => "Check that Remote Desktop or xrdp is listening.",
        DesktopProtocol::Vnc => "Check that your VNC server is listening on this port.",
    };
    let socket = connection
        .ok_or_else(|| format!("TCP: could not connect to port {}. {hint}", request.port))?;
    let mut stream = TimedSocket { socket, deadline };
    check_handshake(request, &mut stream)
        .map(|info| format!("{info}. Authentication was not tested."))
}

fn check_handshake<S: Read + Write + ?Sized>(
    request: &ProbeRequest,
    stream: &mut S,
) -> Result<String, String> {
    let hint = match request.protocol {
        DesktopProtocol::Rdp => "Check that Remote Desktop or xrdp is listening.",
        DesktopProtocol::Vnc => "Check that your VNC server is listening on this port.",
    };
    let info = match request.protocol {
        DesktopProtocol::Rdp => rdp(stream, request.rdp_security),
        DesktopProtocol::Vnc => vnc(stream),
    }
    .map_err(|error| format!("Handshake: {error}. {hint}"))?;
    Ok(info)
}

struct TimedSocket {
    socket: TcpStream,
    deadline: Instant,
}

impl TimedSocket {
    fn remaining(&self) -> io::Result<Duration> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err(io::Error::new(io::ErrorKind::TimedOut, "probe timed out"))
        } else {
            Ok(remaining)
        }
    }
}

impl Read for TimedSocket {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.socket.set_read_timeout(Some(self.remaining()?))?;
        self.socket.read(buffer)
    }
}

impl Write for TimedSocket {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}

fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason.to_owned())
}

fn rdp<S: Read + Write + ?Sized>(stream: &mut S, security: RdpSecurity) -> io::Result<String> {
    let offered: u8 = match security {
        RdpSecurity::Auto => 1 | 2 | 8,
        RdpSecurity::Nla => 2 | 8,
        RdpSecurity::Tls => 1,
    };
    let request = [
        3, 0, 0, 19, 14, 0xe0, 0, 0, 0, 0, 0, 1, 0, 8, 0, offered, 0, 0, 0,
    ];
    stream.write_all(&request)?;
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    let size = u16::from_be_bytes([header[2], header[3]]);
    if header[..2] != [3, 0] || !matches!(size, 11 | 19) {
        return Err(invalid("expected an RDP packet header"));
    }
    let mut body = [0; 15];
    let length = usize::from(size - 4);
    stream.read_exact(&mut body[..length])?;
    if usize::from(body[0]) + 1 != length || body[1] != 0xd0 || body[6] != 0 {
        return Err(invalid("expected an X.224 connection confirmation"));
    }
    if size == 11 {
        return Err(invalid(
            "the server did not negotiate encrypted RDP security",
        ));
    }
    if body[9..11] != [8, 0] {
        return Err(invalid("invalid RDP negotiation length"));
    }
    let protocol = u32::from_le_bytes([body[11], body[12], body[13], body[14]]);
    match (body[7], protocol, security) {
        (2, 2 | 8, RdpSecurity::Auto | RdpSecurity::Nla) => Ok("RDP endpoint agreed to NLA".into()),
        (2, 1, RdpSecurity::Auto | RdpSecurity::Tls) => Ok("RDP endpoint agreed to TLS".into()),
        (3, 5, _) => Err(invalid("the server requires NLA")),
        (3, _, _) => Err(invalid(&format!(
            "the server rejected RDP negotiation (code {protocol})"
        ))),
        _ => Err(invalid(
            "the server selected an unexpected RDP security mode",
        )),
    }
}

fn vnc<S: Read + Write + ?Sized>(stream: &mut S) -> io::Result<String> {
    let mut banner = [0; 12];
    stream.read_exact(&mut banner)?;
    let version = match &banner {
        b"RFB 003.003\n" => "3.3",
        b"RFB 003.007\n" => "3.7",
        b"RFB 003.008\n" => "3.8",
        _ => {
            return Err(invalid(
                "expected a supported VNC version (3.3, 3.7, or 3.8)",
            ))
        }
    };
    stream.write_all(&banner)?;
    let security = if version == "3.3" {
        let mut kind = [0; 4];
        stream.read_exact(&mut kind)?;
        vec![u8::try_from(u32::from_be_bytes(kind))
            .map_err(|_| invalid("unsupported VNC security type"))?]
    } else {
        let mut count = [0];
        stream.read_exact(&mut count)?;
        let mut types = vec![0; usize::from(count[0])];
        stream.read_exact(&mut types)?;
        types
    };
    let kind = if security.contains(&2) {
        "VNC password authentication offered"
    } else if security.contains(&1) {
        "no authentication offered"
    } else {
        return Err(invalid(
            "the server offered no supported VNC authentication method",
        ));
    };
    Ok(format!("VNC {version} endpoint confirmed, {kind}"))
}

pub(crate) fn menu_status(
    ctx: &egui::Context,
    host: &crate::config::SshRemoteHost,
    protocol: DesktopProtocol,
) -> (&'static str, Option<String>) {
    let (port, rdp_security, tunnel_via_ssh, gateway) = match protocol {
        DesktopProtocol::Rdp => match host.rdp.as_ref().filter(|profile| profile.enabled) {
            Some(profile) => (
                profile.port,
                profile.security,
                profile.tunnel_via_ssh,
                (!profile.gateway.trim().is_empty()).then(|| profile.gateway.clone()),
            ),
            None => return ("○ Not configured", None),
        },
        DesktopProtocol::Vnc => match host.vnc.as_ref().filter(|profile| profile.enabled) {
            Some(profile) => (
                profile.port,
                RdpSecurity::Auto,
                profile.tunnel_via_ssh,
                None,
            ),
            None => return ("○ Not configured", None),
        },
    };
    let mut ssh_host = host.clone();
    ssh_host.password.clear();
    let request = ProbeRequest {
        host: host.host.trim().trim_matches(['[', ']']).into(),
        port,
        protocol,
        rdp_security,
        ssh_host: Some(ssh_host),
        tunnel_via_ssh,
        gateway,
    };
    let last = ctx
        .data_mut(|data| data.get_temp::<(ProbeRequest, ProbeResult)>(probe_result_id(protocol)));
    if let Some((_, result)) = last.filter(|(tested, _)| tested == &request) {
        return match result {
            Ok(info) => ("● Reachable", Some(info)),
            Err(error) => ("◐ Unreachable", Some(error)),
        };
    }
    ("○ Not tested", None)
}

fn probe_result_id(protocol: DesktopProtocol) -> egui::Id {
    egui::Id::new(("remote_desktop_probe_result", protocol as u8))
}

type ProbeResult = Result<String, String>;

#[derive(Default)]
pub(crate) struct ProbeControl {
    pending: Option<(ProbeRequest, std::sync::mpsc::Receiver<ProbeResult>)>,
    last: Option<(ProbeRequest, ProbeResult)>,
}

impl ProbeControl {
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        request: Option<ProbeRequest>,
        unavailable: &str,
        timeout: Duration,
    ) {
        if let Some((tested, receiver)) = &self.pending {
            use std::sync::mpsc::TryRecvError;
            let result = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => {
                    Some(Err("The connection check stopped unexpectedly.".into()))
                }
                Err(TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        probe_result_id(tested.protocol),
                        (tested.clone(), result.clone()),
                    );
                });
                self.last = Some((tested.clone(), result));
                self.pending = None;
            }
        }

        ui.horizontal_wrapped(|ui| {
            let button = ui.add_enabled(
                self.pending.is_none() && request.is_some(),
                egui::Button::new("Test Connection"),
            );
            if button.clicked() {
                if let Some(request) = request.as_ref() {
                    let (sender, receiver) = std::sync::mpsc::channel();
                    let target = request.clone();
                    self.pending = Some((request.clone(), receiver));
                    let ctx = ui.ctx().clone();
                    std::thread::spawn(move || {
                        let _ = sender.send(run(&target, timeout));
                        ctx.request_repaint();
                    });
                }
            }

            if self.pending.is_some() {
                ui.spinner();
                ui.label("Testing…");
            } else if request.is_none() {
                ui.label(unavailable);
            } else if let Some((_, result)) = self
                .last
                .as_ref()
                .filter(|(tested, _)| request.as_ref() == Some(tested))
            {
                match result {
                    Ok(info) => {
                        ui.label("● Protocol reachable").on_hover_text(info);
                    }
                    Err(error) => {
                        ui.label(format!("○ {error}"));
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_transport_settings_for_tunneled_probes() {
        let request = ProbeRequest {
            host: "127.0.0.1".into(),
            port: 5900,
            protocol: DesktopProtocol::Vnc,
            rdp_security: RdpSecurity::Auto,
            ssh_host: None,
            tunnel_via_ssh: true,
            gateway: None,
        };
        assert!(run(&request, Duration::from_secs(1))
            .unwrap_err()
            .contains("host settings are unavailable"));
    }

    #[test]
    fn rejects_gateway_route_for_vnc_before_connecting() {
        let request = ProbeRequest {
            host: "127.0.0.1".into(),
            port: 5900,
            protocol: DesktopProtocol::Vnc,
            rdp_security: RdpSecurity::Auto,
            ssh_host: None,
            tunnel_via_ssh: false,
            gateway: Some("gateway.example.test".into()),
        };
        assert!(run(&request, Duration::from_secs(1))
            .unwrap_err()
            .contains("only be used for RDP"));
    }

    #[test]
    fn server_detection_reports_only_rdp_vnc_ports_and_processes() {
        let report = format_detection_report(
            "State Local Address:Port\nLISTEN 0 128 0.0.0.0:3389 0.0.0.0:*\nLISTEN 0 64 *:5901 *:*\nLISTEN 0 64 *:8080 *:*\nxrdp\nnginx\n",
        );
        assert!(report.contains(":3389"));
        assert!(report.contains(":5901"));
        assert!(report.contains("xrdp"));
        assert!(!report.contains("8080"));
        assert!(!report.contains("nginx"));
        assert!(!contains_port("*:59010", 5901));
        let missing = format_detection_report("ss is unavailable");
        assert!(missing.contains("No listener found"));
        assert!(missing.contains("sudo dnf install xrdp"));
        assert!(missing.contains("sudo apt install xrdp"));
        assert!(missing.contains("vncserver :1"));
    }

    struct Peer {
        response: io::Cursor<Vec<u8>>,
        request: Vec<u8>,
    }

    impl Peer {
        fn new(response: &[u8]) -> Self {
            Self {
                response: io::Cursor::new(response.to_vec()),
                request: Vec::new(),
            }
        }
    }

    impl Read for Peer {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.response.read(bytes)
        }
    }

    impl Write for Peer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.request.write(bytes)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn checks_vnc_banner_and_security_without_sending_credentials() {
        let mut peer = Peer::new(b"RFB 003.008\n\x02\x01\x02");
        assert!(vnc(&mut peer).unwrap().contains("password"));
        assert_eq!(peer.request, b"RFB 003.008\n");
        assert!(vnc(&mut Peer::new(b"RFB 003.008\n\x01\x13")).is_err());
        assert!(vnc(&mut Peer::new(b"RFB 003.003\n\0\0\0\x02")).is_ok());
    }

    #[test]
    fn rejects_rdp_security_downgrade_and_malformed_reply() {
        let mut reply = [3, 0, 0, 19, 14, 0xd0, 0, 0, 0, 0, 0, 2, 0, 8, 0, 2, 0, 0, 0];
        let mut auto = Peer::new(&reply);
        assert!(rdp(&mut auto, RdpSecurity::Auto).is_ok());
        assert_eq!(auto.request[15], 11);
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Nla).is_ok());
        reply[15] = 8;
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Nla).is_ok());
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Auto).is_ok());
        reply[15] = 1;
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Nla).is_err());
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Auto).is_ok());
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Tls).is_ok());
        reply[3] = 255;
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Auto).is_err());
    }
}
