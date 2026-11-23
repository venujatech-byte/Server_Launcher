//! Wi-Fi subsystem: telemetry queries, active AP details, link metrics, and socket monitoring.

use std::process::Command;
use std::time::Instant;
#[cfg(not(windows))]
use std::fs;

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
    fn windows_adapter_and_wlan_data_use_reported_values_only() {
        let data = serde_json::json!({
            "adapters": [{"name":"Wi-Fi", "description":"Intel Wireless", "status":"Up", "mac":"AA:BB:CC:DD:EE:FF", "mtu":1500,
                "ipv4":[{"address":"192.168.1.5", "prefix":24}], "ipv6":["fe80::1"], "gateway":["192.168.1.1"], "dns":["1.1.1.1"], "sent":2048}],
            "wlan":"Name : Wi-Fi\nState : connected\nSSID : Office\nBSSID : AA:BB:CC:11:22:33\nSignal : 78%\nChannel : 6\nReceive rate (Mbps) : 144\nTransmit rate (Mbps) : 144\nAuthentication : WPA2-Personal\n"
        });
        let snapshot = super::parse_windows_wifi_snapshot(&data, Some("Wi-Fi"));
        assert!(snapshot.is_connected);
        assert_eq!(snapshot.state, "CONNECTED");
        assert_eq!(snapshot.ssid, "Office");
        assert_eq!(snapshot.signal_pct, Some(78));
        assert_eq!(snapshot.ip_v4, "192.168.1.5/24");
        assert_eq!(snapshot.subnet_mask, "255.255.255.0 (/24)");
        assert_eq!(snapshot.tx_bytes, 2048);
    }

    #[test]
    fn missing_windows_wifi_inventory_stays_unavailable() {
        let snapshot = super::parse_windows_wifi_snapshot(&serde_json::json!({"adapters":[], "wlan":""}), None);
        assert!(snapshot.interface.is_empty());
        assert!(!snapshot.is_connected);
        assert_eq!(snapshot.state, "No wireless adapter reported");
    }

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
#[cfg(not(windows))]
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

#[cfg(windows)]
pub fn query_wifi_status(selected: Option<&str>) -> WifiSnapshot {
    use std::os::windows::process::CommandExt;

    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", WINDOWS_WIFI_QUERY])
        .creation_flags(0x08000000)
        .output();
    let Ok(output) = output else {
        return WifiSnapshot { state: "Wi-Fi telemetry unavailable".into(), last_updated: Instant::now(), ..Default::default() };
    };
    if !output.status.success() {
        return WifiSnapshot { state: "Wi-Fi telemetry unavailable".into(), last_updated: Instant::now(), ..Default::default() };
    }
    let Ok(data) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return WifiSnapshot { state: "Wi-Fi telemetry unavailable".into(), last_updated: Instant::now(), ..Default::default() };
    };
    parse_windows_wifi_snapshot(&data, selected)
}

#[cfg(windows)]
const WINDOWS_WIFI_QUERY: &str = r#"$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.UTF8Encoding]::new();
$adapters=@(Get-NetAdapter -IncludeHidden -ErrorAction Stop | Where-Object { $_.ifType -eq 71 });
$items=@(foreach($adapter in $adapters) {
  $ip=Get-NetIPConfiguration -InterfaceIndex $adapter.ifIndex -ErrorAction SilentlyContinue | Select-Object -First 1;
  $ipif=Get-NetIPInterface -InterfaceIndex $adapter.ifIndex -AddressFamily IPv4 -ErrorAction SilentlyContinue | Select-Object -First 1;
  $stats=Get-NetAdapterStatistics -Name $adapter.Name -ErrorAction SilentlyContinue;
  [pscustomobject]@{
    name=$adapter.Name; description=$adapter.InterfaceDescription; status=[string]$adapter.Status;
    mac=$adapter.MacAddress; driver=$adapter.DriverInformation; mtu=$ipif.NlMtu; index=$adapter.ifIndex;
    ipv4=@($ip.IPv4Address | ForEach-Object { @{address=$_.IPAddress; prefix=$_.PrefixLength} });
    ipv6=@($ip.IPv6Address | ForEach-Object { $_.IPAddress });
    gateway=@($ip.IPv4DefaultGateway | ForEach-Object { $_.NextHop });
    dns=@($ip.DNSServer.ServerAddresses);
    sent=$stats.SentBytes; received=$stats.ReceivedBytes
  }
});
$wlan=(netsh wlan show interfaces 2>$null | Out-String);
[pscustomobject]@{adapters=$items; wlan=$wlan} | ConvertTo-Json -Depth 6 -Compress"#;

