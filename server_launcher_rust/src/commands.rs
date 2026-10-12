use crate::config::{SavedCommand, SshRemoteHost};
use crate::remote::build_ssh_command;
use crate::service::{LogEntry, LogKind};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalCommandStatus {
    Idle,
    Running(String),
    Success(i32),
    Failed(String),
}

pub struct TerminalSession {
    pub logs: Arc<Mutex<VecDeque<LogEntry>>>,
    pub input_text: String,
    pub history: Vec<String>,
    pub history_idx: Option<usize>,
    pub status: Arc<Mutex<TerminalCommandStatus>>,
    pub running_child: Option<Arc<Mutex<Option<Child>>>>,
    pub running_pid: Option<u32>,
    pub stdin_writer: Option<Arc<Mutex<std::process::ChildStdin>>>,
    pub cwd: PathBuf,
    pub prev_cwd: Option<PathBuf>,
    pub auto_scroll: bool,
    pub request_focus: bool,
}

impl Default for TerminalSession {
    fn default() -> Self {
        let initial_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(256))),
            input_text: String::new(),
            history: Vec::new(),
            history_idx: None,
            status: Arc::new(Mutex::new(TerminalCommandStatus::Idle)),
            running_child: None,
            running_pid: None,
            stdin_writer: None,
            cwd: initial_cwd,
            prev_cwd: None,
            auto_scroll: true,
            request_focus: true,
        }
    }
}

impl TerminalSession {
    pub fn clear(&mut self) {
        if let Ok(mut l) = self.logs.lock() {
            l.clear();
        }
    }

    pub fn is_running(&self) -> bool {
        if let Ok(st) = self.status.lock() {
            matches!(*st, TerminalCommandStatus::Running(_))
        } else {
            false
        }
    }

    pub fn send_stdin(&mut self, text: &str) -> bool {
        if let Some(stdin_arc) = &self.stdin_writer {
            if let Ok(mut writer) = stdin_arc.lock() {
                let _ = writeln!(writer, "{}", text);
                let _ = writer.flush();
                if let Ok(mut l) = self.logs.lock() {
                    l.push_back(LogEntry::new(text.to_string(), LogKind::Stdin));
                }
                return true;
            }
        }
        false
    }

    pub fn cancel_line(&mut self) {
        let prompt = self.get_prompt_str(None);
        if let Ok(mut l) = self.logs.lock() {
            l.push_back(LogEntry::new(format!("{}{} ^C", prompt, self.input_text), LogKind::Stdin));
        }
        self.input_text.clear();
        self.history_idx = None;
    }

    pub fn interrupt(&mut self) {
        if let Some(pid) = self.running_pid {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGINT);
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
        if let Some(stdin_arc) = &self.stdin_writer {
            if let Ok(mut sin) = stdin_arc.lock() {
                let _ = sin.write_all(b"\x03\n");
                let _ = sin.flush();
            }
        }
        if let Some(child_arc) = &self.running_child {
            if let Ok(mut lock) = child_arc.lock() {
                if let Some(child) = lock.as_mut() {
                    let _ = child.kill();
                }
            }
        }
        if let Ok(mut st) = self.status.lock() {
            if matches!(*st, TerminalCommandStatus::Running(_)) {
                *st = TerminalCommandStatus::Failed("Interrupted (SIGINT)".to_string());
            }
        }
        if let Ok(mut l) = self.logs.lock() {
            l.push_back(LogEntry::new("^C".to_string(), LogKind::Warning));
        }
        self.running_child = None;
        self.running_pid = None;
        self.stdin_writer = None;
    }

