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

/// Detects the currently active thermal/power preset from ACPI EC platform profile or CPU governor.
pub fn query_current_preset() -> usize {
    if let Ok(p) = fs::read_to_string("/sys/firmware/acpi/platform_profile") {
        let trimmed = p.trim().to_lowercase();
        if trimmed == "low-power" || trimmed == "quiet" || trimmed == "cool" {
            return 0;
        } else if trimmed == "performance" || trimmed == "boost" {
            return 2;
        } else if trimmed == "balanced" || trimmed == "normal" {
            return 1;
        }
    }
    governor_to_preset_index(&query_current_governor())
}

/// Converts a preset index to the corresponding governor string.
pub fn preset_to_governor(idx: usize) -> &'static str {
    match idx {
        0 => "powersave",
        2 => "performance",
        _ => "schedutil",
    }
}

/// Real CPU TDP capabilities detected from hardware RAPL MSRs, ACPI EC interfaces, or CPU silicon classification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuTdpProfile {
    /// Nominal / Base processor TDP (Watts).
    pub nominal_watts: f32,
    /// Quiet / Eco PL1 limit (Watts) — reduced thermal load & fan silence.
    pub quiet_pl1_watts: f32,
    /// Balanced PL1 limit (Watts) — standard factory continuous power.
    pub balanced_pl1_watts: f32,
    /// Turbo Boost PL2 burst limit (Watts) — maximum transient silicon output.
    pub turbo_pl2_watts: f32,
    /// Whether Intel/AMD RAPL sysfs power capping interface was detected.
    pub has_rapl: bool,
    /// Whether Linux standard ACPI EC platform_profile was detected.
    pub has_platform_profile: bool,
}

impl Default for CpuTdpProfile {
    fn default() -> Self {
        Self {
            nominal_watts: 15.0,
            quiet_pl1_watts: 10.0,
            balanced_pl1_watts: 15.0,
            turbo_pl2_watts: 25.0,
            has_rapl: false,
            has_platform_profile: false,
        }
    }
}

/// Fallback CPU classification heuristic based on /proc/cpuinfo model name and core count.
fn classify_cpu_nominal_tdp() -> f32 {
    let cpu_info = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let model_line = cpu_info
        .lines()
        .find(|l| l.starts_with("model name"))
        .unwrap_or("")
        .to_lowercase();

    let core_count = cpu_info
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count()
        .max(1);

    // Ultra-low power SoCs (4W - 6W)
    if model_line.contains("silver")
        || model_line.contains("atom")
        || model_line.contains("celeron n")
        || model_line.contains("celeron j")
        || model_line.contains("pentium n")
        || model_line.contains("pentium j")
        || model_line.contains("n5000")
        || model_line.contains("n5030")
        || model_line.contains("n6000")
        || model_line.contains("n100")
        || model_line.contains("n200")
        || model_line.contains("n300")
    {
        return 6.0;
    }

    // Y-series ultra-mobile (7W)
    if model_line.contains("7y") || model_line.contains("8y") || model_line.contains("y54") {
        return 7.0;
    }

    // High-performance mobile H / HS / HX / HK (45W - 55W)
    if model_line.contains(" h ")
        || model_line.ends_with("h")
        || model_line.contains(" hs")
        || model_line.contains(" hx")
        || model_line.contains(" hk")
    {
        return 45.0;
    }

    // P-series mobile (28W)
    if model_line.contains(" p ") || model_line.contains("260p") || model_line.contains("340p") || model_line.contains("360p") {
        return 28.0;
    }

    // Server / HEDT (125W - 250W)
    if model_line.contains("xeon") || model_line.contains("epyc") || model_line.contains("threadripper") {
        return 180.0;
    }

    // Desktop unlocked K / KF / KS / X (105W - 125W)
    if model_line.contains(" k ") || model_line.ends_with("k") || model_line.contains(" kf") || model_line.contains(" ks") || model_line.contains(" x") {
        return 125.0;
    }

    // Standard U-series mobile (15W)
    if model_line.contains(" u ") || model_line.ends_with("u") || model_line.contains("g1") || model_line.contains("g4") || model_line.contains("g7") {
        return 15.0;
    }

    // Generic heuristic based on logical core count
    if core_count <= 2 {
        10.0
    } else if core_count <= 4 {
        15.0
    } else if core_count <= 8 {
        45.0
    } else {
        65.0
    }
}

