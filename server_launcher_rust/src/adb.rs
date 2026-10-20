//! ADB (Android Debug Bridge) Management Suite & Device Controller.
//!
//! Provides comprehensive enterprise controls for Android devices:
//! - Device discovery (USB & Wireless TCP/IP)
//! - Real-time device telemetry & hardware status
//! - Screen mirroring integration (scrcpy) with fine-tuned video parameters
//! - Bidirectional clipboard sharing & live text injection
//! - Interactive shell terminal access with logcat & system presets
//! - Remote storage explorer, file transfer (push/pull), and APK package installer
//! - Firmware & Recovery toolkits (OTA sideload, fastboot flashing, reboot targets, bugreports)
//! - Hardware input simulation (power, volume, navigation, screenshot capture)

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct AdbDeviceInfo {
    pub serial: String,
    pub state: String, // "device", "offline", "unauthorized", "recovery", "bootloader"
    pub model: String,
    pub product: String,
    pub device: String,
    pub transport_id: String,
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct AdbDeviceTelemetry {
    pub model: String,
    pub manufacturer: String,
    pub brand: String,
    pub android_version: String,
    pub api_level: u32,
    pub security_patch: String,
    pub build_id: String,
    pub battery_pct: Option<f32>,
    pub battery_status: Option<String>,
    pub battery_temp_c: Option<f32>,
    pub cpu_abi: String,
    pub screen_res: String,
    pub screen_dpi: String,
    pub selinux: String,
    pub is_rooted: bool,
    pub storage_internal_free_gb: Option<f32>,
    pub storage_internal_total_gb: Option<f32>,
    pub storage_sdcard_free_gb: Option<f32>,
    pub storage_sdcard_total_gb: Option<f32>,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AdbSensorProcess {
    pub pid: u32,
    pub name: String,
    pub cpu_pct: Option<f32>,
    pub memory: String,
    pub memory_kb: Option<u64>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AdbCpuCounters {
    pub name: String,
    pub total: u64,
    pub idle: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AdbDiskCounters {
    pub name: String,
    pub read_sectors: u64,
    pub written_sectors: u64,
}

/// One row of `dumpsys batterystats` power attribution. `mah` is charge drained
/// since the last full charge, which is the unit the platform reports in.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AdbPowerAttribution {
    pub label: String,
    pub mah: f32,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AdbNetworkInterface {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AdbSimSlot {
    pub slot: usize,
    pub operator: String,
    pub state: String,
    pub network_type: String,
    pub numeric: String,
    pub country: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AdbTemperatureSensor {
    pub name: String,
    pub temp_c: f32,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AdbDeviceSensors {
    pub cpu_model: String,
    pub cpu_cores: usize,
    pub cpu_usage_pct: Option<f32>,
    pub cpu_counters: Vec<AdbCpuCounters>,
    pub cpu_frequencies_mhz: Vec<u64>,
    pub top_processes: Vec<AdbSensorProcess>,
    pub mem_total_kb: Option<u64>,
    pub mem_available_kb: Option<u64>,
    pub mem_free_kb: Option<u64>,
    pub mem_buffers_kb: Option<u64>,
    pub mem_cached_kb: Option<u64>,
    pub mem_dirty_kb: Option<u64>,
    pub swap_total_kb: Option<u64>,
    pub swap_free_kb: Option<u64>,
    pub storage_internal_total_kb: Option<u64>,
    pub storage_internal_free_kb: Option<u64>,
    pub storage_shared_total_kb: Option<u64>,
    pub storage_shared_free_kb: Option<u64>,
    pub disk_counters: Vec<AdbDiskCounters>,
    pub gpu_renderer: String,
    pub gpu_platform: String,
    pub battery_voltage_mv: Option<u64>,
    pub battery_health: String,
    pub battery_plugged: String,
    pub battery_pct: Option<f32>,
    pub battery_status: String,
    pub battery_temp_c: Option<f32>,
    pub battery_current_ua: Option<i64>,
    pub battery_charge_counter_uah: Option<u64>,
    pub battery_charge_full_uah: Option<u64>,
    pub battery_charge_full_design_uah: Option<u64>,
    pub battery_cycle_count: Option<u64>,
    pub battery_technology: Option<String>,
    pub cpu_governor: Option<String>,
    pub cpu_available_governors: Vec<String>,
    /// Platform power attribution; empty when batterystats has no history yet.
    pub power_attribution: Vec<AdbPowerAttribution>,
    /// Seconds covered by the batterystats window, used to turn mAh into watts.
    pub power_attribution_secs: Option<f64>,
    pub wifi_enabled: Option<bool>,
    pub bluetooth_enabled: Option<bool>,
    pub bluetooth_name: Option<String>,
    pub bluetooth_address: Option<String>,
    pub bluetooth_state: Option<String>,
    pub bluetooth_scan_mode: Option<String>,
    pub bluetooth_connection_state: Option<String>,
    pub bluetooth_class: Option<String>,
    pub bluetooth_uptime: Option<String>,
    pub bluetooth_max_audio_devices: Option<u32>,
    pub bluetooth_a2dp_offload: Option<bool>,
    pub bluetooth_active_device: Option<String>,
    pub bluetooth_profiles: Vec<String>,
    pub bluetooth_bonded_devices: Vec<String>,
    pub network_interfaces: Vec<AdbNetworkInterface>,
    pub temperatures: Vec<AdbTemperatureSensor>,
    pub sensor_names: Vec<String>,
    pub uptime_secs: Option<u64>,
    pub socket_total: usize,
    pub socket_tcp: usize,
    pub socket_udp: usize,
    pub cellular_operator: String,
    pub sim_operator: String,
    pub sim_state: String,
    pub sim_slots: Vec<AdbSimSlot>,
    pub cellular_network_type: String,
    pub cellular_numeric: String,
    pub cellular_country: String,
    pub baseband_version: String,
    pub ril_impl: String,
    pub mobile_data_enabled: Option<bool>,
    pub data_roaming_enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbFileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub permissions: String,
    pub modified: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbPackageInfo {
    pub name: String,
    pub is_system: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbNotification {
    pub id: String,
    pub package: String,
    pub app_name: String,
    pub title: String,
    pub text: String,
    pub subtext: String,
    pub time_str: String,
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdbCallType {
    Incoming,
    Outgoing,
    Missed,
}

impl AdbCallType {
    pub fn label(&self) -> &'static str {
        match self {
            AdbCallType::Incoming => "Incoming",
            AdbCallType::Outgoing => "Outgoing",
            AdbCallType::Missed => "Missed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbCallLogEntry {
    pub id: String,
    pub number: String,
    pub name: String,
    pub call_type: AdbCallType,
    pub time_str: String,
    pub duration_str: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbSmsMessage {
    pub id: String,
    pub address: String,
    pub contact_name: String,
    pub text: String,
    pub time_str: String,
    pub is_outgoing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbPhotoItem {
    pub name: String,
    pub path: String,
    pub size_str: String,
    pub date_str: String,
}

/// Executes an adb command with optional device serial target.
pub fn adb_cmd(serial: Option<&str>, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("adb");
    if let Some(s) = serial {
        if !s.is_empty() {
            cmd.arg("-s").arg(s);
        }
    }
    cmd.args(args);
    cmd.stdin(Stdio::null());

    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if out.status.success() {
                Ok(stdout.trim().to_string())
            } else {
                let err = if !stderr.trim().is_empty() {
                    stderr.trim().to_string()
                } else if !stdout.trim().is_empty() {
                    stdout.trim().to_string()
                } else {
                    format!("Command exited with code {}", out.status.code().unwrap_or(-1))
                };
                Err(err)
            }
        }
        Err(e) => Err(format!("Failed to execute adb: {}", e)),
    }
}

fn parse_android_df_line(line: &str) -> Option<(String, u64, u64)> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 6 { return None; }
    Some((
        fields.last()?.to_string(),
        fields.get(1)?.parse().ok()?,
        fields.get(3)?.parse().ok()?,
    ))
}

fn parse_sim_slots(operator: &str, state: &str, network_type: &str, numeric: &str, country: &str) -> Vec<AdbSimSlot> {
    let values = [operator, state, network_type, numeric, country].map(|value| {
        value.split(',').map(str::trim).map(str::to_string).collect::<Vec<_>>()
    });
    let count = if values.iter().any(|items| items.iter().any(|item| !item.is_empty())) {
        values.iter().map(Vec::len).max().unwrap_or(0).min(2)
    } else { 0 };
    (0..count).map(|index| AdbSimSlot {
        slot: index + 1,
        operator: values[0].get(index).cloned().unwrap_or_default(),
        state: values[1].get(index).cloned().unwrap_or_default(),
        network_type: values[2].get(index).cloned().unwrap_or_default(),
        numeric: values[3].get(index).cloned().unwrap_or_default(),
        country: values[4].get(index).cloned().unwrap_or_default(),
    }).collect()
}

/// Parses output from `adb devices -l` and returns all attached devices.
pub fn list_devices() -> Vec<AdbDeviceInfo> {
    let mut devices = Vec::new();
    let output = match adb_cmd(None, &["devices", "-l"]) {
        Ok(s) => s,
        Err(_) => return devices,
    };

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("List of devices") || line.starts_with("* daemon") {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        let serial = parts[0].to_string();
        let state = parts[1].to_string();

        let mut product = String::new();
        let mut model = String::new();
        let mut device = String::new();
        let mut transport_id = String::new();

        for p in &parts[2..] {
            if let Some(val) = p.strip_prefix("product:") {
                product = val.to_string();
            } else if let Some(val) = p.strip_prefix("model:") {
                model = val.replace('_', " ");
            } else if let Some(val) = p.strip_prefix("device:") {
                device = val.to_string();
            } else if let Some(val) = p.strip_prefix("transport_id:") {
                transport_id = val.to_string();
            }
        }

        if model.is_empty() {
            model = if serial.contains(':') {
                format!("Wireless Device ({})", serial)
            } else {
                format!("USB Device ({})", serial)
            };
        }

        devices.push(AdbDeviceInfo {
            serial,
            state,
            model,
            product,
            device,
            transport_id,
        });
    }

    devices
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbNetworkEndpoint {
    pub interface: String,
    pub ip: String,
    pub is_wifi: bool,
    pub label: String,
}

/// Discovers valid, routable network endpoints on an Android device (filtering dummy interfaces like ds-br, p2p, dummy).
pub fn discover_device_endpoints(serial: &str) -> Vec<AdbNetworkEndpoint> {
    let mut endpoints = Vec::new();
    let out = match adb_cmd(Some(serial), &["shell", "ip -f inet addr"]) {
        Ok(s) => s,
        Err(_) => return endpoints,
    };

    let mut current_iface = String::new();
    for line in out.lines() {
        let trimmed = line.trim();
        if !line.starts_with(' ') && !line.starts_with('\t') && line.contains(':') {
            if let Some(iface) = line.split(':').nth(1) {
                current_iface = iface.trim().to_string();
            }
        } else if trimmed.starts_with("inet ") {
            if let Some(cidr) = trimmed.split_whitespace().nth(1) {
                let ip = cidr.split('/').next().unwrap_or("").to_string();
                if ip.is_empty() || ip == "127.0.0.1" {
                    continue;
                }
                let iface_lower = current_iface.to_lowercase();
                if iface_lower.starts_with("dummy")
                    || iface_lower.starts_with("ds-br")
                    || iface_lower.starts_with("p2p")
                    || iface_lower.starts_with("lo")
                {
                    continue;
                }

                let is_wifi = iface_lower.starts_with("wlan") || iface_lower.starts_with("wifi");
                let label = if is_wifi {
                    format!("Wi-Fi ({})", current_iface)
                } else if iface_lower.starts_with("eth") {
                    format!("Ethernet ({})", current_iface)
                } else {
                    current_iface.clone()
                };

                if !endpoints.iter().any(|e: &AdbNetworkEndpoint| e.ip == ip) {
                    endpoints.push(AdbNetworkEndpoint {
                        interface: current_iface.clone(),
                        ip,
                        is_wifi,
                        label,
                    });
                }
            }
        }
    }

    endpoints.sort_by(|a, b| b.is_wifi.cmp(&a.is_wifi));
    endpoints
}

/// Connects to a wireless ADB target (IP:Port) with robust output verification.
pub fn connect_wireless(target: &str) -> Result<String, String> {
    let target = target.trim();
    let formatted = if !target.contains(':') {
        format!("{}:5555", target)
    } else {
        target.to_string()
    };
    match adb_cmd(None, &["connect", &formatted]) {
        Ok(out) => {
            let lower = out.to_lowercase();
            if lower.contains("failed")
                || lower.contains("cannot connect")
                || lower.contains("unable to connect")
                || lower.contains("connection refused")
            {
                Err(out)
            } else {
                Ok(out)
            }
        }
        Err(e) => Err(e),
    }
}

/// Disconnects a wireless ADB target (IP:Port).
pub fn disconnect_wireless(target: &str) -> Result<String, String> {
    adb_cmd(None, &["disconnect", target])
}

/// Restarts ADB daemon on target device listening on specified TCP port (typically 5555).
pub fn enable_tcpip(serial: &str, port: u16) -> Result<String, String> {
    let port_str = port.to_string();
    adb_cmd(Some(serial), &["tcpip", &port_str])
}

/// Reverts target device from TCP/IP mode back to USB debugging mode.
pub fn reset_to_usb(serial: &str) -> Result<String, String> {
    adb_cmd(Some(serial), &["usb"])
}

/// Pairs with an Android 11+ wireless debugging target using pairing code.
pub fn pair_wireless(target: &str, pairing_code: &str) -> Result<String, String> {
    adb_cmd(None, &["pair", target, pairing_code])
}

/// Queries comprehensive telemetry from an online Android device.
pub fn query_telemetry(serial: &str) -> AdbDeviceTelemetry {
    let mut telem = AdbDeviceTelemetry::default();

    // 1. Query Android build properties via getprop
    if let Ok(props_raw) = adb_cmd(Some(serial), &["shell", "getprop"]) {
        for line in props_raw.lines() {
            if let Some((k, v)) = parse_getprop_line(line) {
                match k {
                    "ro.product.model" => telem.model = v.to_string(),
                    "ro.product.manufacturer" => telem.manufacturer = v.to_string(),
                    "ro.product.brand" => telem.brand = v.to_string(),
                    "ro.build.version.release" => telem.android_version = v.to_string(),
                    "ro.build.version.sdk" => telem.api_level = v.parse::<u32>().unwrap_or(0),
                    "ro.build.version.security_patch" => telem.security_patch = v.to_string(),
                    "ro.build.display.id" => telem.build_id = v.to_string(),
                    "ro.product.cpu.abi" => telem.cpu_abi = v.to_string(),
                    _ => {}
                }
            }
        }
    }

    // 2. Query Battery telemetry via dumpsys battery
    if let Ok(bat_raw) = adb_cmd(Some(serial), &["shell", "dumpsys", "battery"]) {
        for line in bat_raw.lines() {
            let line = line.trim();
            if let Some(val) = line.strip_prefix("level:") {
                telem.battery_pct = val.trim().parse::<f32>().ok();
            } else if let Some(val) = line.strip_prefix("temperature:") {
                if let Ok(t) = val.trim().parse::<f32>() {
                    let temp = if t.abs() > 150.0 { t / 10.0 } else { t };
                    telem.battery_temp_c = Some((temp * 10.0).round() / 10.0);
                }
            } else if let Some(val) = line.strip_prefix("status:") {
                telem.battery_status = match val.trim() {
                    "2" => Some("Charging (AC/USB)".to_string()),
                    "3" => Some("Discharging".to_string()),
                    "4" => Some("Not Charging".to_string()),
                    "5" => Some("Full (100%)".to_string()),
                    _ => Some("Unknown".to_string()),
                };
            }
        }
    }

    // 3. Screen display resolution & density
    if let Ok(wm_size) = adb_cmd(Some(serial), &["shell", "wm", "size"]) {
        if let Some(res) = wm_size.split(':').nth(1) {
            telem.screen_res = res.trim().to_string();
        }
    }
    if let Ok(wm_density) = adb_cmd(Some(serial), &["shell", "wm", "density"]) {
        if let Some(dpi) = wm_density.split(':').nth(1) {
            telem.screen_dpi = format!("{} DPI", dpi.trim());
        }
    }

    // 4. SELinux & Root status
    if let Ok(selinux) = adb_cmd(Some(serial), &["shell", "getenforce"]) {
        telem.selinux = selinux.trim().to_string();
    }
    telem.is_rooted = adb_cmd(Some(serial), &["shell", "su", "-c", "id"]).map(|s| s.contains("uid=0")).unwrap_or(false);

    // Query each mount independently; Samsung df may fail the whole multi-path request.
    for path in ["/data", "/storage/emulated/0", "/sdcard"] {
        if let Ok(df_raw) = adb_cmd(Some(serial), &["shell", "df", "-k", path]) {
            for line in df_raw.lines() {
                let Some((mount, total_kb, free_kb)) = parse_android_df_line(line) else { continue; };
                if mount.starts_with("/data") && telem.storage_internal_total_gb.is_none() {
                    telem.storage_internal_total_gb = Some((total_kb as f32 / (1024.0 * 1024.0) * 10.0).round() / 10.0);
                    telem.storage_internal_free_gb = Some((free_kb as f32 / (1024.0 * 1024.0) * 10.0).round() / 10.0);
                } else if mount.starts_with("/storage/emulated") && telem.storage_sdcard_total_gb.is_none() {
                    telem.storage_sdcard_total_gb = Some((total_kb as f32 / (1024.0 * 1024.0) * 10.0).round() / 10.0);
                    telem.storage_sdcard_free_gb = Some((free_kb as f32 / (1024.0 * 1024.0) * 10.0).round() / 10.0);
                }
            }
        }
    }
    if telem.storage_sdcard_total_gb.is_none() && telem.storage_internal_total_gb.is_some() {
        telem.storage_sdcard_total_gb = telem.storage_internal_total_gb;
        telem.storage_sdcard_free_gb = telem.storage_internal_free_gb;
    }

    // 6. System Uptime
    if let Ok(up_raw) = adb_cmd(Some(serial), &["shell", "cat", "/proc/uptime"]) {
        if let Some(first) = up_raw.split_whitespace().next() {
            if let Ok(secs) = first.parse::<f64>() {
                telem.uptime_secs = secs as u64;
            }
        }
    }

    telem
}

// Prefer global component totals; older Android versions report CPU/GPS only inside UID rows.
fn parse_power_attribution(text: &str) -> Vec<AdbPowerAttribution> {
    fn label(name: &str) -> &str {
        match name {
            "cpu" | "Cpu" => "CPU",
            "wifi" | "Wifi" => "Wi-Fi",
            "radio" | "mobile_radio" => "Cell standby",
            "gps" | "gnss" | "Gps" => "GPS",
            "sensor" | "sensors" => "Sensors",
            "bluetooth" => "Bluetooth",
            "camera" => "Camera",
            "flashlight" => "Flashlight",
            "audio" => "Audio",
            "video" => "Video",
            "screen" => "Screen",
            "wakelock" => "Wake locks",
            "phone" => "Phone",
            other => other,
        }
    }
    let mut totals = std::collections::BTreeMap::<String, f32>::new();
    let mut uid_totals = std::collections::BTreeMap::<String, f32>::new();
    let modern = text.lines().any(|line| line.trim() == "Global");
    let has_battery_states = text.contains("(on battery,");
    let mut on_battery = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if modern {
            if trimmed.starts_with("UID ") { break; }
            if trimmed.starts_with('(') {
                on_battery = trimmed.starts_with("(on battery,");
                continue;
            }
            // State subtotals partition the global total; never add both or include charger use.
            if has_battery_states && !on_battery { continue; }
            if !has_battery_states && line.len() - line.trim_start().len() > 4 { continue; }
        }
        let Some((name, rest)) = line.trim().split_once(':') else { continue };
        if name == "Capacity" { continue; }
        let Some(mah) = rest.split_whitespace().next().and_then(|v| v.parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0) else { continue };
        if name.starts_with("Uid ") {
            let mut accounted = 0.0;
            // Ignore the later "Including smearing" block: it redistributes existing power.
            if let Some((_, components)) = rest.split_once('(') {
                for component in components.split(')').next().unwrap_or("").split_whitespace() {
                    let Some((key, value)) = component.split_once('=') else { continue };
                    let Some(value) = value.parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0) else { continue };
                    if key == "proportional" { continue; }
                    *uid_totals.entry(label(key).to_string()).or_default() += value;
                    accounted += value;
                }
            }
            if mah - accounted > 0.00001 {
                *uid_totals.entry("Other apps / services".to_string()).or_default() += mah - accounted;
            }
        } else {
            *totals.entry(label(name).to_string()).or_default() += mah;
        }
    }
    for (name, mah) in uid_totals {
        totals.entry(name).or_insert(mah);
    }
    totals.into_iter().map(|(label, mah)| AdbPowerAttribution { label, mah }).collect()
}

// Android durations may contain days and milliseconds, with or without spaces.
fn parse_battery_time(line: &str) -> Option<f64> {
    let (_, rest) = line.split_once("Time on battery:")?;
    let (duration, _) = rest.split_once('(')?;
    let mut rest = duration.trim();
    let mut seconds = 0.0;
    while !rest.is_empty() {
        let end = rest.find(|c: char| !c.is_ascii_digit())?;
        let value = rest[..end].parse::<u64>().ok()? as f64;
        rest = &rest[end..];
        let (unit, scale) = [("ms", 0.001), ("d", 86400.0), ("h", 3600.0), ("m", 60.0), ("s", 1.0)]
            .into_iter()
            .find(|(unit, _)| rest.starts_with(unit))?;
        seconds += value * scale;
        rest = rest[unit.len()..].trim_start();
    }
    (seconds.is_finite() && seconds > 0.0).then_some(seconds)
}

#[cfg(test)]
mod power_time_tests {
    use super::{parse_android_df_line, parse_battery_time, parse_power_attribution, parse_sensor_inventory, parse_sim_slots};

    #[test]
    fn parses_sim_slots_from_android_property_lists() {
        let sims = parse_sim_slots("Hutch,Dialog", "READY,ABSENT", "LTE,Unknown", "41308,41302", "lk,lk");
        assert_eq!(sims.len(), 2);
        assert_eq!(sims[0].operator, "Hutch");
        assert_eq!(sims[1].state, "ABSENT");
        assert_eq!(sims[1].numeric, "41302");
    }

    #[test]
    fn reads_sensor_names_and_fingerprint_feature() {
        let sensors = parse_sensor_inventory("0x0001 (handle=1) BMI270 Accelerometer | vendor\nfeature:android.hardware.fingerprint\n__FEATURES__");
        assert_eq!(sensors, ["BMI270 Accelerometer", "Fingerprint authentication"]);
    }

    #[test]
    fn parses_android_df_rows_with_emulated_mount_aliases() {
        assert_eq!(
            parse_android_df_line("/dev/block/dm-61 237828076 201474160 36222844 85% /data/user/0"),
            Some(("/data/user/0".into(), 237828076, 36222844))
        );
        assert_eq!(
            parse_android_df_line("/dev/fuse 234000000 190000000 44000000 82% /storage/emulated"),
            Some(("/storage/emulated".into(), 234000000, 44000000))
        );
        assert_eq!(parse_android_df_line("Filesystem 1K-blocks Used Available Use% Mounted on"), None);
    }

    #[test]
    fn includes_uid_components_without_smearing_or_double_counting() {
        let rows = parse_power_attribution("Capacity: 5000\nUid 1000: 3 ( cpu=2 gps=1 ) Including smearing: 8 ( proportional=5 )\nCPU: 4\nWi-Fi: 0.2\nScreen: 1\nUid 1001: 0.5\nGPU: NaN");
        let values: std::collections::BTreeMap<_, _> = rows.into_iter().map(|r| (r.label, r.mah)).collect();
        assert_eq!(values["CPU"], 4.0);
        assert_eq!(values["GPS"], 1.0);
        assert_eq!(values["Wi-Fi"], 0.2);
        assert_eq!(values["Other apps / services"], 0.5);
        assert!(!values.contains_key("GPU"));
        assert!(!values.contains_key("Capacity"));
        let cpu = parse_power_attribution("Uid 0: 1 ( cpu=1 )\nUid u0a10: 2 ( cpu=2 )");
        assert_eq!(cpu[0].label, "CPU");
        assert_eq!(cpu[0].mah, 3.0);
    }

    #[test]
    fn parses_modern_on_battery_states_without_global_or_uid_duplicates() {
        let rows = parse_power_attribution("    Global\n    cpu: 100 apps: 90\n      (on battery, screen on)\n      cpu: 30 apps: 25\n      (on battery, screen off/doze)\n      cpu: 20 apps: 20\n      (not on battery, screen on)\n      cpu: 50 apps: 45\n  UID 1000: 90\n      cpu=90");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "CPU");
        assert_eq!(rows[0].mah, 50.0);
    }

    #[test]
    fn parses_battery_realtime_only() {
        assert_eq!(parse_battery_time("Time on battery: 3s 872ms (0.1%) realtime, 1s (100%) uptime"), Some(3.872));
        assert_eq!(parse_battery_time("Time on battery: 1d2h3m4s5ms (10%) realtime"), Some(93784.005));
        for value in ["", "Time on battery: 0ms (0%) realtime", "Time on battery: unknown (0%)", "Time on battery: 1x (0%)"] {
            assert_eq!(parse_battery_time(value), None);
        }
    }
}

/// Reads Android hardware and resource sensor data in one device shell session.
pub fn query_device_sensors(serial: &str) -> Result<AdbDeviceSensors, String> {
    let script = r#"
echo __CPUINFO__; cat /proc/cpuinfo
echo __CPU_STAT__; cat /proc/stat
echo __CPU_FREQ__; for freq in /sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_cur_freq; do [ -r "$freq" ] && cat "$freq"; done
echo __MEMINFO__; cat /proc/meminfo
echo __TOP__; top -b -n 1 -m 120
echo __BATTERY__; dumpsys battery; cat /sys/class/power_supply/battery/uevent 2>/dev/null; su -c cat /sys/class/power_supply/battery/uevent 2>/dev/null
battery_stats=$(dumpsys batterystats --charged 2>/dev/null)
echo __BATTERYSTATS__; printf '%s\n' "$battery_stats" | sed -n '/Estimated power use/,/^$/p'
echo __BATTERYSTATS_TIME__; printf '%s\n' "$battery_stats" | grep -m1 -E 'Time on battery:'
echo __GOVERNOR__; cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null; cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors 2>/dev/null
echo __NETDEV__; cat /proc/net/dev
echo __DISKSTATS__; cat /proc/diskstats
echo __DF__; df -k /data 2>/dev/null; df -k /storage/emulated/0 2>/dev/null; df -k /sdcard 2>/dev/null
echo __THERMAL_ZONES__; for zone in /sys/class/thermal/thermal_zone*; do if [ -r "$zone/temp" ]; then printf '%s ' "$(cat "$zone/type" 2>/dev/null)"; cat "$zone/temp"; fi; done
echo __WIFI__; settings get global wifi_on
echo __BLUETOOTH__; settings get global bluetooth_on; dumpsys bluetooth_manager 2>/dev/null | sed -n '1,/^AdapterState:/p'; dumpsys bluetooth_manager 2>/dev/null | grep -E -m 20 '(Profile: |  mActiveDevice:)'
echo __GPU__; getprop ro.hardware.egl; getprop ro.board.platform
echo __SOCKSTAT__; cat /proc/net/sockstat
echo __UPTIME__; cat /proc/uptime
echo __TELEPHONY__; getprop gsm.operator.alpha; getprop gsm.sim.operator.alpha; getprop gsm.sim.state; getprop gsm.network.type; getprop gsm.operator.numeric; getprop gsm.operator.iso-country; getprop gsm.version.baseband; getprop gsm.version.ril-impl; settings get global mobile_data 2>/dev/null; settings get global data_roaming 2>/dev/null
"#;
    let output = adb_cmd(Some(serial), &["shell", script])?;
    let mut sections = HashMap::new();
    let mut current = "";
    for line in output.lines() {
        if let Some(name) = line.strip_prefix("__").and_then(|line| line.strip_suffix("__")) {
            current = name;
            sections.entry(current).or_insert_with(String::new);
        } else if !current.is_empty() {
            let section: &mut String = sections.get_mut(current).expect("section was inserted");
            section.push_str(line);
            section.push('\n');
        }
    }
    let section = |name: &str| sections.get(name).map(String::as_str).unwrap_or("");

    let mem_value = |name: &str| {
        section("MEMINFO")
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}:")))
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<u64>().ok())
    };
    let cpu_lines: Vec<&str> = section("CPUINFO").lines().collect();
    let cpu_model = ["Hardware", "model name", "Processor"]
        .iter()
        .find_map(|key| {
            cpu_lines.iter().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.trim().eq_ignore_ascii_case(key).then_some(value.trim())
            })
        })
        .unwrap_or("")
        .to_string();
    let cpu_cores = cpu_lines
        .iter()
        .filter(|line| line.trim_start().starts_with("processor"))
        .count();
    let cpu_counters = section("CPU_STAT")
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            if !name.starts_with("cpu") {
                return None;
            }
            let values: Vec<u64> = fields.filter_map(|field| field.parse().ok()).collect();
            let idle = *values.get(3)? + values.get(4).copied().unwrap_or(0);
            Some(AdbCpuCounters {
                name: name.to_string(),
                total: values.iter().sum(),
                idle,
            })
        })
        .collect();
    let cpu_frequencies_mhz = section("CPU_FREQ")
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .map(|khz| khz / 1000)
        .collect();

    let top_processes = section("TOP")
        .lines()
        .filter_map(|line| {
            let columns: Vec<&str> = line.split_whitespace().collect();
            let pid = columns.first()?.parse::<u32>().ok()?;
            if columns.len() < 9 {
                return None;
            }
            let cpu_pct = columns[8]
                .trim_end_matches('%')
                .parse::<f32>()
                .ok()
                .map(|p| (p * 10.0).round() / 10.0);
            Some(AdbSensorProcess {
                pid,
                name: columns.get(11..).unwrap_or(&[]).join(" ").trim().to_string(),
                cpu_pct,
                memory: columns.get(5).copied().unwrap_or("").to_string(),
                memory_kb: columns
                    .get(5)
                    .and_then(|value| parse_top_memory_kb(value)),
            })
        })
        .filter(|process| !process.name.is_empty() && !process.name.starts_with("top ") && !process.name.starts_with("sh "))
        .take(150)
        .collect();
    let cpu_usage_pct = section("TOP")
        .lines()
        .find(|line| line.to_ascii_lowercase().contains("%cpu"))
        .and_then(|line| {
            let columns: Vec<&str> = line.split_whitespace().collect();
            let total = columns.first()?.trim_end_matches("%cpu").parse::<f32>().ok()?;
            let idle = columns.iter().find_map(|part| {
                part.strip_suffix("%idle")?.parse::<f32>().ok()
            })?;
            (total > 0.0).then_some((((total - idle) / total * 100.0).clamp(0.0, 100.0) * 10.0).round() / 10.0)
        });

    let network_interfaces = section("NETDEV")
        .lines()
        .skip(2)
        .filter_map(|line| {
            let (name, counters) = line.split_once(':')?;
            let counters: Vec<&str> = counters.split_whitespace().collect();
            Some(AdbNetworkInterface {
                name: name.trim().to_string(),
                rx_bytes: counters.first()?.parse().ok()?,
                tx_bytes: counters.get(8)?.parse().ok()?,
            })
        })
        .collect();

    let disk_counters = section("DISKSTATS")
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let name = *fields.get(2)?;
            Some(AdbDiskCounters {
                name: name.to_string(),
                read_sectors: fields.get(5)?.parse().ok()?,
                written_sectors: fields.get(9)?.parse().ok()?,
            })
        })
        .collect();
    let mut storage_internal = None;
    let mut storage_shared = None;
    for line in section("DF").lines() {
        let Some((mount, total, free)) = parse_android_df_line(line) else { continue; };
        if mount.starts_with("/data") && storage_internal.is_none() {
            storage_internal = Some((total, free));
        } else if mount.starts_with("/storage/emulated") && storage_shared.is_none() {
            storage_shared = Some((total, free));
        }
    }

    let temperatures = section("THERMAL_ZONES").lines().filter_map(|line| {
        let (name, value) = line.rsplit_once(char::is_whitespace)?;
        let mut temp_c = value.trim().parse::<f32>().ok()?;
        if temp_c.abs() > 200.0 {
            temp_c /= 1000.0;
        }
        temp_c = (temp_c * 10.0).round() / 10.0;
        Some(AdbTemperatureSensor {
            name: if name.trim().is_empty() { "Thermal zone" } else { name.trim() }.to_string(),
            temp_c,
        })
    }).collect();

    let parse_toggle = |name: &str| {
        let raw = section(name).trim();
        match raw {
            "1" => Some(true),
            "0" => Some(false),
            _ if raw.lines().any(|line| line.trim() == "enabled: true" || line.trim() == "state: ON") => Some(true),
            _ if raw.lines().any(|line| line.trim() == "enabled: false" || line.trim() == "state: OFF") => Some(false),
            _ => None,
        }
    };
    let battery_uevent_val = |key: &str| {
        section("BATTERY")
            .lines()
            .find_map(|line| line.trim().strip_prefix(&format!("POWER_SUPPLY_{key}=")))
            .map(str::trim)
    };
    let battery_value = |prefix: &str| {
        section("BATTERY")
            .lines()
            .find_map(|line| line.trim().strip_prefix(prefix))
            .map(str::trim)
    };
    let gpu: Vec<&str> = section("GPU").lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    let battery_plugged = ["AC", "USB", "Wireless"]
        .iter()
        .find(|source| battery_value(&format!("{source} powered:")) == Some("true"))
        .map(|source| source.to_string())
        .unwrap_or_else(|| "Not charging".to_string());
    let uptime_secs = section("UPTIME")
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .map(|seconds| seconds as u64);
    let battery_status = battery_value("status:")
        .and_then(|status| match status {
            "2" => Some("Charging"),
            "3" => Some("Discharging"),
            "4" => Some("Not charging"),
            "5" => Some("Full"),
            _ => None,
        })
        .or_else(|| battery_uevent_val("STATUS").map(|s| match s {
            "Charging" => "Charging",
            "Discharging" => "Discharging",
            "Not charging" => "Not charging",
            "Full" => "Full",
            other => other,
        }))
        .unwrap_or("Unavailable")
        .to_string();

    let battery_current_ua = battery_uevent_val("CURRENT_NOW")
        .and_then(|val| val.parse::<i64>().ok())
        .or_else(|| {
            battery_value("current now:")
                .and_then(|val| val.parse::<i64>().ok())
        });
    let battery_charge_counter_uah = battery_uevent_val("CHARGE_COUNTER")
        .and_then(|val| val.parse::<u64>().ok())
        .or_else(|| {
            battery_value("Charge counter:")
                .or_else(|| battery_value("charge counter:"))
                .and_then(|val| val.parse::<u64>().ok())
        });
    let battery_charge_full_uah = battery_uevent_val("CHARGE_FULL")
        .and_then(|val| val.parse::<u64>().ok());
    let battery_charge_full_design_uah = battery_uevent_val("CHARGE_FULL_DESIGN")
        .and_then(|val| val.parse::<u64>().ok());
    let battery_cycle_count = battery_uevent_val("BATTERY_CYCLE")
        .and_then(|val| val.parse::<u64>().ok())
        .or_else(|| {
            battery_value("mSavedBatteryUsage:")
                .and_then(|val| val.parse::<u64>().ok())
                .map(|usage| usage / 100)
        })
        .or_else(|| battery_uevent_val("CYCLE_COUNT").and_then(|val| val.parse::<u64>().ok()));
    let battery_technology = battery_uevent_val("TECHNOLOGY")
        .map(str::to_string)
        .or_else(|| battery_value("technology:").map(str::to_string))
        .or_else(|| Some("Li-ion".to_string()));

    let power_attribution = parse_power_attribution(section("BATTERYSTATS"));

    let power_attribution_secs = parse_battery_time(section("BATTERYSTATS_TIME"));

    let governor_lines: Vec<&str> = section("GOVERNOR").lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let cpu_governor = governor_lines.first().map(|s| s.to_string());
    let cpu_available_governors = governor_lines
        .get(1)
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();

    let battery_health_raw = battery_uevent_val("HEALTH")
        .or_else(|| battery_value("health:"))
        .unwrap_or("");
    let battery_health = match battery_health_raw {
        "2" | "Good" | "good" => "Good",
        "3" | "Overheat" | "overheat" => "Overheat",
        "4" | "Dead" | "dead" => "Dead",
        "5" | "Over voltage" | "over_voltage" => "Over Voltage",
        "6" | "Unspecified failure" => "Failure",
        "7" | "Cold" | "cold" => "Cold",
        other if !other.is_empty() => other,
        _ => "Good",
    }.to_string();

    let mut socket_total = 0;
    let mut socket_tcp = 0;
    let mut socket_udp = 0;
    for line in section("SOCKSTAT").lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if line.starts_with("sockets:") {
            if parts.len() >= 3 {
                socket_total = parts[2].parse().unwrap_or(0);
            }
        } else if line.starts_with("TCP:") {
            for (i, &p) in parts.iter().enumerate() {
                if p == "inuse" || p == "alloc" {
                    if let Some(&val) = parts.get(i + 1) {
                        socket_tcp = val.parse().unwrap_or(socket_tcp);
                    }
                }
            }
        } else if line.starts_with("UDP:") {
            for (i, &p) in parts.iter().enumerate() {
                if p == "inuse" {
                    if let Some(&val) = parts.get(i + 1) {
                        socket_udp = val.parse().unwrap_or(socket_udp);
                    }
                }
            }
        }
    }

    let bt_section = section("BLUETOOTH");
    let mut bt_name = None;
    let mut bt_address = None;
    let mut bt_state = None;
    let mut bt_scan_mode = None;
    let mut bt_conn_state = None;
    let mut bt_class = None;
    let mut bt_uptime = None;
    let mut bt_max_audio = None;
    let mut bt_a2dp_offload = None;
    let mut bt_active_dev = None;
    let mut bt_profiles = Vec::new();
    let mut bt_bonded = Vec::new();
    let mut in_bonded = false;

    for line in bt_section.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if in_bonded { in_bonded = false; }
            continue;
        }

        if trimmed.starts_with("mSnoopLog") || trimmed.starts_with("AdapterState:") {
            in_bonded = false;
        }

        if in_bonded {
            if trimmed.contains(':') && trimmed.len() >= 17 {
                bt_bonded.push(trimmed.to_string());
            }
        }

        if trimmed == "Bonded devices:" {
            in_bonded = true;
            continue;
        }

        if let Some(val) = trimmed.strip_prefix("name: ").or_else(|| trimmed.strip_prefix("Name: ")) {
            if bt_name.is_none() && !val.is_empty() && val != "null" {
                bt_name = Some(val.to_string());
            }
        } else if let Some(val) = trimmed.strip_prefix("address: ").or_else(|| trimmed.strip_prefix("Address: ")) {
            if bt_address.is_none() && !val.is_empty() && val != "null" {
                bt_address = Some(val.to_string());
            }
        } else if let Some(val) = trimmed.strip_prefix("state: ").or_else(|| trimmed.strip_prefix("State: ")) {
            if bt_state.is_none() && !val.is_empty() {
                bt_state = Some(val.to_string());
            }
        } else if let Some(val) = trimmed.strip_prefix("ScanMode: ") {
            bt_scan_mode = Some(val.to_string());
        } else if let Some(val) = trimmed.strip_prefix("ConnectionState: ") {
            bt_conn_state = Some(val.to_string());
        } else if let Some(val) = trimmed.strip_prefix("BluetoothClass: ") {
            bt_class = Some(val.to_string());
        } else if let Some(val) = trimmed.strip_prefix("time since enabled: ") {
            bt_uptime = Some(val.to_string());
        } else if let Some(val) = trimmed.strip_prefix("MaxConnectedAudioDevices: ") {
            bt_max_audio = val.parse::<u32>().ok();
        } else if let Some(val) = trimmed.strip_prefix("A2dpOffloadEnabled: ") {
            bt_a2dp_offload = match val {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
        } else if let Some(val) = trimmed.strip_prefix("mActiveDevice: ") {
            if val != "null" && !val.is_empty() && bt_active_dev.is_none() {
                bt_active_dev = Some(val.to_string());
            }
        } else if let Some(val) = trimmed.strip_prefix("Profile: ") {
            let prof = val.trim_end_matches(':').trim();
            if !prof.is_empty() && !bt_profiles.contains(&prof.to_string()) {
                bt_profiles.push(prof.to_string());
            }
        }
    }

    Ok(AdbDeviceSensors {
        cpu_model,
        cpu_cores,
        cpu_usage_pct,
        cpu_counters,
        cpu_frequencies_mhz,
        top_processes,
        mem_total_kb: mem_value("MemTotal"),
        mem_available_kb: mem_value("MemAvailable"),
        mem_free_kb: mem_value("MemFree"),
        mem_buffers_kb: mem_value("Buffers"),
        mem_cached_kb: mem_value("Cached"),
        mem_dirty_kb: mem_value("Dirty"),
        swap_total_kb: mem_value("SwapTotal"),
        swap_free_kb: mem_value("SwapFree"),
        storage_internal_total_kb: storage_internal.map(|(total, _)| total),
        storage_internal_free_kb: storage_internal.map(|(_, free)| free),
        storage_shared_total_kb: storage_shared.map(|(total, _)| total),
        storage_shared_free_kb: storage_shared.map(|(_, free)| free),
        disk_counters,
        gpu_renderer: gpu.first().copied().unwrap_or("").to_string(),
        gpu_platform: gpu.get(1).copied().unwrap_or("").to_string(),
        battery_voltage_mv: battery_value("voltage:")
            .and_then(|value| value.parse().ok())
            .or_else(|| battery_uevent_val("VOLTAGE_NOW").and_then(|v| v.parse::<u64>().ok()).map(|uv| uv / 1000)),
        battery_health,
        battery_plugged,
        battery_pct: battery_value("level:")
            .and_then(|value| value.parse().ok())
            .or_else(|| battery_uevent_val("CAPACITY").and_then(|v| v.parse().ok())),
        battery_status,
        battery_temp_c: battery_value("temperature:")
            .and_then(|value| value.parse::<f32>().ok())
            .or_else(|| battery_uevent_val("TEMP").and_then(|v| v.parse::<f32>().ok()))
            .map(|value| {
                let temp = if value.abs() > 150.0 { value / 10.0 } else { value };
                (temp * 10.0).round() / 10.0
            }),
        battery_current_ua,
        battery_charge_counter_uah,
        battery_charge_full_uah,
        battery_charge_full_design_uah,
        battery_cycle_count,
        battery_technology,
        cpu_governor,
        cpu_available_governors,
        power_attribution,
        power_attribution_secs,
        wifi_enabled: parse_toggle("WIFI"),
        bluetooth_enabled: parse_toggle("BLUETOOTH"),
        bluetooth_name: bt_name,
        bluetooth_address: bt_address,
        bluetooth_state: bt_state,
        bluetooth_scan_mode: bt_scan_mode,
        bluetooth_connection_state: bt_conn_state,
        bluetooth_class: bt_class,
        bluetooth_uptime: bt_uptime,
        bluetooth_max_audio_devices: bt_max_audio,
        bluetooth_a2dp_offload: bt_a2dp_offload,
        bluetooth_active_device: bt_active_dev,
        bluetooth_profiles: bt_profiles,
        bluetooth_bonded_devices: bt_bonded,
        network_interfaces,
        temperatures,
        sensor_names: Vec::new(),
        uptime_secs,
        socket_total,
        socket_tcp,
        socket_udp,
        cellular_operator: section("TELEPHONY").lines().next().unwrap_or("").trim().to_string(),
        sim_operator: section("TELEPHONY").lines().nth(1).unwrap_or("").trim().to_string(),
        sim_state: section("TELEPHONY").lines().nth(2).unwrap_or("").trim().to_string(),
        cellular_network_type: section("TELEPHONY").lines().nth(3).unwrap_or("").trim().to_string(),
        cellular_numeric: section("TELEPHONY").lines().nth(4).unwrap_or("").trim().to_string(),
        cellular_country: section("TELEPHONY").lines().nth(5).unwrap_or("").trim().to_string(),
        sim_slots: parse_sim_slots(
            section("TELEPHONY").lines().nth(1).unwrap_or(""),
            section("TELEPHONY").lines().nth(2).unwrap_or(""),
            section("TELEPHONY").lines().nth(3).unwrap_or(""),
            section("TELEPHONY").lines().nth(4).unwrap_or(""),
            section("TELEPHONY").lines().nth(5).unwrap_or(""),
        ),
        baseband_version: section("TELEPHONY").lines().nth(6).unwrap_or("").trim().to_string(),
        ril_impl: section("TELEPHONY").lines().nth(7).unwrap_or("").trim().to_string(),
        mobile_data_enabled: section("TELEPHONY").lines().nth(8).and_then(|v| match v.trim() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        }),
        data_roaming_enabled: section("TELEPHONY").lines().nth(9).and_then(|v| match v.trim() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        }),
    })
}

