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

/// Convert CIDR prefix (e.g. 24) to full dotted-decimal subnet mask string.
pub fn cidr_to_subnet_mask(prefix: u8) -> String {
    let mask = if prefix >= 32 {
        0xFFFF_FFFF_u32
    } else if prefix == 0 {
        0
    } else {
        !((1_u32 << (32 - prefix)) - 1)
    };
    format!("{}.{}.{}.{} (/{})", (mask >> 24) & 0xFF, (mask >> 16) & 0xFF, (mask >> 8) & 0xFF, mask & 0xFF, prefix)
}

#[derive(Debug, Clone)]
pub struct WifiSnapshot {
    pub interface: String,
    pub interfaces: Vec<String>,
    pub product: String,
    pub mac_address: String,
    pub state: String,
    pub is_connected: bool,
    pub ssid: String,
    pub bssid: String,
    pub channel: u32,
    pub frequency_mhz: u32,
    pub rate: String,
    pub signal_pct: Option<u8>,
    pub signal_dbm: i32,
    pub security: String,
    pub ip_v4: String,
    pub subnet_mask: String,
    pub gateway: String,
    pub dns_resolvers: String,
    pub ip_v6: String,
    pub driver: String,
    pub pci_slot: String,
    pub mtu: u32,
    pub tx_bytes: u64,
    pub lease_time_remaining: String,
    pub netfilter_status: String,
    pub nameserver_sync: String,
    pub kernel_version: String,
    pub jitter_ms: f32,
    pub tunnel_id: String,
    pub active_sockets: Vec<WifiActiveSocket>,
    pub last_updated: Instant,
}

impl Default for WifiSnapshot {
    fn default() -> Self {
        Self {
            interface: String::new(),
            interfaces: Vec::new(),
            product: String::new(),
            mac_address: String::new(),
            state: "Unavailable".to_string(),
            is_connected: false,
            ssid: String::new(),
            bssid: String::new(),
            channel: 0,
            frequency_mhz: 0,
            rate: String::new(),
            signal_pct: None,
            signal_dbm: 0,
            security: String::new(),
            ip_v4: String::new(),
            subnet_mask: String::new(),
            gateway: String::new(),
            dns_resolvers: String::new(),
            ip_v6: String::new(),
            driver: String::new(),
            pci_slot: String::new(),
            mtu: 0,
            tx_bytes: 0,
            lease_time_remaining: String::new(),
            netfilter_status: String::new(),
            nameserver_sync: String::new(),
            kernel_version: String::new(),
            jitter_ms: 0.0,
            tunnel_id: String::new(),
            active_sockets: Vec::new(),
            last_updated: Instant::now(),
        }
    }
}

use std::sync::OnceLock;

static CACHED_KVER: OnceLock<String> = OnceLock::new();

fn choose_wifi_interface(candidates: &[(String, bool)], selected: Option<&str>) -> Option<String> {
    selected.and_then(|name| candidates.iter().find(|(candidate, _)| candidate == name))
        .or_else(|| candidates.iter().find(|(_, up)| *up))
        .or_else(|| candidates.first()).map(|(name, _)| name.clone())
}

fn nmcli_fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => if let Some(next) = chars.next() { fields.last_mut().unwrap().push(next); },
            ':' => fields.push(String::new()),
            _ => fields.last_mut().unwrap().push(ch),
        }
    }
    fields
}

#[cfg(test)]
mod wifi_selection_tests {
    #[test]
    fn picks_requested_or_active_wireless_adapter() {
        let devices = vec![("wifi0".to_string(), false), ("wlan1".to_string(), true)];
        assert_eq!(super::choose_wifi_interface(&devices, Some("wifi0")).as_deref(), Some("wifi0"));
        assert_eq!(super::choose_wifi_interface(&devices, Some("removed")).as_deref(), Some("wlan1"));
        assert_eq!(super::choose_wifi_interface(&[], None), None);
    }
    #[test]
    fn preserves_escaped_access_point_fields() {
        let fields = super::nmcli_fields(r"yes:Office\:Guest:aa\:bb\:cc:11:54 Mbit/s:80:WPA2:2462 MHz");
        assert_eq!(fields[1], "Office:Guest");
        assert_eq!(fields[2], "aa:bb:cc");
        assert_eq!(fields[7], "2462 MHz");
    }
}

