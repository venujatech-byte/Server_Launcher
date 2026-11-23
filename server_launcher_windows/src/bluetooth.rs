// ─────────────────────────────────────────────────────────────────────────────
// Bluetooth Subsystem Manager & BlueZ Controller Interface
// ─────────────────────────────────────────────────────────────────────────────

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use crate::config::SshRemoteHost;

#[derive(Debug, Clone, Default)]
pub struct BluetoothDevice {
    pub mac: String,
    pub name: String,
    pub alias: String,
    pub icon: String,
    pub paired: bool,
    pub connected: bool,
    pub trusted: bool,
    pub blocked: bool,
    pub battery_pct: Option<u8>,
    pub rssi: Option<i16>,
    pub uuids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BluetoothController {
    pub mac: String,
    pub name: String,
    pub alias: String,
    pub powered: bool,
    pub discoverable: bool,
    pub discoverable_reported: bool,
    pub pairable: bool,
    pub discovering: bool,
    pub manufacturer: String,
    pub version: String,
}

#[derive(Debug, Clone, Default)]
pub struct BluetoothSnapshot {
    pub available: bool,
    pub controller: Option<BluetoothController>,
    pub devices: Vec<BluetoothDevice>,
    pub discovered_devices: Vec<BluetoothDevice>,
    pub last_updated: Option<Instant>,
    pub is_scanning: bool,
    pub status_msg: Option<(String, bool, Instant)>,
}

impl BluetoothSnapshot {
    pub fn get_connected_count(&self) -> usize {
        self.devices.iter().filter(|d| d.connected).count()
    }

    pub fn get_paired_count(&self) -> usize {
        self.devices.iter().filter(|d| d.paired).count()
    }
}

/// Query BlueZ via `bluetoothctl` to fetch real adapter state and connected/paired devices.
pub fn query_bluetooth_status() -> BluetoothSnapshot {
    if cfg!(windows) {
        return query_local_windows_bluetooth_status();
    }
    let mut snap = BluetoothSnapshot::default();
    snap.last_updated = Some(Instant::now());

    // 1. Query controller via `bluetoothctl show`
    let show_output = Command::new("bluetoothctl")
        .arg("show")
        .output();

    if let Ok(out) = show_output {
        if out.status.success() {
            snap.controller = parse_controller_show(&String::from_utf8_lossy(&out.stdout));
            snap.available = snap.controller.is_some();
        }
    }
    if !snap.available { return snap; }

    // 2. Query known paired/connected devices via `bluetoothctl devices`
    if let Ok(devices_out) = Command::new("bluetoothctl").arg("devices").output() {
        if devices_out.status.success() {
            let dev_text = String::from_utf8_lossy(&devices_out.stdout);
            for line in dev_text.lines() {
                // Format: Device 41:42:80:43:55:B1 联想thinkplus-GM2 pro
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 && parts[0] == "Device" {
                    let mac = parts[1].to_string();
                    let name = parts[2..].join(" ");
                    
                    // Fetch full info for this device
                    let mut dev = query_device_info(&mac);
                    if dev.name.is_empty() {
                        dev.name = clean_device_name(&name);
                    }
                    if dev.paired {
                        snap.devices.push(dev);
                    } else {
                        snap.discovered_devices.push(dev);
                    }
                }
            }
        }
    }

    // Sort devices: Connected devices first, then alphabetically
    snap.devices.sort_by(|a, b| {
        b.connected.cmp(&a.connected).then_with(|| a.name.cmp(&b.name))
    });

    snap
}

/// Read Bluetooth state from a Linux SSH host without changing its radio state.
pub fn query_remote_bluetooth_status(host: &SshRemoteHost) -> Result<BluetoothSnapshot, String> {
    let script = r#"export LC_ALL=C
if ! command -v bluetoothctl >/dev/null 2>&1; then echo 'bluetoothctl is not installed' >&2; exit 127; fi
printf '__CONTROLLER__\n'
bluetoothctl show
printf '\n__DEVICES__\n'
bluetoothctl devices | while read -r kind mac rest; do
    [ "$kind" = Device ] || continue
    case "$mac" in [0-9A-Fa-f][0-9A-Fa-f]:[0-9A-Fa-f][0-9A-Fa-f]:[0-9A-Fa-f][0-9A-Fa-f]:[0-9A-Fa-f][0-9A-Fa-f]:[0-9A-Fa-f][0-9A-Fa-f]:[0-9A-Fa-f][0-9A-Fa-f]) ;; *) continue ;; esac
    printf '\n__DEVICE__%s\n' "$mac"
    bluetoothctl info "$mac"
