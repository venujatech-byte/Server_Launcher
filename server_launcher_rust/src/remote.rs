use crate::config::SshRemoteHost;
use std::collections::BTreeSet;
use std::process::Command;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct RemoteListener {
    pub proto: String,
    pub port: u16,
    pub name: String,
    pub pid: Option<u32>,
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
    let clean_svc = clean_service_name(name);
    if !clean_svc.is_empty() {
        format!("sudo systemctl start {} 2>/dev/null || sudo service {} start", clean_svc, clean_svc)
    } else {
        match port {
            80 | 8080 => "sudo systemctl start nginx 2>/dev/null || sudo systemctl start apache2 2>/dev/null".to_string(),
            3000 => "npm start".to_string(),
            5173 => "npm run dev".to_string(),
            _ => format!("echo 'Please specify start command for port {}'", port),
        }
    }
}

pub fn build_remote_stop_cmd(name: &str, port: u16, pid: Option<u32>) -> String {
    let clean_svc = clean_service_name(name);
    let pid_str = pid.map(|p| p.to_string()).unwrap_or_default();
    format!(
r#"echo "[launcher] Stopping {name} on port {port}..."; \
if [ -n "{clean_svc}" ] && sudo systemctl is-active --quiet "{clean_svc}" 2>/dev/null; then \
    sudo systemctl stop "{clean_svc}" && echo "[launcher] {clean_svc} stopped via systemd."; \
elif [ -n "{pid_str}" ]; then \
    kill -15 {pid_str} 2>/dev/null || kill -9 {pid_str} 2>/dev/null; \
    echo "[launcher] Stopped PID {pid_str}."; \
else \
    fuser -k -n tcp {port} 2>/dev/null || true; \
    echo "[launcher] Stopped listeners on port {port}."; \
fi"#
    )
}

pub fn build_remote_restart_cmd(name: &str, port: u16, pid: Option<u32>) -> String {
    let clean_svc = clean_service_name(name);
    let pid_str = pid.map(|p| p.to_string()).unwrap_or_default();
    format!(
r#"echo "[launcher] Restarting {name} on port {port}..."; \
if [ -n "{clean_svc}" ] && sudo systemctl is-active --quiet "{clean_svc}" 2>/dev/null; then \
    sudo systemctl restart "{clean_svc}" && echo "[launcher] {clean_svc} restarted via systemd."; \
elif [ -n "{pid_str}" ]; then \
    CMD=$(tr '\0' ' ' < /proc/{pid_str}/cmdline 2>/dev/null || ps -p {pid_str} -o args= 2>/dev/null); \
    CWD=$(readlink /proc/{pid_str}/cwd 2>/dev/null || echo "."); \
    kill -15 {pid_str} 2>/dev/null || kill -9 {pid_str} 2>/dev/null; \
    sleep 1; \
    if [ -n "$CMD" ]; then \
        (cd "$CWD" && nohup $CMD >/dev/null 2>&1 &); \
        echo "[launcher] Relaunched: $CMD"; \
    elif [ -n "{clean_svc}" ]; then \
        sudo systemctl restart "{clean_svc}" 2>/dev/null || true; \
    fi; \
elif [ -n "{clean_svc}" ]; then \
    sudo systemctl restart "{clean_svc}" 2>/dev/null || echo "[launcher] systemctl restart {clean_svc} failed."; \
else \
    echo "[launcher] Could not determine restart command for port {port}."; \
fi"#
    )
}

pub fn build_remote_start_cmd(name: &str, port: u16, custom_cmd: Option<&str>) -> String {
    let clean_svc = clean_service_name(name);
    if let Some(cmd) = custom_cmd {
        if !cmd.trim().is_empty() {
            return format!(
r#"echo "[launcher] Starting {name} ({cmd})..."; \
nohup {cmd} >/dev/null 2>&1 & \
echo "[launcher] Started {name} in background.""#
            );
        }
    }
    format!(
r#"echo "[launcher] Starting {name} on port {port}..."; \
if [ -n "{clean_svc}" ]; then \
    sudo systemctl start "{clean_svc}" 2>/dev/null && echo "[launcher] {clean_svc} started via systemd." || echo "[launcher] Failed to start {clean_svc}."; \
else \
    echo "[launcher] No start command configured for port {port}."; \
fi"#
    )
}

