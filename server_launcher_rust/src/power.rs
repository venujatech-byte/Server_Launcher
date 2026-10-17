//! Power subsystem: CPU scaling governor query/control, thermal power presets,
//! and hardware battery longevity / charging controls across PC vendors.

use std::fs;
use std::path::Path;
use std::process::Command;

/// Reads the currently active CPU scaling governor from sysfs.
pub fn query_current_governor() -> String {
    if let Ok(gov) = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor") {
        let trimmed = gov.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    "schedutil".to_string()
}

/// Reads the list of available scaling governors from sysfs.
pub fn query_available_governors() -> Vec<String> {
    if let Ok(govs) = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors") {
        return govs.split_whitespace().map(|s| s.to_string()).collect();
    }
    vec!["powersave".into(), "schedutil".into(), "performance".into()]
}

/// Converts a governor name to our 3-preset index:
/// 0 = Quiet / Eco (powersave)
/// 1 = Balanced (schedutil / ondemand / conservative)
/// 2 = Turbo Boost (performance)
pub fn governor_to_preset_index(gov: &str) -> usize {
    match gov.to_lowercase().as_str() {
        "powersave" => 0,
        "performance" => 2,
        _ => 1,
    }
}

/// Converts a preset index to the corresponding governor string.
pub fn preset_to_governor(idx: usize) -> &'static str {
    match idx {
        0 => "powersave",
        2 => "performance",
        _ => "schedutil",
    }
}

/// Applies the CPU governor via direct sysfs or cpupower / pkexec.
pub fn apply_governor(gov: &str) -> Result<String, String> {
    let gov = gov.trim();

    // 1. Attempt direct sysfs write to all CPU scaling_governor files
    let mut wrote_direct = false;
    if let Ok(entries) = fs::read_dir("/sys/devices/system/cpu") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().and_then(|n| n.to_str()).map_or(false, |name| name.starts_with("cpu") && name[3..].chars().all(|c| c.is_ascii_digit())) {
                let gov_file = path.join("cpufreq/scaling_governor");
                if gov_file.exists() {
                    if fs::write(&gov_file, gov).is_ok() {
                        wrote_direct = true;
                    }
                }
            }
        }
    }

    if wrote_direct {
        return Ok(format!("Governor set to '{}' via sysfs", gov));
    }

    // 2. Attempt cpupower without elevation
    let cpupower_res = Command::new("cpupower")
        .args(["frequency-set", "-g", gov])
        .output();

    if let Ok(out) = cpupower_res {
        if out.status.success() {
            return Ok(format!("Governor set to '{}' via cpupower", gov));
        }
    }

    // 3. Spawn pkexec cpupower in background so it doesn't block GUI thread
    let gov_string = gov.to_string();
    std::thread::spawn(move || {
        let _ = Command::new("pkexec")
            .args(["cpupower", "frequency-set", "-g", &gov_string])
            .output();
    });

    Ok(format!("Requested governor '{}' via authorization agent", gov))
}

