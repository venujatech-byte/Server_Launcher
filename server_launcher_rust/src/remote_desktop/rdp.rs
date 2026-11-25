//! The blocking IronRDP client runs inside the isolated viewer process.

use super::{
    credentials::{self, DesktopProtocol},
    framebuffer::Region,
    profile::{RdpProfile, RdpSecurity, Resolution},
    tunnel::{self, DesktopStream},
    vnc::Connection,
};
use crate::config::SshRemoteHost;
use ironrdp::{
    connector::{self, sspi},
    graphics::image_processing::PixelFormat,
    pdu::{
        gcc::KeyboardType,
        rdp::{
            capability_sets::MajorPlatformType,
            client_info::{PerformanceFlags, TimezoneInfo},
        },
    },
    session::{image::DecodedImage, ActiveStageBuilder, ActiveStageOutput},
};
use ironrdp_blocking::Framed;
use std::{
    io,
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    sync::{atomic::Ordering, Arc, Mutex},
    time::{Duration, Instant},
};
use x509_cert::der::Decode;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn start(host: SshRemoteHost, ctx: egui::Context) -> Arc<Mutex<Connection>> {
    Connection::start_worker("Connecting to RDP…", ctx, move |state, ctx| {
        run(&host, state, ctx)
    })
}

fn error(message: &str) -> io::Error {
    io::Error::other(message)
}

struct NoKdc;

impl sspi::network_client::NetworkClient for NoKdc {
    fn send(&self, _: &sspi::generator::NetworkRequest) -> sspi::Result<Vec<u8>> {
        Err(sspi::Error::new(
            sspi::ErrorKind::UnsupportedFunction,
            "This viewer does not contact a Kerberos KDC.",
        ))
    }
}

