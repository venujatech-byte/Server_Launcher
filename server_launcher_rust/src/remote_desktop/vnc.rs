use super::{
    credentials::{self, DesktopProtocol},
    framebuffer::{Framebuffer, Region},
    profile::{Clipboard, Encoding, Quality},
    tunnel::{self, DesktopStream},
};
use crate::config::SshRemoteHost;
use des::{
    cipher::{Block, BlockEncrypt, KeyInit},
    Des,
};
use std::{
    hash::{BuildHasher, RandomState},
    io::{self, BufReader, Read, Write},
    net::{IpAddr, TcpStream, ToSocketAddrs},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub(crate) const CLIPBOARD_LIMIT: usize = 1024 * 1024;

const RAW: i32 = 0;
const COPY_RECT: i32 = 1;
const TIGHT: i32 = 7;
const ZRLE: i32 = 16;
const RICH_CURSOR: i32 = -239;
const X_CURSOR: i32 = -240;
const POINTER_POS: [i32; 2] = [-225, -232];
const DESKTOP_SIZE: i32 = -223;
const EXTENDED_DESKTOP_SIZE: i32 = -308;
const COMPRESS_LEVEL_0: i32 = -256;
const QUALITY_LEVEL_0: i32 = -32;
const MAX_ZRLE_COMPRESSED: usize = 64 * 1024 * 1024;
const MAX_TIGHT_COMPRESSED: usize = 64 * 1024 * 1024;

const TIMEOUT: Duration = Duration::from_secs(8);
const TUNNEL_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Input {
    Key {
        keysym: u32,
        physical_key: Option<egui::Key>,
        pressed: bool,
    },
    #[cfg(target_os = "windows")]
    RdpKey {
        scan_code: u8,
        extended: bool,
        pressed: bool,
    },
    Pointer {
        x: u16,
        y: u16,
        buttons: u8,
    },
    PointerMove {
        x: u16,
        y: u16,
        buttons: u8,
    },
    PointerExtra {
        x: u16,
        y: u16,
        button: u8,
        pressed: bool,
    },
    UnicodeText(char),
    ClipboardText(String),
    ClipboardImage(super::clipboard_image::ClipboardImage),
    ClipboardFiles(Vec<PathBuf>),
    DownloadClipboardFiles(PathBuf),
    VncResize {
        size: [u16; 2],
        screen: [u8; 16],
    },
}

impl Input {
    fn write(self, stream: &mut impl Write) -> io::Result<bool> {
        match self {
            Self::ClipboardText(text) => {
                let Some(text): Option<Vec<u8>> = text
                    .chars()
                    .map(|ch| u8::try_from(u32::from(ch)).ok())
                    .collect()
                else {
                    return Ok(false);
                };
                let [a, b, c, d] = (text.len() as u32).to_be_bytes();
                stream.write_all(&[6, 0, 0, 0, a, b, c, d])?;
                stream.write_all(&text).map(|()| true)
            }
            Self::ClipboardImage(_) | Self::ClipboardFiles(_) | Self::DownloadClipboardFiles(_) => {
                Ok(false)
            }
            #[cfg(target_os = "windows")]
            Self::RdpKey { .. } => Ok(false),
            Self::UnicodeText(ch) => {
                let codepoint = u32::from(ch);
                let keysym = if codepoint <= 0xff {
                    codepoint
                } else {
                    0x0100_0000 | codepoint
                };
                let [a, b, c, d] = keysym.to_be_bytes();
                stream.write_all(&[4, 1, 0, 0, a, b, c, d])?;
                stream.write_all(&[4, 0, 0, 0, a, b, c, d]).map(|()| false)
            }
            Self::VncResize {
                size: [width, height],
                mut screen,
            } => {
                let [w0, w1] = width.to_be_bytes();
                let [h0, h1] = height.to_be_bytes();
                screen[4..12].copy_from_slice(&[0, 0, 0, 0, w0, w1, h0, h1]);
                let mut message = [0; 24];
                message[..8].copy_from_slice(&[251, 0, w0, w1, h0, h1, 1, 0]);
                message[8..].copy_from_slice(&screen);
                stream.write_all(&message).map(|()| false)
            }
            Self::Key {
                keysym, pressed, ..
            } => {
                let [a, b, c, d] = keysym.to_be_bytes();
                stream
                    .write_all(&[4, u8::from(pressed), 0, 0, a, b, c, d])
                    .map(|()| false)
            }
            Self::Pointer { x, y, buttons } | Self::PointerMove { x, y, buttons } => {
                let [a, b] = x.to_be_bytes();
                let [c, d] = y.to_be_bytes();
                stream.write_all(&[5, buttons, a, b, c, d]).map(|()| false)
            }
            // Classic RFB has no separate side-button event; its high button-mask bits
            // are commonly interpreted as wheel events by servers.
            Self::PointerExtra { .. } => Ok(false),
        }
    }
}

fn clipboard_unsupported_reason(event: &Input) -> Option<&'static str> {
    match event {
        Input::ClipboardText(text) if text.chars().any(|ch| u32::from(ch) > 0xff) => {
            Some("Classic VNC can send Latin-1 text only")
        }
        Input::ClipboardImage(_) => Some("Classic VNC does not support clipboard images"),
        Input::ClipboardFiles(_) => Some("Classic VNC does not support clipboard files"),
        Input::DownloadClipboardFiles(_) => {
            Some("Classic VNC does not support clipboard file downloads")
        }
        _ => None,
    }
}

