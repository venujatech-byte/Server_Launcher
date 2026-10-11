use crate::config::SshRemoteHost;
use crate::remote::build_ssh_command;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemdService {
    pub name: String,
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub description: String,
    pub is_user: bool,
}

impl SystemdService {
    pub fn is_active(&self) -> bool {
        self.active_state == "active"
    }

    pub fn is_failed(&self) -> bool {
        self.active_state == "failed"
    }
}

pub fn parse_systemctl_output(output: &str, is_user: bool) -> Vec<SystemdService> {
    let mut services = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let clean = trimmed.trim_start_matches("●").trim_start_matches('*').trim();
        let parts: Vec<&str> = clean.split_whitespace().collect();
        if parts.len() >= 4 {
            let unit_name = parts[0];
            if !unit_name.ends_with(".service") {
                continue;
            }
            let load_state = parts[1].to_string();
            let active_state = parts[2].to_string();
            let sub_state = parts[3].to_string();
            let description = if parts.len() > 4 {
                parts[4..].join(" ")
            } else {
                String::new()
            };

            services.push(SystemdService {
                name: unit_name.to_string(),
                load_state,
                active_state,
                sub_state,
                description,
                is_user,
            });
        }
    }
    services
}

pub fn list_local_services(user_mode: bool) -> Result<Vec<SystemdService>, String> {
    let mut cmd = Command::new("systemctl");
    if user_mode {
        cmd.arg("--user");
    }
    cmd.args(["list-units", "--type=service", "--all", "--no-pager", "--plain", "--no-legend"]);

    match cmd.output() {
        Ok(out) => {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                Ok(parse_systemctl_output(&stdout, user_mode))
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                Err(err.trim().to_string())
            }
        }
        Err(e) => Err(format!("systemctl error: {}", e)),
    }
}

pub fn list_remote_services(host: &SshRemoteHost, user_mode: bool) -> Result<Vec<SystemdService>, String> {
    let pass = host.password.trim();
    let cmd_str = if user_mode {
        // Non-interactive SSH sessions lack XDG_RUNTIME_DIR and DBUS_SESSION_BUS_ADDRESS.
        // We must set them explicitly so `systemctl --user` can connect to the user's bus.
        // Also try with --no-legend first, fall back without it for older systemd versions.
        concat!(
            "uid=$(id -u); ",
            "export XDG_RUNTIME_DIR=/run/user/$uid; ",
            "export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$uid/bus; ",
            "systemctl --user list-units --type=service --all --no-pager --plain --no-legend 2>/dev/null ",
            "|| systemctl --user list-units --type=service --all --no-pager --plain 2>/dev/null"
        ).to_string()
    } else {
        // System scope: try plain first (works if user has read access),
        // then sudo -n (passwordless sudo), then password sudo, then --no-legend fallback.
        if !pass.is_empty() {
            format!(
                "echo {pass} | sudo -S systemctl list-units --type=service --all --no-pager --plain --no-legend 2>/dev/null \
                 || sudo -n systemctl list-units --type=service --all --no-pager --plain --no-legend 2>/dev/null \
                 || systemctl list-units --type=service --all --no-pager --plain --no-legend 2>/dev/null \
                 || systemctl list-units --type=service --all --no-pager --plain 2>/dev/null",
                pass = pass
            )
        } else {
            "sudo -n systemctl list-units --type=service --all --no-pager --plain --no-legend 2>/dev/null \
             || systemctl list-units --type=service --all --no-pager --plain --no-legend 2>/dev/null \
             || systemctl list-units --type=service --all --no-pager --plain 2>/dev/null".to_string()
        }
    };

    let mut cmd = build_ssh_command(host, &cmd_str);

    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let parsed = parse_systemctl_output(&stdout, user_mode);
            if parsed.is_empty() && !out.status.success() {
                // Command failed with no output — try to get a useful error from stderr
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                if err.is_empty() {
                    Err("systemctl failed. Check SSH connectivity and that systemd is running on the remote host.".to_string())
                } else {
                    Err(err)
                }
            } else {
                Ok(parsed)
            }
        }
        Err(e) => Err(format!("SSH error: {}", e)),
    }
}

