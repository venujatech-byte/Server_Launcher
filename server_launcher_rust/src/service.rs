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
    pub started_at: Option<std::time::Instant>,

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
            started_at: None,
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
                self.started_at = Some(std::time::Instant::now());
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

        if self.config.own_console {
            let pid_file = std::env::temp_dir().join(format!("launcher_{}.pid", self.config.key));
            if let Ok(content) = std::fs::read_to_string(&pid_file) {
                if let Ok(p) = content.trim().parse::<u32>() {
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-(p as i32), libc::SIGKILL);
                        libc::kill(p as i32, libc::SIGKILL);
                    }
                }
            }
            let _ = std::fs::remove_file(&pid_file);
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
        if self.config.own_console {
            let pid_file = std::env::temp_dir().join(format!("launcher_{}.pid", self.config.key));
            if let Ok(content) = std::fs::read_to_string(&pid_file) {
                if let Ok(p) = content.trim().parse::<u32>() {
                    self.pid = Some(p);
                }
            }

            if let Some(pid) = self.pid {
                #[cfg(unix)]
                {
                    let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
                    if !alive {
                        self.state = ServiceState::Stopped;
                        self.pid = None;
                        self.child_handle = None;
                        self.started_at = None;
                        self.cpu_usage = 0.0;
                        self.memory_mb = 0.0;
                        let _ = std::fs::remove_file(&pid_file);
                    } else {
                        self.state = ServiceState::Running;
                    }
                }
            }
        } else if let Some(child) = &mut self.child_handle {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.state = ServiceState::Stopped;
                    self.pid = None;
                    self.child_handle = None;
                    self.stdin_writer = None;
                    self.started_at = None;
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

        // Poll stats across main pid and child processes
        if let Some(pid) = self.pid {
            #[cfg(target_os = "linux")]
            {
                let mut all_pids = vec![pid];
                all_pids.extend(get_child_pids_linux(pid));
                all_pids.sort_unstable();
                all_pids.dedup();

                let total_rss_kb: u64 = all_pids.iter().map(|&p| get_proc_rss_kb_linux(p)).sum();
                self.memory_mb = (total_rss_kb as f32) / 1024.0;

                let mut total_cpu = 0.0;
                for &p in &all_pids {
                    let s_pid = Pid::from_u32(p);
                    if let Some(process) = sys.process(s_pid) {
                        total_cpu += process.cpu_usage();
                    }
                }
                self.cpu_usage = total_cpu;
            }

            #[cfg(not(target_os = "linux"))]
            {
                let s_pid = Pid::from_u32(pid);
                let mut total_cpu = 0.0;
                let mut total_mem_bytes = 0;

                for (p_pid, process) in sys.processes() {
                    if *p_pid == s_pid || process.parent() == Some(s_pid) {
                        total_cpu += process.cpu_usage();
                        total_mem_bytes += process.memory();
                    }
                }
                self.cpu_usage = total_cpu;
                self.memory_mb = (total_mem_bytes as f32) / (1024.0 * 1024.0);
            }
        }

        // Poll port if configured
        if self.config.port > 0 {
            let addr = SocketAddr::from(([127, 0, 0, 1], self.config.port));
            self.port_open = TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok();
        }
    }
}

#[cfg(target_os = "linux")]
fn get_child_pids_linux(pid: u32) -> Vec<u32> {
    let mut children = Vec::new();
    let task_path = format!("/proc/{}/task", pid);
    if let Ok(entries) = std::fs::read_dir(&task_path) {
        for entry in entries.flatten() {
            let child_file = entry.path().join("children");
            if let Ok(content) = std::fs::read_to_string(&child_file) {
                for token in content.split_whitespace() {
                    if let Ok(cid) = token.parse::<u32>() {
                        if !children.contains(&cid) {
                            children.push(cid);
                            children.extend(get_child_pids_linux(cid));
                        }
                    }
                }
            }
        }
    }
    children
}

#[cfg(target_os = "linux")]
fn get_proc_rss_kb_linux(pid: u32) -> u64 {
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
fn find_terminal() -> Option<(&'static str, &'static [&'static str])> {
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

fn spawn_own_console(
    key: &str,
    name: &str,
    cmd_str: &str,
    cwd_str: &str,
    env_vars: &std::collections::HashMap<String, String>,
) -> std::io::Result<Child> {
    #[cfg(target_os = "windows")]
    {
        let temp_dir = std::env::temp_dir();
        let batch_path = temp_dir.join(format!("launcher_{}.cmd", key));
        let mut lines = Vec::new();
        lines.push("@echo off".to_string());
        lines.push(format!("title Server - {}", name));
        if !cwd_str.is_empty() && cwd_str != "." {
            lines.push(format!("cd /d \"{}\"", cwd_str));
        }
        for (k, v) in env_vars {
            lines.push(format!("set \"{}={}\"", k, v));
        }
        lines.push(cmd_str.to_string());
        lines.push("pause".to_string());
        let _ = std::fs::write(&batch_path, lines.join("\r\n"));

        let mut cmd = Command::new("cmd.exe");
        cmd.args(&[
            "/C",
            "start",
            &format!("Server - {}", name),
            "cmd.exe",
            "/K",
            &batch_path.to_string_lossy(),
        ]);
        if !cwd_str.is_empty() && cwd_str != "." {
            cmd.current_dir(cwd_str);
        }
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