fn run(host: &SshRemoteHost, state: &Mutex<Connection>, ctx: &egui::Context) -> io::Result<()> {
    let profile = host
        .rdp
        .as_ref()
        .filter(|p| p.enabled)
        .ok_or_else(|| error("RDP is disabled for this host."))?;
    if !profile.gateway.trim().is_empty() {
        return Err(error(
            "Microsoft RD Gateway is not supported yet. No connection was attempted.",
        ));
    }
    if profile.security == RdpSecurity::Rdp {
        return Err(error(
            "Legacy unencrypted RDP security is unsupported. Select NLA or TLS.",
        ));
    }
    let password = credentials::load(host, DesktopProtocol::Rdp)
        .map_err(|message| error(&message))?
        .ok_or_else(|| error("Save an RDP password in the host settings before connecting."))?;
    let hostname = host.host.trim().trim_matches(['[', ']']);
    let server_name = rustls::pki_types::ServerName::try_from(hostname.to_owned())
        .map_err(|_| error("The saved RDP host is not a valid TLS name."))?;

    let (socket, local_addr): (Box<dyn DesktopStream>, SocketAddr) = if profile.tunnel_via_ssh {
        (
            tunnel::open(host, profile.port, CONNECT_TIMEOUT)?,
            SocketAddr::from(([127, 0, 0, 1], 0)),
        )
    } else {
        let socket = open_direct(hostname, profile.port)?;
        let local_addr = socket.local_addr()?;
        (Box::new(socket), local_addr)
    };
    let mut framed = Framed::new(socket);
    let mut connector =
        connector::ClientConnector::new(config(host, profile, &password), local_addr);
    drop(password);
    let upgrade = ironrdp_blocking::connect_begin(&mut framed, &mut connector).map_err(|_| {
        error("RDP security negotiation failed. Check the server's NLA and TLS settings.")
    })?;
    let (socket, unprocessed) = framed.into_inner();
    if !unprocessed.is_empty() {
        return Err(error(
            "The RDP server sent unexpected data before the TLS handshake.",
        ));
    }

    let mut roots = rustls::RootCertStore::empty();
    roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
    if roots.is_empty() {
        return Err(error("Could not load system TLS certificate authorities."));
    }
    let mut tls_config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| error("Could not configure RDP TLS."))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    tls_config.resumption = rustls::client::Resumption::disabled();

    let tls_client = rustls::ClientConnection::new(Arc::new(tls_config), server_name)
        .map_err(|_| error("Could not start RDP TLS."))?;
    let mut tls_stream = rustls::StreamOwned::new(tls_client, socket);
    while tls_stream.conn.is_handshaking() {
        tls_stream.conn.complete_io(&mut tls_stream.sock)
            .map_err(|_| error("RDP TLS verification failed. Check the server name and install its CA in the system trust store."))?;
    }
    let cert = tls_stream
        .conn
        .peer_certificates()
        .and_then(|chain| chain.first())
        .ok_or_else(|| error("The RDP host did not send a certificate."))?;
    let cert = x509_cert::Certificate::from_der(cert.as_ref())
        .map_err(|_| error("The RDP host sent an invalid certificate."))?;
    let binding = cert
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .as_bytes()
        .ok_or_else(|| error("The RDP server key could not be read."))?
        .to_owned();
    let upgrade = ironrdp_blocking::mark_as_upgraded(upgrade, &mut connector);
    let mut framed = Framed::new(tls_stream);
    let result = ironrdp_blocking::connect_finalize(
        upgrade, connector, &mut framed, &mut NoKdc,
        hostname.to_owned().into(), binding, None,
    ).map_err(|_| error("RDP login failed. Check the username, domain, password, and Remote Desktop permissions."))?;

    let width = result.desktop_size.width;
    let height = result.desktop_size.height;
    let (input, overflow) = {
        let mut shared = state.lock().unwrap();
        shared
            .framebuffer
            .resize([width.into(), height.into()])
            .map_err(error)?;
        shared.connected = true;
        shared.status = if profile.tunnel_via_ssh {
            "Connected · TLS verified · SSH tunnel"
        } else {
            "Connected · TLS verified"
        }
        .into();
        shared.open_input()
    };
    ctx.request_repaint();
    let mut image = DecodedImage::new(PixelFormat::RgbA32, width, height);
    let mut stage = ActiveStageBuilder {
        static_channels: result.static_channels,
        user_channel_id: result.user_channel_id,
        io_channel_id: result.io_channel_id,
        message_channel_id: result.message_channel_id,
        share_id: result.share_id,
        compression_type: result.compression_type,
        enable_server_pointer: result.enable_server_pointer,
        pointer_software_rendering: result.pointer_software_rendering,
    }
    .build();

    framed
        .get_inner()
        .0
        .sock
        .set_read_timeout(Duration::from_millis(30))?;
    let mut encoder = super::rdp_input::Encoder::default();
    let mut events = Vec::with_capacity(64);
    let mut rgba = Vec::new();
    loop {
        if overflow.load(Ordering::Acquire) {
            return Err(error(
                "The RDP input queue is full. The session was closed to release held inputs.",
            ));
        }
        events.clear();
        for event in input.try_iter().take(32) {
            encoder.encode(event, &mut events);
        }
        if !events.is_empty() {
            let frames = stage
                .process_fastpath_input(&mut image, &events)
                .map_err(|_| error("Could not encode RDP input."))?;
            for frame in frames {
                if let ActiveStageOutput::ResponseFrame(bytes) = frame {
                    framed.write_all(&bytes)?;
                }
            }
        }
        let (action, payload) = match framed.read_pdu() {
            Ok(frame) => frame,
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(_) => return Err(error("The RDP connection closed.")),
        };
        let outputs = stage
            .process(&mut image, action, &payload)
            .map_err(|_| error("The RDP server sent an unsupported or invalid update."))?;
        for output in outputs {
            match output {
                ActiveStageOutput::ResponseFrame(frame) => framed.write_all(&frame)?,
                ActiveStageOutput::GraphicsUpdate(rect) => {
                    let [x, y, right, bottom] =
                        [rect.left, rect.top, rect.right, rect.bottom].map(usize::from);
                    if right < x
                        || bottom < y
                        || right >= usize::from(width)
                        || bottom >= usize::from(height)
                    {
                        return Err(error("The RDP server reported invalid framebuffer damage."));
                    }
                    let region = Region {
                        origin: [x, y],
                        size: [right - x + 1, bottom - y + 1],
                    };
                    rgba.clear();
                    for row in y..=bottom {
                        let start = row * image.stride() + x * 4;
                        rgba.extend_from_slice(&image.data()[start..start + region.size[0] * 4]);
                    }
                    state
                        .lock()
                        .unwrap()
                        .framebuffer
                        .write_rgba(region, &rgba)
                        .map_err(error)?;
                    ctx.request_repaint();
                }
                ActiveStageOutput::Terminate(_) => {
                    return Err(error("The RDP server ended the session."))
                }
                ActiveStageOutput::DeactivateAll => return Err(error(
                    "The RDP server requested session reactivation, which is not supported yet.",
                )),
                _ => {}
            }
        }
    }
}