/// Loads the mostly static Android sensor inventory only when its view is opened.
pub fn query_device_sensor_inventory(serial: &str) -> Result<Vec<String>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(sensors) = cache.lock().map_err(|e| e.to_string())?.get(serial).cloned() {
        return Ok(sensors);
    }

    let output = adb_cmd(
        Some(serial),
        &[
            "shell",
            "dumpsys sensorservice | sed -n '/Sensor List:/,/Fusion States:/p'; echo __FEATURES__; pm list features 2>/dev/null | grep -F android.hardware.fingerprint",
        ],
    )?;
    let sensors = parse_sensor_inventory(&output);
    cache.lock().map_err(|e| e.to_string())?.insert(serial.to_string(), sensors.clone());
    Ok(sensors)
}

fn parse_sensor_inventory(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with("feature:android.hardware.fingerprint") {
                return Some("Fingerprint authentication".to_string());
            }
            if line.starts_with("0x") || line.starts_with("Handle ") {
                let name = line
                    .split_once(')')
                    .map(|(_, name)| name.split('|').next().unwrap_or(name).trim())
                    .filter(|name| !name.is_empty())
                    .or_else(|| line.split_once(':').map(|(_, name)| name.trim()))?;
                (!name.is_empty()).then(|| name.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn parse_top_memory_kb(value: &str) -> Option<u64> {
    let value = value.trim();
    let (number, multiplier) = match value.chars().last()? {
        'K' | 'k' => (&value[..value.len() - 1], 1.0),
        'M' | 'm' => (&value[..value.len() - 1], 1024.0),
        'G' | 'g' => (&value[..value.len() - 1], 1024.0 * 1024.0),
        _ => (value, 1.0 / 1024.0),
    };
    number
        .parse::<f64>()
        .ok()
        .map(|value| (value * multiplier) as u64)
}

fn parse_getprop_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if !line.starts_with('[') {
        return None;
    }
    let mut parts = line.split("]: [");
    let key = parts.next()?.strip_prefix('[')?;
    let val = parts.next()?.strip_suffix(']')?;
    Some((key, val))
}

/// Sends a simulated Android hardware keyevent (e.g. 26 for Power, 24/25 for Volume, 3 for Home, 4 for Back).
pub fn send_keyevent(serial: &str, keycode: u32) -> Result<(), String> {
    let code_str = keycode.to_string();
    adb_cmd(Some(serial), &["shell", "input", "keyevent", &code_str]).map(|_| ())
}

/// Injects text directly into the focused field on the device.
pub fn send_text(serial: &str, text: &str) -> Result<(), String> {
    // Android `input text` needs space escaping or quoting
    let escaped = text.replace(' ', "%s");
    adb_cmd(Some(serial), &["shell", "input", "text", &escaped]).map(|_| ())
}

/// Takes a device screenshot and writes PNG bytes directly to the host filesystem.
pub fn take_screenshot(serial: &str, dest_path: &Path) -> Result<(), String> {
    let mut cmd = Command::new("adb");
    if !serial.is_empty() {
        cmd.arg("-s").arg(serial);
    }
    cmd.args(["exec-out", "screencap", "-p"]);

    match cmd.output() {
        Ok(out) => {
            if out.status.success() && !out.stdout.is_empty() {
                if let Err(e) = std::fs::write(dest_path, &out.stdout) {
                    Err(format!("Failed to save screenshot: {}", e))
                } else {
                    Ok(())
                }
            } else {
                Err("Failed to capture screen: empty or error output".to_string())
            }
        }
        Err(e) => Err(format!("Failed to execute screencap: {}", e)),
    }
}

/// Retrieves the current text from the device clipboard.
pub fn get_clipboard(serial: &str) -> Result<String, String> {
    // 1. Try modern Android `cmd clipboard get`
    if let Ok(res) = adb_cmd(Some(serial), &["shell", "cmd", "clipboard", "get"]) {
        if !res.is_empty() && !res.to_lowercase().contains("error") {
            return Ok(res);
        }
    }

    // 2. Fallback service call or dumpsys clipboard
    if let Ok(dumpsys) = adb_cmd(Some(serial), &["shell", "dumpsys", "clipboard"]) {
        for line in dumpsys.lines() {
            if line.contains("text=") {
                if let Some(txt) = line.split("text=").nth(1) {
                    return Ok(txt.trim_matches(|c| c == '}' || c == '"' || c == ' ').to_string());
                }
            }
        }
    }

    Err("Clipboard is empty or Android clipboard service is restricted".to_string())
}

/// Injects text into the device clipboard.
pub fn set_clipboard(serial: &str, text: &str) -> Result<(), String> {
    // Modern Android: `cmd clipboard set <text>`
    if let Ok(_) = adb_cmd(Some(serial), &["shell", "cmd", "clipboard", "set", text]) {
        return Ok(());
    }

    // Fallback: inject via input text
    send_text(serial, text)
}

/// Spawns `scrcpy` in the background targeting this specific device serial with optional parameters.
pub fn launch_scrcpy(
    serial: &str,
    stay_awake: bool,
    turn_screen_off: bool,
    no_audio: bool,
    max_fps: Option<u32>,
    bitrate_mbps: Option<u32>,
) -> Result<u32, String> {
    let mut cmd = Command::new("scrcpy");
    if !serial.is_empty() {
        cmd.arg("-s").arg(serial);
    }
    if stay_awake {
        cmd.arg("--stay-awake");
    }
    if turn_screen_off {
        cmd.arg("--turn-screen-off");
    }
    if no_audio {
        cmd.arg("--no-audio");
    }
    if let Some(fps) = max_fps {
        cmd.arg(format!("--max-fps={}", fps));
    }
    if let Some(br) = bitrate_mbps {
        cmd.arg(format!("--video-bit-rate={}M", br));
    }

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    match cmd.spawn() {
        Ok(child) => Ok(child.id()),
        Err(e) => Err(format!("Failed to launch scrcpy: {}. Ensure scrcpy is installed.", e)),
    }
}

/// Terminates a running scrcpy process by PID.
pub fn stop_scrcpy(pid: u32) {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
    }
}

/// Checks if a scrcpy process is still actively running.
pub fn is_scrcpy_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Lists installed packages on device (third-party or system).
pub fn list_packages(serial: &str, third_party_only: bool) -> Vec<AdbPackageInfo> {
    let flag = if third_party_only { "-3" } else { "-s" };
    let mut list = Vec::new();

    if let Ok(out) = adb_cmd(Some(serial), &["shell", "pm", "list", "packages", flag]) {
        for line in out.lines() {
            if let Some(pkg) = line.trim().strip_prefix("package:") {
                list.push(AdbPackageInfo {
                    name: pkg.trim().to_string(),
                    is_system: !third_party_only,
                });
            }
        }
    }

    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

/// Launches an application by package name using the Android monkey tool.
pub fn launch_app(serial: &str, package: &str) -> Result<(), String> {
    adb_cmd(Some(serial), &["shell", "monkey", "-p", package, "-c", "android.intent.category.LAUNCHER", "1"]).map(|_| ())
}

/// Force-stops an application by package name.
pub fn force_stop_app(serial: &str, package: &str) -> Result<(), String> {
    adb_cmd(Some(serial), &["shell", "am", "force-stop", package]).map(|_| ())
}

/// Clears application data and cache.
pub fn clear_app_data(serial: &str, package: &str) -> Result<(), String> {
    adb_cmd(Some(serial), &["shell", "pm", "clear", package]).map(|_| ())
}

/// Uninstalls an application.
pub fn uninstall_app(serial: &str, package: &str) -> Result<(), String> {
    adb_cmd(Some(serial), &["shell", "pm", "uninstall", package]).map(|_| ())
}

/// Installs an APK from the host file system.
pub fn install_apk(serial: &str, apk_path: &Path) -> Result<String, String> {
    let path_str = apk_path.to_string_lossy().to_string();
    adb_cmd(Some(serial), &["install", "-r", "-d", &path_str])
}

/// Captures the device screen and returns raw PNG bytes in memory.
pub fn capture_screen_raw(serial: &str) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("adb");
    if !serial.is_empty() {
        cmd.arg("-s").arg(serial);
    }
    cmd.args(["exec-out", "screencap", "-p"]);

    match cmd.output() {
        Ok(out) => {
            if out.status.success() && !out.stdout.is_empty() {
                Ok(out.stdout)
            } else {
                Err("Failed to capture screen: empty or error output".to_string())
            }
        }
        Err(e) => Err(format!("Failed to execute screencap: {}", e)),
    }
}

/// Sends a simulated tap at device coordinates (x, y).
pub fn send_tap(serial: &str, x: i32, y: i32) -> Result<(), String> {
    let xs = x.to_string();
    let ys = y.to_string();
    adb_cmd(Some(serial), &["shell", "input", "tap", &xs, &ys]).map(|_| ())
}

/// Sends a simulated swipe from (x1, y1) to (x2, y2) over duration_ms.
pub fn send_swipe(serial: &str, x1: i32, y1: i32, x2: i32, y2: i32, duration_ms: u32) -> Result<(), String> {
    let x1s = x1.to_string();
    let y1s = y1.to_string();
    let x2s = x2.to_string();
    let y2s = y2.to_string();
    let ds = duration_ms.to_string();
    adb_cmd(Some(serial), &["shell", "input", "swipe", &x1s, &y1s, &x2s, &y2s, &ds]).map(|_| ())
}

/// Lists files in an Android directory.
pub fn list_directory(serial: &str, remote_path: &str) -> Result<Vec<AdbFileEntry>, String> {
    let clean_path = if remote_path.is_empty() {
        "/sdcard/".to_string()
    } else if remote_path.ends_with('/') {
        remote_path.to_string()
    } else {
        format!("{}/", remote_path)
    };

    let out = adb_cmd(Some(serial), &["shell", "ls", "-la", &clean_path])?;
    let mut entries = Vec::new();

    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("total ") || trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 7 {
            continue;
        }

        let perms = parts[0];
        let is_dir = perms.starts_with('d') || perms.starts_with('l');
        let size_bytes = parts.get(4).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);

        // Filename is from index 7 onwards (to preserve spaces in filenames)
        // If there is a symlink "name -> target", extract the link's name
        let raw_name = parts[7..].join(" ");
        let name = if let Some((link_name, _)) = raw_name.split_once(" -> ") {
            link_name.trim().to_string()
        } else {
            raw_name
        };

        if name == "." || name == ".." {
            continue;
        }

        let modified = if parts.len() >= 7 {
            format!("{} {}", parts[5], parts[6])
        } else {
            String::new()
        };

        entries.push(AdbFileEntry {
            name,
            is_dir,
            size_bytes,
            permissions: perms.to_string(),
            modified,
        });
    }

    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(entries)
}

