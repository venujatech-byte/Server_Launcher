use crate::config::SshRemoteHost;
use std::{
    io::{self, Read, Write},
    time::Duration,
};

#[cfg(unix)]
use std::{
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    process::{Child, Command, Stdio},
};

#[cfg(unix)]
pub(crate) struct Tunnel {
    stream: UnixStream,
    child: Child,
}

#[cfg(unix)]
impl Tunnel {
    pub(crate) fn open(host: &SshRemoteHost, port: u16, timeout: Duration) -> io::Result<Self> {
        let password = host
            .credential_ref
            .as_deref()
            .map(|reference| {
                crate::ssh_credentials::load(reference, &crate::ssh_credentials::binding(host))
            })
            .transpose()
            .map_err(io::Error::other)?
            .map(zeroize::Zeroizing::new);
        let password_reader = if let Some(password) = &password {
            if !crate::remote::is_sshpass_available() {
                return Err(io::Error::other(
                    "Install sshpass to use the saved SSH password for this tunnel.",
                ));
            }
            if password.len() > 4096 || password.contains(['\n', '\r', '\0']) {
                return Err(io::Error::other(
                    "The saved SSH password cannot be passed to sshpass.",
                ));
            }
            let (reader, mut writer) = UnixStream::pair()?;
            writer.write_all(password.as_bytes())?;
            writer.write_all(b"\n")?;
            Some(reader)
        } else {
            None
        };
        let password_fd = password_reader.as_ref().map(AsRawFd::as_raw_fd);
        let mut command = if let Some(fd) = password_fd {
            let mut command = Command::new("sshpass");
            command.arg("-d").arg(fd.to_string()).arg("ssh");
            command
        } else {
            let mut command = Command::new("ssh");
            command.args(["-o", "BatchMode=yes"]);
            command
        };
        let (stream, ssh_stream) = UnixStream::pair()?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        command
            .args(crate::remote::ssh_policy_args(host))
            .args(["-o", "ConnectTimeout=5", "-o", "ExitOnForwardFailure=yes"])
            .args([
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=2",
            ])
            .args(["-o", "ControlPath=none", "-o", "ForkAfterAuthentication=no"])
            .args([
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "PermitLocalCommand=no",
            ])
            .args(["-o", "ForwardAgent=no", "-x", "-T"])
            .arg("-p")
            .arg(host.port.to_string());
        if !host.key_path.trim().is_empty() {
            command.arg("-i").arg(host.key_path.trim());
        }
        let target = format!("{}@{}", host.user.trim(), host.host.trim());
        command
            .arg("-W")
            .arg(format!("127.0.0.1:{port}"))
            .arg("--")
            .arg(target)
            .stdin(Stdio::from(OwnedFd::from(ssh_stream.try_clone()?)))
            .stdout(Stdio::from(OwnedFd::from(ssh_stream)))
            .stderr(Stdio::null())
            .process_group(0);

        // The password descriptor is inherited only by this SSH child tree.
        #[cfg(target_os = "linux")]
        let parent = std::process::id();
        unsafe {
            command.pre_exec(move || {
                #[cfg(target_os = "linux")]
                {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::getppid() as u32 != parent {
                        return Err(io::Error::from(io::ErrorKind::Interrupted));
                    }
                }
                if let Some(fd) = password_fd {
                    let flags = libc::fcntl(fd, libc::F_GETFD);
                    if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let child = command.spawn().map_err(|error| {
            io::Error::other(format!("Could not start the SSH tunnel: {error}"))
        })?;
        drop(password_reader);
        Ok(Self { stream, child })
    }
}

#[cfg(unix)]
impl Read for Tunnel {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.stream.read(bytes)
    }
}

#[cfg(unix)]
impl Write for Tunnel {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

#[cfg(unix)]
impl Drop for Tunnel {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            // The unreaped group leader still owns this process group, including sshpass children.
            unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
        }
        let _ = self.child.wait();
    }
}

pub(crate) trait InputSocket: Write + Send {
    fn disconnect(&self);
}

pub(crate) trait DesktopStream: Read + Write + Sync {
    fn input_socket(&self) -> io::Result<Box<dyn InputSocket>>;
}

impl InputSocket for std::net::TcpStream {
    fn disconnect(&self) {
        let _ = self.shutdown(std::net::Shutdown::Both);
    }
}

impl DesktopStream for std::net::TcpStream {
    fn input_socket(&self) -> io::Result<Box<dyn InputSocket>> {
        Ok(Box::new(self.try_clone()?))
    }
}

#[cfg(unix)]
impl InputSocket for UnixStream {
    fn disconnect(&self) {
        let _ = self.shutdown(std::net::Shutdown::Both);
    }
}

#[cfg(unix)]
impl DesktopStream for Tunnel {
    fn input_socket(&self) -> io::Result<Box<dyn InputSocket>> {
        Ok(Box::new(self.stream.try_clone()?))
    }
}

pub(crate) fn open(
    host: &SshRemoteHost,
    port: u16,
    timeout: Duration,
) -> io::Result<Box<dyn DesktopStream>> {
    #[cfg(unix)]
    {
        Tunnel::open(host, port, timeout).map(|stream| Box::new(stream) as Box<dyn DesktopStream>)
    }
    #[cfg(not(unix))]
    {
        let _ = (host, port, timeout);
        Err(io::Error::other(
            "SSH desktop tunnels are not available on this platform yet.",
        ))
    }
}
