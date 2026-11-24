use crate::config::ServerConfig;
use chrono::Local;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use sysinfo::{Pid, ProcessesToUpdate, System};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Errored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogKind {
    Launcher,
    Stdout,
    Stderr,
    Stdin,
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub text: String,
    pub timestamp: String,
    pub kind: LogKind,
}

impl LogEntry {
    pub fn new(text: String, kind: LogKind) -> Self {
        let timestamp = Local::now().format("%H:%M:%S").to_string();
        Self {
            text,
            timestamp,
            kind,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ProcessIdentity {
    key: String,
    command: String,
    cwd: String,
    pid: u32,
    start_time: u64,
}

impl ProcessIdentity {
    fn matches(&self, config: &ServerConfig, pid: u32, start_time: u64) -> bool {
        self.pid == pid && pid > 0 && self.start_time > 0 && self.start_time == start_time
            && self.key == config.key && self.command == config.command && self.cwd == config.cwd
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum ProcessRecord {
    Identity(ProcessIdentity),
    Legacy(u32),
}

impl ProcessRecord {
    fn pid(&self) -> u32 {
        match self { Self::Identity(identity) => identity.pid, Self::Legacy(pid) => *pid }
    }
}

fn pid_file(config: &ServerConfig) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("launcher_{}.pid", config.key))
}

fn read_process_record(config: &ServerConfig) -> Option<ProcessRecord> {
    serde_json::from_str(&std::fs::read_to_string(pid_file(config)).ok()?).ok()
}

fn write_process_record(config: &ServerConfig, pid: u32) -> std::io::Result<()> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), sysinfo::ProcessRefreshKind::new());
    let process = sys.process(Pid::from_u32(pid)).filter(|p| p.start_time() > 0)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Process identity unavailable"))?;
    let identity = ProcessIdentity {
        key: config.key.clone(), command: config.command.clone(), cwd: config.cwd.clone(),
        pid, start_time: process.start_time(),
    };
    write_identity(config, &identity)
}

fn write_identity(config: &ServerConfig, identity: &ProcessIdentity) -> std::io::Result<()> {
    std::fs::write(pid_file(config), serde_json::to_vec(identity)?)
}

// sysinfo 0.31 computes Linux CPU percentages only during an All refresh.
// Keep this scheduled: extra refreshes reset the per-process CPU time baselines.
pub fn refresh_process_metrics(sys: &mut System) {
    let refresh = sysinfo::ProcessRefreshKind::new().with_cpu();
    #[cfg(not(target_os = "linux"))]
    let refresh = refresh.with_memory();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, refresh);
}

fn process_tree_pids(root: Pid, parents: impl IntoIterator<Item = (Pid, Option<Pid>)>) -> Vec<Pid> {
    let parents: Vec<_> = parents.into_iter().collect();
    let mut tree = vec![root];
    let mut index = 0;
    while index < tree.len() {
        for &(pid, parent) in &parents {
            if parent == Some(tree[index]) && !tree.contains(&pid) { tree.push(pid); }
        }
        index += 1;
    }
    tree
}

fn refresh_process_alive(pid: u32) -> bool {
    // Keep liveness probes separate so they cannot overwrite the CPU sampling baseline.
    let mut sys = System::new();
    // Targeted sysinfo refreshes retain dead entries, so presence in the cache is not enough.
    let updated = sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), sysinfo::ProcessRefreshKind::new());
    updated > 0 && sys.process(Pid::from_u32(pid)).is_some_and(|p| p.status() != sysinfo::ProcessStatus::Zombie)
}

fn legacy_process_matches(config: &ServerConfig, command: &[std::ffi::OsString], cwd: Option<&std::path::Path>) -> bool {
    if config.command.trim().is_empty() { return false; }
    let expected_cwd = std::fs::canonicalize(&config.cwd).ok();
    let actual_cwd = cwd.and_then(|path| std::fs::canonicalize(path).ok());
    if expected_cwd.is_none() || expected_cwd != actual_cwd { return false; }
    let args: Vec<_> = command.iter().map(|arg| arg.to_string_lossy()).collect();
    let shell_command = args.windows(2).any(|pair| {
        (pair[0] == "-c" || pair[0].eq_ignore_ascii_case("/C") || pair[0].eq_ignore_ascii_case("/K"))
            && pair[1] == config.command
    });
    let wrapper = std::env::temp_dir().join(format!("launcher_{}.sh", config.key));
    shell_command || args.join(" ") == config.command
        || (config.own_console && command.iter().any(|arg| std::path::Path::new(arg) == wrapper))
}

// A port describes availability, never which config owns a process.
// Reject ambiguous legacy records rather than assigning the same PID to multiple configs.
pub fn reconnect_services(services: &mut [Service]) {
    let records: Vec<_> = services.iter().map(|service| {
        if service.state == ServiceState::Stopped { read_process_record(&service.config) } else { None }
    }).collect();
    let pids: Vec<_> = records.iter().flatten().map(ProcessRecord::pid).filter(|&pid| pid > 0).map(Pid::from_u32).collect();
    if pids.is_empty() { return; }
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), sysinfo::ProcessRefreshKind::new()
        .with_cmd(sysinfo::UpdateKind::Always).with_cwd(sysinfo::UpdateKind::Always));
    let candidates: Vec<_> = records.iter().zip(services.iter()).map(|(record, service)| {
        let record = record.as_ref()?;
        let process = sys.process(Pid::from_u32(record.pid()))?;
        if process.status() == sysinfo::ProcessStatus::Zombie { return None; }
        let matches = match record {
            ProcessRecord::Identity(identity) => identity.matches(&service.config, record.pid(), process.start_time()),
            ProcessRecord::Legacy(_) => legacy_process_matches(&service.config, process.cmd(), process.cwd()),
        };
        matches.then_some((record.pid(), process.start_time()))
    }).collect();
    let claimed: std::collections::HashSet<_> = services.iter().filter_map(|service| service.pid).collect();
    for (service, candidate) in services.iter_mut().zip(candidates.iter()) {
        if let Some((pid, start_time)) = candidate {
            if !claimed.contains(pid) && candidates.iter().filter(|candidate| candidate.is_some_and(|(other, _)| other == *pid)).count() == 1 {
                service.attach_existing(*pid, *start_time);
            } else {
                service.append_log(format!("[launcher] Cannot reconnect: PID {} is claimed by multiple server configs. Start this server explicitly to record its identity.", pid), LogKind::Warning);
            }
        }
    }
}

pub struct Service {
    pub config: ServerConfig,
    pub state: ServiceState,
    pub pid: Option<u32>,
    pub port_open: bool,
    pub cpu_usage: f32,
    pub memory_mb: f32,
    pub started_at: Option<std::time::Instant>,

    pub logs: Arc<Mutex<VecDeque<LogEntry>>>,
    stdin_writer: Option<Arc<Mutex<Box<dyn Write + Send>>>>,
    child_handle: Option<Child>,

    max_logs: usize,