done"#;
    let output = crate::remote::build_ssh_command(host, script)
        .output()
        .map_err(|e| format!("Failed to run remote bluetoothctl: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() { "Remote Bluetooth query failed".into() } else { stderr });
    }
    parse_remote_bluetooth_output(&String::from_utf8_lossy(&output.stdout))
}

fn encoded_windows_bluetooth_script() -> String {
    use base64::Engine;

    let script = include_str!("../../shared/remote_windows_bluetooth.ps1");
    base64::engine::general_purpose::STANDARD.encode(script.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>())
}

fn windows_bluetooth_command() -> String {
    format!("powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {}", encoded_windows_bluetooth_script())
}

fn parse_windows_bluetooth_output(text: &str) -> Result<BluetoothSnapshot, String> {
    let parsed: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| format!("Invalid Windows Bluetooth response: {error}"))?;
    let devices = parsed["devices"].as_array()
        .ok_or_else(|| "Windows Bluetooth response has no device list".to_string())?;
    let mut snapshot = BluetoothSnapshot { available: parsed["available"].as_bool() == Some(true), last_updated: Some(Instant::now()), ..Default::default() };
    for device in devices {
        let Some(mac) = device["mac"].as_str().filter(|mac| valid_bluetooth_address(mac)) else { continue; };
        if !device["paired"].as_bool().unwrap_or(false) { continue; }
        let name = device["name"].as_str().filter(|name| !name.trim().is_empty()).unwrap_or(mac);
        snapshot.devices.push(BluetoothDevice {
            mac: mac.to_ascii_uppercase(), name: clean_device_name(name), paired: true,
            connected: device["connected"].as_bool().unwrap_or(false), ..Default::default()
        });
    }
    snapshot.devices.sort_by(|a, b| b.connected.cmp(&a.connected).then_with(|| a.name.cmp(&b.name)));
    Ok(snapshot)
}

