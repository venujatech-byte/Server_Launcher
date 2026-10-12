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

pub fn control_local_service(unit: &str, action: &str, user_mode: bool, sudo_password: Option<&str>) -> Result<String, String> {
    if user_mode {
        let mut cmd = Command::new("systemctl");
        cmd.arg("--user").arg(action);
        if action != "daemon-reload" && !unit.is_empty() {
            cmd.arg(unit);
        }
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
        let pass = sudo_password.unwrap_or("").trim();
        if !pass.is_empty() {
            let mut cmd = Command::new("sudo");
            cmd.arg("-S").arg("-p").arg("").arg("systemctl").arg(action);
            if action != "daemon-reload" && !unit.is_empty() {
                cmd.arg(unit);
            }
            cmd.stdin(std::process::Stdio::piped())
               .stdout(std::process::Stdio::piped())
               .stderr(std::process::Stdio::piped());

            match cmd.spawn() {
                Ok(mut child) => {
                    if let Some(mut stdin) = child.stdin.take() {
                        use std::io::Write;
                        let _ = writeln!(stdin, "{}", pass);
                    }
                    match child.wait_with_output() {
                        Ok(out) => {
                            if out.status.success() {
                                Ok(format!("Successfully ran {} on {}", action, unit))
                            } else {
                                let err = String::from_utf8_lossy(&out.stderr);
                                let trimmed = err.trim();
                                if trimmed.to_lowercase().contains("incorrect password")
                                    || trimmed.to_lowercase().contains("authentication failure")
                                    || trimmed.to_lowercase().contains("try again")
                                {
                                    Err("Incorrect sudo password. Please try again.".to_string())
                                } else if !trimmed.is_empty() {
                                    Err(trimmed.to_string())
                                } else {
                                    Err(format!("systemctl {} failed with exit code {}", action, out.status.code().unwrap_or(-1)))
                                }
                            }
                        }
                        Err(e) => Err(format!("Failed to wait for sudo: {}", e)),
                    }
                }
                Err(e) => Err(format!("Failed to spawn sudo: {}", e)),
            }
        } else {
            // Password not provided: try sudo -n first, then pkexec, then systemctl directly
            let mut sudo_n = Command::new("sudo");
            sudo_n.arg("-n").arg("systemctl").arg(action);
            if action != "daemon-reload" && !unit.is_empty() {
                sudo_n.arg(unit);
            }
            if let Ok(o) = sudo_n.output() {
                if o.status.success() {
                    return Ok(format!("Successfully ran {} on {}", action, unit));
                }
            }

            let mut pk = Command::new("pkexec");
            pk.arg("systemctl").arg(action);
            if action != "daemon-reload" && !unit.is_empty() {
                pk.arg(unit);
            }
            if let Ok(o) = pk.output() {
                if o.status.success() {
                    return Ok(format!("Successfully ran {} on {}", action, unit));
                }
            }

            let mut plain = Command::new("systemctl");
            plain.arg(action);
            if action != "daemon-reload" && !unit.is_empty() {
                plain.arg(unit);
            }
            match plain.output() {
                Ok(o) if o.status.success() => Ok(format!("Successfully ran {} on {}", action, unit)),
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr);
                    let msg = err.trim().to_string();
                    if msg.contains("Interactive authentication required") {
                        Err("Interactive authentication required. Sudo password needed.".to_string())
                    } else if !msg.is_empty() {
                        Err(msg)
                    } else {
                        Err("Sudo password needed.".to_string())
                    }
                }
                Err(e) => Err(format!("Failed to execute systemctl {}: {}", action, e)),
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

pub fn fetch_local_journal(unit: &str, user_mode: bool, lines: usize, sudo_password: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new("journalctl");
    if user_mode {
        cmd.arg("--user");
    }
    cmd.arg("-u").arg(unit).arg("-n").arg(lines.to_string()).arg("--no-pager");

    let normal_res = cmd.output();
    let stdout_str = match &normal_res {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
        _ => String::new(),
    };

    if !stdout_str.trim().is_empty() {
        return Ok(stdout_str);
    }

    // If it's a system service and sudo_password is provided, try sudo journalctl
    if !user_mode {
        if let Some(pass) = sudo_password.map(|p| p.trim()).filter(|p| !p.is_empty()) {
            let mut sudo_cmd = Command::new("sudo");
            sudo_cmd.arg("-S").arg("-p").arg("").arg("journalctl").arg("-u").arg(unit).arg("-n").arg(lines.to_string()).arg("--no-pager");
            sudo_cmd.stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
            if let Ok(mut child) = sudo_cmd.spawn() {
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    let _ = writeln!(stdin, "{}", pass);
                }
                if let Ok(out) = child.wait_with_output() {
                    if out.status.success() {
                        let out_str = String::from_utf8_lossy(&out.stdout).to_string();
                        if !out_str.trim().is_empty() {
                            return Ok(out_str);
                        }
                    }
                }
            }
        }
    }

    match normal_res {
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

pub fn create_local_service(def: &NewServiceDefinition, sudo_password: Option<&str>) -> Result<String, String> {
    let service_content = def.generate_service_content();
    let unit_name = if def.name.ends_with(".service") {
        def.name.clone()
    } else {
        format!("{}.service", def.name.trim())
    };

    if def.is_user {
        let home = std::env::var("HOME").map_err(|_| "HOME directory not found".to_string())?;
        let target_dir = PathBuf::from(home).join(".config").join("systemd").join("user");
        fs::create_dir_all(&target_dir).map_err(|e| format!("Failed to create dir {:?}: {}", target_dir, e))?;
        let target_file = target_dir.join(&unit_name);
        fs::write(&target_file, &service_content).map_err(|e| format!("Failed to write service file {:?}: {}", target_file, e))?;

        // daemon-reload
        let mut reload_cmd = Command::new("systemctl");
        reload_cmd.arg("--user").arg("daemon-reload");
        let _ = reload_cmd.output();

        // enable --now
        let mut enable_cmd = Command::new("systemctl");
        enable_cmd.arg("--user").arg("enable").arg("--now").arg(&unit_name);
        let out = enable_cmd.output().map_err(|e| format!("Failed to enable unit: {}", e))?;

        if out.status.success() {
            Ok(format!("Created and started service {} at {:?}", unit_name, target_file))
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            Ok(format!("Created service at {:?}. (Note: enable returned {})", target_file, err.trim()))
        }
    } else {
        // System service requires root (sudo)
        let pass = sudo_password.unwrap_or("").trim();
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!("sl_sys_{}_{}.service", unit_name, std::process::id()));
        fs::write(&temp_path, &service_content).map_err(|e| format!("Failed to write temp service file {:?}: {}", temp_path, e))?;

        let target_file = format!("/etc/systemd/system/{}", unit_name);
        let temp_file_str = temp_path.to_string_lossy().to_string();

        let run_sudo = |sh_cmd: &str| -> Result<(), String> {
            let mut cmd = Command::new("sudo");
            if !pass.is_empty() {
                cmd.arg("-S").arg("-p").arg("");
            } else {
                cmd.arg("-n");
            }
            cmd.arg("sh").arg("-c").arg(sh_cmd);
            cmd.stdin(std::process::Stdio::piped())
               .stdout(std::process::Stdio::piped())
               .stderr(std::process::Stdio::piped());

            let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn sudo: {}", e))?;
            if !pass.is_empty() {
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    let _ = writeln!(stdin, "{}", pass);
                }
            }
            let out = child.wait_with_output().map_err(|e| format!("Failed waiting for sudo: {}", e))?;
            if out.status.success() {
                Ok(())
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                let trimmed = err.trim();
                if trimmed.to_lowercase().contains("incorrect password")
                    || trimmed.to_lowercase().contains("authentication failure")
                    || trimmed.to_lowercase().contains("try again")
                {
                    Err("Incorrect sudo password. Please try again.".to_string())
                } else if !trimmed.is_empty() {
                    Err(trimmed.to_string())
                } else {
                    Err(format!("Sudo command failed with exit code {}", out.status.code().unwrap_or(-1)))
                }
            }
        };

        let copy_cmd = format!(
            "cp '{}' '{}' && chmod 644 '{}' && systemctl daemon-reload && systemctl enable --now '{}'",
            temp_file_str, target_file, target_file, unit_name
        );

        let res = run_sudo(&copy_cmd);
        let _ = fs::remove_file(&temp_path);

        match res {
            Ok(()) => Ok(format!("Created and started system service {} at {}", unit_name, target_file)),
            Err(e) => Err(e),
        }
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitFilesInfo {
    pub fragment_path: Option<String>,
    pub drop_in_paths: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitDependenciesInfo {
    pub requires: Vec<String>,
    pub wants: Vec<String>,
    pub required_by: Vec<String>,
    pub wanted_by: Vec<String>,
    pub after: Vec<String>,
    pub before: Vec<String>,
    pub conflicts: Vec<String>,
    pub part_of: Vec<String>,
    pub binds_to: Vec<String>,
    pub tree_output: String,
}

pub fn parse_show_files_output(stdout: &str) -> UnitFilesInfo {
    let mut info = UnitFilesInfo::default();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("FragmentPath=") {
            let path = rest.trim();
            if !path.is_empty() {
                info.fragment_path = Some(path.to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("DropInPaths=") {
            for p in rest.split_whitespace() {
                if !p.is_empty() && !info.drop_in_paths.contains(&p.to_string()) {
                    info.drop_in_paths.push(p.to_string());
                }
            }
        }
    }
    info
}

pub fn parse_show_dependencies_output(stdout: &str) -> UnitDependenciesInfo {
    let mut info = UnitDependenciesInfo::default();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some((key, val)) = trimmed.split_once('=') {
            let list: Vec<String> = val.split_whitespace().map(|s| s.to_string()).collect();
            match key {
                "Requires" => info.requires = list,
                "Wants" => info.wants = list,
                "RequiredBy" => info.required_by = list,
                "WantedBy" => info.wanted_by = list,
                "After" => info.after = list,
                "Before" => info.before = list,
                "Conflicts" => info.conflicts = list,
                "PartOf" => info.part_of = list,
                "BindsTo" => info.binds_to = list,
                _ => {}
            }
        }
    }
    info
}

pub fn get_local_unit_files(unit: &str, user_mode: bool) -> UnitFilesInfo {
    let mut cmd = Command::new("systemctl");
    if user_mode {
        cmd.arg("--user");
    }
    cmd.args(["show", "-p", "FragmentPath", "-p", "DropInPaths", unit]);
    let mut info = match cmd.output() {
        Ok(out) if out.status.success() => {
            parse_show_files_output(&String::from_utf8_lossy(&out.stdout))
        }
        _ => UnitFilesInfo::default(),
    };

    // Fallbacks if FragmentPath not found
    if info.fragment_path.is_none() {
        if user_mode {
            if let Ok(home) = std::env::var("HOME") {
                let p = PathBuf::from(home).join(".config").join("systemd").join("user").join(unit);
                if p.exists() {
                    info.fragment_path = Some(p.to_string_lossy().to_string());
                }
            }
        } else {
            for dir in ["/etc/systemd/system", "/lib/systemd/system", "/usr/lib/systemd/system"] {
                let p = PathBuf::from(dir).join(unit);
                if p.exists() {
                    info.fragment_path = Some(p.to_string_lossy().to_string());
                    break;
                }
            }
        }
    }

    // Also check for drop-in folder if empty
    let dropin_dir = if user_mode {
        std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config").join("systemd").join("user").join(format!("{}.d", unit)))
    } else {
        Some(PathBuf::from("/etc/systemd/system").join(format!("{}.d", unit)))
    };
    if let Some(dir) = dropin_dir {
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().map_or(false, |ext| ext == "conf") {
                        let path_str = path.to_string_lossy().to_string();
                        if !info.drop_in_paths.contains(&path_str) {
                            info.drop_in_paths.push(path_str);
                        }
                    }
                }
            }
        }
    }
    info.drop_in_paths.sort();
    info
}

pub fn get_remote_unit_files(host: &SshRemoteHost, unit: &str, user_mode: bool) -> Result<UnitFilesInfo, String> {
    let script = if user_mode {
        format!(
            "uid=$(id -u); export XDG_RUNTIME_DIR=/run/user/$uid; export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$uid/bus; \
             systemctl --user show -p FragmentPath -p DropInPaths {unit} 2>/dev/null",
            unit = unit
        )
    } else {
        format!("systemctl show -p FragmentPath -p DropInPaths {unit} 2>/dev/null", unit = unit)
    };
    let mut cmd = build_ssh_command(host, &script);
    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut info = parse_show_files_output(&stdout);
            info.drop_in_paths.sort();
            Ok(info)
        }
        Err(e) => Err(format!("SSH error: {}", e)),
    }
}