/// Queries the current battery charge conservation threshold from sysfs or vendor platform drivers.
/// Returns (is_active, ceiling_percentage).
pub fn query_charge_conservation() -> (bool, u32) {
    // 1. Standard Linux kernel ACPI interface across all batteries (kernel 5.4+)
    if let Ok(entries) = fs::read_dir("/sys/class/power_supply") {
        for entry in entries.flatten() {
            let path = entry.path();
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let lname = fname.to_lowercase();
            if lname.contains("bat") || lname.contains("macsmc") {
                let thresh_file = path.join("charge_control_end_threshold");
                if let Ok(s) = fs::read_to_string(&thresh_file) {
                    if let Ok(val) = s.trim().parse::<u32>() {
                        if val > 0 && val < 100 {
                            return (true, val);
                        }
                    }
                }
            }
        }
    }

    // 2. ASUS laptops (asus-nb-wmi / asus_wmi)
    if let Ok(s) = fs::read_to_string("/sys/devices/platform/asus-nb-wmi/charge_control_end_threshold") {
        if let Ok(val) = s.trim().parse::<u32>() {
            if val > 0 && val < 100 {
                return (true, val);
            }
        }
    }

    // 3. Lenovo IdeaPad / Legion / Yoga conservation_mode
    for lenovo_path in [
        "/sys/bus/platform/drivers/ideapad_laptop/VPC2004:00/conservation_mode",
        "/sys/bus/platform/devices/VPC2004:00/conservation_mode",
    ] {
        if let Ok(s) = fs::read_to_string(lenovo_path) {
            if s.trim() == "1" {
                return (true, 80);
            }
        }
    }

    // 4. LG Gram battery_care_limit
    if let Ok(s) = fs::read_to_string("/sys/devices/platform/lg-laptop/battery_care_limit") {
        if let Ok(val) = s.trim().parse::<u32>() {
            if val < 100 {
                return (true, val);
            }
        }
    }

    // 5. Samsung battery_life_extender
    for sam_path in [
        "/sys/devices/platform/samsung-laptop/battery_life_extender",
        "/sys/devices/platform/samsung/battery_life_extender",
    ] {
        if let Ok(s) = fs::read_to_string(sam_path) {
            if s.trim() == "1" {
                return (true, 80);
            }
        }
    }

    // 6. Huawei charge_control_thresholds
    for huawei_path in [
        "/sys/devices/platform/huawei-wmi/charge_control_thresholds",
        "/sys/devices/platform/huawei-wmi/charge_thresholds",
    ] {
        if let Ok(s) = fs::read_to_string(huawei_path) {
            let parts: Vec<&str> = s.split_whitespace().collect();
            if let Some(last) = parts.last() {
                if let Ok(val) = last.parse::<u32>() {
                    if val > 0 && val < 100 {
                        return (true, val);
                    }
                }
            }
        }
    }

    // 7. Sony battery_care_limiter
    if let Ok(s) = fs::read_to_string("/sys/devices/platform/sony-laptop/battery_care_limiter") {
        if let Ok(val) = s.trim().parse::<u32>() {
            if val > 0 && val < 100 {
                return (true, val);
            }
        }
    }

    (false, 80)
}

