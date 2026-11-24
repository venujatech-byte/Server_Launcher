use std::collections::BTreeMap;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerInfo {
    pub proto: String,
    pub port: u16,
    pub pid: Option<u32>,
    pub start_marker: Option<u64>,
    pub name: String,
}

#[derive(Clone, Default)]
pub struct SystemPortScanner {
    pub listeners: Arc<Mutex<Vec<ListenerInfo>>>,
    pub last_scan_time: Arc<Mutex<String>>,
    pub is_scanning: Arc<Mutex<bool>>,
}

impl SystemPortScanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn trigger_scan(&self) {
        let is_scanning = Arc::clone(&self.is_scanning);
        {
            let mut scanning = is_scanning.lock().unwrap();
            if *scanning {
                return;
            }
            *scanning = true;
        }

        let listeners_arc = Arc::clone(&self.listeners);
        let time_arc = Arc::clone(&self.last_scan_time);

        thread::spawn(move || {
            let found = scan_system_listening_ports();
            let now = chrono::Local::now().format("%H:%M:%S").to_string();

            if let Ok(mut list) = listeners_arc.lock() {
                *list = found;
            }
            if let Ok(mut t) = time_arc.lock() {
                *t = now;
            }
            if let Ok(mut scanning) = is_scanning.lock() {
                *scanning = false;
            }
        });
    }

    pub fn get_listeners(&self) -> Vec<ListenerInfo> {
        self.listeners.lock().unwrap().clone()
    }

    pub fn get_last_scan_time(&self) -> String {
        self.last_scan_time.lock().unwrap().clone()
    }
}

pub fn get_lan_ip() -> String {
    use std::net::UdpSocket;
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                return local_addr.ip().to_string();
            }
        }
    }
    "127.0.0.1".to_string()
}