fn enqueue_input(sender: &mpsc::SyncSender<Input>, overflow: &AtomicBool, event: Input) {
    if let Err(mpsc::TrySendError::Full(event)) = sender.try_send(event) {
        if !matches!(event, Input::PointerMove { .. }) {
            // Disconnect rather than lose key releases when the network is stalled.
            overflow.store(true, Ordering::Release);
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
pub(crate) struct InputHandle {
    sender: mpsc::SyncSender<Input>,
    overflow: Arc<AtomicBool>,
}

#[cfg(target_os = "windows")]
impl InputHandle {
    pub(crate) fn send(&self, event: Input) {
        enqueue_input(&self.sender, &self.overflow, event);
    }
}

pub(crate) struct Connection {
    pub framebuffer: Framebuffer,
    pub status: String,
    pub connected: bool,
    pub(crate) display_updates: u64,
    pub(crate) received_bytes: Arc<AtomicU64>,
    pub(crate) sent_bytes: Arc<AtomicU64>,
    pub(crate) connected_since: Option<Instant>,
    pub(crate) finished: bool,
    pub(crate) password_retry: bool,
    pub(crate) reconnect_at: Option<Instant>,
    pub(crate) receiving_frame: bool,
    pub(crate) clipboard_too_large: bool,
    pub(crate) clipboard_sent: Arc<AtomicBool>,
    pub(crate) clipboard_unsupported: Arc<AtomicBool>,
    pub(crate) clipboard_text: Option<String>,
    pub(crate) clipboard_image: Option<super::clipboard_image::ClipboardImage>,
    pub(crate) remote_clipboard_paths: Vec<String>,
    pub(crate) remote_clipboard_file_count: usize,
    clipboard_download_active: bool,
    pub(crate) clipboard_file_download: Option<String>,
    pub(crate) vnc_screen: Option<[u8; 16]>,
    pub(crate) vnc_cursor: Option<Arc<egui::ColorImage>>,
    pub(crate) vnc_cursor_hotspot: [usize; 2],
    pub(crate) vnc_cursor_position: Option<[usize; 2]>,
    pub(crate) vnc_cursor_revision: u64,
    pub(crate) rdp_resize_supported: bool,
    pub(crate) rdp_resize_enabled: bool,
    pub(crate) resize_request: Option<[u32; 2]>,
    pub(crate) trust_prompt: Option<super::trust::Prompt>,
    input: Option<mpsc::SyncSender<Input>>,
    input_overflow: Arc<AtomicBool>,
}

impl Connection {
    pub(crate) fn start(
        host: SshRemoteHost,
        config_path: PathBuf,
        session_password: Option<Zeroizing<String>>,
        ctx: egui::Context,
    ) -> Arc<Mutex<Self>> {
        let reconnect = host
            .vnc
            .as_ref()
            .is_some_and(|profile| profile.auto_reconnect);
        let session_password = Arc::new(Mutex::new(session_password));
        Self::start_worker("Connecting to VNC…", reconnect, ctx, move |state, ctx| {
            let password = session_password.lock().unwrap().clone();
            run(&host, &config_path, password, state, ctx)
        })
    }

    pub(crate) fn start_worker(
        initial_status: &'static str,
        auto_reconnect: bool,
        ctx: egui::Context,
        run: impl Fn(&Arc<Mutex<Self>>, &egui::Context) -> io::Result<()> + Send + 'static,
    ) -> Arc<Mutex<Self>> {
        let state = Arc::new(Mutex::new(Self {
            framebuffer: Framebuffer::default(),
            status: initial_status.into(),
            connected: false,
            display_updates: 0,
            received_bytes: Arc::new(AtomicU64::new(0)),
            sent_bytes: Arc::new(AtomicU64::new(0)),
            connected_since: None,
            finished: false,
            password_retry: false,
            reconnect_at: None,
            receiving_frame: false,
            clipboard_too_large: false,
            clipboard_sent: Arc::new(AtomicBool::new(false)),
            clipboard_unsupported: Arc::new(AtomicBool::new(false)),
            clipboard_text: None,
            clipboard_image: None,
            remote_clipboard_paths: Vec::new(),
            remote_clipboard_file_count: 0,
            clipboard_download_active: false,
            clipboard_file_download: None,
            vnc_screen: None,
            vnc_cursor: None,
            vnc_cursor_hotspot: [0; 2],
            vnc_cursor_position: None,
            vnc_cursor_revision: 0,
            rdp_resize_supported: false,
            rdp_resize_enabled: true,
            resize_request: None,
            trust_prompt: None,
            input: None,
            input_overflow: Arc::new(AtomicBool::new(false)),
        }));
        let worker = state.clone();
        std::thread::spawn(move || {
            let jitter = RandomState::new();
            let mut retries = 0;
            loop {
                let result = run(&worker, &ctx);
                let mut state = worker.lock().unwrap();
                if state
                    .connected_since
                    .take()
                    .is_some_and(|since| since.elapsed() >= Duration::from_secs(30))
                {
                    retries = 0;
                }
                let password_retry = result
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.kind() == io::ErrorKind::PermissionDenied);
                let should_retry = should_retry_after(
                    auto_reconnect,
                    retries,
                    result.as_ref().err().map(io::Error::kind),
                );
                state.connected = false;
                state.input = None;
                state.clipboard_sent = Arc::new(AtomicBool::new(false));
                state.clipboard_unsupported = Arc::new(AtomicBool::new(false));
                state.clipboard_text = None;
                state.clipboard_image = None;
                state.remote_clipboard_paths.clear();
                state.remote_clipboard_file_count = 0;
                state.clipboard_download_active = false;
                state.clipboard_file_download = None;
                state.clipboard_too_large = false;
                state.vnc_screen = None;
                state.vnc_cursor = None;
                state.vnc_cursor_position = None;
                state.resize_request = None;
                state.rdp_resize_supported = false;
                state.status = result
                    .err()
                    .map_or_else(|| "Session ended".into(), |error| error.to_string());
                state.password_retry = password_retry;
                if !should_retry {
                    state.finished = true;
                    ctx.request_repaint();
                    break;
                }
                let delay = reconnect_delay(retries, jitter.hash_one(retries));
                retries += 1;
                state.reconnect_at = Some(Instant::now() + delay);
                ctx.request_repaint();
                drop(state);

                std::thread::sleep(delay);
                let mut state = worker.lock().unwrap();
                if state.reconnect_at.take().is_none() {
                    state.finished = true;
                    ctx.request_repaint();
                    break;
                }
                state.status = initial_status.into();
                state.password_retry = false;
                state.framebuffer = Framebuffer::default();
                state.received_bytes.store(0, Ordering::Relaxed);
                state.sent_bytes.store(0, Ordering::Relaxed);
                state.receiving_frame = false;
                state.input_overflow = Arc::new(AtomicBool::new(false));
                ctx.request_repaint();
            }
        });
        state
    }

    pub(crate) fn open_input(&mut self) -> (mpsc::Receiver<Input>, Arc<AtomicBool>) {
        let (sender, receiver) = mpsc::sync_channel(256);
        self.input = Some(sender);
        (receiver, self.input_overflow.clone())
    }

    pub(crate) fn send_input(&self, event: Input) {
        if let Some(input) = &self.input {
            enqueue_input(input, &self.input_overflow, event);
        }
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn input_handle(&self) -> Option<InputHandle> {
        self.input.as_ref().map(|sender| InputHandle {
            sender: sender.clone(),
            overflow: self.input_overflow.clone(),
        })
    }
}

fn reconnect_delay(attempt: u32, entropy: u64) -> Duration {
    let base_ms = 1_000_u64 << attempt;
    Duration::from_millis(base_ms + entropy % (base_ms / 4 + 1))
}

fn should_retry(auto_reconnect: bool, retries: u32) -> bool {
    auto_reconnect && retries < 5
}

fn should_retry_after(
    auto_reconnect: bool,
    retries: u32,
    error_kind: Option<io::ErrorKind>,
) -> bool {
    should_retry(auto_reconnect, retries) && error_kind != Some(io::ErrorKind::PermissionDenied)
}

fn use_rgb565(quality: Quality, color_depth: u8) -> bool {
    match quality {
        Quality::Fast => true,
        Quality::Auto => color_depth <= 16,
        Quality::Best | Quality::Good => false,
    }
}

fn tight_quality_encodings(quality: Quality, tight_enabled: bool) -> Vec<i32> {
    if !tight_enabled {
        return Vec::new();
    }
    let (compression, jpeg_quality) = match quality {
        Quality::Auto => return Vec::new(),
        Quality::Fast => (1, 3),
        Quality::Good => (5, 6),
        Quality::Best => (9, 9),
    };
    vec![
        COMPRESS_LEVEL_0 + compression,
        QUALITY_LEVEL_0 + jpeg_quality,
    ]
}

#[cfg(test)]
mod warning_tests {
    use super::needs_unencrypted_warning;

    #[test]
    fn warns_for_public_and_unresolved_vnc_hosts() {
        assert!(needs_unencrypted_warning("203.0.113.10"));
        assert!(needs_unencrypted_warning("vnc.example.test"));
        assert!(!needs_unencrypted_warning("192.168.1.12"));
        assert!(!needs_unencrypted_warning("100.94.20.30"));
    }
}

fn error(message: &str) -> io::Error {
    io::Error::other(message)
}

fn authentication_error(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn private(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        }
        IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or_else(
            || ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
            |ip| private(IpAddr::V4(ip)),
        ),
    }
}

pub(crate) fn needs_unencrypted_warning(host: &str) -> bool {
    let address = host.trim().trim_matches(['[', ']']);
    // shortcut: hostnames are treated as potentially public; resolve asynchronously to avoid a
    // conservative warning for private DNS names.
    address
        .parse::<IpAddr>()
        .map_or(true, |address| !private(address))
}

fn run(
    host: &SshRemoteHost,
    config_path: &std::path::Path,
    session_password: Option<Zeroizing<String>>,
    state: &Arc<Mutex<Connection>>,
    ctx: &egui::Context,
) -> io::Result<()> {
    let profile = host
        .vnc
        .as_ref()
        .filter(|p| p.enabled)
        .ok_or_else(|| error("VNC is disabled for this host."))?;
    let username = profile.username.trim();
    if !username.is_empty() && !profile.tunnel_via_ssh {
        return Err(authentication_error(
            "VNC username authentication requires Tunnel via SSH so the credentials stay encrypted.",
        ));
    }
    let password = match session_password {
        Some(password) => Some(password),
        None => credentials::load(host, DesktopProtocol::Vnc)
            .map_err(|message| authentication_error(&message))?,
    };
    if password.as_ref().is_some_and(|value| value.is_empty()) {
        return Err(authentication_error(
            "The saved VNC password is empty. Update it in the VNC tab.",
        ));
    }
    if username.is_empty() && password.as_ref().is_some_and(|value| value.len() > 8) {
        return Err(error(
            "Classic VNC passwords must be no longer than 8 bytes.",
        ));
    }
    if !username.is_empty()
        && (username.len() > 1024
            || password
                .as_ref()
                .is_none_or(|value| value.is_empty() || value.len() > 4096))
    {
        return Err(authentication_error(
            "VNC username authentication requires a password; the username must be at most 1024 bytes and the password at most 4096 bytes.",
        ));
    }
    let stream: Box<dyn DesktopStream> = if profile.tunnel_via_ssh {
        tunnel::open(
            host,
            profile.port,
            TUNNEL_HANDSHAKE_TIMEOUT,
            Some(config_path),
        )?
    } else {
        Box::new(open_direct(
            host,
            profile.port,
            profile.tcp_user_timeout_secs,
        )?)
    };
    let sent_bytes = state.lock().unwrap().sent_bytes.clone();
    let mut stream = tunnel::count_writes(stream, sent_bytes);
    let mut size = handshake(
        &mut stream,
        profile.shared,
        (!username.is_empty()).then_some(username.as_bytes()),
        password.as_deref().map(String::as_bytes),
        profile.tunnel_via_ssh,
    ).map_err(|err| {
        if profile.tunnel_via_ssh && matches!(err.kind(), io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset | io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) {
            error("The SSH tunnel did not reach the VNC server. Check SSH access and whether VNC is listening on the remote loopback address.")
        } else {
            err
        }
    })?;
    drop(password);
    stream.set_read_timeout(TIMEOUT)?;
    {
        let mut state = state.lock().unwrap();
        state.framebuffer.resize(size).map_err(error)?;
        state.status = if profile.tunnel_via_ssh {
            "Connected · SSH tunnel"
        } else {
            "Connected"
        }
        .into();
        state.connected = true;
        state.connected_since = Some(Instant::now());
    }
    let compact_pixels = use_rgb565(profile.quality, profile.color_depth);
    // RGB565 halves the raw payload. Good and Best retain full color.
    let pixel_format = if compact_pixels {
        [16, 16, 0, 1, 0, 31, 0, 63, 0, 31, 11, 5, 0, 0, 0, 0]
    } else {
        [32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 0, 8, 16, 0, 0, 0]
    };
    let mut format_message = [0; 20];
    format_message[4..].copy_from_slice(&pixel_format);
    stream.write_all(&format_message)?;
    let mut encodings = vec![2, 0];
    let mut enabled_encodings = Vec::with_capacity(6);
    for encoding in &profile.encodings {
        let encoding = match encoding {
            Encoding::Zrle => ZRLE,
            Encoding::CopyRect => COPY_RECT,
            Encoding::Raw => RAW,
            Encoding::Tight => TIGHT,
        };
        if !enabled_encodings.contains(&encoding) {
            enabled_encodings.push(encoding);
        }
    }
    if !enabled_encodings
        .iter()
        .any(|encoding| matches!(*encoding, TIGHT | ZRLE | COPY_RECT | RAW))
    {
        enabled_encodings.push(RAW);
    }
    let tight_enabled = enabled_encodings.contains(&TIGHT);
    enabled_encodings.extend(tight_quality_encodings(profile.quality, tight_enabled));
    let cursor_encodings: &[i32] = if profile.show_remote_cursor {
        &[RICH_CURSOR, X_CURSOR, POINTER_POS[0], POINTER_POS[1]]
    } else {
        &[]
    };
    for encoding in cursor_encodings
        .iter()
        .copied()
        .chain([EXTENDED_DESKTOP_SIZE, DESKTOP_SIZE])
    {
        if !enabled_encodings.contains(&encoding) {
            enabled_encodings.push(encoding);
        }
    }
    let count = u16::try_from(enabled_encodings.len())
        .map_err(|_| error("Too many VNC encodings were configured."))?;
    encodings.extend_from_slice(&count.to_be_bytes());
    for encoding in enabled_encodings {
        encodings.extend_from_slice(&encoding.to_be_bytes());
    }
    stream.write_all(&encodings)?;
    let writer = Arc::new(Mutex::new(stream.input_socket()?));
    if !profile.view_only {
        let (pending, overflow) = state.lock().unwrap().open_input();
        let writer = writer.clone();
        let clipboard_sent = state.lock().unwrap().clipboard_sent.clone();
        let clipboard_unsupported = state.lock().unwrap().clipboard_unsupported.clone();
        let ctx = ctx.clone();
        let download_host = host.clone();
        let download_state = Arc::clone(state);
        std::thread::spawn(move || {
            while let Ok(event) = pending.recv() {
                if let Input::DownloadClipboardFiles(directory) = event {
                    let paths = {
                        let mut state = download_state.lock().unwrap();
                        if state.clipboard_download_active {
                            state.clipboard_file_download =
                                Some("A clipboard file download is already running.".into());
                            ctx.request_repaint();
                            continue;
                        }
                        state.clipboard_download_active = true;
                        state.clipboard_file_download =
                            Some("Downloading clipboard files over SSH…".into());
                        state.remote_clipboard_paths.clone()
                    };
                    ctx.request_repaint();
                    let host = download_host.clone();
                    let state = download_state.clone();
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        let result = download_clipboard_files(&host, &paths, &directory);
                        let mut state = state.lock().unwrap();
                        state.clipboard_download_active = false;
                        state.clipboard_file_download = Some(match result {
                            Ok(message) => message,
                            Err(error) => format!("Clipboard download failed: {error}"),
                        });
                        drop(state);
                        ctx.request_repaint();
                    });
                    continue;
                }
                let mut writer = writer.lock().unwrap();
                if overflow.load(Ordering::Acquire) {
                    writer.disconnect();
                    break;
                }
                let unsupported = clipboard_unsupported_reason(&event).is_some();
                match event.write(&mut *writer) {
                    Ok(true) => {
                        clipboard_sent.store(true, Ordering::Relaxed);
                        ctx.request_repaint();
                    }
                    Ok(false) if unsupported => {
                        clipboard_unsupported.store(true, Ordering::Relaxed);
                        ctx.request_repaint();
                    }
                    Ok(false) => {}
                    Err(_) => {
                        writer.disconnect();
                        break;
                    }
                }
            }
        });
    }
    ctx.request_repaint();
    request(&mut *writer.lock().unwrap(), size, false)?;

    let received_bytes = state.lock().unwrap().received_bytes.clone();
    let mut stream = BufReader::with_capacity(
        64 * 1024,
        CountReads {
            stream,
            received_bytes,
        },
    );

    let mut pixels = Vec::new();
    let mut zrle = ZrleDecoder::default();
    let mut tight = TightDecoder::default();
    loop {
        let mut kind = [0];
        match stream.read_exact(&mut kind) {
            Ok(()) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(_) => return Err(error("The VNC connection closed.")),
        }
        match kind[0] {
            0 => {
                let mut header = [0; 3];
                stream.read_exact(&mut header)?;
                let count = u16::from_be_bytes([header[1], header[2]]);
                if count > 4096 {
                    return Err(error("The VNC update contains too many rectangles."));
                }
                let revision = {
                    let mut state = state.lock().unwrap();
                    state.receiving_frame = true;
                    state.framebuffer.revision
                };
                let mut resized = false;
                for index in 0..count {
                    if let Some(new_size) = read_rectangle(
                        &mut stream,
                        size,
                        state,
                        &mut pixels,
                        index + 1 == count,
                        compact_pixels,
                        &mut zrle,
                        &mut tight,
                    )? {
                        size = new_size;
                        resized = true;
                    }
                }
                let mut state = state.lock().unwrap();
                state.receiving_frame = false;
                if state.framebuffer.revision != revision {
                    state.display_updates = state.display_updates.wrapping_add(1);
                }
                drop(state);
                ctx.request_repaint();
                request(&mut *writer.lock().unwrap(), size, !resized)?;
            }
            2 => {}
            3 => {
                let mut header = [0; 7];
                stream.read_exact(&mut header)?;
                let size = u32::from_be_bytes(header[3..7].try_into().unwrap());
                let receive = matches!(
                    profile.clipboard,
                    Clipboard::Both | Clipboard::RemoteToLocal
                );
                if !receive || size as usize > CLIPBOARD_LIMIT {
                    discard_clipboard(&mut stream, size)?;
                    if receive {
                        state.lock().unwrap().clipboard_too_large = true;
                        ctx.request_repaint();
                    }
                } else {
                    let mut text = vec![0; size as usize];
                    stream.read_exact(&mut text)?;
                    let text: String = text.into_iter().map(char::from).collect();
                    let paths = remote_file_uri_paths(&text);
                    let mut state = state.lock().unwrap();
                    state.remote_clipboard_file_count = paths.len();
                    state.remote_clipboard_paths = paths;
                    state.clipboard_text = Some(text);
                    drop(state);
                    ctx.request_repaint();
                }
            }
            _ => return Err(error("The VNC server sent an unsupported message.")),
        }
    }
}

