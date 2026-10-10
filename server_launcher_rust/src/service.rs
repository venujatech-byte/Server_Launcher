use crate::config::ServerConfig;
use chrono::Local;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use sysinfo::{Pid, System};

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

pub struct Service {
    pub config: ServerConfig,
    pub state: ServiceState,
    pub pid: Option<u32>,
    pub port_open: bool,
    pub cpu_usage: f32,
    pub memory_mb: f32,

    pub logs: Arc<Mutex<VecDeque<LogEntry>>>,
    stdin_writer: Option<Arc<Mutex<ChildStdin>>>,
    child_handle: Option<Child>,

    max_logs: usize,
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
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(3000))),
            stdin_writer: None,
            child_handle: None,
            max_logs: 3000,
        }
    }

    pub fn append_log(&self, text: String, kind: LogKind) {
        let entry = LogEntry::new(text, kind);
        if let Ok(mut logs) = self.logs.lock() {
            if logs.len() >= self.max_logs {
                logs.pop_front();
            }
            logs.push_back(entry);
        }
    }

    pub fn clear_logs(&self) {
        if let Ok(mut logs) = self.logs.lock() {
            logs.clear();
        }
    }

    pub fn start(&mut self) {
        if self.state == ServiceState::Running || self.state == ServiceState::Starting {
            return;
        }

        self.append_log(
            format!("[launcher] Starting service: {} ...", self.config.name),
            LogKind::Launcher,
        );
        self.state = ServiceState::Starting;

        let cmd_str = self.config.command.clone();
        let cwd_str = self.config.cwd.clone();
        let env_vars = self.config.env.clone();

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

        for (k, v) in env_vars {
            cmd.env(k, v);
        }

        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        match cmd.spawn() {
            Ok(mut child) => {
                let pid = child.id();
                self.pid = Some(pid);
                self.state = ServiceState::Running;
                self.append_log(
                    format!("[launcher] Process spawned (PID: {})", pid),
                    LogKind::Launcher,
                );

                if let Some(stdin) = child.stdin.take() {
                    self.stdin_writer = Some(Arc::new(Mutex::new(stdin)));
                }

                // Pipe stdout reader thread
                if let Some(stdout) = child.stdout.take() {
                    let logs_clone = Arc::clone(&self.logs);
                    let max_logs = self.max_logs;
                    thread::spawn(move || {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines().flatten() {
                            let kind = if line.to_lowercase().contains("error:")
                                || line.to_lowercase().contains("exception")
                            {
                                LogKind::Error
                            } else if line.to_lowercase().contains("warning:") {
                                LogKind::Warning
                            } else {
                                LogKind::Stdout
                            };
                            let entry = LogEntry::new(line, kind);
                            if let Ok(mut l) = logs_clone.lock() {
                                if l.len() >= max_logs {
                                    l.pop_front();
                                }
                                l.push_back(entry);
                            }
                        }
                    });
                }

                // Pipe stderr reader thread
                if let Some(stderr) = child.stderr.take() {
                    let logs_clone = Arc::clone(&self.logs);
                    let max_logs = self.max_logs;
                    thread::spawn(move || {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines().flatten() {
                            let entry = LogEntry::new(line, LogKind::Stderr);
                            if let Ok(mut l) = logs_clone.lock() {
                                if l.len() >= max_logs {
                                    l.pop_front();
                                }
                                l.push_back(entry);
                            }
                        }
                    });
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

    pub fn stop(&mut self) {
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
                    // Try graceful SIGTERM to process group, then SIGKILL
                    libc::kill(-(pid as i32), libc::SIGTERM);
                }
            }
        }

        if let Some(mut child) = self.child_handle.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        self.pid = None;
        self.stdin_writer = None;
        self.state = ServiceState::Stopped;
        self.cpu_usage = 0.0;
        self.memory_mb = 0.0;
        self.append_log("[launcher] Service stopped.".to_string(), LogKind::Launcher);
    }

    pub fn send_input(&self, input_text: &str) {
        if let Some(stdin_mutex) = &self.stdin_writer {
            if let Ok(mut writer) = stdin_mutex.lock() {
                let _ = writeln!(writer, "{}", input_text);
                let _ = writer.flush();
                self.append_log(format!("> {}", input_text), LogKind::Stdin);
            }
        }
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

    pub fn poll_status(&mut self, sys: &mut System) {
        // Check if child exited
        if let Some(child) = &mut self.child_handle {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.state = ServiceState::Stopped;
                    self.pid = None;
                    self.child_handle = None;
                    self.stdin_writer = None;
                    self.cpu_usage = 0.0;
                    self.memory_mb = 0.0;
                    self.append_log(
                        format!("[launcher] Process exited with status: {}", status),
                        LogKind::Launcher,
                    );
                }
                Ok(None) => {
                    self.state = ServiceState::Running;
                }
                Err(_) => {}
            }
        }

        // Poll stats
        if let Some(pid) = self.pid {
            let s_pid = Pid::from_u32(pid);
            if let Some(process) = sys.process(s_pid) {
                self.cpu_usage = process.cpu_usage();
                self.memory_mb = (process.memory() as f32) / (1024.0 * 1024.0);
            }
        }

        // Poll port if configured
        if self.config.port > 0 {
            let addr = SocketAddr::from(([127, 0, 0, 1], self.config.port));
            self.port_open = TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok();
        }
    }
}