    pub restart_at: Option<std::time::Instant>,
    pub crash_timestamps: Vec<std::time::Instant>,
    // Rate-limit TCP port probes to every 5 seconds
    last_port_check: std::time::Instant,
}

impl Service {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config,
            state: ServiceState::Stopped,
            pid: None,
            port_open: false,
            cpu_usage: 0.0,
            memory_mb: 0.0,
            started_at: None,
            logs: Arc::new(Mutex::new(VecDeque::new())),
            stdin_writer: None,
            child_handle: None,
            max_logs: 2000,
            restart_at: None,
            crash_timestamps: Vec::new(),
            // Initialize far in the past so first poll immediately checks the port
            last_port_check: std::time::Instant::now() - std::time::Duration::from_secs(10),
        }
    }

    pub fn append_log(&self, text: String, kind: LogKind) {
        if text.contains('\n') || text.contains('\r') {
            for line in text.split(|c| c == '\n' || c == '\r') {
                let trimmed = line.trim_end();
                if !trimmed.is_empty() {
                    let entry = LogEntry::new(trimmed.to_string(), kind.clone());
                    if let Ok(mut logs) = self.logs.lock() {
                        if logs.len() >= self.max_logs {
                            logs.pop_front();
                        }
                        logs.push_back(entry);
                    }
                }
            }
        } else {
            let entry = LogEntry::new(text, kind);
            if let Ok(mut logs) = self.logs.lock() {
                if logs.len() >= self.max_logs {
                    logs.pop_front();
                }
                logs.push_back(entry);
            }
        }
    }

    pub fn clear_logs(&self) {
        if let Ok(mut logs) = self.logs.lock() {
            logs.clear();
        }
    }
}

enum SpawnedIo {
    #[cfg(unix)]
    Pty {
        reader: std::fs::File,
        writer: std::fs::File,
    },
    Piped {
        stdin: Option<ChildStdin>,
        stdout: Option<std::process::ChildStdout>,
        stderr: Option<std::process::ChildStderr>,
    },
}

#[cfg(unix)]
fn spawn_child(mut cmd: Command) -> std::io::Result<(Child, SpawnedIo)> {
    use std::os::unix::io::FromRawFd;

    let mut master = 0;
    let mut slave = 0;
    let res = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    if res == 0 {
        let ws = libc::winsize {
            ws_row: 24,
            ws_col: 120,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        unsafe { libc::ioctl(master, libc::TIOCSWINSZ, &ws) };

        let mut termios: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(slave, &mut termios) } == 0 {
            termios.c_lflag &= !libc::ECHO;
            unsafe { libc::tcsetattr(slave, libc::TCSANOW, &termios) };
        }

        let slave_in = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
        let slave_out = unsafe { Stdio::from_raw_fd(libc::dup(slave)) };
        let slave_err = unsafe { Stdio::from_raw_fd(slave) };

        cmd.stdin(slave_in);
        cmd.stdout(slave_out);
        cmd.stderr(slave_err);

        match cmd.spawn() {
            Ok(child) => {
                let master_write_fd = unsafe { libc::dup(master) };
                let master_writer = unsafe { std::fs::File::from_raw_fd(master_write_fd) };
                let master_reader = unsafe { std::fs::File::from_raw_fd(master) };
                return Ok((
                    child,
                    SpawnedIo::Pty {
                        reader: master_reader,
                        writer: master_writer,
                    },
                ));
            }
            Err(e) => {
                unsafe { libc::close(master); }
                return Err(e);
            }
        }
    }

    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    Ok((child, SpawnedIo::Piped { stdin, stdout, stderr }))
}

#[cfg(not(unix))]
fn spawn_child(mut cmd: Command) -> std::io::Result<(Child, SpawnedIo)> {
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    Ok((child, SpawnedIo::Piped { stdin, stdout, stderr }))
}

impl Service {
    pub fn start(&mut self) {
        self.restart_at = None;
        if self.state == ServiceState::Running || self.state == ServiceState::Starting {
            return;
        }

        self.append_log(
            format!("[launcher] Starting service: {} ...", self.config.name),
            LogKind::Launcher,
        );
        self.state = ServiceState::Starting;
        let _ = std::fs::remove_file(pid_file(&self.config));

        let cmd_str = self.config.command.clone();
        let cwd_str = self.config.cwd.clone();
        let env_vars = self.config.env.clone();

        // 1. If own_console is enabled, launch in terminal window
        if self.config.own_console {
            self.append_log(
                "[launcher] Launching in external console window...".to_string(),
                LogKind::Launcher,
            );
            match spawn_own_console(
                &self.config.key,
                &self.config.name,
                &cmd_str,
                &cwd_str,
                &env_vars,
            ) {
                Ok(child) => {
                    let pid = child.id();
                    self.pid = Some(pid);
                    self.started_at = Some(std::time::Instant::now());
                    self.state = ServiceState::Running;
                    self.append_log(
                        format!("[launcher] Running in console window (PID: {})", pid),
                        LogKind::Launcher,
                    );
                    #[cfg(target_os = "windows")]
                    self.save_process_identity(pid);
                    self.child_handle = Some(child);
                    return;
                }
                Err(err) => {
                    self.append_log(
                        format!(
                            "[launcher] Failed to open external console ({}), falling back to embedded mode",
                            err
                        ),
                        LogKind::Warning,
                    );
                }
            }
        }

        let mut cmd;
        #[cfg(target_os = "windows")]
        {
            cmd = Command::new("cmd.exe");
            cmd.args(&["/C", &cmd_str]);
        }

        #[cfg(not(target_os = "windows"))]
        {
            cmd = Command::new("sh");
            cmd.args(&["-c", &cmd_str]);
            // On Unix systems, set process group to easily terminate all child processes
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                cmd.process_group(0);
            }
        }

        if !cwd_str.is_empty() && cwd_str != "." {
            cmd.current_dir(&cwd_str);
        }

        prepare_environment(&mut cmd, &cwd_str, &env_vars);