fn remote_file_uri_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let Some(uri) = line.strip_prefix("file://") else {
            continue;
        };
        let path = if uri.starts_with('/') {
            uri.to_owned()
        } else if let Some((authority, path)) = uri.split_once('/') {
            if !authority.eq_ignore_ascii_case("localhost") {
                continue;
            }
            format!("/{path}")
        } else {
            continue;
        };
        let Some(path) = percent_decode_uri_path(&path) else {
            continue;
        };
        let parsed = std::path::Path::new(&path);
        if !parsed.is_absolute()
            || parsed
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
            || path.contains(['\0', '\r', '\n'])
        {
            continue;
        }
        if !paths.contains(&path) {
            paths.push(path);
            if paths.len() == 128 {
                break;
            }
        }
    }
    paths
}

fn percent_decode_uri_path(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            decoded.push((hex_digit(high)? << 4) | hex_digit(low)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn download_clipboard_files(
    host: &crate::config::SshRemoteHost,
    sources: &[String],
    directory: &std::path::Path,
) -> Result<String, String> {
    if !directory.is_dir() {
        return Err("Choose an existing local folder.".into());
    }
    if sources.is_empty() {
        return Err("The remote clipboard did not contain downloadable file paths.".into());
    }
    let mut host = host.clone();
    if let Some(reference) = &host.credential_ref {
        host.password =
            crate::ssh_credentials::load(reference, &crate::ssh_credentials::binding(&host))?;
    }
    let sources = sources.iter().take(128).collect::<Vec<_>>();
    let mut destinations = Vec::with_capacity(sources.len());
    let mut names = std::collections::HashSet::new();
    for source in &sources {
        let remote_path = std::path::Path::new(source);
        let name = remote_path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty() && *name != "." && *name != "..")
            .ok_or("The remote clipboard contains an invalid file name.")?;
        if name.chars().any(char::is_control) || name.contains(['/', '\\']) {
            return Err("The remote clipboard contains an unsafe file name.".into());
        }
        if !names.insert(name.to_lowercase()) {
            return Err(format!("More than one clipboard item is named {name:?}."));
        }
        let destination = directory.join(name);
        if std::fs::symlink_metadata(&destination).is_ok() {
            return Err(format!("A local item named {name:?} already exists."));
        }
        destinations.push((source, destination));
    }
    for (source, destination) in destinations {
        let mut literal = String::with_capacity(source.len());
        for ch in source.chars() {
            if matches!(ch, '\\' | '*' | '?' | '[' | ']') {
                literal.push('\\');
            }
            literal.push(ch);
        }
        let hostname = host
            .host
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']');
        let target = if hostname.contains(':') {
            format!("{}@[{hostname}]:{literal}", host.user.trim())
        } else {
            format!("{}@{hostname}:{literal}", host.user.trim())
        };
        let output = crate::remote::build_scp_command(&host)
            .arg("-r")
            .arg("--")
            .arg(target)
            .arg(&destination)
            .output()
            .map_err(|error| format!("Could not start SCP: {error}"))?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr);
            return Err(if message.trim().is_empty() {
                format!("SCP exited with status {:?}.", output.status.code())
            } else {
                message.trim().chars().take(400).collect()
            });
        }
    }
    Ok(format!(
        "Downloaded {} clipboard item(s) over SSH.",
        sources.len()
    ))
}

fn discard_clipboard(stream: &mut impl Read, mut remaining: u32) -> io::Result<()> {
    // Bound the work spent skipping an unwanted clipboard message from the server.
    if remaining > 16 * 1024 * 1024 {
        return Err(error(
            "The VNC server sent an excessively large clipboard message.",
        ));
    }
    let deadline = Instant::now() + TIMEOUT;
    let mut buffer = [0; 8192];
    while remaining != 0 {
        if Instant::now() >= deadline {
            return Err(error("Timed out skipping a VNC clipboard message."));
        }
        let length = (remaining as usize).min(buffer.len());
        let read = stream.read(&mut buffer[..length])?;
        if read == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        remaining -= read as u32;
    }
    Ok(())
}

struct CountReads<R> {
    stream: R,
    received_bytes: Arc<AtomicU64>,
}

impl<R: Read> Read for CountReads<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = self.stream.read(buffer)?;
        self.received_bytes
            .fetch_add(length as u64, Ordering::Relaxed);
        Ok(length)
    }
}

