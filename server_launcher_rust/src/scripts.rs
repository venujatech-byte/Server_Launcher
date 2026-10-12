use crate::config::{CustomScript, SshRemoteHost};
use crate::remote::build_ssh_command;
use crate::service::{LogEntry, LogKind};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptRunStatus {
    Idle,
    Running,
    Success(i32),
    Failed(String),
}

pub struct ScriptExecution {
    pub script_id: String,
    pub status: Arc<Mutex<ScriptRunStatus>>,
    pub logs: Arc<Mutex<VecDeque<LogEntry>>>,
    pub pid: Option<u32>,
    pub started_at: Option<Instant>,
    pub cancel_flag: Arc<AtomicBool>,
    pub local_child: Option<Arc<Mutex<Option<Child>>>>,
    pub askpass_dir: Option<std::path::PathBuf>,
}

impl ScriptExecution {
    pub fn new(script_id: String) -> Self {
        Self {
            script_id,
            status: Arc::new(Mutex::new(ScriptRunStatus::Idle)),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            pid: None,
            started_at: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            local_child: None,
            askpass_dir: None,
        }
    }

    pub fn stop(&mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        if let Some(pid) = self.pid {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
                libc::kill(pid as i32, libc::SIGKILL);
            }
        }
        if let Some(child_arc) = &self.local_child {
            if let Ok(mut lock) = child_arc.lock() {
                if let Some(child) = lock.as_mut() {
                    let _ = child.kill();
                }
            }
        }
        if let Some(dir) = self.askpass_dir.take() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        if let Ok(mut st) = self.status.lock() {
            if *st == ScriptRunStatus::Running {
                *st = ScriptRunStatus::Failed("Stopped by user".to_string());
            }
        }
    }
}

pub fn which_sudo() -> Option<String> {
    for p in &["/usr/bin/sudo", "/bin/sudo", "/usr/local/bin/sudo", "/run/wrappers/bin/sudo"] {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    None
}

/// Detects whether a script body contains invocations of sudo, pkexec, or doas.
pub fn script_requires_sudo(script_body: &str) -> bool {
    for line in script_body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }

        let mut in_quote = None;
        let mut current_token = String::new();
        let mut tokens = Vec::new();

        for c in trimmed.chars() {
            if let Some(q) = in_quote {
                if c == q {
                    in_quote = None;
                }
            } else if c == '"' || c == '\'' {
                in_quote = Some(c);
            } else if c.is_whitespace() || c == ';' || c == '&' || c == '|' || c == '(' || c == ')' || c == '`' {
                if !current_token.is_empty() {
                    tokens.push(std::mem::take(&mut current_token));
                }
            } else {
                current_token.push(c);
            }
        }
        if !current_token.is_empty() {
            tokens.push(current_token);
        }

        for token in tokens {
            if token == "sudo" || token.ends_with("/sudo") || token == "pkexec" || token == "doas" {
                return true;
            }
        }
    }
    false
}

