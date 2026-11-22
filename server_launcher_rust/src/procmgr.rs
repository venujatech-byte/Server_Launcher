use crate::config::SshRemoteHost;
use crate::remote::build_ssh_command;
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Default)]
pub struct ProcessDetails {
    pub pid: u32,
    pub name: String,
    pub state: String,
    pub ppid: Option<u32>,
    pub user: String,
    pub threads: usize,
    pub cmdline: String,
    pub exe_path: String,
    pub cwd: String,
    pub rss_bytes: u64,
    pub vsz_bytes: u64,
    pub nice: i32,
    pub cpu_usage: f32,
}

#[derive(Debug, Clone)]
pub struct MemoryMapEntry {
    pub address: String,
    pub perms: String,
    pub offset: String,
    pub dev: String,
    pub inode: String,
    pub pathname: String,
}

#[derive(Debug, Clone)]
pub struct OpenFileEntry {
    pub fd: String,
    pub path: String,
}

pub fn parse_memory_maps(raw: &str) -> Vec<MemoryMapEntry> {
    let mut entries = Vec::new();
    for line in raw.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 5 {
            let pathname = if parts.len() >= 6 {
                parts[5..].join(" ")
            } else {
                String::new()
            };
            entries.push(MemoryMapEntry {
                address: parts[0].to_string(),
                perms: parts[1].to_string(),
                offset: parts[2].to_string(),
                dev: parts[3].to_string(),
                inode: parts[4].to_string(),
                pathname,
            });
        }
    }
    entries
}

pub fn parse_open_files(raw: &str) -> Vec<OpenFileEntry> {
    let mut entries = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((fd_part, path_part)) = trimmed.split_once("->") {
            let last_token = fd_part.split_whitespace().last().unwrap_or(fd_part.trim());
            let fd = last_token.rsplit('/').next().unwrap_or(last_token).to_string();
            entries.push(OpenFileEntry {
                fd,
                path: path_part.trim().to_string(),
            });
        } else {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                entries.push(OpenFileEntry {
                    fd: parts[0].to_string(),
                    path: parts[1..].join(" "),
                });
            }
        }
    }
    entries
}

pub fn fetch_local_process_details(pid: u32, default_name: &str, cpu_usage: f32, default_mem: u64) -> ProcessDetails {
    let mut details = ProcessDetails {
        pid,
        name: default_name.to_string(),
        state: "Running".to_string(),
        ppid: None,
        user: String::new(),
        threads: 1,
        cmdline: String::new(),
        exe_path: String::new(),
        cwd: String::new(),
        rss_bytes: default_mem,
        vsz_bytes: 0,
        nice: 0,
        cpu_usage,
    };

    #[cfg(unix)]
    {
        if let Ok(content) = std::fs::read_to_string(format!("/proc/{}/status", pid)) {
            for line in content.lines() {
                if let Some(val) = line.strip_prefix("Name:\t") {
                    if details.name.is_empty() || details.name == default_name {
                        details.name = val.trim().to_string();
                    }
                } else if let Some(val) = line.strip_prefix("State:\t") {
                    details.state = val.trim().to_string();
                } else if let Some(val) = line.strip_prefix("PPid:\t") {
                    details.ppid = val.trim().parse::<u32>().ok();
                } else if let Some(val) = line.strip_prefix("Uid:\t") {
                    let uids: Vec<&str> = val.split_whitespace().collect();
                    if let Some(first_uid) = uids.first() {
                        details.user = format!("UID {}", first_uid);
                    }
                } else if let Some(val) = line.strip_prefix("Threads:\t") {
                    details.threads = val.trim().parse::<usize>().unwrap_or(1);
                } else if let Some(val) = line.strip_prefix("VmRSS:\t") {
                    let parts: Vec<&str> = val.split_whitespace().collect();
                    if let Some(kb_str) = parts.first() {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            details.rss_bytes = kb * 1024;
                        }
                    }
                } else if let Some(val) = line.strip_prefix("VmSize:\t") {
                    let parts: Vec<&str> = val.split_whitespace().collect();
                    if let Some(kb_str) = parts.first() {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            details.vsz_bytes = kb * 1024;
                        }
                    }
                }
            }
        }

        if let Ok(bytes) = std::fs::read(format!("/proc/{}/cmdline", pid)) {
            let cmd = bytes.split(|&b| b == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).to_string()).collect::<Vec<_>>().join(" ");
            if !cmd.is_empty() {
                details.cmdline = cmd;
            }
        }

        if let Ok(exe) = std::fs::read_link(format!("/proc/{}/exe", pid)) {
            details.exe_path = exe.to_string_lossy().to_string();
        }

        if let Ok(cwd) = std::fs::read_link(format!("/proc/{}/cwd", pid)) {
            details.cwd = cwd.to_string_lossy().to_string();
        }

        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{}/stat", pid)) {
            if let Some(idx) = stat.rfind(')') {
                let rest = &stat[idx + 1..];
                let fields: Vec<&str> = rest.split_whitespace().collect();
                if fields.len() > 16 {
                    if let Ok(nice_val) = fields[16].parse::<i32>() {
                        details.nice = nice_val;
                    }
                }
            }
        }
    }

    if details.cmdline.is_empty() {
        details.cmdline = details.name.clone();
    }
    details
}