fn open_direct(host: &SshRemoteHost, port: u16, user_timeout_secs: u32) -> io::Result<TcpStream> {
    let address = host.host.trim().trim_matches(['[', ']']);
    let addresses = (address, port)
        .to_socket_addrs()
        .map_err(|_| error("Could not resolve the VNC host."))?;
    let mut last_error = None;
    let stream = addresses
        .take(16)
        .find_map(|addr| match TcpStream::connect_timeout(&addr, TIMEOUT) {
            Ok(stream) => Some(stream),
            Err(error) => {
                last_error = Some(error);
                None
            }
        })
        .ok_or_else(|| {
            let hint = match last_error.as_ref().map(io::Error::kind) {
                Some(io::ErrorKind::ConnectionRefused) => {
                    "Check that VNC is listening on this port."
                }
                Some(io::ErrorKind::TimedOut) => "Check the network route and firewall.",
                _ => "Check that the host and VNC port are reachable.",
            };
            error(&format!("Could not reach VNC on TCP port {port}. {hint}"))
        })?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    set_tcp_user_timeout(&stream, user_timeout_secs)?;
    Ok(stream)
}

fn set_tcp_user_timeout(stream: &TcpStream, seconds: u32) -> io::Result<()> {
    if seconds == 0 {
        return Ok(());
    }
    let timeout_ms = seconds.checked_mul(1_000).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "TCP user timeout is too large.",
        )
    })?;

    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;

        let result = unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::IPPROTO_TCP,
                libc::TCP_USER_TIMEOUT,
                (&timeout_ms as *const u32).cast(),
                std::mem::size_of_val(&timeout_ms) as libc::socklen_t,
            )
        };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (stream, timeout_ms);
        Ok(())
    }
}

fn handshake(
    stream: &mut (impl Read + Write),
    shared: bool,
    username: Option<&[u8]>,
    password: Option<&[u8]>,
    allow_tunneled_plain: bool,
) -> io::Result<[usize; 2]> {
    if username.is_some() && !allow_tunneled_plain {
        return Err(authentication_error(
            "VNC username authentication requires Tunnel via SSH so the credentials stay encrypted.",
        ));
    }
    if username.is_some() && password.map_or(true, |password| password.is_empty()) {
        return Err(authentication_error(
            "VNC username authentication requires a non-empty password.",
        ));
    }
    if password.is_some_and(|password| password.is_empty()) {
        return Err(authentication_error(
            "The saved VNC password is empty. Update it in the VNC tab.",
        ));
    }
    if username.is_none() && password.is_some_and(|password| password.len() > 8) {
        return Err(error(
            "Classic VNC passwords must be no longer than 8 bytes.",
        ));
    }
    if username.is_some_and(|username| username.is_empty() || username.len() > 1024)
        || password.is_some_and(|password| password.len() > 4096)
    {
        return Err(authentication_error(
            "VNC username/password authentication requires a non-empty username (at most 1024 bytes) and a password no longer than 4096 bytes.",
        ));
    }
    let mut version = [0; 12];
    stream.read_exact(&mut version)?;
    if !matches!(
        &version,
        b"RFB 003.003\n" | b"RFB 003.007\n" | b"RFB 003.008\n"
    ) {
        return Err(error("The server did not offer a supported VNC version."));
    }
    stream.write_all(&version)?;
    let preferred = if username.is_some() {
        19
    } else if password.is_some() {
        2
    } else {
        1
    };
    let compatible = if &version == b"RFB 003.003\n" {
        if username.is_some() {
            return Err(authentication_error(
                "This VNC server uses an older protocol without tunneled username authentication.",
            ));
        }
        let mut kind = [0; 4];
        stream.read_exact(&mut kind)?;
        let offered = u32::from_be_bytes(kind);
        if offered == u32::from(preferred) {
            true
        } else if offered == 2 && password.is_none() {
            return Err(authentication_error(
                "The VNC server requires a password. Enter one to connect.",
            ));
        } else {
            false
        }
    } else {
        let mut count = [0];
        stream.read_exact(&mut count)?;
        let mut offered = [0; 255];
        let offered = &mut offered[..usize::from(count[0])];
        stream.read_exact(offered)?;
        if offered.contains(&preferred) && (password.is_some() || !offered.contains(&2)) {
            stream.write_all(&[preferred])?;
            true
        } else if password.is_none() && offered.contains(&2) {
            return Err(authentication_error(
                "The VNC server requires a password. Enter one to connect.",
            ));
        } else {
            false
        }
    };
    if !compatible {
        return Err(error(
            "The VNC server did not offer a supported security mode.",
        ));
    }
    if username.is_some() {
        let mut server_version = [0; 2];
        stream.read_exact(&mut server_version)?;
        if server_version != [0, 2] {
            return Err(error(
                "The VNC server offered an unsupported VeNCrypt version.",
            ));
        }
        stream.write_all(&[0, 2])?;
        let mut acknowledgement = [0];
        stream.read_exact(&mut acknowledgement)?;
        if acknowledgement != [0] {
            return Err(authentication_error(
                "The VNC server rejected VeNCrypt negotiation.",
            ));
        }
        let mut count = [0];
        stream.read_exact(&mut count)?;
        if count[0] > 64 {
            return Err(error("The VNC server offered too many VeNCrypt subtypes."));
        }
        let mut subtypes = vec![0; usize::from(count[0]) * 4];
        stream.read_exact(&mut subtypes)?;
        if !subtypes
            .chunks_exact(4)
            .any(|subtype| subtype == 256u32.to_be_bytes())
        {
            return Err(authentication_error(
                "The VNC server does not offer VeNCrypt Plain authentication. Configure the server for VeNCrypt Plain and keep Tunnel via SSH enabled.",
            ));
        }
        stream.write_all(&256u32.to_be_bytes())?;
        let username = username.unwrap();
        let password = password
            .ok_or_else(|| authentication_error("Enter a VNC password before connecting."))?;
        stream.write_all(&(username.len() as u32).to_be_bytes())?;
        stream.write_all(&(password.len() as u32).to_be_bytes())?;
        stream.write_all(username)?;
        stream.write_all(password)?;
    } else if let Some(password) = password {
        let mut challenge = [0; 16];
        stream.read_exact(&mut challenge)?;
        let response = response(password, challenge);
        stream.write_all(&*response)?;
    }
    if username.is_some() || password.is_some() || &version == b"RFB 003.008\n" {
        let mut result = [0; 4];
        stream.read_exact(&mut result)?;
        if result != [0; 4] {
            return Err(authentication_error(
                "The VNC server rejected authentication. Check the password or try again later.",
            ));
        }
    }
    stream.write_all(&[u8::from(shared)])?;
    let mut init = [0; 24];
    stream.read_exact(&mut init)?;
    let name_length = u32::from_be_bytes(init[20..24].try_into().unwrap());
    if name_length > 4096 {
        return Err(error("The VNC desktop name exceeds the viewer limit."));
    }
    let mut name = [0; 4096];
    stream.read_exact(&mut name[..name_length as usize])?;
    Ok([
        usize::from(u16::from_be_bytes([init[0], init[1]])),
        usize::from(u16::from_be_bytes([init[2], init[3]])),
    ])
}

fn response(password: &[u8], challenge: [u8; 16]) -> Zeroizing<[u8; 16]> {
    let mut key = Zeroizing::new([0; 8]);
    // Classic RFB uses at most eight password bytes with each byte’s bits reversed.
    for (target, byte) in key.iter_mut().zip(password) {
        *target = byte.reverse_bits();
    }
    let cipher = Des::new((&*key).into());
    let mut reply = Zeroizing::new(challenge);
    for chunk in reply.chunks_exact_mut(8) {
        cipher.encrypt_block(Block::<Des>::from_mut_slice(chunk));
    }
    reply
}

fn request(
    stream: &mut impl Write,
    [width, height]: [usize; 2],
    incremental: bool,
) -> io::Result<()> {
    let [w0, w1] = (width as u16).to_be_bytes();
    let [h0, h1] = (height as u16).to_be_bytes();
    stream.write_all(&[3, u8::from(incremental), 0, 0, 0, 0, w0, w1, h0, h1])
}

