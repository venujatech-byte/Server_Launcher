use crate::config::SshRemoteHost;
use std::collections::BTreeSet;
use std::process::Command;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct RemoteListener {
    pub proto: String,
    pub port: u16,
    pub name: String,
    pub pid: Option<u32>,
    #[serde(default)]
    pub start_marker: Option<u64>,
    pub cmd: String,
    #[serde(default)]
    pub cpu_pct: Option<f32>,
    #[serde(default)]
    pub mem_mb: Option<f32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct RemoteStoppedServer {
    pub name: String,
    pub port: u16,
    pub proto: String,
    pub last_pid: Option<u32>,
    pub cmd: String,
    pub start_cmd: String,
    #[serde(default)]
    pub cwd: String,
}

pub fn clean_service_name(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.contains("nginx") {
        "nginx".to_string()
    } else if lower.contains("apache") || lower.contains("httpd") {
        "apache2".to_string()
    } else if lower.contains("sshd") || lower == "ssh" {
        "sshd".to_string()
    } else if lower.contains("mysql") {
        "mysql".to_string()
    } else if lower.contains("mariadb") {
        "mariadb".to_string()
    } else if lower.contains("postgres") {
        "postgresql".to_string()
    } else if lower.contains("redis") {
        "redis-server".to_string()
    } else if lower.contains("docker") {
        "docker".to_string()
    } else if lower.contains("mongodb") || lower.contains("mongod") {
        "mongod".to_string()
    } else if lower.starts_with("port_") {
        String::new()
    } else {
        name.split_whitespace().next().unwrap_or("").trim().to_string()
    }
}

pub fn guess_remote_start_cmd(name: &str, port: u16) -> String {
    if let Some(unit) = remote_service_operand(name) {
        format!("sudo -n -- systemctl start -- {unit} || sudo -n -- service {unit} start")
    } else {
        match port {
            80 | 8080 => "sudo systemctl start nginx 2>/dev/null || sudo systemctl start apache2 2>/dev/null".to_string(),
            3000 => "npm start".to_string(),
            5173 => "npm run dev".to_string(),
            _ => format!("echo 'Please specify start command for port {}'", port),
        }
    }
}

pub fn build_remote_restart_cmd(listener: &RemoteListener, ssh_port: u16) -> Option<String> {
    let pid = listener.pid.filter(|&pid| pid > 1)?;
    let marker = listener.start_marker?;
    let name = listener.name.to_ascii_lowercase();
    if !listener.proto.eq_ignore_ascii_case("tcp")
        || matches!(listener.port, 22) || listener.port == ssh_port
        || name == "ssh" || name.split(|c: char| !c.is_ascii_alphanumeric()).any(|part| part == "sshd")
    {
        return None;
    }
    let unit = remote_service_operand(&listener.name)?;
    Some(format!(r#"set -f
if [ "$(systemctl show --property=MainPID --value -- {unit})" != "{pid}" ]; then
    echo '[launcher] Restart requires a systemd service whose main process owns this listener.' >&2
    exit 1
fi
if ! IFS= read -r stat < /proc/{pid}/stat; then
    exit 1
fi
set -- ${{stat##*) }}
if [ "${{20}}" != "{marker}" ]; then
    echo '[launcher] Process changed or exited. Scan again before restarting.' >&2
    exit 1
fi
sudo -n -- systemctl restart -- {unit}"#))
}

pub fn remote_service_operand(name: &str) -> Option<String> {
    let unit = clean_service_name(name);
    if unit.is_empty() || unit.starts_with('-')
        || !unit.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_.@-".contains(&byte))
    {
        return None;
    }
    Some(shell_quote(&unit))
}

pub fn build_remote_start_cmd(name: &str, port: u16, custom_cmd: Option<&str>, cwd: &str) -> String {
    if let Some(command) = custom_cmd.filter(|command| !command.trim().is_empty()) {
        let command = shell_quote(command);
        let message = shell_quote(&format!("[launcher] Launch requested for {name}."));
        let change_dir = match cwd {
            "" => String::new(),
            "~" => "cd -- \"$HOME\" || exit 1; ".into(),
            path => {
                let path = match path.strip_prefix("~/") {
                    Some(relative) => format!("\"$HOME\"/{}", shell_quote(relative)),
                    None if path.starts_with('/') => shell_quote(path),
                    None => shell_quote(&format!("./{path}")),
                };
                format!("cd -- {path} || exit 1; ")
            }
        };
        return format!("{change_dir}nohup sh -c {command} </dev/null >/dev/null 2>&1 & printf '%s\\n' {message}");
    }
    if let Some(unit) = remote_service_operand(name) {
        return format!("sudo -n -- systemctl start -- {unit}");
    }
    let message = shell_quote(&format!("[launcher] No start command configured for port {port}."));
    format!("printf '%s\\n' {message} >&2; exit 1")
}

pub fn is_sshpass_available() -> bool {
    #[cfg(unix)]
    {
        Command::new("sshpass")
            .arg("-V")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn ssh_policy_args(host: &SshRemoteHost) -> Vec<&'static str> {
    let host_key = if host.strict_host_key {
        "StrictHostKeyChecking=yes"
    } else {
        "StrictHostKeyChecking=accept-new"
    };
    let mut args = vec!["-o", host_key];
    if !host.use_agent {
        args.extend(["-o", "IdentityAgent=none"]);
    }
    args
}

// OpenSSH's ConnectTimeout covers the handshake, not the remote command.
fn ssh_probe_output(mut command: Command, timeout: std::time::Duration) -> std::io::Result<std::process::Output> {
    use std::io::{Read, Seek, SeekFrom};
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    // Files avoid pipe backpressure and can be read even if a descendant inherited a handle.
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command.stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            result => {
                #[cfg(unix)]
                unsafe {
                    // The unreaped child owns the process-group ID, including when wrapped by sshpass.
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                return Err(result.err().unwrap_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::TimedOut, "SSH probe exceeded its deadline")
                }));
            }
        }
    };

    const MAX_OUTPUT: u64 = 4 * 1024 * 1024;
    let read_output = |file: &mut std::fs::File| -> std::io::Result<Vec<u8>> {
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(MAX_OUTPUT + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_OUTPUT {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "SSH probe output exceeded 4 MiB"));
        }
        Ok(bytes)
    };
    Ok(std::process::Output { status, stdout: read_output(&mut stdout)?, stderr: read_output(&mut stderr)? })
}

/// Test connection to remote SSH host with timeout
pub fn test_ssh_connection(host: &SshRemoteHost, timeout_seconds: u32) -> Result<String, String> {
    let has_password = !host.password.trim().is_empty();
    let mut cmd;

    if has_password && is_sshpass_available() {
        cmd = Command::new("sshpass");
        cmd.arg("-e").arg("ssh");
        cmd.env("SSHPASS", host.password.trim());
    } else {
        cmd = Command::new("ssh");
        if !has_password {
            cmd.arg("-o").arg("BatchMode=yes");
        }
    }

    cmd.args(ssh_policy_args(host)).arg("-o").arg("ConnectTimeout=5")
        .arg("-p").arg(host.port.to_string());

    if !host.key_path.trim().is_empty() {
        cmd.arg("-i").arg(host.key_path.trim());
    }

    let target = format!("{}@{}", host.user.trim(), host.host.trim());
    cmd.arg("--").arg(&target).arg("echo __LAUNCHER_SSH_OK__ && uname -s -r -m");

    match ssh_probe_output(cmd, std::time::Duration::from_secs(u64::from(timeout_seconds.clamp(5, 120)))) {
        Ok(out) => {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stdout.contains("__LAUNCHER_SSH_OK__") {
                    let info = stdout.replace("__LAUNCHER_SSH_OK__", "").trim().to_string();
                    let sys_info = if info.is_empty() { "Linux/Unix".to_string() } else { info };
                    Ok(format!("Connected successfully! ({})", sys_info))
                } else {
                    Ok("Connected successfully!".to_string())
                }
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let err_msg = if stderr.trim().is_empty() {
                    format!("SSH exited with status code {:?}", out.status.code())
                } else {
                    stderr.trim().to_string()
                };
                Err(err_msg)
            }
        }
        Err(e) => Err(format!("Failed to execute ssh: {}", e)),
    }
}