pub fn fetch_remote_process_details(host: &SshRemoteHost, pid: u32, default_name: &str, cpu_usage: f32, default_mem: u64) -> Result<ProcessDetails, String> {
    let script = format!(
        r#"python3 -c "
import os, json
pid = {pid}
res = {{'pid': pid, 'name': '{name}', 'state': 'Running', 'ppid': None, 'user': '', 'threads': 1, 'cmdline': '', 'exe': '', 'cwd': '', 'rss': {mem}, 'vsz': 0, 'nice': 0}}
try:
    with open(f'/proc/{{pid}}/status') as f:
        for line in f:
            if line.startswith('Name:'): res['name'] = line.split(':', 1)[1].strip()
            elif line.startswith('State:'): res['state'] = line.split(':', 1)[1].strip()
            elif line.startswith('PPid:'): res['ppid'] = int(line.split(':', 1)[1].strip())
            elif line.startswith('Uid:'): res['user'] = 'UID ' + line.split(':', 1)[1].split()[0]
            elif line.startswith('Threads:'): res['threads'] = int(line.split(':', 1)[1].strip())
            elif line.startswith('VmRSS:'): res['rss'] = int(line.split(':', 1)[1].split()[0]) * 1024
            elif line.startswith('VmSize:'): res['vsz'] = int(line.split(':', 1)[1].split()[0]) * 1024
except: pass
try:
    with open(f'/proc/{{pid}}/cmdline', 'rb') as f:
        parts = [p.decode('utf-8', errors='ignore') for p in f.read().split(b'\x00') if p]
        if parts: res['cmdline'] = ' '.join(parts)
except: pass
try: res['exe'] = os.readlink(f'/proc/{{pid}}/exe')
except: pass
try: res['cwd'] = os.readlink(f'/proc/{{pid}}/cwd')
except: pass
try:
    with open(f'/proc/{{pid}}/stat') as f:
        r = f.read()
        idx = r.rfind(')')
        if idx != -1:
            flds = r[idx+1:].split()
            if len(flds) > 16: res['nice'] = int(flds[16])
except: pass
print(json.dumps(res))
""#,
        pid = pid,
        name = default_name.replace('\'', ""),
        mem = default_mem
    );

    let mut cmd = build_ssh_command(host, &script);
    let out = cmd.output().map_err(|e| format!("SSH command failed: {}", e))?;
    if !out.status.success() {
        return Ok(ProcessDetails {
            pid,
            name: default_name.to_string(),
            state: "Unknown".to_string(),
            ppid: None,
            user: String::new(),
            threads: 1,
            cmdline: default_name.to_string(),
            exe_path: String::new(),
            cwd: String::new(),
            rss_bytes: default_mem,
            vsz_bytes: 0,
            nice: 0,
            cpu_usage,
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        Ok(ProcessDetails {
            pid,
            name: v["name"].as_str().unwrap_or(default_name).to_string(),
            state: v["state"].as_str().unwrap_or("Running").to_string(),
            ppid: v["ppid"].as_u64().map(|p| p as u32),
            user: v["user"].as_str().unwrap_or("").to_string(),
            threads: v["threads"].as_u64().unwrap_or(1) as usize,
            cmdline: v["cmdline"].as_str().unwrap_or(default_name).to_string(),
            exe_path: v["exe"].as_str().unwrap_or("").to_string(),
            cwd: v["cwd"].as_str().unwrap_or("").to_string(),
            rss_bytes: v["rss"].as_u64().unwrap_or(default_mem),
            vsz_bytes: v["vsz"].as_u64().unwrap_or(0),
            nice: v["nice"].as_i64().unwrap_or(0) as i32,
            cpu_usage,
        })
    } else {
        Ok(ProcessDetails {
            pid,
            name: default_name.to_string(),
            state: "Running".to_string(),
            ppid: None,
            user: String::new(),
            threads: 1,
            cmdline: default_name.to_string(),
            exe_path: String::new(),
            cwd: String::new(),
            rss_bytes: default_mem,
            vsz_bytes: 0,
            nice: 0,
            cpu_usage,
        })
    }
}

pub fn fetch_local_memory_maps(pid: u32) -> Result<Vec<MemoryMapEntry>, String> {
    match std::fs::read_to_string(format!("/proc/{}/maps", pid)) {
        Ok(content) => Ok(parse_memory_maps(&content)),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                Err(format!("Permission denied reading /proc/{}/maps. Process is owned by another user or root.", pid))
            } else if e.kind() == std::io::ErrorKind::NotFound {
                Err(format!("Process PID {} no longer exists.", pid))
            } else {
                Err(format!("Failed to read /proc/{}/maps: {}", pid, e))
            }
        }
    }
}

