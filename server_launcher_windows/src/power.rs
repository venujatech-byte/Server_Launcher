//! Power telemetry and battery charging controls across PC vendors.

use std::fs;
use std::path::Path;

/// Power profile inputs for the illustrative system power estimate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuTdpProfile {
    /// Reported package power cap or estimated nominal CPU power (Watts).
    pub nominal_watts: f32,
    /// Estimated quiet-mode CPU power (Watts).
    pub quiet_pl1_watts: f32,
    /// Estimated nominal CPU power (Watts).
    pub balanced_pl1_watts: f32,
    /// Estimated peak CPU power (Watts).
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

/// Heuristic CPU classification based on processor model string and core count.
pub fn classify_nominal_tdp_from_model_and_cores(model: &str, cores: usize) -> f32 {
    let model_line = model.to_lowercase();
    let core_count = cores.max(1);

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

/// Fallback CPU classification heuristic based on /proc/cpuinfo model name and core count.
fn classify_cpu_nominal_tdp() -> f32 {
    let cpu_info = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let model_line = cpu_info
        .lines()
        .find(|l| l.starts_with("model name"))
        .unwrap_or("");

    let core_count = cpu_info
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count()
        .max(1);

    classify_nominal_tdp_from_model_and_cores(model_line, core_count)
}

pub fn calculate_turbo_tdp(nominal: f32) -> f32 {
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

/// Builds inputs for the illustrative power estimate from a reported cap or CPU model heuristic.
pub fn tdp_profile_from_nominal_or_cpu(nominal_opt: Option<f32>, model_name: &str, core_count: usize) -> CpuTdpProfile {
    let nominal_watts = if let Some(n) = nominal_opt {
        let rounded = (n * 2.0).round() / 2.0;
        if rounded > 0.0 { rounded } else { 15.0 }
    } else {
        classify_nominal_tdp_from_model_and_cores(model_name, core_count)
    };

    let quiet_pl1_watts = if nominal_watts <= 8.0 {
        ((nominal_watts * 0.75) * 2.0).round() / 2.0
    } else if nominal_watts <= 20.0 {
        (nominal_watts * 0.67).round().max(7.0)
    } else if nominal_watts <= 35.0 {
        (nominal_watts * 0.70).round()
    } else if nominal_watts <= 65.0 {
        (nominal_watts * 0.65).round()
    } else {
        (nominal_watts * 0.70).round()
    };

    let balanced_pl1_watts = nominal_watts;
    let turbo_pl2_watts = calculate_turbo_tdp(nominal_watts);

    CpuTdpProfile {
        nominal_watts,
        quiet_pl1_watts,
        balanced_pl1_watts,
        turbo_pl2_watts,
        has_rapl: nominal_opt.is_some(),
        has_platform_profile: false,
    }
}

/// Reads Linux package power caps when available; otherwise estimates nominal power from CPU model.
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

    // The preset values feed the estimate only; this app does not apply power limits.
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

/// Set the kernel-reported charge limit for one explicitly selected battery.
pub fn set_charge_limit(battery: &str, limit: u32, host: Option<&crate::config::SshRemoteHost>, password: Option<&str>) -> Result<String, String> {
    if host.is_none() && !cfg!(target_os = "linux") { return Err("Local charge-limit control is available only on Linux".into()); }
    if battery.is_empty() || !battery.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-') || !(1..=100).contains(&limit) {
        return Err("Invalid battery or charge limit".into());
    }
    let script = format!(r#"set -eu
p='/sys/class/power_supply/{battery}'
[ "$(cat "$p/type")" = Battery ] || {{ echo 'Battery is no longer available' >&2; exit 1; }}
f="$p/charge_control_end_threshold"
[ -f "$f" ] || {{ echo 'Charge limit is unsupported by this battery' >&2; exit 1; }}
printf '%s' '{limit}' > "$f"
[ "$(cat "$f")" = '{limit}' ] || {{ echo 'Charge limit read-back did not match' >&2; exit 1; }}"#);
    if let Some(host) = host {
        crate::systemd::remote_script(host, &script, false, true, password)?;
    } else {
        #[cfg(not(windows))]
        crate::systemd::run_local("sh", &["-c", &script], true, password)?;
        #[cfg(windows)]
        return Err("Local charge-limit control is available only on Linux".into());
    }
    Ok(format!("{} charge limit set to {}%", battery, limit))
}

#[cfg(test)]
mod charge_limit_tests {
    #[test]
    fn rejects_unsafe_battery_names_and_limits() {
        assert!(super::set_charge_limit("../BAT0", 80, None, None).is_err());
        assert!(super::set_charge_limit("BAT0", 101, None, None).is_err());
    }
}
