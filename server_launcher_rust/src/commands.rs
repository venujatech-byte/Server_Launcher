use crate::config::{SavedCommand, SshRemoteHost};
use crate::remote::build_ssh_command;
use crate::service::{LogEntry, LogKind};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
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
    pub auto_scroll: bool,
}

impl Default for TerminalSession {
    fn default() -> Self {
        Self {
            logs: Arc::new(Mutex::new(VecDeque::new())),
            input_text: String::new(),
            history: Vec::new(),
            history_idx: None,
            status: Arc::new(Mutex::new(TerminalCommandStatus::Idle)),
            running_child: None,
            running_pid: None,
            auto_scroll: true,
        }
    }
}

impl TerminalSession {
    pub fn clear(&mut self) {
        if let Ok(mut l) = self.logs.lock() {
            l.clear();
        }
    }

    pub fn interrupt(&mut self) {
        if let Some(pid) = self.running_pid {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGINT);
                libc::kill(pid as i32, libc::SIGTERM);
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
                *st = TerminalCommandStatus::Failed("Interrupted by user".to_string());
            }
        }
        if let Ok(mut l) = self.logs.lock() {
            l.push_back(LogEntry::new("⏹ Process interrupted (SIGINT)".to_string(), LogKind::Warning));
        }
        self.running_child = None;
        self.running_pid = None;
    }

    pub fn execute_local(&mut self, cmd_str: &str, cwd: &str) {
        let trimmed = cmd_str.trim();
        if trimmed.is_empty() {
            return;
        }

        // Add to history
        if self.history.last().map(|s| s.as_str()) != Some(trimmed) {
            self.history.push(trimmed.to_string());
        }
        self.history_idx = None;
        self.input_text.clear();

        // Log command prompt
        {
            let mut l = self.logs.lock().unwrap();
            l.push_back(LogEntry::new(format!("$ {}", trimmed), LogKind::Stdin));
        }

        if let Ok(mut st) = self.status.lock() {
            *st = TerminalCommandStatus::Running(trimmed.to_string());
        }

        let mut command = Command::new("bash");
        command.arg("-c").arg(trimmed);
        if !cwd.trim().is_empty() {
            command.current_dir(cwd.trim());
        }
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
                                if q.len() > 10000 {
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
                                if q.len() > 10000 {
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
            return;
        }

        // Add to history
        if self.history.last().map(|s| s.as_str()) != Some(trimmed) {
            self.history.push(trimmed.to_string());
        }
        self.history_idx = None;
        self.input_text.clear();

        // Log prompt
        {
            let mut l = self.logs.lock().unwrap();
            l.push_back(LogEntry::new(format!("{}@{}:$ {}", host.user, host.host, trimmed), LogKind::Stdin));
        }

        if let Ok(mut st) = self.status.lock() {
            *st = TerminalCommandStatus::Running(trimmed.to_string());
        }

        let mut command = build_ssh_command(host, trimmed);
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
                                if q.len() > 10000 {
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
                                if q.len() > 10000 {
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

        term.clear();
        assert!(term.logs.lock().unwrap().is_empty());
    }

    #[test]
    fn test_command_modal_state() {
        let mut state = CommandModalState::default();
        state.open_create(None);
        assert!(state.open);
        assert!(!state.is_edit);
        assert_eq!(state.category, "General");

        let cmd = SavedCommand {
            id: "cmd_1".to_string(),
            name: "Git Status".to_string(),
            command: "git status".to_string(),
            description: "Show repository status".to_string(),
            category: "Git".to_string(),
            host_id: None,
        };

        state.open_edit(&cmd);
        assert!(state.is_edit);
        assert_eq!(state.name, "Git Status");
        assert_eq!(state.command, "git status");
        assert_eq!(state.category, "Git");
    }
}