/// Prepare a Command that executes a shell command on the remote host over SSH
pub fn build_ssh_command(host: &SshRemoteHost, remote_cmd: &str) -> Command {
    let has_password = !host.password.trim().is_empty();
    let mut cmd;

    if has_password && is_sshpass_available() {
        cmd = Command::new("sshpass");
        cmd.arg("-e").arg("ssh");
        cmd.env("SSHPASS", host.password.trim());
    } else {
        cmd = Command::new("ssh");
        if !has_password {
            cmd.arg("-o").arg("BatchMode=yes");
        }
    }

    cmd.args(ssh_policy_args(host)).arg("-o").arg("ConnectTimeout=5")
        .arg("-o").arg("ServerAliveInterval=15")
        .arg("-o").arg("ServerAliveCountMax=2")
        .arg("-p").arg(host.port.to_string());

    if !host.key_path.trim().is_empty() {
        cmd.arg("-i").arg(host.key_path.trim());
    }

    let target = format!("{}@{}", host.user.trim(), host.host.trim());
    cmd.arg("--").arg(&target);

    let full_remote_cmd = if !host.remote_cwd.trim().is_empty() {
        format!("cd {} 2>/dev/null || true; {}", shell_quote(host.remote_cwd.trim()), remote_cmd)
    } else {
        remote_cmd.to_string()
    };

    cmd.arg(full_remote_cmd);
    cmd
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Fetch listening ports and processes on the remote PC
pub fn fetch_remote_listeners(host: &SshRemoteHost, timeout_seconds: u32) -> Result<Vec<RemoteListener>, String> {
    // Run ss -tulpn or lsof or netstat on the remote host, plus ps for CPU & RAM metrics
    let remote_script = r#"
set -f
metrics=$(
ps -eo pid,%cpu,rss --no-headers 2>/dev/null | while read -r pid cpu rss; do
    marker=
    if { IFS= read -r stat < "/proc/$pid/stat"; } 2>/dev/null; then
        set -- ${stat##*) }
        marker=${20}
    fi
    printf '%s %s %s %s\n' "$pid" "$cpu" "$rss" "$marker"
done
)
if command -v ss >/dev/null 2>&1; then
    ss -tulpn -H 2>/dev/null
elif command -v netstat >/dev/null 2>&1; then
    netstat -tulpn 2>/dev/null
else
    echo "NO_NET_TOOLS"
fi
echo "---PS_METRICS---"
printf '%s\n' "$metrics"
"#;

    let cmd = build_ssh_command(host, remote_script);
    let out = ssh_probe_output(cmd, std::time::Duration::from_secs(u64::from(timeout_seconds.clamp(5, 120)))).map_err(|e| format!("SSH error: {}", e))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(if err.trim().is_empty() { "Failed to query remote ports".to_string() } else { err.trim().to_string() });
    }

    let text = String::from_utf8_lossy(&out.stdout);
    Ok(parse_remote_listeners_str(&text))
}

pub fn parse_remote_listeners_str(text: &str) -> Vec<RemoteListener> {
    let mut listeners = Vec::new();
    let mut seen_ports = BTreeSet::new();

    let mut net_lines = Vec::new();
    let mut ps_lines = Vec::new();
    let mut in_ps = false;

    for line in text.lines() {
        let line = line.trim();
        if line == "---PS_METRICS---" {
            in_ps = true;
            continue;
        }
        if in_ps {
            ps_lines.push(line);
        } else {
            net_lines.push(line);
        }
    }

    let mut metrics: std::collections::HashMap<u32, (f32, f32, Option<u64>)> = std::collections::HashMap::new();
    for line in ps_lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            if let Ok(pid) = parts[0].parse::<u32>() {
                let cpu = parts[1].parse::<f32>().unwrap_or(0.0);
                let rss_kb = parts[2].parse::<f32>().unwrap_or(0.0);
                let mem_mb = (rss_kb / 1024.0 * 10.0).round() / 10.0;
                metrics.insert(pid, (cpu, mem_mb, parts.get(3).and_then(|marker| marker.parse().ok())));
            }
        }
    }

    for line in net_lines {
        if line.is_empty() || line.starts_with("Proto") || line.starts_with("Active") {
            continue;
        }

        // Parse ss -tulpn line e.g.:
        // tcp LISTEN 0 128 0.0.0.0:22 0.0.0.0:* users:(("sshd",pid=850,fd=3))
        // tcp LISTEN 0 511 *:80 *:* users:(("nginx",pid=1234,fd=6))
        // udp UNCONN 0 0 0.0.0.0:5353 0.0.0.0:* users:(("avahi-daemon",pid=600,fd=12))
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let proto = parts[0].to_uppercase();
        let addr_col = parts.iter().find(|p| p.contains(':') && !p.starts_with("users:"));

        if let Some(addr) = addr_col {
            let port_str = addr.rsplit(':').next().unwrap_or("");
            if let Ok(port) = port_str.parse::<u16>() {
                if seen_ports.contains(&port) {
                    continue;
                }
                seen_ports.insert(port);

                // Parse process name and PID from users:(("...",pid=...,...))
                let mut proc_name = "remote_process".to_string();
                let mut pid: Option<u32> = None;

                if let Some(user_part) = parts.iter().find(|p| p.contains("users:") || p.contains("pid=")) {
                    if let Some(start) = user_part.find('"') {
                        if let Some(end) = user_part[start + 1..].find('"') {
                            proc_name = user_part[start + 1..start + 1 + end].to_string();
                        }
                    }
                    if let Some(pid_idx) = user_part.find("pid=") {
                        let sub = &user_part[pid_idx + 4..];
                        let num_str: String = sub.chars().take_while(|c| c.is_ascii_digit()).collect();
                        if let Ok(p) = num_str.parse::<u32>() {
                            pid = Some(p);
                        }
                    }
                }

                if proc_name == "remote_process" {
                    // Fallback to port-based common name
                    proc_name = match port {
                        22 => "sshd".to_string(),
                        80 => "http / nginx".to_string(),
                        443 => "https".to_string(),
                        3000 => "node / app".to_string(),
                        5173 => "vite".to_string(),
                        5432 => "postgres".to_string(),
                        3306 => "mysql".to_string(),
                        6379 => "redis".to_string(),
                        8000 => "web server".to_string(),
                        8080 => "http-alt".to_string(),
                        27017 => "mongodb".to_string(),
                        _ => format!("port_{}", port),
                    };
                }

                let (cpu_pct, mem_mb, start_marker) = pid
                    .and_then(|p| metrics.get(&p).copied())
                    .map(|(c, m, marker)| (Some(c), Some(m), marker))
                    .unwrap_or((None, None, None));

                listeners.push(RemoteListener {
                    proto,
                    port,
                    name: proc_name,
                    pid,
                    start_marker,
                    cmd: format!("Listening on :{}", port),
                    cpu_pct,
                    mem_mb,
                });
            }
        }
    }

    listeners.sort_by_key(|l| l.port);
    listeners
}

