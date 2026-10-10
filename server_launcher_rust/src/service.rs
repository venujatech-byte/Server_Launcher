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

    pub restart_at: Option<std::time::Instant>,
    pub crash_timestamps: Vec<std::time::Instant>,
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
            restart_at: None,
            crash_timestamps: Vec::new(),
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

        prepare_environment(&mut cmd, &cwd_str, &env_vars);

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

                if let Some(stdout) = child.stdout.take() {
                    spawn_stream_reader(
                        stdout,
                        Arc::clone(&self.logs),
                        self.max_logs,
                        LogKind::Stdout,
                    );
                }

                if let Some(stderr) = child.stderr.take() {
                    spawn_stream_reader(
                        stderr,
                        Arc::clone(&self.logs),
                        self.max_logs,
                        LogKind::Stderr,
                    );
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
        }

        // Poll stats across main pid and child processes (ONLY when active)
        if let Some(pid) = self.pid {
            #[cfg(target_os = "linux")]
            {
                let mut all_pids = vec![pid];
                all_pids.extend(get_child_pids_linux(pid));
                all_pids.sort_unstable();
                all_pids.dedup();

                let total_rss_kb: u64 = all_pids.iter().map(|&p| get_proc_rss_kb_linux(p)).sum();
                self.memory_mb = (total_rss_kb as f32) / 1024.0;

                // Refresh ONLY our specific PIDs in sysinfo (not the entire OS!)
                let s_pids: Vec<Pid> = all_pids.iter().map(|&p| Pid::from_u32(p)).collect();
                sys.refresh_processes(ProcessesToUpdate::Some(&s_pids));

                let mut total_cpu = 0.0;
                for &s_pid in &s_pids {
                    if let Some(process) = sys.process(s_pid) {
                        total_cpu += process.cpu_usage();
                    }
                }
                self.cpu_usage = total_cpu;
            }

            #[cfg(not(target_os = "linux"))]
            {
                let s_pid = Pid::from_u32(pid);
                sys.refresh_processes(ProcessesToUpdate::Some(&[s_pid]));
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

            // Poll port if configured with tiny timeout
            if self.config.port > 0 {
                let addr = SocketAddr::from(([127, 0, 0, 1], self.config.port));
                self.port_open = TcpStream::connect_timeout(&addr, Duration::from_millis(20)).is_ok();
            }
        } else {
            self.port_open = false;
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
        let script_path = script_dir.join(format!("launcher_term_{}.sh", port));

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
        let chosen = find_terminal();

        let mut cmd;
        if let Some((term, args)) = chosen {
            cmd = Command::new(term);
            for arg in args {
                cmd.arg(arg);
            }
            cmd.arg(&script_str);
        } else {
            cmd = Command::new("x-terminal-emulator");
            cmd.args(&["-e", &script_str]);
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
}