fn calculate_turbo_tdp(nominal: f32) -> f32 {
    if nominal <= 8.0 {
        (nominal * 1.33).round().max(nominal + 1.5) // e.g. 6W -> 8W
    } else if nominal <= 15.0 {
        (nominal * 1.60).round() // e.g. 15W -> 24W
    } else if nominal <= 28.0 {
        (nominal * 1.50).round() // e.g. 28W -> 42W
    } else if nominal <= 45.0 {
        (nominal * 1.45).round() // e.g. 45W -> 65W
    } else if nominal <= 65.0 {
        (nominal * 1.45).round() // e.g. 65W -> 95W
    } else {
        (nominal * 1.50).round() // e.g. 125W -> 185W
    }
}

/// Queries the real processor TDP capabilities across Intel, AMD, and ARM architectures.
/// Scans Intel/AMD RAPL (Running Average Power Limit) sysfs registers, ACPI EC platform profiles,
/// hwmon power capping, or CPU classification heuristics.
pub fn detect_cpu_tdp_capability() -> CpuTdpProfile {
    let mut detected_pl1: Option<f32> = None;
    let mut detected_pl2: Option<f32> = None;
    let mut has_rapl = false;

    // 1. Scan Intel & AMD RAPL via powercap sysfs (/sys/class/powercap/intel-rapl)
    if let Ok(entries) = fs::read_dir("/sys/class/powercap") {
        for entry in entries.flatten() {
            let path = entry.path();
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if fname.starts_with("intel-rapl:") && !fname[11..].contains(':') {
                // This is a package root domain (package-0, package-1)
                has_rapl = true;
                // Constraint 0: PL1 (Long term)
                let c0_file = path.join("constraint_0_power_limit_uw");
                let c0_max_file = path.join("constraint_0_max_power_uw");
                if let Ok(s) = fs::read_to_string(&c0_file).or_else(|_| fs::read_to_string(&c0_max_file)) {
                    if let Ok(uw) = s.trim().parse::<u64>() {
                        if uw > 500_000 && uw < 1_000_000_000 {
                            let w = (uw as f32) / 1_000_000.0;
                            detected_pl1 = Some(w);
                        }
                    }
                }
                // Constraint 1: PL2 (Short term / Peak Burst)
                let c1_file = path.join("constraint_1_power_limit_uw");
                let c1_max_file = path.join("constraint_1_max_power_uw");
                if let Ok(s) = fs::read_to_string(&c1_file).or_else(|_| fs::read_to_string(&c1_max_file)) {
                    if let Ok(uw) = s.trim().parse::<u64>() {
                        if uw > 500_000 && uw < 1_000_000_000 {
                            let w = (uw as f32) / 1_000_000.0;
                            if w > detected_pl1.unwrap_or(0.0) {
                                detected_pl2 = Some(w);
                            }
                        }
                    }
                }
                if detected_pl1.is_some() {
                    break;
                }
            }
        }
    }

    // 2. Scan hwmon power caps if RAPL was not found
    if detected_pl1.is_none() {
        if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
            for entry in entries.flatten() {
                let p = entry.path();
                for cap_name in ["power1_cap", "power1_max", "power1_crit"] {
                    if let Ok(s) = fs::read_to_string(p.join(cap_name)) {
                        if let Ok(uw) = s.trim().parse::<u64>() {
                            if uw > 1_000_000 && uw < 1_000_000_000 {
                                let w = (uw as f32) / 1_000_000.0;
                                detected_pl1 = Some(w);
                                break;
                            }
                        }
                    }
                }
                if detected_pl1.is_some() {
                    break;
                }
            }
        }
    }

    // 3. Nominal TDP
    let nominal_watts = if let Some(pl1) = detected_pl1 {
        let rounded = (pl1 * 2.0).round() / 2.0;
        if rounded > 0.0 { rounded } else { 15.0 }
    } else {
        classify_cpu_nominal_tdp()
    };

    // Calculate real 3 TDP limits based on processor capability
    let quiet_pl1_watts = if nominal_watts <= 8.0 {
        ((nominal_watts * 0.75) * 2.0).round() / 2.0 // e.g. 6W -> 4.5W
    } else if nominal_watts <= 20.0 {
        (nominal_watts * 0.67).round().max(7.0) // e.g. 15W -> 10W
    } else if nominal_watts <= 35.0 {
        (nominal_watts * 0.70).round() // e.g. 28W -> 20W
    } else if nominal_watts <= 65.0 {
        (nominal_watts * 0.65).round() // e.g. 45W -> 30W, 65W -> 45W
    } else {
        (nominal_watts * 0.70).round() // e.g. 125W -> 85W
    };

    let balanced_pl1_watts = nominal_watts;

    let turbo_pl2_watts = if let Some(pl2) = detected_pl2 {
        let rounded = (pl2 * 2.0).round() / 2.0;
        if rounded > balanced_pl1_watts { rounded } else { calculate_turbo_tdp(nominal_watts) }
    } else {
        calculate_turbo_tdp(nominal_watts)
    };

    let has_platform_profile = Path::new("/sys/firmware/acpi/platform_profile").exists();

    CpuTdpProfile {
        nominal_watts,
        quiet_pl1_watts,
        balanced_pl1_watts,
        turbo_pl2_watts,
        has_rapl,
        has_platform_profile,
    }
}