fn read_rectangle(
    stream: &mut impl Read,
    size: [usize; 2],
    state: &Mutex<Connection>,
    pixels: &mut Vec<u8>,
    last: bool,
    compact_pixels: bool,
    zrle: &mut ZrleDecoder,
    tight: &mut TightDecoder,
) -> io::Result<Option<[usize; 2]>> {
    let mut rect = [0; 12];
    stream.read_exact(&mut rect)?;
    let field = |i| usize::from(u16::from_be_bytes([rect[i], rect[i + 1]]));
    let [x, y, w, h] = [field(0), field(2), field(4), field(6)];
    let encoding = i32::from_be_bytes([rect[8], rect[9], rect[10], rect[11]]);
    if encoding == RICH_CURSOR {
        let cursor = if w == 0 || h == 0 {
            None
        } else {
            if w > 256 || h > 256 || x >= w || y >= h {
                return Err(error("The VNC server sent an invalid cursor shape."));
            }
            Some(Arc::new(read_cursor(stream, w, h, compact_pixels)?))
        };
        let mut state = state.lock().unwrap();
        state.vnc_cursor = cursor;
        if w != 0 && h != 0 {
            state.vnc_cursor_hotspot = [x, y];
        }
        state.vnc_cursor_revision = state.vnc_cursor_revision.wrapping_add(1);
        return Ok(None);
    }
    if encoding == X_CURSOR {
        let cursor = if w == 0 || h == 0 {
            None
        } else {
            if w > 256 || h > 256 || x >= w || y >= h {
                return Err(error("The VNC server sent an invalid cursor shape."));
            }
            Some(Arc::new(read_x_cursor(stream, w, h)?))
        };
        let mut state = state.lock().unwrap();
        state.vnc_cursor = cursor;
        if w != 0 && h != 0 {
            state.vnc_cursor_hotspot = [x, y];
        }
        state.vnc_cursor_revision = state.vnc_cursor_revision.wrapping_add(1);
        return Ok(None);
    }
    if POINTER_POS.contains(&encoding) {
        let mut state = state.lock().unwrap();
        state.vnc_cursor_position = (x <= size[0] && y <= size[1]).then_some([x, y]);
        return Ok(None);
    }
    if encoding == EXTENDED_DESKTOP_SIZE {
        let mut header = [0; 4];
        stream.read_exact(&mut header)?;
        let mut screen = [0; 16];
        for _ in 0..header[0] {
            stream.read_exact(&mut screen)?;
        }
        let mut state = state.lock().unwrap();
        if x == 1 && y != 0 {
            state.status = format!("VNC server rejected the resolution change (code {y})");
            return Ok(None);
        }
        state.vnc_screen = (header[0] == 1).then_some(screen);
        if x == 1 {
            state.status = format!("VNC resolution changed to {w} × {h}");
        }
        if [w, h] == size {
            return Ok(None);
        }
        state.framebuffer.resize([w, h]).map_err(error)?;
        return Ok(Some([w, h]));
    }
    if encoding == DESKTOP_SIZE {
        if !last {
            return Err(error("The VNC server sent an invalid desktop size update."));
        }
        state
            .lock()
            .unwrap()
            .framebuffer
            .resize([w, h])
            .map_err(error)?;
        return Ok(Some([w, h]));
    }
    if w == 0 || h == 0 || x + w > size[0] || y + h > size[1] {
        return Err(error("The VNC server sent an invalid rectangle."));
    }
    let region = Region {
        origin: [x, y],
        size: [w, h],
    };
    match encoding {
        RAW => {
            // Decode in row groups to bound the temporary pixel buffer.
            for row in (0..h).step_by(64) {
                let rows = (h - row).min(64);
                pixels.resize(w * rows * 4, 0);
                if compact_pixels {
                    stream.read_exact(&mut pixels[..w * rows * 2])?;
                    expand_rgb565(pixels);
                } else {
                    stream.read_exact(pixels)?;
                }
                state
                    .lock()
                    .unwrap()
                    .framebuffer
                    .write_rgba(
                        Region {
                            origin: [x, y + row],
                            size: [w, rows],
                        },
                        pixels,
                    )
                    .map_err(error)?;
            }
        }
        COPY_RECT => {
            let mut origin = [0; 4];
            stream.read_exact(&mut origin)?;
            let source = [
                usize::from(u16::from_be_bytes([origin[0], origin[1]])),
                usize::from(u16::from_be_bytes([origin[2], origin[3]])),
            ];
            state
                .lock()
                .unwrap()
                .framebuffer
                .copy_rect(source, region)
                .map_err(error)?;
        }
        ZRLE => {
            let mut length = [0; 4];
            stream.read_exact(&mut length)?;
            let length = u32::from_be_bytes(length) as usize;
            if length > MAX_ZRLE_COMPRESSED {
                return Err(error(
                    "The VNC ZRLE rectangle exceeds the compressed data limit.",
                ));
            }
            let mut compressed = vec![0; length];
            stream.read_exact(&mut compressed)?;
            let decoded = zrle.decompress(&compressed, w, h, compact_pixels)?;
            let mut state = state.lock().unwrap();
            state
                .framebuffer
                .write_rgba(region, &decoded)
                .map_err(error)?;
        }
        TIGHT => {
            let decoded = tight.decode(stream, w, h, compact_pixels)?;
            state
                .lock()
                .unwrap()
                .framebuffer
                .write_rgba(region, &decoded)
                .map_err(error)?;
        }
        _ => {
            return Err(error(
                "The VNC server sent an unsupported framebuffer encoding.",
            ))
        }
    }
    Ok(None)
}

fn read_cursor(
    stream: &mut impl Read,
    width: usize,
    height: usize,
    compact_pixels: bool,
) -> io::Result<egui::ColorImage> {
    let area = width
        .checked_mul(height)
        .filter(|area| *area <= 256 * 256)
        .ok_or_else(|| error("The VNC cursor exceeds the image limit."))?;
    let pixel_bytes = if compact_pixels { 2 } else { 4 };
    let mut pixels = vec![0; area * pixel_bytes];
    stream.read_exact(&mut pixels)?;
    if compact_pixels {
        pixels.resize(area * 4, 0);
        expand_rgb565(&mut pixels);
    }
    let mask_stride = width.div_ceil(8);
    let mut mask = vec![0; mask_stride * height];
    stream.read_exact(&mut mask)?;
    let colors = pixels
        .chunks_exact(4)
        .zip((0..area).map(|i| {
            mask[i / width * mask_stride + (i % width) / 8] & (0x80 >> (i % width % 8)) != 0
        }))
        .map(|(pixel, visible)| {
            egui::Color32::from_rgba_unmultiplied(
                pixel[0],
                pixel[1],
                pixel[2],
                if visible { 255 } else { 0 },
            )
        })
        .collect();
    Ok(egui::ColorImage {
        size: [width, height],
        pixels: colors,
    })
}

fn read_x_cursor(
    stream: &mut impl Read,
    width: usize,
    height: usize,
) -> io::Result<egui::ColorImage> {
    let mut rgb = [0; 6];
    stream.read_exact(&mut rgb)?;
    let stride = width.div_ceil(8);
    let bytes = stride * height;
    let mut bitmap = vec![0; bytes];
    let mut mask = vec![0; bytes];
    stream.read_exact(&mut bitmap)?;
    stream.read_exact(&mut mask)?;
    let pixels = (0..width * height)
        .map(|i| {
            let bit = 0x80 >> (i % width % 8);
            let row_byte = i / width * stride + (i % width) / 8;
            let color = if bitmap[row_byte] & bit != 0 {
                &rgb[..3]
            } else {
                &rgb[3..]
            };
            egui::Color32::from_rgba_unmultiplied(
                color[0],
                color[1],
                color[2],
                if mask[row_byte] & bit != 0 { 255 } else { 0 },
            )
        })
        .collect();
    Ok(egui::ColorImage {
        size: [width, height],
        pixels,
    })
}

#[derive(Default)]
struct TightDecoder {
    streams: [Option<flate2::Decompress>; 4],
}

impl TightDecoder {
    fn decode(
        &mut self,
        reader: &mut impl Read,
        width: usize,
        height: usize,
        compact_pixels: bool,
    ) -> io::Result<Vec<u8>> {
        let area = width
            .checked_mul(height)
            .filter(|area| *area <= 16 * 1024 * 1024)
            .ok_or_else(|| error("The VNC Tight rectangle exceeds the pixel limit."))?;
        let control = read_byte(reader)?;
        for (index, stream) in self.streams.iter_mut().enumerate() {
            if control & (1 << index) != 0 {
                *stream = None;
            }
        }
        let mode = control >> 4;
        if mode == 8 {
            let pixel = read_tight_pixel(reader, compact_pixels)?;
            let mut output = Vec::with_capacity(area * 4);
            for _ in 0..area {
                output.extend_from_slice(&pixel);
            }
            return Ok(output);
        }
        if mode == 9 || mode == 10 {
            let length = read_tight_length(reader)?;
            if length == 0 || length > MAX_TIGHT_COMPRESSED {
                return Err(error("The VNC Tight image exceeds the data limit."));
            }
            let mut encoded = vec![0; length];
            reader.read_exact(&mut encoded)?;
            let mut reader = image::ImageReader::new(io::Cursor::new(&encoded))
                .with_guessed_format()
                .map_err(|_| error("The VNC server sent an invalid Tight image."))?;
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(width as u32);
            limits.max_image_height = Some(height as u32);
            limits.max_alloc = Some((area * 4) as u64);
            reader.limits(limits);
            let image = reader
                .decode()
                .map_err(|_| error("The VNC server sent an invalid Tight image."))?;
            if image.width() as usize != width || image.height() as usize != height {
                return Err(error("The VNC Tight image has an unexpected size."));
            }
            return Ok(image.to_rgba8().into_raw());
        }
        if mode > 7 {
            return Err(error(
                "The VNC server sent an unsupported Tight subencoding.",
            ));
        }

        let stream_id = usize::from(mode & 3);
        let filter = if mode & 4 != 0 { read_byte(reader)? } else { 0 };
        let palette = if filter == 1 {
            let count = usize::from(read_byte(reader)?) + 1;
            let mut palette = Vec::with_capacity(count);
            for _ in 0..count {
                palette.push(read_tight_pixel(reader, compact_pixels)?);
            }
            Some(palette)
        } else {
            None
        };
        let pixel_bytes = if compact_pixels { 2 } else { 3 };
        let data_length = match filter {
            0 => area
                .checked_mul(pixel_bytes)
                .ok_or_else(|| error("The VNC Tight rectangle is too large."))?,
            1 => {
                let count = palette.as_ref().unwrap().len();
                if count <= 2 {
                    height
                        .checked_mul(width.div_ceil(8))
                        .ok_or_else(|| error("The VNC Tight palette is too large."))?
                } else {
                    area
                }
            }
            2 if !compact_pixels => area
                .checked_mul(3)
                .ok_or_else(|| error("The VNC Tight rectangle is too large."))?,
            2 => {
                return Err(error(
                    "The VNC Tight gradient filter requires 24-bit color.",
                ))
            }
            _ => return Err(error("The VNC server selected an unknown Tight filter.")),
        };
        let decoded = if data_length < 12 {
            let mut data = vec![0; data_length];
            reader.read_exact(&mut data)?;
            data
        } else {
            let length = read_tight_length(reader)?;
            if length == 0 || length > MAX_TIGHT_COMPRESSED {
                return Err(error("The VNC Tight data exceeds the compressed limit."));
            }
            let mut compressed = vec![0; length];
            reader.read_exact(&mut compressed)?;
            let stream =
                self.streams[stream_id].get_or_insert_with(|| flate2::Decompress::new(true));
            let mut decoded = Vec::with_capacity(data_length);
            let input_before = stream.total_in();
            stream
                .decompress_vec(&compressed, &mut decoded, flate2::FlushDecompress::Sync)
                .map_err(|_| error("The VNC server sent invalid Tight compressed data."))?;
            if stream.total_in() - input_before != compressed.len() as u64
                || decoded.len() != data_length
            {
                return Err(error("The VNC Tight data is truncated or oversized."));
            }
            decoded
        };
        match filter {
            0 => decode_tight_pixels(&decoded, compact_pixels),
            1 => decode_tight_palette(&decoded, width, height, palette.unwrap()),
            2 => Ok(decode_tight_gradient(&decoded, width, height)),
            _ => unreachable!(),
        }
    }
}

