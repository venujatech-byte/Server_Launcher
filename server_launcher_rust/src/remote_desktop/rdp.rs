//! The blocking IronRDP client runs inside the isolated viewer process.

use super::{
    credentials::{self, DesktopProtocol},
    framebuffer::Region,
    profile::{Clipboard, RdpProfile, RdpSecurity, Resolution},
    trust::Verifier,
    tunnel::{self, DesktopStream},
    vnc::{Connection, Input},
};
use crate::config::SshRemoteHost;
use ironrdp::{
    cliprdr::{
        pdu::{ClipboardFormat, ClipboardFormatId},
        CliprdrClient,
    },
    connector::{self, connection_activation::ConnectionActivationState, sspi, Sequence},
    core::WriteBuf,
    displaycontrol::client::DisplayControlClient,
    dvc::DrdynvcClient,
    graphics::image_processing::PixelFormat,
    pdu::{
        gcc::KeyboardType,
        rdp::{
            capability_sets::MajorPlatformType,
            client_info::{CompressionType, PerformanceFlags, TimezoneInfo},
        },
    },
    session::{fast_path, image::DecodedImage, ActiveStageBuilder, ActiveStageOutput},
};
use ironrdp_blocking::Framed;
use std::{
    io,
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use x509_cert::der::Decode;

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn start(
    host: SshRemoteHost,
    config_path: PathBuf,
    monitor_size: Option<[u32; 2]>,
    ctx: egui::Context,
) -> Arc<Mutex<Connection>> {
    let reconnect = host
        .rdp
        .as_ref()
        .is_some_and(|profile| profile.auto_reconnect);
    Connection::start_worker("Connecting to RDP…", reconnect, ctx, move |state, ctx| {
        run(&host, &config_path, monitor_size, state, ctx)
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

fn run(
    host: &SshRemoteHost,
    config_path: &Path,
    monitor_size: Option<[u32; 2]>,
    state: &Arc<Mutex<Connection>>,
    ctx: &egui::Context,
) -> io::Result<()> {
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
        connector::ClientConnector::new(config(host, profile, &password, monitor_size), local_addr);
    if profile.clipboard != Clipboard::Off {
        connector.attach_static_channel(CliprdrClient::new(Box::new(
            super::rdp_clipboard::ClipboardBackend::new(matches!(
                profile.clipboard,
                Clipboard::Both | Clipboard::RemoteToLocal
            )),
        )));
    }
    let max_monitor_area = Arc::new(AtomicU64::new(0));
    let server_limit = max_monitor_area.clone();
    let resize_state = state.clone();
    let resize_ctx = ctx.clone();
    connector.attach_static_channel(DrdynvcClient::new().with_dynamic_channel(
        DisplayControlClient::new(move |capabilities| {
            let limit = capabilities.max_monitor_area();
            server_limit.store(limit, Ordering::Release);
            resize_state.lock().unwrap().rdp_resize_supported = limit != 0;
            resize_ctx.request_repaint();
            Ok(Vec::new())
        }),
    ));
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

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(Verifier::new(
        config_path,
        host,
        provider.clone(),
        state.clone(),
        ctx.clone(),
    ));
    let mut tls_config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| error("Could not configure RDP TLS."))?
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();
    tls_config.resumption = rustls::client::Resumption::disabled();

    let tls_client = rustls::ClientConnection::new(Arc::new(tls_config), server_name)
        .map_err(|_| error("Could not start RDP TLS."))?;
    let mut tls_stream = rustls::StreamOwned::new(tls_client, socket);
    while tls_stream.conn.is_handshaking() {
        tls_stream
            .conn
            .complete_io(&mut tls_stream.sock)
            .map_err(|_| error(verifier.failure()))?;
    }
    verifier.finish()?;
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

    let mut width = result.desktop_size.width;
    let mut height = result.desktop_size.height;
    let activation_factory = result.activation_factory;
    let io_channel_id = result.io_channel_id;
    let user_channel_id = result.user_channel_id;
    let (input, overflow) = {
        let mut shared = state.lock().unwrap();
        shared
            .framebuffer
            .resize([width.into(), height.into()])
            .map_err(error)?;
        shared.connected = true;
        shared.connected_since = Some(Instant::now());
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
        .set_read_timeout(INPUT_POLL_INTERVAL)?;
    let mut encoder = super::rdp_input::Encoder::default();
    let mut events = Vec::with_capacity(64);
    let mut proposed_resize = None;
    let received_bytes = state.lock().unwrap().received_bytes.clone();
    loop {
        if overflow.load(Ordering::Acquire) {
            return Err(error(
                "The RDP input queue is full. The session was closed to release held inputs.",
            ));
        }
        let (resize_enabled, requested_size) = {
            let mut state = state.lock().unwrap();
            (state.rdp_resize_enabled, state.resize_request.take())
        };
        if resize_enabled {
            if let Some(size) = requested_size {
                proposed_resize = Some((size, Instant::now()));
            }
            if let Some(([desired_width, desired_height], changed)) = proposed_resize {
                let max = max_monitor_area.load(Ordering::Acquire);
                let current = [u32::from(width), u32::from(height)];
                if [desired_width, desired_height] == current {
                    proposed_resize = None;
                } else if max != 0 && changed.elapsed() >= Duration::from_millis(180) {
                    let size = fit_server_limit(desired_width, desired_height, max);
                    if let Some([new_width, new_height]) = size {
                        if let Some(packet) = stage.encode_resize(new_width, new_height, None, None)
                        {
                            let bytes = packet
                                .map_err(|_| error("Could not encode RDP desktop resize."))?;
                            framed.write_all(&bytes)?;
                            proposed_resize = None;
                        }
                    } else {
                        proposed_resize = None;
                    }
                }
            }
        } else {
            proposed_resize = None;
        }
        events.clear();
        for event in input.try_iter().take(32) {
            if let Input::ClipboardText(text) = event {
                if matches!(
                    profile.clipboard,
                    Clipboard::Both | Clipboard::LocalToRemote
                ) && !text.contains('\0')
                    && text.encode_utf16().count() < super::vnc::CLIPBOARD_LIMIT / 2
                {
                    if let Some(channel) = stage.get_svc_processor_mut::<CliprdrClient>() {
                        if let Some(clipboard) =
                            channel.downcast_backend_mut::<super::rdp_clipboard::ClipboardBackend>()
                        {
                            clipboard.local_text = Some(text);
                            if let Ok(offer) = channel.initiate_copy(&[ClipboardFormat::new(
                                ClipboardFormatId::CF_UNICODETEXT,
                            )]) {
                                if let Ok(packet) = stage.process_svc_processor_messages(offer) {
                                    framed.write_all(&packet)?;
                                }
                            }
                        }
                    }
                }
            } else {
                encoder.encode(event, &mut events);
            }
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
        if let Some(channel) = stage.get_svc_processor_mut::<CliprdrClient>() {
            if let Some(clipboard) =
                channel.downcast_backend_mut::<super::rdp_clipboard::ClipboardBackend>()
            {
                if let Some(text) = clipboard.received.take() {
                    state.lock().unwrap().clipboard_text = Some(text);
                    ctx.request_repaint();
                }
                let reply = clipboard.reply.take();
                let sent_text = reply.as_ref().is_some_and(|reply| !reply.is_error());
                let request_text = clipboard.request_text();
                let response = reply.and_then(|reply| channel.submit_format_data(reply).ok());
                let request = request_text
                    .then(|| {
                        channel
                            .initiate_paste(ClipboardFormatId::CF_UNICODETEXT)
                            .ok()
                    })
                    .flatten();
                if let Some(packet) =
                    response.and_then(|message| stage.process_svc_processor_messages(message).ok())
                {
                    framed.write_all(&packet)?;
                    if sent_text {
                        state
                            .lock()
                            .unwrap()
                            .clipboard_sent
                            .store(true, Ordering::Relaxed);
                        ctx.request_repaint();
                    }
                }
                if request_text {
                    if let Some(packet) = request
                        .and_then(|message| stage.process_svc_processor_messages(message).ok())
                    {
                        framed.write_all(&packet)?;
                    } else if let Some(clipboard) = stage
                        .get_svc_processor_mut::<CliprdrClient>()
                        .and_then(|channel| {
                            channel.downcast_backend_mut::<super::rdp_clipboard::ClipboardBackend>()
                        })
                    {
                        clipboard.cancel_request();
                    }
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
        received_bytes.fetch_add(payload.len() as u64, Ordering::Relaxed);
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
                    let stride = image.stride();
                    let offset = y * stride + x * 4;
                    state
                        .lock()
                        .unwrap()
                        .framebuffer
                        .write_strided_rgba(region, &image.data()[offset..], stride)
                        .map_err(error)?;
                    ctx.request_repaint();
                }
                ActiveStageOutput::Terminate(_) => {
                    return Err(error("The RDP server ended the session."))
                }
                ActiveStageOutput::DeactivateAll => {
                    let mut activation = activation_factory.create();
                    framed
                        .get_inner()
                        .0
                        .sock
                        .set_read_timeout(CONNECT_TIMEOUT)?;
                    let mut output = WriteBuf::new();
                    let deadline = Instant::now() + CONNECT_TIMEOUT;
                    loop {
                        if let ConnectionActivationState::Finalized {
                            desktop_size,
                            share_id,
                            enable_server_pointer,
                            pointer_software_rendering,
                        } = activation.connection_activation_state()
                        {
                            width = desktop_size.width;
                            height = desktop_size.height;
                            state
                                .lock()
                                .unwrap()
                                .framebuffer
                                .resize([width.into(), height.into()])
                                .map_err(error)?;
                            image = DecodedImage::new(PixelFormat::RgbA32, width, height);
                            stage.set_share_id(share_id);
                            stage.set_enable_server_pointer(enable_server_pointer);
                            stage.set_fastpath_processor(
                                fast_path::ProcessorBuilder {
                                    io_channel_id,
                                    user_channel_id,
                                    share_id,
                                    enable_server_pointer,
                                    pointer_software_rendering,
                                    bulk_decompressor: None,
                                }
                                .build(),
                            );
                            break;
                        }
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            return Err(error("RDP session reactivation timed out."));
                        }
                        framed.get_inner().0.sock.set_read_timeout(remaining)?;
                        output.clear();
                        let written = if let Some(hint) = activation.next_pdu_hint() {
                            let pdu = framed.read_by_hint(hint)?;
                            activation.step(&pdu, &mut output)
                        } else {
                            activation.step_no_input(&mut output)
                        }
                        .map_err(|_| error("RDP session reactivation failed."))?;
                        if let Some(length) = written.size() {
                            framed.write_all(&output[..length])?;
                        }
                    }
                    framed
                        .get_inner()
                        .0
                        .sock
                        .set_read_timeout(INPUT_POLL_INTERVAL)?;
                    ctx.request_repaint();
                }
                _ => {}
            }
        }
    }
}

pub(crate) fn display_dimensions(width: u32, height: u32) -> [u32; 2] {
    [width.clamp(200, 4096) & !1, height.clamp(200, 4096)]
}

fn fit_server_limit(width: u32, height: u32, area: u64) -> Option<[u32; 2]> {
    if area < 40_000 {
        return None;
    }
    if u64::from(width) * u64::from(height) <= area {
        return Some([width, height]);
    }
    let scale = (area as f64 / (f64::from(width) * f64::from(height))).sqrt();
    let [width, height] = display_dimensions(
        (f64::from(width) * scale) as u32,
        (f64::from(height) * scale) as u32,
    );
    let height = height.min((area / u64::from(width)) as u32);
    if height < 200 {
        Some([width.min((area / 200) as u32 & !1), 200])
    } else {
        Some([width, height])
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
    socket.set_nodelay(true)?;
    socket.set_read_timeout(Some(CONNECT_TIMEOUT))?;
    socket.set_write_timeout(Some(CONNECT_TIMEOUT))?;
    Ok(socket)
}

fn initial_dimensions(profile: &RdpProfile, monitor_size: Option<[u32; 2]>) -> [u16; 2] {
    match profile.resolution {
        Resolution::Fixed => [profile.width.clamp(1, 4096), profile.height.clamp(1, 4096)],
        Resolution::MatchLocalScreen => monitor_size.unwrap_or([1280, 720]).map(|size| size as u16),
        Resolution::FitWindow => [1280, 720],
    }
}

fn config(
    host: &SshRemoteHost,
    profile: &RdpProfile,
    password: &str,
    monitor_size: Option<[u32; 2]>,
) -> connector::Config {
    let username = if profile.same_username_as_ssh {
        &host.user
    } else {
        &profile.username
    };
    let tls_only = profile.security == RdpSecurity::Tls;
    let [width, height] = initial_dimensions(profile, monitor_size);
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
        bitmap: ironrdp::pdu::rdp::capability_sets::client_codecs_capabilities(&[])
            .ok()
            .map(|codecs| connector::BitmapConfig {
                color_depth: match profile.color_depth {
                    16 | 24 => u32::from(profile.color_depth),
                    _ => 32,
                },
                lossy_compression: false,
                codecs,
            }),
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
        compression_type: Some(CompressionType::Rdp61),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_monitor_dimensions_and_handles_missing_monitor_info() {
        let profile = RdpProfile {
            resolution: Resolution::MatchLocalScreen,
            ..Default::default()
        };
        assert_eq!(
            initial_dimensions(&profile, Some([2560, 1440])),
            [2560, 1440]
        );
        assert_eq!(initial_dimensions(&profile, None), [1280, 720]);
    }

    #[test]
    fn normalizes_remote_resize_and_respects_server_area() {
        assert_eq!(display_dimensions(1001, 9000), [1000, 4096]);
        assert_eq!(display_dimensions(1, 1), [200, 200]);
        assert_eq!(fit_server_limit(1024, 768, 1_000_000), Some([1024, 768]));
        assert_eq!(fit_server_limit(200, 4096, 40_000), Some([200, 200]));
        assert!(fit_server_limit(200, 200, 39_999).is_none());
        let [width, height] = fit_server_limit(4000, 3000, 510_000).unwrap();
        assert!(width % 2 == 0 && width * height <= 510_000);
    }
}