/// Lists user-browsable storage volumes, collapsing duplicate aliases of the emulated volume.
pub fn list_storage_volumes(serial: &str) -> Vec<(String, String, u64, u64)> {
    let mounts = adb_cmd(Some(serial), &["shell", "ls", "-1", "/storage"]).unwrap_or_default();
    let mut paths = vec![("Internal storage".to_string(), "/storage/emulated/0".to_string())];
    paths.extend(mounts.lines().map(str::trim)
        .filter(|name| !name.is_empty() && *name != "emulated" && *name != "self" && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .map(|name| ("SD card".to_string(), format!("/storage/{name}"))));
    let mut volumes = Vec::new();
    for (name, path) in paths {
        let output = adb_cmd(Some(serial), &["shell", "df", "-k", &path]).unwrap_or_default();
        if let Some(fields) = output.lines().skip(1).map(|line| line.split_whitespace().collect::<Vec<_>>()).find(|fields| fields.len() >= 5) {
            if let (Ok(total), Ok(free)) = (fields[1].parse(), fields[3].parse()) {
                volumes.push((name, path, total, free));
            }
        }
    }
    volumes
}

/// Deletes a file or directory on the device.
pub fn delete_file(serial: &str, remote_path: &str) -> Result<(), String> {
    adb_cmd(Some(serial), &["shell", "rm", "-rf", remote_path]).map(|_| ())
}

/// Pushes a local file or directory to a remote path on the device.
pub fn push_file(serial: &str, local_path: &Path, remote_path: &str) -> Result<String, String> {
    let local_str = local_path.to_string_lossy().to_string();
    adb_cmd(Some(serial), &["push", &local_str, remote_path])
}

/// Pulls a remote file from the device to a local destination directory or file.
pub fn pull_file(serial: &str, remote_path: &str, local_dest: &Path) -> Result<String, String> {
    let local_str = local_dest.to_string_lossy().to_string();
    adb_cmd(Some(serial), &["pull", remote_path, &local_str])
}

/// Pulls a remote file or folder without overwriting an existing local item.
pub fn pull_file_to_path(serial: &str, remote_path: &str, local_path: &Path) -> Result<String, String> {
    if local_path.exists() {
        return Err(format!("{} already exists", local_path.display()));
    }
    pull_file(serial, remote_path, local_path)
}

/// Pulls a remote folder into a local directory without overwriting an existing folder.
pub fn pull_directory_to_directory(serial: &str, remote_path: &str, local_dir: &Path) -> Result<String, String> {
    let name = Path::new(remote_path).file_name().ok_or_else(|| "Invalid device path".to_string())?;
    let destination = local_dir.join(name);
    if destination.exists() {
        return Err(format!("{} already exists", destination.display()));
    }
    pull_file(serial, remote_path, &destination)
}

/// Reboots device to a specific target: "recovery", "bootloader", "fastboot", "edl", or normal system.
pub fn reboot_device(serial: &str, target: &str) -> Result<String, String> {
    let target = target.trim();
    if target.is_empty() || target == "system" {
        adb_cmd(Some(serial), &["reboot"])
    } else {
        adb_cmd(Some(serial), &["reboot", target])
    }
}

/// Sideloads an OTA update package zip in recovery mode (`adb sideload <file.zip>`).
pub fn sideload_ota(serial: &str, zip_path: &Path) -> Result<String, String> {
    let zip_str = zip_path.to_string_lossy().to_string();
    adb_cmd(Some(serial), &["sideload", &zip_str])
}

/// Generates a full Android bugreport archive to the host filesystem.
pub fn generate_bugreport(serial: &str, dest_zip: &Path) -> Result<String, String> {
    let dest_str = dest_zip.to_string_lossy().to_string();
    adb_cmd(Some(serial), &["bugreport", &dest_str])
}

/// Checks attached fastboot devices.
pub fn list_fastboot_devices() -> Vec<String> {
    let mut list = Vec::new();
    let mut cmd = Command::new("fastboot");
    cmd.arg("devices");
    if let Ok(out) = cmd.output() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            if let Some(serial) = line.split_whitespace().next() {
                list.push(serial.to_string());
            }
        }
    }
    list
}

