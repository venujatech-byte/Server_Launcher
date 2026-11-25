use super::{credentials::DesktopProtocol, profile::RdpSecurity};
use std::{
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProbeRequest {
    pub host: String,
    pub port: u16,
    pub protocol: DesktopProtocol,
    pub rdp_security: RdpSecurity,
}

pub(crate) fn run(request: &ProbeRequest, timeout: Duration) -> Result<String, String> {
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
    let info = match request.protocol {
        DesktopProtocol::Rdp => rdp(&mut stream, request.rdp_security),
        DesktopProtocol::Vnc => vnc(&mut stream),
    }
    .map_err(|error| format!("Handshake: {error}. {hint}"))?;
    Ok(format!("{info}. Authentication was not tested."))
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

fn rdp(stream: &mut (impl Read + Write), security: RdpSecurity) -> io::Result<String> {
    let offered: u8 = match security {
        RdpSecurity::Auto | RdpSecurity::Nla => 2,
        RdpSecurity::Tls => 1,
        RdpSecurity::Rdp => 0,
    };
    let mut request = [
        3, 0, 0, 19, 14, 0xe0, 0, 0, 0, 0, 0, 1, 0, 8, 0, offered, 0, 0, 0,
    ];
    let size = if security == RdpSecurity::Rdp {
        request[3] = 11;
        request[4] = 6;
        11
    } else {
        19
    };
    stream.write_all(&request[..size])?;
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
        return if security == RdpSecurity::Rdp {
            Ok("Legacy RDP endpoint confirmed".into())
        } else {
            Err(invalid(
                "the server did not negotiate encrypted RDP security",
            ))
        };
    }
    if body[9..11] != [8, 0] {
        return Err(invalid("invalid RDP negotiation length"));
    }
    let protocol = u32::from_le_bytes([body[11], body[12], body[13], body[14]]);
    match (body[7], protocol, security) {
        (2, 2, RdpSecurity::Auto | RdpSecurity::Nla) => Ok("RDP endpoint agreed to NLA".into()),
        (2, 1, RdpSecurity::Tls) => Ok("RDP endpoint agreed to TLS".into()),
        (3, 5, _) => Err(invalid("the server requires NLA")),
        (3, _, _) => Err(invalid(&format!(
            "the server rejected RDP negotiation (code {protocol})"
        ))),
        _ => Err(invalid(
            "the server selected an unexpected RDP security mode",
        )),
    }
}

fn vnc(stream: &mut (impl Read + Write)) -> io::Result<String> {
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
    let (port, rdp_security, direct) = match protocol {
        DesktopProtocol::Rdp => match host.rdp.as_ref().filter(|profile| profile.enabled) {
            Some(profile) => (
                profile.port,
                profile.security,
                !profile.tunnel_via_ssh && profile.gateway.trim().is_empty(),
            ),
            None => return ("○ Not configured", None),
        },
        DesktopProtocol::Vnc => match host.vnc.as_ref().filter(|profile| profile.enabled) {
            Some(profile) => (profile.port, RdpSecurity::Auto, !profile.tunnel_via_ssh),
            None => return ("○ Not configured", None),
        },
    };
    let request = ProbeRequest {
        host: host.host.trim().trim_matches(['[', ']']).into(),
        port,
        protocol,
        rdp_security,
    };
    if direct {
        let last = ctx.data_mut(|data| {
            data.get_temp::<(ProbeRequest, ProbeResult)>(probe_result_id(protocol))
        });
        if let Some((_, result)) = last.filter(|(tested, _)| tested == &request) {
            return match result {
                Ok(info) => ("● Reachable", Some(info)),
                Err(error) => ("◐ Unreachable", Some(error)),
            };
        }
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
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Nla).is_ok());
        reply[15] = 1;
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Nla).is_err());
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Auto).is_err());
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Tls).is_ok());
        reply[3] = 255;
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Auto).is_err());
    }
}
