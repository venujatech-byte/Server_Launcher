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

use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdbDeviceInfo {
    pub serial: String,
    pub state: String, // "device", "offline", "unauthorized", "recovery", "bootloader"
    pub model: String,
    pub product: String,
    pub device: String,
    pub transport_id: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
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
                    telem.battery_temp_c = Some(t / 10.0);
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

    // 5. Partition storage metrics via df -k /data /sdcard
    if let Ok(df_raw) = adb_cmd(Some(serial), &["shell", "df", "-k", "/data", "/sdcard"]) {
        for line in df_raw.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 {
                let total_kb = parts[1].parse::<f32>().unwrap_or(0.0);
                let free_kb = parts[3].parse::<f32>().unwrap_or(0.0);
                let mount = parts[5];
                if mount == "/data" {
                    telem.storage_internal_total_gb = Some(total_kb / (1024.0 * 1024.0));
                    telem.storage_internal_free_gb = Some(free_kb / (1024.0 * 1024.0));
                } else if mount.contains("sdcard") || mount.contains("emulated") {
                    telem.storage_sdcard_total_gb = Some(total_kb / (1024.0 * 1024.0));
                    telem.storage_sdcard_free_gb = Some(free_kb / (1024.0 * 1024.0));
                }
            }
        }
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
    let xfce_cmd = format!("adb -s {} shell", serial);
    let terminal_runners = [
        ("gnome-terminal", vec!["--", "adb", "-s", serial, "shell"]),
        ("kgx", vec!["--", "adb", "-s", serial, "shell"]),
        ("konsole", vec!["-e", "adb", "-s", serial, "shell"]),
        ("xfce4-terminal", vec!["-e", xfce_cmd.as_str()]),
        ("alacritty", vec!["-e", "adb", "-s", serial, "shell"]),
        ("kitty", vec!["adb", "-s", serial, "shell"]),
        ("xterm", vec!["-e", "adb", "-s", serial, "shell"]),
    ];

    for (term_bin, args) in terminal_runners {
        if let Ok(mut child) = Command::new(term_bin).args(&args).spawn() {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(());
        }
    }

    Err("No compatible terminal emulator (gnome-terminal, alacritty, kitty, xterm) found.".to_string())
}