fn read_byte(reader: &mut impl Read) -> io::Result<u8> {
    let mut byte = [0];
    reader.read_exact(&mut byte)?;
    Ok(byte[0])
}

fn read_tight_length(reader: &mut impl Read) -> io::Result<usize> {
    let first = read_byte(reader)?;
    let mut length = usize::from(first & 0x7f);
    if first & 0x80 != 0 {
        let second = read_byte(reader)?;
        length |= usize::from(second & 0x7f) << 7;
        if second & 0x80 != 0 {
            length |= usize::from(read_byte(reader)?) << 14;
        }
    }
    Ok(length)
}

fn read_tight_pixel(reader: &mut impl Read, compact_pixels: bool) -> io::Result<[u8; 4]> {
    let length = if compact_pixels { 2 } else { 3 };
    let mut bytes = [0; 3];
    reader.read_exact(&mut bytes[..length])?;
    let mut data = &bytes[..length];
    take_pixel(&mut data, compact_pixels)
}

fn decode_tight_pixels(data: &[u8], compact_pixels: bool) -> io::Result<Vec<u8>> {
    let mut data = data;
    let mut output = Vec::with_capacity(data.len() / if compact_pixels { 2 } else { 3 } * 4);
    while !data.is_empty() {
        output.extend_from_slice(&take_pixel(&mut data, compact_pixels)?);
    }
    Ok(output)
}

fn decode_tight_palette(
    data: &[u8],
    width: usize,
    height: usize,
    palette: Vec<[u8; 4]>,
) -> io::Result<Vec<u8>> {
    let area = width * height;
    let mut output = Vec::with_capacity(area * 4);
    let mut data = data;
    if palette.len() <= 2 {
        for _ in 0..height {
            let row = take_bytes(&mut data, width.div_ceil(8))?;
            for x in 0..width {
                let index = usize::from((row[x / 8] >> (7 - x % 8)) & 1);
                output.extend_from_slice(
                    palette.get(index).ok_or_else(|| {
                        error("The VNC Tight palette references a missing color.")
                    })?,
                );
            }
        }
    } else {
        for index in take_bytes(&mut data, area)? {
            output.extend_from_slice(
                palette
                    .get(usize::from(*index))
                    .ok_or_else(|| error("The VNC Tight palette references a missing color."))?,
            );
        }
    }
    if !data.is_empty() {
        return Err(error("The VNC Tight palette contains trailing data."));
    }
    Ok(output)
}

fn decode_tight_gradient(data: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut output = vec![0; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let index = (y * width + x) * 4;
            for channel in 0..3 {
                let left = if x > 0 {
                    output[index - 4 + channel]
                } else {
                    0
                };
                let up = if y > 0 {
                    output[index - width * 4 + channel]
                } else {
                    0
                };
                let upper_left = if x > 0 && y > 0 {
                    output[index - width * 4 - 4 + channel]
                } else {
                    0
                };
                let estimate = i16::from(left) + i16::from(up) - i16::from(upper_left);
                let estimate = estimate
                    .clamp(i16::from(left.min(up)), i16::from(left.max(up)))
                    .clamp(0, 255) as u8;
                output[index + channel] =
                    estimate.wrapping_add(data[(y * width + x) * 3 + channel]);
            }
            output[index + 3] = 255;
        }
    }
    output
}

#[derive(Default)]
struct ZrleDecoder {
    stream: Option<flate2::Decompress>,
}

impl ZrleDecoder {
    fn decompress(
        &mut self,
        compressed: &[u8],
        width: usize,
        height: usize,
        compact_pixels: bool,
    ) -> io::Result<Vec<u8>> {
        let area = width
            .checked_mul(height)
            .filter(|area| *area <= 16 * 1024 * 1024)
            .ok_or_else(|| error("The VNC ZRLE rectangle exceeds the pixel limit."))?;
        let pixels = u64::try_from(area).map_err(|_| error("Invalid VNC rectangle size."))?;
        let tiles = width.div_ceil(64).saturating_mul(height.div_ceil(64)) as u64;
        let output_limit = pixels
            .checked_mul(4)
            .and_then(|bytes| bytes.checked_add(tiles * 384 + 4096))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or_else(|| error("The VNC ZRLE rectangle is too large."))?;
        let stream = self
            .stream
            .get_or_insert_with(|| flate2::Decompress::new(true));
        let mut output = Vec::with_capacity(output_limit);
        let input_before = stream.total_in();
        let status = stream
            .decompress_vec(compressed, &mut output, flate2::FlushDecompress::Sync)
            .map_err(|_| error("The VNC server sent invalid ZRLE data."))?;
        if stream.total_in() - input_before != compressed.len() as u64 {
            return Err(error(
                "The VNC server sent truncated or oversized ZRLE data.",
            ));
        }
        if output.len() == output_limit && status == flate2::Status::BufError {
            return Err(error(
                "The VNC ZRLE rectangle exceeds the decoded data limit.",
            ));
        }
        decode_zrle_tiles(&output, width, height, compact_pixels)
    }
}

fn decode_zrle_tiles(
    data: &[u8],
    width: usize,
    height: usize,
    compact_pixels: bool,
) -> io::Result<Vec<u8>> {
    let area = width
        .checked_mul(height)
        .filter(|area| *area <= 16 * 1024 * 1024)
        .ok_or_else(|| error("The VNC ZRLE rectangle exceeds the pixel limit."))?;
    let mut output = vec![0; area * 4];
    let mut input = data;
    for tile_y in (0..height).step_by(64) {
        for tile_x in (0..width).step_by(64) {
            let tile_width = (width - tile_x).min(64);
            let tile_height = (height - tile_y).min(64);
            let subencoding = take_byte(&mut input)?;
            let tile_area = tile_width * tile_height;
            let mut tile = Vec::with_capacity(tile_area);
            match subencoding {
                0 => {
                    for _ in 0..tile_area {
                        tile.push(take_pixel(&mut input, compact_pixels)?);
                    }
                }
                1 => {
                    let pixel = take_pixel(&mut input, compact_pixels)?;
                    tile.resize(tile_area, pixel);
                }
                2..=16 => {
                    let palette_size = usize::from(subencoding);
                    let palette = take_palette(&mut input, palette_size, compact_pixels)?;
                    let bits = if palette_size <= 2 {
                        1
                    } else if palette_size <= 4 {
                        2
                    } else {
                        4
                    };
                    for _ in 0..tile_height {
                        let row_bytes = (tile_width * bits).div_ceil(8);
                        let row = take_bytes(&mut input, row_bytes)?;
                        for x in 0..tile_width {
                            let bit = x * bits;
                            let index = (row[bit / 8] >> (8 - bits - bit % 8)) & ((1 << bits) - 1);
                            tile.push(*palette.get(usize::from(index)).ok_or_else(|| {
                                error("The VNC ZRLE tile references a missing palette color.")
                            })?);
                        }
                    }
                }
                128 => {
                    while tile.len() < tile_area {
                        let pixel = take_pixel(&mut input, compact_pixels)?;
                        let run = take_run_length(&mut input)?;
                        append_run(&mut tile, pixel, run, tile_area)?;
                    }
                }
                129..=255 => {
                    let palette =
                        take_palette(&mut input, usize::from(subencoding - 128), compact_pixels)?;
                    while tile.len() < tile_area {
                        let encoded = take_byte(&mut input)?;
                        let index = usize::from(encoded & 0x7f);
                        let pixel = *palette.get(index).ok_or_else(|| {
                            error("The VNC ZRLE tile references a missing palette color.")
                        })?;
                        let run = if encoded & 0x80 != 0 {
                            take_run_length(&mut input)?
                        } else {
                            1
                        };
                        append_run(&mut tile, pixel, run, tile_area)?;
                    }
                }
                _ => return Err(error("The VNC server sent an unsupported ZRLE tile.")),
            }
            if tile.len() != tile_area {
                return Err(error("The VNC ZRLE tile has an invalid pixel count."));
            }
            for (index, pixel) in tile.into_iter().enumerate() {
                let x = tile_x + index % tile_width;
                let y = tile_y + index / tile_width;
                output[(y * width + x) * 4..][..4].copy_from_slice(&pixel);
            }
        }
    }
    if !input.is_empty() {
        return Err(error("The VNC ZRLE rectangle contains trailing data."));
    }
    Ok(output)
}

fn take_byte(input: &mut &[u8]) -> io::Result<u8> {
    let (&byte, rest) = input
        .split_first()
        .ok_or_else(|| error("The VNC server sent truncated ZRLE data."))?;
    *input = rest;
    Ok(byte)
}

fn take_bytes<'a>(input: &mut &'a [u8], count: usize) -> io::Result<&'a [u8]> {
    if input.len() < count {
        return Err(error("The VNC server sent truncated ZRLE data."));
    }
    let (bytes, rest) = input.split_at(count);
    *input = rest;
    Ok(bytes)
}

fn take_pixel(input: &mut &[u8], compact_pixels: bool) -> io::Result<[u8; 4]> {
    if compact_pixels {
        let pixel = take_bytes(input, 2)?;
        let value = u16::from_le_bytes([pixel[0], pixel[1]]);
        let [red, green, blue] = [
            (value >> 11) as u8,
            ((value >> 5) & 63) as u8,
            (value & 31) as u8,
        ];
        Ok([
            (red << 3) | (red >> 2),
            (green << 2) | (green >> 4),
            (blue << 3) | (blue >> 2),
            255,
        ])
    } else {
        let pixel = take_bytes(input, 3)?;
        Ok([pixel[0], pixel[1], pixel[2], 255])
    }
}

fn take_palette(input: &mut &[u8], size: usize, compact_pixels: bool) -> io::Result<Vec<[u8; 4]>> {
    (0..size)
        .map(|_| take_pixel(input, compact_pixels))
        .collect()
}