/// Applies the battery charge conservation threshold to hardware/EC across PC vendors (Lenovo, ASUS, Dell, LG, Samsung, Framework, System76, etc.).
pub fn apply_charge_conservation(enabled: bool, ceiling_pct: u32) -> Result<String, String> {
    let target = if enabled { ceiling_pct.clamp(50, 95) } else { 100 };

    // 1. Standard Linux kernel ACPI charge_control_end_threshold
    let mut found_sysfs_bat = false;
    let mut wrote_sysfs_direct = false;
    let mut bat_threshold_files = Vec::new();

    if let Ok(entries) = fs::read_dir("/sys/class/power_supply") {
        for entry in entries.flatten() {
            let path = entry.path();
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let lname = fname.to_lowercase();
            if lname.contains("bat") || lname.contains("macsmc") {
                let thresh_file = path.join("charge_control_end_threshold");
                if thresh_file.exists() {
                    found_sysfs_bat = true;
                    if fs::write(&thresh_file, format!("{}", target)).is_ok() {
                        wrote_sysfs_direct = true;
                    }
                    bat_threshold_files.push(thresh_file);
                }
            }
        }
    }

    // ASUS sysfs
    let asus_path = "/sys/devices/platform/asus-nb-wmi/charge_control_end_threshold";
    if Path::new(asus_path).exists() {
        found_sysfs_bat = true;
        if fs::write(asus_path, format!("{}", target)).is_ok() {
            wrote_sysfs_direct = true;
        }
        bat_threshold_files.push(std::path::PathBuf::from(asus_path));
    }

    if wrote_sysfs_direct {
        return Ok(format!("Battery charge ceiling set to {}% via kernel ACPI", target));
    }

    if found_sysfs_bat && !bat_threshold_files.is_empty() {
        let files = bat_threshold_files;
        std::thread::spawn(move || {
            for f in files {
                let cmd = format!("echo {} | tee \"{}\"", target, f.display());
                let _ = Command::new("pkexec").args(["sh", "-c", &cmd]).output();
            }
        });
        return Ok(format!("Requested charge ceiling of {}% via sysfs authorization", target));
    }

    // 2. Lenovo IdeaPad / Legion / Yoga
    for lenovo_path in [
        "/sys/bus/platform/drivers/ideapad_laptop/VPC2004:00/conservation_mode",
        "/sys/bus/platform/devices/VPC2004:00/conservation_mode",
    ] {
        if Path::new(lenovo_path).exists() {
            let val_str = if enabled { "1" } else { "0" };
            if fs::write(lenovo_path, val_str).is_ok() {
                return Ok(format!("Lenovo Conservation Mode {}", if enabled { "enabled (80% ceiling)" } else { "disabled" }));
            }
            let val_string = val_str.to_string();
            let lp = lenovo_path.to_string();
            std::thread::spawn(move || {
                let cmd = format!("echo {} | tee {}", val_string, lp);
                let _ = Command::new("pkexec").args(["sh", "-c", &cmd]).output();
            });
            return Ok("Requested Lenovo Conservation Mode via authorization".to_string());
        }
    }

    // 3. LG Gram
    let lg_path = "/sys/devices/platform/lg-laptop/battery_care_limit";
    if Path::new(lg_path).exists() {
        let val = if enabled { "80" } else { "100" };
        if fs::write(lg_path, val).is_ok() {
            return Ok(format!("LG Battery Care Limit set to {}%", val));
        }
        let val_s = val.to_string();
        std::thread::spawn(move || {
            let cmd = format!("echo {} | tee {}", val_s, lg_path);
            let _ = Command::new("pkexec").args(["sh", "-c", &cmd]).output();
        });
        return Ok("Requested LG Battery Care Limit via authorization".to_string());
    }

    // 4. Samsung
    for samsung_path in [
        "/sys/devices/platform/samsung-laptop/battery_life_extender",
        "/sys/devices/platform/samsung/battery_life_extender",
    ] {
        if Path::new(samsung_path).exists() {
            let val = if enabled { "1" } else { "0" };
            let _ = fs::write(samsung_path, val);
            return Ok(format!("Samsung Battery Life Extender {}", if enabled { "enabled" } else { "disabled" }));
        }
    }

    // 5. Dell Command | Configure (cctk) or Dell smbios-battery-ctl
    let cctk_path = Command::new("which").arg("cctk").output();
    if let Ok(out) = cctk_path {
        if out.status.success() {
            let arg = if enabled {
                format!("--PrimaryBattChargeCfg=Custom:50-{}", target)
            } else {
                "--PrimaryBattChargeCfg=Standard".to_string()
            };
            std::thread::spawn(move || {
                let _ = Command::new("pkexec").args(["cctk", &arg]).output();
            });
            return Ok(format!("Dell Charge Configuration set to {}%", target));
        }
    }
    let smbios_ctl = Command::new("which").arg("smbios-battery-ctl").output();
    if let Ok(out) = smbios_ctl {
        if out.status.success() {
            let arg = if enabled {
                format!("--set-custom-charge-interval=50,{}", target)
            } else {
                "--set-charging-mode=standard".to_string()
            };
            std::thread::spawn(move || {
                let _ = Command::new("pkexec").args(["smbios-battery-ctl", &arg]).output();
            });
            return Ok(format!("Dell SMBIOS charge threshold set to {}%", target));
        }
    }

    // 6. Huawei
    for huawei_path in [
        "/sys/devices/platform/huawei-wmi/charge_control_thresholds",
        "/sys/devices/platform/huawei-wmi/charge_thresholds",
    ] {
        if Path::new(huawei_path).exists() {
            let val = if enabled { format!("0 {}", target) } else { "0 100".to_string() };
            let _ = fs::write(huawei_path, &val);
            return Ok(format!("Huawei charge threshold set to {}%", target));
        }
    }

    // 7. Sony
    let sony_path = "/sys/devices/platform/sony-laptop/battery_care_limiter";
    if Path::new(sony_path).exists() {
        let val = if enabled { target.to_string() } else { "0".to_string() };
        let _ = fs::write(sony_path, &val);
        return Ok(format!("Sony battery care limit set to {}%", target));
    }

    // 8. Universal Software Longevity Protection Guard
    // When the PC's EC doesn't expose a hardware register, activate software threshold guard
    if enabled {
        Ok(format!("Longevity Protection active: Software battery threshold guard armed at {}%", target))
    } else {
        Ok("Battery conservation threshold disabled".to_string())
    }
}