/// Read remembered devices locally or remotely without changing Windows Bluetooth state.
pub fn query_remote_windows_bluetooth_status(host: &SshRemoteHost) -> Result<BluetoothSnapshot, String> {
    let command = windows_bluetooth_command();
    let output = crate::remote::build_ssh_command(host, &command)
        .output()
        .map_err(|error| format!("Failed to run the remote Windows Bluetooth query: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() { "Remote Windows Bluetooth query failed".into() } else { stderr });
    }
    parse_windows_bluetooth_output(&String::from_utf8_lossy(&output.stdout))
}

pub fn query_local_windows_bluetooth_status() -> BluetoothSnapshot {
    let mut command = Command::new("powershell.exe");
    command.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand"])
        .arg(encoded_windows_bluetooth_script());
    crate::commands::hide_console(&mut command);
    let result = command.output();
    match result {
        Ok(output) if output.status.success() => parse_windows_bluetooth_output(&String::from_utf8_lossy(&output.stdout)),
        Ok(output) => Err({
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.is_empty() { "Windows Bluetooth query failed".into() } else { stderr }
        }),
        Err(error) => Err(format!("Failed to run the Windows Bluetooth query: {error}")),
    }.unwrap_or_else(|error| BluetoothSnapshot {
        status_msg: Some((error, true, Instant::now())), last_updated: Some(Instant::now()), ..Default::default()
    })
}

fn parse_remote_bluetooth_output(text: &str) -> Result<BluetoothSnapshot, String> {
    let mut snapshot = BluetoothSnapshot { last_updated: Some(Instant::now()), ..Default::default() };
    let Some((_, rest)) = text.split_once("__CONTROLLER__\n") else { return Err("Remote Bluetooth response was incomplete".into()) };
    let Some((controller_text, devices_text)) = rest.split_once("\n__DEVICES__\n") else { return Err("Remote Bluetooth response was incomplete".into()) };
    snapshot.controller = parse_controller_show(controller_text);
    snapshot.available = snapshot.controller.is_some();
    for entry in devices_text.split("\n__DEVICE__").skip(1) {
        let Some((mac, info)) = entry.split_once('\n') else { continue };
        if !valid_bluetooth_address(mac) { continue; }
        let device = parse_device_info(mac, info);
        if device.paired { snapshot.devices.push(device); }
    }
    snapshot.devices.sort_by(|a, b| b.connected.cmp(&a.connected).then_with(|| a.name.cmp(&b.name)));
    Ok(snapshot)
}

fn valid_bluetooth_address(mac: &str) -> bool {
    mac.len() == 17 && mac.split(':').count() == 6 && mac.split(':').all(|part| part.len() == 2 && part.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Parse `bluetoothctl show` output into a BluetoothController struct.
fn parse_controller_show(output: &str) -> Option<BluetoothController> {
    let mut ctrl = BluetoothController::default();
    let mut found = false;
    let mut powered_reported = false;

    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("Controller ") {
            found = true;
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                ctrl.mac = parts[1].to_string();
            }
        } else if let Some(stripped) = line.strip_prefix("Name: ") {
            ctrl.name = stripped.trim().to_string();
        } else if let Some(stripped) = line.strip_prefix("Alias: ") {
            ctrl.alias = stripped.trim().to_string();
        } else if let Some(stripped) = line.strip_prefix("Powered: ") {
            powered_reported = matches!(stripped.trim(), "yes" | "no");
            ctrl.powered = stripped.trim() == "yes";
        } else if let Some(stripped) = line.strip_prefix("Discoverable: ") {
            ctrl.discoverable_reported = matches!(stripped.trim(), "yes" | "no");
            ctrl.discoverable = stripped.trim() == "yes";
        } else if let Some(stripped) = line.strip_prefix("Pairable: ") {
            ctrl.pairable = stripped.trim() == "yes";
        } else if let Some(stripped) = line.strip_prefix("Discovering: ") {
            ctrl.discovering = stripped.trim() == "yes";
        } else if let Some(stripped) = line.strip_prefix("Manufacturer: ") {
            ctrl.manufacturer = stripped.trim().to_string();
        } else if let Some(stripped) = line.strip_prefix("Version: ") {
            ctrl.version = stripped.trim().to_string();
        }
    }

    if found && powered_reported {
        Some(ctrl)
    } else {
        None
    }
}

#[cfg(test)]
mod controller_tests {
    #[test]
    fn controller_must_report_its_power_state() {
        assert!(super::parse_controller_show("No default controller available").is_none());
        assert!(super::parse_controller_show("Controller AA:BB:CC:DD:EE:FF\nName: adapter").is_none());
        let controller = super::parse_controller_show("Controller AA:BB:CC:DD:EE:FF\nPowered: no").unwrap();
        assert!(!controller.powered);
    }
}

/// Query individual device info via `bluetoothctl info <mac>`
pub fn query_device_info(mac: &str) -> BluetoothDevice {
    if let Ok(out) = Command::new("bluetoothctl").args(["info", mac]).output() {
        if out.status.success() {
            return parse_device_info(mac, &String::from_utf8_lossy(&out.stdout));
        }
    }
    parse_device_info(mac, "")
}

fn parse_device_info(mac: &str, text: &str) -> BluetoothDevice {
    let mut dev = BluetoothDevice { mac: mac.to_string(), ..Default::default() };
    for line in text.lines() {
                let line = line.trim();
                if let Some(stripped) = line.strip_prefix("Name: ") {
                    dev.name = stripped.trim().to_string();
                } else if let Some(stripped) = line.strip_prefix("Alias: ") {
                    dev.alias = stripped.trim().to_string();
                } else if let Some(stripped) = line.strip_prefix("Icon: ") {
                    dev.icon = stripped.trim().to_string();
                } else if let Some(stripped) = line.strip_prefix("Paired: ") {
                    dev.paired = stripped.trim() == "yes";
                } else if let Some(stripped) = line.strip_prefix("Connected: ") {
                    dev.connected = stripped.trim() == "yes";
                } else if let Some(stripped) = line.strip_prefix("Trusted: ") {
                    dev.trusted = stripped.trim() == "yes";
                } else if let Some(stripped) = line.strip_prefix("Blocked: ") {
                    dev.blocked = stripped.trim() == "yes";
                } else if let Some(stripped) = line.strip_prefix("Battery Percentage: ") {
                    // Usually format: `0x50 (80)` or `80`
                    if let Some(open) = stripped.find('(') {
                        if let Some(close) = stripped.find(')') {
                            if let Ok(pct) = stripped[open + 1..close].trim().parse::<u8>() {
                                dev.battery_pct = Some(pct);
                            }
                        }
                    } else if let Ok(pct) = stripped.trim().parse::<u8>() {
                        dev.battery_pct = Some(pct);
                    }
                } else if let Some(stripped) = line.strip_prefix("RSSI: ") {
                    if let Ok(val) = stripped.trim().parse::<i16>() {
                        dev.rssi = Some(val);
                    }
                } else if let Some(stripped) = line.strip_prefix("UUID: ") {
                    dev.uuids.push(stripped.trim().to_string());
                }
    }

    if dev.name.is_empty() && !dev.alias.is_empty() {
        dev.name = dev.alias.clone();
    }
    if dev.name.is_empty() {
        dev.name = dev.mac.clone();
    }

    dev.name = clean_device_name(&dev.name);

    dev
}

#[cfg(test)]
mod remote_bluetooth_tests {
    #[test]
    fn parses_remote_status_and_ignores_invalid_addresses() {
        let output = "__CONTROLLER__\nController AA:BB:CC:DD:EE:FF (public)\nPowered: yes\n\n__DEVICES__\n__DEVICE__11:22:33:44:55:66\nDevice 11:22:33:44:55:66 Headphones\nName: Headphones\nPaired: yes\nConnected: yes\n__DEVICE__invalid\nName: ignored\nPaired: yes\n";
        let snapshot = super::parse_remote_bluetooth_output(output).unwrap();
        assert!(snapshot.available);
        assert_eq!(snapshot.get_connected_count(), 1);
        assert_eq!(snapshot.devices[0].name, "Headphones");
    }

    #[test]
    fn parses_remote_windows_paired_devices_and_drops_unpaired_rows() {
        let output = r#"{"available":true,"devices":[{"mac":"11:22:33:44:55:66","name":"Headphones","paired":true,"connected":true},{"mac":"invalid","name":"Bad","paired":true},{"mac":"AA:BB:CC:DD:EE:FF","name":"Unpaired","paired":false,"connected":false}]}"#;
        let snapshot = super::parse_windows_bluetooth_output(output).unwrap();
        assert!(snapshot.available);
        assert_eq!(snapshot.devices.len(), 1);
        assert_eq!(snapshot.get_connected_count(), 1);
        assert_eq!(snapshot.devices[0].name, "Headphones");
    }

    #[test]
    fn remote_windows_bluetooth_command_uses_read_only_native_enumeration() {
        use base64::Engine;
        let command = super::windows_bluetooth_command();
        let encoded = command.strip_prefix("powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand ").unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap();
        let words = bytes.chunks_exact(2).map(|word| u16::from_le_bytes([word[0], word[1]])).collect::<Vec<_>>();
        let script = String::from_utf16(&words).unwrap();
        assert!(script.contains("BluetoothFindFirstDevice"));
        assert!(script.contains("info.IsAuthenticated"));
        assert!(script.contains("info.IsConnected"));
        assert!(script.contains("Inquiry = false"));
        assert!(!script.contains("BluetoothRemoveDevice"));
    }
}

/// Sanitizes device names to avoid unrenderable Unicode tofu squares (e.g. CJK brand prefixes)
pub fn clean_device_name(name: &str) -> String {
    let mut s = name.to_string();
    s = s.replace("联想", "Lenovo ");
    s = s.replace("小米", "Xiaomi ");
    s = s.replace("华为", "Huawei ");

    let cleaned: String = s.chars().map(|c| {
        if c.is_ascii() || ('\u{00A0}'..='\u{024F}').contains(&c) {
            c
        } else {
            ' '
        }
    }).collect();

    let trimmed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        name.to_string()
    } else {
        trimmed
    }
}