#[cfg(any(windows, test))]
fn parse_windows_wifi_snapshot(data: &serde_json::Value, selected: Option<&str>) -> WifiSnapshot {
    let rows = data["adapters"].as_array().cloned().unwrap_or_default();
    let candidates: Vec<_> = rows.iter().filter_map(|row| {
        let name = row["name"].as_str()?.trim();
        (!name.is_empty()).then(|| (name.to_owned(), row["status"].as_str().is_some_and(|state| state.eq_ignore_ascii_case("up"))))
    }).collect();
    let mut snapshot = WifiSnapshot { last_updated: Instant::now(), interfaces: candidates.iter().map(|(name, _)| name.clone()).collect(), ..Default::default() };
    let Some(interface) = choose_wifi_interface(&candidates, selected) else {
        snapshot.state = "No wireless adapter reported".into();
        return snapshot;
    };
    let Some(adapter) = rows.iter().find(|row| row["name"].as_str() == Some(interface.as_str())) else { return snapshot; };
    let wlan = parse_windows_wlan_interface(data["wlan"].as_str().unwrap_or(""), &interface);
    let adapter_status = adapter["status"].as_str().unwrap_or("Unknown");
    snapshot.interface = interface;
    snapshot.product = adapter["description"].as_str().filter(|value| !value.trim().is_empty()).unwrap_or("Wi-Fi adapter").into();
    snapshot.mac_address = adapter["mac"].as_str().unwrap_or("").into();
    snapshot.driver = adapter["driver"].as_str().unwrap_or("").into();
    snapshot.mtu = adapter["mtu"].as_u64().and_then(|value| u32::try_from(value).ok()).unwrap_or(0);
    snapshot.tx_bytes = adapter["sent"].as_u64().unwrap_or(0);
    snapshot.state = wlan.state.unwrap_or_else(|| adapter_status.to_ascii_uppercase());
    snapshot.is_connected = snapshot.state.eq_ignore_ascii_case("connected");
    snapshot.ssid = wlan.ssid.unwrap_or_default();
    snapshot.bssid = wlan.bssid.unwrap_or_default();
    snapshot.signal_pct = wlan.signal_pct;
    snapshot.channel = wlan.channel.unwrap_or(0);
    snapshot.rate = wlan.rate.unwrap_or_default();
    snapshot.security = wlan.security.unwrap_or_default();
    if let Some(address) = adapter["ipv4"].as_array().and_then(|addresses| addresses.first()) {
        let ip = address["address"].as_str().unwrap_or("");
        let prefix = address["prefix"].as_u64().and_then(|value| u8::try_from(value).ok()).filter(|value| *value <= 32);
        if !ip.is_empty() {
            snapshot.ip_v4 = prefix.map_or_else(|| ip.into(), |prefix| format!("{ip}/{prefix}"));
            if let Some(prefix) = prefix { snapshot.subnet_mask = cidr_to_subnet_mask(prefix); }
        }
    }
    snapshot.ip_v6 = adapter["ipv6"].as_array().and_then(|values| values.iter().find_map(|value| value.as_str())).unwrap_or("").into();
    snapshot.gateway = adapter["gateway"].as_array().and_then(|values| values.iter().find_map(|value| value.as_str())).unwrap_or("").into();
    snapshot.dns_resolvers = adapter["dns"].as_array().map(|values| values.iter().filter_map(|value| value.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    snapshot
}

#[cfg(any(windows, test))]
#[derive(Default)]
struct WindowsWlanInterface {
    state: Option<String>, ssid: Option<String>, bssid: Option<String>, signal_pct: Option<u8>,
    channel: Option<u32>, rate: Option<String>, security: Option<String>,
}

#[cfg(any(windows, test))]
fn parse_windows_wlan_interface(text: &str, adapter: &str) -> WindowsWlanInterface {
    // shortcut: netsh field names are localized; use WLAN API calls if non-English Windows needs reliable association details.
    let mut sections = Vec::<Vec<(String, String)>>::new();
    let mut section = Vec::<(String, String)>::new();
    for line in text.lines().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if !section.is_empty() { sections.push(section.drain(..).collect()); }
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            section.push((key.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    let fields = sections.iter().find(|section| section.iter().any(|(key, value)| key == "name" && value.eq_ignore_ascii_case(adapter)));
    let Some(fields) = fields else { return WindowsWlanInterface::default(); };
    let get = |key: &str| fields.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str());
    let state = get("state").filter(|value| matches!(value.to_ascii_lowercase().as_str(), "connected" | "disconnected")).map(str::to_ascii_uppercase);
    let signal_pct = get("signal").and_then(|value| value.trim_end_matches('%').parse::<u8>().ok()).filter(|value| *value <= 100);
    let channel = get("channel").and_then(|value| value.parse::<u32>().ok()).filter(|value| *value > 0);
    let rx = get("receive rate (mbps)");
    let tx = get("transmit rate (mbps)");
    let rate = (rx.is_some() || tx.is_some()).then(|| format!("RX {} / TX {} Mbps", rx.unwrap_or("—"), tx.unwrap_or("—")));
    WindowsWlanInterface { state, ssid: get("ssid").filter(|value| !value.is_empty()).map(str::to_owned), bssid: get("bssid").map(str::to_owned), signal_pct, channel, rate, security: get("authentication").map(str::to_owned) }
}

fn command_result(success: bool, stderr: &str, operation: &str) -> Result<(), String> {
    if success { Ok(()) } else if stderr.trim().is_empty() { Err(format!("{operation} failed")) } else { Err(format!("{operation} failed: {}", stderr.trim())) }
}

/// Request a NetworkManager scan on one selected interface.
#[cfg(not(windows))]
pub fn scan_wifi(interface: &str) -> Result<String, String> {
    if interface.is_empty() || interface.len() > 15 || !interface.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')) {
        return Err("Invalid Wi-Fi interface".into());
    }
    let output = Command::new("nmcli").args(["--wait", "15", "dev", "wifi", "rescan", "ifname", interface]).output()
        .map_err(|error| format!("Could not start Wi-Fi scan: {error}"))?;
    command_result(output.status.success(), &String::from_utf8_lossy(&output.stderr), "Wi-Fi scan")?;
    Ok(format!("Wi-Fi scan requested for {interface}"))
}

#[cfg(windows)]
pub fn scan_wifi(interface: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    if interface.is_empty() || interface.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0)) { return Err("Invalid Wi-Fi interface".into()); }
    let selector = format!("interface={interface}");
    let output = Command::new("netsh.exe").args(["wlan", "show", "networks", selector.as_str(), "mode=bssid"])
        .creation_flags(0x08000000).output().map_err(|error| format!("Could not start Wi-Fi scan: {error}"))?;
    command_result(output.status.success(), &String::from_utf8_lossy(&output.stderr), "Wi-Fi scan")?;
    Ok(format!("Wi-Fi scan requested for {interface}"))
}

