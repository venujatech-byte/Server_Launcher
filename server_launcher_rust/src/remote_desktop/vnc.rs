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
const ZRLE: i32 = 16;
const DESKTOP_SIZE: i32 = -223;
const EXTENDED_DESKTOP_SIZE: i32 = -308;
const MAX_ZRLE_COMPRESSED: usize = 64 * 1024 * 1024;

const TIMEOUT: Duration = Duration::from_secs(8);
const TUNNEL_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Input {
    Key {
        keysym: u32,
        physical_key: Option<egui::Key>,
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
    ClipboardText(String),
    ClipboardImage(super::clipboard_image::ClipboardImage),
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
            Self::ClipboardImage(_) => Ok(false),
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

pub(crate) struct Connection {
    pub framebuffer: Framebuffer,
    pub status: String,
    pub connected: bool,
    pub(crate) display_updates: u64,
    pub(crate) received_bytes: Arc<AtomicU64>,
    pub(crate) connected_since: Option<Instant>,
    pub(crate) finished: bool,
    pub(crate) reconnect_at: Option<Instant>,
    pub(crate) receiving_frame: bool,
    pub(crate) clipboard_too_large: bool,
    pub(crate) clipboard_sent: Arc<AtomicBool>,
    pub(crate) clipboard_unsupported: Arc<AtomicBool>,
    pub(crate) clipboard_text: Option<String>,
    pub(crate) clipboard_image: Option<super::clipboard_image::ClipboardImage>,
    pub(crate) vnc_screen: Option<[u8; 16]>,
    pub(crate) rdp_resize_supported: bool,
    pub(crate) rdp_resize_enabled: bool,
    pub(crate) resize_request: Option<[u32; 2]>,
    pub(crate) trust_prompt: Option<super::trust::Prompt>,
    input: Option<mpsc::SyncSender<Input>>,
    input_overflow: Arc<AtomicBool>,
}

impl Connection {
    pub(crate) fn start(host: SshRemoteHost, ctx: egui::Context) -> Arc<Mutex<Self>> {
        let reconnect = host
            .vnc
            .as_ref()
            .is_some_and(|profile| profile.auto_reconnect);
        Self::start_worker("Connecting to VNC…", reconnect, ctx, move |state, ctx| {
            run(&host, state, ctx)
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
            connected_since: None,
            finished: false,
            reconnect_at: None,
            receiving_frame: false,
            clipboard_too_large: false,
            clipboard_sent: Arc::new(AtomicBool::new(false)),
            clipboard_unsupported: Arc::new(AtomicBool::new(false)),
            clipboard_text: None,
            clipboard_image: None,
            vnc_screen: None,
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
                let should_retry =
                    auto_reconnect && (state.connected || retries > 0) && retries < 5;
                state.connected = false;
                state.input = None;
                state.clipboard_sent = Arc::new(AtomicBool::new(false));
                state.clipboard_unsupported = Arc::new(AtomicBool::new(false));
                state.clipboard_text = None;
                state.clipboard_image = None;
                state.clipboard_too_large = false;
                state.vnc_screen = None;
                state.resize_request = None;
                state.rdp_resize_supported = false;
                state.status = result
                    .err()
                    .map_or_else(|| "Session ended".into(), |error| error.to_string());
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
                state.framebuffer = Framebuffer::default();
                state.received_bytes.store(0, Ordering::Relaxed);
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
}

fn reconnect_delay(attempt: u32, entropy: u64) -> Duration {
    let base_ms = 1_000_u64 << attempt;
    Duration::from_millis(base_ms + entropy % (base_ms / 4 + 1))
}

fn use_rgb565(quality: Quality, color_depth: u8) -> bool {
    match quality {
        Quality::Fast => true,
        Quality::Auto => color_depth <= 16,
        Quality::Best | Quality::Good => false,
    }
}

#[cfg(test)]
mod warning_tests {
    use super::needs_unencrypted_warning;

    #[test]
    fn warns_for_public_and_unresolved_vnc_hosts() {
        assert!(needs_unencrypted_warning("203.0.113.10"));
        assert!(needs_unencrypted_warning("vnc.example.test"));
        assert!(!needs_unencrypted_warning("192.168.1.12"));
    }
}

fn error(message: &str) -> io::Error {
    io::Error::other(message)
}

fn private(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
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

fn run(host: &SshRemoteHost, state: &Mutex<Connection>, ctx: &egui::Context) -> io::Result<()> {
    let profile = host
        .vnc
        .as_ref()
        .filter(|p| p.enabled)
        .ok_or_else(|| error("VNC is disabled for this host."))?;
    if !profile.username.trim().is_empty() {
        return Err(error(
            "This VNC viewer supports classic password authentication without a username.",
        ));
    }
    let password =
        credentials::load(host, DesktopProtocol::Vnc).map_err(|message| error(&message))?;
    if password.as_ref().is_some_and(|value| value.is_empty()) {
        return Err(error(
            "The saved VNC password is empty. Update it in the VNC tab.",
        ));
    }
    let mut stream: Box<dyn DesktopStream> = if profile.tunnel_via_ssh {
        tunnel::open(host, profile.port, TUNNEL_HANDSHAKE_TIMEOUT)?
    } else {
        Box::new(open_direct(
            host,
            profile.port,
            profile.tcp_user_timeout_secs,
        )?)
    };
    let mut size = handshake(
        &mut stream,
        profile.shared,
        password.as_deref().map(String::as_bytes),
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
    let mut enabled_encodings = Vec::with_capacity(5);
    for encoding in &profile.encodings {
        let encoding = match encoding {
            Encoding::Zrle => ZRLE,
            Encoding::CopyRect => COPY_RECT,
            Encoding::Raw => RAW,
            Encoding::Tight => continue,
        };
        if !enabled_encodings.contains(&encoding) {
            enabled_encodings.push(encoding);
        }
    }
    if !enabled_encodings
        .iter()
        .any(|encoding| matches!(*encoding, ZRLE | COPY_RECT | RAW))
    {
        enabled_encodings.push(RAW);
    }
    for encoding in [EXTENDED_DESKTOP_SIZE, DESKTOP_SIZE] {
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
        std::thread::spawn(move || {
            while let Ok(event) = pending.recv() {
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
                    state.lock().unwrap().clipboard_text =
                        Some(text.into_iter().map(char::from).collect());
                    ctx.request_repaint();
                }
            }
            _ => return Err(error("The VNC server sent an unsupported message.")),
        }
    }
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

fn open_direct(
    host: &SshRemoteHost,
    port: u16,
    user_timeout_secs: u32,
) -> io::Result<TcpStream> {
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
        io::Error::new(io::ErrorKind::InvalidInput, "TCP user timeout is too large.")
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
    password: Option<&[u8]>,
) -> io::Result<[usize; 2]> {
    let mut version = [0; 12];
    stream.read_exact(&mut version)?;
    if !matches!(
        &version,
        b"RFB 003.003\n" | b"RFB 003.007\n" | b"RFB 003.008\n"
    ) {
        return Err(error("The server did not offer a supported VNC version."));
    }
    stream.write_all(&version)?;
    let preferred = if password.is_some() { 2 } else { 1 };
    let compatible = if &version == b"RFB 003.003\n" {
        let mut kind = [0; 4];
        stream.read_exact(&mut kind)?;
        u32::from_be_bytes(kind) == u32::from(preferred)
    } else {
        let mut count = [0];
        stream.read_exact(&mut count)?;
        let mut offered = [0; 255];
        let offered = &mut offered[..usize::from(count[0])];
        stream.read_exact(offered)?;
        if offered.contains(&preferred) && (password.is_some() || !offered.contains(&2)) {
            stream.write_all(&[preferred])?;
            true
        } else {
            false
        }
    };
    if !compatible {
        return Err(error(if password.is_some() {
            "The VNC server did not offer classic password authentication."
        } else {
            "The VNC server requires a password or does not offer a supported security mode."
        }));
    }
    if let Some(password) = password {
        let mut challenge = [0; 16];
        stream.read_exact(&mut challenge)?;
        let response = response(password, challenge);
        stream.write_all(&*response)?;
    }
    if password.is_some() || &version == b"RFB 003.008\n" {
        let mut result = [0; 4];
        stream.read_exact(&mut result)?;
        if result != [0; 4] {
            return Err(error(
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
) -> io::Result<Option<[usize; 2]>> {
    let mut rect = [0; 12];
    stream.read_exact(&mut rect)?;
    let field = |i| usize::from(u16::from_be_bytes([rect[i], rect[i + 1]]));
    let [x, y, w, h] = [field(0), field(2), field(4), field(6)];
    let encoding = i32::from_be_bytes([rect[8], rect[9], rect[10], rect[11]]);
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
        _ => {
            return Err(error(
                "The VNC server sent an unsupported framebuffer encoding.",
            ))
        }
    }
    Ok(None)
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
    fn automatic_vnc_quality_honors_manual_pixel_depth() {
        assert!(use_rgb565(Quality::Auto, 16));
        assert!(!use_rgb565(Quality::Auto, 24));
        assert!(!use_rgb565(Quality::Auto, 32));
        assert!(use_rgb565(Quality::Fast, 32));
        assert!(!use_rgb565(Quality::Good, 16));
        assert!(!use_rgb565(Quality::Best, 16));
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
    fn writes_classic_clipboard_message() {
        let mut wire = Vec::new();
        assert!(Input::ClipboardText("aé".into()).write(&mut wire).unwrap());
        assert_eq!(wire, [6, 0, 0, 0, 0, 0, 0, 2, b'a', 0xe9]);
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
        assert!(handshake(&mut peer, true, Some(b"test")).is_err());
        assert_eq!(peer.outgoing, b"RFB 003.008\n");
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
        assert_eq!(handshake(&mut peer, true, Some(b"test")).unwrap(), [16, 8]);
        assert_eq!(&peer.outgoing[..13], b"RFB 003.008\n\x02");
        assert_eq!(&peer.outgoing[13..29], &*response(b"test", [0; 16]));
        assert_eq!(&peer.outgoing[29..], &[1]);
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