pub fn scan_system_listening_ports() -> Vec<ListenerInfo> {
    let mut map: BTreeMap<u16, ListenerInfo> = BTreeMap::new();

    #[cfg(not(target_os = "windows"))]
    {
        // 1. Try ss -tlpn
        if let Ok(output) = Command::new("ss").args(["-tlpn"]).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().skip(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 && parts[0] == "LISTEN" {
                    let local_addr = parts[3];
                    if let Some(port_str) = local_addr.rsplit(':').next() {
                        if let Ok(port) = port_str.parse::<u16>() {
                            let mut pid = None;
                            let mut name = "system/other".to_string();

                            // parse users:(("name",pid=123,...))
                            if let Some(idx) = line.find("pid=") {
                                let rem = &line[idx + 4..];
                                let pid_digits: String = rem.chars().take_while(|c| c.is_ascii_digit()).collect();
                                if let Ok(p) = pid_digits.parse::<u32>() {
                                    pid = Some(p);
                                    if let Some(n_idx) = line.find("users:((\"") {
                                        let name_rem = &line[n_idx + 9..];
                                        if let Some(end_q) = name_rem.find('"') {
                                            name = name_rem[..end_q].to_string();
                                        }
                                    }
                                }
                            }

                            map.insert(
                                port,
                                ListenerInfo {
                                    proto: "TCP".to_string(),
                                    port,
                                    pid,
                                    start_marker: None,
                                    name,
                                },
                            );
                        }
                    }
                }
            }
        }

        // 2. Try lsof -iTCP -sTCP:LISTEN -n -P to supplement user-owned listeners
        if let Ok(output) = Command::new("lsof")
            .args(["-iTCP", "-sTCP:LISTEN", "-n", "-P"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().skip(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 9 {
                    let proc_name = parts[0].to_string();
                    if let Ok(p) = parts[1].parse::<u32>() {
                        let addr_col = parts[8];
                        if let Some(port_str) = addr_col.rsplit(':').next() {
                            if let Ok(port) = port_str.parse::<u16>() {
                                map.insert(
                                    port,
                                    ListenerInfo {
                                        proto: "TCP".to_string(),
                                        port,
                                        pid: Some(p),
                                        start_marker: None,
                                        name: proc_name,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }

        // Resolve better names from /proc/{pid}/comm for any system/other entries
        for info in map.values_mut() {
            if (info.name == "system/other" || info.name.is_empty()) && info.pid.is_some() {
                if let Some(p) = info.pid {
                    if let Ok(comm) = std::fs::read_to_string(format!("/proc/{}/comm", p)) {
                        let trimmed = comm.trim();
                        if !trimmed.is_empty() {
                            info.name = trimmed.to_string();
                        }
                    }
                }
            }
        }

        // 3. Fallback to /proc/net/tcp and tcp6 if map is still empty
        if map.is_empty() {
            for proc_file in &["/proc/net/tcp", "/proc/net/tcp6"] {
                if let Ok(content) = std::fs::read_to_string(proc_file) {
                    for line in content.lines().skip(1) {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 4 && parts[3] == "0A" {
                            // 0A is TCP_LISTEN in /proc/net/tcp
                            let local = parts[1];
                            if let Some(hex_port) = local.split(':').nth(1) {
                                if let Ok(port) = u16::from_str_radix(hex_port, 16) {
                                    map.entry(port).or_insert_with(|| ListenerInfo {
                                        proto: "TCP".to_string(),
                                        port,
                                        pid: None,
                                        start_marker: None,
                                        name: "system/other".to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = Command::new("netstat").args(["-ano", "-p", "tcp"]).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 && parts[0] == "TCP" && parts[3] == "LISTENING" {
                    let local_addr = parts[1];
                    if let Some(port_str) = local_addr.rsplit(':').next() {
                        if let Ok(port) = port_str.parse::<u16>() {
                            let pid = parts.get(4).and_then(|p| p.parse::<u32>().ok());
                            map.insert(
                                port,
                                ListenerInfo {
                                    proto: "TCP".to_string(),
                                    port,
                                    pid,
                                    start_marker: None,
                                    name: "system/other".to_string(),
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    for listener in map.values_mut() {
        listener.start_marker = listener.pid.and_then(listener_process_start);
    }
    map.into_values().collect()
}

pub fn listener_process_start(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    return std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok().and_then(|stat| crate::sensors::parse_proc_start_marker(&stat));
    #[cfg(windows)]
    return crate::procmgr::windows_process_start_marker(pid);
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = pid;
        None
    }
}

pub fn stop_listener(listener: &ListenerInfo) -> Result<(), String> {
    let pid = listener.pid.ok_or("Process identity is unavailable; refresh the listener list.")?;
    #[cfg(windows)]
    return crate::procmgr::force_stop_windows_process(pid, listener.start_marker);
    #[cfg(not(windows))]
    {
        if listener.start_marker.is_none() {
            return Err("Process identity is unavailable; refresh the listener list.".into());
        }
        crate::procmgr::send_local_signal(pid, "TERM", listener.start_marker)
    }
}

pub fn listener_launch(listener: &ListenerInfo) -> Option<(String, String)> {
    #[cfg(target_os = "linux")]
    {
        let pid = listener.pid?;
        let marker = listener.start_marker?;
        if listener_process_start(pid) != Some(marker) {
            return None;
        }
        let proc_dir = std::path::PathBuf::from(format!("/proc/{pid}"));
        let executable = std::fs::read_link(proc_dir.join("exe")).ok()?;
        let cwd = std::fs::read_link(proc_dir.join("cwd")).ok()?;
        if !executable.is_file() || !cwd.is_dir() {
            return None;
        }
        let bytes = std::fs::read(proc_dir.join("cmdline")).ok()?;
        let argv = bytes.strip_suffix(&[0])?;
        let mut command = crate::remote::shell_quote(executable.to_str()?);
        for arg in argv.split(|&byte| byte == 0).skip(1) {
            command.push(' ');
            command.push_str(&crate::remote::shell_quote(std::str::from_utf8(arg).ok()?));
        }
        (listener_process_start(pid) == Some(marker)).then_some((command, cwd.to_str()?.into()))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = listener;
        None
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn stale_listener_identity_cannot_capture_launch_details() {
        let pid = std::process::id();
        let marker = listener_process_start(pid).unwrap();
        let listener = ListenerInfo {
            proto: "TCP".into(),
            port: 8080,
            pid: Some(pid),
            start_marker: Some(marker.wrapping_add(1)),
            name: "test".into(),
        };
        assert!(listener_launch(&listener).is_none());
    }
}