pub fn read_local_unit_file(path: &str, sudo_password: Option<&str>) -> Result<String, String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(e) => {
            let pass = sudo_password.unwrap_or("").trim();
            if !pass.is_empty() {
                let mut cmd = Command::new("sudo");
                cmd.arg("-S").arg("-p").arg("").arg("cat").arg(path);
                cmd.stdin(std::process::Stdio::piped())
                   .stdout(std::process::Stdio::piped())
                   .stderr(std::process::Stdio::piped());
                if let Ok(mut child) = cmd.spawn() {
                    if let Some(mut stdin) = child.stdin.take() {
                        use std::io::Write;
                        let _ = writeln!(stdin, "{}", pass);
                    }
                    if let Ok(out) = child.wait_with_output() {
                        if out.status.success() {
                            return Ok(String::from_utf8_lossy(&out.stdout).to_string());
                        }
                    }
                }
            }
            Err(format!("Could not read file {}: {}", path, e))
        }
    }
}

pub fn read_remote_unit_file(host: &SshRemoteHost, path: &str) -> Result<String, String> {
    let pass = host.password.trim();
    let script = if !pass.is_empty() {
        format!("cat '{path}' 2>/dev/null || (echo {pass} | sudo -S cat '{path}' 2>/dev/null) || sudo -n cat '{path}'", path = path, pass = pass)
    } else {
        format!("cat '{path}' 2>/dev/null || sudo -n cat '{path}'", path = path)
    };
    let mut cmd = build_ssh_command(host, &script);
    match cmd.output() {
        Ok(out) => {
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).to_string())
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                Err(if err.trim().is_empty() { "Failed to read remote unit file".to_string() } else { err.trim().to_string() })
            }
        }
        Err(e) => Err(format!("SSH error reading unit file: {}", e)),
    }
}