/// Launch an external terminal running an interactive SSH session to this host
pub fn launch_external_ssh_terminal(host: &SshRemoteHost) -> Result<(), String> {
    launch_ssh_in_terminal(host, None)
}

/// Launch an external terminal running a specific command on this remote host.
pub fn launch_external_ssh_command(host: &SshRemoteHost, cmd_to_run: &str) -> Result<(), String> {
    launch_ssh_in_terminal(host, Some(cmd_to_run))
}

fn launch_ssh_in_terminal(host: &SshRemoteHost, command: Option<&str>) -> Result<(), String> {
    let mut args = vec![
        "-t".to_owned(), "-p".to_owned(), host.port.to_string(),
    ];
    args.extend(ssh_policy_args(host).into_iter().map(str::to_owned));
    if !host.key_path.trim().is_empty() {
        args.extend(["-i".to_owned(), host.key_path.trim().to_owned()]);
    }
    args.extend(["--".to_owned(), format!("{}@{}", host.user.trim(), host.host.trim())]);
    match (host.remote_cwd.trim(), command) {
        ("", None) => {}
        ("", Some(command)) => args.push(command.to_owned()),
        (cwd, None) => args.push(format!("cd {} && exec bash -l", shell_quote(cwd))),
        (cwd, Some(command)) => args.push(format!("cd {} && {}", shell_quote(cwd), command)),
    }

    // The external terminal prompts for credentials when key or agent authentication fails.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        Command::new("ssh.exe").args(args).creation_flags(0x00000010)
            .spawn().map_err(|e| format!("Failed to open SSH terminal: {e}"))?;
    }

    #[cfg(not(windows))]
    {
        use std::io::Write;
        use std::os::unix::{fs::PermissionsExt, process::CommandExt};
        let mut script = tempfile::NamedTempFile::new()
            .map_err(|e| format!("Could not create SSH launcher: {e}"))?;
        let args = args.iter().map(|arg| shell_quote(arg)).collect::<Vec<_>>().join(" ");
        writeln!(script, "#!/bin/sh\nrm -f -- \"$0\"\nssh {args}\nprintf '\\nPress Enter to close… '\nread -r ignored")
            .map_err(|e| format!("Could not write SSH launcher: {e}"))?;
        script.as_file().set_permissions(std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("Could not make SSH launcher executable: {e}"))?;
        let (terminal, terminal_args) = crate::service::find_terminal()
            .unwrap_or(("x-terminal-emulator", &["-e"]));
        // Close the writable handle before executing the script.
        let path = script.into_temp_path().keep()
            .map_err(|e| format!("Could not retain SSH launcher: {e}"))?;
        if let Err(error) = Command::new(terminal).args(terminal_args).arg(&path).process_group(0).spawn() {
            let _ = std::fs::remove_file(&path);
            return Err(format!("Failed to open SSH terminal: {error}"));
        }
    }
    Ok(())
}