pub fn control_local_service(unit: &str, action: &str, user_mode: bool) -> Result<String, String> {
    if user_mode {
        let mut cmd = Command::new("systemctl");
        cmd.arg("--user").arg(action).arg(unit);
        match cmd.output() {
            Ok(out) => {
                if out.status.success() {
                    Ok(format!("Successfully ran {} on {}", action, unit))
                } else {
                    let err = String::from_utf8_lossy(&out.stderr);
                    Err(err.trim().to_string())
                }
            }
            Err(e) => Err(format!("Failed to execute systemctl {}: {}", action, e)),
        }
    } else {
        // System-level services require root privileges.
        // Try pkexec (GUI/polkit prompt) first if on graphical Linux desktop, or systemctl fallback
        let out = Command::new("pkexec")
            .arg("systemctl")
            .arg(action)
            .arg(unit)
            .output();

        match out {
            Ok(o) if o.status.success() => Ok(format!("Successfully ran {} on {}", action, unit)),
            _ => {
                let mut cmd = Command::new("systemctl");
                cmd.arg(action).arg(unit);
                match cmd.output() {
                    Ok(o) if o.status.success() => Ok(format!("Successfully ran {} on {}", action, unit)),
                    Ok(o) => {
                        let err = String::from_utf8_lossy(&o.stderr);
                        let msg = err.trim().to_string();
                        if msg.contains("Interactive authentication required") {
                            Err(format!("{}. System-level units require root (sudo). Run via root terminal or configure polkit/sudoers.", msg))
                        } else {
                            Err(msg)
                        }
                    }
                    Err(e) => Err(format!("Failed to execute systemctl {}: {}", action, e)),
                }
            }
        }
    }
}

pub fn control_remote_service(host: &SshRemoteHost, unit: &str, action: &str, user_mode: bool) -> Result<String, String> {
    let cmd_str = if user_mode {
        // Set XDG_RUNTIME_DIR so systemctl --user works in non-interactive SSH sessions
        format!(
            "uid=$(id -u); export XDG_RUNTIME_DIR=/run/user/$uid; export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$uid/bus; systemctl --user {} {}",
            action, unit
        )
    } else {
        // For system services:
        // If password is provided, try echo password | sudo -S systemctl
        // Otherwise try sudo -n systemctl, then systemctl directly
        let pass = host.password.trim();
        if !pass.is_empty() {
            format!("echo {} | sudo -S systemctl {} {} 2>/dev/null || sudo -n systemctl {} {} 2>/dev/null || systemctl {} {}",
                pass, action, unit, action, unit, action, unit)
        } else {
            format!("sudo -n systemctl {} {} 2>/dev/null || systemctl {} {}",
                action, unit, action, unit)
        }
    };
    let mut cmd = build_ssh_command(host, &cmd_str);

    match cmd.output() {
        Ok(out) => {
            if out.status.success() {
                Ok(format!("Remote {} on {} succeeded", action, unit))
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                let err_trimmed = err.trim().to_string();
                let final_err = if err_trimmed.is_empty() {
                    String::from_utf8_lossy(&out.stdout).trim().to_string()
                } else {
                    err_trimmed
                };
                let final_err = if final_err.is_empty() {
                    format!("systemctl {} failed (exit {}). Check sudo permissions or try 'system' scope.", action, out.status.code().unwrap_or(-1))
                } else {
                    final_err
                };
                if final_err.contains("Interactive authentication required") {
                    Err(format!("{}. Tip: System-level services on remote machines require root. Configure passwordless sudo for '{}' on the remote host (e.g. '{} ALL=(ALL) NOPASSWD: /bin/systemctl') or set the SSH password in Edit Host.", final_err, host.user, host.user))
                } else {
                    Err(final_err)
                }
            }
        }
        Err(e) => Err(format!("SSH control error: {}", e)),
    }
}

pub fn fetch_local_journal(unit: &str, user_mode: bool, lines: usize) -> Result<String, String> {
    let mut cmd = Command::new("journalctl");
    if user_mode {
        cmd.arg("--user");
    }
    cmd.arg("-u").arg(unit).arg("-n").arg(lines.to_string()).arg("--no-pager");

    match cmd.output() {
        Ok(out) => Ok(String::from_utf8_lossy(&out.stdout).to_string()),
        Err(e) => Err(format!("journalctl error: {}", e)),
    }
}