pub fn write_local_unit_file(path: &str, content: &str, user_mode: bool, sudo_password: Option<&str>) -> Result<String, String> {
    let target = PathBuf::from(path);
    if user_mode || path.starts_with("/home/") || path.contains("/.config/") {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory {:?}: {}", parent, e))?;
        }
        fs::write(&target, content).map_err(|e| format!("Failed to write {:?}: {}", target, e))?;

        let mut reload_cmd = Command::new("systemctl");
        if user_mode {
            reload_cmd.arg("--user");
        }
        reload_cmd.arg("daemon-reload");
        let _ = reload_cmd.output();
        Ok(format!("Saved {:?} and reloaded daemon", target))
    } else {
        // System file requires root
        let pass = sudo_password.unwrap_or("").trim();
        let temp_dir = std::env::temp_dir();
        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
        let temp_path = temp_dir.join(format!("sl_edit_{}_{}.conf", std::process::id(), nanos));
        fs::write(&temp_path, content).map_err(|e| format!("Failed to write temp file {:?}: {}", temp_path, e))?;

        let temp_file_str = temp_path.to_string_lossy().to_string();
        let parent_str = target.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| "/etc/systemd/system".to_string());
        let sh_cmd = format!(
            "mkdir -p '{}' && cp '{}' '{}' && chmod 644 '{}' && systemctl daemon-reload",
            parent_str, temp_file_str, path, path
        );

        let mut cmd = Command::new("sudo");
        if !pass.is_empty() {
            cmd.arg("-S").arg("-p").arg("");
        } else {
            cmd.arg("-n");
        }
        cmd.arg("sh").arg("-c").arg(&sh_cmd);
        cmd.stdin(std::process::Stdio::piped())
           .stdout(std::process::Stdio::piped())
           .stderr(std::process::Stdio::piped());

        let res = match cmd.spawn() {
            Ok(mut child) => {
                if !pass.is_empty() {
                    if let Some(mut stdin) = child.stdin.take() {
                        use std::io::Write;
                        let _ = writeln!(stdin, "{}", pass);
                    }
                }
                match child.wait_with_output() {
                    Ok(out) => {
                        if out.status.success() {
                            Ok(format!("Saved {} and reloaded daemon", path))
                        } else {
                            let err = String::from_utf8_lossy(&out.stderr);
                            Err(if err.trim().is_empty() { "Sudo write failed".to_string() } else { err.trim().to_string() })
                        }
                    }
                    Err(e) => Err(format!("Failed to wait for sudo: {}", e)),
                }
            }
            Err(e) => Err(format!("Failed to spawn sudo: {}", e)),
        };
        let _ = fs::remove_file(&temp_path);
        res
    }
}

