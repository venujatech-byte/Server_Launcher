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

    match show_output {
        Ok(out) if out.status.success() => {
            snap.available = true;
            let text = String::from_utf8_lossy(&out.stdout);
            snap.controller = parse_controller_show(&text);
        }
        _ => {
            // Check fallback via rfkill
            if let Ok(rf_out) = Command::new("rfkill").args(["list", "bluetooth"]).output() {
                if rf_out.status.success() {
                    let rf_text = String::from_utf8_lossy(&rf_out.stdout);
                    let soft_blocked = rf_text.contains("Soft blocked: yes");
                    let hard_blocked = rf_text.contains("Hard blocked: yes");
                    snap.available = true;
                    snap.controller = Some(BluetoothController {
                        mac: "00:00:00:00:00:00".to_string(),
                        name: "Bluetooth Controller".to_string(),
                        alias: "hci0".to_string(),
                        powered: !soft_blocked && !hard_blocked,
                        discoverable: false,
                        pairable: true,
                        discovering: false,
                        manufacturer: "Generic".to_string(),
                        version: "BlueZ".to_string(),
                    });
                }
            }
        }
    }

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
                        dev.name = name;
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
            ctrl.powered = stripped.trim() == "yes";
        } else if let Some(stripped) = line.strip_prefix("Discoverable: ") {
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

    if found {
        Some(ctrl)
    } else {
        None
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

    dev
}

/// Set Bluetooth radio power (ON or OFF)
pub fn set_radio_power(on: bool) -> Result<(), String> {
    let arg = if on { "on" } else { "off" };
    let res = Command::new("bluetoothctl")
        .args(["power", arg])
        .output();

    // Also run rfkill as backup
    let rf_arg = if on { "unblock" } else { "block" };
    let _ = Command::new("rfkill").args([rf_arg, "bluetooth"]).output();

    match res {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => Err(format!("Failed to execute bluetoothctl: {}", e)),
    }
}

/// Set Bluetooth discoverable state
pub fn set_discoverable(on: bool) -> Result<(), String> {
    let arg = if on { "on" } else { "off" };
    let res = Command::new("bluetoothctl")
        .args(["discoverable", arg])
        .output();

    match res {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => Err(format!("Failed to execute bluetoothctl: {}", e)),
    }
}

/// Connect to a device by MAC address
pub fn connect_device(mac: &str) -> Result<(), String> {
    let res = Command::new("bluetoothctl")
        .args(["connect", mac])
        .output();

    match res {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let msg = String::from_utf8_lossy(&out.stdout);
            if msg.contains("Connection successful") {
                Ok(())
            } else {
                Err(format!("{}: {}", String::from_utf8_lossy(&out.stderr), msg))
            }
        }
        Err(e) => Err(format!("Failed to connect: {}", e)),
    }
}

/// Disconnect a device by MAC address
pub fn disconnect_device(mac: &str) -> Result<(), String> {
    let res = Command::new("bluetoothctl")
        .args(["disconnect", mac])
        .output();

    match res {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => Err(format!("Failed to disconnect: {}", e)),
    }
}

/// Pair and trust a device by MAC address
pub fn pair_and_trust(mac: &str) -> Result<(), String> {
    let _ = Command::new("bluetoothctl").args(["pair", mac]).output();
    let res = Command::new("bluetoothctl").args(["trust", mac]).output();

    match res {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => Err(format!("Failed to pair: {}", e)),
    }
}

/// Remove/unpair a device by MAC address
pub fn remove_device(mac: &str) -> Result<(), String> {
    let res = Command::new("bluetoothctl")
        .args(["remove", mac])
        .output();

    match res {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => Err(format!("Failed to remove device: {}", e)),
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
        let _ = Command::new("bluetoothctl")
            .args(["--timeout", "8", "scan", "on"])
            .output();

        // Refresh snapshot
        let updated = query_bluetooth_status();
        {
            let mut snap = shared_snap.lock().unwrap();
            *snap = updated;
            snap.is_scanning = false;
            snap.status_msg = Some(("Device discovery completed".to_string(), false, Instant::now()));
        }

        scanning_flag.store(false, Ordering::SeqCst);
    });
}
