use std::collections::BTreeMap;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerInfo {
    pub proto: String,
    pub port: u16,
    pub pid: Option<u32>,
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

fn scan_system_listening_ports() -> Vec<ListenerInfo> {
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
                                    name,
                                },
                            );
                        }
                    }
                }
            }
        }

        // 2. Fallback to /proc/net/tcp and tcp6 if ss returned nothing
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
                                    name: "system/other".to_string(),
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    map.into_values().collect()
}