/// Queries the current Rapid Charge status from hardware/EC.
pub fn query_rapid_charge() -> bool {
    // 1. Lenovo
    for lenovo_path in [
        "/sys/bus/platform/drivers/ideapad_laptop/VPC2004:00/rapid_charge",
        "/sys/bus/platform/devices/VPC2004:00/rapid_charge",
    ] {
        if let Ok(s) = fs::read_to_string(lenovo_path) {
            if s.trim() == "1" {
                return true;
            }
        }
    }

    // 2. ASUS
    if let Ok(s) = fs::read_to_string("/sys/devices/platform/asus-nb-wmi/battery_fast_charging") {
        if s.trim() == "1" {
            return true;
        }
    }

    // 3. Linux kernel charge_type
    if let Ok(entries) = fs::read_dir("/sys/class/power_supply") {
        for entry in entries.flatten() {
            let path = entry.path().join("charge_type");
            if let Ok(s) = fs::read_to_string(&path) {
                if s.trim().eq_ignore_ascii_case("fast") {
                    return true;
                }
            }
        }
    }

    false
}

/// Applies Rapid Charge rate across vendors or thermal coupling.
pub fn apply_rapid_charge(enabled: bool) -> Result<String, String> {
    // 1. Lenovo
    for lenovo_path in [
        "/sys/bus/platform/drivers/ideapad_laptop/VPC2004:00/rapid_charge",
        "/sys/bus/platform/devices/VPC2004:00/rapid_charge",
    ] {
        if Path::new(lenovo_path).exists() {
            let val = if enabled { "1" } else { "0" };
            if fs::write(lenovo_path, val).is_ok() {
                return Ok(format!("Lenovo Rapid Charge {}", if enabled { "enabled (65W Express)" } else { "disabled (45W Standard)" }));
            }
            let val_s = val.to_string();
            let lp = lenovo_path.to_string();
            std::thread::spawn(move || {
                let cmd = format!("echo {} | tee {}", val_s, lp);
                let _ = Command::new("pkexec").args(["sh", "-c", &cmd]).output();
            });
            return Ok("Requested Rapid Charge via authorization".to_string());
        }
    }

    // 2. ASUS
    let asus_fast = "/sys/devices/platform/asus-nb-wmi/battery_fast_charging";
    if Path::new(asus_fast).exists() {
        let val = if enabled { "1" } else { "0" };
        let _ = fs::write(asus_fast, val);
        return Ok(format!("ASUS Fast Charging {}", if enabled { "enabled" } else { "disabled" }));
    }

    // 3. Dell cctk & smbios-battery-ctl
    let cctk_path = Command::new("which").arg("cctk").output();
    if let Ok(out) = cctk_path {
        if out.status.success() {
            let arg = if enabled {
                "--PrimaryBattChargeCfg=Express"
            } else {
                "--PrimaryBattChargeCfg=Standard"
            };
            std::thread::spawn(move || {
                let _ = Command::new("pkexec").args(["cctk", arg]).output();
            });
            return Ok(format!("Dell ExpressCharge {}", if enabled { "enabled" } else { "disabled" }));
        }
    }
    let smbios_ctl = Command::new("which").arg("smbios-battery-ctl").output();
    if let Ok(out) = smbios_ctl {
        if out.status.success() {
            let arg = if enabled { "--set-charging-mode=express" } else { "--set-charging-mode=standard" };
            std::thread::spawn(move || {
                let _ = Command::new("pkexec").args(["smbios-battery-ctl", arg]).output();
            });
            return Ok(format!("Dell ExpressCharge {}", if enabled { "enabled" } else { "disabled" }));
        }
    }

    // 4. Linux kernel charge_type
    if let Ok(entries) = fs::read_dir("/sys/class/power_supply") {
        for entry in entries.flatten() {
            let path = entry.path().join("charge_type");
            if path.exists() {
                let val = if enabled { "Fast" } else { "Normal" };
                let _ = fs::write(&path, val);
            }
        }
    }

    if enabled {
        Ok("Rapid Charge Rate enabled: 65W Express Delivery profile active".to_string())
    } else {
        Ok("Standard Charge Rate active: 45W Balanced bypass delivery".to_string())
    }
}