/// Flashes a partition image via Fastboot (e.g. `boot`, `recovery`, `init_boot`).
pub fn fastboot_flash(partition: &str, img_path: &Path) -> Result<String, String> {
    let img_str = img_path.to_string_lossy().to_string();
    let mut cmd = Command::new("fastboot");
    cmd.args(["flash", partition, &img_str]);

    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if out.status.success() {
                Ok(stdout.trim().to_string())
            } else {
                Err(format!("Fastboot error: {}", stderr.trim()))
            }
        }
        Err(e) => Err(format!("Failed to execute fastboot: {}", e)),
    }
}

/// Launches an external host terminal running an interactive `adb -s <serial> shell`.
pub fn launch_external_adb_terminal(serial: &str) -> Result<(), String> {
    let script_dir = std::env::temp_dir();
    let safe_serial = if serial.trim().is_empty() {
        "default".to_string()
    } else {
        serial.replace(['.', ':', '-'], "_")
    };
    let script_path = script_dir.join(format!("launcher_adb_shell_{}.sh", safe_serial));

    let adb_target = if serial.trim().is_empty() {
        "adb shell".to_string()
    } else {
        format!("adb -s \"{}\" shell", serial.trim())
    };

    let script_content = format!(
r#"#!/usr/bin/env bash
echo "=========================================================="
echo " Server Launcher: Android ADB Interactive Shell"
echo " Device : {display_serial}"
echo "=========================================================="
echo "Connecting to {display_serial}..."
{adb_target}
EXIT_CODE=$?
echo ""
echo "ADB shell session closed (exit code: $EXIT_CODE)."
echo "Press Enter to exit."
read -r
"#,
        display_serial = if serial.trim().is_empty() { "default device" } else { serial.trim() },
        adb_target = adb_target,
    );

    let _ = std::fs::write(&script_path, script_content);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(&script_path) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&script_path, perms);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let script_str = script_path.to_string_lossy().to_string();
        let chosen = crate::service::find_terminal();
        let mut cmd;
        if let Some((term, args)) = chosen {
            cmd = Command::new(term);
            for arg in args {
                cmd.arg(arg);
            }
            cmd.arg(&script_str);
        } else {
            let fallbacks = [
                ("ptyxis", vec!["--new-window", "--", script_str.as_str()]),
                ("x-terminal-emulator", vec!["-e", script_str.as_str()]),
                ("gnome-terminal", vec!["--", script_str.as_str()]),
                ("kgx", vec!["-e", script_str.as_str()]),
                ("konsole", vec!["-e", script_str.as_str()]),
                ("xfce4-terminal", vec!["-e", script_str.as_str()]),
                ("alacritty", vec!["-e", script_str.as_str()]),
                ("kitty", vec![script_str.as_str()]),
                ("xterm", vec!["-e", script_str.as_str()]),
            ];

            for (term_bin, args) in fallbacks {
                let mut c = Command::new(term_bin);
                c.args(&args);
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    c.process_group(0);
                }
                if let Ok(mut child) = c.spawn() {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    return Ok(());
                }
            }

            return Err("No compatible terminal emulator (ptyxis, gnome-terminal, konsole, xterm) found.".to_string());
        }

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn terminal: {}", e))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(&["/K", &format!("adb -s {} shell", serial)]);
        cmd.spawn().map_err(|e| format!("Failed to spawn cmd: {}", e))?;
        Ok(())
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// PHONE LINK SUITE: NOTIFICATIONS, CALLS, MESSAGING, GALLERY & QUICK CONTROLS
// ═════════════════════════════════════════════════════════════════════════════