pub fn write_remote_unit_file(host: &SshRemoteHost, path: &str, content: &str, user_mode: bool) -> Result<String, String> {
    let pass = host.password.trim();
    let script = if user_mode {
        format!(
            "mkdir -p \"$(dirname '{path}')\" && cat << 'EOF' > '{path}'\n{content}\nEOF\n\
             uid=$(id -u); export XDG_RUNTIME_DIR=/run/user/$uid; export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$uid/bus; \
             systemctl --user daemon-reload",
            path = path, content = content
        )
    } else if !pass.is_empty() {
        format!(
            "echo {pass} | sudo -S mkdir -p \"$(dirname '{path}')\" && \
             echo '{pass}' | sudo -S tee '{path}' > /dev/null << 'EOF'\n{content}\nEOF\n\
             echo {pass} | sudo -S systemctl daemon-reload",
            path = path, content = content, pass = pass
        )
    } else {
        format!(
            "sudo -n mkdir -p \"$(dirname '{path}')\" && \
             sudo -n tee '{path}' > /dev/null << 'EOF'\n{content}\nEOF\n\
             sudo -n systemctl daemon-reload",
            path = path, content = content
        )
    };
    let mut cmd = build_ssh_command(host, &script);
    match cmd.output() {
        Ok(out) => {
            if out.status.success() {
                Ok(format!("Remote {} saved and daemon reloaded", path))
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                Err(if err.trim().is_empty() { "Failed to write remote file".to_string() } else { err.trim().to_string() })
            }
        }
        Err(e) => Err(format!("SSH error writing file: {}", e)),
    }
}