pub fn fetch_remote_memory_maps(host: &SshRemoteHost, pid: u32) -> Result<Vec<MemoryMapEntry>, String> {
    let script = format!("cat /proc/{}/maps 2>&1", pid);
    let mut cmd = build_ssh_command(host, &script);
    let out = cmd.output().map_err(|e| format!("SSH command error: {}", e))?;
    let content = String::from_utf8_lossy(&out.stdout);
    if content.contains("Permission denied") {
        return Err(format!("Permission denied reading /proc/{}/maps on remote host. Process is owned by another user or root.", pid));
    }
    if content.contains("No such file or directory") {
        return Err(format!("Process PID {} no longer exists on remote host.", pid));
    }
    Ok(parse_memory_maps(&content))
}

pub fn fetch_local_open_files(pid: u32) -> Result<Vec<OpenFileEntry>, String> {
    let mut entries = Vec::new();
    let fd_dir = format!("/proc/{}/fd", pid);
    match std::fs::read_dir(&fd_dir) {
        Ok(rd) => {
            for entry in rd.flatten() {
                let fd = entry.file_name().to_string_lossy().to_string();
                let path = std::fs::read_link(entry.path())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| "Unknown".to_string());
                entries.push(OpenFileEntry { fd, path });
            }
            entries.sort_by(|a, b| a.fd.parse::<u32>().unwrap_or(0).cmp(&b.fd.parse::<u32>().unwrap_or(0)));
            Ok(entries)
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                Err(format!("Permission denied reading /proc/{}/fd. Process is owned by another user or root.", pid))
            } else if e.kind() == std::io::ErrorKind::NotFound {
                Err(format!("Process PID {} no longer exists.", pid))
            } else {
                Err(format!("Failed to read /proc/{}/fd: {}", pid, e))
            }
        }
    }
}

pub fn fetch_remote_open_files(host: &SshRemoteHost, pid: u32) -> Result<Vec<OpenFileEntry>, String> {
    let script = format!("ls -l /proc/{}/fd 2>&1", pid);
    let mut cmd = build_ssh_command(host, &script);
    let out = cmd.output().map_err(|e| format!("SSH command error: {}", e))?;
    let content = String::from_utf8_lossy(&out.stdout);
    if content.contains("Permission denied") {
        return Err(format!("Permission denied reading /proc/{}/fd on remote host. Process is owned by another user or root.", pid));
    }
    if content.contains("No such file or directory") {
        return Err(format!("Process PID {} no longer exists on remote host.", pid));
    }
    Ok(parse_open_files(&content))
}

pub fn set_local_priority(pid: u32, nice: i32, expected_start: Option<u64>) -> Result<(), String> {
    verify_local_process(pid, expected_start)?;
    let out = Command::new("renice")
        .args(["-n", &nice.to_string(), "-p", &pid.to_string()])
        .output();

    match out {
        Ok(o) if o.status.success() => Ok(()),
        _ => {
            verify_local_process(pid, expected_start)?;
            let sudo_out = Command::new("sudo")
                .args(["-n", "renice", "-n", &nice.to_string(), "-p", &pid.to_string()])
                .output();
            match sudo_out {
                Ok(s) if s.status.success() => Ok(()),
                Ok(s) => Err(String::from_utf8_lossy(&s.stderr).to_string()),
                Err(e) => Err(e.to_string()),
            }
        }
    }
}