        match spawn_child(cmd) {
            Ok((child, io)) => {
                let pid = child.id();
                self.pid = Some(pid);
                self.started_at = Some(std::time::Instant::now());
                self.state = ServiceState::Running;

                self.save_process_identity(pid);

                self.append_log(
                    format!("[launcher] Process spawned (PID: {})", pid),
                    LogKind::Launcher,
                );

                match io {
                    #[cfg(unix)]
                    SpawnedIo::Pty { reader, writer } => {
                        self.stdin_writer = Some(Arc::new(Mutex::new(Box::new(writer))));
                        spawn_stream_reader(
                            reader,
                            Arc::clone(&self.logs),
                            self.max_logs,
                            LogKind::Stdout,
                        );
                    }
                    SpawnedIo::Piped { stdin, stdout, stderr } => {
                        if let Some(sin) = stdin {
                            self.stdin_writer = Some(Arc::new(Mutex::new(Box::new(sin))));
                        }
                        if let Some(sout) = stdout {
                            spawn_stream_reader(
                                sout,
                                Arc::clone(&self.logs),
                                self.max_logs,
                                LogKind::Stdout,
                            );
                        }
                        if let Some(serr) = stderr {
                            spawn_stream_reader(
                                serr,
                                Arc::clone(&self.logs),
                                self.max_logs,
                                LogKind::Stderr,
                            );
                        }
                    }
                }

                self.child_handle = Some(child);
            }
            Err(err) => {
                self.state = ServiceState::Errored;
                self.append_log(
                    format!("[launcher] Failed to start service: {}", err),
                    LogKind::Error,
                );
            }
        }
    }

    fn save_process_identity(&self, pid: u32) {
        if let Err(error) = write_process_record(&self.config, pid) {
            self.append_log(format!("[launcher] Could not save process identity: {}", error), LogKind::Warning);
        }
    }

    fn attach_existing(&mut self, pid: u32, start_time: u64) {
        if self.state == ServiceState::Running {
            return;
        }

        self.pid = Some(pid);
        self.state = ServiceState::Running;
        self.started_at = Some(std::time::Instant::now());
        self.port_open = self.config.port > 0;
        self.restart_at = None;
        // Allow port check to run soon after reconnect
        self.last_port_check = std::time::Instant::now() - std::time::Duration::from_secs(10);

        // Preserve the start time we verified; a fresh lookup could observe a reused PID.
        let identity = ProcessIdentity {
            key: self.config.key.clone(), command: self.config.command.clone(), cwd: self.config.cwd.clone(),
            pid, start_time,
        };
        if let Err(error) = write_identity(&self.config, &identity) {
            self.append_log(format!("[launcher] Could not save process identity: {}", error), LogKind::Warning);
        }
        let reason = "verified process identity from previous session";

        self.append_log(
            format!(
                "[launcher] Reconnected to previously running process (PID: {}, {}).",
                pid, reason
            ),
            LogKind::Launcher,
        );
        self.append_log(
            "[launcher] Terminal session re-attached. Controls and inputs are ready.".to_string(),
            LogKind::Launcher,
        );

        #[cfg(target_os = "linux")]
        {
            let logs_clone = Arc::clone(&self.logs);
            let max_logs = self.max_logs;
            thread::spawn(move || {
                if let Ok(output) = Command::new("journalctl")
                    .args(["-q", &format!("_PID={}", pid), "-n", "25", "--no-pager"])
                    .output()
                {
                    let out = String::from_utf8_lossy(&output.stdout);
                    for line in out.lines() {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            push_log_line(trimmed, &logs_clone, max_logs, LogKind::Stdout);
                        }
                    }
                }
            });
        }
    }

    pub fn stop(&mut self) {
        self.restart_at = None;
        if self.state == ServiceState::Stopped {
            return;
        }

        self.state = ServiceState::Stopping;
        self.append_log(
            format!("[launcher] Stopping service: {} ...", self.config.name),
            LogKind::Launcher,
        );

        if !self.config.stop_command.trim().is_empty() {
            let stop_cmd = self.config.stop_command.clone();
            let cwd = self.config.cwd.clone();
            thread::spawn(move || {
                #[cfg(target_os = "windows")]
                let _ = Command::new("cmd.exe")
                    .args(&["/C", &stop_cmd])
                    .current_dir(cwd)
                    .output();

                #[cfg(not(target_os = "windows"))]
                let _ = Command::new("sh")
                    .args(&["-c", &stop_cmd])
                    .current_dir(cwd)
                    .output();
            });
        }

        let pid_file = std::env::temp_dir().join(format!("launcher_{}.pid", self.config.key));
        let _ = std::fs::remove_file(&pid_file);

        if let Some(pid) = self.pid {
            #[cfg(target_os = "windows")]
            {
                // Kill process tree cleanly on Windows
                let _ = Command::new("taskkill")
                    .args(&["/F", "/T", "/PID", &pid.to_string()])
                    .output();
            }

            #[cfg(not(target_os = "windows"))]
            {
                #[cfg(unix)]
                unsafe {
                    // Try graceful SIGTERM to process group, then direct process
                    libc::kill(-(pid as i32), libc::SIGTERM);
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
        }

        if let Some(mut child) = self.child_handle.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        self.pid = None;
        self.stdin_writer = None;
        self.started_at = None;
        self.state = ServiceState::Stopped;
        self.cpu_usage = 0.0;
        self.memory_mb = 0.0;
        self.append_log("[launcher] Service stopped.".to_string(), LogKind::Launcher);
    }

    pub fn restart(&mut self) {
        self.stop();
        self.start();
    }

    pub fn uptime_formatted(&self) -> String {
        if let Some(start) = self.started_at {
            let secs = start.elapsed().as_secs();
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            if h > 0 {
                format!("{}h {:02}m", h, m)
            } else if m > 0 {
                format!("{}m {:02}s", m, s)
            } else {
                format!("{}s", s)
            }
        } else {
            String::new()
        }
    }

    pub fn is_active(&self) -> bool {
        self.state == ServiceState::Running
            || self.state == ServiceState::Starting
            || self.stdin_writer.is_some()
            || self.child_handle.is_some()
            || self.pid.is_some()
    }

    pub fn send_input(&self, input_text: &str) {
        if let Some(stdin_mutex) = &self.stdin_writer {
            if let Ok(mut writer) = stdin_mutex.lock() {
                let _ = writeln!(writer, "{}", input_text);
                let _ = writer.flush();
                self.append_log(format!("> {}", input_text), LogKind::Stdin);
                return;
            }
        }

        #[cfg(target_os = "linux")]
        if let Some(pid) = self.pid {
            let stdin_path = format!("/proc/{}/fd/0", pid);
            if let Ok(mut file) = std::fs::OpenOptions::new().write(true).open(&stdin_path) {
                let _ = writeln!(file, "{}", input_text);
                let _ = file.flush();
                self.append_log(format!("> {}", input_text), LogKind::Stdin);
                return;
            }
        }

        self.append_log(
            format!("[warning] Cannot send '{}': server is not running or stdin is closed.", input_text),
            LogKind::Warning,
        );
    }

    pub fn send_interrupt(&self) {
        if let Some(pid) = self.pid {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(pid as i32), libc::SIGINT);
            }
            #[cfg(target_os = "windows")]
            {
                // On Windows fallback to sending empty line or stopping
                self.send_input("\x03");
            }
            self.append_log("[launcher] Sent interrupt (SIGINT)".to_string(), LogKind::Launcher);
        }
    }

    pub fn open_console(&self) {
        let name = self.config.name.clone();
        let pid = self.pid;
        let port = self.config.port;
        let cwd = self.config.cwd.clone();
        let cmd = self.config.command.clone();
        thread::spawn(move || {
            let _ = launch_external_terminal(&name, pid, port, &cwd, &cmd);
        });
    }

    pub fn poll_status(&mut self, sys: &System) {
        // Fast path: if service is completely stopped and no restart scheduled, do nothing
        if self.state == ServiceState::Stopped && self.child_handle.is_none() && self.pid.is_none() && self.restart_at.is_none() {
            self.port_open = false;
            self.cpu_usage = 0.0;
            self.memory_mb = 0.0;
            return;
        }

        // Check pending auto-restart timer
        if let Some(restart_time) = self.restart_at {
            if std::time::Instant::now() >= restart_time {
                self.restart_at = None;
                self.append_log(
                    "[launcher] Auto-restarting crashed service now...".to_string(),
                    LogKind::Launcher,
                );
                self.start();
                return;
            } else {
                // Keep Errored state while waiting for restart delay
                return;
            }
        }

        if self.config.own_console {
            let pid_file = std::env::temp_dir().join(format!("launcher_{}.pid", self.config.key));
            // Unix terminal wrappers publish their shell PID after the terminal starts.
            if let Some(ProcessRecord::Legacy(pid)) = read_process_record(&self.config) {
                if pid > 0 {
                    self.pid = Some(pid);
                    self.save_process_identity(pid);
                }
            }

            if let Some(pid) = self.pid {
                {
                    let alive = refresh_process_alive(pid);
                    if !alive {
                        self.state = ServiceState::Stopped;
                        self.pid = None;
                        self.child_handle = None;
                        self.started_at = None;
                        self.cpu_usage = 0.0;
                        self.memory_mb = 0.0;
                        self.port_open = false;
                        let _ = std::fs::remove_file(&pid_file);

                        self.append_log(
                            "[launcher] Console process terminated.".to_string(),
                            LogKind::Launcher,
                        );

                        if self.config.auto_restart {
                            let now = std::time::Instant::now();
                            self.crash_timestamps.retain(|&t| now.duration_since(t) < Duration::from_secs(10));
                            self.crash_timestamps.push(now);

                            if self.crash_timestamps.len() > 5 {
                                self.append_log(
                                    "[launcher] Crash loop detected (>5 crashes in 10s). Auto-restart paused. Click 'Start' to retry.".to_string(),
                                    LogKind::Error,
                                );
                                self.restart_at = None;
                            } else {
                                self.state = ServiceState::Errored;
                                self.append_log(
                                    "[launcher] Auto-restart on crash enabled. Restarting in 2 seconds...".to_string(),
                                    LogKind::Warning,
                                );
                                self.restart_at = Some(now + Duration::from_millis(2000));
                            }
                        }
                        return;
                    } else {
                        self.state = ServiceState::Running;
                    }
                }
            }
        } else if let Some(child) = &mut self.child_handle {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.pid = None;
                    self.child_handle = None;
                    self.stdin_writer = None;
                    self.started_at = None;
                    self.cpu_usage = 0.0;
                    self.memory_mb = 0.0;
                    self.port_open = false;

                    let is_crash = !status.success();
                    if is_crash {
                        self.state = ServiceState::Errored;
                        self.append_log(
                            format!("[launcher] Process crashed with status: {}", status),
                            LogKind::Error,
                        );
                    } else {
                        self.state = ServiceState::Stopped;
                        self.append_log(
                            format!("[launcher] Process exited with status: {}", status),
                            LogKind::Launcher,
                        );
                    }

                    if self.config.auto_restart {
                        let now = std::time::Instant::now();
                        self.crash_timestamps.retain(|&t| now.duration_since(t) < Duration::from_secs(10));
                        self.crash_timestamps.push(now);

                        if self.crash_timestamps.len() > 5 {
                            self.append_log(
                                "[launcher] Crash loop detected (>5 crashes in 10s). Auto-restart paused. Click 'Start' to retry.".to_string(),
                                LogKind::Error,
                            );
                            self.restart_at = None;
                        } else {
                            self.state = ServiceState::Errored;
                            self.append_log(
                                "[launcher] Auto-restart on crash enabled. Restarting in 2 seconds...".to_string(),
                                LogKind::Warning,
                            );
                            self.restart_at = Some(now + Duration::from_millis(2000));
                        }
                    }
                    return;
                }
                Ok(None) => {
                    self.state = ServiceState::Running;
                }
                Err(_) => {}
            }
        } else if let Some(pid) = self.pid {
            let alive = refresh_process_alive(pid);

            if !alive {
                self.state = ServiceState::Stopped;
                self.pid = None;
                self.child_handle = None;
                self.started_at = None;
                self.cpu_usage = 0.0;
                self.memory_mb = 0.0;
                self.port_open = false;
                let pid_file = std::env::temp_dir().join(format!("launcher_{}.pid", self.config.key));
                let _ = std::fs::remove_file(&pid_file);

                self.append_log(
                    "[launcher] Attached process terminated.".to_string(),
                    LogKind::Launcher,
                );

                if self.config.auto_restart {
                    let now = std::time::Instant::now();
                    self.crash_timestamps.retain(|&t| now.duration_since(t) < Duration::from_secs(10));
                    self.crash_timestamps.push(now);

                    if self.crash_timestamps.len() > 5 {
                        self.append_log(
                            "[launcher] Crash loop detected (>5 crashes in 10s). Auto-restart paused. Click 'Start' to retry.".to_string(),
                            LogKind::Error,
                        );
                        self.restart_at = None;
                    } else {
                        self.state = ServiceState::Errored;
                        self.append_log(
                            "[launcher] Auto-restart on crash enabled. Restarting in 2 seconds...".to_string(),
                            LogKind::Warning,
                        );
                        self.restart_at = Some(now + Duration::from_millis(2000));
                    }
                }
                return;
            } else {
                self.state = ServiceState::Running;
            }
        }

        // Poll stats across main pid and child processes (ONLY when active)
        if let Some(pid) = self.pid {
            let pids = process_tree_pids(Pid::from_u32(pid), sys.processes().iter()
                .filter(|(_, process)| process.thread_kind().is_none())
                .map(|(&pid, process)| (pid, process.parent())));
            self.cpu_usage = pids.iter().filter_map(|pid| sys.process(*pid)).map(|process| process.cpu_usage()).sum();
            #[cfg(target_os = "linux")]
            {
                let total_rss_kb: u64 = pids.iter().map(|pid| get_proc_rss_kb_linux(pid.as_u32())).sum();
                self.memory_mb = (total_rss_kb as f32) / 1024.0;
            }
            #[cfg(not(target_os = "linux"))]
            {
                let total_mem_bytes: u64 = pids.iter().filter_map(|pid| sys.process(*pid)).map(|process| process.memory()).sum();
                self.memory_mb = (total_mem_bytes as f32) / (1024.0 * 1024.0);
            }

            // Poll port if configured — rate-limited to every 5s to avoid I/O overhead
            if self.config.port > 0 {
                if self.last_port_check.elapsed() >= Duration::from_secs(5) {
                    let addr = SocketAddr::from(([127, 0, 0, 1], self.config.port));
                    self.port_open = TcpStream::connect_timeout(&addr, Duration::from_millis(20)).is_ok();
                    self.last_port_check = std::time::Instant::now();
                }
            }
        } else {
            self.port_open = false;
        }
    }
}