#[allow(dead_code)]
pub fn build_remote_stop_all_cmd(listeners: &[RemoteListener], host_port: u16) -> String {
    let targets: Vec<_> = listeners.iter().filter(|l| l.port != host_port && l.port != 22).collect();
    if targets.is_empty() {
        return "echo '[launcher] No non-SSH servers to stop.'".to_string();
    }
    let pids: Vec<String> = targets.iter().filter_map(|l| l.pid.map(|p| p.to_string())).collect();
    let ports: Vec<String> = targets.iter().map(|l| l.port.to_string()).collect();
    let mut script = String::from("echo '[launcher] Stopping all remote servers (preserving SSH session)...'; ");
    if !pids.is_empty() {
        script.push_str(&format!("kill -15 {} 2>/dev/null || kill -9 {} 2>/dev/null; ", pids.join(" "), pids.join(" ")));
    }
    for port in ports {
        script.push_str(&format!("fuser -k -n tcp {} 2>/dev/null || true; ", port));
    }
    script.push_str("echo '[launcher] All remote servers stopped.'");
    script
}

#[allow(dead_code)]
pub fn build_remote_restart_all_cmd(listeners: &[RemoteListener], host_port: u16) -> String {
    let targets: Vec<_> = listeners.iter().filter(|l| l.port != host_port && l.port != 22).collect();
    if targets.is_empty() {
        return "echo '[launcher] No non-SSH servers to restart.'".to_string();
    }
    let mut script = String::from("echo '[launcher] Restarting all remote servers (preserving SSH session)...'; ");
    for l in &targets {
        let clean = clean_service_name(&l.name);
        if !clean.is_empty() {
            script.push_str(&format!("(sudo systemctl restart {} 2>/dev/null || true); ", clean));
        } else if let Some(pid) = l.pid {
            script.push_str(&format!(
                "CMD=$(tr '\\0' ' ' < /proc/{}/cmdline 2>/dev/null || ps -p {} -o args= 2>/dev/null); CWD=$(readlink /proc/{}/cwd 2>/dev/null || echo '.'); kill -15 {} 2>/dev/null || kill -9 {} 2>/dev/null; (cd \"$CWD\" && nohup $CMD >/dev/null 2>&1 &); ",
                pid, pid, pid, pid, pid
            ));
        }
    }
    script.push_str("echo '[launcher] Restart commands dispatched.'");
    script
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

/// Test connection to remote SSH host with timeout
pub fn test_ssh_connection(host: &SshRemoteHost) -> Result<String, String> {
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

    cmd.arg("-o").arg("ConnectTimeout=5")
        .arg("-o").arg("StrictHostKeyChecking=accept-new")
        .arg("-p").arg(host.port.to_string());

    if !host.key_path.trim().is_empty() {
        cmd.arg("-i").arg(host.key_path.trim());
    }

    let target = format!("{}@{}", host.user.trim(), host.host.trim());
    cmd.arg(&target).arg("echo __LAUNCHER_SSH_OK__ && uname -s -r -m");

    match cmd.output() {
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

    cmd.arg("-o").arg("ConnectTimeout=5")
        .arg("-o").arg("ServerAliveInterval=15")
        .arg("-o").arg("ServerAliveCountMax=2")
        .arg("-o").arg("StrictHostKeyChecking=accept-new")
        .arg("-p").arg(host.port.to_string());

    if !host.key_path.trim().is_empty() {
        cmd.arg("-i").arg(host.key_path.trim());
    }

    let target = format!("{}@{}", host.user.trim(), host.host.trim());
    cmd.arg(&target);

    let full_remote_cmd = if !host.remote_cwd.trim().is_empty() {
        format!("cd \"{}\" 2>/dev/null || true; {}", host.remote_cwd.trim(), remote_cmd)
    } else {
        remote_cmd.to_string()
    };

    cmd.arg(full_remote_cmd);
    cmd
}

/// Fetch listening ports and processes on the remote PC
pub fn fetch_remote_listeners(host: &SshRemoteHost) -> Result<Vec<RemoteListener>, String> {
    // Run ss -tulpn or lsof or netstat on the remote host, plus ps for CPU & RAM metrics
    let remote_script = r#"
if command -v ss >/dev/null 2>&1; then
    ss -tulpn -H 2>/dev/null
elif command -v netstat >/dev/null 2>&1; then
    netstat -tulpn 2>/dev/null
else
    echo "NO_NET_TOOLS"
fi
echo "---PS_METRICS---"
ps -eo pid,%cpu,rss --no-headers 2>/dev/null || true
"#;

    let mut cmd = build_ssh_command(host, remote_script);
    let out = cmd.output().map_err(|e| format!("SSH error: {}", e))?;

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

    let mut metrics: std::collections::HashMap<u32, (f32, f32)> = std::collections::HashMap::new();
    for line in ps_lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            if let Ok(pid) = parts[0].parse::<u32>() {
                let cpu = parts[1].parse::<f32>().unwrap_or(0.0);
                let rss_kb = parts[2].parse::<f32>().unwrap_or(0.0);
                let mem_mb = (rss_kb / 1024.0 * 10.0).round() / 10.0;
                metrics.insert(pid, (cpu, mem_mb));
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

                let (cpu_pct, mem_mb) = pid
                    .and_then(|p| metrics.get(&p).copied())
                    .map(|(c, m)| (Some(c), Some(m)))
                    .unwrap_or((None, None));

                listeners.push(RemoteListener {
                    proto,
                    port,
                    name: proc_name,
                    pid,
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
    let script_dir = std::env::temp_dir();
    let script_path = script_dir.join(format!("launcher_ssh_{}.sh", host.id));

    let key_arg = if !host.key_path.trim().is_empty() {
        format!("-i \"{}\"", host.key_path.trim())
    } else {
        String::new()
    };

    let target = format!("{}@{}", host.user.trim(), host.host.trim());
    let cwd_cmd = if !host.remote_cwd.trim().is_empty() {
        format!(" -t \"cd '{}' && exec bash -l\"", host.remote_cwd.trim())
    } else {
        String::new()
    };

    let (pass_block, ssh_cmd) = if !host.password.trim().is_empty() {
        let escaped_pass = host.password.trim().replace('"', "\\\"").replace('$', "\\$");
        (
            format!(
r#"if command -v sshpass >/dev/null 2>&1; then
    export SSHPASS="{}"
    SSH_BIN="sshpass -e ssh"
else
    echo "Note: sshpass not installed. Please enter password if prompted."
    SSH_BIN="ssh"
fi
"#,
                escaped_pass
            ),
            "$SSH_BIN"
        )
    } else {
        (String::new(), "ssh")
    };

    let script_content = format!(
r#"#!/usr/bin/env bash
echo "=========================================================="
echo " Server Launcher: Remote SSH Terminal"
echo " Host   : {name} ({target}:{port})"
echo "=========================================================="
echo "Connecting to {target}..."
{pass_block}
{ssh_cmd} -p {port} {key_arg} {target}{cwd_cmd}
echo ""
echo "SSH connection closed. Press enter to exit."
read -r
"#,
        name = host.name,
        target = target,
        port = host.port,
        key_arg = key_arg,
        cwd_cmd = cwd_cmd,
        pass_block = pass_block,
        ssh_cmd = ssh_cmd
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
            cmd = Command::new(term);
            for arg in args {
                cmd.arg(arg);
            }
            cmd.arg(&script_str);
        } else {
            cmd = Command::new("x-terminal-emulator");
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
        let mut cmd = Command::new("cmd.exe");
        cmd.args(&["/K", &format!("ssh -p {} {} {}", host.port, key_arg, target)]);
        cmd.spawn().map_err(|e| format!("Failed to spawn cmd: {}", e))?;
        Ok(())
    }
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
                    remote_cwd: String::new(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
850  0.2  12480
1234  1.8  48920
5678  12.5  150400
"#;

        let listeners = parse_remote_listeners_str(ss_output);
        assert_eq!(listeners.len(), 3);

        assert_eq!(listeners[0].port, 22);
        assert_eq!(listeners[0].name, "sshd");
        assert_eq!(listeners[0].pid, Some(850));
        assert_eq!(listeners[0].cpu_pct, Some(0.2));
        assert_eq!(listeners[0].mem_mb, Some(12.2)); // 12480 / 1024 = 12.1875 -> 12.2

        assert_eq!(listeners[1].port, 80);
        assert_eq!(listeners[1].name, "nginx");
        assert_eq!(listeners[1].pid, Some(1234));
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

        let stop_cmd = build_remote_stop_cmd("nginx", 80, Some(1234));
        assert!(stop_cmd.contains("systemctl stop \"nginx\""));
        assert!(stop_cmd.contains("kill -15 1234"));

        let restart_cmd = build_remote_restart_cmd("nginx", 80, Some(1234));
        assert!(restart_cmd.contains("systemctl restart \"nginx\""));

        let start_cmd = build_remote_start_cmd("nginx", 80, None);
        assert!(start_cmd.contains("systemctl start \"nginx\""));

        let start_custom = build_remote_start_cmd("app", 3000, Some("python3 main.py"));
        assert!(start_custom.contains("nohup python3 main.py"));

        let listeners = vec![
            RemoteListener {
                proto: "TCP".to_string(),
                port: 22,
                name: "sshd".to_string(),
                pid: Some(100),
                cmd: "".to_string(),
                cpu_pct: None,
                mem_mb: None,
            },
            RemoteListener {
                proto: "TCP".to_string(),
                port: 80,
                name: "nginx".to_string(),
                pid: Some(200),
                cmd: "".to_string(),
                cpu_pct: None,
                mem_mb: None,
            },
        ];
        let stop_all = build_remote_stop_all_cmd(&listeners, 22);
        // Ensure sshd port 22 is NOT killed!
        assert!(!stop_all.contains("kill -15 100"));
        assert!(stop_all.contains("kill -15 200"));
    }
}