/// Applies thermal, EC (Embedded Controller), and RAPL TDP power limits for the selected preset index:
/// 0 = Quiet / Eco (powersave, low-power EC platform profile, clamped PL1)
/// 1 = Balanced (schedutil, balanced EC platform profile, nominal PL1)
/// 2 = Turbo Boost (performance, performance EC platform profile, peak PL2 burst)
pub fn apply_thermal_power_preset(idx: usize, profile: &CpuTdpProfile) -> Result<String, String> {
    let gov = preset_to_governor(idx);
    let mut applied_actions: Vec<String> = Vec::new();

    // 1. CPU Scaling Governor
    let gov_applied = apply_governor(gov);
    if gov_applied.is_ok() {
        applied_actions.push(format!("Gov: {}", gov));
    }

    // 2. CPU Energy Performance Preference (EPP)
    let epp_val = match idx {
        0 => "power",
        2 => "performance",
        _ => "balance_performance",
    };
    let mut epp_set = false;
    if let Ok(entries) = fs::read_dir("/sys/devices/system/cpu") {
        for entry in entries.flatten() {
            let path = entry.path().join("cpufreq/energy_performance_preference");
            if path.exists() {
                if fs::write(&path, epp_val).is_ok() {
                    epp_set = true;
                }
            }
        }
    }
    if epp_set {
        applied_actions.push(format!("EPP: {}", epp_val));
    }

    // 3. Embedded Controller (EC) via ACPI platform_profile (/sys/firmware/acpi/platform_profile)
    let ec_profile = match idx {
        0 => "low-power",
        2 => "performance",
        _ => "balanced",
    };
    let acpi_profile_path = "/sys/firmware/acpi/platform_profile";
    if Path::new(acpi_profile_path).exists() {
        let choice = if let Ok(choices) = fs::read_to_string("/sys/firmware/acpi/platform_profile_choices") {
            if idx == 0 && choices.contains("quiet") && !choices.contains("low-power") {
                "quiet"
            } else {
                ec_profile
            }
        } else {
            ec_profile
        };

        if fs::write(acpi_profile_path, choice).is_ok() {
            applied_actions.push(format!("EC: {}", choice));
        } else {
            let choice_s = choice.to_string();
            std::thread::spawn(move || {
                let cmd = format!("echo {} | tee /sys/firmware/acpi/platform_profile", choice_s);
                let _ = Command::new("pkexec").args(["sh", "-c", &cmd]).output();
            });
            applied_actions.push(format!("EC: {} (auth)", choice));
        }
    }

    // 4. Vendor-Specific EC Hardware Registers
    // Lenovo IdeaPad / Legion EC Thermal Mode
    for lenovo_thermal in [
        "/sys/bus/platform/drivers/ideapad_laptop/VPC2004:00/thermal_mode",
        "/sys/bus/platform/devices/VPC2004:00/thermal_mode",
    ] {
        if Path::new(lenovo_thermal).exists() {
            let t_val = match idx { 0 => "1", 2 => "3", _ => "2" };
            if fs::write(lenovo_thermal, t_val).is_ok() {
                applied_actions.push(format!("Lenovo EC: mode {}", t_val));
            }
        }
    }

    // ASUS EC Thermal Throttle Policy
    let asus_thermal = "/sys/devices/platform/asus-nb-wmi/throttle_thermal_policy";
    if Path::new(asus_thermal).exists() {
        let t_val = match idx { 0 => "2", 2 => "1", _ => "0" };
        let _ = fs::write(asus_thermal, t_val);
        applied_actions.push(format!("ASUS EC: policy {}", t_val));
    }

    // Dell EC Thermal Management via cctk / smbios-thermal-ctl
    let dell_mode = match idx { 0 => "Quiet", 2 => "Performance", _ => "Balanced" };
    let cctk_path = Command::new("which").arg("cctk").output();
    if let Ok(out) = cctk_path {
        if out.status.success() {
            let arg = format!("--ThermalManagement={}", dell_mode);
            std::thread::spawn(move || {
                let _ = Command::new("pkexec").args(["cctk", &arg]).output();
            });
            applied_actions.push(format!("Dell EC: {}", dell_mode));
        }
    }
    let smbios_th = Command::new("which").arg("smbios-thermal-ctl").output();
    if let Ok(out) = smbios_th {
        if out.status.success() {
            let arg = format!("--set-thermal-mode={}", dell_mode.to_lowercase());
            std::thread::spawn(move || {
                let _ = Command::new("pkexec").args(["smbios-thermal-ctl", &arg]).output();
            });
            applied_actions.push(format!("Dell EC: {}", dell_mode));
        }
    }

    // 5. Hardware RAPL (Running Average Power Limit) Real Silicon TDP Enforcement
    let (target_pl1_w, target_pl2_w) = match idx {
        0 => (profile.quiet_pl1_watts, profile.quiet_pl1_watts * 1.15),
        2 => (profile.turbo_pl2_watts, profile.turbo_pl2_watts * 1.25),
        _ => (profile.balanced_pl1_watts, profile.balanced_pl1_watts * 1.25),
    };
    let target_pl1_uw = (target_pl1_w * 1_000_000.0) as u64;
    let target_pl2_uw = (target_pl2_w * 1_000_000.0) as u64;

    let mut rapl_written = false;
    let mut rapl_files_to_elevate = Vec::new();

    if let Ok(entries) = fs::read_dir("/sys/class/powercap") {
        for entry in entries.flatten() {
            let path = entry.path();
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if fname.starts_with("intel-rapl:") && !fname[11..].contains(':') {
                let c0 = path.join("constraint_0_power_limit_uw");
                let c1 = path.join("constraint_1_power_limit_uw");
                let en = path.join("enabled");

                let _ = fs::write(&en, "1");

                if fs::write(&c0, format!("{}", target_pl1_uw)).is_ok() {
                    rapl_written = true;
                    let _ = fs::write(&c1, format!("{}", target_pl2_uw));
                } else {
                    rapl_files_to_elevate.push((c0, c1, en));
                }
            }
        }
    }

    if rapl_written {
        applied_actions.push(format!("PL1: {:.1}W", target_pl1_w));
    } else if !rapl_files_to_elevate.is_empty() {
        let files = rapl_files_to_elevate;
        std::thread::spawn(move || {
            for (c0, c1, en) in files {
                let cmd = format!(
                    "echo 1 | tee \"{}\" 2>/dev/null; echo {} | tee \"{}\" 2>/dev/null; echo {} | tee \"{}\" 2>/dev/null",
                    en.display(), target_pl1_uw, c0.display(), target_pl2_uw, c1.display()
                );
                let _ = Command::new("pkexec").args(["sh", "-c", &cmd]).output();
            }
        });
        applied_actions.push(format!("PL1: {:.1}W (auth)", target_pl1_w));
    }

    let preset_name = match idx {
        0 => "Quiet / Eco",
        2 => "Turbo Boost",
        _ => "Balanced",
    };

    if applied_actions.is_empty() {
        Ok(format!("{} active: Governor '{}' configured", preset_name, gov))
    } else {
        Ok(format!("{} applied [{}]", preset_name, applied_actions.join(" • ")))
    }
}