    pub fn get_prompt_str(&self, host: Option<&SshRemoteHost>) -> String {
        if let Some(h) = host {
            format!("{}@{}:~$ ", h.user, h.host)
        } else {
            let username = std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "user".to_string());

            let hostname = match std::fs::read_to_string("/etc/hostname") {
                Ok(h) => h.trim().to_string(),
                Err(_) => "local".to_string(),
            };

            let home_dir = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok().map(PathBuf::from);
            let display_path = if let Some(home) = home_dir {
                if let Ok(rel) = self.cwd.strip_prefix(&home) {
                    let rel_str = rel.display().to_string();
                    if rel_str.is_empty() {
                        "~".to_string()
                    } else {
                        format!("~/{}", rel_str)
                    }
                } else {
                    self.cwd.display().to_string()
                }
            } else {
                self.cwd.display().to_string()
            };

            format!("{}@{}:{}$ ", username, hostname, display_path)
        }
    }

    pub fn autocomplete_path(&mut self) {
        if self.input_text.is_empty() {
            return;
        }
        let words: Vec<String> = self.input_text.split_whitespace().map(|s| s.to_string()).collect();
        let last_token: String = if self.input_text.ends_with(' ') {
            String::new()
        } else {
            words.last().cloned().unwrap_or_default()
        };

        let target_dir = if last_token.contains('/') {
            let (dir_part, _) = last_token.rsplit_once('/').unwrap();
            if dir_part.is_empty() {
                PathBuf::from("/")
            } else if dir_part.starts_with('~') {
                if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
                    PathBuf::from(home).join(dir_part[1..].trim_start_matches('/'))
                } else {
                    self.cwd.join(dir_part)
                }
            } else {
                self.cwd.join(dir_part)
            }
        } else {
            self.cwd.clone()
        };

        let prefix = if last_token.contains('/') {
            last_token.rsplit_once('/').unwrap().1.to_string()
        } else {
            last_token.clone()
        };

        if let Ok(entries) = std::fs::read_dir(&target_dir) {
            let mut matches = Vec::new();
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(&prefix) {
                    let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    matches.push((name, is_dir));
                }
            }

            if matches.len() == 1 {
                let (matched_name, is_dir) = &matches[0];
                let suffix = if *is_dir { "/" } else { " " };
                let completed = if last_token.contains('/') {
                    let (dir_part, _) = last_token.rsplit_once('/').unwrap();
                    format!("{}/{}{}", dir_part, matched_name, suffix)
                } else {
                    format!("{}{}", matched_name, suffix)
                };

                if last_token.is_empty() {
                    self.input_text.push_str(&completed);
                } else {
                    let prefix_len = self.input_text.len() - last_token.len();
                    self.input_text.truncate(prefix_len);
                    self.input_text.push_str(&completed);
                }
            } else if matches.len() > 1 {
                let first = &matches[0].0;
                let mut common_len = first.len();
                for (m, _) in &matches[1..] {
                    common_len = common_len.min(m.len());
                    while !m.starts_with(&first[..common_len]) {
                        common_len -= 1;
                    }
                }
                if common_len > prefix.len() {
                    let common_str = &first[..common_len];
                    let prefix_len = self.input_text.len() - last_token.len();
                    self.input_text.truncate(prefix_len);
                    if last_token.contains('/') {
                        let (dir_part, _) = last_token.rsplit_once('/').unwrap();
                        self.input_text.push_str(&format!("{}/{}", dir_part, common_str));
                    } else {
                        self.input_text.push_str(common_str);
                    }
                } else {
                    let candidate_names: Vec<String> = matches
                        .iter()
                        .map(|(m, is_dir)| if *is_dir { format!("{}/", m) } else { m.clone() })
                        .collect();
                    if let Ok(mut l) = self.logs.lock() {
                        l.push_back(LogEntry::new(candidate_names.join("   "), LogKind::Launcher));
                    }
                }
            }
        }
    }

    pub fn execute_local(&mut self, cmd_str: &str, cwd_override: &str) {
        let trimmed = cmd_str.trim();
        if trimmed.is_empty() {
            let prompt = self.get_prompt_str(None);
            if let Ok(mut l) = self.logs.lock() {
                l.push_back(LogEntry::new(prompt, LogKind::Stdin));
            }
            self.input_text.clear();
            return;
        }

        // Add to history
        if self.history.last().map(|s| s.as_str()) != Some(trimmed) {
            self.history.push(trimmed.to_string());
        }
        self.history_idx = None;
        self.input_text.clear();

        let prompt = self.get_prompt_str(None);
        {
            let mut l = self.logs.lock().unwrap();
            l.push_back(LogEntry::new(format!("{}{}", prompt, trimmed), LogKind::Stdin));
        }

        // Built-in: clear / cls
        if trimmed == "clear" || trimmed == "cls" {
            self.clear();
            if let Ok(mut st) = self.status.lock() {
                *st = TerminalCommandStatus::Idle;
            }
            return;
        }

        // Built-in: pwd
        if trimmed == "pwd" {
            if let Ok(mut l) = self.logs.lock() {
                l.push_back(LogEntry::new(self.cwd.display().to_string(), LogKind::Stdout));
            }
            if let Ok(mut st) = self.status.lock() {
                *st = TerminalCommandStatus::Idle;
            }
            return;
        }

        // Built-in: history
        if trimmed == "history" {
            if let Ok(mut l) = self.logs.lock() {
                for (i, h) in self.history.iter().enumerate() {
                    l.push_back(LogEntry::new(format!("{:4}  {}", i + 1, h), LogKind::Stdout));
                }
            }
            if let Ok(mut st) = self.status.lock() {
                *st = TerminalCommandStatus::Idle;
            }
            return;
        }

        // Built-in: help
        if trimmed == "help" {
            if let Ok(mut l) = self.logs.lock() {
                l.push_back(LogEntry::new("--- Terminal Commands & Shortcuts ---".to_string(), LogKind::Launcher));
                l.push_back(LogEntry::new("  cd <path>    Change current directory (supports ~, .., -)".to_string(), LogKind::Stdout));
                l.push_back(LogEntry::new("  pwd          Print current working directory".to_string(), LogKind::Stdout));
                l.push_back(LogEntry::new("  clear / cls  Clear terminal output".to_string(), LogKind::Stdout));
                l.push_back(LogEntry::new("  history      Show command history".to_string(), LogKind::Stdout));
                l.push_back(LogEntry::new("  Ctrl + C     Interrupt running command (SIGINT) or clear line".to_string(), LogKind::Stdout));
                l.push_back(LogEntry::new("  Ctrl + L     Clear terminal screen".to_string(), LogKind::Stdout));
                l.push_back(LogEntry::new("  Up / Down    Navigate command history".to_string(), LogKind::Stdout));
                l.push_back(LogEntry::new("  Tab          Auto-complete paths and files".to_string(), LogKind::Stdout));
            }
            if let Ok(mut st) = self.status.lock() {
                *st = TerminalCommandStatus::Idle;
            }
            return;
        }

        // Built-in: cd
        if trimmed == "cd" || trimmed.starts_with("cd ") {
            let target_str = if trimmed == "cd" {
                "~"
            } else {
                trimmed["cd ".len()..].trim()
            };

            let target_path = if target_str == "-" {
                if let Some(prev) = &self.prev_cwd {
                    prev.clone()
                } else {
                    if let Ok(mut l) = self.logs.lock() {
                        l.push_back(LogEntry::new("bash: cd: OLDPWD not set".to_string(), LogKind::Stderr));
                    }
                    return;
                }
            } else if target_str.starts_with('~') {
                let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_else(|_| "/".to_string());
                if target_str == "~" {
                    PathBuf::from(home)
                } else {
                    PathBuf::from(home).join(target_str[1..].trim_start_matches('/'))
                }
            } else {
                let p = PathBuf::from(target_str);
                if p.is_absolute() {
                    p
                } else {
                    self.cwd.join(p)
                }
            };

            match std::fs::canonicalize(&target_path) {
                Ok(canonical) => {
                    if canonical.is_dir() {
                        self.prev_cwd = Some(self.cwd.clone());
                        self.cwd = canonical;
                        if let Ok(mut st) = self.status.lock() {
                            *st = TerminalCommandStatus::Idle;
                        }
                    } else {
                        if let Ok(mut l) = self.logs.lock() {
                            l.push_back(LogEntry::new(format!("bash: cd: {}: Not a directory", target_str), LogKind::Stderr));
                        }
                        if let Ok(mut st) = self.status.lock() {
                            *st = TerminalCommandStatus::Failed("Not a directory".to_string());
                        }
                    }
                }
                Err(_) => {
                    if let Ok(mut l) = self.logs.lock() {
                        l.push_back(LogEntry::new(format!("bash: cd: {}: No such file or directory", target_str), LogKind::Stderr));
                    }
                    if let Ok(mut st) = self.status.lock() {
                        *st = TerminalCommandStatus::Failed("No such file or directory".to_string());
                    }
                }
            }
            return;
        }

        if let Ok(mut st) = self.status.lock() {
            *st = TerminalCommandStatus::Running(trimmed.to_string());
        }

        #[cfg(target_os = "windows")]
        let mut command = {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(trimmed);
            c
        };

        #[cfg(not(target_os = "windows"))]
        let mut command = {
            let mut c = Command::new("bash");
            c.arg("-c").arg(trimmed);
            c
        };

        let run_dir = if !cwd_override.trim().is_empty() {
            PathBuf::from(cwd_override.trim())
        } else {
            self.cwd.clone()
        };
        command.current_dir(run_dir);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let logs_arc = Arc::clone(&self.logs);
        let status_arc = Arc::clone(&self.status);

        match command.spawn() {
            Ok(child) => {
                let pid = child.id();
                self.running_pid = Some(pid);
                let child_arc = Arc::new(Mutex::new(Some(child)));
                self.running_child = Some(Arc::clone(&child_arc));

                let stdin = {
                    let mut lock = child_arc.lock().unwrap();
                    lock.as_mut().and_then(|c| c.stdin.take())
                };
                if let Some(sin) = stdin {
                    self.stdin_writer = Some(Arc::new(Mutex::new(sin)));
                }

                let stdout = {
                    let mut lock = child_arc.lock().unwrap();
                    lock.as_mut().and_then(|c| c.stdout.take())
                };
                let stderr = {
                    let mut lock = child_arc.lock().unwrap();
                    lock.as_mut().and_then(|c| c.stderr.take())
                };

                let logs_out = Arc::clone(&logs_arc);
                if let Some(out) = stdout {
                    thread::spawn(move || {
                        let reader = BufReader::new(out);
                        for line in reader.lines() {
                            if let Ok(l) = line {
                                let mut q = logs_out.lock().unwrap();
                                q.push_back(LogEntry::new(l, LogKind::Stdout));
                                if q.len() > 2000 {
                                    q.pop_front();
                                }
                            }
                        }
                    });
                }

                let logs_err = Arc::clone(&logs_arc);
                if let Some(err) = stderr {
                    thread::spawn(move || {
                        let reader = BufReader::new(err);
                        for line in reader.lines() {
                            if let Ok(l) = line {
                                let mut q = logs_err.lock().unwrap();
                                q.push_back(LogEntry::new(l, LogKind::Stderr));
                                if q.len() > 2000 {
                                    q.pop_front();
                                }
                            }
                        }
                    });
                }

                let status_clone = Arc::clone(&status_arc);
                let child_wait = Arc::clone(&child_arc);
                thread::spawn(move || {
                    let exit = {
                        let mut lock = child_wait.lock().unwrap();
                        if let Some(c) = lock.as_mut() {
                            c.wait().ok()
                        } else {
                            None
                        }
                    };

                    if let Some(res) = exit {
                        let code = res.code().unwrap_or(if res.success() { 0 } else { 1 });
                        if let Ok(mut st) = status_clone.lock() {
                            if res.success() {
                                *st = TerminalCommandStatus::Success(code);
                            } else {
                                *st = TerminalCommandStatus::Failed(format!("Exit code {}", code));
                            }
                        }
                    }
                });
            }
            Err(e) => {
                let mut l = logs_arc.lock().unwrap();
                l.push_back(LogEntry::new(format!("Command spawn error: {}", e), LogKind::Error));
                if let Ok(mut st) = status_arc.lock() {
                    *st = TerminalCommandStatus::Failed(format!("Error: {}", e));
                }
            }
        }
    }

    pub fn execute_remote(&mut self, host: &SshRemoteHost, cmd_str: &str) {
        let trimmed = cmd_str.trim();
        if trimmed.is_empty() {
            let prompt = self.get_prompt_str(Some(host));
            if let Ok(mut l) = self.logs.lock() {
                l.push_back(LogEntry::new(prompt, LogKind::Stdin));
            }
            self.input_text.clear();
            return;
        }

        if trimmed == "clear" || trimmed == "cls" {
            self.clear();
            if let Ok(mut st) = self.status.lock() {
                *st = TerminalCommandStatus::Idle;
            }
            return;
        }

        // Add to history
        if self.history.last().map(|s| s.as_str()) != Some(trimmed) {
            self.history.push(trimmed.to_string());
        }
        self.history_idx = None;
        self.input_text.clear();

        // Log prompt
        let prompt = self.get_prompt_str(Some(host));
        {
            let mut l = self.logs.lock().unwrap();
            l.push_back(LogEntry::new(format!("{}{}", prompt, trimmed), LogKind::Stdin));
        }

        if let Ok(mut st) = self.status.lock() {
            *st = TerminalCommandStatus::Running(trimmed.to_string());
        }

        let mut command = build_ssh_command(host, trimmed);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let logs_arc = Arc::clone(&self.logs);
        let status_arc = Arc::clone(&self.status);

        match command.spawn() {
            Ok(child) => {
                let pid = child.id();
                self.running_pid = Some(pid);
                let child_arc = Arc::new(Mutex::new(Some(child)));
                self.running_child = Some(Arc::clone(&child_arc));

                let stdin = {
                    let mut lock = child_arc.lock().unwrap();
                    lock.as_mut().and_then(|c| c.stdin.take())
                };
                if let Some(sin) = stdin {
                    self.stdin_writer = Some(Arc::new(Mutex::new(sin)));
                }

                let stdout = {
                    let mut lock = child_arc.lock().unwrap();
                    lock.as_mut().and_then(|c| c.stdout.take())
                };
                let stderr = {
                    let mut lock = child_arc.lock().unwrap();
                    lock.as_mut().and_then(|c| c.stderr.take())
                };

                let logs_out = Arc::clone(&logs_arc);
                if let Some(out) = stdout {
                    thread::spawn(move || {
                        let reader = BufReader::new(out);
                        for line in reader.lines() {
                            if let Ok(l) = line {
                                let mut q = logs_out.lock().unwrap();
                                q.push_back(LogEntry::new(l, LogKind::Stdout));
                                if q.len() > 2000 {
                                    q.pop_front();
                                }
                            }
                        }
                    });
                }

                let logs_err = Arc::clone(&logs_arc);
                if let Some(err) = stderr {
                    thread::spawn(move || {
                        let reader = BufReader::new(err);
                        for line in reader.lines() {
                            if let Ok(l) = line {
                                let mut q = logs_err.lock().unwrap();
                                q.push_back(LogEntry::new(l, LogKind::Stderr));
                                if q.len() > 2000 {
                                    q.pop_front();
                                }
                            }
                        }
                    });
                }

                let status_clone = Arc::clone(&status_arc);
                let child_wait = Arc::clone(&child_arc);
                thread::spawn(move || {
                    let exit = {
                        let mut lock = child_wait.lock().unwrap();
                        if let Some(c) = lock.as_mut() {
                            c.wait().ok()
                        } else {
                            None
                        }
                    };

                    if let Some(res) = exit {
                        let code = res.code().unwrap_or(if res.success() { 0 } else { 1 });
                        if let Ok(mut st) = status_clone.lock() {
                            if res.success() {
                                *st = TerminalCommandStatus::Success(code);
                            } else {
                                *st = TerminalCommandStatus::Failed(format!("Exit code {}", code));
                            }
                        }
                    }
                });
            }
            Err(e) => {
                let mut l = logs_arc.lock().unwrap();
                l.push_back(LogEntry::new(format!("SSH spawn error: {}", e), LogKind::Error));
                if let Ok(mut st) = status_arc.lock() {
                    *st = TerminalCommandStatus::Failed(format!("Error: {}", e));
                }
            }
        }
    }
}