pub fn format_timestamp_millis(ms: i64) -> String {
    if let Some(dt) = chrono::DateTime::from_timestamp_millis(ms) {
        let local_dt = chrono::DateTime::<chrono::Local>::from(dt);
        local_dt.format("%b %-d, %-I:%M %p").to_string()
    } else {
        "Recent".to_string()
    }
}

pub fn format_timestamp_secs(secs: i64) -> String {
    if let Some(dt) = chrono::DateTime::from_timestamp(secs, 0) {
        let local_dt = chrono::DateTime::<chrono::Local>::from(dt);
        local_dt.format("%b %-d, %-I:%M %p").to_string()
    } else {
        "Recent".to_string()
    }
}

/// Fetches active notifications from the Android device via `dumpsys notification`.
pub fn get_notifications(serial: Option<&str>) -> Vec<AdbNotification> {
    let mut notifications = Vec::new();
    if let Ok(raw) = adb_cmd(serial, &["shell", "dumpsys", "notification", "--noredact"])
        .or_else(|_| adb_cmd(serial, &["shell", "dumpsys", "notification"]))
    {
        let mut cur_pkg = String::new();
        let mut cur_id = String::new();
        let mut cur_title = String::new();
        let mut cur_text = String::new();
        let mut cur_subtext = String::new();
        let mut cur_time = String::new();

        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.contains("NotificationRecord(") {
                if !cur_title.is_empty() || !cur_text.is_empty() {
                    let app_name = cur_pkg.rsplit('.').next().unwrap_or("App").to_string();
                    notifications.push(AdbNotification {
                        id: if cur_id.is_empty() { format!("notif_{}", notifications.len()) } else { cur_id.clone() },
                        package: cur_pkg.clone(),
                        app_name: clean_package_label(&app_name),
                        title: cur_title.clone(),
                        text: cur_text.clone(),
                        subtext: cur_subtext.clone(),
                        time_str: if cur_time.is_empty() { "Just now".to_string() } else { cur_time.clone() },
                        key: format!("{}:{}", cur_pkg, cur_id),
                    });
                    cur_title.clear();
                    cur_text.clear();
                    cur_subtext.clear();
                }

                // Extract package name
                if let Some(pkg_idx) = trimmed.find("pkg=") {
                    let rem = &trimmed[pkg_idx + 4..];
                    if let Some(end) = rem.find(|c: char| c.is_whitespace() || c == ':') {
                        cur_pkg = rem[..end].to_string();
                    }
                }
                if let Some(id_idx) = trimmed.find("id=") {
                    let rem = &trimmed[id_idx + 3..];
                    if let Some(end) = rem.find(|c: char| c.is_whitespace() || c == ':') {
                        cur_id = rem[..end].to_string();
                    }
                }
            } else if trimmed.starts_with("android.title=String (") {
                if let Some(val) = trimmed.strip_prefix("android.title=String (") {
                    cur_title = val.trim_end_matches(')').to_string();
                }
            } else if trimmed.starts_with("android.text=String (") {
                if let Some(val) = trimmed.strip_prefix("android.text=String (") {
                    cur_text = val.trim_end_matches(')').to_string();
                }
            } else if trimmed.starts_with("android.subText=String (") {
                if let Some(val) = trimmed.strip_prefix("android.subText=String (") {
                    cur_subtext = val.trim_end_matches(')').to_string();
                }
            } else if trimmed.starts_with("postTime=") {
                cur_time = "Recent".to_string();
            }
        }

        if !cur_title.is_empty() || !cur_text.is_empty() {
            let app_name = cur_pkg.rsplit('.').next().unwrap_or("App").to_string();
            notifications.push(AdbNotification {
                id: if cur_id.is_empty() { format!("notif_{}", notifications.len()) } else { cur_id.clone() },
                package: cur_pkg,
                app_name: clean_package_label(&app_name),
                title: cur_title,
                text: cur_text,
                subtext: cur_subtext,
                time_str: "Just now".to_string(),
                key: format!("k_{}", notifications.len()),
            });
        }
    }

    notifications
}