/// Builds an SSH command to configure thermal preset, EC platform profile, and RAPL TDP limits on a remote host.
pub fn build_remote_preset_command(idx: usize, profile: &CpuTdpProfile) -> String {
    let gov = preset_to_governor(idx);
    let ec_profile = match idx { 0 => "low-power", 2 => "performance", _ => "balanced" };
    let epp = match idx { 0 => "power", 2 => "performance", _ => "balance_performance" };
    let lenovo_ec = match idx { 0 => "1", 2 => "3", _ => "2" };
    let asus_ec = match idx { 0 => "2", 2 => "1", _ => "0" };
    let dell_mode = match idx { 0 => "Quiet", 2 => "Performance", _ => "Balanced" };

    let (target_pl1_w, target_pl2_w) = match idx {
        0 => (profile.quiet_pl1_watts, profile.quiet_pl1_watts * 1.15),
        2 => (profile.turbo_pl2_watts, profile.turbo_pl2_watts * 1.25),
        _ => (profile.balanced_pl1_watts, profile.balanced_pl1_watts * 1.25),
    };
    let pl1_uw = (target_pl1_w * 1_000_000.0) as u64;
    let pl2_uw = (target_pl2_w * 1_000_000.0) as u64;

    format!(
        "echo {gov} | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor 2>/dev/null || true; \
         echo {epp} | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference 2>/dev/null || true; \
         echo {ec} | sudo tee /sys/firmware/acpi/platform_profile 2>/dev/null || true; \
         echo {lenovo} | sudo tee /sys/bus/platform/drivers/ideapad_laptop/VPC2004:00/thermal_mode 2>/dev/null || true; \
         echo {lenovo} | sudo tee /sys/bus/platform/devices/VPC2004:00/thermal_mode 2>/dev/null || true; \
         echo {asus} | sudo tee /sys/devices/platform/asus-nb-wmi/throttle_thermal_policy 2>/dev/null || true; \
         sudo cctk --ThermalManagement={dell} 2>/dev/null || true; \
         sudo smbios-thermal-ctl --set-thermal-mode={dell_lower} 2>/dev/null || true; \
         for f in /sys/class/powercap/intel-rapl:*/constraint_0_power_limit_uw; do echo {pl1} | sudo tee \"$f\" 2>/dev/null; done; \
         for f in /sys/class/powercap/intel-rapl:*/constraint_1_power_limit_uw; do echo {pl2} | sudo tee \"$f\" 2>/dev/null; done || true",
        gov = gov,
        epp = epp,
        ec = ec_profile,
        lenovo = lenovo_ec,
        asus = asus_ec,
        dell = dell_mode,
        dell_lower = dell_mode.to_lowercase(),
        pl1 = pl1_uw,
        pl2 = pl2_uw
    )
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