/// Parse ~/.ssh/config to import existing configured hosts
pub fn import_from_ssh_config() -> Vec<SshRemoteHost> {
    let home = match std::env::var("HOME") {
        Ok(h) => std::path::PathBuf::from(h),
        Err(_) => return Vec::new(),
    };
    let config_path = home.join(".ssh/config");
    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    parse_ssh_config_str(&content, &home)
}

pub fn parse_ssh_config_str(content: &str, home: &std::path::Path) -> Vec<SshRemoteHost> {
    let mut hosts = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_host = String::new();
    let mut current_user = "ubuntu".to_string();
    let mut current_port: u16 = 22;
    let mut current_key = String::new();

    let flush = |hosts: &mut Vec<SshRemoteHost>, name: &mut Option<String>, host: &mut String, user: &mut String, port: &mut u16, key: &mut String| {
        if let Some(n) = name.take() {
            if n != "*" && !n.contains('?') {
                let actual_host = if host.is_empty() { n.clone() } else { host.clone() };
                let id = format!("ssh_{}", n.to_lowercase().replace([' ', '.', '-'], "_"));
                hosts.push(SshRemoteHost {
                    id,
                    name: n,
                    host: actual_host,
                    port: *port,
                    user: user.clone(),
                    key_path: key.clone(),
                    password: String::new(),
                    credential_ref: None,
                    remote_cwd: String::new(),
                    use_agent: true,
                    strict_host_key: false,
                });
            }
        }
        *host = String::new();
        *user = "ubuntu".to_string();
        *port = 22;
        *key = String::new();
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let key = parts.next().unwrap_or("").to_lowercase();
        let val = parts.next().unwrap_or("").to_string();

        if key == "host" {
            flush(&mut hosts, &mut current_name, &mut current_host, &mut current_user, &mut current_port, &mut current_key);
            current_name = Some(val);
        } else if key == "hostname" {
            current_host = val;
        } else if key == "user" {
            current_user = val;
        } else if key == "port" {
            if let Ok(p) = val.parse::<u16>() {
                current_port = p;
            }
        } else if key == "identityfile" {
            let expanded = if val.starts_with("~/") {
                format!("{}/{}", home.display(), &val[2..])
            } else {
                val
            };
            current_key = expanded;
        }
    }

    flush(&mut hosts, &mut current_name, &mut current_host, &mut current_user, &mut current_port, &mut current_key);
    hosts
}