fn open_direct(hostname: &str, port: u16) -> io::Result<TcpStream> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let addresses = (hostname, port)
        .to_socket_addrs()
        .map_err(|_| error("Could not resolve the RDP host."))?;
    let socket = addresses.take(16).find_map(|address| {
        let remaining = deadline.saturating_duration_since(Instant::now());
        (!remaining.is_zero()).then(|| TcpStream::connect_timeout(&address, remaining.min(Duration::from_secs(3))).ok()).flatten()
    }).ok_or_else(|| error("Could not connect to the RDP port. Check that Remote Desktop is enabled on the host."))?;
    socket.set_read_timeout(Some(CONNECT_TIMEOUT))?;
    socket.set_write_timeout(Some(CONNECT_TIMEOUT))?;
    Ok(socket)
}

fn config(host: &SshRemoteHost, profile: &RdpProfile, password: &str) -> connector::Config {
    let username = if profile.same_username_as_ssh {
        &host.user
    } else {
        &profile.username
    };
    let tls_only = profile.security == RdpSecurity::Tls;
    let (width, height) = if profile.resolution == Resolution::Fixed {
        (profile.width.clamp(1, 4096), profile.height.clamp(1, 4096))
    } else {
        (1280, 720)
    };
    let mut performance = PerformanceFlags::default();
    performance.set(PerformanceFlags::DISABLE_WALLPAPER, !profile.wallpaper);
    performance.set(
        PerformanceFlags::ENABLE_FONT_SMOOTHING,
        profile.font_smoothing,
    );
    performance.set(
        PerformanceFlags::DISABLE_MENUANIMATIONS,
        !profile.animations,
    );
    performance.set(PerformanceFlags::DISABLE_THEMING, !profile.themes);
    connector::Config {
        credentials: connector::Credentials::UsernamePassword {
            username: username.clone(),
            password: password.to_owned(),
        },
        domain: (!profile.domain.trim().is_empty()).then(|| profile.domain.clone()),
        enable_tls: tls_only,
        enable_credssp: !tls_only,
        keyboard_type: KeyboardType::IbmEnhanced,
        keyboard_subtype: 0,
        keyboard_layout: profile.keyboard_layout.unwrap_or(0),
        keyboard_functional_keys_count: 12,
        ime_file_name: String::new(),
        dig_product_id: String::new(),
        desktop_size: connector::DesktopSize { width, height },
        bitmap: None,
        client_build: 0,
        client_name: "ServerLauncher".into(),
        client_dir: String::new(),
        platform: if cfg!(windows) {
            MajorPlatformType::WINDOWS
        } else {
            MajorPlatformType::UNIX
        },
        enable_server_pointer: false,
        request_data: None,
        autologon: tls_only,
        enable_audio_playback: false,
        compression_type: None,
        pointer_software_rendering: false,
        multitransport_flags: None,
        performance_flags: performance,
        desktop_scale_factor: 0,
        hardware_id: None,
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
        alternate_shell: String::new(),
        work_dir: String::new(),
    }
}