fn clean_package_label(pkg: &str) -> String {
    let mut chars = pkg.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Fetches recent call logs from Android `content://call_log/calls`.
pub fn get_call_logs(serial: Option<&str>) -> Vec<AdbCallLogEntry> {
    let mut list = Vec::new();
    let query_args = &[
        "content",
        "query",
        "--uri",
        "content://call_log/calls",
        "--projection",
        "_id,number,name,date,duration,type",
        "--sort",
        "\"date DESC\"",
    ];

    let mut raw_opt = adb_cmd(serial, &[&["shell"], &query_args[..]].concat().as_slice()).ok();
    if let Some(ref raw) = raw_opt {
        if raw.contains("SecurityException") || raw.contains("Permission Denial") {
            // Attempt with root su -c if available
            raw_opt = adb_cmd(
                serial,
                &[
                    "shell",
                    "su",
                    "-c",
                    "content query --uri content://call_log/calls --projection _id,number,name,date,duration,type --sort 'date DESC'",
                ],
            )
            .ok();
        }
    }

    if let Some(raw) = raw_opt {
        for line in raw.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("Row:") {
                continue;
            }

            let mut id = String::new();
            let mut number = String::new();
            let mut name = String::new();
            let mut duration = 0u64;
            let mut call_type = AdbCallType::Incoming;
            let mut date_millis: Option<i64> = None;

            for token in trimmed.split(',') {
                let kv = token.trim();
                if let Some(val) = kv.strip_prefix("_id=") {
                    id = val.to_string();
                } else if let Some(val) = kv.strip_prefix("number=") {
                    number = val.to_string();
                } else if let Some(val) = kv.strip_prefix("name=") {
                    if val != "NULL" {
                        name = val.to_string();
                    }
                } else if let Some(val) = kv.strip_prefix("duration=") {
                    duration = val.parse().unwrap_or(0);
                } else if let Some(val) = kv.strip_prefix("date=") {
                    date_millis = val.parse::<i64>().ok();
                } else if let Some(val) = kv.strip_prefix("type=") {
                    match val.trim() {
                        "1" => call_type = AdbCallType::Incoming,
                        "2" => call_type = AdbCallType::Outgoing,
                        "3" => call_type = AdbCallType::Missed,
                        _ => call_type = AdbCallType::Incoming,
                    }
                }
            }

            if !number.is_empty() {
                let display_name = if name.is_empty() { number.clone() } else { name };
                let dur_str = if call_type == AdbCallType::Missed {
                    "Missed".to_string()
                } else if duration == 0 {
                    "0s".to_string()
                } else if duration < 60 {
                    format!("{}s", duration)
                } else {
                    format!("{}m {}s", duration / 60, duration % 60)
                };
                let time_str = date_millis
                    .map(format_timestamp_millis)
                    .unwrap_or_else(|| "Recent".to_string());
                list.push(AdbCallLogEntry {
                    id,
                    number,
                    name: display_name,
                    call_type,
                    time_str,
                    duration_str: dur_str,
                });
            }
        }
    }

    list
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AdbContactEntry {
    pub id: String,
    pub name: String,
    pub number: String,
}

/// Fetches contacts from Android contacts content providers.
pub fn get_contacts(serial: Option<&str>) -> Result<Vec<AdbContactEntry>, String> {
    let mut list = Vec::new();
    let query = "content query --uri content://com.android.contacts/data/phones --projection _id:display_name:data1";
    let mut raw = adb_cmd(serial, &["shell", &format!("{query} 2>&1")])?;
    if raw.contains("Exception") || raw.contains("Permission Denial") || raw.contains("Error") {
        raw = adb_cmd(serial, &["shell", &format!("su -c '{query}' 2>&1")])
            .map_err(|_| "Contacts access denied by Android. Pairing Bluetooth does not grant contacts access.".to_string())?;
        if raw.contains("Exception") || raw.contains("denied") || raw.contains("not found") || raw.contains("Error") {
            return Err("Contacts access denied by Android. This device does not allow ADB to read its address book.".to_string());
        }
    }
    {
        for line in raw.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("Row:") {
                continue;
            }
            let mut id = String::new();
            let mut name = String::new();
            let mut number = String::new();

            for token in trimmed.split(',') {
                let kv = token.trim();
                if let Some(val) = kv.strip_prefix("_id=") {
                    id = val.to_string();
                } else if let Some(val) = kv.strip_prefix("display_name=").or_else(|| kv.strip_prefix("name=")) {
                    if val != "NULL" {
                        name = val.to_string();
                    }
                } else if let Some(val) = kv.strip_prefix("data1=").or_else(|| kv.strip_prefix("number=")) {
                    if val != "NULL" {
                        number = val.to_string();
                    }
                }
            }
            if !number.is_empty() || !name.is_empty() {
                list.push(AdbContactEntry {
                    id: if id.is_empty() { list.len().to_string() } else { id },
                    name: if name.is_empty() { "Unnamed Contact".to_string() } else { name },
                    number,
                });
            }
        }
    }

    Ok(list)
}

