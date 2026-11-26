use super::{
    credentials::{self, DesktopProtocol},
    framebuffer::{Framebuffer, Region},
    profile::{Clipboard, Quality},
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
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub(crate) const CLIPBOARD_LIMIT: usize = 1024 * 1024;

const RAW: i32 = 0;
const COPY_RECT: i32 = 1;
const DESKTOP_SIZE: i32 = -223;
const EXTENDED_DESKTOP_SIZE: i32 = -308;

const TIMEOUT: Duration = Duration::from_secs(8);
const TUNNEL_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Input {
    Key { keysym: u32, pressed: bool },
    Pointer { x: u16, y: u16, buttons: u8 },
    ClipboardText(Vec<u8>),
    VncResize { size: [u16; 2], screen: [u8; 16] },
}

impl Input {
    fn write(self, stream: &mut impl Write) -> io::Result<()> {
        match self {
            Self::ClipboardText(text) => {
                let [a, b, c, d] = (text.len() as u32).to_be_bytes();
                stream.write_all(&[6, 0, 0, 0, a, b, c, d])?;
                stream.write_all(&text)
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
                stream.write_all(&message)
            }
            Self::Key { keysym, pressed } => {
                let [a, b, c, d] = keysym.to_be_bytes();
                stream.write_all(&[4, u8::from(pressed), 0, 0, a, b, c, d])
            }
            Self::Pointer { x, y, buttons } => {
                let [a, b] = x.to_be_bytes();
                let [c, d] = y.to_be_bytes();
                stream.write_all(&[5, buttons, a, b, c, d])
            }
        }
    }
}

pub(crate) struct Connection {
    pub framebuffer: Framebuffer,
    pub status: String,
    pub connected: bool,
    pub(crate) connected_since: Option<Instant>,
    pub(crate) finished: bool,
    pub(crate) reconnect_at: Option<Instant>,
    pub(crate) receiving_frame: bool,
    pub(crate) clipboard_text: Option<String>,
    pub(crate) vnc_screen: Option<[u8; 16]>,
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
            connected_since: None,
            finished: false,
            reconnect_at: None,
            receiving_frame: false,
            clipboard_text: None,
            vnc_screen: None,
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
                state.clipboard_text = None;
                state.vnc_screen = None;
                state.resize_request = None;
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
            if matches!(input.try_send(event), Err(mpsc::TrySendError::Full(_))) {
                // Disconnect rather than lose key releases when the network is stalled.
                self.input_overflow.store(true, Ordering::Release);
            }
        }
    }
}

fn reconnect_delay(attempt: u32, entropy: u64) -> Duration {
    let base_ms = 1_000_u64 << attempt;
    Duration::from_millis(base_ms + entropy % (base_ms / 4 + 1))
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
        Box::new(open_direct(host, profile.port)?)
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
    let compact_pixels = matches!(profile.quality, Quality::Auto | Quality::Fast);
    // RGB565 halves the raw payload. Good and Best retain full color.
    let pixel_format = if compact_pixels {
        [16, 16, 0, 1, 0, 31, 0, 63, 0, 31, 11, 5, 0, 0, 0, 0]
    } else {
        [32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 0, 8, 16, 0, 0, 0]
    };
    let mut format_message = [0; 20];
    format_message[4..].copy_from_slice(&pixel_format);
    stream.write_all(&format_message)?;
    let mut encodings = vec![2, 0, 0, 4];
    for encoding in [COPY_RECT, RAW, EXTENDED_DESKTOP_SIZE, DESKTOP_SIZE] {
        encodings.extend_from_slice(&encoding.to_be_bytes());
    }
    stream.write_all(&encodings)?;
    let writer = Arc::new(Mutex::new(stream.input_socket()?));
    if !profile.view_only {
        let (pending, overflow) = state.lock().unwrap().open_input();
        let writer = writer.clone();
        std::thread::spawn(move || {
            while let Ok(event) = pending.recv() {
                let mut writer = writer.lock().unwrap();
                if overflow.load(Ordering::Acquire) || event.write(&mut *writer).is_err() {
                    writer.disconnect();
                    break;
                }
            }
        });
    }
    ctx.request_repaint();
    request(&mut *writer.lock().unwrap(), size, false)?;

    let mut stream = BufReader::with_capacity(64 * 1024, stream);
    let mut pixels = Vec::new();
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
                state.lock().unwrap().receiving_frame = true;
                let mut resized = false;
                for index in 0..count {
                    if let Some(new_size) = read_rectangle(
                        &mut stream,
                        size,
                        state,
                        &mut pixels,
                        index + 1 == count,
                        compact_pixels,
                    )? {
                        size = new_size;
                        resized = true;
                    }
                }
                state.lock().unwrap().receiving_frame = false;
                ctx.request_repaint();
                request(&mut *writer.lock().unwrap(), size, !resized)?;
            }
            2 => {}
            3 => {
                let mut header = [0; 7];
                stream.read_exact(&mut header)?;
                let size = u32::from_be_bytes(header[3..7].try_into().unwrap());
                if size as usize > CLIPBOARD_LIMIT {
                    return Err(error("The VNC clipboard message exceeds the viewer limit."));
                }
                let mut text = vec![0; size as usize];
                stream.read_exact(&mut text)?;
                if matches!(
                    profile.clipboard,
                    Clipboard::Both | Clipboard::RemoteToLocal
                ) {
                    state.lock().unwrap().clipboard_text =
                        Some(text.into_iter().map(char::from).collect());
                    ctx.request_repaint();
                }
            }
            _ => return Err(error("The VNC server sent an unsupported message.")),
        }
    }
}

fn open_direct(host: &SshRemoteHost, port: u16) -> io::Result<TcpStream> {
    let address = host.host.trim().trim_matches(['[', ']']);
    let addresses = (address, port)
        .to_socket_addrs()
        .map_err(|_| error("Could not resolve the VNC host."))?;
    let mut private_addresses = addresses
        .filter(|addr| private(addr.ip()))
        .take(3)
        .peekable();
    if private_addresses.peek().is_none() {
        return Err(error(
            "Direct VNC requires a private address. Enable Tunnel via SSH in the VNC tab.",
        ));
    }
    let mut last_error = None;
    let stream = private_addresses
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
    Ok(stream)
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
        _ => {
            return Err(error(
                "The VNC server sent an unsupported framebuffer encoding.",
            ))
        }
    }
    Ok(None)
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

    #[test]
    fn expands_packed_vnc_pixels_in_place() {
        let mut pixels = [0x00, 0xf8, 0xe0, 0x07, 0x1f, 0x00, 0, 0, 0, 0, 0, 0];
        expand_rgb565(&mut pixels);
        assert_eq!(pixels, [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn writes_classic_clipboard_message() {
        let mut wire = Vec::new();
        Input::ClipboardText(vec![b'a', 0xe9])
            .write(&mut wire)
            .unwrap();
        assert_eq!(wire, [6, 0, 0, 0, 0, 0, 0, 2, b'a', 0xe9]);
    }

    #[test]
    fn reconnect_backoff_grows_and_adds_bounded_jitter() {
        assert_eq!(reconnect_delay(0, 0), Duration::from_secs(1));
        assert_eq!(reconnect_delay(4, 4_000), Duration::from_secs(20));
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
    fn direct_vnc_explains_when_ssh_tunnel_is_required() {
        let host: SshRemoteHost =
            serde_json::from_str(r#"{"id":"test","name":"Test","host":"203.0.113.10"}"#).unwrap();
        let error = open_direct(&host, 5900).unwrap_err();
        assert!(error.to_string().contains("Enable Tunnel via SSH"));
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