pub fn set_remote_priority(host: &SshRemoteHost, pid: u32, nice: i32, expected_start: Option<u64>) -> Result<(), String> {
    let command = guarded_remote_process_command(pid, expected_start, &format!("renice -n {} -p {}", nice, pid))?;
    run_remote_process_command(host, &command)
}

pub fn get_local_affinity(pid: u32) -> Result<Vec<usize>, String> {
    let out = Command::new("taskset")
        .args(["-c", "-p", &pid.to_string()])
        .output()
        .map_err(|e| format!("Failed to run taskset: {}", e))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_affinity_list(&stdout)
}

pub fn get_remote_affinity(host: &SshRemoteHost, pid: u32) -> Result<Vec<usize>, String> {
    let script = format!("taskset -c -p {} 2>/dev/null", pid);
    let mut cmd = build_ssh_command(host, &script);
    let out = cmd.output().map_err(|e| format!("SSH command error: {}", e))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_affinity_list(&stdout)
}

fn parse_affinity_list(stdout: &str) -> Result<Vec<usize>, String> {
    if let Some(list_part) = stdout.split(':').nth(1) {
        let mut cores = Vec::new();
        for item in list_part.trim().split(',') {
            let item = item.trim();
            if let Some((start_s, end_s)) = item.split_once('-') {
                if let (Ok(s), Ok(e)) = (start_s.trim().parse::<usize>(), end_s.trim().parse::<usize>()) {
                    for c in s..=e {
                        cores.push(c);
                    }
                }
            } else if let Ok(c) = item.parse::<usize>() {
                cores.push(c);
            }
        }
        cores.sort_unstable();
        cores.dedup();
        Ok(cores)
    } else {
        Err("Unable to parse taskset output".to_string())
    }
}

pub fn set_local_affinity(pid: u32, cores: &[usize], expected_start: Option<u64>) -> Result<(), String> {
    verify_local_process(pid, expected_start)?;
    if cores.is_empty() {
        return Err("Cannot set empty CPU affinity".to_string());
    }
    let list_str = cores.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",");
    let out = Command::new("taskset")
        .args(["-c", "-p", &list_str, &pid.to_string()])
        .output();

    match out {
        Ok(o) if o.status.success() => Ok(()),
        _ => {
            verify_local_process(pid, expected_start)?;
            let sudo_out = Command::new("sudo")
                .args(["-n", "taskset", "-c", "-p", &list_str, &pid.to_string()])
                .output();
            match sudo_out {
                Ok(s) if s.status.success() => Ok(()),
                Ok(s) => Err(String::from_utf8_lossy(&s.stderr).to_string()),
                Err(e) => Err(e.to_string()),
            }
        }
    }
}

pub fn set_remote_affinity(host: &SshRemoteHost, pid: u32, cores: &[usize], expected_start: Option<u64>) -> Result<(), String> {
    if cores.is_empty() {
        return Err("Cannot set empty CPU affinity".to_string());
    }
    let list = cores.iter().map(|core| core.to_string()).collect::<Vec<_>>().join(",");
    let command = guarded_remote_process_command(pid, expected_start, &format!("taskset -c -p {} {}", list, pid))?;
    run_remote_process_command(host, &command)
}

// shortcut: renice/taskset setters only take numeric PIDs; keep identity checks until handle-based setters exist.
#[cfg(target_os = "linux")]
fn verify_local_process(pid: u32, expected_start: Option<u64>) -> Result<(), String> {
    let expected = expected_start.ok_or("Process identity is unavailable; refresh the list")?;
    let actual = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()
        .and_then(|stat| crate::sensors::parse_proc_start_marker(&stat));
    if actual == Some(expected) { Ok(()) } else { Err("Process changed or exited; refresh the list".into()) }
}

#[cfg(not(target_os = "linux"))]
fn verify_local_process(_pid: u32, _expected_start: Option<u64>) -> Result<(), String> {
    Err("Process scheduling controls are unavailable on this platform".into())
}

