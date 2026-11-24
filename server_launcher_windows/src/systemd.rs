use crate::config::SshRemoteHost;
use crate::remote::{build_ssh_command, shell_quote};
#[cfg(not(windows))]
use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub fn needs_sudo_password(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    [
        "sudo authentication required",
        "a password is required",
        "no password was provided",
        "incorrect password",
        "authentication failure",
        "sorry, try again",
        "interactive authentication required",
        "a terminal is required",
        "no tty present",
    ]
    .iter()
    .any(|pattern| error.contains(pattern))
}

fn checked_output(output: std::process::Output, context: &str) -> Result<String, String> {
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if !stderr.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };
    let error = if detail.is_empty() {
        format!("{} failed (exit {:?})", context, output.status.code())
    } else {
        format!("{}: {}", context, detail)
    };
    if needs_sudo_password(&error) {
        Err(format!("Sudo authentication required. {}", error))
    } else {
        Err(error)
    }
}

const SERVICE_OUTPUT_LIMIT: usize = 2 * 1024 * 1024;

fn read_service_output(mut source: impl std::io::Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    use std::io::Read;

    let mut output = Vec::new();
    source.by_ref().take(limit as u64).read_to_end(&mut output)?;
    let mut extra = [0];
    let exceeded = source.read(&mut extra)? != 0;
    if exceeded {
        // Drain excess bytes so the subprocess can finish without blocking on a full pipe.
        std::io::copy(&mut source, &mut std::io::sink())?;
    }
    Ok((output, exceeded))
}

fn run_command(mut command: Command, password: Option<&str>) -> Result<String, String> {
    use std::io::Write;
    command
        .env("LC_ALL", "C")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    command.stdin(if password.is_some() {
        std::process::Stdio::piped()
    } else {
        std::process::Stdio::null()
    });
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start service command: {}", error))?;
    if let Some(password) = password {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "{}", password);
        }
    }
    let stdout = child.stdout.take().expect("piped service stdout");
    let stderr = child.stderr.take().expect("piped service stderr");
    let (stdout, stderr) = std::thread::scope(|scope| {
        let stdout = scope.spawn(move || read_service_output(stdout, SERVICE_OUTPUT_LIMIT));
        let stderr = read_service_output(stderr, SERVICE_OUTPUT_LIMIT);
        (stdout.join().expect("service output reader"), stderr)
    });
    let status = child.wait().map_err(|error| error.to_string())?;
    let (stdout, stdout_exceeded) = stdout.map_err(|error| error.to_string())?;
    let (stderr, stderr_exceeded) = stderr.map_err(|error| error.to_string())?;
    if stdout_exceeded || stderr_exceeded {
        return Err("Service command output exceeded the 2 MiB limit.".into());
    }
    checked_output(std::process::Output { status, stdout, stderr }, "Service command")
}

#[cfg(not(windows))]
pub(crate) fn run_local(
    program: &str,
    args: &[&str],
    privileged: bool,
    password: Option<&str>,
) -> Result<String, String> {
    let password = password.filter(|password| !password.is_empty());
    #[cfg(unix)]
    let is_root = unsafe { libc::geteuid() == 0 };
    #[cfg(not(unix))]
    let is_root = false;
    let mut command;
    if privileged && !is_root {
        command = Command::new("sudo");
        if password.is_some() {
            command.args(["-S", "-p", ""]);
        } else {
            command.arg("-n");
        }
        command.arg("--").arg(program).args(args);
    } else {
        command = Command::new(program);
        command.args(args);
    }
    run_command(
        command,
        if privileged && !is_root {
            password
        } else {
            None
        },
    )
}

#[cfg(not(windows))]
fn local_systemctl(
    user_mode: bool,
    args: &[&str],
    password: Option<&str>,
) -> Result<String, String> {
    let mut all_args = vec!["--no-ask-password", "--no-pager"];
    if user_mode {
        all_args.push("--user");
    }
    all_args.extend_from_slice(args);
    run_local("systemctl", &all_args, !user_mode, password)
}

pub(crate) fn remote_script(
    host: &SshRemoteHost,
    script: &str,
    user_mode: bool,
    privileged: bool,
    password: Option<&str>,
) -> Result<String, String> {
    // SSH does not normally initialize the user's systemd bus environment.
    let bus = if user_mode {
        "uid=$(id -u); export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$uid}; export DBUS_SESSION_BUS_ADDRESS=${DBUS_SESSION_BUS_ADDRESS:-unix:path=$XDG_RUNTIME_DIR/bus}; "
    } else {
        ""
    };
    let script = format!("export LC_ALL=C; {}{}", bus, script);
    let password = password.filter(|password| !password.is_empty());
    let command = if privileged {
        let sudo = if password.is_some() {
            "sudo -S -p '' --"
        } else {
            "sudo -n --"
        };
        format!("export LC_ALL=C; if [ \"$(id -u)\" -eq 0 ]; then sh -c {script}; else {sudo} sh -c {script} sh \"$HOME\" \"${{XDG_CONFIG_HOME:-$HOME/.config}}\"; fi", script = shell_quote(&format!("export HOME=\"${{1:-$HOME}}\"; export XDG_CONFIG_HOME=\"${{2:-${{XDG_CONFIG_HOME:-$HOME/.config}}}}\"; {}", script)))
    } else {
        script
    };
    // Sudo credentials travel over stdin, never in SSH arguments or the SSH credential field.
    run_command(
        build_ssh_command(host, &command),
        if privileged { password } else { None },
    )
}