fn take_run_length(input: &mut &[u8]) -> io::Result<usize> {
    let mut length = 1usize;
    loop {
        let byte = usize::from(take_byte(input)?);
        length = length
            .checked_add(byte)
            .ok_or_else(|| error("The VNC ZRLE run is too large."))?;
        if byte != 255 {
            return Ok(length);
        }
    }
}

fn append_run(
    tile: &mut Vec<[u8; 4]>,
    pixel: [u8; 4],
    length: usize,
    limit: usize,
) -> io::Result<()> {
    if length == 0 || length > limit.saturating_sub(tile.len()) {
        return Err(error("The VNC ZRLE run exceeds its tile."));
    }
    tile.resize(tile.len() + length, pixel);
    Ok(())
}

fn expand_rgb565(pixels: &mut [u8]) {
    // Expand backwards so unread packed pixels stay intact.
    for index in (0..pixels.len() / 4).rev() {
        let value = u16::from_le_bytes([pixels[index * 2], pixels[index * 2 + 1]]);
        let red = (value >> 11) as u8;
        let green = ((value >> 5) & 63) as u8;
        let blue = (value & 31) as u8;
        pixels[index * 4..][..4].copy_from_slice(&[
            (red << 3) | (red >> 2),
            (green << 2) | (green >> 4),
            (blue << 3) | (blue >> 2),
            255,
        ]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn expands_packed_vnc_pixels_in_place() {
        let mut pixels = [0x00, 0xf8, 0xe0, 0x07, 0x1f, 0x00, 0, 0, 0, 0, 0, 0];
        expand_rgb565(&mut pixels);
        assert_eq!(pixels, [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn decodes_rich_cursor_mask_and_rgb_pixels() {
        let mut wire = &[255, 0, 0, 0, 0, 255, 0, 0, 0b1000_0000][..];
        let image = read_cursor(&mut wire, 2, 1, false).unwrap();
        assert_eq!(image.pixels[0], egui::Color32::RED);
        assert_eq!(image.pixels[1].a(), 0);
    }

    #[test]
    fn rejects_invalid_classic_password_lengths_before_reading_the_server() {
        let mut stream = io::Cursor::new(Vec::<u8>::new());
        assert!(
            handshake(&mut stream, true, None, Some(b"123456789"), false)
                .unwrap_err()
                .to_string()
                .contains("8 bytes")
        );
        assert!(handshake(&mut stream, true, None, Some(b""), false)
            .unwrap_err()
            .to_string()
            .contains("empty"));
    }

    #[test]
    fn decodes_x_cursor_colors_and_transparency_mask() {
        let mut wire = &[255, 0, 0, 0, 0, 255, 0b1000_0000, 0b1000_0000][..];
        let image = read_x_cursor(&mut wire, 2, 1).unwrap();
        assert_eq!(image.pixels[0], egui::Color32::RED);
        assert_eq!(image.pixels[1].a(), 0);
    }

    #[test]
    fn automatic_vnc_quality_honors_manual_pixel_depth() {
        assert!(use_rgb565(Quality::Auto, 16));
        assert!(!use_rgb565(Quality::Auto, 24));
        assert!(!use_rgb565(Quality::Auto, 32));
        assert!(use_rgb565(Quality::Fast, 32));
        assert!(!use_rgb565(Quality::Good, 16));
        assert!(!use_rgb565(Quality::Best, 16));
    }

    #[test]
    fn tight_quality_maps_presets_to_server_hints_only_when_enabled() {
        assert!(tight_quality_encodings(Quality::Auto, true).is_empty());
        assert!(tight_quality_encodings(Quality::Best, false).is_empty());
        assert_eq!(
            tight_quality_encodings(Quality::Fast, true),
            vec![COMPRESS_LEVEL_0 + 1, QUALITY_LEVEL_0 + 3]
        );
        assert_eq!(
            tight_quality_encodings(Quality::Good, true),
            vec![COMPRESS_LEVEL_0 + 5, QUALITY_LEVEL_0 + 6]
        );
        assert_eq!(
            tight_quality_encodings(Quality::Best, true),
            vec![COMPRESS_LEVEL_0 + 9, QUALITY_LEVEL_0 + 9]
        );
    }

    #[test]
    fn decodes_zrle_raw_solid_palette_and_rle_tiles() {
        let raw = decode_zrle_tiles(&[0, 255, 0, 0, 0, 255, 0], 2, 1, false).unwrap();
        assert_eq!(&raw, &[255, 0, 0, 255, 0, 255, 0, 255]);

        let solid = decode_zrle_tiles(&[1, 0, 0, 255], 2, 1, false).unwrap();
        assert_eq!(&solid, &[0, 0, 255, 255, 0, 0, 255, 255]);

        let packed =
            decode_zrle_tiles(&[2, 255, 0, 0, 0, 255, 0, 0b0100_0000], 2, 1, false).unwrap();
        assert_eq!(&packed, &[255, 0, 0, 255, 0, 255, 0, 255]);

        let rle = decode_zrle_tiles(&[128, 255, 0, 0, 1], 2, 1, false).unwrap();
        assert_eq!(&rle, &[255, 0, 0, 255, 255, 0, 0, 255]);

        let palette_rle = decode_zrle_tiles(&[129, 0, 0, 255, 0x80, 1], 2, 1, false).unwrap();
        assert_eq!(&palette_rle, &[0, 0, 255, 255, 0, 0, 255, 255]);

        let rgb565 = decode_zrle_tiles(&[0, 0x00, 0xf8], 1, 1, true).unwrap();
        assert_eq!(&rgb565, &[255, 0, 0, 255]);
    }

    #[test]
    fn rejects_zrle_tile_overruns_and_truncated_pixels() {
        assert!(decode_zrle_tiles(&[128, 0, 0, 0, 255, 255], 1, 1, false).is_err());
        assert!(decode_zrle_tiles(&[0, 0], 1, 1, false).is_err());
    }

    #[test]
    fn decodes_tight_fill_copy_palette_and_gradient() {
        let mut decoder = TightDecoder::default();
        let fill = decoder
            .decode(&mut &[0x80, 10, 20, 30][..], 2, 1, false)
            .unwrap();
        assert_eq!(fill, [10, 20, 30, 255, 10, 20, 30, 255]);

        let copy = decoder
            .decode(&mut &[0x00, 10, 20, 30][..], 1, 1, false)
            .unwrap();
        assert_eq!(copy, [10, 20, 30, 255]);

        let palette = decoder
            .decode(
                &mut &[0x40, 1, 1, 255, 0, 0, 0, 255, 0, 0b0100_0000][..],
                2,
                1,
                false,
            )
            .unwrap();
        assert_eq!(palette, [255, 0, 0, 255, 0, 255, 0, 255]);

        let gradient = decoder
            .decode(&mut &[0x40, 2, 10, 20, 30, 1, 2, 3][..], 2, 1, false)
            .unwrap();
        assert_eq!(gradient, [10, 20, 30, 255, 11, 22, 33, 255]);
    }

    #[test]
    fn decodes_tight_compressed_blocks_and_resets_streams() {
        use flate2::{write::ZlibEncoder, Compression};

        let first = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120];
        let second = [120, 110, 100, 90, 80, 70, 60, 50, 40, 30, 20, 10];
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&first).unwrap();
        encoder.flush().unwrap();
        let first_block = std::mem::take(encoder.get_mut());
        encoder.write_all(&second).unwrap();
        encoder.flush().unwrap();
        let second_block = std::mem::take(encoder.get_mut());

        let mut decoder = TightDecoder::default();
        let mut first_wire = vec![0x01]; // Reset Tight stream zero, then use copy filter.
        first_wire.extend_from_slice(&encode_tight_length(first_block.len()));
        first_wire.extend_from_slice(&first_block);
        let decoded = decoder
            .decode(&mut first_wire.as_slice(), 4, 1, false)
            .unwrap();
        assert_eq!(
            decoded,
            [10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,]
        );

        let mut second_wire = vec![0x00]; // Reuse the persistent zlib stream.
        second_wire.extend_from_slice(&encode_tight_length(second_block.len()));
        second_wire.extend_from_slice(&second_block);
        assert_eq!(
            decoder
                .decode(&mut second_wire.as_slice(), 4, 1, false)
                .unwrap(),
            [120, 110, 100, 255, 90, 80, 70, 255, 60, 50, 40, 255, 30, 20, 10, 255]
        );
    }

    #[test]
    fn decodes_tight_jpeg_and_rejects_bad_dimensions() {
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut encoded)
            .encode(&[255, 0, 0], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        let mut wire = vec![0x90];
        wire.extend_from_slice(&encode_tight_length(encoded.len()));
        wire.extend_from_slice(&encoded);

        let mut decoder = TightDecoder::default();
        let decoded = decoder.decode(&mut wire.as_slice(), 1, 1, false).unwrap();
        assert_eq!(decoded.len(), 4);
        assert_eq!(decoded[3], 255);

        assert!(decoder.decode(&mut wire.as_slice(), 2, 1, false).is_err());
    }

    #[test]
    fn decodes_tight_png() {
        use image::ImageEncoder;

        let mut encoded = Vec::new();
        image::codecs::png::PngEncoder::new(&mut encoded)
            .write_image(&[12, 34, 56, 255], 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        let mut wire = vec![0xa0];
        wire.extend_from_slice(&encode_tight_length(encoded.len()));
        wire.extend_from_slice(&encoded);

        assert_eq!(
            TightDecoder::default()
                .decode(&mut wire.as_slice(), 1, 1, false)
                .unwrap(),
            [12, 34, 56, 255]
        );
    }

    fn encode_tight_length(length: usize) -> Vec<u8> {
        let mut length = length;
        let mut result = Vec::new();
        loop {
            let mut byte = (length & 0x7f) as u8;
            length >>= 7;
            if length != 0 {
                byte |= 0x80;
            }
            result.push(byte);
            if length == 0 {
                return result;
            }
        }
    }

    #[test]
    fn writes_classic_clipboard_message() {
        let mut wire = Vec::new();
        assert!(Input::ClipboardText("aé".into()).write(&mut wire).unwrap());
        assert_eq!(wire, [6, 0, 0, 0, 0, 0, 0, 2, b'a', 0xe9]);
    }

    #[test]
    fn writes_unicode_text_as_x11_keysyms() {
        let mut wire = Vec::new();
        assert!(!Input::UnicodeText('é').write(&mut wire).unwrap());
        assert_eq!(wire, [4, 1, 0, 0, 0, 0, 0, 0xe9, 4, 0, 0, 0, 0, 0, 0, 0xe9]);
        let mut unicode_wire = Vec::new();
        assert!(!Input::UnicodeText('界').write(&mut unicode_wire).unwrap());
        assert_eq!(
            unicode_wire,
            [4, 1, 0, 0, 0x01, 0x00, 0x75, 0x4c, 4, 0, 0, 0, 0x01, 0x00, 0x75, 0x4c,]
        );
        assert!(clipboard_unsupported_reason(&Input::ClipboardText("界".into())).is_some());
    }

    #[test]
    fn identifies_classic_clipboard_format_limits() {
        assert_eq!(
            clipboard_unsupported_reason(&Input::ClipboardText("hello é".into())),
            None
        );
        assert!(clipboard_unsupported_reason(&Input::ClipboardText("hello ✓".into())).is_some());
        assert!(clipboard_unsupported_reason(&Input::ClipboardImage(
            super::super::clipboard_image::ClipboardImage {
                width: 1,
                height: 1,
                rgba: vec![0; 4],
            }
        ))
        .is_some());
    }

    #[test]
    fn writes_pointer_motion_with_current_button_mask() {
        let mut wire = Vec::new();
        assert!(!Input::PointerMove {
            x: 0x1234,
            y: 0x5678,
            buttons: 1,
        }
        .write(&mut wire)
        .unwrap());
        assert_eq!(wire, [5, 1, 0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn full_input_queue_drops_only_pointer_motion() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let overflow = AtomicBool::new(false);
        sender
            .try_send(Input::Key {
                keysym: 0xff0d,
                physical_key: None,
                pressed: true,
            })
            .unwrap();
        enqueue_input(
            &sender,
            &overflow,
            Input::PointerMove {
                x: 10,
                y: 20,
                buttons: 0,
            },
        );
        assert!(!overflow.load(Ordering::Acquire));
        enqueue_input(
            &sender,
            &overflow,
            Input::Pointer {
                x: 10,
                y: 20,
                buttons: 1,
            },
        );
        assert!(overflow.load(Ordering::Acquire));
        assert!(receiver.try_recv().is_ok());
    }

    #[test]
    fn ignores_extra_pointer_buttons_in_classic_rfb() {
        let mut wire = Vec::new();
        assert!(!Input::PointerExtra {
            x: 1,
            y: 2,
            button: 1,
            pressed: true,
        }
        .write(&mut wire)
        .unwrap());
        assert!(wire.is_empty());
    }

    #[test]
    fn reconnect_backoff_grows_and_adds_bounded_jitter() {
        assert_eq!(reconnect_delay(0, 0), Duration::from_secs(1));
        assert_eq!(reconnect_delay(4, 4_000), Duration::from_secs(20));
    }

    #[test]
    fn allows_up_to_five_retries_for_initial_failures() {
        assert!(should_retry(true, 0));
        assert!(should_retry(true, 4));
        assert!(!should_retry(true, 5));
        assert!(!should_retry(false, 0));
    }

    #[test]
    fn authentication_failures_do_not_retry_automatically() {
        assert!(!should_retry_after(
            true,
            0,
            Some(io::ErrorKind::PermissionDenied)
        ));
        assert!(should_retry_after(true, 0, Some(io::ErrorKind::TimedOut)));
    }

    #[test]
    fn counts_only_received_bytes() {
        let received_bytes = Arc::new(AtomicU64::new(0));
        let mut reader = CountReads {
            stream: &b"frame"[..],
            received_bytes: received_bytes.clone(),
        };
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"frame");
        assert_eq!(received_bytes.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn discard_leaves_following_message_unread() {
        let mut stream = &b"skipnext"[..];
        discard_clipboard(&mut stream, 4).unwrap();
        assert_eq!(stream, b"next");
        assert!(discard_clipboard(&mut stream, 5).is_err());
    }

    struct Transcript {
        incoming: io::Cursor<Vec<u8>>,
        outgoing: Vec<u8>,
    }

    impl Read for Transcript {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.incoming.read(buffer)
        }
    }

    impl Write for Transcript {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.outgoing.write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn resize_message_preserves_screen_identity_and_flags() {
        let mut screen = [0; 16];
        screen[3] = 7;
        screen[15] = 9;
        let mut wire = Vec::new();
        Input::VncResize {
            size: [1280, 720],
            screen,
        }
        .write(&mut wire)
        .unwrap();
        assert_eq!(
            wire,
            [251, 0, 5, 0, 2, 208, 1, 0, 0, 0, 0, 7, 0, 0, 0, 0, 5, 0, 2, 208, 0, 0, 0, 9]
        );
    }

    #[test]
    fn direct_vnc_connects_to_a_reachable_local_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || listener.accept().unwrap());
        let host: SshRemoteHost =
            serde_json::from_str(r#"{"id":"test","name":"Test","host":"127.0.0.1"}"#).unwrap();
        let stream = open_direct(&host, port, 1).unwrap();
        assert_eq!(stream.peer_addr().unwrap().port(), port);
        server.join().unwrap();
    }

    #[test]
    fn classic_auth_uses_only_the_first_eight_password_bytes() {
        let challenge = [0x42; 16];
        assert_eq!(
            *response(b"abcdefgh-extra", challenge),
            *response(b"abcdefgh", challenge)
        );
    }

    #[test]
    fn configured_password_never_accepts_none_security() {
        let mut peer = Transcript {
            incoming: io::Cursor::new(b"RFB 003.008\n\x01\x01".to_vec()),
            outgoing: Vec::new(),
        };
        assert!(handshake(&mut peer, true, None, Some(b"test"), false).is_err());
        assert_eq!(peer.outgoing, b"RFB 003.008\n");
    }

    #[test]
    fn only_password_security_offers_request_a_password() {
        let mut password_offer = Transcript {
            incoming: io::Cursor::new(b"RFB 003.008\n\x01\x02".to_vec()),
            outgoing: Vec::new(),
        };
        assert_eq!(
            handshake(&mut password_offer, true, None, None, false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );

        let mut unsupported_offer = Transcript {
            incoming: io::Cursor::new(b"RFB 003.008\n\x01\x13".to_vec()),
            outgoing: Vec::new(),
        };
        assert_eq!(
            handshake(&mut unsupported_offer, true, None, None, false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::Other
        );
    }

    #[test]
    fn password_handshake_sends_response_then_client_init() {
        let mut input = b"RFB 003.008\n\x02\x01\x02".to_vec();
        input.extend_from_slice(&[0; 16]);
        input.extend_from_slice(&[0; 4]);
        input.extend_from_slice(&[0, 16, 0, 8]);
        input.extend_from_slice(&[0; 20]);
        let mut peer = Transcript {
            incoming: io::Cursor::new(input),
            outgoing: Vec::new(),
        };
        assert_eq!(
            handshake(&mut peer, true, None, Some(b"test"), false).unwrap(),
            [16, 8]
        );
        assert_eq!(&peer.outgoing[..13], b"RFB 003.008\n\x02");
        assert_eq!(&peer.outgoing[13..29], &*response(b"test", [0; 16]));
        assert_eq!(&peer.outgoing[29..], &[1]);
    }

    #[test]
    fn vencrypt_plain_username_auth_is_only_allowed_inside_the_ssh_tunnel() {
        let mut direct = Transcript {
            incoming: io::Cursor::new(Vec::new()),
            outgoing: Vec::new(),
        };
        assert_eq!(
            handshake(&mut direct, true, Some(b"alice"), Some(b"secret"), false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(direct.outgoing.is_empty());

        let mut input = b"RFB 003.008\n\x01\x13".to_vec();
        input.extend_from_slice(&[0, 2]); // VeNCrypt 0.2
        input.extend_from_slice(&[0, 2]); // Success, two subtypes follow
        input.extend_from_slice(&[0x00, 0x00, 0x01, 0x03]); // VeNCrypt TLSPlain
        input.extend_from_slice(&[0x00, 0x00, 0x01, 0x00]); // VeNCrypt Plain
        input.extend_from_slice(&[0; 4]); // SecurityResult
        input.extend_from_slice(&[0, 16, 0, 8]);
        input.extend_from_slice(&[0; 20]);
        let mut peer = Transcript {
            incoming: io::Cursor::new(input),
            outgoing: Vec::new(),
        };

        assert_eq!(
            handshake(
                &mut peer,
                true,
                Some(b"alice"),
                Some(b"correct horse"),
                true,
            )
            .unwrap(),
            [16, 8]
        );
        let mut expected = b"RFB 003.008\n".to_vec();
        expected.extend_from_slice(&[19, 0, 2]);
        expected.extend_from_slice(&256u32.to_be_bytes());
        expected.extend_from_slice(&5u32.to_be_bytes());
        expected.extend_from_slice(&13u32.to_be_bytes());
        expected.extend_from_slice(b"alice");
        expected.extend_from_slice(b"correct horse");
        expected.push(1);
        assert_eq!(peer.outgoing, expected);
    }

    #[test]
    fn vnc_response_reverses_the_key_bits_and_encrypts_both_blocks() {
        let key: [u8; 8] = [0x13, 0x34, 0x57, 0x79, 0x9b, 0xbc, 0xdf, 0xf1];
        let password = key.map(u8::reverse_bits);
        let plain = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
        let challenge = std::array::from_fn(|i| plain[i % 8]);
        let encrypted = [0x85, 0xe8, 0x13, 0x54, 0x0f, 0x0a, 0xb4, 0x05];
        assert_eq!(&response(&password, challenge)[..8], &encrypted);
        assert_eq!(&response(&password, challenge)[8..], &encrypted);
    }
}