/// Flush the local resolver cache, preserving errors from the available tool.
#[cfg(not(windows))]
pub fn flush_dns() -> Result<String, String> {
    for program in ["resolvectl", "systemd-resolve"] {
        let argument = if program == "systemd-resolve" { "--flush-caches" } else { "flush-caches" };
        match Command::new(program).arg(argument).output() {
            Ok(output) => {
                command_result(output.status.success(), &String::from_utf8_lossy(&output.stderr), "DNS cache flush")?;
                return Ok("DNS resolver cache flushed".into());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("Could not start DNS cache flush: {error}")),
        }
    }
    Err("DNS cache flush is unavailable: no supported resolver tool found".into())
}

#[cfg(windows)]
pub fn flush_dns() -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let output = Command::new("ipconfig.exe").arg("/flushdns").creation_flags(0x08000000).output()
        .map_err(|error| format!("Could not start DNS cache flush: {error}"))?;
    let detail = if output.stderr.is_empty() { String::from_utf8_lossy(&output.stdout) } else { String::from_utf8_lossy(&output.stderr) };
    command_result(output.status.success(), &detail, "DNS cache flush")?;
    Ok("DNS resolver cache flushed".into())
}

#[cfg(test)]
mod wifi_action_tests {
    #[test]
    fn command_errors_and_invalid_interfaces_are_reported() {
        assert!(super::command_result(false, "permission denied", "scan").unwrap_err().contains("permission denied"));
        assert!(super::command_result(false, "", "scan").is_err());
        assert!(super::command_result(true, "", "scan").is_ok());
        assert!(super::scan_wifi("../bad").is_err());
    }
}