pub fn fetch_remote_journal(host: &SshRemoteHost, unit: &str, user_mode: bool, lines: usize) -> Result<String, String> {
    let pass = host.password.trim();
    let cmd_str = if user_mode {
        // Set XDG_RUNTIME_DIR so journalctl --user works in non-interactive SSH sessions
        format!(
            "uid=$(id -u); export XDG_RUNTIME_DIR=/run/user/$uid; export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$uid/bus; journalctl --user -u {} -n {} --no-pager 2>/dev/null",
            unit, lines
        )
    } else {
        // System journal: try plain first, then sudo (for root-only logs).
        // Password-based sudo: echo pass | sudo -S journalctl ...
        // Passwordless sudo: sudo -n journalctl ...
        // Final fallback: plain journalctl (works if user is in 'adm'/'systemd-journal' group)
        if !pass.is_empty() {
            format!(
                "echo {pass} | sudo -S journalctl -u {unit} -n {lines} --no-pager 2>/dev/null \
                 || sudo -n journalctl -u {unit} -n {lines} --no-pager 2>/dev/null \
                 || journalctl -u {unit} -n {lines} --no-pager 2>/dev/null",
                pass = pass, unit = unit, lines = lines
            )
        } else {
            format!(
                "sudo -n journalctl -u {unit} -n {lines} --no-pager 2>/dev/null \
                 || journalctl -u {unit} -n {lines} --no-pager 2>/dev/null",
                unit = unit, lines = lines
            )
        }
    };
    let mut cmd = build_ssh_command(host, &cmd_str);

    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            if stdout.trim().is_empty() {
                if !out.status.success() {
                    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    Err(if err.is_empty() {
                        format!("journalctl returned no output for '{}'. The service may not have logged anything yet, or you may need sudo access.", unit)
                    } else {
                        err
                    })
                } else {
                    // Command succeeded but no output = no logs yet
                    Ok(format!("-- No journal entries found for {} --", unit))
                }
            } else {
                Ok(stdout)
            }
        }
        Err(e) => Err(format!("SSH journalctl error: {}", e)),
    }
}

#[derive(Debug, Clone)]
pub struct NewServiceDefinition {
    pub name: String,
    pub description: String,
    pub exec_start: String,
    pub working_dir: String,
    pub restart: String,
    pub is_user: bool,
    pub environment: String,
}

impl Default for NewServiceDefinition {
    fn default() -> Self {
        Self {
            name: "custom-app.service".to_string(),
            description: "Custom App Service".to_string(),
            exec_start: "/usr/bin/python3 main.py".to_string(),
            working_dir: String::new(),
            restart: "always".to_string(),
            is_user: true,
            environment: String::new(),
        }
    }
}

impl NewServiceDefinition {
    pub fn generate_service_content(&self) -> String {
        let mut content = String::new();
        content.push_str("[Unit]\n");
        content.push_str(&format!("Description={}\n", self.description.trim()));
        content.push_str("After=network.target\n\n");

        content.push_str("[Service]\n");
        content.push_str("Type=simple\n");
        if !self.working_dir.trim().is_empty() {
            content.push_str(&format!("WorkingDirectory={}\n", self.working_dir.trim()));
        }
        if !self.environment.trim().is_empty() {
            content.push_str(&format!("Environment={}\n", self.environment.trim()));
        }
        content.push_str(&format!("ExecStart={}\n", self.exec_start.trim()));
        content.push_str(&format!("Restart={}\n", self.restart.trim()));
        content.push_str("RestartSec=3\n\n");

        content.push_str("[Install]\n");
        if self.is_user {
            content.push_str("WantedBy=default.target\n");
        } else {
            content.push_str("WantedBy=multi-user.target\n");
        }

        content
    }
}

