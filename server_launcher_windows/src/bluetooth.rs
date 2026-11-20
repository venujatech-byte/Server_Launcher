// ─────────────────────────────────────────────────────────────────────────────
// Bluetooth Subsystem Manager & BlueZ Controller Interface
// ─────────────────────────────────────────────────────────────────────────────

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

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
    let mut dev = BluetoothDevice {
        mac: mac.to_string(),
        ..Default::default()
    };

    if let Ok(out) = Command::new("bluetoothctl").args(["info", mac]).output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
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