pub fn delete_local_drop_in(path: &str, user_mode: bool, sudo_password: Option<&str>) -> Result<String, String> {
    if user_mode || path.starts_with("/home/") || path.contains("/.config/") {
        let _ = fs::remove_file(path);
        let mut reload_cmd = Command::new("systemctl");
        if user_mode {
            reload_cmd.arg("--user");
        }
        reload_cmd.arg("daemon-reload");
        let _ = reload_cmd.output();
        Ok(format!("Deleted drop-in {} and reloaded daemon", path))
    } else {
        let pass = sudo_password.unwrap_or("").trim();
        let sh_cmd = format!("rm -f '{}' && systemctl daemon-reload", path);
        let mut cmd = Command::new("sudo");
        if !pass.is_empty() {
            cmd.arg("-S").arg("-p").arg("");
        } else {
            cmd.arg("-n");
        }
        cmd.arg("sh").arg("-c").arg(&sh_cmd);
        cmd.stdin(std::process::Stdio::piped())
           .stdout(std::process::Stdio::piped())
           .stderr(std::process::Stdio::piped());

        match cmd.spawn() {
            Ok(mut child) => {
                if !pass.is_empty() {
                    if let Some(mut stdin) = child.stdin.take() {
                        use std::io::Write;
                        let _ = writeln!(stdin, "{}", pass);
                    }
                }
                match child.wait_with_output() {
                    Ok(out) if out.status.success() => Ok(format!("Deleted drop-in {} and reloaded daemon", path)),
                    Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
                    Err(e) => Err(format!("Sudo error: {}", e)),
                }
            }
            Err(e) => Err(format!("Failed to spawn sudo: {}", e)),
        }
    }
}

pub fn delete_remote_drop_in(host: &SshRemoteHost, path: &str, user_mode: bool) -> Result<String, String> {
    let pass = host.password.trim();
    let script = if user_mode {
        format!(
            "rm -f '{path}'; uid=$(id -u); export XDG_RUNTIME_DIR=/run/user/$uid; export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$uid/bus; \
             systemctl --user daemon-reload",
            path = path
        )
    } else if !pass.is_empty() {
        format!("echo {pass} | sudo -S rm -f '{path}' && echo {pass} | sudo -S systemctl daemon-reload", path = path, pass = pass)
    } else {
        format!("sudo -n rm -f '{path}' && sudo -n systemctl daemon-reload", path = path)
    };
    let mut cmd = build_ssh_command(host, &script);
    match cmd.output() {
        Ok(out) => {
            if out.status.success() {
                Ok(format!("Remote drop-in {} deleted", path))
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                Err(if err.trim().is_empty() { "Failed to delete remote drop-in".to_string() } else { err.trim().to_string() })
            }
        }
        Err(e) => Err(format!("SSH error: {}", e)),
    }
}

