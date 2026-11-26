use crate::config::SshRemoteHost;
use std::{
    io::{self, Read, Write},
    path::Path,
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

#[cfg(windows)]
use std::{
    net::{TcpListener, TcpStream},
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command, Stdio},
};

#[cfg(unix)]
pub(crate) struct Tunnel {
    stream: UnixStream,
    child: Child,
}

#[cfg(unix)]
impl Tunnel {
    pub(crate) fn open(
        host: &SshRemoteHost,
        port: u16,
        timeout: Duration,
        _config_path: Option<&Path>,
    ) -> io::Result<Self> {
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

#[cfg(windows)]
pub(crate) struct Tunnel {
    stream: TcpStream,
    child: Child,
    _job: KillOnCloseJob,
}

#[cfg(windows)]
struct KillOnCloseJob(isize);

#[cfg(windows)]
impl Drop for KillOnCloseJob {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0 as _) };
    }
}

#[cfg(windows)]
impl Tunnel {
    pub(crate) fn open(
        host: &SshRemoteHost,
        port: u16,
        timeout: Duration,
        config_path: Option<&Path>,
    ) -> io::Result<Self> {
        let password_auth = host.credential_ref.is_some() || !host.password.trim().is_empty();
        if password_auth && config_path.is_none() {
            return Err(io::Error::other(
                "A saved configuration is required to unlock the SSH password for this tunnel.",
            ));
        }
        if host.key_path.trim().is_empty() && !host.use_agent && !password_auth {
            return Err(io::Error::other(
                "Configure an SSH key, agent, or saved SSH password before opening this tunnel.",
            ));
        }

        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let local_addr = listener.local_addr()?;

        let forward = format!("127.0.0.1:{}:127.0.0.1:{port}", local_addr.port());
        let target = format!("{}@{}", host.user.trim(), host.host.trim());
        let mut command = Command::new("ssh.exe");
        command
            .args(["-F", "none"])
            .args(["-o", if password_auth { "BatchMode=no" } else { "BatchMode=yes" }])
            .args(crate::remote::ssh_policy_args(host))
            .args([
                "-o",
                "ConnectTimeout=5",
                "-o",
                "ExitOnForwardFailure=yes",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=2",
                "-o",
                "PermitLocalCommand=no",
                "-o",
                "ForwardAgent=no",
                "-x",
                "-T",
                "-N",
            ])
            .arg("-L")
            .arg(forward)
            .arg("-p")
            .arg(host.port.to_string());
        if !host.key_path.trim().is_empty() {
            command.arg("-i").arg(host.key_path.trim());
        }
        drop(listener);
        let mut child = command
            .arg("--")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|error| {
                io::Error::other(format!("Could not start the SSH tunnel: {error}"))
            })?;
        let job = match kill_on_close_job(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };

        let deadline = std::time::Instant::now() + timeout;
        loop {
            if child.try_wait()?.is_some() {
                return Err(io::Error::other(
                    "The SSH tunnel exited before connecting. Check SSH key or agent access and the remote desktop port.",
                ));
            }
            if let Ok(stream) = TcpStream::connect_timeout(&local_addr, Duration::from_millis(100))
            {
                if child.try_wait()?.is_some() {
                    return Err(io::Error::other("The SSH tunnel closed during connection."));
                }
                stream.set_nodelay(true)?;
                stream.set_read_timeout(Some(timeout))?;
                stream.set_write_timeout(Some(timeout))?;
                return Ok(Self {
                    stream,
                    child,
                    _job: job,
                });
            }
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Timed out opening the SSH tunnel. Check SSH key or agent access and the remote desktop port.",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(windows)]
fn kill_on_close_job(child: &Child) -> io::Result<KillOnCloseJob> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
    };

    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    };
    let assigned = configured != 0
        && unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) } != 0;
    if !assigned {
        let error = io::Error::last_os_error();
        unsafe { CloseHandle(job) };
        return Err(io::Error::other(format!(
            "Could not safely attach the SSH tunnel to its cleanup job: {error}"
        )));
    }
    Ok(KillOnCloseJob(job as isize))
}

pub(crate) trait InputSocket: Write + Send {
    fn disconnect(&self);
}

pub(crate) trait DesktopStream: Read + Write + Sync {
    fn input_socket(&self) -> io::Result<Box<dyn InputSocket>>;
    fn set_read_timeout(&self, timeout: Duration) -> io::Result<()>;
}

impl InputSocket for std::net::TcpStream {
    fn disconnect(&self) {
        let _ = self.shutdown(std::net::Shutdown::Both);
    }
}

impl DesktopStream for std::net::TcpStream {
    fn set_read_timeout(&self, timeout: Duration) -> io::Result<()> {
        std::net::TcpStream::set_read_timeout(self, Some(timeout))
    }

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
    fn set_read_timeout(&self, timeout: Duration) -> io::Result<()> {
        self.stream.set_read_timeout(Some(timeout))
    }

    fn input_socket(&self) -> io::Result<Box<dyn InputSocket>> {
        Ok(Box::new(self.stream.try_clone()?))
    }
}

#[cfg(windows)]
impl Read for Tunnel {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.stream.read(bytes)
    }
}

#[cfg(windows)]
impl Write for Tunnel {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

#[cfg(windows)]
impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(windows)]
impl InputSocket for Tunnel {
    fn disconnect(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

#[cfg(windows)]
impl DesktopStream for Tunnel {
    fn set_read_timeout(&self, timeout: Duration) -> io::Result<()> {
        self.stream.set_read_timeout(Some(timeout))
    }

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
    #[cfg(windows)]
    {
        Tunnel::open(host, port, timeout).map(|stream| Box::new(stream) as Box<dyn DesktopStream>)
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        let _ = (host, port, timeout);
        Err(io::Error::other(
            "SSH desktop tunnels are not available on this platform yet.",
        ))
    }
}
