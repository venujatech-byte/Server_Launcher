//! The blocking IronRDP client runs inside the isolated viewer process.

use super::{
    credentials::{self, DesktopProtocol},
    framebuffer::Region,
    profile::{Audio, Clipboard, Quality, RdpColorProfile, RdpProfile, RdpSecurity, Resolution},
    trust::Verifier,
    tunnel::{self, DesktopStream, InputSocket},
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
    displaycontrol::{
        client::DisplayControlClient,
        pdu::{DisplayControlMonitorLayout, DisplayControlPdu, MonitorLayoutEntry},
    },
    dvc::{DrdynvcClient, DvcChannelListener, DvcClientProcessor, DynamicChannelId},
    graphics::image_processing::PixelFormat,
    pdu::{
        gcc::KeyboardType,
        rdp::{
            capability_sets::MajorPlatformType,
            client_info::{CompressionType, PerformanceFlags, TimezoneInfo},
        },
    },
    rdpsnd::{
        client::{
            Clock, RdpsndClientHandler, RdpsndDvcState, DVC_CHANNEL_NAME, LOSSY_DVC_CHANNEL_NAME,
        },
        pdu::{AudioFormat, AudioFormatFlags, PitchPdu, QualityMode, VolumePdu},
    },
    session::{fast_path, image::DecodedImage, ActiveStageBuilder, ActiveStageOutput},
};
use ironrdp_blocking::Framed;
use ironrdp_egfx::client::{GraphicsPipelineClient, GraphicsPipelineHandler};
use ironrdp_egfx::decode::H264Decoder;
use std::{
    borrow::Cow,
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex, PoisonError,
    },
    time::{Duration, Instant},
};
use x509_cert::der::Decode;
use zeroize::Zeroizing;

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const AUDIO_INPUT_QUEUE: usize = 64;

#[derive(Debug, Clone)]
struct SharedPlayback {
    backend: Arc<Mutex<ironrdp_rdpsnd_native::cpal::RdpsndBackend>>,
    formats: Arc<[AudioFormat]>,
}

impl SharedPlayback {
    fn new() -> Self {
        let backend = ironrdp_rdpsnd_native::cpal::RdpsndBackend::new();
        let formats = backend.get_formats().to_vec().into();
        Self {
            backend: Arc::new(Mutex::new(backend)),
            formats,
        }
    }