pub fn get_local_unit_dependencies(unit: &str, user_mode: bool) -> UnitDependenciesInfo {
    let mut cmd = Command::new("systemctl");
    if user_mode {
        cmd.arg("--user");
    }
    cmd.args(["show", "-p", "Requires", "-p", "Wants", "-p", "RequiredBy", "-p", "WantedBy", "-p", "After", "-p", "Before", "-p", "Conflicts", "-p", "PartOf", "-p", "BindsTo", unit]);
    let mut info = match cmd.output() {
        Ok(out) if out.status.success() => {
            parse_show_dependencies_output(&String::from_utf8_lossy(&out.stdout))
        }
        _ => UnitDependenciesInfo::default(),
    };

    let mut tree_cmd = Command::new("systemctl");
    if user_mode {
        tree_cmd.arg("--user");
    }
    tree_cmd.args(["list-dependencies", unit, "--all", "--no-pager"]);
    if let Ok(out) = tree_cmd.output() {
        if out.status.success() {
            info.tree_output = String::from_utf8_lossy(&out.stdout).to_string();
        }
    }
    info
}

pub fn get_remote_unit_dependencies(host: &SshRemoteHost, unit: &str, user_mode: bool) -> Result<UnitDependenciesInfo, String> {
    let script = if user_mode {
        format!(
            "uid=$(id -u); export XDG_RUNTIME_DIR=/run/user/$uid; export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$uid/bus; \
             systemctl --user show -p Requires -p Wants -p RequiredBy -p WantedBy -p After -p Before -p Conflicts -p PartOf -p BindsTo {unit} 2>/dev/null; \
             echo '---TREE---'; \
             systemctl --user list-dependencies {unit} --all --no-pager 2>/dev/null",
            unit = unit
        )
    } else {
        format!(
            "systemctl show -p Requires -p Wants -p RequiredBy -p WantedBy -p After -p Before -p Conflicts -p PartOf -p BindsTo {unit} 2>/dev/null; \
             echo '---TREE---'; \
             systemctl list-dependencies {unit} --all --no-pager 2>/dev/null",
            unit = unit
        )
    };
    let mut cmd = build_ssh_command(host, &script);
    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut parts = stdout.split("---TREE---");
            let show_out = parts.next().unwrap_or("");
            let tree_out = parts.next().unwrap_or("").trim().to_string();
            let mut info = parse_show_dependencies_output(show_out);
            info.tree_output = tree_out;
            Ok(info)
        }
        Err(e) => Err(format!("SSH error: {}", e)),
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

    #[test]
    fn test_parse_show_files_output() {
        let raw = r#"
FragmentPath=/lib/systemd/system/docker.service
DropInPaths=/etc/systemd/system/docker.service.d/override.conf /etc/systemd/system/docker.service.d/http-proxy.conf
"#;
        let parsed = parse_show_files_output(raw);
        assert_eq!(parsed.fragment_path, Some("/lib/systemd/system/docker.service".to_string()));
        assert_eq!(parsed.drop_in_paths.len(), 2);
        assert_eq!(parsed.drop_in_paths[0], "/etc/systemd/system/docker.service.d/override.conf");
        assert_eq!(parsed.drop_in_paths[1], "/etc/systemd/system/docker.service.d/http-proxy.conf");
    }

    #[test]
    fn test_parse_show_dependencies_output() {
        let raw = r#"
Requires=sysinit.target system.slice
Wants=network-online.target
RequiredBy=multi-user.target
WantedBy=
After=sysinit.target basic.target
Before=shutdown.target
Conflicts=shutdown.target
PartOf=
BindsTo=
"#;
        let deps = parse_show_dependencies_output(raw);
        assert_eq!(deps.requires, vec!["sysinit.target", "system.slice"]);
        assert_eq!(deps.wants, vec!["network-online.target"]);
        assert_eq!(deps.required_by, vec!["multi-user.target"]);
        assert_eq!(deps.after, vec!["sysinit.target", "basic.target"]);
        assert_eq!(deps.before, vec!["shutdown.target"]);
        assert_eq!(deps.conflicts, vec!["shutdown.target"]);
    }
}