#[cfg(target_os = "linux")]
pub fn get_proc_rss_kb_linux(pid: u32) -> u64 {
    // Check if this PID is a thread group leader (process) or internal thread
    if let Ok(status) = std::fs::read_to_string(format!("/proc/{}/status", pid)) {
        let mut tgid = pid;
        for line in status.lines() {
            if line.starts_with("Tgid:") {
                if let Some(num) = line.split_whitespace().nth(1) {
                    if let Ok(t) = num.parse::<u32>() {
                        tgid = t;
                    }
                }
            }
        }
        if tgid != pid {
            return 0; // Skip thread; its memory is already accounted for by the leader
        }
    }

    // Try smaps_rollup for Rss
    if let Ok(content) = std::fs::read_to_string(format!("/proc/{}/smaps_rollup", pid)) {
        for line in content.lines() {
            if line.starts_with("Rss:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(kb) = parts[1].parse::<u64>() {
                        return kb;
                    }
                }
            }
        }
    }

    // Fallback to /proc/{pid}/status VmRSS
    if let Ok(content) = std::fs::read_to_string(format!("/proc/{}/status", pid)) {
        for line in content.lines() {
            if line.starts_with("VmRSS:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(kb) = parts[1].parse::<u64>() {
                        return kb;
                    }
                }
            }
        }
    }
    0
}