fn remote_systemctl(
    host: &SshRemoteHost,
    user_mode: bool,
    args: &[&str],
    password: Option<&str>,
) -> Result<String, String> {
    let args = args
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    remote_script(
        host,
        &format!(
            "systemctl --no-ask-password --no-pager {} {}",
            if user_mode { "--user" } else { "" },
            args
        ),
        user_mode,
        !user_mode,
        password,
    )
}

pub fn user_unit_directory() -> Result<PathBuf, String> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .ok_or_else(|| "Cannot determine the user's configuration directory".to_string())?;
    Ok(config.join("systemd").join("user"))
}

pub fn validate_unit_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 255
        || name.starts_with('-')
        || name == "."
        || name == ".."
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b":_-.@\\".contains(&byte))
    {
        return Err(
            "Invalid unit name. Use a unit filename, without paths, whitespace or shell syntax."
                .into(),
        );
    }
    Ok(())
}

pub fn service_unit_name(def: &NewServiceDefinition) -> Result<String, String> {
    let name = def.name.trim();
    validate_unit_name(name)?;
    for value in [
        &def.description,
        &def.exec_start,
        &def.working_dir,
        &def.environment,
        &def.restart,
    ] {
        if value.contains(['\n', '\r', '\0']) {
            return Err("Service fields must contain a single line.".into());
        }
    }
    if def.exec_start.trim().is_empty() {
        return Err("ExecStart cannot be empty".into());
    }
    if ![
        "no",
        "always",
        "on-success",
        "on-failure",
        "on-abnormal",
        "on-watchdog",
        "on-abort",
    ]
    .contains(&def.restart.as_str())
    {
        return Err("Unsupported restart policy".into());
    }
    let name = if name.ends_with(".service") { name.to_string() } else { format!("{}.service", name) };
    validate_unit_name(&name)?;
    Ok(name)
}

fn validate_action(unit: &str, action: &str) -> Result<(), String> {
    if ![
        "start",
        "stop",
        "restart",
        "reload",
        "enable",
        "disable",
        "mask",
        "unmask",
        "daemon-reload",
        "finish-create",
    ]
    .contains(&action)
    {
        return Err("Unsupported systemd action".into());
    }
    if action != "daemon-reload" {
        validate_unit_name(unit)?;
    }
    Ok(())
}

fn remote_path(path: &str) -> Result<String, String> {
    if path.contains(['\n', '\r', '\0']) {
        return Err("Invalid unit file path".into());
    }
    if let Some(rest) = path.strip_prefix("~/.config/systemd/user/") {
        Ok(format!(
            "\"${{XDG_CONFIG_HOME:-$HOME/.config}}/systemd/user\"/{}",
            shell_quote(rest)
        ))
    } else if let Some(rest) = path.strip_prefix("~/") {
        Ok(format!("\"$HOME\"/{}", shell_quote(rest)))
    } else if path.starts_with('/') {
        Ok(shell_quote(path))
    } else {
        Err("A unit file path must be absolute or start with ~/".into())
    }
}

fn validate_drop_in(path: &str) -> Result<(), String> {
    let Some((parent, name)) = path.rsplit_once('/') else {
        return Err("Invalid drop-in path".into());
    };
    if path.contains(['\n', '\r', '\0'])
        || !(path.starts_with('/') || path.starts_with("~/"))
        || !parent.ends_with(".d")
        || !name.ends_with(".conf")
        || name.is_empty()
        || path.split('/').any(|part| part == "..")
    {
        return Err("Only .conf files inside a unit's .d directory can be deleted.".into());
    }
    Ok(())
}