/// Reboot the remote SSH host device using sudo authorization.
pub fn reboot_remote_device(host: &SshRemoteHost, sudo_password: &str) -> Result<String, String> {
    run_remote_power_action(host, sudo_password, "reboot")
}

/// Shut down the remote SSH host device using sudo authorization.
pub fn shutdown_remote_device(host: &SshRemoteHost, sudo_password: &str) -> Result<String, String> {
    run_remote_power_action(host, sudo_password, "poweroff")
}

fn run_remote_power_action(host: &SshRemoteHost, password: &str, action: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::Stdio;
    debug_assert!(matches!(action, "reboot" | "poweroff"));
    let password = if password.is_empty() { &host.password } else { password };
    if password.contains(['\r', '\n']) {
        return Err("The sudo password contains a line break.".into());
    }
    let sudo = if password.is_empty() { "sudo -n --" } else { "sudo -S -p '' --" };
    let mut command = build_ssh_command(host, &format!("{sudo} {action}"));
    command.stdin(if password.is_empty() { Stdio::null() } else { Stdio::piped() })
        .stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("Could not start SSH: {e}"))?;
    let write_result = if let Some(mut input) = child.stdin.take() {
        input.write_all(password.as_bytes()).and_then(|()| input.write_all(b"\n"))
    } else {
        Ok(())
    };
    let output = child.wait_with_output().map_err(|e| format!("SSH failed: {e}"))?;
    if output.status.success() {
        return Ok(format!("Remote {action} command accepted."));
    }
    let error = String::from_utf8_lossy(&output.stderr);
    Err(if error.trim().is_empty() {
        write_result.err().map_or_else(
            || format!("Remote {action} could not be confirmed ({})", output.status),
            |e| format!("Could not send sudo credentials: {e}"),
        )
    } else {
        format!("Remote {action} could not be confirmed: {}", error.trim())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[cfg(unix)]
    #[test]
    fn stalled_ssh_probe_times_out() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30"]);
        let error = ssh_probe_output(command, std::time::Duration::from_millis(50)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    }

    #[test]
    fn test_parse_ssh_config_str() {
        let dummy_home = Path::new("/home/testuser");
        let config_text = r#"
# Sample SSH Config
Host myserver
    HostName 192.168.1.100
    User devops
    Port 2222
    IdentityFile ~/.ssh/id_rsa

Host aws-ec2
    HostName ec2.compute.amazonaws.com
    User ec2-user
    Port 22

Host *
    ServerAliveInterval 60
"#;

        let hosts = parse_ssh_config_str(config_text, dummy_home);
        assert_eq!(hosts.len(), 2);

        assert_eq!(hosts[0].name, "myserver");
        assert_eq!(hosts[0].host, "192.168.1.100");
        assert_eq!(hosts[0].user, "devops");
        assert_eq!(hosts[0].port, 2222);
        assert_eq!(hosts[0].key_path, "/home/testuser/.ssh/id_rsa");

        assert_eq!(hosts[1].name, "aws-ec2");
        assert_eq!(hosts[1].host, "ec2.compute.amazonaws.com");
        assert_eq!(hosts[1].user, "ec2-user");
        assert_eq!(hosts[1].port, 22);
    }

    #[test]
    fn test_parse_remote_listeners_str() {
        let ss_output = r#"
Netid  State   Recv-Q  Send-Q   Local Address:Port   Peer Address:Port  Process
tcp    LISTEN  0       128            0.0.0.0:22          0.0.0.0:*      users:(("sshd",pid=850,fd=3))
tcp    LISTEN  0       511                  *:80                *:*      users:(("nginx",pid=1234,fd=6))
tcp    LISTEN  0       128            0.0.0.0:3000        0.0.0.0:*      users:(("node",pid=5678,fd=10))
---PS_METRICS---
850  0.2  12480  98765
1234  1.8  48920
5678  12.5  150400
"#;

        let listeners = parse_remote_listeners_str(ss_output);
        assert_eq!(listeners.len(), 3);

        assert_eq!(listeners[0].port, 22);
        assert_eq!(listeners[0].name, "sshd");
        assert_eq!(listeners[0].pid, Some(850));
        assert_eq!(listeners[0].start_marker, Some(98765));
        assert_eq!(listeners[0].cpu_pct, Some(0.2));
        assert_eq!(listeners[0].mem_mb, Some(12.2)); // 12480 / 1024 = 12.1875 -> 12.2

        assert_eq!(listeners[1].port, 80);
        assert_eq!(listeners[1].name, "nginx");
        assert_eq!(listeners[1].pid, Some(1234));
        assert_eq!(listeners[1].start_marker, None);
        assert_eq!(listeners[1].cpu_pct, Some(1.8));

        assert_eq!(listeners[2].port, 3000);
        assert_eq!(listeners[2].name, "node");
        assert_eq!(listeners[2].pid, Some(5678));
        assert_eq!(listeners[2].cpu_pct, Some(12.5));
    }

    #[test]
    fn test_remote_server_cmd_builders() {
        assert_eq!(clean_service_name("http / nginx"), "nginx");
        assert_eq!(clean_service_name("sshd"), "sshd");
        assert_eq!(clean_service_name("port_8080"), "");

        let start_cmd = build_remote_start_cmd("nginx", 80, None, "");
        assert!(start_cmd.contains("systemctl start -- 'nginx'"));

        let start_custom = build_remote_start_cmd("app", 3000, Some("python3 main.py"), "~/a b");
        assert!(start_custom.starts_with("cd -- \"$HOME\"/'a b' || exit 1; "));
        assert!(start_custom.contains("nohup sh -c 'python3 main.py'"));
        assert!(remote_service_operand("-option").is_none());
        assert!(remote_service_operand("app;id").is_none());

    }

    #[test]
    fn restart_requires_identity_and_excludes_ssh() {
        let mut listener = RemoteListener {
            proto: "TCP".into(),
            port: 80,
            name: "nginx".into(),
            pid: Some(1234),
            start_marker: Some(5678),
            cmd: String::new(),
            cpu_pct: None,
            mem_mb: None,
        };
        let command = build_remote_restart_cmd(&listener, 2222).unwrap();
        assert!(command.contains("MainPID"));
        assert!(command.contains("5678"));
        assert!(command.ends_with("systemctl restart -- 'nginx'"));
        assert!(build_remote_restart_cmd(&listener, 80).is_none());
        listener.name = "sshd".into();
        assert!(build_remote_restart_cmd(&listener, 2222).is_none());
        listener.name = "nginx".into();
        listener.start_marker = None;
        assert!(build_remote_restart_cmd(&listener, 2222).is_none());
    }
}