#[cfg(not(target_os = "windows"))]
pub fn find_terminal() -> Option<(&'static str, &'static [&'static str])> {
    let terminals: &[(&str, &[&str])] = &[
        ("ptyxis", &["--new-window", "--"]),
        ("kgx", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        ("xfce4-terminal", &["-e"]),
        ("x-terminal-emulator", &["-e"]),
        ("alacritty", &["-e"]),
        ("kitty", &[]),
        ("wezterm", &["start", "--"]),
        ("tilix", &["-e"]),
        ("xterm", &["-e"]),
        ("foot", &[]),
        ("mate-terminal", &["-e"]),
        ("lxterminal", &["-e"]),
        ("terminator", &["-x"]),
        ("xdg-terminal-exec", &[]),
    ];

    let path_var = std::env::var("PATH").unwrap_or_default();
    let dirs: Vec<std::path::PathBuf> = std::env::split_paths(&path_var).collect();

    for (term, args) in terminals {
        for dir in &dirs {
            let candidate = dir.join(term);
            if candidate.is_file() {
                return Some((*term, *args));
            }
        }
    }
    None
}

pub fn open_folder(cwd: &str) {
    let path = if cwd.trim().is_empty() || cwd.trim() == "." {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        let p = std::path::Path::new(cwd.trim());
        if p.is_relative() {
            std::env::current_dir().map(|cd| cd.join(p)).unwrap_or_else(|_| p.to_path_buf())
        } else {
            p.to_path_buf()
        }
    };

    thread::spawn(move || {
        #[cfg(target_os = "linux")]
        {
            let _ = Command::new("xdg-open").arg(&path).spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = Command::new("open").arg(&path).spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = Command::new("explorer.exe").arg(&path).spawn();
        }
    });
}

pub fn open_in_vscode(cwd: &str) {
    let path = if cwd.trim().is_empty() || cwd.trim() == "." {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        let p = std::path::Path::new(cwd.trim());
        if p.is_relative() {
            std::env::current_dir().map(|cd| cd.join(p)).unwrap_or_else(|_| p.to_path_buf())
        } else {
            p.to_path_buf()
        }
    };

    thread::spawn(move || {
        #[cfg(target_os = "windows")]
        {
            if Command::new("code.cmd").arg(&path).spawn().is_err() {
                let _ = Command::new("code").arg(&path).spawn();
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            if Command::new("code").arg(&path).spawn().is_err() {
                if Command::new("codium").arg(&path).spawn().is_err() {
                    let _ = Command::new("code-insiders").arg(&path).spawn();
                }
            }
        }
    });
}

pub fn open_in_console(name: &str, pid: Option<u32>, port: u16, cwd: &str, cmd_str: &str) {
    let name = name.to_string();
    let cwd = cwd.to_string();
    let cmd_str = cmd_str.to_string();
    thread::spawn(move || {
        let _ = launch_external_terminal(&name, pid, port, &cwd, &cmd_str);
    });
}

pub fn launch_external_terminal(
    name: &str,
    pid: Option<u32>,
    port: u16,
    cwd: &str,
    cmd_str: &str,
) -> Result<(), String> {
    let cwd_path = if cwd.is_empty() { "." } else { cwd };
    #[cfg(not(target_os = "windows"))]
    {
        let pid_num = pid.unwrap_or(0);
        let script_dir = std::env::temp_dir();
        let safe_name: String = name.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
        let script_path = script_dir.join(format!("launcher_term_{}_{}.sh", safe_name, port));

        let script_content = format!(
r#"#!/usr/bin/env bash
cd "{cwd}" 2>/dev/null || true
echo "=========================================================="
echo " Server Launcher: External Terminal"
echo " Service : {name}"
echo " Port    : :{port}"
echo " PID     : {pid_num}"
echo " CWD     : {cwd}"
echo " Command : {cmd_str}"
echo "=========================================================="
echo ""
if command -v journalctl >/dev/null 2>&1 && [ "{pid_num}" -gt 0 ]; then
    echo "--- Systemd / journald output for PID {pid_num} ---"
    journalctl _PID={pid_num} -n 25 --no-pager 2>/dev/null || true
    echo ""
fi
echo "Interactive shell opened in {cwd}."
echo "Type exit or press Ctrl+D to close."
echo ""
exec bash
"#,
            cwd = cwd_path,
            name = name,
            port = port,
            pid_num = pid_num,
            cmd_str = cmd_str
        );

        if let Err(e) = std::fs::write(&script_path, script_content) {
            return Err(format!("Failed to write terminal script: {}", e));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(&script_path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(&script_path, perms);
            }
        }

        let script_str = script_path.to_string_lossy().to_string();

        #[cfg(target_os = "macos")]
        {
            let mut cmd = Command::new("open");
            cmd.args(&["-a", "Terminal", &script_str]);
            return cmd.spawn().map(|_| ()).map_err(|e| format!("Failed to spawn Terminal: {}", e));
        }

        #[cfg(not(target_os = "macos"))]
        {
            let chosen = find_terminal();

            let mut cmd;
            if let Some((term, args)) = chosen {
                cmd = Command::new(term);
                for arg in args {
                    cmd.arg(arg);
                }
                cmd.arg(&script_str);
            } else {
                let mut fallback_spawned = false;
                for fb in &["x-terminal-emulator", "xterm", "sh"] {
                    let mut c = Command::new(fb);
                    if *fb == "sh" {
                        c.args(&["-c", &script_str]);
                    } else {
                        c.args(&["-e", &script_str]);
                    }
                    c.current_dir(cwd_path);
                    if c.spawn().is_ok() {
                        fallback_spawned = true;
                        break;
                    }
                }
                if !fallback_spawned {
                    return Err("No compatible terminal emulator found on this system".to_string());
                }
                return Ok(());
            }

            cmd.current_dir(cwd_path);
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                cmd.process_group(0);
            }
            cmd.spawn().map_err(|e| format!("Failed to spawn terminal: {}", e))?;
            Ok(())
        }
    }

    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd.exe");
        let title = format!("Server: {} (: {})", name, port);
        cmd.args(&["/K", &format!("title {} && cd /d \"{}\"", title, cwd_path)]);
        cmd.spawn().map_err(|e| format!("Failed to spawn cmd: {}", e))?;
        Ok(())
    }
}

fn spawn_own_console(
    key: &str,
    name: &str,
    cmd_str: &str,
    cwd_str: &str,
    env_vars: &std::collections::HashMap<String, String>,
) -> std::io::Result<Child> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let _ = (key, name);
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/D", "/K", cmd_str]);
        cmd.creation_flags(0x00000010); // CREATE_NEW_CONSOLE keeps this child tracked.
        if !cwd_str.is_empty() && cwd_str != "." {
            cmd.current_dir(cwd_str);
        }
        cmd.envs(env_vars);
        cmd.spawn()
    }

    #[cfg(not(target_os = "windows"))]
    {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("launcher_{}.sh", key));
        let pid_path = temp_dir.join(format!("launcher_{}.pid", key));
        let pid_str = pid_path.to_string_lossy().to_string();

        let mut lines = Vec::new();
        lines.push("#!/usr/bin/env bash".to_string());
        lines.push(format!("echo $$ > \"{}\"", pid_str));
        lines.push(format!("trap 'rm -f \"{}\"' EXIT", pid_str));
        for (k, v) in env_vars {
            lines.push(format!("export {}=\"{}\"", k, v.replace('"', "\\\"")));
        }
        if !cwd_str.is_empty() && cwd_str != "." {
            lines.push(format!("cd \"{}\"", cwd_str));
        }
        lines.push(format!("echo -ne \"\\033]0;Server - {}\\007\"", name));
        lines.push(cmd_str.to_string());
        lines.push("EXIT_CODE=$?".to_string());
        lines.push("echo \"\"".to_string());
        lines.push("echo \"[launcher] Process exited with code $EXIT_CODE.\"".to_string());
        lines.push("read -n 1 -s -r -p \"Press any key to close this terminal...\"".to_string());

        let _ = std::fs::write(&script_path, lines.join("\n"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(&script_path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(&script_path, perms);
            }
        }

        let script_str = script_path.to_string_lossy().to_string();
        let chosen = find_terminal();

        let mut cmd;
        if let Some((term, args)) = chosen {
            cmd = Command::new(term);
            for arg in args {
                cmd.arg(arg);
            }
            cmd.arg(&script_str);
        } else {
            cmd = Command::new("sh");
            cmd.args(&["-c", &script_str]);
        }

        if !cwd_str.is_empty() && cwd_str != "." {
            cmd.current_dir(cwd_str);
        }
        for (k, v) in env_vars {
            cmd.env(k, v);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        cmd.spawn()
    }
}

pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if let Some(&next) = chars.peek() {
                if next == '[' {
                    chars.next(); // consume '['
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch >= '@' && ch <= '~' {
                            break;
                        }
                    }
                    continue;
                } else if next == ']' {
                    chars.next(); // consume ']'
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch == '\x07' || ch == '\x1b' {
                            if ch == '\x1b' && chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                    }
                    continue;
                } else if next == '(' || next == ')' {
                    chars.next();
                    chars.next();
                    continue;
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

pub fn push_log_line(
    raw_line: &str,
    logs: &Arc<Mutex<VecDeque<LogEntry>>>,
    max_logs: usize,
    default_kind: LogKind,
) {
    let cleaned = strip_ansi(raw_line);
    let trimmed = cleaned.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return;
    }

    let kind = match default_kind {
        LogKind::Stderr => {
            let lower = trimmed.to_lowercase();
            if lower.contains("warning:") || lower.contains("warn:") {
                LogKind::Warning
            } else {
                LogKind::Stderr
            }
        }
        _ => {
            let lower = trimmed.to_lowercase();
            if lower.contains("error:") || lower.contains("exception") || lower.contains("fatal:") {
                LogKind::Error
            } else if lower.contains("warning:") || lower.contains("warn:") {
                LogKind::Warning
            } else {
                default_kind
            }
        }
    };

    let entry = LogEntry::new(trimmed.to_string(), kind);
    if let Ok(mut l) = logs.lock() {
        if l.len() >= max_logs {
            l.pop_front();
        }
        l.push_back(entry);
    }
}

pub fn spawn_stream_reader<R: Read + Send + 'static>(
    mut stream: R,
    logs: Arc<Mutex<VecDeque<LogEntry>>>,
    max_logs: usize,
    default_kind: LogKind,
) {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut pending = Vec::new();

        loop {
            match stream.read(&mut buf) {
                Ok(0) => {
                    // EOF reached: flush any remaining bytes
                    if !pending.is_empty() {
                        let text = String::from_utf8_lossy(&pending);
                        for line in text.split(|c| c == '\n' || c == '\r') {
                            push_log_line(line, &logs, max_logs, default_kind.clone());
                        }
                    }
                    break;
                }
                Ok(n) => {
                    let chunk = &buf[..n];
                    pending.extend_from_slice(chunk);

                    // Check if pending contains any newline or carriage return delimiter
                    if pending.contains(&b'\n') || pending.contains(&b'\r') {
                        let mut last_delim = 0;
                        for (idx, b) in pending.iter().enumerate() {
                            if *b == b'\n' || *b == b'\r' {
                                last_delim = idx + 1;
                            }
                        }

                        let complete_part = String::from_utf8_lossy(&pending[..last_delim]).to_string();
                        for line in complete_part.split(|c| c == '\n' || c == '\r') {
                            push_log_line(line, &logs, max_logs, default_kind.clone());
                        }

                        pending = pending[last_delim..].to_vec();
                    } else if pending.len() > 1024 {
                        // Flush long output lines even without newline to prevent stalling
                        let text = String::from_utf8_lossy(&pending).to_string();
                        push_log_line(&text, &logs, max_logs, default_kind.clone());
                        pending.clear();
                    }
                }
                Err(_) => {
                    if !pending.is_empty() {
                        let text = String::from_utf8_lossy(&pending);
                        for line in text.split(|c| c == '\n' || c == '\r') {
                            push_log_line(line, &logs, max_logs, default_kind.clone());
                        }
                    }
                    break;
                }
            }
        }
    });
}