fn checked_bluetooth_result(success: bool, stdout: &str, stderr: &str) -> Result<(), String> {
    let combined = format!("{}\n{}", stdout.trim(), stderr.trim());
    let lower = combined.to_ascii_lowercase();
    let failed = ["failed", "not available", "not ready", "no default controller", "org.bluez.error", "not found", "invalid argument"]
        .iter().any(|message| lower.contains(message));
    if success && !failed { Ok(()) } else if combined.trim().is_empty() { Err("Bluetooth command failed".into()) } else { Err(combined.trim().to_string()) }
}

fn run_bluetooth_command(args: &[&str]) -> Result<(), String> {
    let output = Command::new("bluetoothctl").args(["--timeout", "20"]).args(args).env("LC_ALL", "C")
        .output().map_err(|error| format!("Could not start bluetoothctl: {error}"))?;
    checked_bluetooth_result(output.status.success(), &String::from_utf8_lossy(&output.stdout), &String::from_utf8_lossy(&output.stderr))
}

fn read_controller() -> Result<BluetoothController, String> {
    let output = Command::new("bluetoothctl").arg("show").env("LC_ALL", "C").output()
        .map_err(|error| format!("Could not query Bluetooth controller: {error}"))?;
    checked_bluetooth_result(output.status.success(), &String::from_utf8_lossy(&output.stdout), &String::from_utf8_lossy(&output.stderr))?;
    parse_controller_show(&String::from_utf8_lossy(&output.stdout)).ok_or_else(|| "Bluetooth controller state is unavailable".into())
}