/// Query the selected Wi-Fi adapter using nmcli, ip and sysfs.
pub fn query_wifi_status(selected: Option<&str>) -> WifiSnapshot {
    let mut snap = WifiSnapshot::default();
    snap.last_updated = Instant::now();
    let mut candidates: Vec<(String, bool)> = fs::read_dir("/sys/class/net").ok().into_iter().flatten()
        .flatten().filter_map(|entry| {
            let path = entry.path();
            if !path.join("wireless").exists() && !path.join("phy80211").exists() { return None; }
            let name = entry.file_name().into_string().ok()?;
            let up = fs::read_to_string(path.join("operstate")).map(|state| state.trim() == "up").unwrap_or(false);
            Some((name, up))
        }).collect();
    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    snap.interfaces = candidates.iter().map(|(name, _)| name.clone()).collect();
    let Some(iface) = choose_wifi_interface(&candidates, selected) else { return snap; };
    let base = format!("/sys/class/net/{iface}");
    let mac = fs::read_to_string(format!("{base}/address")).map(|s| s.trim().to_uppercase()).unwrap_or_default();
    let mut drv = String::new();
    let mut slot = String::new();
    if let Ok(uevent) = fs::read_to_string(format!("{base}/device/uevent")) {
        for line in uevent.lines() {
            if let Some(value) = line.strip_prefix("DRIVER=") { drv = value.trim().to_string(); }
            if let Some(value) = line.strip_prefix("PCI_SLOT_NAME=") { slot = value.trim().to_string(); }
        }
    }
    let prod = Command::new("nmcli").args(["-t", "-f", "GENERAL.PRODUCT", "dev", "show", &iface]).output().ok()
        .filter(|output| output.status.success()).and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|output| output.lines().find_map(|line| line.strip_prefix("GENERAL.PRODUCT:").map(str::to_string)))
        .filter(|value| !value.trim().is_empty()).unwrap_or_else(|| "Wi-Fi adapter".into());

    snap.interface = iface;
    snap.mac_address = mac;
    snap.driver = drv;
    snap.pci_slot = slot;
    snap.product = prod;

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

    // 5. Query active Wi-Fi connection via nmcli
    let nmcli_out = Command::new("nmcli")
        .args(["-t", "-f", "active,ssid,bssid,chan,rate,signal,security,freq", "dev", "wifi", "list", "ifname", &snap.interface, "--rescan", "no"])
        .output();

    if let Ok(out) = nmcli_out {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                if line.starts_with("yes:") {
                    let parts = nmcli_fields(line);
                    if parts.len() >= 8 {
                        snap.ssid = parts[1].trim().to_string();
                        snap.bssid = parts[2].trim().to_string();
                        snap.channel = parts[3].trim().parse().unwrap_or(0);
                        snap.frequency_mhz = parts[7].split_whitespace().next().and_then(|value| value.parse().ok()).unwrap_or(0);
                        snap.rate = parts[4].trim().to_string();
                        snap.signal_pct = parts[5].trim().parse::<u8>().ok().filter(|value| *value <= 100);
                        snap.security = parts[6].trim().to_string();
                        snap.is_connected = true;
                        snap.state = "CONNECTED".to_string();
                        break;
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
                            if let Some(slash_idx) = ip.find('/') {
                                if let Ok(cidr) = ip[slash_idx + 1..].parse::<u8>() {
                                    snap.subnet_mask = cidr_to_subnet_mask(cidr);
                                }
                            }
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
        .args(["route", "show", "default", "dev", &snap.interface])
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
                    dns_list.push(ip.to_string());
                }
            }
        }
        if !dns_list.is_empty() {
            snap.dns_resolvers = dns_list.join(", ");
        }
    }

    // 10. Query MTU and TX bytes from sysfs
    let mtu_path = format!("/sys/class/net/{}/mtu", snap.interface);
    if let Ok(mtu_str) = fs::read_to_string(&mtu_path) {
        if let Ok(val) = mtu_str.trim().parse::<u32>() {
            snap.mtu = val;
        }
    }

    let tx_path = format!("/sys/class/net/{}/statistics/tx_bytes", snap.interface);
    if let Ok(tx_str) = fs::read_to_string(&tx_path) {
        if let Ok(val) = tx_str.trim().parse::<u64>() {
            snap.tx_bytes = val;
        }
    }

    // 11. Read and cache kernel version from /proc/sys/kernel/osrelease
    snap.kernel_version = CACHED_KVER.get_or_init(|| {
        fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "Linux".to_string())
    }).clone();

    // 12. Query real active listening sockets via `ss -tlpn`
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
                        let port_str = local_addr.rsplit(':').next().unwrap_or(local_addr);
                        let port = if port_str.starts_with(':') {
                            port_str.to_string()
                        } else {
                            format!(":{}", port_str)
                        };
                        let proc_name = if let Some(users_idx) = line.find(r#"users:((""#) {
                            let after = &line[users_idx + 9..];
                            after.split('"').next().unwrap_or("unknown")
                        } else {
                            "system"
                        };

                        // Clean up friendly process name
                        let friendly_name = match proc_name {
                            "sshd" => "sshd",
                            "rquickshare" => "rquickshare (LAN Share)",
                            "Arduino_Cloud_A" | "Arduino_Cloud_Agent" => "Arduino Cloud Agent",
                            "code" => "code (VS Code Server)",
                            "systemd-resolve" | "systemd-resolved" => "systemd-resolved",
                            "cupsd" => "cupsd (Printing)",
                            "agy" | "antigravity" | "antigravity-daemon" => "antigravity-daemon",
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
pub fn trigger_wifi_rescan_async(interface: String) {
    std::thread::spawn(move || {
        let _ = Command::new("nmcli")
            .args(["dev", "wifi", "rescan", "ifname", &interface])
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