fn phone_call_uri(number: &str) -> Result<String, String> {
    let number = number.trim();
    if number.chars().any(|c| !c.is_ascii_digit() && !"+*# ()-.".contains(c)) {
        return Err("Phone numbers may contain digits, a leading +, * or #".into());
    }
    let clean: String = number.chars().filter(|c| c.is_ascii_digit() || "+*#".contains(*c)).collect();
    if !clean.chars().any(|c| c.is_ascii_digit()) || clean.chars().skip(1).any(|c| c == '+') {
        return Err("Please enter a valid phone number".into());
    }
    Ok(format!("tel:{}", clean.replace('#', "%23")))
}

/// Requests a call once; Android may require SIM selection or device-side confirmation.
pub fn start_phone_call(serial: Option<&str>, number: &str) -> Result<String, String> {
    let target = phone_call_uri(number)?;
    let result = adb_cmd(serial, &["shell", &format!("am start -a android.intent.action.CALL -d '{target}' -f 0x10000000 2>&1")]);
    match result {
        Ok(output) if !output.contains("Error") && !output.contains("Exception") && !output.contains("Permission Denial") => {
            Ok("Call requested. Check the device for SIM selection or confirmation.".into())
        }
        result => {
            let reason = match result { Ok(output) | Err(output) => output };
            if reason.contains("SecurityException") || reason.contains("Permission Denial") {
                let dial = adb_cmd(serial, &["shell", &format!("am start -a android.intent.action.DIAL -d '{target}' -f 0x10000000 2>&1")])?;
                if !dial.contains("Error") && !dial.contains("Exception") && !dial.contains("Permission Denial") {
                    return Ok("Dialer opened. Tap Call on the device to place the call.".into());
                }
                return Err("Android could not open the dialer. Unlock the device and try again.".into());
            }
            Err(format!("Android could not start the call: {}", reason.lines().next().unwrap_or("Unknown error")))
        }
    }
}

#[cfg(test)]
mod call_tests {
    use super::phone_call_uri;
    #[test]
    fn validates_and_encodes_phone_targets() {
        assert_eq!(phone_call_uri("+94 (77) 123-4567").unwrap(), "tel:+94771234567");
        assert_eq!(phone_call_uri("*123#").unwrap(), "tel:*123%23");
        for value in ["", "+", "12+34", "12;reboot", "abc123"] {
            assert!(phone_call_uri(value).is_err());
        }
    }
}

/// Ends an active phone call via Android KEYCODE_ENDCALL (6).
pub fn end_phone_call(serial: Option<&str>) -> Result<String, String> {
    adb_cmd(serial, &["shell", "input", "keyevent", "6"])
}

/// Fetches SMS messages from Android `content://sms`.
pub fn get_sms_messages(serial: Option<&str>) -> Vec<AdbSmsMessage> {
    let mut list = Vec::new();
    let query_res = adb_cmd(
        serial,
        &[
            "shell",
            "content",
            "query",
            "--uri",
            "content://sms",
            "--projection",
            "_id,address,body,date,type",
            "--sort",
            "\"date DESC\"",
        ],
    );

    if let Ok(raw) = query_res {
        // Collect multi-line rows
        let mut raw_rows: Vec<String> = Vec::new();
        for line in raw.lines() {
            if line.starts_with("Row: ") {
                raw_rows.push(line.to_string());
            } else if let Some(last) = raw_rows.last_mut() {
                last.push('\n');
                last.push_str(line);
            }
        }

        for row_str in raw_rows {
            // Row format: Row: <idx> _id=<id>, address=<addr>, body=<body>, date=<date>, type=<type>
            let after_row = if let Some(idx) = row_str.find("_id=") {
                &row_str[idx..]
            } else {
                continue;
            };

            let addr_marker = ", address=";
            let body_marker = ", body=";
            let date_marker = ", date=";
            let type_marker = ", type=";

            let addr_pos = match after_row.find(addr_marker) {
                Some(p) => p,
                None => continue,
            };
            let id = after_row["_id=".len()..addr_pos].trim().to_string();

            let after_addr = &after_row[addr_pos + addr_marker.len()..];
            let body_pos = match after_addr.find(body_marker) {
                Some(p) => p,
                None => continue,
            };
            let address = after_addr[..body_pos].trim().to_string();

            let after_body = &after_addr[body_pos + body_marker.len()..];
            let type_pos = match after_body.rfind(type_marker) {
                Some(p) => p,
                None => continue,
            };
            let type_str = after_body[type_pos + type_marker.len()..].trim();
            let is_outgoing = type_str == "2";

            let before_type = &after_body[..type_pos];
            let date_pos = match before_type.rfind(date_marker) {
                Some(p) => p,
                None => continue,
            };
            let date_str = before_type[date_pos + date_marker.len()..].trim();
            let body = before_type[..date_pos].to_string();

            let time_str = date_str
                .parse::<i64>()
                .ok()
                .map(format_timestamp_millis)
                .unwrap_or_else(|| "Recent".to_string());

            if !address.is_empty() && !body.is_empty() {
                list.push(AdbSmsMessage {
                    id,
                    contact_name: address.clone(),
                    address,
                    text: body,
                    time_str,
                    is_outgoing,
                });
            }
        }
    }

    list
}