pub fn open_system_terminal(cwd: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("cmd.exe")
            .current_dir(cwd)
            .spawn()
            .map_err(|e| format!("Failed to launch cmd.exe: {}", e))?;
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let terms = ["x-terminal-emulator", "gnome-terminal", "konsole", "alacritty", "kitty", "terminator", "xfce4-terminal", "xterm"];
        for t in terms {
            if Command::new("which").arg(t).output().map(|o| o.status.success()).unwrap_or(false) {
                let res = Command::new(t).current_dir(cwd).spawn();
                if res.is_ok() {
                    return Ok(());
                }
            }
        }
        Err("No supported external terminal found (gnome-terminal, alacritty, kitty, xterm)".to_string())
    }
}

pub fn launch_external_command(cmd_str: &str, cwd: &Path) -> Result<(), String> {
    let script_path = std::env::temp_dir().join(format!(
        "launcher_cmd_{}.sh",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    let script_content = format!(
r#"#!/usr/bin/env bash
echo "=========================================================="
echo " Server Launcher: Local Command Execution"
echo " Command: {cmd}"
echo "=========================================================="
cd "{cwd}"
{cmd}
EXIT_CODE=$?
echo ""
echo "[launcher] Command finished with exit code $EXIT_CODE."
read -n 1 -s -r -p "Press any key to close this terminal..."
"#,
        cmd = cmd_str,
        cwd = cwd.display()
    );

    let _ = std::fs::write(&script_path, script_content);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(&script_path) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&script_path, perms);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let script_str = script_path.to_string_lossy().to_string();
        let chosen = crate::service::find_terminal();
        let mut cmd;
        if let Some((term, args)) = chosen {
            cmd = std::process::Command::new(term);
            for arg in args {
                cmd.arg(arg);
            }
            cmd.arg(&script_str);
        } else {
            cmd = std::process::Command::new("x-terminal-emulator");
            cmd.args(&["-e", &script_str]);
        }
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
        let mut cmd = std::process::Command::new("cmd.exe");
        cmd.args(&["/K", cmd_str]);
        cmd.current_dir(cwd);
        cmd.spawn().map_err(|e| format!("Failed to spawn cmd: {}", e))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommandModalState {
    pub open: bool,
    pub is_edit: bool,
    pub id: String,
    pub name: String,
    pub command: String,
    pub description: String,
    pub category: String,
    pub host_id: Option<String>,
    pub run_in_terminal: bool,
    pub error: Option<String>,
}

impl CommandModalState {
    pub fn open_create(&mut self, current_host: Option<String>) {
        self.open = true;
        self.is_edit = false;
        self.id = format!("cmd_{}", chrono::Utc::now().timestamp_millis());
        self.name = String::new();
        self.command = String::new();
        self.description = String::new();
        self.category = "General".to_string();
        self.host_id = current_host;
        self.run_in_terminal = false;
        self.error = None;
    }

    pub fn open_edit(&mut self, cmd: &SavedCommand) {
        self.open = true;
        self.is_edit = true;
        self.id = cmd.id.clone();
        self.name = cmd.name.clone();
        self.command = cmd.command.clone();
        self.description = cmd.description.clone();
        self.category = cmd.category.clone();
        self.host_id = cmd.host_id.clone();
        self.run_in_terminal = cmd.run_in_terminal;
        self.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_session_default() {
        let mut term = TerminalSession::default();
        assert!(term.history.is_empty());
        assert!(term.input_text.is_empty());
        assert_eq!(*term.status.lock().unwrap(), TerminalCommandStatus::Idle);
        assert!(!term.is_running());
        assert!(term.cwd.exists());

        term.clear();
        assert!(term.logs.lock().unwrap().is_empty());
    }

    #[test]
    fn test_terminal_builtins_clear_pwd() {
        let mut term = TerminalSession::default();
        term.execute_local("echo hello", "");
        assert_eq!(term.history.len(), 1);

        term.execute_local("clear", "");
        assert!(term.logs.lock().unwrap().is_empty());

        term.execute_local("pwd", "");
        let logs = term.logs.lock().unwrap();
        assert!(logs.iter().any(|l| l.text.contains(&term.cwd.display().to_string())));
    }

    #[test]
    fn test_terminal_cd_navigation() {
        let mut term = TerminalSession::default();
        let original_cwd = term.cwd.clone();

        term.execute_local("cd ..", "");
        assert_ne!(term.cwd, original_cwd);
        assert!(term.cwd.is_dir());

        term.execute_local("cd -", "");
        assert_eq!(term.cwd, original_cwd);
    }

    #[test]
    fn test_terminal_prompt_str() {
        let term = TerminalSession::default();
        let prompt = term.get_prompt_str(None);
        assert!(prompt.ends_with("$ "));
        assert!(prompt.contains('@'));
    }

    #[test]
    fn test_command_modal_state() {
        let mut state = CommandModalState::default();
        state.open_create(None);
        assert!(state.open);
        assert!(!state.is_edit);
        assert!(!state.run_in_terminal);
        assert_eq!(state.category, "General");

        let cmd = SavedCommand {
            id: "cmd_1".to_string(),
            name: "Git Status".to_string(),
            command: "git status".to_string(),
            description: "Show repository status".to_string(),
            category: "Git".to_string(),
            host_id: None,
            run_in_terminal: true,
        };

        state.open_edit(&cmd);
        assert!(state.is_edit);
        assert_eq!(state.name, "Git Status");
        assert_eq!(state.command, "git status");
        assert_eq!(state.category, "Git");
        assert!(state.run_in_terminal);
    }

    #[test]
    fn test_parse_command_json_files() {
        let fedora_path = std::path::Path::new("../fedora_commands.json");
        if fedora_path.exists() {
            let content = std::fs::read_to_string(fedora_path).expect("Read fedora_commands.json");
            let cmds: Vec<SavedCommand> = serde_json::from_str(&content).expect("Parse fedora_commands.json");
            assert!(!cmds.is_empty(), "fedora_commands.json should have commands");
            for c in &cmds {
                assert!(!c.name.is_empty());
                assert!(c.command.starts_with("cd "), "Command '{}' should direct to specific location", c.name);
            }
        }

        let ubuntu_path = std::path::Path::new("../ubuntu_commands.json");
        if ubuntu_path.exists() {
            let content = std::fs::read_to_string(ubuntu_path).expect("Read ubuntu_commands.json");
            let cmds: Vec<SavedCommand> = serde_json::from_str(&content).expect("Parse ubuntu_commands.json");
            assert!(!cmds.is_empty(), "ubuntu_commands.json should have commands");
            for c in &cmds {
                assert!(!c.name.is_empty());
                assert!(c.command.starts_with("cd "), "Command '{}' should direct to specific location", c.name);
            }
        }
    }
}