fn guarded_remote_process_command(pid: u32, expected_start: Option<u64>, command: &str) -> Result<String, String> {
    let expected = expected_start.ok_or("Process identity is unavailable; refresh the list")?;
    let script = r#"import os, sys
pid, expected = map(int, sys.argv[1:3])
with open('/proc/%d/stat' % pid) as f:
    actual = int(f.read().rsplit(')', 1)[1].split()[19])
if actual != expected:
    sys.exit('Process changed or exited; refresh the list')
os.execvp(sys.argv[3], sys.argv[3:])
"#;
    Ok(format!("python3 -c {} {} {} {}", crate::remote::shell_quote(script), pid, expected, command))
}

#[cfg(target_os = "linux")]
pub fn send_local_signal(pid: u32, sig: &str, expected_start: Option<u64>) -> Result<(), String> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let signal = match sig {
        "STOP" => libc::SIGSTOP,
        "CONT" => libc::SIGCONT,
        "TERM" => libc::SIGTERM,
        "KILL" => libc::SIGKILL,
        _ => return Err("Unsupported process signal".to_string()),
    };
    let expected = expected_start.ok_or("Process identity is unavailable; refresh the list")?;
    let raw_fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if raw_fd < 0 {
        return Err(format!("Cannot open process handle: {}", std::io::Error::last_os_error()));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw_fd as i32) };
    let actual = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok().and_then(|stat| crate::sensors::parse_proc_start_marker(&stat));
    if actual != Some(expected) {
        return Err("Process changed or exited; refresh the list".to_string());
    }
    let result = unsafe { libc::syscall(libc::SYS_pidfd_send_signal, fd.as_raw_fd(), signal, std::ptr::null::<libc::siginfo_t>(), 0) };
    if result == 0 { return Ok(()); }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() != Some(libc::EPERM) {
        return Err(format!("Cannot send signal: {error}"));
    }
    let script = r#"import os, signal, sys
pid, expected, sig = map(int, sys.argv[1:])
fd = os.pidfd_open(pid)
with open('/proc/%d/stat' % pid) as f:
    actual = int(f.read().rsplit(')', 1)[1].split()[19])
if actual != expected:
    sys.exit('Process changed or exited; refresh the list')
signal.pidfd_send_signal(fd, sig)
"#;
    let output = Command::new("sudo")
        .args(["-n", "python3", "-c", script, &pid.to_string(), &expected.to_string(), &signal.to_string()])
        .output().map_err(|error| format!("Cannot request elevated signal: {error}"))?;
    if output.status.success() { Ok(()) } else {
        let detail = String::from_utf8_lossy(&output.stderr);
        Err(format!("Cannot send signal with elevation: {}", detail.trim()))
    }
}

#[cfg(not(target_os = "linux"))]
pub fn send_local_signal(pid: u32, sig: &str, _expected_start: Option<u64>) -> Result<(), String> {
    let out = Command::new("kill").args([&format!("-{}", sig), &pid.to_string()]).output();
    match out {
        Ok(o) if o.status.success() => Ok(()),
        _ => {
            let sudo_out = Command::new("sudo").args(["-n", "kill", &format!("-{}", sig), &pid.to_string()]).output();
            match sudo_out {
                Ok(s) if s.status.success() => Ok(()),
                Ok(s) => Err(String::from_utf8_lossy(&s.stderr).to_string()),
                Err(e) => Err(e.to_string()),
            }
        }
    }
}

pub fn send_remote_signal(host: &SshRemoteHost, pid: u32, sig: &str, expected_start: Option<u64>) -> Result<(), String> {
    let command = remote_signal_command(pid, sig, expected_start)?;
    run_remote_process_command(host, &command)
}

fn remote_signal_command(pid: u32, sig: &str, expected_start: Option<u64>) -> Result<String, String> {
    if !valid_process_signal(sig) {
        return Err("Unsupported process signal".to_string());
    }
    let expected = expected_start.ok_or("Process identity is unavailable; refresh the list")?;
    let script = r#"import os, signal, sys
pid, expected = map(int, sys.argv[1:3])
fd = os.pidfd_open(pid)
with open('/proc/%d/stat' % pid) as f:
    actual = int(f.read().rsplit(')', 1)[1].split()[19])
if actual != expected:
    sys.exit('Process changed or exited; refresh the list')
signal.pidfd_send_signal(fd, getattr(signal, 'SIG' + sys.argv[3]))
"#;
    Ok(format!("python3 -c {} {} {} {}", crate::remote::shell_quote(script), pid, expected, sig))
}