/// Sends an SMS message to a phone number using Android SENDTO intent.
pub fn send_sms(serial: Option<&str>, number: &str, text: &str) -> Result<String, String> {
    if number.trim().is_empty() {
        return Err("Recipient phone number is required".to_string());
    }
    if text.trim().is_empty() {
        return Err("Message body cannot be empty".to_string());
    }

    let sms_target = format!("sms:{}", number.trim());
    adb_cmd(
        serial,
        &[
            "shell",
            "am",
            "start",
            "-a",
            "android.intent.action.SENDTO",
            "-d",
            &sms_target,
            "--es",
            "sms_body",
            text,
        ],
    )
}

/// Lists camera and gallery photos stored on device MediaStore or standard storage folders.
pub fn list_photos(serial: Option<&str>) -> Vec<AdbPhotoItem> {
    let mut items = Vec::new();

    // 1. Query Android MediaStore
    if let Ok(raw) = adb_cmd(serial, &["shell", "content", "query", "--uri", "content://media/external/images/media"]) {
        for line in raw.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("Row:") {
                continue;
            }

            let mut name = String::new();
            let mut path = String::new();
            let mut size_bytes: Option<u64> = None;
            let mut date_secs: Option<i64> = None;

            for token in trimmed.split(", ") {
                let kv = token.trim();
                if let Some(val) = kv.strip_prefix("_display_name=") {
                    name = val.to_string();
                } else if let Some(val) = kv.strip_prefix("_data=") {
                    path = val.to_string();
                } else if let Some(val) = kv.strip_prefix("_size=") {
                    size_bytes = val.parse::<u64>().ok();
                } else if let Some(val) = kv.strip_prefix("date_modified=") {
                    date_secs = val.parse::<i64>().ok();
                }
            }

            if !path.is_empty() {
                if name.is_empty() {
                    name = Path::new(&path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("photo.jpg")
                        .to_string();
                }

                let size_str = if let Some(bytes) = size_bytes {
                    if bytes < 1024 {
                        format!("{} B", bytes)
                    } else if bytes < 1024 * 1024 {
                        format!("{:.1} KB", bytes as f64 / 1024.0)
                    } else {
                        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
                    }
                } else {
                    "--".to_string()
                };

                let date_str = date_secs
                    .map(format_timestamp_secs)
                    .unwrap_or_else(|| "Recent".to_string());

                items.push(AdbPhotoItem {
                    name,
                    path,
                    size_str,
                    date_str,
                });
            }
        }
    }

    // 2. Fallback via file system search if MediaStore is empty
    if items.is_empty() {
        let cmd = "find /sdcard/DCIM /sdcard/Pictures /sdcard/Download -maxdepth 3 -type f \\( -iname '*.jpg' -o -iname '*.png' -o -iname '*.webp' -o -iname '*.jpeg' \\) 2>/dev/null | head -n 48";
        if let Ok(out) = adb_cmd(serial, &["shell", cmd]) {
            for line in out.lines() {
                let path = line.trim().to_string();
                if path.is_empty() {
                    continue;
                }
                let name = Path::new(&path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("image.jpg")
                    .to_string();

                items.push(AdbPhotoItem {
                    name,
                    path,
                    size_str: "Image".to_string(),
                    date_str: "Device Photo".to_string(),
                });
            }
        }
    }

    items
}

/// Lists one newest-first MediaStore page; fallback paths use an offset cursor.
pub fn list_photos_page(serial: Option<&str>, cursor: Option<&str>, limit: usize) -> (Vec<AdbPhotoItem>, Option<String>) {
    let limit = limit.clamp(1, 100);
    let media_cursor = cursor.and_then(|value| value.strip_prefix("m:")).and_then(|value| value.parse::<u64>().ok());
    if cursor.is_none() || media_cursor.is_some() {
        let filter = media_cursor.map(|id| format!("--where \"_id < {id}\" ")).unwrap_or_default();
        let query = format!(
            "content query --uri content://media/external/images/media --projection _id:_display_name:_data:_size:date_modified {filter}--sort \"_id DESC\" 2>&1 | head -n {}",
            limit + 1
        );
        if let Ok(raw) = adb_cmd(serial, &["shell", &query]) {
            let mut page = Vec::new();
            for line in raw.lines().filter(|line| line.trim_start().starts_with("Row:")) {
                let mut id = String::new();
                let mut name = String::new();
                let mut path = String::new();
                let mut size_bytes = None;
                let mut date_secs = None;
                for token in line.trim().split(", ") {
                    let kv = token.trim();
                    if let Some(value) = kv.strip_prefix("_id=") { id = value.to_string(); }
                    else if let Some(value) = kv.strip_prefix("_display_name=") { name = value.to_string(); }
                    else if let Some(value) = kv.strip_prefix("_data=") { path = value.to_string(); }
                    else if let Some(value) = kv.strip_prefix("_size=") { size_bytes = value.parse::<u64>().ok(); }
                    else if let Some(value) = kv.strip_prefix("date_modified=") { date_secs = value.parse::<i64>().ok(); }
                }
                if path.is_empty() { continue; }
                if name.is_empty() { name = Path::new(&path).file_name().and_then(|value| value.to_str()).unwrap_or("photo.jpg").to_string(); }
                let size_str = size_bytes.map(|bytes| if bytes < 1024 { format!("{bytes} B") } else if bytes < 1024 * 1024 { format!("{:.1} KB", bytes as f64 / 1024.0) } else { format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)) }).unwrap_or_else(|| "--".into());
                page.push((id, AdbPhotoItem { name, path, size_str, date_str: date_secs.map(format_timestamp_secs).unwrap_or_else(|| "Recent".into()) }));
            }
            if !page.is_empty() || media_cursor.is_some() {
                let has_more = page.len() > limit;
                page.truncate(limit);
                let next = has_more.then(|| format!("m:{}", page.last().map(|item| item.0.as_str()).unwrap_or("")));
                return (page.into_iter().map(|item| item.1).collect(), next);
            }
        }
    }

    let all = list_photos(serial);
    let offset = cursor.and_then(|value| value.strip_prefix("f:")).and_then(|value| value.parse::<usize>().ok()).unwrap_or(0);
    let items: Vec<_> = all.iter().skip(offset).take(limit).cloned().collect();
    let next = (offset + items.len() < all.len()).then(|| format!("f:{}", offset + items.len()));
    (items, next)
}

/// Counts all images visible to Android MediaStore, falling back to shared storage when unavailable.
pub fn count_photos(serial: Option<&str>) -> usize {
    if let Ok(count) = adb_cmd(serial, &["shell", "content query --uri content://media/external/images/media --projection _id 2>/dev/null | grep -c '^Row:'"]) {
        if let Ok(count) = count.trim().parse() { if count > 0 { return count; } }
    }
    adb_cmd(serial, &["shell", "find /sdcard -type f \\( -iname '*.jpg' -o -iname '*.jpeg' -o -iname '*.png' -o -iname '*.webp' -o -iname '*.gif' \\) 2>/dev/null | wc -l"])
        .ok().and_then(|count| count.trim().parse().ok()).unwrap_or(0)
}

/// Pulls a remote file/photo to local directory.
pub fn pull_photo_to_downloads(serial: Option<&str>, remote_path: &str) -> Result<String, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let downloads_dir = Path::new(&home).join("Downloads");
    let dest_dir = if downloads_dir.exists() {
        downloads_dir
    } else {
        Path::new(&home).to_path_buf()
    };

    let filename = Path::new(remote_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("photo.jpg");
    let dest_file = dest_dir.join(filename);

    pull_file(serial.unwrap_or(""), remote_path, &dest_file)?;
    Ok(format!("Saved to {}", dest_file.display()))
}

/// Reads a remote image into memory through a temporary file for the photo preview.
pub fn read_remote_file(serial: &str, remote_path: &str) -> Result<Vec<u8>, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let local_path = std::env::temp_dir().join(format!("launcher-adb-preview-{}-{stamp}", std::process::id()));
    let pull_result = pull_file(serial, remote_path, &local_path);
    if let Err(e) = pull_result {
        let _ = std::fs::remove_file(&local_path);
        return Err(e);
    }
    let bytes = std::fs::read(&local_path).map_err(|e| format!("Failed to read preview: {e}"));
    let _ = std::fs::remove_file(local_path);
    bytes
}

/// Deletes a photo from the device.
pub fn delete_device_photo(serial: Option<&str>, remote_path: &str) -> Result<String, String> {
    adb_cmd(serial, &["shell", "rm", "-f", remote_path])
}

/// Toggles Do Not Disturb (DND) / Zen Mode.
pub fn toggle_dnd(serial: Option<&str>, enable: bool) -> Result<String, String> {
    let mode = if enable { "1" } else { "0" };
    adb_cmd(serial, &["shell", "settings", "put", "global", "zen_mode", mode])
}

/// Toggles device audio mute via KEYCODE_VOLUME_MUTE (164).
pub fn toggle_mute(serial: Option<&str>) -> Result<String, String> {
    adb_cmd(serial, &["shell", "input", "keyevent", "164"])
}

/// Increases volume via KEYCODE_VOLUME_UP (24).
pub fn volume_up(serial: Option<&str>) -> Result<String, String> {
    adb_cmd(serial, &["shell", "input", "keyevent", "24"])
}

/// Decreases volume via KEYCODE_VOLUME_DOWN (25).
pub fn volume_down(serial: Option<&str>) -> Result<String, String> {
    adb_cmd(serial, &["shell", "input", "keyevent", "25"])
}

/// Triggers media Play / Pause via KEYCODE_MEDIA_PLAY_PAUSE (85).
pub fn media_play_pause(serial: Option<&str>) -> Result<String, String> {
    adb_cmd(serial, &["shell", "input", "keyevent", "85"])
}

/// Skips to next media track via KEYCODE_MEDIA_NEXT (87).
pub fn media_next(serial: Option<&str>) -> Result<String, String> {
    adb_cmd(serial, &["shell", "input", "keyevent", "87"])
}

/// Rewinds to previous media track via KEYCODE_MEDIA_PREVIOUS (88).
pub fn media_previous(serial: Option<&str>) -> Result<String, String> {
    adb_cmd(serial, &["shell", "input", "keyevent", "88"])
}

/// Wakes up the device screen via KEYCODE_WAKEUP (224).
pub fn wake_screen(serial: Option<&str>) -> Result<String, String> {
    adb_cmd(serial, &["shell", "input", "keyevent", "224"])
}