pub fn create_local_service(def: &NewServiceDefinition) -> Result<String, String> {
    let service_content = def.generate_service_content();
    let unit_name = if def.name.ends_with(".service") {
        def.name.clone()
    } else {
        format!("{}.service", def.name.trim())
    };

    let target_dir: PathBuf = if def.is_user {
        let home = std::env::var("HOME").map_err(|_| "HOME directory not found".to_string())?;
        PathBuf::from(home).join(".config").join("systemd").join("user")
    } else {
        PathBuf::from("/etc/systemd/system")
    };

    fs::create_dir_all(&target_dir).map_err(|e| format!("Failed to create dir {:?}: {}", target_dir, e))?;
    let target_file = target_dir.join(&unit_name);
    fs::write(&target_file, service_content).map_err(|e| format!("Failed to write service file {:?}: {}", target_file, e))?;

    // daemon-reload
    let mut reload_cmd = Command::new("systemctl");
    if def.is_user {
        reload_cmd.arg("--user");
    }
    reload_cmd.arg("daemon-reload");
    let _ = reload_cmd.output();

    // enable --now
    let mut enable_cmd = Command::new("systemctl");
    if def.is_user {
        enable_cmd.arg("--user");
    }
    enable_cmd.arg("enable").arg("--now").arg(&unit_name);
    let out = enable_cmd.output().map_err(|e| format!("Failed to enable unit: {}", e))?;

    if out.status.success() {
        Ok(format!("Created and started service {} at {:?}", unit_name, target_file))
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Ok(format!("Created service at {:?}. (Note: enable returned {})", target_file, err.trim()))
    }
}

pub fn create_remote_service(host: &SshRemoteHost, def: &NewServiceDefinition) -> Result<String, String> {
    let service_content = def.generate_service_content();
    let unit_name = if def.name.ends_with(".service") {
        def.name.clone()
    } else {
        format!("{}.service", def.name.trim())
    };

    let target_path = if def.is_user {
        format!("~/.config/systemd/user/{}", unit_name)
    } else {
        format!("/etc/systemd/system/{}", unit_name)
    };

    let user_flag = if def.is_user { "--user" } else { "" };

    let script = format!(
        "mkdir -p ~/.config/systemd/user && cat << 'EOF' > {} \n{}\nEOF\nsystemctl {} daemon-reload && systemctl {} enable --now {}",
        target_path, service_content, user_flag, user_flag, unit_name
    );

    let mut cmd = build_ssh_command(host, &script);
    match cmd.output() {
        Ok(out) => {
            if out.status.success() {
                Ok(format!("Remote service {} created and started at {}", unit_name, target_path))
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                Err(if err.trim().is_empty() { "Failed to create remote systemd service".to_string() } else { err.trim().to_string() })
            }
        }
        Err(e) => Err(format!("SSH error creating service: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_systemctl_output() {
        let sample_output = r#"
  cron.service               loaded active running Regular background program processing daemon
● failed-app.service         loaded failed failed  My Crashed Application
  docker.service             loaded active running Docker Application Container Engine
"#;
        let services = parse_systemctl_output(sample_output, false);
        assert_eq!(services.len(), 3);
        assert_eq!(services[0].name, "cron.service");
        assert!(services[0].is_active());
        assert!(!services[0].is_failed());

        assert_eq!(services[1].name, "failed-app.service");
        assert!(services[1].is_failed());
        assert!(!services[1].is_active());

        assert_eq!(services[2].name, "docker.service");
        assert_eq!(services[2].description, "Docker Application Container Engine");
    }

    #[test]
    fn test_generate_service_content() {
        let def = NewServiceDefinition {
            name: "web.service".to_string(),
            description: "Web App".to_string(),
            exec_start: "/usr/bin/node index.js".to_string(),
            working_dir: "/home/user/web".to_string(),
            restart: "always".to_string(),
            is_user: true,
            environment: "PORT=3000".to_string(),
        };

        let content = def.generate_service_content();
        assert!(content.contains("Description=Web App"));
        assert!(content.contains("ExecStart=/usr/bin/node index.js"));
        assert!(content.contains("WorkingDirectory=/home/user/web"));
        assert!(content.contains("Environment=PORT=3000"));
        assert!(content.contains("WantedBy=default.target"));
    }
}