    fn with_backend<T>(
        &self,
        f: impl FnOnce(&mut ironrdp_rdpsnd_native::cpal::RdpsndBackend) -> T,
    ) -> T {
        f(&mut self.backend.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl RdpsndClientHandler for SharedPlayback {
    fn get_flags(&self) -> AudioFormatFlags {
        self.with_backend(|backend| backend.get_flags())
    }

    fn get_formats(&self) -> &[AudioFormat] {
        &self.formats
    }

    fn choose_formats(&self, formats: &[AudioFormat]) -> Vec<AudioFormat> {
        self.with_backend(|backend| backend.choose_formats(formats))
    }

    fn wave(&mut self, format: &AudioFormat, timestamp: u32, data: Cow<'_, [u8]>) {
        self.with_backend(|backend| backend.wave(format, timestamp, data));
    }

    fn playback_latency(&self) -> Duration {
        self.with_backend(|backend| backend.playback_latency())
    }

    fn set_volume(&mut self, volume: VolumePdu) {
        self.with_backend(|backend| backend.set_volume(volume));
    }

    fn set_pitch(&mut self, pitch: PitchPdu) {
        self.with_backend(|backend| backend.set_pitch(pitch));
    }

    fn close(&mut self) {
        self.with_backend(ironrdp_rdpsnd_native::cpal::RdpsndBackend::close);
    }
}

fn audio_clock() -> Clock {
    let origin = Instant::now();
    Box::new(move || origin.elapsed())
}

struct AudioInputListener {
    sender: SyncSender<(DynamicChannelId, Vec<ironrdp::svc::SvcMessage>)>,
}

impl DvcChannelListener for AudioInputListener {
    fn channel_name(&self) -> &str {
        ironrdp_rdpeai::CHANNEL_NAME
    }

    fn create(&mut self, _: DynamicChannelId) -> Option<Box<dyn DvcClientProcessor>> {
        let sender = self.sender.clone();
        Some(Box::new(ironrdp_rdpeai::client::RdpeaiClient::new(
            Box::new(ironrdp_rdpsnd_native::capture::RdpeaiCaptureBackend::new()),
            Box::new(move |channel_id, messages| {
                sender.try_send((channel_id, messages)).map_err(|_| {
                    ironrdp::pdu::pdu_other_err!("RDP microphone send queue is full")
                })?;
                Ok(())
            }),
        )))
    }
}

pub(crate) fn start(
    host: SshRemoteHost,
    config_path: PathBuf,
    monitor_size: Option<[u32; 2]>,
    fit_window_size: Option<[u32; 2]>,
    session_password: Option<Zeroizing<String>>,
    ctx: egui::Context,
) -> Arc<Mutex<Connection>> {
    let reconnect = host
        .rdp
        .as_ref()
        .is_some_and(|profile| profile.auto_reconnect);
    let session_password = Arc::new(Mutex::new(session_password));
    Connection::start_worker("Connecting to RDP…", reconnect, ctx, move |state, ctx| {
        let password = session_password.lock().unwrap().clone();
        run(
            &host,
            &config_path,
            monitor_size,
            fit_window_size,
            password,
            state,
            ctx,
        )
    })
}

fn error(message: &str) -> io::Error {
    io::Error::other(message)
}

fn authentication_error(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
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
    fit_window_size: Option<[u32; 2]>,
    session_password: Option<Zeroizing<String>>,
    state: &Arc<Mutex<Connection>>,
    ctx: &egui::Context,
) -> io::Result<()> {
    let profile = host
        .rdp
        .as_ref()
        .filter(|p| p.enabled)
        .ok_or_else(|| error("RDP is disabled for this host."))?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let use_egfx = bitmap_quality(profile).0 != 8;
    let desktop_settings = crate::config::ConfigFile::load_from_file(config_path).remote_desktop;
    let max_clipboard_image_bytes = usize::from(desktop_settings.clipboard_image_limit_mb.clamp(1, 64))
        * 1024
        * 1024;
    let h264_decoder = if use_egfx {
        desktop_settings
            .h264_library_path
            .map(|path| {
                ironrdp_egfx::decode::OpenH264Decoder::from_library_path(&path)
                    .map(|decoder| Box::new(decoder) as Box<dyn H264Decoder>)
                    .map_err(|_| {
                        error(
                            "The selected OpenH264 library is unsupported or could not be loaded.",
                        )
                    })
            })
            .transpose()?
    } else {
        None
    };
    let avc_enabled = h264_decoder.is_some();
    let password = match session_password {
        Some(password) => password,
        None => credentials::load(host, DesktopProtocol::Rdp)
            .map_err(|message| authentication_error(&message))?
            .ok_or_else(|| {
                authentication_error("No saved RDP password is available for this host.")
            })?,
    };
    let hostname = host.host.trim().trim_matches(['[', ']']);
    let server_name = rustls::pki_types::ServerName::try_from(hostname.to_owned())
        .map_err(|_| error("The saved RDP host is not a valid TLS name."))?;

    let (socket, local_addr): (Box<dyn DesktopStream>, SocketAddr) =
        if !profile.gateway.trim().is_empty() {
            if profile.tunnel_via_ssh {
                return Err(error(
                    "RD Gateway and Tunnel via SSH cannot be used together.",
                ));
            }
            open_gateway(
                &profile.gateway,
                hostname,
                profile.port,
                if profile.same_username_as_ssh {
                    &host.user
                } else {
                    &profile.username
                },
                &profile.domain,
                &password,
            )?
        } else if profile.tunnel_via_ssh {
            (
                tunnel::open(host, profile.port, CONNECT_TIMEOUT, Some(config_path))?,
                SocketAddr::from(([127, 0, 0, 1], 0)),
            )
        } else {
            let socket = open_direct(hostname, profile.port)?;
            let local_addr = socket.local_addr()?;
            (Box::new(socket), local_addr)
        };
    let mut framed = Framed::new(socket);
    let mut connector =
        connector::ClientConnector::new(
            config(host, profile, &password, monitor_size, fit_window_size),
            local_addr,
        );
    if let Some(cluster_data) = administrative_cluster_data(profile.admin_session) {
        connector = connector.with_cluster_data(cluster_data);
    }
    let drives_attached = super::rdp_drives::attach(&mut connector, profile)?;
    if profile.clipboard != Clipboard::Off {
        connector.attach_static_channel(CliprdrClient::new(Box::new(
            super::rdp_clipboard::ClipboardBackend::new(
                matches!(profile.clipboard, Clipboard::Both | Clipboard::RemoteToLocal),
                matches!(profile.clipboard, Clipboard::Both | Clipboard::LocalToRemote),
                max_clipboard_image_bytes,
            ),
        )));
    }
    let playback = if profile.audio == Audio::Local {
        let playback = SharedPlayback::new();
        connector.attach_static_channel(ironrdp::rdpsnd::client::Rdpsnd::new(Box::new(
            playback.clone(),
        )));
        if !drives_attached {
            connector.attach_static_channel(ironrdp::rdpdr::Rdpdr::new(
                Box::new(ironrdp::rdpdr::NoopRdpdrBackend),
                "ServerLauncher".into(),
            ));
        }
        Some(playback)
    } else {
        None
    };
    let max_monitor_area = Arc::new(AtomicU64::new(0));
    let server_limit = max_monitor_area.clone();
    let resize_state = state.clone();
    let resize_ctx = ctx.clone();
    let multi_monitor = profile.multi_monitor;
    let [desktop_width, desktop_height] =
        initial_dimensions(profile, monitor_size, fit_window_size).map(u32::from);
    let mut drdynvc = DrdynvcClient::new();
    if let Some(playback) = playback {
        let state = RdpsndDvcState::new(Box::new(playback))
            .with_quality_mode(QualityMode::Dynamic)
            .with_clock(audio_clock());
        let lossy = state.clone();
        drdynvc = drdynvc
            .with_dynamic_channel_factory(DVC_CHANNEL_NAME, move || state.channel())
            .with_dynamic_channel_factory(LOSSY_DVC_CHANNEL_NAME, move || lossy.lossy_channel());
    }
    let (audio_input_tx, audio_input_rx) = mpsc::sync_channel(AUDIO_INPUT_QUEUE);
    if profile.mic {
        drdynvc = drdynvc.with_listener(AudioInputListener {
            sender: audio_input_tx,
        });
    }
    if use_egfx {
        drdynvc = drdynvc.with_dynamic_channel(GraphicsPipelineClient::new(
            Box::new(EgfxHandler {
                color_profile: profile.color_profile.unwrap_or_default(),
                avc_enabled,
            }),
            h264_decoder,
        ));
    }
    drdynvc = drdynvc.with_dynamic_channel(DisplayControlClient::new(move |capabilities| {
        let limit = capabilities.max_monitor_area();
        server_limit.store(limit, Ordering::Release);
        resize_state.lock().unwrap().rdp_resize_supported = limit != 0;
        resize_ctx.request_repaint();
        let layout = if multi_monitor {
            dual_monitor_layout(desktop_width, desktop_height, limit)
        } else {
            None
        };
        if multi_monitor && layout.is_none() {
            resize_state.lock().unwrap().status =
                "The RDP server did not report enough display area for two monitors.".into();
        }
        Ok(layout
            .map(|layout| {
                let pdu: DisplayControlPdu = layout.into();
                vec![Box::new(pdu) as ironrdp::dvc::DvcMessage]
            })
            .unwrap_or_default())
    }));
    connector.attach_static_channel(drdynvc);
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
    ).map_err(|_| authentication_error("RDP login failed. Check the username, domain, password, and Remote Desktop permissions."))?
        .into_connected()
        .map_err(|_| error("The RDP server requested a session redirect that this client does not support."))?;

    let mut width = result.desktop_size.width;
    let mut height = result.desktop_size.height;
    let activation_factory = result.activation_factory;
    let io_channel_id = result.io_channel_id;
    let user_channel_id = result.user_channel_id;
    let compression_type = result.compression_type;
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
        compression_type,
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
    let mut dirty_regions = Vec::with_capacity(8);
    let mut converted_damage = Vec::new();
    let mut proposed_resize = None;
    let received_bytes = state.lock().unwrap().received_bytes.clone();
    loop {
        for (channel_id, messages) in audio_input_rx.try_iter().take(8) {
            let batch = ironrdp::dvc::DvcMessageBatch::try_new(channel_id, messages)
                .map_err(|_| error("Could not validate RDP microphone data."))?;
            let packet = stage
                .encode_dvc_messages(batch.into_messages())
                .map_err(|_| error("Could not encode RDP microphone data."))?;
            framed.write_all(&packet)?;
        }
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
            if let Input::DownloadClipboardFiles(directory) = &event {
                if matches!(profile.clipboard, Clipboard::Both | Clipboard::RemoteToLocal) {
                    if let Some(clipboard) = stage
                        .get_svc_processor_mut::<CliprdrClient>()
                        .and_then(|channel| {
                            channel.downcast_backend_mut::<
                                super::rdp_clipboard::ClipboardBackend,
                            >()
                        })
                    {
                        clipboard.start_remote_download(directory.clone());
                    }
                }
            } else if let Input::ClipboardFiles(paths) = &event {
                if matches!(profile.clipboard, Clipboard::Both | Clipboard::LocalToRemote) {
                    let (shared_paths, descriptors) =
                        super::rdp_clipboard::shared_file_descriptors(paths);
                    if !descriptors.is_empty() {
                        if let Some(channel) = stage.get_svc_processor_mut::<CliprdrClient>() {
                            if let Some(clipboard) = channel.downcast_backend_mut::<
                                super::rdp_clipboard::ClipboardBackend,
                            >() {
                                clipboard.local_files = shared_paths;
                                if let Ok(offer) = channel.initiate_file_copy(descriptors) {
                                    if let Ok(packet) = stage.process_svc_processor_messages(offer)
                                    {
                                        framed.write_all(&packet)?;
                                    }
                                }
                            }
                        }
                    }
                }
            } else if let Input::ClipboardText(text) = &event {
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
                            clipboard.local_text = Some(text.clone());
                            clipboard.local_image = None;
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
            } else if let Input::ClipboardImage(image) = &event {
                if matches!(
                    profile.clipboard,
                    Clipboard::Both | Clipboard::LocalToRemote
                ) {
                    if let Ok(image) = super::clipboard_image::encode_dibv5_with_limit(
                        image,
                        max_clipboard_image_bytes,
                    ) {
                        if let Some(channel) = stage.get_svc_processor_mut::<CliprdrClient>() {
                            if let Some(clipboard) = channel
                                .downcast_backend_mut::<super::rdp_clipboard::ClipboardBackend>()
                            {
                                clipboard.local_text = None;
                                clipboard.local_image = Some(image);
                                if let Ok(offer) = channel.initiate_copy(&[
                                    ClipboardFormat::new(ClipboardFormatId::CF_DIBV5),
                                    ClipboardFormat::new(ClipboardFormatId::CF_DIB),
                                ]) {
                                    if let Ok(packet) = stage.process_svc_processor_messages(offer)
                                    {
                                        framed.write_all(&packet)?;
                                    }
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
        let clipboard_messages = stage
            .get_svc_processor_mut::<CliprdrClient>()
            .and_then(|channel| {
                channel.downcast_backend_mut::<super::rdp_clipboard::ClipboardBackend>()
            })
            .map(|clipboard| {
                let received = clipboard.received.take();
                let received_image = clipboard.received_image.take();
                let reply = clipboard.reply.take();
                let file_response = clipboard.take_file_response();
                let remote_file_request = clipboard.take_remote_file_request();
                let request_format = clipboard.request_format();
                (
                    received,
                    received_image,
                    reply,
                    file_response,
                    remote_file_request,
                    request_format,
                    clipboard.remote_file_count(),
                    clipboard.take_remote_download_result(),
                )
            });
        if let Some((
            received,
            received_image,
            reply,
            file_response,
            remote_file_request,
            request_format,
            remote_file_count,
            download_result,
        )) = clipboard_messages
        {
            if let Some(text) = received {
                state.lock().unwrap().clipboard_text = Some(text);
                ctx.request_repaint();
            }
            if let Some(image) = received_image {
                state.lock().unwrap().clipboard_image = Some(image);
                ctx.request_repaint();
            }
            let sent_clipboard = reply.as_ref().is_some_and(|reply| !reply.is_error());
            let response = reply.and_then(|reply| {
                stage
                    .get_svc_processor_mut::<CliprdrClient>()?
                    .submit_format_data(reply)
                    .ok()
            });
            if let Some(packet) = response
                .and_then(|message| stage.process_svc_processor_messages(message).ok())
            {
                framed.write_all(&packet)?;
                if sent_clipboard {
                    state
                        .lock()
                        .unwrap()
                        .clipboard_sent
                        .store(true, Ordering::Relaxed);
                    ctx.request_repaint();
                }
            }
            let file_response = file_response.and_then(|response| {
                stage
                    .get_svc_processor_mut::<CliprdrClient>()?
                    .submit_file_contents(response)
                    .ok()
            });
            if let Some(packet) = file_response
                .and_then(|message| stage.process_svc_processor_messages(message).ok())
            {
                framed.write_all(&packet)?;
            }
            let remote_file_request = remote_file_request.and_then(|request| {
                stage
                    .get_svc_processor_mut::<CliprdrClient>()?
                    .request_file_contents(request)
                    .ok()
            });
            if let Some(packet) = remote_file_request
                .and_then(|message| stage.process_svc_processor_messages(message).ok())
            {
                framed.write_all(&packet)?;
            }
            if let Some(format) = request_format {
                let request = stage
                    .get_svc_processor_mut::<CliprdrClient>()
                    .and_then(|channel| channel.initiate_paste(format).ok());
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
            let mut state = state.lock().unwrap();
            state.remote_clipboard_file_count = remote_file_count;
            if download_result.is_some() {
                state.clipboard_file_download = download_result;
                ctx.request_repaint();
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
            .process(&mut image, action, &payload, framed.last_read_at())
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
                    dirty_regions.push(region);
                }
                ActiveStageOutput::Terminate(_) => {
                    return Err(error("The RDP server ended the session."))
                }
                ActiveStageOutput::DeactivateAll => {
                    publish_damage(
                        &mut dirty_regions,
                        &image,
                        width,
                        height,
                        state,
                        ctx,
                        &mut converted_damage,
                    )?;
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
                            ..
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
                            activation.step(&pdu, framed.last_read_at(), &mut output)
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
        publish_damage(
            &mut dirty_regions,
            &image,
            width,
            height,
            state,
            ctx,
            &mut converted_damage,
        )?;
    }
}

struct GatewayStream {
    reader: Mutex<tokio::io::ReadHalf<ironrdp_mstsgu::GwClient>>,
    output: Arc<GatewayOutput>,
    runtime: Arc<tokio::runtime::Runtime>,
    read_timeout_ms: AtomicU64,
}

impl Read for GatewayStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        use tokio::io::AsyncReadExt;
        let timeout = Duration::from_millis(self.read_timeout_ms.load(Ordering::Relaxed));
        self.runtime.block_on(async {
            tokio::time::timeout(
                timeout,
                self.reader
                    .lock()
                    .map_err(|_| io::Error::other("RD Gateway stream lock poisoned"))?
                    .read(buffer),
            )
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "RD Gateway read timed out"))?
        })
    }
}

impl Write for GatewayStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.output.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

impl DesktopStream for GatewayStream {
    fn input_socket(&self) -> io::Result<Box<dyn InputSocket>> {
        Ok(Box::new(GatewayInput {
            output: Arc::clone(&self.output),
        }))
    }

    fn set_read_timeout(&self, timeout: Duration) -> io::Result<()> {
        self.read_timeout_ms.store(
            timeout.as_millis().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
        Ok(())
    }
}

struct GatewayOutput {
    writer: Mutex<tokio::io::WriteHalf<ironrdp_mstsgu::GwClient>>,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl GatewayOutput {
    fn write(&self, buffer: &[u8]) -> io::Result<usize> {
        use tokio::io::AsyncWriteExt;
        self.runtime.block_on(async {
            tokio::time::timeout(
                CONNECT_TIMEOUT,
                self.writer
                    .lock()
                    .map_err(|_| io::Error::other("RD Gateway writer lock poisoned"))?
                    .write(buffer),
            )
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "RD Gateway write timed out"))?
        })
    }

    fn flush(&self) -> io::Result<()> {
        use tokio::io::AsyncWriteExt;
        self.runtime.block_on(async {
            tokio::time::timeout(
                CONNECT_TIMEOUT,
                self.writer
                    .lock()
                    .map_err(|_| io::Error::other("RD Gateway writer lock poisoned"))?
                    .flush(),
            )
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "RD Gateway flush timed out"))?
        })
    }

    fn shutdown(&self) {
        use tokio::io::AsyncWriteExt;
        let _ = self.runtime.block_on(async {
            tokio::time::timeout(CONNECT_TIMEOUT, async {
                self.writer
                    .lock()
                    .map_err(|_| io::Error::other("RD Gateway writer lock poisoned"))?
                    .shutdown()
                    .await
            })
            .await
        });
    }
}

struct GatewayInput {
    output: Arc<GatewayOutput>,
}

impl Write for GatewayInput {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.output.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

impl InputSocket for GatewayInput {
    fn disconnect(&self) {
        self.output.shutdown();
    }
}

pub(crate) fn open_gateway(
    endpoint: &str,
    server: &str,
    server_port: u16,
    username: &str,
    domain: &str,
    password: &str,
) -> io::Result<(Box<dyn DesktopStream>, SocketAddr)> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let endpoint = gateway_endpoint(endpoint)?;
    let username = if domain.trim().is_empty() {
        username.to_owned()
    } else {
        format!("{}\\{}", domain.trim(), username)
    };
    let target = ironrdp_mstsgu::GwConnectTarget {
        gw_endpoint: endpoint,
        gw_user: username,
        gw_pass: password.to_owned(),
        smart_card: None,
        server: server.to_owned(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| error("Could not start the RD Gateway transport."))?;
    let result = block_on_timeout(
        &runtime,
        CONNECT_TIMEOUT,
        ironrdp_mstsgu::GwClient::connect_with_port_and_certificate_validation(
            &target,
            "ServerLauncher",
            server_port,
            ironrdp_tls::CertificateValidation::Strict,
            None,
        ),
    );
    let (stream, local_addr) = result
        .map_err(|_| error("The RD Gateway connection timed out."))?
        .map_err(|_| error("Could not connect to the RD Gateway. Check its address, certificate, and credentials."))?;
    let runtime = Arc::new(runtime);
    let (reader, writer) = tokio::io::split(stream);
    Ok((
        Box::new(GatewayStream {
            reader: Mutex::new(reader),
            output: Arc::new(GatewayOutput {
                writer: Mutex::new(writer),
                runtime: Arc::clone(&runtime),
            }),
            runtime,
            read_timeout_ms: AtomicU64::new(CONNECT_TIMEOUT.as_millis() as u64),
        }),
        local_addr,
    ))
}

fn block_on_timeout<T>(
    runtime: &tokio::runtime::Runtime,
    duration: Duration,
    future: impl std::future::Future<Output = T>,
) -> Result<T, tokio::time::error::Elapsed> {
    runtime.block_on(async move { tokio::time::timeout(duration, future).await })
}

pub(crate) fn gateway_endpoint(endpoint: &str) -> io::Result<String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() || endpoint.contains("://") || endpoint.contains(['/', '?', '#', '@']) {
        return Err(error(
            "Enter an RD Gateway host, optionally followed by a port.",
        ));
    }
    if let Ok(address) = endpoint.parse::<std::net::IpAddr>() {
        return Ok(match address {
            std::net::IpAddr::V4(_) => format!("{address}:443"),
            std::net::IpAddr::V6(_) => format!("[{address}]:443"),
        });
    }
    if endpoint.starts_with('[') {
        let (address, suffix) = endpoint
            .split_once(']')
            .ok_or_else(|| error("The RD Gateway IPv6 address is invalid."))?;
        address[1..]
            .parse::<std::net::Ipv6Addr>()
            .map_err(|_| error("The RD Gateway IPv6 address is invalid."))?;
        if suffix.is_empty() {
            return Ok(format!("{endpoint}:443"));
        }
        let port = suffix
            .strip_prefix(':')
            .and_then(|port| port.parse::<u16>().ok())
            .filter(|port| *port != 0)
            .ok_or_else(|| error("The RD Gateway port must be between 1 and 65535."))?;
        return Ok(format!("[{address}]:{port}", address = &address[1..]));
    }
    if let Some((host, port)) = endpoint.rsplit_once(':') {
        if !host.contains(':') && !host.is_empty() {
            let port = port
                .parse::<u16>()
                .ok()
                .filter(|port| *port != 0)
                .ok_or_else(|| error("The RD Gateway port must be between 1 and 65535."))?;
            return Ok(format!("{host}:{port}"));
        }
        return Err(error(
            "IPv6 RD Gateway addresses must be enclosed in brackets.",
        ));
    }
    if endpoint.chars().any(char::is_whitespace) {
        return Err(error("The RD Gateway host cannot contain spaces."));
    }
    Ok(format!("{endpoint}:443"))
}

#[cfg(test)]
mod gateway_tests {
    use super::{block_on_timeout, gateway_endpoint};
    use std::time::Duration;

    #[test]
    fn normalizes_gateway_hosts_and_checks_explicit_ports() {
        assert_eq!(
            gateway_endpoint("rdg.example.test").unwrap(),
            "rdg.example.test:443"
        );
        assert_eq!(
            gateway_endpoint("rdg.example.test:8443").unwrap(),
            "rdg.example.test:8443"
        );
        assert_eq!(
            gateway_endpoint("2001:db8::1").unwrap(),
            "[2001:db8::1]:443"
        );
        assert_eq!(
            gateway_endpoint("[2001:db8::1]:8443").unwrap(),
            "[2001:db8::1]:8443"
        );
        assert!(gateway_endpoint("rdg.example.test:0").is_err());
        assert!(gateway_endpoint("https://rdg.example.test").is_err());
    }

    #[test]
    fn gateway_timeout_is_created_inside_the_worker_runtime() {
        std::thread::spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            assert_eq!(
                block_on_timeout(&runtime, Duration::from_secs(1), async { 7 }).unwrap(),
                7
            );
        })
        .join()
        .unwrap();
    }
}

fn publish_damage(
    regions: &mut Vec<Region>,
    image: &DecodedImage,
    width: u16,
    height: u16,
    state: &Arc<Mutex<Connection>>,
    ctx: &egui::Context,
    converted_pixels: &mut Vec<egui::Color32>,
) -> io::Result<()> {
    if regions.is_empty() {
        return Ok(());
    }
    let prepared = prepare_damage(
        regions,
        image.data(),
        image.stride(),
        width,
        height,
        converted_pixels,
    )?;
    let mut shared = state.lock().unwrap();
    for (region, range) in prepared {
        shared
            .framebuffer
            .write_colors(region, &converted_pixels[range])
            .map_err(error)?;
    }
    regions.clear();
    shared.display_updates = shared.display_updates.wrapping_add(1);
    drop(shared);
    ctx.request_repaint();
    Ok(())
}

fn prepare_damage(
    regions: &[Region],
    data: &[u8],
    stride: usize,
    width: u16,
    height: u16,
    pixels: &mut Vec<egui::Color32>,
) -> io::Result<Vec<(Region, Range<usize>)>> {
    pixels.clear();
    let minimum_stride = usize::from(width)
        .checked_mul(4)
        .ok_or_else(|| error("The RDP server reported invalid framebuffer damage."))?;
    if stride < minimum_stride {
        return Err(error("The RDP server reported invalid framebuffer damage."));
    }
    let frame_pixels = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| error("The RDP server reported invalid framebuffer damage."))?;
    let mut damaged_pixels = 0usize;
    for &region in regions {
        let [x, y] = region.origin;
        let [w, h] = region.size;
        let right = x
            .checked_add(w)
            .ok_or_else(|| error("The RDP server reported invalid framebuffer damage."))?;
        let bottom = y
            .checked_add(h)
            .ok_or_else(|| error("The RDP server reported invalid framebuffer damage."))?;
        if w == 0 || h == 0 || right > usize::from(width) || bottom > usize::from(height) {
            return Err(error("The RDP server reported invalid framebuffer damage."));
        }
        damaged_pixels = damaged_pixels
            .checked_add(
                w.checked_mul(h)
                    .ok_or_else(|| error("The RDP server reported invalid framebuffer damage."))?,
            )
            .ok_or_else(|| error("The RDP server reported invalid framebuffer damage."))?;
    }
    let full_frame = [Region {
        origin: [0, 0],
        size: [usize::from(width), usize::from(height)],
    }];
    let regions = if damaged_pixels >= frame_pixels {
        &full_frame[..]
    } else {
        regions
    };
    let mut prepared = Vec::with_capacity(regions.len());
    for &region in regions {
        let [x, y] = region.origin;
        let [w, h] = region.size;
        let bottom = y + h;
        let count = w
            .checked_mul(h)
            .ok_or_else(|| error("The RDP server reported invalid framebuffer damage."))?;
        pixels
            .try_reserve(count)
            .map_err(|_| error("Could not allocate the RDP frame update."))?;
        let start = pixels.len();
        for row in y..bottom {
            let byte_start = row
                .checked_mul(stride)
                .and_then(|offset| x.checked_mul(4).and_then(|x| offset.checked_add(x)))
                .ok_or_else(|| error("The RDP server reported invalid framebuffer damage."))?;
            let byte_end =
                byte_start
                    .checked_add(w.checked_mul(4).ok_or_else(|| {
                        error("The RDP server reported invalid framebuffer damage.")
                    })?)
                    .ok_or_else(|| error("The RDP server reported invalid framebuffer damage."))?;
            let row_data = data
                .get(byte_start..byte_end)
                .ok_or_else(|| error("The RDP server reported invalid framebuffer damage."))?;
            pixels.extend(
                row_data
                    .chunks_exact(4)
                    .map(|pixel| egui::Color32::from_rgb(pixel[0], pixel[1], pixel[2])),
            );
        }
        prepared.push((region, start..pixels.len()));
    }
    Ok(prepared)
}

struct EgfxHandler {
    color_profile: RdpColorProfile,
    avc_enabled: bool,
}

impl GraphicsPipelineHandler for EgfxHandler {
    fn capabilities(&self) -> Vec<ironrdp_egfx::pdu::CapabilitySet> {
        use ironrdp_egfx::pdu::{CapabilitiesV107Flags, CapabilitiesV8Flags, CapabilitySet as Cap};
        if !self.avc_enabled {
            match self.color_profile {
                RdpColorProfile::GfxRfx => {
                    return vec![Cap::V8 {
                        flags: CapabilitiesV8Flags::SMALL_CACHE,
                    }];
                }
                RdpColorProfile::GfxRfxProgressive => {
                    return vec![Cap::V10_7 {
                        flags: CapabilitiesV107Flags::AVC_DISABLED
                            | CapabilitiesV107Flags::SMALL_CACHE,
                    }];
                }
                _ => {}
            }
            use ironrdp_egfx::pdu::{
                CapabilitiesV10Flags, CapabilitiesV103Flags, CapabilitiesV104Flags,
                CapabilitiesV81Flags,
            };
            let no_avc = CapabilitiesV104Flags::AVC_DISABLED | CapabilitiesV104Flags::SMALL_CACHE;
            return vec![
                Cap::V10_7 {
                    flags: CapabilitiesV107Flags::AVC_DISABLED | CapabilitiesV107Flags::SMALL_CACHE,
                },
                Cap::V10_6 { flags: no_avc },
                Cap::V10_5 { flags: no_avc },
                Cap::V10_4 { flags: no_avc },
                Cap::V10_3 {
                    flags: CapabilitiesV103Flags::AVC_DISABLED,
                },
                Cap::V10_2 {
                    flags: CapabilitiesV10Flags::AVC_DISABLED
                        | CapabilitiesV10Flags::SMALL_CACHE,
                },
                Cap::V10 {
                    flags: CapabilitiesV10Flags::AVC_DISABLED
                        | CapabilitiesV10Flags::SMALL_CACHE,
                },
                Cap::V8_1 {
                    flags: CapabilitiesV81Flags::SMALL_CACHE,
                },
                Cap::V8 {
                    flags: CapabilitiesV8Flags::SMALL_CACHE,
                },
            ];
        }
        match self.color_profile {
            RdpColorProfile::GfxRfx => vec![Cap::V8 {
                flags: CapabilitiesV8Flags::SMALL_CACHE,
            }],
            RdpColorProfile::GfxAvc420 => vec![
                Cap::V8_1 {
                    flags: ironrdp_egfx::pdu::CapabilitiesV81Flags::AVC420_ENABLED
                        | ironrdp_egfx::pdu::CapabilitiesV81Flags::SMALL_CACHE,
                },
                Cap::V8 {
                    flags: CapabilitiesV8Flags::SMALL_CACHE,
                },
            ],
            RdpColorProfile::GfxAvc444 => vec![
                Cap::V10_7 {
                    flags: CapabilitiesV107Flags::SMALL_CACHE,
                },
                Cap::V8_1 {
                    flags: ironrdp_egfx::pdu::CapabilitiesV81Flags::AVC420_ENABLED
                        | ironrdp_egfx::pdu::CapabilitiesV81Flags::SMALL_CACHE,
                },
                Cap::V8 {
                    flags: CapabilitiesV8Flags::SMALL_CACHE,
                },
            ],
            RdpColorProfile::GfxRfxProgressive => vec![Cap::V10_7 {
                flags: CapabilitiesV107Flags::AVC_DISABLED | CapabilitiesV107Flags::SMALL_CACHE,
            }],
            _ => vec![
                Cap::V10_7 {
                    flags: CapabilitiesV107Flags::SMALL_CACHE,
                },
                Cap::V10_6 {
                    flags: ironrdp_egfx::pdu::CapabilitiesV104Flags::SMALL_CACHE,
                },
                Cap::V10_5 {
                    flags: ironrdp_egfx::pdu::CapabilitiesV104Flags::SMALL_CACHE,
                },
                Cap::V10_4 {
                    flags: ironrdp_egfx::pdu::CapabilitiesV104Flags::SMALL_CACHE,
                },
                Cap::V10_3 {
                    flags: ironrdp_egfx::pdu::CapabilitiesV103Flags::empty(),
                },
                Cap::V10_2 {
                    flags: ironrdp_egfx::pdu::CapabilitiesV10Flags::SMALL_CACHE,
                },
                Cap::V10_1,
                Cap::V10 {
                    flags: ironrdp_egfx::pdu::CapabilitiesV10Flags::SMALL_CACHE,
                },
                Cap::V8_1 {
                    flags: ironrdp_egfx::pdu::CapabilitiesV81Flags::AVC420_ENABLED
                        | ironrdp_egfx::pdu::CapabilitiesV81Flags::SMALL_CACHE,
                },
                Cap::V8 {
                    flags: CapabilitiesV8Flags::SMALL_CACHE,
                },
            ],
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

fn initial_dimensions(
    profile: &RdpProfile,
    monitor_size: Option<[u32; 2]>,
    fit_window_size: Option<[u32; 2]>,
) -> [u16; 2] {
    match profile.resolution {
        Resolution::Fixed => [profile.width.clamp(1, 4096), profile.height.clamp(1, 4096)],
        Resolution::MatchLocalScreen => monitor_size.unwrap_or([1280, 720]).map(|size| size as u16),
        Resolution::FitWindow => fit_window_size
            .unwrap_or([1280, 720])
            .map(|size| size.clamp(200, 4096) as u16),
    }
}

fn dual_monitor_layout(
    width: u32,
    height: u32,
    max_monitor_area: u64,
) -> Option<DisplayControlMonitorLayout> {
    let monitor_width = (width.min(8192) / 2) & !1;
    if monitor_width < 200 || !(200..=8192).contains(&height) {
        return None;
    }
    let layout_area = u64::from(monitor_width) * u64::from(height) * 2;
    if layout_area > max_monitor_area {
        return None;
    }
    let primary = MonitorLayoutEntry::new_primary(monitor_width, height).ok()?;
    let secondary = MonitorLayoutEntry::new_secondary(monitor_width, height)
        .ok()?
        .with_position(monitor_width as i32, 0)
        .ok()?;
    DisplayControlMonitorLayout::new(&[primary, secondary]).ok()
}

#[cfg(test)]
#[test]
fn multi_monitor_layout_fits_area_and_places_two_outputs_side_by_side() {
    let layout = dual_monitor_layout(1920, 1080, 1920 * 1080).unwrap();
    assert_eq!(layout.monitors().len(), 2);
    assert!(layout.monitors()[0].is_primary());
    assert_eq!(layout.monitors()[0].position(), Some((0, 0)));
    assert_eq!(layout.monitors()[0].dimensions(), (960, 1080));
    assert_eq!(layout.monitors()[1].position(), Some((960, 0)));
    assert!(dual_monitor_layout(1920, 1080, 100).is_none());
}

fn config(
    host: &SshRemoteHost,
    profile: &RdpProfile,
    password: &str,
    monitor_size: Option<[u32; 2]>,
    fit_window_size: Option<[u32; 2]>,
) -> connector::Config {
    let username = if profile.same_username_as_ssh {
        &host.user
    } else {
        &profile.username
    };
    let (enable_tls, enable_credssp) = match profile.security {
        RdpSecurity::Auto => (true, true),
        RdpSecurity::Nla => (false, true),
        RdpSecurity::Tls => (true, false),
    };
    let [width, height] = initial_dimensions(profile, monitor_size, fit_window_size);
    let (color_depth, lossy_compression) = bitmap_quality(profile);
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
        enable_tls,
        enable_credssp,
        enable_standard_rdp_security: false,
        keyboard_type: KeyboardType::IBM_ENHANCED,
        keyboard_subtype: 0,
        keyboard_layout: profile.keyboard_layout.unwrap_or(0),
        keyboard_functional_keys_count: 12,
        connection_type: ironrdp::pdu::gcc::ConnectionType::BroadbandHigh,
        ime_file_name: String::new(),
        dig_product_id: String::new(),
        desktop_size: connector::DesktopSize { width, height },
        bitmap: ironrdp::pdu::rdp::capability_sets::client_codecs_capabilities(&[])
            .ok()
            .map(|codecs| connector::BitmapConfig {
                color_depth,
                lossy_compression,
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
        enable_server_pointer: true,
        request_data: None,
        autologon: true,
        enable_audio_playback: profile.audio == Audio::Local,
        enable_audio_capture: profile.mic,
        audio_playback_mode: None,
        compression_type: Some(CompressionType::Rdp61),
        pointer_software_rendering: true,
        multitransport_flags: None,
        support_dyn_vc_gfx_protocol: color_depth != 8,
        rdp_version: ironrdp::pdu::gcc::RdpVersion::V5_PLUS,
        performance_flags: performance,
        desktop_scale_factor: 0,
        desktop_physical_size: None,
        desktop_orientation: None,
        device_scale_factor: None,
        monitor_layout: None,
        monitor_extended_layout: None,
        relative_mouse_input: false,
        hardware_id: None,
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        remote_application_mode: false,
        rail_support_level: ironrdp::pdu::rdp::capability_sets::RailSupportLevel::empty(),
    }
}

fn bitmap_quality(profile: &RdpProfile) -> (u32, bool) {
    if let Some(color_profile) = profile.color_profile {
        return match color_profile {
            RdpColorProfile::Automatic
            | RdpColorProfile::GfxRfx
            | RdpColorProfile::GfxRfxProgressive
            | RdpColorProfile::GfxAvc420
            | RdpColorProfile::GfxAvc444
            | RdpColorProfile::RemoteFx
            | RdpColorProfile::TrueColor32 => (32, false),
            RdpColorProfile::TrueColor24 => (24, false),
            RdpColorProfile::HighColor16 => (16, false),
            RdpColorProfile::HighColor15 => (15, false),
            RdpColorProfile::Color256 => (8, false),
        };
    }
    match profile.quality {
        Quality::Auto => (
            match profile.color_depth {
                16 | 24 => u32::from(profile.color_depth),
                _ => 32,
            },
            false,
        ),
        Quality::Best => (32, false),
        Quality::Good => (24, true),
        Quality::Fast => (16, true),
    }
}

fn administrative_cluster_data(enabled: bool) -> Option<ironrdp::pdu::gcc::ClientClusterData> {
    use ironrdp::pdu::gcc::{ClientClusterData, RedirectionFlags, RedirectionVersion};

    enabled.then_some(ClientClusterData {
        flags: RedirectionFlags::REDIRECTION_SUPPORTED
            | RedirectionFlags::REDIRECTED_SESSION_FIELD_VALID,
        redirection_version: RedirectionVersion::V5,
        redirected_session_id: 0,
    })
}

#[cfg(test)]
#[test]
fn rdp_quality_presets_set_bitmap_depth_and_adaptive_color() {
    let mut profile = RdpProfile::default();
    assert_eq!(bitmap_quality(&profile), (32, false));
    profile.color_depth = 16;
    assert_eq!(bitmap_quality(&profile), (16, false));
    profile.quality = Quality::Best;
    assert_eq!(bitmap_quality(&profile), (32, false));
    profile.quality = Quality::Good;
    assert_eq!(bitmap_quality(&profile), (24, true));
    profile.quality = Quality::Fast;
    assert_eq!(bitmap_quality(&profile), (16, true));
    profile.color_profile = Some(RdpColorProfile::HighColor15);
    assert_eq!(bitmap_quality(&profile), (15, false));
    profile.color_profile = Some(RdpColorProfile::TrueColor24);
    assert_eq!(bitmap_quality(&profile), (24, false));
    profile.color_profile = Some(RdpColorProfile::Color256);
    assert_eq!(bitmap_quality(&profile), (8, false));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphics_capabilities_never_advertise_avc_without_a_decoder() {
        use ironrdp_egfx::pdu::{
            CapabilitiesV107Flags, CapabilitiesV10Flags, CapabilitiesV103Flags,
            CapabilitiesV104Flags, CapabilitiesV81Flags, CapabilitySet,
        };

        let capabilities = EgfxHandler {
            color_profile: RdpColorProfile::Automatic,
            avc_enabled: false,
        }
        .capabilities();
        assert!(capabilities.iter().all(|capability| match capability {
            CapabilitySet::V8_1 { flags } => {
                !flags.contains(CapabilitiesV81Flags::AVC420_ENABLED)
            }
            CapabilitySet::V10 { flags } | CapabilitySet::V10_2 { flags } => {
                flags.contains(CapabilitiesV10Flags::AVC_DISABLED)
            }
            CapabilitySet::V10_3 { flags } => {
                flags.contains(CapabilitiesV103Flags::AVC_DISABLED)
            }
            CapabilitySet::V10_4 { flags }
            | CapabilitySet::V10_5 { flags }
            | CapabilitySet::V10_6 { flags }
            | CapabilitySet::V10_6Err { flags } => {
                flags.contains(CapabilitiesV104Flags::AVC_DISABLED)
            }
            CapabilitySet::V10_7 { flags } => {
                flags.contains(CapabilitiesV107Flags::AVC_DISABLED)
            }
            CapabilitySet::V8 { .. } => true,
            CapabilitySet::V10_1 => false,
        }));
    }

    #[test]
    fn graphics_capabilities_advertise_avc_when_a_decoder_is_loaded() {
        use ironrdp_egfx::pdu::{CapabilitiesV81Flags, CapabilitySet};

        let capabilities = EgfxHandler {
            color_profile: RdpColorProfile::GfxAvc420,
            avc_enabled: true,
        }
        .capabilities();
        assert!(capabilities.iter().any(|capability| matches!(
            capability,
            CapabilitySet::V8_1 { flags }
                if flags.contains(CapabilitiesV81Flags::AVC420_ENABLED)
        )));
    }

    #[test]
    fn prepares_damage_pixels_outside_the_shared_framebuffer() {
        let data = [
            255, 0, 0, 0, 0, 255, 0, 0, 91, 91, 91, 91, 0, 0, 255, 0, 255, 255, 255, 0,
        ];
        let regions = [Region {
            origin: [0, 0],
            size: [2, 2],
        }];
        let mut pixels = Vec::new();
        let prepared = prepare_damage(&regions, &data, 12, 2, 2, &mut pixels).unwrap();
        assert_eq!(prepared, [(regions[0], 0..4)]);
        assert_eq!(
            pixels,
            [
                egui::Color32::RED,
                egui::Color32::GREEN,
                egui::Color32::BLUE,
                egui::Color32::WHITE,
            ]
        );
    }

    #[test]
    fn rejects_damage_outside_decoded_image_data() {
        let mut pixels = Vec::new();
        let result = prepare_damage(
            &[Region {
                origin: [0, 1],
                size: [1, 1],
            }],
            &[0, 0, 0, 0],
            4,
            1,
            2,
            &mut pixels,
        );
        assert!(result.is_err());
    }

    #[test]
    fn coalesces_damage_larger_than_the_frame_into_one_bounded_upload() {
        let data = [255, 0, 0, 0, 0, 255, 0, 0];
        let region = Region {
            origin: [0, 0],
            size: [2, 1],
        };
        let mut pixels = Vec::new();
        let prepared = prepare_damage(&[region, region], &data, 8, 2, 1, &mut pixels).unwrap();
        assert_eq!(prepared, [(region, 0..2)]);
        assert_eq!(pixels, [egui::Color32::RED, egui::Color32::GREEN]);
    }

    #[test]
    fn administrative_session_requests_session_zero_only_when_enabled() {
        use ironrdp::pdu::gcc::{RedirectionFlags, RedirectionVersion};

        assert!(administrative_cluster_data(false).is_none());
        let cluster = administrative_cluster_data(true).unwrap();
        assert!(cluster
            .flags
            .contains(RedirectionFlags::REDIRECTION_SUPPORTED));
        assert!(cluster
            .flags
            .contains(RedirectionFlags::REDIRECTED_SESSION_FIELD_VALID));
        assert_eq!(cluster.redirection_version, RedirectionVersion::V5);
        assert_eq!(cluster.redirected_session_id, 0);
    }

    #[test]
    fn security_modes_never_enable_legacy_rdp_security() {
        let host = SshRemoteHost {
            id: "test".into(),
            name: "Test".into(),
            host: "host.example".into(),
            port: 22,
            user: "user".into(),
            key_path: String::new(),
            password: String::new(),
            credential_ref: None,
            remote_cwd: String::new(),
            use_agent: true,
            strict_host_key: false,
            rdp: None,
            vnc: None,
        };

        for (security, tls, credssp) in [
            (RdpSecurity::Auto, true, true),
            (RdpSecurity::Nla, false, true),
            (RdpSecurity::Tls, true, false),
        ] {
            let profile = RdpProfile {
                security,
                ..Default::default()
            };
            let config = config(&host, &profile, "test-password", None, None);
            assert_eq!((config.enable_tls, config.enable_credssp), (tls, credssp));
            assert!(!config.enable_standard_rdp_security);
            assert!(config.enable_audio_playback);
        }

        let profile = RdpProfile {
            audio: Audio::Off,
            ..Default::default()
        };
        assert!(!config(&host, &profile, "test-password", None, None).enable_audio_playback);
        assert!(!config(&host, &profile, "test-password", None, None).enable_audio_capture);
        let profile = RdpProfile {
            mic: true,
            ..Default::default()
        };
        assert!(config(&host, &profile, "test-password", None, None).enable_audio_capture);
    }

    #[test]
    fn matches_monitor_dimensions_and_handles_missing_monitor_info() {
        let profile = RdpProfile {
            resolution: Resolution::MatchLocalScreen,
            ..Default::default()
        };
        assert_eq!(
            initial_dimensions(&profile, Some([2560, 1440]), None),
            [2560, 1440]
        );
        assert_eq!(initial_dimensions(&profile, None, None), [1280, 720]);
    }

    #[test]
    fn fit_window_uses_measured_canvas_dimensions_for_initial_session() {
        let profile = RdpProfile::default();
        assert_eq!(
            initial_dimensions(&profile, Some([2560, 1440]), Some([1350, 666])),
            [1350, 666]
        );
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

    #[test]
    fn gfx_profile_limits_advertised_capabilities() {
        let rfx = EgfxHandler {
            color_profile: RdpColorProfile::GfxRfx,
            avc_enabled: false,
        }
        .capabilities();
        assert!(matches!(
            rfx.as_slice(),
            [ironrdp_egfx::pdu::CapabilitySet::V8 { .. }]
        ));

        let progressive = EgfxHandler {
            color_profile: RdpColorProfile::GfxRfxProgressive,
            avc_enabled: false,
        }
        .capabilities();
        assert!(matches!(
            progressive.as_slice(),
            [ironrdp_egfx::pdu::CapabilitySet::V10_7 { flags }]
                if flags.contains(ironrdp_egfx::pdu::CapabilitiesV107Flags::AVC_DISABLED)
        ));
    }
}