fn valid_process_signal(sig: &str) -> bool {
    matches!(sig, "STOP" | "CONT" | "TERM" | "KILL")
}

fn run_remote_process_command(host: &SshRemoteHost, command: &str) -> Result<(), String> {
    let password = host.password.trim();
    if password.contains(['\n', '\r']) {
        return Err("Passwords containing line breaks are not supported for sudo".to_string());
    }
    let sudo = if password.is_empty() { "sudo -n" } else { "sudo -S -p ''" };
    let script = format!("({command}) 2>/dev/null || {sudo} {command}");
    let mut cmd = build_ssh_command(host, &script);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|error| format!("SSH command failed: {error}"))?;
    let write_error = child.stdin.take().and_then(|mut stdin| {
        if password.is_empty() { None } else { writeln!(stdin, "{password}").err() }
    });
    let out = child.wait_with_output().map_err(|error| format!("SSH command failed: {error}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&out.stderr);
        Err(if !error.trim().is_empty() { error.trim().to_string() } else if let Some(error) = write_error { format!("Could not send sudo password: {error}") } else { "Remote process command failed".to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn local_signal_rejects_a_changed_process_identity() {
        let pid = std::process::id();
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let marker = crate::sensors::parse_proc_start_marker(&stat).unwrap();
        let error = super::send_local_signal(pid, "TERM", Some(marker.wrapping_add(1))).unwrap_err();
        assert!(error.contains("changed or exited"), "{error}");
    }

    #[test]
    fn scheduling_commands_require_a_process_identity() {
        assert!(super::guarded_remote_process_command(42, None, "renice -n 0 -p 42").is_err());
        assert!(super::guarded_remote_process_command(42, Some(100), "renice -n 0 -p 42").unwrap().ends_with(" 42 100 renice -n 0 -p 42"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn scheduling_rejects_a_changed_process_identity() {
        let pid = std::process::id();
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let marker = crate::sensors::parse_proc_start_marker(&stat).unwrap();
        assert!(super::verify_local_process(pid, Some(marker)).is_ok());
        assert!(super::verify_local_process(pid, Some(marker.wrapping_add(1))).is_err());
    }

    #[test]
    fn remote_signal_requires_identity_and_valid_signal() {
        assert!(super::remote_signal_command(42, "TERM", None).is_err());
        assert!(super::remote_signal_command(42, "TERM; id", Some(100)).is_err());
        assert!(super::remote_signal_command(42, "TERM", Some(100)).unwrap().ends_with(" 42 100 TERM"));
    }

    #[test]
    fn process_signals_reject_shell_input() {
        assert!(valid_process_signal("TERM"));
        assert!(!valid_process_signal("TERM; id"));
        assert!(!valid_process_signal(""));
    }

    #[test]
    fn test_parse_memory_maps() {
        let maps = "00400000-00452000 r-xp 00000000 08:02 173521 /usr/bin/dbus-daemon\n7fff5be00000-7fff5be21000 rw-p 00000000 00:00 0 [stack]";
        let entries = parse_memory_maps(maps);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].address, "00400000-00452000");
        assert_eq!(entries[0].perms, "r-xp");
        assert_eq!(entries[0].pathname, "/usr/bin/dbus-daemon");
        assert_eq!(entries[1].pathname, "[stack]");
    }

    #[test]
    fn test_parse_open_files() {
        let raw = "lrwx------ 1 user user 64 Oct 12 10:00 0 -> /dev/pts/1\nlr-x------ 1 user user 64 Oct 12 10:00 3 -> /etc/hosts";
        let entries = parse_open_files(raw);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].fd, "0");
        assert_eq!(entries[0].path, "/dev/pts/1");
        assert_eq!(entries[1].fd, "3");
        assert_eq!(entries[1].path, "/etc/hosts");
    }

    #[test]
    fn test_parse_affinity_list() {
        assert_eq!(parse_affinity_list("pid 123's current affinity list: 0-3").unwrap(), vec![0, 1, 2, 3]);
        assert_eq!(parse_affinity_list("pid 123's current affinity list: 0,2,4").unwrap(), vec![0, 2, 4]);
    }
}