fn validate_unit_write_path(path: &str, user_mode: bool) -> Result<(), String> {
    let relative = if user_mode {
        path.strip_prefix("~/.config/systemd/user/")
            .or_else(|| path.split_once("/systemd/user/")
                .filter(|(home, _)| home.starts_with('/') && home.len() > 1)
                .map(|(_, relative)| relative))
    } else {
        path.strip_prefix("/etc/systemd/system/")
    };
    let Some(relative) = relative else {
        return Err("Unit files can only be written inside the Systemd configuration directory.".into());
    };
    if relative.is_empty() || path.contains(['\n', '\r', '\0'])
        || relative.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("Invalid Systemd configuration path.".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn validate_local_unit_path(path: &str, user_mode: bool) -> Result<(PathBuf, PathBuf), String> {
    let target = PathBuf::from(path);
    let root = if user_mode { user_unit_directory()? } else { PathBuf::from("/etc/systemd/system") };
    let relative = target.strip_prefix(&root).map_err(|_| "Unit files can only be written inside the Systemd configuration directory.".to_string())?;
    if !target.is_absolute() || relative.as_os_str().is_empty()
        || relative.components().any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("Invalid Systemd configuration path.".into());
    }
    Ok((target, root))
}

#[cfg(not(windows))]
fn validate_local_unit_target(path: &str, user_mode: bool) -> Result<(PathBuf, PathBuf), String> {
    let (target, root) = validate_local_unit_path(path, user_mode)?;
    let root = fs::canonicalize(&root).map_err(|error| format!("Could not resolve Systemd configuration directory: {error}"))?;
    let parent = target.parent().ok_or_else(|| "Invalid unit file path".to_string())?;
    let parent = fs::canonicalize(parent).map_err(|error| format!("Could not resolve unit file directory: {error}"))?;
    if !parent.starts_with(&root) {
        return Err("Unit file destination resolves outside the Systemd configuration directory.".into());
    }
    validate_regular_unit_file(&target)?;
    Ok((target, root))
}

fn validate_regular_unit_file(target: &std::path::Path) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(target) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("Unit file destination must be a regular, non-symlink file.".into());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn validate_local_unit_parent(parent: &std::path::Path, root: &std::path::Path) -> Result<(), String> {
    let parent = fs::canonicalize(parent).map_err(|error| format!("Could not resolve unit file directory: {error}"))?;
    if parent.starts_with(root) { Ok(()) } else {
        Err("Unit file destination resolves outside the Systemd configuration directory.".into())
    }
}

#[cfg(not(windows))]
fn write_privileged_unit_content(path: &str, content: &str, user_mode: bool, password: Option<&str>) -> Result<(), String> {
    use std::io::Write;
    let (target, root) = validate_local_unit_target(path, user_mode)?;
    let temporary = std::env::temp_dir().join(format!(
        "sl_unit_{}_{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let result = file
        .write_all(content.as_bytes())
        .map_err(|error| error.to_string())
        .and_then(|_| {
            validate_local_unit_parent(target.parent().ok_or_else(|| "Invalid unit file path".to_string())?, &root)?;
            run_local(
                "sh",
                &["-c", "set -eu; target=$2; dir=${target%/*}; test ! -L \"$target\"; temp=$(mktemp \"$dir/.launcher-unit.XXXXXX\"); trap 'rm -f -- \"$temp\"' EXIT; cat -- \"$1\" > \"$temp\"; if [ -e \"$target\" ]; then chmod --reference=\"$target\" \"$temp\"; else chmod 0644 \"$temp\"; fi; mv -f -- \"$temp\" \"$target\"", "sh", &temporary.to_string_lossy(), path],
                true,
                password,
            )
            .map(|_| ())
        });
    drop(file);
    let _ = fs::remove_file(&temporary);
    result
}

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
        let clean = trimmed
            .trim_start_matches("●")
            .trim_start_matches('*')
            .trim();
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

#[cfg(not(windows))]
pub fn list_local_services(user_mode: bool) -> Result<Vec<SystemdService>, String> {
    let mut cmd = Command::new("systemctl");
    if user_mode {
        cmd.arg("--user");
    }
    cmd.args([
        "list-units",
        "--type=service",
        "--all",
        "--no-pager",
        "--plain",
        "--no-legend",
    ]);

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

pub fn list_remote_services(
    host: &SshRemoteHost,
    user_mode: bool,
) -> Result<Vec<SystemdService>, String> {
    // Listing does not require root; do not use an SSH password as a sudo password.
    let output = remote_script(
        host,
        &format!(
            "systemctl {} list-units --type=service --all --no-pager --plain --no-legend",
            if user_mode { "--user" } else { "" }
        ),
        user_mode,
        false,
        None,
    )?;
    Ok(parse_systemctl_output(&output, user_mode))
}

#[cfg(not(windows))]
pub fn control_local_service(
    unit: &str,
    action: &str,
    user_mode: bool,
    sudo_password: Option<&str>,
) -> Result<String, String> {
    validate_action(unit, action)?;
    if action == "finish-create" {
        local_systemctl(user_mode, &["daemon-reload"], sudo_password)?;
        local_systemctl(user_mode, &["enable", "--now", "--", unit], sudo_password)?;
        return Ok(format!("Enabled and started {}", unit));
    }
    let args = if action == "daemon-reload" {
        vec![action]
    } else {
        vec![action, "--", unit]
    };
    local_systemctl(user_mode, &args, sudo_password)?;
    Ok(format!("{} succeeded for {}", action, unit))
}

pub fn control_remote_service(
    host: &SshRemoteHost,
    unit: &str,
    action: &str,
    user_mode: bool,
    sudo_password: Option<&str>,
) -> Result<String, String> {
    validate_action(unit, action)?;
    if action == "finish-create" {
        remote_systemctl(host, user_mode, &["daemon-reload"], sudo_password)?;
        remote_systemctl(
            host,
            user_mode,
            &["enable", "--now", "--", unit],
            sudo_password,
        )?;
        return Ok(format!("Enabled and started {}", unit));
    }
    let args = if action == "daemon-reload" {
        vec![action]
    } else {
        vec![action, "--", unit]
    };
    remote_systemctl(host, user_mode, &args, sudo_password)?;
    Ok(format!("{} succeeded for {}", action, unit))
}

#[cfg(not(windows))]
pub fn fetch_local_journal(
    unit: &str,
    user_mode: bool,
    lines: usize,
    sudo_password: Option<&str>,
) -> Result<String, String> {
    validate_unit_name(unit)?;
    let count = lines.clamp(1, 1000).to_string();
    let mut args = vec!["--no-pager", "-n", count.as_str(), "-u", unit];
    if user_mode {
        args.push("--user");
    }
    match run_local("journalctl", &args, false, None) {
        Ok(output) => Ok(output),
        Err(error)
            if !user_mode
                && (error.to_ascii_lowercase().contains("permission")
                    || needs_sudo_password(&error)) =>
        {
            run_local("journalctl", &args, true, sudo_password)
        }
        Err(error) => Err(error),
    }
}

pub fn fetch_remote_journal(
    host: &SshRemoteHost,
    unit: &str,
    user_mode: bool,
    lines: usize,
    sudo_password: Option<&str>,
) -> Result<String, String> {
    validate_unit_name(unit)?;
    let script = format!(
        "journalctl {} --no-pager -n {} -u {}",
        if user_mode { "--user" } else { "" },
        lines.clamp(1, 1000),
        shell_quote(unit)
    );
    match remote_script(host, &script, user_mode, false, None) {
        Ok(output) => Ok(output),
        Err(error)
            if !user_mode
                && (error.to_ascii_lowercase().contains("permission")
                    || needs_sudo_password(&error)) =>
        {
            remote_script(host, &script, false, true, sudo_password)
        }
        Err(error) => Err(error),
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

#[cfg(not(windows))]
pub fn create_local_service(
    def: &NewServiceDefinition,
    sudo_password: Option<&str>,
) -> Result<String, String> {
    let name = service_unit_name(def)?;
    let directory = if def.is_user {
        user_unit_directory()?
    } else {
        PathBuf::from("/etc/systemd/system")
    };
    let target = directory.join(&name);
    create_unit_file(
        &target.to_string_lossy(),
        &def.generate_service_content(),
        def.is_user,
        sudo_password,
    )?;
    local_systemctl(def.is_user, &["daemon-reload"], sudo_password)
        .map_err(|error| format!("Unit file created, but daemon reload failed: {}", error))?;
    local_systemctl(
        def.is_user,
        &["enable", "--now", "--", &name],
        sudo_password,
    )
    .map_err(|error| format!("Unit file saved, but enabling/starting failed: {}", error))?;
    Ok(format!("Created, enabled and started {}", name))
}

pub fn create_remote_service(
    host: &SshRemoteHost,
    def: &NewServiceDefinition,
    sudo_password: Option<&str>,
) -> Result<String, String> {
    let name = service_unit_name(def)?;
    let target = if def.is_user {
        format!("~/.config/systemd/user/{}", name)
    } else {
        format!("/etc/systemd/system/{}", name)
    };
    remote_write_content(
        host,
        &target,
        &def.generate_service_content(),
        def.is_user,
        sudo_password,
        true,
    )?;
    remote_systemctl(
        host,
        def.is_user,
        &["enable", "--now", "--", &name],
        sudo_password,
    )
    .map_err(|error| format!("Unit file saved, but enabling/starting failed: {}", error))?;
    Ok(format!("Created, enabled and started {}", name))
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

#[cfg(not(windows))]
pub fn get_local_unit_files(unit: &str, user_mode: bool) -> Result<UnitFilesInfo, String> {
    validate_unit_name(unit)?;
    let mut args = vec![
        "show",
        "-p",
        "FragmentPath",
        "-p",
        "DropInPaths",
        "--",
        unit,
    ];
    if user_mode {
        args.insert(0, "--user");
    }
    // systemctl knows the actual search paths, including XDG, vendor and runtime units.
    let output = run_local("systemctl", &args, false, None)?;
    Ok(parse_show_files_output(&output))
}

pub fn get_remote_unit_files(
    host: &SshRemoteHost,
    unit: &str,
    user_mode: bool,
) -> Result<UnitFilesInfo, String> {
    validate_unit_name(unit)?;
    let script = format!(
        "systemctl {} show -p FragmentPath -p DropInPaths -- {}",
        if user_mode { "--user" } else { "" },
        shell_quote(unit)
    );
    Ok(parse_show_files_output(&remote_script(
        host, &script, user_mode, false, None,
    )?))
}

#[cfg(not(windows))]
pub fn read_local_unit_file(path: &str, sudo_password: Option<&str>) -> Result<String, String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            run_local("cat", &["--", path], true, sudo_password)
        }
        Err(error) => Err(format!("Could not read {}: {}", path, error)),
    }
}

pub fn read_remote_unit_file(
    host: &SshRemoteHost,
    path: &str,
    sudo_password: Option<&str>,
) -> Result<String, String> {
    let script = format!("cat -- {}", remote_path(path)?);
    match remote_script(host, &script, false, false, None) {
        Ok(output) => Ok(output),
        Err(error) if error.to_ascii_lowercase().contains("permission denied") => {
            remote_script(host, &script, false, true, sudo_password)
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(windows))]
pub fn write_local_unit_file(
    path: &str,
    content: &str,
    user_mode: bool,
    sudo_password: Option<&str>,
) -> Result<String, String> {
    let (target, root) = validate_local_unit_target(path, user_mode)?;
    // Permissions, not /home or /.config substrings, determine whether sudo is needed.
    let parent = target.parent().ok_or_else(|| "Invalid unit file path".to_string())?;
    validate_local_unit_parent(parent, &root)?;
    let write = (|| {
        use std::io::Write;
        let mode = fs::metadata(&target).ok().map(|metadata| metadata.permissions());
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(content.as_bytes())?;
        temp.as_file().sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = mode.map(|permissions| permissions.mode() & 0o777).unwrap_or(0o644);
            temp.as_file().set_permissions(fs::Permissions::from_mode(mode))?;
        }
        temp.persist(&target).map(|_| ()).map_err(|error| error.error)
    })();
    match write {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied && !user_mode => {
            write_privileged_unit_content(path, content, user_mode, sudo_password)?
        }
        Err(error) => return Err(format!("Could not write {}: {}", path, error)),
    }
    local_systemctl(user_mode, &["daemon-reload"], sudo_password)
        .map_err(|error| format!("File saved, but daemon reload failed: {}", error))?;
    Ok(format!("Saved {} and reloaded daemon", path))
}

pub fn write_remote_unit_file(
    host: &SshRemoteHost,
    path: &str,
    content: &str,
    user_mode: bool,
    sudo_password: Option<&str>,
) -> Result<String, String> {
    remote_write_content(host, path, content, user_mode, sudo_password, false)
}

#[cfg(not(windows))]
pub fn delete_local_drop_in(
    path: &str,
    user_mode: bool,
    sudo_password: Option<&str>,
) -> Result<String, String> {
    validate_drop_in(path)?;
    let (target, _) = validate_local_unit_target(path, user_mode)?;
    match fs::remove_file(target) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied && !user_mode => {
            run_local("rm", &["-f", "--", path], true, sudo_password)?;
        }
        Err(error) => return Err(format!("Could not delete {}: {}", path, error)),
    }
    local_systemctl(user_mode, &["daemon-reload"], sudo_password)
        .map_err(|error| format!("File deleted, but daemon reload failed: {}", error))?;
    Ok(format!("Deleted {} and reloaded daemon", path))
}

pub fn delete_remote_drop_in(
    host: &SshRemoteHost,
    path: &str,
    user_mode: bool,
    sudo_password: Option<&str>,
) -> Result<String, String> {
    validate_drop_in(path)?;
    validate_unit_write_path(path, user_mode)?;
    let script = format!("rm -f -- {}", remote_path(path)?);
    match remote_script(host, &script, false, false, None) {
        Ok(_) => {}
        Err(error) if !user_mode && error.to_ascii_lowercase().contains("permission denied") => {
            remote_script(host, &script, false, true, sudo_password)?;
        }
        Err(error) => return Err(error),
    }
    remote_systemctl(host, user_mode, &["daemon-reload"], sudo_password)
        .map_err(|error| format!("File deleted, but daemon reload failed: {}", error))?;
    Ok("Drop-in deleted and daemon reloaded".into())
}

#[cfg(not(windows))]
pub fn get_local_unit_dependencies(
    unit: &str,
    user_mode: bool,
) -> Result<UnitDependenciesInfo, String> {
    validate_unit_name(unit)?;
    let mut args = vec![
        "show",
        "-p",
        "Requires",
        "-p",
        "Wants",
        "-p",
        "RequiredBy",
        "-p",
        "WantedBy",
        "-p",
        "After",
        "-p",
        "Before",
        "-p",
        "Conflicts",
        "-p",
        "PartOf",
        "-p",
        "BindsTo",
        "--",
        unit,
    ];
    if user_mode {
        args.insert(0, "--user");
    }
    let mut info = parse_show_dependencies_output(&run_local("systemctl", &args, false, None)?);
    let mut tree_args = vec!["list-dependencies", "--all", "--no-pager", "--", unit];
    if user_mode {
        tree_args.insert(0, "--user");
    }
    info.tree_output = run_local("systemctl", &tree_args, false, None)?;
    Ok(info)
}

pub fn get_remote_unit_dependencies(
    host: &SshRemoteHost,
    unit: &str,
    user_mode: bool,
) -> Result<UnitDependenciesInfo, String> {
    validate_unit_name(unit)?;
    let script = format!("systemctl {flag} show -p Requires -p Wants -p RequiredBy -p WantedBy -p After -p Before -p Conflicts -p PartOf -p BindsTo -- {unit} && printf '\\n---TREE---\\n' && systemctl {flag} list-dependencies --all --no-pager -- {unit}", flag = if user_mode { "--user" } else { "" }, unit = shell_quote(unit));
    let output = remote_script(host, &script, user_mode, false, None)?;
    let (properties, tree) = output
        .split_once("\n---TREE---\n")
        .ok_or_else(|| "Incomplete dependency response".to_string())?;
    let mut info = parse_show_dependencies_output(properties);
    info.tree_output = tree.trim_end().to_string();
    Ok(info)
}

fn enabled_state(output: &str) -> Result<bool, String> {
    match output.trim() {
        "enabled" | "enabled-runtime" => Ok(true),
        "disabled" | "static" | "indirect" | "masked" | "masked-runtime" | "generated"
        | "transient" | "linked" | "linked-runtime" | "alias" => Ok(false),
        _ => Err(format!(
            "Cannot determine unit enablement: {}",
            output.trim()
        )),
    }
}

#[cfg(not(windows))]
pub fn is_local_service_enabled(unit: &str, user_mode: bool) -> Result<bool, String> {
    validate_unit_name(unit)?;
    let mut command = Command::new("systemctl");
    command.env("LC_ALL", "C").arg("--no-pager");
    if user_mode {
        command.arg("--user");
    }
    let output = command
        .args(["is-enabled", "--", unit])
        .output()
        .map_err(|error| error.to_string())?;
    let state = String::from_utf8_lossy(&output.stdout);
    if let Ok(enabled) = enabled_state(&state) {
        return Ok(enabled);
    }
    checked_output(output, "Query unit enablement").and_then(|output| enabled_state(&output))
}

pub fn is_remote_service_enabled(
    host: &SshRemoteHost,
    unit: &str,
    user_mode: bool,
) -> Result<bool, String> {
    validate_unit_name(unit)?;
    let script = format!("state=$(systemctl {} is-enabled -- {}); status=$?; case \"$state\" in enabled|enabled-runtime|disabled|static|indirect|masked|masked-runtime|generated|transient|linked|linked-runtime|alias) printf '%s' \"$state\";; *) exit \"$status\";; esac", if user_mode { "--user" } else { "" }, shell_quote(unit));
    enabled_state(&remote_script(host, &script, user_mode, false, None)?)
}

pub fn validate_drop_in_name(name: &str) -> Result<(), String> {
    validate_unit_name(name)?;
    if name.starts_with('.') || !name.ends_with(".conf") {
        return Err("Use a drop-in filename ending in .conf, without directories.".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn create_unit_file(path: &str, content: &str, user_mode: bool, password: Option<&str>) -> Result<(), String> {
    use std::io::Write;
    validate_local_unit_path(path, user_mode)?;
    let target = PathBuf::from(path);
    if !target.is_absolute() {
        return Err("A unit file path must be absolute".into());
    }
    let parent = target
        .parent()
        .ok_or_else(|| "Invalid unit file path".to_string())?;
    let create = fs::create_dir_all(parent).and_then(|_| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o644);
        }
        let mut file = options.open(&target)?;
        let result = file.write_all(content.as_bytes());
        if result.is_err() {
            drop(file);
            let _ = fs::remove_file(&target);
        }
        result
    });
    match create {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied && !user_mode => {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(content);
            let script = format!(
                "set -eu; dir={}; mkdir -p -- \"$dir\"; temp=$(mktemp \"$dir/.launcher-unit.XXXXXX\"); trap 'rm -f -- \"$temp\"' EXIT; printf '%s' {} | base64 -d > \"$temp\"; chmod 0644 \"$temp\"; ln -- \"$temp\" {}",
                shell_quote(&parent.to_string_lossy()),
                shell_quote(&encoded),
                shell_quote(path)
            );
            run_local("sh", &["-c", &script], true, password).map(|_| ())
        }
        Err(error) => Err(format!("Could not create {}: {}", path, error)),
    }
}

#[cfg(not(windows))]
pub fn create_local_drop_in(
    path: &str,
    content: &str,
    user_mode: bool,
    password: Option<&str>,
) -> Result<String, String> {
    validate_drop_in(path)?;
    validate_local_unit_path(path, user_mode)?;
    create_unit_file(path, content, user_mode, password)?;
    local_systemctl(user_mode, &["daemon-reload"], password)
        .map_err(|error| format!("Drop-in created, but daemon reload failed: {}", error))?;
    Ok(format!("Created {} and reloaded daemon", path))
}

fn remote_write_content(
    host: &SshRemoteHost,
    path: &str,
    content: &str,
    user_mode: bool,
    password: Option<&str>,
    create_new: bool,
) -> Result<String, String> {
    use base64::Engine;
    validate_unit_write_path(path, user_mode)?;
    let path = remote_path(path)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(content);
    let script = format!(
        "set -eu; target={path}; dir=$(dirname -- \"$target\"); mkdir -p -- \"$dir\"; test ! -L \"$target\"; temp=$(mktemp \"$dir/.launcher-unit.XXXXXX\"); trap 'rm -f -- \"$temp\"' EXIT; printf '%s' {content} | base64 -d > \"$temp\"; if [ -e \"$target\" ]; then chmod --reference=\"$target\" \"$temp\"; else chmod 0644 \"$temp\"; fi; {install}",
        install = if create_new { "ln -- \"$temp\" \"$target\"" } else { "mv -f -- \"$temp\" \"$target\"" },
        content = shell_quote(&encoded)
    );
    match remote_script(host, &script, false, false, None) {
        Ok(_) => {}
        Err(error) if !user_mode && error.to_ascii_lowercase().contains("permission denied") => {
            remote_script(host, &script, false, true, password)?;
        }
        Err(error) => return Err(error),
    }
    remote_systemctl(host, user_mode, &["daemon-reload"], password)
        .map_err(|error| format!("File saved, but daemon reload failed: {}", error))?;
    Ok("Unit file saved and daemon reloaded".into())
}

pub fn create_remote_drop_in(
    host: &SshRemoteHost,
    path: &str,
    content: &str,
    user_mode: bool,
    password: Option<&str>,
) -> Result<String, String> {
    validate_drop_in(path)?;
    remote_write_content(host, path, content, user_mode, password, true)
}

fn parse_unit_status(unit: &str, user_mode: bool, output: &str) -> SystemdService {
    let values: std::collections::HashMap<_, _> = output
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    SystemdService {
        name: unit.to_string(),
        is_user: user_mode,
        load_state: values.get("LoadState").unwrap_or(&"unknown").to_string(),
        active_state: values.get("ActiveState").unwrap_or(&"unknown").to_string(),
        sub_state: values.get("SubState").unwrap_or(&"unknown").to_string(),
        description: values.get("Description").unwrap_or(&"").to_string(),
    }
}

#[cfg(not(windows))]
pub fn get_local_unit_status(unit: &str, user_mode: bool) -> Result<SystemdService, String> {
    validate_unit_name(unit)?;
    let mut args = vec![
        "show",
        "-p",
        "LoadState",
        "-p",
        "ActiveState",
        "-p",
        "SubState",
        "-p",
        "Description",
        "--",
        unit,
    ];
    if user_mode {
        args.insert(0, "--user");
    }
    Ok(parse_unit_status(
        unit,
        user_mode,
        &run_local("systemctl", &args, false, None)?,
    ))
}

pub fn get_remote_unit_status(
    host: &SshRemoteHost,
    unit: &str,
    user_mode: bool,
) -> Result<SystemdService, String> {
    validate_unit_name(unit)?;
    let script = format!(
        "systemctl {} show -p LoadState -p ActiveState -p SubState -p Description -- {}",
        if user_mode { "--user" } else { "" },
        shell_quote(unit)
    );
    Ok(parse_unit_status(
        unit,
        user_mode,
        &remote_script(host, &script, user_mode, false, None)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_output_drains_bytes_beyond_the_limit() {
        let mut source = std::io::Cursor::new(b"abcd");
        assert_eq!(read_service_output(&mut source, 4).unwrap(), (b"abcd".to_vec(), false));
        source.set_position(0);
        assert_eq!(read_service_output(&mut source, 2).unwrap(), (b"ab".to_vec(), true));
        assert_eq!(source.position(), 4);
    }

    #[test]
    fn authentication_errors_do_not_hide_configuration_failures() {
        assert!(needs_sudo_password("sudo: a password is required"));
        assert!(needs_sudo_password("sudo: 1 incorrect password attempt"));
        assert!(!needs_sudo_password("Unit invalid.service not found"));
        assert!(!needs_sudo_password("SSH: Permission denied (publickey)"));
        assert!(!needs_sudo_password(
            "sudo: user is not in the sudoers file"
        ));
    }

    #[test]
    fn unit_names_and_dropins_cannot_inject_shell_commands_or_paths() {
        for unit in [
            "../test.service",
            "--all",
            "test;reboot.service",
            "bad\nname.service",
        ] {
            assert!(validate_unit_name(unit).is_err(), "{unit}");
        }
        assert!(validate_unit_name(r"app-example\x2dscope.service").is_ok());
        assert!(validate_drop_in("/etc/systemd/system/web.service.d/override.conf").is_ok());
        for path in [
            "/etc/passwd",
            "/etc/systemd/system/x.d/../override.conf",
            "/x.d/test\n.conf",
        ] {
            assert!(validate_drop_in(path).is_err(), "{path}");
        }
        assert!(validate_action("", "daemon-reload").is_ok());
        assert!(validate_action("x.service", "restart; reboot").is_err());
    }

    #[test]
    fn unit_writes_stay_in_configuration_overrides_and_reject_symlinks() {
        assert!(validate_unit_write_path("/etc/systemd/system/app.service", false).is_ok());
        assert!(validate_unit_write_path("~/.config/systemd/user/app.service", true).is_ok());
        for path in ["/usr/lib/systemd/system/app.service", "/etc/passwd", "/etc/systemd/system/../passwd"] {
            assert!(validate_unit_write_path(path, false).is_err(), "{path}");
        }

        let outside = std::env::temp_dir().join(format!("sl-unit-outside-{}.service", std::process::id()));
        fs::write(&outside, "preserve").unwrap();
        assert!(validate_unit_write_path(outside.to_str().unwrap(), false).is_err());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "preserve");
        fs::remove_file(outside).unwrap();

        let directory = std::env::temp_dir().join(format!("sl-unit-link-test-{}", std::process::id()));
        let target = directory.join("target");
        let link = directory.join("link.service");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(&target, "preserve").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &link).unwrap();
        assert!(validate_regular_unit_file(&link).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "preserve");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn new_service_fields_cannot_inject_additional_directives() {
        let mut def = NewServiceDefinition::default();
        def.description = "description\nExecStart=/bin/false".into();
        assert!(service_unit_name(&def).is_err());
        def.description = "Application".into();
        def.name = "application".into();
        assert_eq!(service_unit_name(&def).unwrap(), "application.service");
    }

    #[test]
    fn remote_paths_expand_on_the_remote_host_and_quote_shell_syntax() {
        assert!(remote_path("relative/path").is_err());
        assert!(remote_path("/tmp/file\nother").is_err());
        assert!(remote_path("~/.config/systemd/user/app.service")
            .unwrap()
            .contains("XDG_CONFIG_HOME"));
        assert_eq!(
            remote_path("/tmp/it's.conf").unwrap(),
            shell_quote("/tmp/it's.conf")
        );
    }

    #[test]
    fn inspection_preserves_inactive_and_unknown_status() {
        let stopped = parse_unit_status(
            "web.service",
            true,
            "LoadState=loaded\nActiveState=inactive\nSubState=dead\n",
        );
        assert!(!stopped.is_active());
        assert_eq!(stopped.sub_state, "dead");
        let absent = parse_unit_status(
            "missing.service",
            false,
            "LoadState=not-found\nActiveState=inactive\n",
        );
        assert_eq!(absent.load_state, "not-found");
        assert!(!absent.is_active());
        assert!(!parse_unit_status("unknown.service", false, "").is_active());
        assert_eq!(enabled_state("enabled-runtime").unwrap(), true);
        assert_eq!(enabled_state("masked").unwrap(), false);
        assert!(enabled_state("").is_err());
    }

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
        assert_eq!(
            services[2].description,
            "Docker Application Container Engine"
        );
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
        assert_eq!(
            parsed.fragment_path,
            Some("/lib/systemd/system/docker.service".to_string())
        );
        assert_eq!(parsed.drop_in_paths.len(), 2);
        assert_eq!(
            parsed.drop_in_paths[0],
            "/etc/systemd/system/docker.service.d/override.conf"
        );
        assert_eq!(
            parsed.drop_in_paths[1],
            "/etc/systemd/system/docker.service.d/http-proxy.conf"
        );
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

#[cfg(windows)]
#[path = "windows_services.rs"]
mod windows_services;
#[cfg(windows)]
pub use windows_services::*;