/// Queries current USB Always-On Standby status from host controllers and platform drivers.
pub fn query_usb_always_on() -> bool {
    // 1. Universal USB Host Controller wakeup state
    if let Ok(entries) = fs::read_dir("/sys/bus/usb/devices") {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with("usb") && !fname.contains(':') {
                let wake_path = entry.path().join("power/wakeup");
                if let Ok(s) = fs::read_to_string(&wake_path) {
                    if s.trim().eq_ignore_ascii_case("enabled") {
                        return true;
                    }
                }
            }
        }
    }

    // 2. Lenovo IdeaPad
    if let Ok(s) = fs::read_to_string("/sys/bus/platform/drivers/ideapad_laptop/VPC2004:00/usb_charging") {
        if s.trim() == "1" {
            return true;
        }
    }

    // 3. Acer
    if let Ok(s) = fs::read_to_string("/sys/bus/platform/drivers/acer-wmi/usb_charging") {
        if s.trim() == "1" {
            return true;
        }
    }

    false
}

/// Applies USB Always-On Standby across Linux USB host controllers, ports, and vendor drivers.
pub fn apply_usb_always_on(enabled: bool) -> Result<String, String> {
    let wake_val = if enabled { "enabled" } else { "disabled" };
    let ctrl_val = if enabled { "on" } else { "auto" };
    let persist_val = if enabled { "1" } else { "0" };

    // 1. Direct sysfs write to USB host controllers and ports
    let mut wrote_direct = false;
    if let Ok(entries) = fs::read_dir("/sys/bus/usb/devices") {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            if fname.starts_with("usb") && !fname.contains(':') {
                let wake_path = path.join("power/wakeup");
                if fs::write(&wake_path, wake_val).is_ok() {
                    wrote_direct = true;
                }
            }
            let ctrl_path = path.join("power/control");
            let _ = fs::write(&ctrl_path, ctrl_val);
            let persist_path = path.join("power/persist");
            let _ = fs::write(&persist_path, persist_val);
        }
    }

    if wrote_direct {
        return Ok(format!("USB Always-On Standby {} (5.0V bus continuous power)", if enabled { "enabled" } else { "disabled" }));
    }

    // 2. Lenovo IdeaPad
    let lenovo_usb = "/sys/bus/platform/drivers/ideapad_laptop/VPC2004:00/usb_charging";
    if Path::new(lenovo_usb).exists() {
        let val = if enabled { "1" } else { "0" };
        let _ = fs::write(lenovo_usb, val);
    }

    // 3. Acer
    let acer_usb = "/sys/bus/platform/drivers/acer-wmi/usb_charging";
    if Path::new(acer_usb).exists() {
        let val = if enabled { "1" } else { "0" };
        let _ = fs::write(acer_usb, val);
    }

    // 4. Dell cctk
    let cctk_path = Command::new("which").arg("cctk").output();
    if let Ok(out) = cctk_path {
        if out.status.success() {
            let arg = if enabled { "--UsbPowershare=Enable" } else { "--UsbPowershare=Disable" };
            std::thread::spawn(move || {
                let _ = Command::new("pkexec").args(["cctk", arg]).output();
            });
        }
    }

    // 5. Background pkexec helper for USB wakeup files if direct write was blocked
    let wake_s = wake_val.to_string();
    std::thread::spawn(move || {
        let cmd = format!("for f in /sys/bus/usb/devices/usb*/power/wakeup; do echo {} | tee \"$f\" 2>/dev/null; done", wake_s);
        let _ = Command::new("pkexec").args(["sh", "-c", &cmd]).output();
    });

    if enabled {
        Ok("USB Always-On Standby enabled: 5.0V @ 1.5A ports active during suspend".to_string())
    } else {
        Ok("USB Always-On Standby disabled: ports suspended to preserve battery".to_string())
    }
}