/// Set and verify the default Bluetooth controller's power state.
pub fn set_radio_power(on: bool) -> Result<(), String> {
    run_bluetooth_command(&["power", if on { "on" } else { "off" }])?;
    if read_controller()?.powered != on { return Err("Bluetooth radio did not reach the requested state".into()); }
    Ok(())
}

/// Set and verify whether the default controller is discoverable.
pub fn set_discoverable(on: bool) -> Result<(), String> {
    run_bluetooth_command(&["discoverable", if on { "on" } else { "off" }])?;
    let controller = read_controller()?;
    if !controller.discoverable_reported { return Err("Bluetooth discoverability state is unavailable".into()); }
    if controller.discoverable != on { return Err("Discoverability did not reach the requested state".into()); }
    Ok(())
}

fn valid_device_address(mac: &str) -> bool {
    let parts: Vec<_> = mac.split(':').collect();
    parts.len() == 6 && parts.iter().all(|part| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn run_device_command(action: &str, mac: &str) -> Result<(), String> {
    if !valid_device_address(mac) { return Err("Invalid Bluetooth device address".into()); }
    run_bluetooth_command(&[action, mac])
}

fn device_property_state(output: &str, property: &str) -> Option<bool> {
    output.lines().find_map(|line| {
        let (name, value) = line.trim().split_once(':')?;
        if name != property { return None; }
        match value.trim() { "yes" => Some(true), "no" => Some(false), _ => None }
    })
}

fn verify_device_property(mac: &str, property: &str, expected: bool) -> Result<(), String> {
    let output = Command::new("bluetoothctl").args(["--timeout", "20", "info", mac]).env("LC_ALL", "C")
        .output().map_err(|error| format!("Could not query Bluetooth device: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    checked_bluetooth_result(output.status.success(), &stdout, &String::from_utf8_lossy(&output.stderr))?;
    match device_property_state(&stdout, property) {
        Some(value) if value == expected => Ok(()),
        Some(_) => Err(format!("Bluetooth device {property} state did not change")),
        None => Err(format!("Bluetooth device {property} state is unavailable")),
    }
}

/// Connect to a device and check the reported connection state.
pub fn connect_device(mac: &str) -> Result<(), String> {
    run_device_command("connect", mac)?;
    verify_device_property(mac, "Connected", true)
}

/// Disconnect a device and check the reported connection state.
pub fn disconnect_device(mac: &str) -> Result<(), String> {
    run_device_command("disconnect", mac)?;
    verify_device_property(mac, "Connected", false)
}

/// Pair first, then trust the device only if pairing succeeded.
pub fn pair_and_trust(mac: &str) -> Result<(), String> {
    run_device_command("pair", mac)?;
    verify_device_property(mac, "Paired", true)?;
    run_device_command("trust", mac).map_err(|error| format!("Pairing completed, but trust failed: {error}"))?;
    verify_device_property(mac, "Trusted", true).map_err(|error| format!("Pairing completed, but trust could not be verified: {error}"))
}

/// Remove/unpair a device and verify that it is absent from the paired-device list.
pub fn remove_device(mac: &str) -> Result<(), String> {
    run_device_command("remove", mac)?;
    let output = Command::new("bluetoothctl").args(["--timeout", "20", "devices", "Paired"]).env("LC_ALL", "C")
        .output().map_err(|error| format!("Could not query paired devices: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    checked_bluetooth_result(output.status.success(), &stdout, &String::from_utf8_lossy(&output.stderr))?;
    if stdout.lines().any(|line| line.split_whitespace().nth(1).is_some_and(|address| address.eq_ignore_ascii_case(mac))) {
        return Err("Bluetooth device is still paired".into());
    }
    Ok(())
}

#[cfg(test)]
mod action_tests {
    #[test]
    fn reports_command_errors_even_when_the_process_succeeds() {
        assert!(super::checked_bluetooth_result(true, "Failed to pair: org.bluez.Error.AuthenticationFailed", "").is_err());
        assert!(super::checked_bluetooth_result(false, "", "permission denied").is_err());
        assert!(super::checked_bluetooth_result(true, "Pairing successful", "").is_ok());
        assert!(super::valid_device_address("AA:bb:00:11:22:ff"));
        assert!(!super::valid_device_address("--help"));
        assert_eq!(super::device_property_state("Device AA\n Connected: yes\n Trusted: no", "Connected"), Some(true));
        assert_eq!(super::device_property_state("Device AA\n Connected: no", "Connected"), Some(false));
        assert_eq!(super::device_property_state("Device AA", "Connected"), None);
    }
}

/// Asynchronously run bluetoothctl discovery scan for nearby devices (10 seconds)
pub fn trigger_discovery_async(
    shared_snap: Arc<Mutex<BluetoothSnapshot>>,
    scanning_flag: Arc<AtomicBool>,
) {
    if scanning_flag.swap(true, Ordering::SeqCst) {
        return; // Already scanning
    }

    std::thread::spawn(move || {
        // Run scan for 8 seconds
        let result = Command::new("bluetoothctl")
            .args(["--timeout", "8", "scan", "on"]).env("LC_ALL", "C").output()
            .map_err(|error| format!("Could not start Bluetooth scan: {error}"))
            .and_then(|output| checked_bluetooth_result(output.status.success(), &String::from_utf8_lossy(&output.stdout), &String::from_utf8_lossy(&output.stderr)));

        // Refresh snapshot
        let updated = query_bluetooth_status();
        {
            let mut snap = shared_snap.lock().unwrap();
            *snap = updated;
            snap.is_scanning = false;
            snap.status_msg = Some(match result { Ok(()) => ("Device discovery completed".to_string(), false, Instant::now()), Err(error) => (format!("Bluetooth scan failed: {error}"), true, Instant::now()) });
        }

        scanning_flag.store(false, Ordering::SeqCst);
    });
}