/// Checks whether local `sudo` requires authentication by running `sudo -n true`.
/// Returns true if password is required (or sudo fails), false if credentials are cached/passwordless.
pub fn check_local_sudo_needs_password() -> bool {
    #[cfg(unix)]
    {
        let output = Command::new("sudo").arg("-n").arg("true").output();
        match output {
            Ok(out) => !out.status.success(),
            Err(_) => true,
        }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Attempts to validate a sudo password locally using `sudo -S -k -v`.
pub fn validate_local_sudo_password(password: &str) -> Result<(), String> {
    if password.is_empty() {
        return Err("Password cannot be empty.".to_string());
    }

    #[cfg(unix)]
    {
        use std::io::Write;

        let mut child = Command::new("sudo")
            .args(&["-S", "-k", "-v"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to invoke sudo: {}", e))?;

        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "{}", password);
        }

        let output = child.wait_with_output().map_err(|e| format!("Wait failed: {}", e))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr_str = String::from_utf8_lossy(&output.stderr);
            if stderr_str.contains("incorrect password")
                || stderr_str.contains("authentication failure")
                || stderr_str.contains("Sorry, try again")
                || stderr_str.contains("Password:")
            {
                Err("Incorrect sudo password. Please try again.".to_string())
            } else if stderr_str.contains("no new privileges") {
                // In restricted sandbox/container environment, allow execution to proceed
                Ok(())
            } else {
                let msg = stderr_str.trim();
                if msg.is_empty() {
                    Err("Sudo authentication failed.".to_string())
                } else {
                    Err(msg.to_string())
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = password;
        Ok(())
    }
}

pub fn execute_local_script(
    script: &CustomScript,
    execution: &mut ScriptExecution,
    sudo_password: Option<&str>,
) {
    execution.stop();
    execution.logs = Arc::new(Mutex::new(VecDeque::new()));
    execution.cancel_flag = Arc::new(AtomicBool::new(false));
    let status_arc = Arc::clone(&execution.status);
    let logs_arc = Arc::clone(&execution.logs);
    let _cancel_flag = Arc::clone(&execution.cancel_flag);

    if let Ok(mut st) = status_arc.lock() {
        *st = ScriptRunStatus::Running;
    }
    execution.started_at = Some(Instant::now());

    let shell = if script.shell.trim().is_empty() {
        "bash"
    } else {
        script.shell.trim()
    };

    let askpass_setup = if let Some(pwd) = sudo_password {
        if !pwd.is_empty() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let dir = std::env::temp_dir().join(format!(
                    ".sl_askpass_{}_{}",
                    std::process::id(),
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
                ));
                if std::fs::create_dir_all(&dir).is_ok() {
                    let askpass_path = dir.join("askpass");
                    let askpass_content = format!(
                        "#!/bin/sh\ncat << '__LAUNCHER_SUDO_PASS_EOF__'\n{}\n__LAUNCHER_SUDO_PASS_EOF__\n",
                        pwd
                    );
                    if std::fs::write(&askpass_path, askpass_content).is_ok() {
                        let _ = std::fs::set_permissions(&askpass_path, std::fs::Permissions::from_mode(0o700));

                        let sudo_wrapper_path = dir.join("sudo");
                        let real_sudo = which_sudo().unwrap_or_else(|| "/usr/bin/sudo".to_string());
                        let sudo_wrapper_content = format!(
                            "#!/bin/sh\nexport SUDO_ASKPASS='{}'\nexec \"{}\" -A \"$@\"\n",
                            askpass_path.display(),
                            real_sudo
                        );
                        let _ = std::fs::write(&sudo_wrapper_path, sudo_wrapper_content);
                        let _ = std::fs::set_permissions(&sudo_wrapper_path, std::fs::Permissions::from_mode(0o700));
                        Some((dir, askpass_path))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            #[cfg(not(unix))]
            {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let mut cmd = Command::new(shell);
    let script_cmd = if askpass_setup.is_some() && (shell == "bash" || shell == "sh" || shell == "zsh") {
        format!("shopt -s expand_aliases 2>/dev/null; alias sudo='sudo -A'; {}", script.script)
    } else {
        script.script.clone()
    };

    cmd.arg("-c").arg(&script_cmd);

    let cwd = script.cwd.trim();
    if !cwd.is_empty() {
        cmd.current_dir(cwd);
    }

    if let Some((ref dir, ref askpass)) = askpass_setup {
        execution.askpass_dir = Some(dir.clone());
        cmd.env("SUDO_ASKPASS", askpass.to_string_lossy().as_ref());
        let current_path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}:{}", dir.display(), current_path));
        if std::env::var("DISPLAY").is_err() {
            cmd.env("DISPLAY", ":0");
        }
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    match cmd.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            execution.pid = Some(pid);
            let child_arc = Arc::new(Mutex::new(None));
            execution.local_child = Some(Arc::clone(&child_arc));

            let stdout = child.stdout.take();
            let stderr = child.stderr.take();

            {
                let mut l = logs_arc.lock().unwrap();
                l.push_back(LogEntry::new(
                    format!("▶ Starting script '{}' (PID: {}, shell: {})...", script.name, pid, shell),
                    LogKind::Launcher,
                ));
            }

            // Spawn thread to read stdout
            let logs_out = Arc::clone(&logs_arc);
            if let Some(out) = stdout {
                thread::spawn(move || {
                    let reader = BufReader::new(out);
                    for line in reader.lines() {
                        if let Ok(l) = line {
                            let mut queue = logs_out.lock().unwrap();
                            queue.push_back(LogEntry::new(l, LogKind::Stdout));
                            if queue.len() > 10000 {
                                queue.pop_front();
                            }
                        }
                    }
                });
            }

            // Spawn thread to read stderr
            let logs_err = Arc::clone(&logs_arc);
            if let Some(err) = stderr {
                thread::spawn(move || {
                    let reader = BufReader::new(err);
                    for line in reader.lines() {
                        if let Ok(l) = line {
                            let mut queue = logs_err.lock().unwrap();
                            queue.push_back(LogEntry::new(l, LogKind::Stderr));
                            if queue.len() > 10000 {
                                queue.pop_front();
                            }
                        }
                    }
                });
            }

            // Spawn watcher thread for process exit
            let status_clone = Arc::clone(&status_arc);
            let logs_exit = Arc::clone(&logs_arc);
            let script_name = script.name.clone();
            let cleanup_dir = askpass_setup.as_ref().map(|(d, _)| d.clone());
            thread::spawn(move || {
                match child.wait() {
                    Ok(exit) => {
                        let code = exit.code().unwrap_or(if exit.success() { 0 } else { 1 });
                        let mut l = logs_exit.lock().unwrap();
                        if exit.success() {
                            l.push_back(LogEntry::new(
                                format!("✔ Script '{}' completed successfully (exit code: {}).", script_name, code),
                                LogKind::Launcher,
                            ));
                            if let Ok(mut st) = status_clone.lock() {
                                *st = ScriptRunStatus::Success(code);
                            }
                        } else {
                            l.push_back(LogEntry::new(
                                format!("✘ Script '{}' exited with status {}.", script_name, code),
                                LogKind::Error,
                            ));
                            if let Ok(mut st) = status_clone.lock() {
                                *st = ScriptRunStatus::Failed(format!("Exited with code {}", code));
                            }
                        }
                    }
                    Err(e) => {
                        let mut l = logs_exit.lock().unwrap();
                        l.push_back(LogEntry::new(
                            format!("✘ Script wait error: {}", e),
                            LogKind::Error,
                        ));
                        if let Ok(mut st) = status_clone.lock() {
                            *st = ScriptRunStatus::Failed(format!("Wait error: {}", e));
                        }
                    }
                }
                if let Some(dir) = cleanup_dir {
                    let _ = std::fs::remove_dir_all(&dir);
                }
            });
        }
        Err(e) => {
            if let Some((dir, _)) = askpass_setup {
                let _ = std::fs::remove_dir_all(&dir);
            }
            let mut l = logs_arc.lock().unwrap();
            l.push_back(LogEntry::new(
                format!("✘ Failed to spawn script '{}': {}", script.name, e),
                LogKind::Error,
            ));
            if let Ok(mut st) = status_arc.lock() {
                *st = ScriptRunStatus::Failed(format!("Spawn error: {}", e));
            }
        }
    }
}

pub fn execute_remote_script(
    host: &SshRemoteHost,
    script: &CustomScript,
    execution: &mut ScriptExecution,
    sudo_password: Option<&str>,
) {
    execution.stop();
    execution.logs = Arc::new(Mutex::new(VecDeque::new()));
    execution.cancel_flag = Arc::new(AtomicBool::new(false));
    let status_arc = Arc::clone(&execution.status);
    let logs_arc = Arc::clone(&execution.logs);

    if let Ok(mut st) = status_arc.lock() {
        *st = ScriptRunStatus::Running;
    }
    execution.started_at = Some(Instant::now());

    let shell = if script.shell.trim().is_empty() {
        "bash"
    } else {
        script.shell.trim()
    };

    let effective_password = sudo_password.or_else(|| {
        if !host.password.trim().is_empty() {
            Some(host.password.trim())
        } else {
            None
        }
    });

    let remote_wrapper = if let Some(pwd) = effective_password {
        if script_requires_sudo(&script.script) && !pwd.is_empty() {
            format!(
                "AP=$(mktemp 2>/dev/null || echo /tmp/.ap_$$); printf '#!/bin/sh\\ncat << \"__LAUNCHER_SUDO_PASS_EOF__\"\\n%s\\n__LAUNCHER_SUDO_PASS_EOF__\\n' {} > \"$AP\" && chmod 700 \"$AP\" && export SUDO_ASKPASS=\"$AP\" && shopt -s expand_aliases 2>/dev/null; alias sudo='sudo -A'; {} -c {}; RET=$?; rm -f \"$AP\"; exit $RET",
                shlex_quote(pwd),
                shell,
                shlex_quote(&script.script)
            )
        } else {
            format!("{} -c {}", shell, shlex_quote(&script.script))
        }
    } else {
        format!("{} -c {}", shell, shlex_quote(&script.script))
    };

    let mut cmd = build_ssh_command(host, &remote_wrapper);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    match cmd.spawn() {
        Ok(child) => {
            let pid = child.id();
            execution.pid = Some(pid);
            let child_arc = Arc::new(Mutex::new(Some(child)));
            execution.local_child = Some(Arc::clone(&child_arc));

            let stdout = {
                let mut lock = child_arc.lock().unwrap();
                lock.as_mut().and_then(|c| c.stdout.take())
            };
            let stderr = {
                let mut lock = child_arc.lock().unwrap();
                lock.as_mut().and_then(|c| c.stderr.take())
            };

            {
                let mut l = logs_arc.lock().unwrap();
                l.push_back(LogEntry::new(
                    format!(
                        "▶ Running remote script '{}' on {}@{}:{}...",
                        script.name, host.user, host.host, host.port
                    ),
                    LogKind::Launcher,
                ));
            }

            let logs_out = Arc::clone(&logs_arc);
            if let Some(out) = stdout {
                thread::spawn(move || {
                    let reader = BufReader::new(out);
                    for line in reader.lines() {
                        if let Ok(l) = line {
                            let mut queue = logs_out.lock().unwrap();
                            queue.push_back(LogEntry::new(l, LogKind::Stdout));
                            if queue.len() > 10000 {
                                queue.pop_front();
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
                            let mut queue = logs_err.lock().unwrap();
                            queue.push_back(LogEntry::new(l, LogKind::Stderr));
                            if queue.len() > 10000 {
                                queue.pop_front();
                            }
                        }
                    }
                });
            }

            let status_clone = Arc::clone(&status_arc);
            let logs_exit = Arc::clone(&logs_arc);
            let script_name = script.name.clone();
            let child_arc_wait = Arc::clone(&child_arc);
            thread::spawn(move || {
                let res = {
                    let mut lock = child_arc_wait.lock().unwrap();
                    if let Some(child) = lock.as_mut() {
                        child.wait()
                    } else {
                        return;
                    }
                };

                match res {
                    Ok(exit) => {
                        let code = exit.code().unwrap_or(if exit.success() { 0 } else { 1 });
                        let mut l = logs_exit.lock().unwrap();
                        if exit.success() {
                            l.push_back(LogEntry::new(
                                format!("✔ Remote script '{}' completed successfully (exit code: {}).", script_name, code),
                                LogKind::Launcher,
                            ));
                            if let Ok(mut st) = status_clone.lock() {
                                *st = ScriptRunStatus::Success(code);
                            }
                        } else {
                            l.push_back(LogEntry::new(
                                format!("✘ Remote script '{}' finished with code {}.", script_name, code),
                                LogKind::Error,
                            ));
                            if let Ok(mut st) = status_clone.lock() {
                                *st = ScriptRunStatus::Failed(format!("Exited with code {}", code));
                            }
                        }
                    }
                    Err(e) => {
                        let mut l = logs_exit.lock().unwrap();
                        l.push_back(LogEntry::new(
                            format!("✘ Remote script wait error: {}", e),
                            LogKind::Error,
                        ));
                        if let Ok(mut st) = status_clone.lock() {
                            *st = ScriptRunStatus::Failed(format!("Wait error: {}", e));
                        }
                    }
                }
            });
        }
        Err(e) => {
            let mut l = logs_arc.lock().unwrap();
            l.push_back(LogEntry::new(
                format!("✘ Failed to initiate remote script '{}': {}", script.name, e),
                LogKind::Error,
            ));
            if let Ok(mut st) = status_arc.lock() {
                *st = ScriptRunStatus::Failed(format!("SSH spawn error: {}", e));
            }
        }
    }
}

pub fn shlex_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

#[derive(Debug, Clone, Default)]
pub struct ScriptModalState {
    pub open: bool,
    pub is_edit: bool,
    pub id: String,
    pub name: String,
    pub shell: String,
    pub cwd: String,
    pub script_body: String,
    pub autostart: bool,
    pub host_id: Option<String>,
    pub error: Option<String>,
}

impl ScriptModalState {
    pub fn open_create(&mut self, current_host: Option<String>) {
        self.open = true;
        self.is_edit = false;
        self.id = format!("script_{}", chrono::Utc::now().timestamp_millis());
        self.name = String::new();
        self.shell = "bash".to_string();
        self.cwd = String::new();
        self.script_body = "#!/bin/bash\necho \"Running custom script...\"\n".to_string();
        self.autostart = false;
        self.host_id = current_host;
        self.error = None;
    }

    pub fn open_edit(&mut self, script: &CustomScript) {
        self.open = true;
        self.is_edit = true;
        self.id = script.id.clone();
        self.name = script.name.clone();
        self.shell = script.shell.clone();
        self.cwd = script.cwd.clone();
        self.script_body = script.script.clone();
        self.autostart = script.autostart;
        self.host_id = script.host_id.clone();
        self.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shlex_quote() {
        assert_eq!(shlex_quote("hello"), "'hello'");
        assert_eq!(shlex_quote("don't fail"), "'don'\"'\"'t fail'");
    }

    #[test]
    fn test_script_modal_state_create_and_edit() {
        let mut state = ScriptModalState::default();
        state.open_create(Some("remote_1".to_string()));
        assert!(state.open);
        assert!(!state.is_edit);
        assert_eq!(state.host_id, Some("remote_1".to_string()));
        assert_eq!(state.shell, "bash");

        let script = CustomScript {
            id: "s1".to_string(),
            name: "Test Script".to_string(),
            script: "echo 123".to_string(),
            shell: "python3".to_string(),
            cwd: "/tmp".to_string(),
            autostart: true,
            host_id: None,
        };

        state.open_edit(&script);
        assert!(state.is_edit);
        assert_eq!(state.name, "Test Script");
        assert_eq!(state.shell, "python3");
        assert_eq!(state.cwd, "/tmp");
        assert!(state.autostart);
    }

    #[test]
    fn test_script_requires_sudo() {
        assert!(script_requires_sudo("sudo apt update"));
        assert!(script_requires_sudo("sudo -E systemctl restart nginx"));
        assert!(script_requires_sudo("echo 'hello' | sudo tee /etc/issue"));
        assert!(script_requires_sudo("cd /var/www && sudo chown -R www-data:www-data ."));
        assert!(script_requires_sudo("/usr/bin/sudo ls"));
        assert!(script_requires_sudo("pkexec gparted"));
        assert!(script_requires_sudo("doas reboot"));

        // Negative cases
        assert!(!script_requires_sudo("ls -la"));
        assert!(!script_requires_sudo("echo 'sudo is not executed here'"));
        assert!(!script_requires_sudo("# sudo apt update"));
        assert!(!script_requires_sudo("echo \"don't run sudo\""));
    }
}
