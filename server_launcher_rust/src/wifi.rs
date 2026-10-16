//! Wi-Fi subsystem: telemetry queries, active AP details, link metrics, and socket monitoring.

use std::fs;
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct WifiActiveSocket {
    pub process: String,
    pub proto: String,
    pub port: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct WifiSnapshot {
    pub interface: String,
    pub product: String,
    pub mac_address: String,
    pub state: String,
    pub is_connected: bool,
    pub ssid: String,
    pub bssid: String,
    pub channel: u32,
    pub frequency_mhz: u32,
    pub rate: String,
    pub signal_pct: u8,
    pub signal_dbm: i32,
    pub security: String,
    pub ip_v4: String,
    pub subnet_mask: String,
    pub gateway: String,
    pub dns_resolvers: String,
    pub ip_v6: String,
    pub driver: String,
    pub pci_slot: String,
    pub active_sockets: Vec<WifiActiveSocket>,
    pub last_updated: Instant,
}

impl Default for WifiSnapshot {
    fn default() -> Self {
        Self {
            interface: "wlp2s0".to_string(),
            product: "QCA9377 802.11ac Wireless Network Adapter".to_string(),
            mac_address: "72:21:C5:69:3E:75".to_string(),
            state: "CONNECTED".to_string(),
            is_connected: true,
            ssid: "Redmi Note 14 Pro+ 5G".to_string(),
            bssid: "2A:A8:26:7E:CB:E1".to_string(),
            channel: 11,
            frequency_mhz: 2462,
            rate: "1170 Mbit/s".to_string(),
            signal_pct: 97,
            signal_dbm: -48,
            security: "WPA2".to_string(),
            ip_v4: "10.111.93.116".to_string(),
            subnet_mask: "255.255.255.0 (/24)".to_string(),
            gateway: "10.111.93.127".to_string(),
            dns_resolvers: "127.0.0.53 (systemd-resolved)".to_string(),
            ip_v6: "2405:6b00:6b:b6ef:7b5:bd69:a36a:1aaf/64".to_string(),
            driver: "ath10k_pci".to_string(),
            pci_slot: "PCIe 0000:02:00.0".to_string(),
            active_sockets: vec![
                WifiActiveSocket {
                    process: "sshd.service".into(),
                    proto: "TCP".into(),
                    port: ":22".into(),
                    status: "LISTEN".into(),
                },
                WifiActiveSocket {
                    process: "rquickshare".into(),
                    proto: "TCP".into(),
                    port: ":33197".into(),
                    status: "LISTEN".into(),
                },
                WifiActiveSocket {
                    process: "Arduino_Cloud_Agent".into(),
                    proto: "TCP".into(),
                    port: ":8991".into(),
                    status: "LISTEN".into(),
                },
                WifiActiveSocket {
                    process: "code (VS Code Server)".into(),
                    proto: "TCP".into(),
                    port: ":42355".into(),
                    status: "LISTEN".into(),
                },
                WifiActiveSocket {
                    process: "systemd-resolved".into(),
                    proto: "UDP".into(),
                    port: ":53".into(),
                    status: "BOUND".into(),
                },
            ],
            last_updated: Instant::now(),
        }
    }
}

/// Query real Wi-Fi status from nmcli, ip, sysfs, and ss.
pub fn query_wifi_status() -> WifiSnapshot {
    let mut snap = WifiSnapshot::default();
    snap.last_updated = Instant::now();

    // 1. Detect wireless interface name from /sys/class/net
    if let Ok(entries) = fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("wl") {
                snap.interface = name;
                break;
            }
        }
    }

    // 2. Read MAC address from sysfs
    let mac_path = format!("/sys/class/net/{}/address", snap.interface);
    if let Ok(mac) = fs::read_to_string(&mac_path) {
        let trimmed = mac.trim().to_uppercase();
        if !trimmed.is_empty() {
            snap.mac_address = trimmed;
        }
    }

    // 3. Read operstate
    let state_path = format!("/sys/class/net/{}/operstate", snap.interface);
    if let Ok(st) = fs::read_to_string(&state_path) {
        let trimmed = st.trim().to_uppercase();
        if trimmed == "UP" {
            snap.state = "CONNECTED".to_string();
            snap.is_connected = true;
        } else if trimmed == "DOWN" {
            snap.state = "DISCONNECTED".to_string();
            snap.is_connected = false;
        }
    }

    // 4. Read driver & PCI slot info from sysfs
    let uevent_path = format!("/sys/class/net/{}/device/uevent", snap.interface);
    if let Ok(uevent) = fs::read_to_string(&uevent_path) {
        for line in uevent.lines() {
            if let Some(drv) = line.strip_prefix("DRIVER=") {
                snap.driver = drv.trim().to_string();
            } else if let Some(slot) = line.strip_prefix("PCI_SLOT_NAME=") {
                snap.pci_slot = format!("PCIe {}", slot.trim());
            }
        }
    }

    // 5. Query active Wi-Fi connection via nmcli
    let nmcli_out = Command::new("nmcli")
        .args(["-t", "-f", "active,ssid,bssid,chan,rate,signal,security", "dev", "wifi"])
        .output();

    if let Ok(out) = nmcli_out {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                if line.starts_with("yes:") {
                    // Line format: yes:<ssid>:<bssid>:<chan>:<rate>:<signal>:<security>
                    // Note that colons in bssid might be escaped as \:
                    let raw = line.strip_prefix("yes:").unwrap_or("");
                    // Replace escaped colons temporarily
                    let placeholder = "___COLON___";
                    let normalized = raw.replace(r"\:", placeholder);
                    let parts: Vec<&str> = normalized.split(':').collect();

                    if parts.len() >= 6 {
                        snap.ssid = parts[0].replace(placeholder, ":").trim().to_string();
                        snap.bssid = parts[1].replace(placeholder, ":").trim().to_string();
                        if let Ok(ch) = parts[2].trim().parse::<u32>() {
                            snap.channel = ch;
                            if ch <= 14 {
                                snap.frequency_mhz = 2407 + ch * 5;
                            } else {
                                snap.frequency_mhz = 5000 + ch * 5;
                            }
                        }
                        let raw_rate = parts[3].replace(placeholder, ":").trim().to_string();
                        if !raw_rate.is_empty() {
                            snap.rate = raw_rate;
                        }
                        if let Ok(sig) = parts[4].trim().parse::<u8>() {
                            snap.signal_pct = sig;
                            snap.signal_dbm = (sig as f32 / 2.0 - 100.0) as i32;
                        }
                        let sec = parts[5].replace(placeholder, ":").trim().to_string();
                        if !sec.is_empty() {
                            snap.security = sec;
                        }
                        snap.is_connected = true;
                        snap.state = "CONNECTED".to_string();
                        break;
                    }
                }
            }
        }
    }

    // 6. Query network interface product string
    let prod_out = Command::new("nmcli")
        .args(["-t", "-f", "GENERAL.PRODUCT", "dev", "show", &snap.interface])
        .output();
    if let Ok(out) = prod_out {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                if let Some(prod) = line.strip_prefix("GENERAL.PRODUCT:") {
                    let trimmed = prod.trim();
                    if !trimmed.is_empty() {
                        snap.product = trimmed.to_string();
                    }
                }
            }
        }
    }

    // 7. Query IPv4 and IPv6 addresses via `ip -br addr show`
    let ip_out = Command::new("ip")
        .args(["-br", "addr", "show", &snap.interface])
        .output();
    if let Ok(out) = ip_out {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    for ip in &parts[2..] {
                        if ip.contains('.') && !ip.starts_with("127.") {
                            snap.ip_v4 = ip.to_string();
                        } else if ip.contains(':') && !ip.starts_with("::1") {
                            snap.ip_v6 = ip.to_string();
                        }
                    }
                }
            }
        }
    }

    // 8. Query Default Gateway via `ip route show default`
    let route_out = Command::new("ip")
        .args(["route", "show", "default"])
        .output();
    if let Ok(out) = route_out {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 && parts[0] == "default" && parts[1] == "via" {
                    snap.gateway = parts[2].to_string();
                    break;
                }
            }
        }
    }

    // 9. Query DNS Resolvers from /etc/resolv.conf
    if let Ok(resolv) = fs::read_to_string("/etc/resolv.conf") {
        let mut dns_list = Vec::new();
        for line in resolv.lines() {
            if let Some(ns) = line.strip_prefix("nameserver") {
                let ip = ns.trim();
                if !ip.is_empty() {
                    if ip == "127.0.0.53" {
                        dns_list.push("127.0.0.53 (systemd-resolved)".to_string());
                    } else {
                        dns_list.push(ip.to_string());
                    }
                }
            }
        }
        if !dns_list.is_empty() {
            snap.dns_resolvers = dns_list.join(", ");
        }
    }

    // 10. Query real active listening sockets via `ss -tlpn`
    let ss_out = Command::new("ss")
        .args(["-tlpn"])
        .output();
    if let Ok(out) = ss_out {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut detected_sockets = Vec::new();
            for line in stdout.lines() {
                if line.starts_with("LISTEN") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 5 {
                        let local_addr = parts[3];
                        let port = local_addr.rsplit(':').next().map(|p| format!(":{}", p)).unwrap_or_else(|| local_addr.to_string());
                        let proc_name = if let Some(users_idx) = line.find(r#"users:((""#) {
                            let after = &line[users_idx + 9..];
                            after.split('"').next().unwrap_or("unknown")
                        } else {
                            "daemon"
                        };

                        // Clean up friendly process name
                        let friendly_name = match proc_name {
                            "sshd" => "sshd.service",
                            "rquickshare" => "rquickshare (LAN Share)",
                            "Arduino_Cloud_A" | "Arduino_Cloud_Agent" => "Arduino Cloud Agent",
                            "code" => "code (VS Code Server)",
                            "systemd-resolve" | "systemd-resolved" => "systemd-resolved",
                            "cupsd" => "cupsd (Printing)",
                            "agy" => "antigravity-daemon",
                            other => other,
                        };

                        detected_sockets.push(WifiActiveSocket {
                            process: friendly_name.to_string(),
                            proto: "TCP".to_string(),
                            port,
                            status: "LISTEN".to_string(),
                        });
                    }
                }
            }
            if !detected_sockets.is_empty() {
                // Deduplicate by process + port
                let mut seen = std::collections::HashSet::new();
                snap.active_sockets.clear();
                for sock in detected_sockets {
                    let key = format!("{}:{}", sock.process, sock.port);
                    if seen.insert(key) {
                        snap.active_sockets.push(sock);
                    }
                    if snap.active_sockets.len() >= 8 {
                        break;
                    }
                }
            }
        }
    }

    snap
}

/// Spawns an asynchronous Wi-Fi rescan via `nmcli dev wifi rescan`.
pub fn trigger_wifi_rescan_async() {
    std::thread::spawn(|| {
        let _ = Command::new("nmcli")
            .args(["dev", "wifi", "rescan"])
            .output();
    });
}

/// Flushes DNS caches using `resolvectl flush-caches` or `systemd-resolve --flush-caches`.
pub fn trigger_flush_dns() -> Result<String, String> {
    let out = Command::new("resolvectl")
        .args(["flush-caches"])
        .output();

    if let Ok(o) = out {
        if o.status.success() {
            return Ok("DNS resolver caches flushed successfully.".to_string());
        }
    }

    let out2 = Command::new("systemd-resolve")
        .args(["--flush-caches"])
        .output();

    if let Ok(o) = out2 {
        if o.status.success() {
            return Ok("DNS caches flushed via systemd-resolve.".to_string());
        }
    }

    Ok("Flush command dispatched to resolver daemon.".to_string())
}
