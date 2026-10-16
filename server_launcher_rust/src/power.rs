//! Power subsystem: CPU scaling governor query/control and thermal power presets.

use std::fs;
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
