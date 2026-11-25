use super::framebuffer::{Framebuffer, Region};
use crate::config::SshRemoteHost;
use std::{
    io::{self, Read, Write},
    net::{IpAddr, TcpStream, ToSocketAddrs},
    sync::{Arc, Mutex},
    time::Duration,
};

const RAW: i32 = 0;
const COPY_RECT: i32 = 1;
const DESKTOP_SIZE: i32 = -223;

const TIMEOUT: Duration = Duration::from_secs(8);

pub(crate) struct Connection {
    pub framebuffer: Framebuffer,
    pub status: String,
    pub connected: bool,
}

impl Connection {
    pub(crate) fn start(host: SshRemoteHost, ctx: egui::Context) -> Arc<Mutex<Self>> {
        let state = Arc::new(Mutex::new(Self {
            framebuffer: Framebuffer::default(),
            status: "Connecting to VNC…".into(),
            connected: false,
        }));
        let worker = state.clone();
        std::thread::spawn(move || {
            let result = run(&host, &worker, &ctx);
            if let Err(error) = result {
                let mut state = worker.lock().unwrap();
                state.connected = false;
                state.status = error.to_string();
                ctx.request_repaint();
            }
        });
        state
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

fn run(host: &SshRemoteHost, state: &Mutex<Connection>, ctx: &egui::Context) -> io::Result<()> {
    let profile = host
        .vnc
        .as_ref()
        .filter(|p| p.enabled)
        .ok_or_else(|| error("VNC is disabled for this host."))?;
    if profile.tunnel_via_ssh {
        return Err(error("VNC through an SSH tunnel is not available yet."));
    }
    if profile.credential_ref.is_some() || !profile.username.trim().is_empty() {
        return Err(error(
            "VNC authentication is not available yet. No password was sent.",
        ));
    }
    let address = host.host.trim().trim_matches(['[', ']']);
    let addresses = (address, profile.port)
        .to_socket_addrs()
        .map_err(|_| error("Could not resolve the VNC host."))?;
    let mut stream = addresses
        .filter(|addr| private(addr.ip()))
        .take(3)
        .find_map(|addr| TcpStream::connect_timeout(&addr, TIMEOUT).ok())
        .ok_or_else(|| {
            error("Could not connect to VNC on a private address. Check the server and port.")
        })?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let mut size = handshake(&mut stream, profile.shared)?;
    {
        let mut state = state.lock().unwrap();
        state.framebuffer.resize(size).map_err(error)?;
        state.status = "Connected".into();
        state.connected = true;
    }
    // Request little-endian 32-bit pixels with R, G and B in the first three bytes.
    stream.write_all(&[
        0, 0, 0, 0, 32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 0, 8, 16, 0, 0, 0,
    ])?;
    let mut encodings = vec![2, 0, 0, 3];
    for encoding in [COPY_RECT, RAW, DESKTOP_SIZE] {
        encodings.extend_from_slice(&encoding.to_be_bytes());
    }
    stream.write_all(&encodings)?;
    request(&mut stream, size, false)?;
    ctx.request_repaint();

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
                let mut resized = false;
                for index in 0..count {
                    if let Some(new_size) =
                        read_rectangle(&mut stream, size, state, &mut pixels, index + 1 == count)?
                    {
                        size = new_size;
                        resized = true;
                    }
                }
                ctx.request_repaint();
                request(&mut stream, size, !resized)?;
            }
            2 => {}
            3 => {
                let mut header = [0; 7];
                stream.read_exact(&mut header)?;
                let size = u32::from_be_bytes(header[3..7].try_into().unwrap());
                if size > 1024 * 1024 {
                    return Err(error("The VNC clipboard message exceeds the viewer limit."));
                }
                io::copy(
                    &mut Read::by_ref(&mut stream).take(u64::from(size)),
                    &mut io::sink(),
                )?;
            }
            _ => return Err(error("The VNC server sent an unsupported message.")),
        }
    }
}

fn handshake(stream: &mut (impl Read + Write), shared: bool) -> io::Result<[usize; 2]> {
    let mut version = [0; 12];
    stream.read_exact(&mut version)?;
    if !matches!(
        &version,
        b"RFB 003.003\n" | b"RFB 003.007\n" | b"RFB 003.008\n"
    ) {
        return Err(error("The server did not offer a supported VNC version."));
    }
    stream.write_all(&version)?;
    if &version == b"RFB 003.003\n" {
        let mut kind = [0; 4];
        stream.read_exact(&mut kind)?;
        if u32::from_be_bytes(kind) != 1 {
            return Err(error(
                "This VNC server requires authentication that the viewer does not support yet.",
            ));
        }
    } else {
        let mut count = [0];
        stream.read_exact(&mut count)?;
        let mut offered = [0; 255];
        let offered = &mut offered[..usize::from(count[0])];
        stream.read_exact(offered)?;
        if !offered.contains(&1) {
            return Err(error(
                "This VNC server requires authentication that the viewer does not support yet.",
            ));
        }
        if offered.contains(&2) {
            return Err(error("The VNC server offered password authentication. This viewer will not fall back to an unauthenticated connection."));
        }
        stream.write_all(&[1])?;
        if &version == b"RFB 003.008\n" {
            let mut result = [0; 4];
            stream.read_exact(&mut result)?;
            if result != [0; 4] {
                return Err(error("The VNC server rejected the connection."));
            }
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
) -> io::Result<Option<[usize; 2]>> {
    let mut rect = [0; 12];
    stream.read_exact(&mut rect)?;
    let field = |i| usize::from(u16::from_be_bytes([rect[i], rect[i + 1]]));
    let [x, y, w, h] = [field(0), field(2), field(4), field(6)];
    let encoding = i32::from_be_bytes([rect[8], rect[9], rect[10], rect[11]]);
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
            pixels.resize(w * h * 4, 0);
            stream.read_exact(pixels)?;
            state
                .lock()
                .unwrap()
                .framebuffer
                .write_rgba(region, pixels)
                .map_err(error)?;
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