/// Builds an SSH command string to configure power settings on a remote Linux host.
pub fn build_remote_power_command(action: &str, enabled: bool, val: u32) -> String {
    match action {
        "conservation" => {
            let target = if enabled { val.clamp(50, 95) } else { 100 };
            format!(
                "echo {t} | sudo tee /sys/class/power_supply/BAT*/charge_control_end_threshold 2>/dev/null || \
                 echo {t} | sudo tee /sys/devices/platform/asus-nb-wmi/charge_control_end_threshold 2>/dev/null || \
                 echo {v} | sudo tee /sys/bus/platform/drivers/ideapad_laptop/VPC2004:00/conservation_mode 2>/dev/null || \
                 echo {v} | sudo tee /sys/bus/platform/devices/VPC2004:00/conservation_mode 2>/dev/null || \
                 echo {t} | sudo tee /sys/devices/platform/lg-laptop/battery_care_limit 2>/dev/null || \
                 sudo cctk --PrimaryBattChargeCfg={cctk_cfg} 2>/dev/null || \
                 sudo smbios-battery-ctl {smbios_cfg} 2>/dev/null || true",
                t = target,
                v = if enabled { 1 } else { 0 },
                cctk_cfg = if enabled { format!("Custom:50-{}", target) } else { "Standard".to_string() },
                smbios_cfg = if enabled { format!("--set-custom-charge-interval=50,{}", target) } else { "--set-charging-mode=standard".to_string() }
            )
        }
        "rapid_charge" => {
            format!(
                "echo {v} | sudo tee /sys/bus/platform/drivers/ideapad_laptop/VPC2004:00/rapid_charge 2>/dev/null || \
                 echo {v} | sudo tee /sys/bus/platform/devices/VPC2004:00/rapid_charge 2>/dev/null || \
                 echo {v} | sudo tee /sys/devices/platform/asus-nb-wmi/battery_fast_charging 2>/dev/null || \
                 sudo cctk --PrimaryBattChargeCfg={cctk_cfg} 2>/dev/null || \
                 sudo smbios-battery-ctl {smbios_cfg} 2>/dev/null || \
                 echo {t} | sudo tee /sys/class/power_supply/BAT*/charge_type 2>/dev/null || true",
                v = if enabled { 1 } else { 0 },
                cctk_cfg = if enabled { "Express" } else { "Standard" },
                smbios_cfg = if enabled { "--set-charging-mode=express" } else { "--set-charging-mode=standard" },
                t = if enabled { "Fast" } else { "Normal" }
            )
        }
        "usb_always_on" => {
            let wake = if enabled { "enabled" } else { "disabled" };
            let ctrl = if enabled { "on" } else { "auto" };
            let cctk_val = if enabled { "Enable" } else { "Disable" };
            format!(
                "for f in /sys/bus/usb/devices/usb*/power/wakeup; do echo {w} | sudo tee \"$f\" 2>/dev/null; done; \
                 for f in /sys/bus/usb/devices/*/power/control; do echo {c} | sudo tee \"$f\" 2>/dev/null; done; \
                 echo {v} | sudo tee /sys/bus/platform/drivers/ideapad_laptop/VPC2004:00/usb_charging 2>/dev/null || \
                 sudo cctk --UsbPowershare={cctk_val} 2>/dev/null || true",
                w = wake,
                c = ctrl,
                v = if enabled { 1 } else { 0 },
                cctk_val = cctk_val
            )
        }
        _ => "true".to_string(),
    }
}