pub fn prepare_environment(cmd: &mut Command, cwd_str: &str, env_vars: &HashMap<String, String>) {
    // Unbuffer Python and force UTF-8 output across all runtimes
    cmd.env("PYTHONUNBUFFERED", "1");
    cmd.env("PYTHONIOENCODING", "utf-8");
    cmd.env("FORCE_COLOR", "1");
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("EXPO_NO_TELEMETRY", "1");
    cmd.env_remove("CI");

    for (k, v) in env_vars {
        cmd.env(k, v);
    }

    if !cwd_str.is_empty() && cwd_str != "." {
        let cwd_path = std::path::Path::new(cwd_str);
        let mut path_additions = Vec::new();

        // Check virtualenv
        for folder in [".venv", "venv", "env", ".virtualenv"] {
            let venv_dir = cwd_path.join(folder);
            #[cfg(unix)]
            let bin_dir = venv_dir.join("bin");
            #[cfg(windows)]
            let bin_dir = venv_dir.join("Scripts");

            if bin_dir.is_dir() {
                path_additions.push(bin_dir.to_string_lossy().to_string());
                cmd.env("VIRTUAL_ENV", venv_dir.to_string_lossy().to_string());
                break;
            }
        }

        // Check node_modules/.bin
        let node_bin = cwd_path.join("node_modules").join(".bin");
        if node_bin.is_dir() {
            path_additions.push(node_bin.to_string_lossy().to_string());
        }

        if !path_additions.is_empty() {
            let current_path = std::env::var("PATH").unwrap_or_default();
            #[cfg(unix)]
            let sep = ":";
            #[cfg(windows)]
            let sep = ";";

            let new_path = format!("{}{}{}", path_additions.join(sep), sep, current_path);
            cmd.env("PATH", new_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openpty() {
        #[cfg(unix)]
        unsafe {
            use std::os::unix::io::FromRawFd;
            use std::process::Stdio;
            use std::io::{Read, Write};

            let mut master = 0;
            let mut slave = 0;
            let res = libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
            assert_eq!(res, 0);

            // Configure termios to disable echo
            let mut termios: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(slave, &mut termios) == 0 {
                termios.c_lflag &= !libc::ECHO;
                libc::tcsetattr(slave, libc::TCSANOW, &termios);
            }

            // Set terminal size
            let ws = libc::winsize {
                ws_row: 24,
                ws_col: 120,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            libc::ioctl(master, libc::TIOCSWINSZ, &ws);

            let slave_in = Stdio::from_raw_fd(libc::dup(slave));
            let slave_out = Stdio::from_raw_fd(libc::dup(slave));
            let slave_err = Stdio::from_raw_fd(slave);

            let mut child = Command::new("node")
                .args(["-i"]) // interactive node REPL
                .stdin(slave_in)
                .stdout(slave_out)
                .stderr(slave_err)
                .spawn()
                .expect("failed to spawn node");

            let master_write_fd = libc::dup(master);
            let mut master_writer = std::fs::File::from_raw_fd(master_write_fd);
            let mut master_reader = std::fs::File::from_raw_fd(master);

            // Send input to REPL
            writeln!(master_writer, "100 + 234").expect("failed to write to master");
            writeln!(master_writer, ".exit").expect("failed to write exit");

            let mut output = String::new();
            let mut buf = [0u8; 1024];
            while let Ok(n) = master_reader.read(&mut buf) {
                if n == 0 { break; }
                output.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
            let _ = child.wait();
            println!("REPL output:\n{}", output);
            assert!(output.contains("334"));
        }
    }

    #[test]
    fn test_strip_ansi() {
        let input = "\x1b[32mSuccess!\x1b[0m \x1b[1;31mError text\x1b[0m\r\n";
        let cleaned = strip_ansi(input);
        assert_eq!(cleaned, "Success! Error text\n");
    }

    #[test]
    fn test_push_log_line_classification() {
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        push_log_line("\x1b[31mError: connection refused\x1b[0m", &logs, 100, LogKind::Stdout);
        push_log_line("\x1b[33mWarning: low disk space\x1b[0m", &logs, 100, LogKind::Stdout);
        push_log_line("Server ready on port 3000", &logs, 100, LogKind::Stdout);

        let locked = logs.lock().unwrap();
        assert_eq!(locked.len(), 3);
        assert_eq!(locked[0].kind, LogKind::Error);
        assert_eq!(locked[0].text, "Error: connection refused");
        assert_eq!(locked[1].kind, LogKind::Warning);
        assert_eq!(locked[1].text, "Warning: low disk space");
        assert_eq!(locked[2].kind, LogKind::Stdout);
        assert_eq!(locked[2].text, "Server ready on port 3000");
    }

    #[test]
    fn test_push_log_line_empty() {
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        push_log_line("\r\n", &logs, 100, LogKind::Stdout);
        push_log_line("", &logs, 100, LogKind::Stdout);
        let locked = logs.lock().unwrap();
        assert_eq!(locked.len(), 0);
    }

    #[test]
    fn test_service_send_input_stopped_warning() {
        let cfg = ServerConfig {
            key: "test_srv".to_string(),
            name: "Test Server".to_string(),
            command: "echo test".to_string(),
            cwd: ".".to_string(),
            port: 3000,
            auto_restart: false,
            autostart: false,
            group: "default".to_string(),
            env: std::collections::HashMap::new(),
            actions: Vec::new(),
            links: Vec::new(),
            stop_command: String::new(),
            own_console: false,
            ..Default::default()
        };
        let s = Service::new(cfg);
        assert!(!s.is_active());

        s.send_input("h");
        let logs = s.logs.lock().unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].kind, LogKind::Warning);
        assert!(logs[0].text.contains("Cannot send 'h': server is not running or stdin is closed."));
    }

    fn identity_test_config() -> ServerConfig {
        ServerConfig {
            key: format!("identity_test_{}_{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
            command: "python run.py".into(), cwd: ".".into(), port: 5173,
            ..Default::default()
        }
    }

    #[test]
    fn process_tree_includes_nested_children_once_and_ignores_other_roots() {
        let pid = Pid::from_u32;
        let parents = [
            (pid(3), Some(pid(2))), (pid(2), Some(pid(1))),
            (pid(4), Some(pid(3))), (pid(2), Some(pid(1))),
            (pid(9), None), (pid(10), Some(pid(9))),
            (pid(1), Some(pid(4))),
        ];
        let mut tree = process_tree_pids(pid(1), parents);
        tree.sort_unstable();
        assert_eq!(tree, vec![pid(1), pid(2), pid(3), pid(4)]);
        assert_eq!(process_tree_pids(pid(7), parents), vec![pid(7)]);
    }

    #[test]
    fn scheduled_process_refresh_measures_busy_cpu_without_liveness_interference() {
        let pid = Pid::from_u32(std::process::id());
        let mut sys = System::new();
        refresh_process_metrics(&mut sys);
        // The first interval can still be warmup for a process whose initial CPU ticks were zero.
        for _ in 0..2 {
            let until = std::time::Instant::now() + sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(100);
            let mut value = 1_u64;
            while std::time::Instant::now() < until {
                value = std::hint::black_box(value.wrapping_mul(3).wrapping_add(1));
            }
            assert!(refresh_process_alive(pid.as_u32()));
            refresh_process_metrics(&mut sys);
        }
        assert!(sys.process(pid).unwrap().cpu_usage() > 0.0);
    }

    #[cfg(unix)]
    #[test]
    fn dead_processes_are_not_reported_alive_from_stale_sysinfo_entries() {
        let mut child = Command::new("sleep").arg("60").spawn().unwrap();
        let pid = child.id();
        let mut sys = System::new();
        refresh_process_metrics(&mut sys);
        assert!(refresh_process_alive(pid));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(sys.process(Pid::from_u32(pid)).is_some());
        assert!(!refresh_process_alive(pid));
    }

    #[test]
    fn process_identity_rejects_reused_pids_and_changed_configs() {
        let config = identity_test_config();
        let record = ProcessIdentity { key: config.key.clone(), command: config.command.clone(), cwd: config.cwd.clone(), pid: 123, start_time: 456 };
        assert!(record.matches(&config, 123, 456));
        assert!(!record.matches(&config, 124, 456));
        assert!(!record.matches(&config, 123, 457));
        assert!(!ProcessIdentity { start_time: 0, ..record.clone() }.matches(&config, 123, 0));
        for other in [
            ServerConfig { key: "another config".into(), ..config.clone() },
            ServerConfig { command: "npm run dev".into(), ..config.clone() },
            ServerConfig { cwd: "another directory".into(), ..config.clone() },
        ] { assert!(!record.matches(&other, 123, 456)); }
    }

    #[test]
    fn shared_ports_reconnect_only_the_recorded_config_on_restart_and_refresh() {
        let owner = identity_test_config();
        let other = identity_test_config();
        let pid = std::process::id();
        write_process_record(&owner, pid).unwrap();
        let mut services = vec![Service::new(other), Service::new(owner.clone())];
        reconnect_services(&mut services);
        assert_eq!(services[0].state, ServiceState::Stopped);
        assert_eq!(services[0].pid, None);
        assert_eq!(services[1].state, ServiceState::Running);
        assert_eq!(services[1].pid, Some(pid));
        // Refresh must not let a second config take ownership of an already attached PID.
        write_process_record(&services[0].config, pid).unwrap();
        reconnect_services(&mut services);
        assert_eq!(services[0].state, ServiceState::Stopped);
        assert_eq!(services[1].pid, Some(pid));
        for service in services { let _ = std::fs::remove_file(pid_file(&service.config)); }
    }

    #[test]
    fn conflicting_records_and_stale_start_times_do_not_attach() {
        let first = identity_test_config();
        let second = identity_test_config();
        let pid = std::process::id();
        write_process_record(&first, pid).unwrap();
        write_process_record(&second, pid).unwrap();
        let mut services = vec![Service::new(first), Service::new(second)];
        reconnect_services(&mut services);
        assert!(services.iter().all(|service| service.state == ServiceState::Stopped));
        let ProcessRecord::Identity(mut identity) = read_process_record(&services[0].config).unwrap() else { panic!("Expected identity record"); };
        identity.start_time += 1;
        std::fs::write(pid_file(&services[0].config), serde_json::to_vec(&identity).unwrap()).unwrap();
        reconnect_services(&mut services[..1]);
        assert_eq!(services[0].state, ServiceState::Stopped);
        for service in services { let _ = std::fs::remove_file(pid_file(&service.config)); }
    }

    #[test]
    fn legacy_records_migrate_but_duplicate_pid_claims_are_rejected() {
        let pid = std::process::id();
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), sysinfo::ProcessRefreshKind::new()
            .with_cmd(sysinfo::UpdateKind::Always).with_cwd(sysinfo::UpdateKind::Always));
        let process = sys.process(Pid::from_u32(pid)).unwrap();
        let mut config = identity_test_config();
        config.command = process.cmd().iter().map(|arg| arg.to_string_lossy()).collect::<Vec<_>>().join(" ");
        config.cwd = process.cwd().unwrap().to_string_lossy().into_owned();
        let mut duplicate = config.clone();
        duplicate.key = identity_test_config().key;
        for config in [&config, &duplicate] { std::fs::write(pid_file(config), pid.to_string()).unwrap(); }
        let mut services = vec![Service::new(config), Service::new(duplicate)];
        reconnect_services(&mut services);
        assert!(services.iter().all(|service| service.state == ServiceState::Stopped));
        // With only one claim, a validated old integer PID file becomes an identity record.
        reconnect_services(&mut services[..1]);
        assert_eq!(services[0].pid, Some(pid));
        assert!(matches!(read_process_record(&services[0].config), Some(ProcessRecord::Identity(_))));
        for service in services { let _ = std::fs::remove_file(pid_file(&service.config)); }
    }

    #[test]
    fn legacy_pid_files_require_command_and_directory_evidence() {
        let config = identity_test_config();
        let cwd = std::env::current_dir().unwrap();
        let args = ["sh".into(), "-c".into(), config.command.clone().into()];
        assert!(legacy_process_matches(&config, &args, Some(&cwd)));
        assert!(!legacy_process_matches(&config, &args, None));
        assert!(!legacy_process_matches(&config, &["node".into(), "other.js".into()], Some(&cwd)));
        assert!(matches!(serde_json::from_str::<ProcessRecord>("123\n").unwrap(), ProcessRecord::Legacy(123)));
        assert!(serde_json::from_str::<ProcessRecord>("not a pid").is_err());
        let console = ServerConfig { own_console: true, ..config.clone() };
        let wrapper = std::env::temp_dir().join(format!("launcher_{}.sh", config.key));
        assert!(legacy_process_matches(&console, &["bash".into(), wrapper.into_os_string()], Some(&cwd)));
    }

    #[test]
    fn test_service_attach_existing_and_stop() {
        let cfg = ServerConfig {
            key: "test_attach".to_string(),
            name: "Attach Test".to_string(),
            command: "sleep 10".to_string(),
            cwd: ".".to_string(),
            port: 8080,
            auto_restart: false,
            autostart: false,
            group: "default".to_string(),
            env: std::collections::HashMap::new(),
            actions: Vec::new(),
            links: Vec::new(),
            stop_command: String::new(),
            own_console: false,
            ..Default::default()
        };
        let mut s = Service::new(cfg);
        assert_eq!(s.state, ServiceState::Stopped);
        assert!(!s.is_active());

        // Attach to current test process PID
        let current_pid = std::process::id();
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(current_pid)]), sysinfo::ProcessRefreshKind::new());
        let start_time = sys.process(Pid::from_u32(current_pid)).unwrap().start_time();
        s.attach_existing(current_pid, start_time);

        assert_eq!(s.state, ServiceState::Running);
        assert_eq!(s.pid, Some(current_pid));
        assert!(s.port_open);
        assert!(s.is_active());

        let logs = s.logs.lock().unwrap();
        assert!(logs.iter().any(|entry| entry.text.contains("Reconnected to previously running process")));
        assert!(logs.iter().any(|entry| entry.text.contains("Terminal session re-attached")));
        drop(logs);

        // Check that PID file was written
        let pid_file = std::env::temp_dir().join("launcher_test_attach.pid");
        assert!(pid_file.exists());
        let _ = std::fs::remove_file(&pid_file);
    }
}

