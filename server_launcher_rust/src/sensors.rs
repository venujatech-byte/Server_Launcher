use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use sysinfo::{Components, Disks, Networks, System};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SensorCategory {
    Cpu,
    Memory,
    Gpu,
    Storage,
    #[default]
    System,
}

impl SensorCategory {
    pub fn label(&self) -> &'static str {
        match self {
            SensorCategory::Cpu => "CPU",
            SensorCategory::Memory => "Memory (RAM)",
            SensorCategory::Gpu => "GPU",
            SensorCategory::Storage => "Storage / NVMe",
            SensorCategory::System => "System / ACPI",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            SensorCategory::Cpu => "🖥",
            SensorCategory::Memory => "🧠",
            SensorCategory::Gpu => "🎮",
            SensorCategory::Storage => "💾",
            SensorCategory::System => "🌡",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TempSensor {
    pub name: String,
    pub chip: String,
    pub temp: f32,
    pub max: Option<f32>,
    pub crit: Option<f32>,
    pub category: SensorCategory,
}

#[derive(Debug, Clone, Default)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub fs_type: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub usage_pct: f32,
}

#[derive(Debug, Clone, Default)]
pub struct NetInterfaceInfo {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_rate_bps: f64,
    pub tx_rate_bps: f64,
}

#[derive(Debug, Clone, Default)]
pub struct GpuSensorInfo {
    pub name: String,
    pub load_pct: Option<f32>,
    pub temp: Option<f32>,
    pub mem_used_mb: Option<u64>,
    pub mem_total_mb: Option<u64>,
    pub clock_mhz: Option<u32>,
    pub mem_clock_mhz: Option<u32>,
    pub power_watts: Option<f32>,
    pub power_limit_watts: Option<f32>,
    pub driver: String,
}

#[derive(Debug, Clone, Default)]
pub struct BatteryInfo {
    pub name: String,
    pub present: bool,
    pub status: String,
    pub capacity_pct: f32,
    pub power_watts: Option<f32>,
    pub voltage_volts: Option<f32>,
    pub energy_wh: Option<f32>,
    pub energy_full_wh: Option<f32>,
    pub energy_design_wh: Option<f32>,
    pub health_pct: Option<f32>,
    pub cycle_count: Option<u32>,
    pub technology: String,
    pub model_name: String,
    pub manufacturer: String,
}

#[derive(Debug, Clone, Default)]
pub struct PowerSupplyInfo {
    pub ac_online: Option<bool>,
    pub batteries: Vec<BatteryInfo>,
    pub total_battery_pct: Option<f32>,
    pub total_power_watts: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct SystemSensorsSnapshot {
    // CPU
    pub cpu_brand: String,
    pub cpu_cores_physical: usize,
    pub cpu_cores_logical: usize,
    pub cpu_global_usage: f32,
    pub cpu_core_usages: Vec<f32>,
    pub cpu_frequency_mhz: u64,
    pub cpu_frequencies_mhz: Vec<u64>,
    pub cpu_power_watts: Option<f32>,
    pub cpu_tdp_watts: Option<f32>,
    pub cpu_temp: Option<f32>,

    // Memory (RAM & Swap)
    pub mem_total_bytes: u64,
    pub mem_used_bytes: u64,
    pub mem_free_bytes: u64,
    pub mem_usage_pct: f32,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_usage_pct: f32,
    pub ram_temp: Option<f32>,

    // GPU
    pub gpus: Vec<GpuSensorInfo>,

    // Power & Battery
    pub power: PowerSupplyInfo,

    // Storage
    pub disks: Vec<DiskInfo>,
    pub storage_temp: Option<f32>,

    // Network
    pub networks: Vec<NetInterfaceInfo>,
    pub total_rx_rate_bps: f64,
    pub total_tx_rate_bps: f64,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,

    // All temperature sensors
    pub temp_sensors: Vec<TempSensor>,

    pub timestamp: Option<Instant>,
}

pub struct SystemSensorsCollector {
    sys: System,
    disks: Disks,
    networks: Networks,
    components: Components,
    prev_net_rx: HashMap<String, u64>,
    prev_net_tx: HashMap<String, u64>,
    prev_net_time: Instant,
}

impl Default for SystemSensorsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemSensorsCollector {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        let disks = Disks::new_with_refreshed_list();
        let networks = Networks::new_with_refreshed_list();
        let components = Components::new_with_refreshed_list();

        Self {
            sys,
            disks,
            networks,
            components,
            prev_net_rx: HashMap::new(),
            prev_net_tx: HashMap::new(),
            prev_net_time: Instant::now(),
        }
    }

    pub fn collect(&mut self) -> SystemSensorsSnapshot {
        // 1. Refresh System
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.disks.refresh();
        self.networks.refresh();
        self.components.refresh();

        let now = Instant::now();
        let elapsed_secs = (now - self.prev_net_time).as_secs_f64().max(0.1);

        // 2. CPU
        let cpu_brand = self
            .sys
            .cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_else(|| "Unknown CPU".to_string());
        let cpu_cores_physical = self.sys.physical_core_count().unwrap_or(1);
        let cpu_cores_logical = self.sys.cpus().len();
        let cpu_global_usage = self.sys.global_cpu_usage();
        let cpu_core_usages: Vec<f32> = self.sys.cpus().iter().map(|c| c.cpu_usage()).collect();
        let (mut cpu_frequencies_mhz, cpu_power_watts, cpu_tdp_watts) = collect_cpu_frequencies_and_power();
        if cpu_frequencies_mhz.is_empty() {
            cpu_frequencies_mhz = self.sys.cpus().iter().map(|c| c.frequency()).collect();
        }
        let cpu_frequency_mhz = if !cpu_frequencies_mhz.is_empty() {
            (cpu_frequencies_mhz.iter().sum::<u64>() / cpu_frequencies_mhz.len() as u64).max(1)
        } else {
            self.sys.cpus().first().map(|c| c.frequency()).unwrap_or(0)
        };

        // 3. Memory
        let mem_total_bytes = self.sys.total_memory();
        let mem_used_bytes = self.sys.used_memory();
        let mem_free_bytes = self.sys.free_memory();
        let mem_usage_pct = if mem_total_bytes > 0 {
            (mem_used_bytes as f32 / mem_total_bytes as f32) * 100.0
        } else {
            0.0
        };

        let swap_total_bytes = self.sys.total_swap();
        let swap_used_bytes = self.sys.used_swap();
        let swap_usage_pct = if swap_total_bytes > 0 {
            (swap_used_bytes as f32 / swap_total_bytes as f32) * 100.0
        } else {
            0.0
        };

        // 4. Storage / Disks
        let mut disks = Vec::new();
        for d in &self.disks {
            let total = d.total_space();
            let avail = d.available_space();
            let used = total.saturating_sub(avail);
            let pct = if total > 0 {
                (used as f32 / total as f32) * 100.0
            } else {
                0.0
            };
            disks.push(DiskInfo {
                name: d.name().to_string_lossy().to_string(),
                mount_point: d.mount_point().to_string_lossy().to_string(),
                fs_type: d.file_system().to_string_lossy().to_string(),
                total_bytes: total,
                available_bytes: avail,
                used_bytes: used,
                usage_pct: pct,
            });
        }
        // Deduplicate or sort by mount point
        disks.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));

        // 5. Networks
        let mut networks = Vec::new();
        let mut total_rx_rate_bps = 0.0;
        let mut total_tx_rate_bps = 0.0;
        let mut total_rx_bytes = 0;
        let mut total_tx_bytes = 0;

        for (name, net) in &self.networks {
            let rx = net.total_received();
            let tx = net.total_transmitted();
            total_rx_bytes += rx;
            total_tx_bytes += tx;

            let prev_rx = *self.prev_net_rx.get(name).unwrap_or(&rx);
            let prev_tx = *self.prev_net_tx.get(name).unwrap_or(&tx);

            let rx_rate = (rx.saturating_sub(prev_rx) as f64) / elapsed_secs;
            let tx_rate = (tx.saturating_sub(prev_tx) as f64) / elapsed_secs;

            // Only track non-loopback or active interfaces in summary rate
            if !name.starts_with("lo") {
                total_rx_rate_bps += rx_rate;
                total_tx_rate_bps += tx_rate;
            }

            self.prev_net_rx.insert(name.clone(), rx);
            self.prev_net_tx.insert(name.clone(), tx);

            networks.push(NetInterfaceInfo {
                name: name.clone(),
                rx_bytes: rx,
                tx_bytes: tx,
                rx_rate_bps: rx_rate,
                tx_rate_bps: tx_rate,
            });
        }
        networks.sort_by(|a, b| a.name.cmp(&b.name));
        self.prev_net_time = now;

        // 6. Temperature Sensors (sysinfo + hwmon / thermal fallback)
        let mut temp_sensors = collect_hwmon_sensors();

        // If hwmon didn't find anything, fallback to sysinfo components
        if temp_sensors.is_empty() {
            for c in &self.components {
                let label = c.label().to_string();
                let cat = categorize_sensor(&label, "sysinfo");
                temp_sensors.push(TempSensor {
                    name: label,
                    chip: "sysinfo".to_string(),
                    temp: c.temperature(),
                    max: Some(c.max()),
                    crit: c.critical(),
                    category: cat,
                });
            }
        }

        // Identify primary CPU, RAM, Storage, GPU temperatures
        let mut cpu_temp = None;
        let mut ram_temp = None;
        let mut storage_temp = None;
        let mut gpu_temp = None;

        for s in &temp_sensors {
            match s.category {
                SensorCategory::Cpu if cpu_temp.is_none() || s.name.contains("Package") || s.name == "CPU" => {
                    cpu_temp = Some(s.temp);
                }
                SensorCategory::Memory if ram_temp.is_none() || s.name.contains("SODIMM") => {
                    ram_temp = Some(s.temp);
                }
                SensorCategory::Storage if storage_temp.is_none() || s.name.contains("Composite") => {
                    storage_temp = Some(s.temp);
                }
                SensorCategory::Gpu if gpu_temp.is_none() => {
                    gpu_temp = Some(s.temp);
                }
                _ => {}
            }
        }

        // 7. GPU detection (NVIDIA / Intel / AMD)
        let gpus = detect_gpus(gpu_temp);

        // 8. Power Supply & Battery
        let power = collect_power_supply();

        SystemSensorsSnapshot {
            cpu_brand,
            cpu_cores_physical,
            cpu_cores_logical,
            cpu_global_usage,
            cpu_core_usages,
            cpu_frequency_mhz,
            cpu_frequencies_mhz,
            cpu_power_watts,
            cpu_tdp_watts,
            cpu_temp,
            mem_total_bytes,
            mem_used_bytes,
            mem_free_bytes,
            mem_usage_pct,
            swap_total_bytes,
            swap_used_bytes,
            swap_usage_pct,
            ram_temp,
            gpus,
            power,
            disks,
            storage_temp,
            networks,
            total_rx_rate_bps,
            total_tx_rate_bps,
            total_rx_bytes,
            total_tx_bytes,
            temp_sensors,
            timestamp: Some(now),
        }
    }
}

fn categorize_sensor(label: &str, chip: &str) -> SensorCategory {
    let lower_label = label.to_lowercase();
    let lower_chip = chip.to_lowercase();

    if lower_label.contains("cpu")
        || lower_label.contains("core")
        || lower_label.contains("package")
        || lower_label.contains("tcpu")
        || lower_label.contains("x86_pkg")
        || lower_chip.contains("coretemp")
        || lower_chip.contains("k10temp")
        || lower_chip.contains("zenpower")
    {
        SensorCategory::Cpu
    } else if lower_label.contains("sodimm")
        || lower_label.contains("dimm")
        || lower_label.contains("dram")
        || lower_label.contains("memory")
        || lower_label.contains("ram")
    {
        SensorCategory::Memory
    } else if lower_label.contains("gpu")
        || lower_chip.contains("amdgpu")
        || lower_chip.contains("nouveau")
        || lower_chip.contains("nvidia")
        || lower_chip.contains("radeon")
    {
        SensorCategory::Gpu
    } else if lower_label.contains("nvme")
        || lower_label.contains("composite")
        || lower_label.contains("drive")
        || lower_label.contains("disk")
        || lower_label.contains("ssd")
        || lower_label.contains("hdd")
        || lower_chip.contains("nvme")
        || lower_chip.contains("drivetemp")
    {
        SensorCategory::Storage
    } else {
        SensorCategory::System
    }
}

pub fn collect_hwmon_sensors() -> Vec<TempSensor> {
    let mut result = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let hwmon_dir = Path::new("/sys/class/hwmon");
        if hwmon_dir.exists() {
            if let Ok(entries) = fs::read_dir(hwmon_dir) {
                let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
                paths.sort();

                for p in paths {
                    let chip = fs::read_to_string(p.join("name"))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|_| p.file_name().unwrap_or_default().to_string_lossy().to_string());

                    if let Ok(files) = fs::read_dir(&p) {
                        let mut temp_files: Vec<_> = files.flatten().map(|e| e.path()).collect();
                        temp_files.sort();

                        for tf in temp_files {
                            let fname = tf.file_name().unwrap_or_default().to_string_lossy();
                            if fname.starts_with("temp") && fname.ends_with("_input") {
                                let prefix = fname.trim_end_matches("_input");
                                let label_path = p.join(format!("{}_label", prefix));
                                let label = fs::read_to_string(&label_path)
                                    .map(|s| s.trim().to_string())
                                    .unwrap_or_else(|_| prefix.to_string());

                                let crit_path = p.join(format!("{}_crit", prefix));
                                let crit = fs::read_to_string(&crit_path)
                                    .ok()
                                    .and_then(|s| s.trim().parse::<f32>().ok())
                                    .map(|v| v / 1000.0);

                                let max_path = p.join(format!("{}_max", prefix));
                                let max = fs::read_to_string(&max_path)
                                    .ok()
                                    .and_then(|s| s.trim().parse::<f32>().ok())
                                    .map(|v| v / 1000.0);

                                if let Ok(val_str) = fs::read_to_string(&tf) {
                                    if let Ok(raw_val) = val_str.trim().parse::<f32>() {
                                        let temp = raw_val / 1000.0;
                                        // Ignore absurd disconnected sensor readings like -273 or 65261
                                        if temp >= -40.0 && temp <= 150.0 {
                                            let cat = categorize_sensor(&label, &chip);
                                            result.push(TempSensor {
                                                name: label,
                                                chip: chip.clone(),
                                                temp,
                                                max,
                                                crit,
                                                category: cat,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Also check /sys/class/thermal if hwmon had few sensors
        if result.len() < 3 {
            let thermal_dir = Path::new("/sys/class/thermal");
            if thermal_dir.exists() {
                if let Ok(entries) = fs::read_dir(thermal_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        if name.starts_with("thermal_zone") {
                            let type_str = fs::read_to_string(path.join("type"))
                                .map(|s| s.trim().to_string())
                                .unwrap_or_else(|_| name.to_string());
                            if let Ok(temp_str) = fs::read_to_string(path.join("temp")) {
                                if let Ok(raw) = temp_str.trim().parse::<f32>() {
                                    let temp = raw / 1000.0;
                                    if temp >= -40.0 && temp <= 150.0 {
                                        let cat = categorize_sensor(&type_str, "thermal_zone");
                                        result.push(TempSensor {
                                            name: type_str,
                                            chip: "thermal_zone".to_string(),
                                            temp,
                                            max: None,
                                            crit: None,
                                            category: cat,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    result
}

pub fn collect_cpu_frequencies_and_power() -> (Vec<u64>, Option<f32>, Option<f32>) {
    let mut freqs = Vec::new();
    let mut cpu_power_watts = None;
    let mut cpu_tdp_watts = None;

    #[cfg(target_os = "linux")]
    {
        // 1. Read per-core frequencies from cpufreq
        let mut core_idx = 0;
        loop {
            let p = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", core_idx);
            let path = Path::new(&p);
            if !path.exists() {
                break;
            }
            if let Ok(s) = fs::read_to_string(path) {
                if let Ok(khz) = s.trim().parse::<u64>() {
                    freqs.push(khz / 1000);
                }
            }
            core_idx += 1;
        }

        // 2. Read CPU TDP limit from intel-rapl constraint
        let rapl_candidates = [
            "/sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw",
            "/sys/class/powercap/intel-rapl/intel-rapl:0/constraint_0_power_limit_uw",
        ];
        for cand in &rapl_candidates {
            if let Ok(s) = fs::read_to_string(cand) {
                if let Ok(uw) = s.trim().parse::<f32>() {
                    let w = uw / 1_000_000.0;
                    if w > 0.0 {
                        cpu_tdp_watts = Some(w);
                        break;
                    }
                }
            }
        }

        // 3. Read power draw from hwmon if labeled CPU / coretemp / k10temp
        let hwmon_dir = Path::new("/sys/class/hwmon");
        if hwmon_dir.exists() {
            if let Ok(entries) = fs::read_dir(hwmon_dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    let name = fs::read_to_string(p.join("name")).unwrap_or_default().to_lowercase();
                    if name.contains("coretemp") || name.contains("k10temp") || name.contains("zenpower") || name.contains("cpu") {
                        if let Ok(files) = fs::read_dir(&p) {
                            for f in files.flatten() {
                                let fname = f.file_name().to_string_lossy().to_string();
                                if fname.starts_with("power") && fname.ends_with("_input") {
                                    if let Ok(s) = fs::read_to_string(f.path()) {
                                        if let Ok(uw) = s.trim().parse::<f32>() {
                                            let w = uw / 1_000_000.0;
                                            if w > 0.0 && w < 1000.0 {
                                                cpu_power_watts = Some(w);
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if cpu_power_watts.is_some() {
                        break;
                    }
                }
            }
        }
    }

    (freqs, cpu_power_watts, cpu_tdp_watts)
}

pub fn collect_power_supply() -> PowerSupplyInfo {
    let mut ac_online = None;
    let mut batteries = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let ps_dir = Path::new("/sys/class/power_supply");
        if ps_dir.exists() {
            if let Ok(entries) = fs::read_dir(ps_dir) {
                let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
                paths.sort();

                for p in paths {
                    let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let type_str = fs::read_to_string(p.join("type")).unwrap_or_default().trim().to_lowercase();

                    if type_str == "mains" || type_str == "ac" || type_str == "adp" || name.to_lowercase().starts_with("ac") {
                        if let Ok(on_str) = fs::read_to_string(p.join("online")) {
                            if let Ok(val) = on_str.trim().parse::<i32>() {
                                ac_online = Some(val == 1);
                            }
                        }
                    } else if type_str == "battery" || name.to_lowercase().starts_with("bat") {
                        let present = fs::read_to_string(p.join("present"))
                            .map(|s| s.trim() == "1")
                            .unwrap_or(true);
                        if !present {
                            continue;
                        }

                        let status = fs::read_to_string(p.join("status"))
                            .unwrap_or_else(|_| "Unknown".to_string())
                            .trim()
                            .to_string();

                        let capacity_pct = fs::read_to_string(p.join("capacity"))
                            .ok()
                            .and_then(|s| s.trim().parse::<f32>().ok())
                            .unwrap_or(0.0);

                        let voltage_volts = fs::read_to_string(p.join("voltage_now"))
                            .ok()
                            .and_then(|s| s.trim().parse::<f32>().ok())
                            .map(|uv| uv / 1_000_000.0);

                        let mut power_watts = fs::read_to_string(p.join("power_now"))
                            .ok()
                            .and_then(|s| s.trim().parse::<f32>().ok())
                            .map(|uw| uw / 1_000_000.0);

                        if power_watts.is_none() {
                            if let (Some(v), Ok(c_str)) = (voltage_volts, fs::read_to_string(p.join("current_now"))) {
                                if let Ok(ua) = c_str.trim().parse::<f32>() {
                                    power_watts = Some((ua / 1_000_000.0) * v);
                                }
                            }
                        }

                        let mut energy_wh = fs::read_to_string(p.join("energy_now"))
                            .ok()
                            .and_then(|s| s.trim().parse::<f32>().ok())
                            .map(|uwh| uwh / 1_000_000.0);

                        if energy_wh.is_none() {
                            if let (Some(v), Ok(c_str)) = (voltage_volts, fs::read_to_string(p.join("charge_now"))) {
                                if let Ok(uah) = c_str.trim().parse::<f32>() {
                                    energy_wh = Some((uah / 1_000_000.0) * v);
                                }
                            }
                        }

                        let mut energy_full_wh = fs::read_to_string(p.join("energy_full"))
                            .ok()
                            .and_then(|s| s.trim().parse::<f32>().ok())
                            .map(|uwh| uwh / 1_000_000.0);

                        if energy_full_wh.is_none() {
                            if let (Some(v), Ok(c_str)) = (voltage_volts, fs::read_to_string(p.join("charge_full"))) {
                                if let Ok(uah) = c_str.trim().parse::<f32>() {
                                    energy_full_wh = Some((uah / 1_000_000.0) * v);
                                }
                            }
                        }

                        let mut energy_design_wh = fs::read_to_string(p.join("energy_full_design"))
                            .ok()
                            .and_then(|s| s.trim().parse::<f32>().ok())
                            .map(|uwh| uwh / 1_000_000.0);

                        if energy_design_wh.is_none() {
                            if let (Some(v), Ok(c_str)) = (voltage_volts, fs::read_to_string(p.join("charge_full_design"))) {
                                if let Ok(uah) = c_str.trim().parse::<f32>() {
                                    energy_design_wh = Some((uah / 1_000_000.0) * v);
                                }
                            }
                        }

                        let mut health_pct = if let (Some(full), Some(design)) = (energy_full_wh, energy_design_wh) {
                            if design > 0.0 {
                                Some(((full / design) * 100.0).clamp(0.0, 100.0))
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        if health_pct.is_none() {
                            if let (Ok(cf), Ok(cd)) = (
                                fs::read_to_string(p.join("charge_full")),
                                fs::read_to_string(p.join("charge_full_design")),
                            ) {
                                if let (Ok(cf_val), Ok(cd_val)) = (cf.trim().parse::<f32>(), cd.trim().parse::<f32>()) {
                                    if cd_val > 0.0 {
                                        health_pct = Some(((cf_val / cd_val) * 100.0).clamp(0.0, 100.0));
                                    }
                                }
                            }
                        }

                        let cycle_count = fs::read_to_string(p.join("cycle_count"))
                            .ok()
                            .and_then(|s| s.trim().parse::<u32>().ok())
                            .filter(|&c| c > 0);

                        let technology = fs::read_to_string(p.join("technology"))
                            .unwrap_or_default()
                            .trim()
                            .to_string();

                        let model_name = fs::read_to_string(p.join("model_name"))
                            .unwrap_or_default()
                            .trim()
                            .to_string();

                        let manufacturer = fs::read_to_string(p.join("manufacturer"))
                            .unwrap_or_default()
                            .trim()
                            .to_string();

                        batteries.push(BatteryInfo {
                            name,
                            present,
                            status,
                            capacity_pct,
                            power_watts,
                            voltage_volts,
                            energy_wh,
                            energy_full_wh,
                            energy_design_wh,
                            health_pct,
                            cycle_count,
                            technology,
                            model_name,
                            manufacturer,
                        });
                    }
                }
            }
        }
    }

    let total_battery_pct = if !batteries.is_empty() {
        let sum: f32 = batteries.iter().map(|b| b.capacity_pct).sum();
        Some(sum / batteries.len() as f32)
    } else {
        None
    };

    let total_power_watts = {
        let sum: f32 = batteries.iter().filter_map(|b| b.power_watts).sum();
        if sum > 0.0 {
            Some(sum)
        } else {
            None
        }
    };

    PowerSupplyInfo {
        ac_online,
        batteries,
        total_battery_pct,
        total_power_watts,
    }
}

fn detect_gpus(known_temp: Option<f32>) -> Vec<GpuSensorInfo> {
    let mut gpus = Vec::new();

    // 1. Check NVIDIA via nvidia-smi
    let nvidia_out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,temperature.gpu,utilization.gpu,utilization.memory,memory.used,memory.total,clocks.current.graphics,clocks.current.memory,power.draw,power.limit",
            "--format=csv,noheader,nounits"
        ])
        .output();

    if let Ok(out) = nvidia_out {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                if parts.len() >= 6 {
                    let name = parts[0].to_string();
                    let temp = parts[1].parse::<f32>().ok();
                    let load_pct = parts[2].parse::<f32>().ok();
                    let mem_used = parts[4].parse::<u64>().ok();
                    let mem_total = parts[5].parse::<u64>().ok();
                    let clock_mhz = parts.get(6).and_then(|s| s.parse::<u32>().ok());
                    let mem_clock_mhz = parts.get(7).and_then(|s| s.parse::<u32>().ok());
                    let power_watts = parts.get(8).and_then(|s| s.parse::<f32>().ok());
                    let power_limit_watts = parts.get(9).and_then(|s| s.parse::<f32>().ok());

                    gpus.push(GpuSensorInfo {
                        name,
                        load_pct,
                        temp,
                        mem_used_mb: mem_used,
                        mem_total_mb: mem_total,
                        clock_mhz,
                        mem_clock_mhz,
                        power_watts,
                        power_limit_watts,
                        driver: "NVIDIA Proprietary".to_string(),
                    });
                }
            }
        }
    }

    // 2. Check Linux DRM cards if no NVIDIA or integrated GPU
    #[cfg(target_os = "linux")]
    if gpus.is_empty() {
        let drm_dir = Path::new("/sys/class/drm");
        if drm_dir.exists() {
            if let Ok(entries) = fs::read_dir(drm_dir) {
                for e in entries.flatten() {
                    let fname = e.file_name().to_string_lossy().to_string();
                    if fname.starts_with("card") && !fname.contains('-') {
                        let dev = e.path().join("device");
                        let vendor = fs::read_to_string(dev.join("vendor")).unwrap_or_default().trim().to_string();
                        let vendor_name = match vendor.as_str() {
                            "0x8086" => "Intel Integrated Graphics",
                            "0x1002" => "AMD Radeon Graphics",
                            "0x10de" => "NVIDIA Corporation",
                            _ => "Integrated GPU",
                        };

                        // Check frequency
                        let freq_mhz = fs::read_to_string(e.path().join("gt_act_freq_mhz"))
                            .or_else(|_| fs::read_to_string(e.path().join("gt_cur_freq_mhz")))
                            .ok()
                            .and_then(|s| s.trim().parse::<f32>().ok());

                        let max_freq_mhz = fs::read_to_string(e.path().join("gt_max_freq_mhz"))
                            .ok()
                            .and_then(|s| s.trim().parse::<f32>().ok())
                            .unwrap_or(1200.0);

                        let load_pct = freq_mhz.map(|f| ((f / max_freq_mhz) * 100.0).clamp(0.0, 100.0));

                        let mut power_watts = None;
                        let hwmon_p = dev.join("hwmon");
                        if hwmon_p.exists() {
                            if let Ok(entries) = fs::read_dir(&hwmon_p) {
                                for hwe in entries.flatten() {
                                    if let Ok(s) = fs::read_to_string(hwe.path().join("power1_average"))
                                        .or_else(|_| fs::read_to_string(hwe.path().join("power1_input")))
                                    {
                                        if let Ok(uw) = s.trim().parse::<f32>() {
                                            power_watts = Some(uw / 1_000_000.0);
                                            break;
                                        }
                                    }
                                }
                            }
                        }

                        gpus.push(GpuSensorInfo {
                            name: vendor_name.to_string(),
                            load_pct,
                            temp: known_temp,
                            mem_used_mb: None,
                            mem_total_mb: None,
                            clock_mhz: freq_mhz.map(|f| f as u32),
                            mem_clock_mhz: None,
                            power_watts,
                            power_limit_watts: None,
                            driver: "Direct Rendering Manager (DRM)".to_string(),
                        });
                        break;
                    }
                }
            }
        }
    }

    gpus
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    const TB: u64 = 1024 * GB;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

pub fn format_rate(bps: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;

    if bps >= GB {
        format!("{:.2} GB/s", bps / GB)
    } else if bps >= MB {
        format!("{:.1} MB/s", bps / MB)
    } else if bps >= KB {
        format!("{:.1} KB/s", bps / KB)
    } else {
        format!("{:.0} B/s", bps)
    }
}

use crate::config::SshRemoteHost;
use crate::remote::build_ssh_command;
use egui::Color32;

pub fn temp_color(temp: f32) -> Color32 {
    if temp < 55.0 {
        Color32::from_rgb(16, 185, 129) // Emerald green
    } else if temp < 75.0 {
        Color32::from_rgb(245, 158, 11)  // Amber / yellow
    } else {
        Color32::from_rgb(239, 68, 68)   // Red
    }
}

pub fn query_remote_sensors(host: &SshRemoteHost) -> Result<SystemSensorsSnapshot, String> {
    let script = r#"python3 -c '
import os, glob, json

mem = {}
try:
    with open("/proc/meminfo") as f:
        for line in f:
            p = line.split(":")
            if len(p) == 2:
                mem[p[0].strip()] = int(p[1].strip().split()[0]) * 1024
except:
    pass

cpu_model = "Remote CPU"
cores = 0
try:
    with open("/proc/cpuinfo") as f:
        for line in f:
            if "model name" in line and cpu_model == "Remote CPU":
                cpu_model = line.split(":")[1].strip()
            if "processor" in line:
                cores += 1
except:
    pass

cpu_freqs = []
for p in sorted(glob.glob("/sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_cur_freq")):
    try:
        cpu_freqs.append(int(open(p).read().strip()) // 1000)
    except:
        pass

cpu_tdp = None
for p in glob.glob("/sys/class/powercap/intel-rapl*/constraint_0_power_limit_uw"):
    try:
        v = int(open(p).read().strip()) / 1e6
        if v > 0:
            cpu_tdp = v
            break
    except:
        pass

cpu_power = None
for hw in glob.glob("/sys/class/hwmon/hwmon*"):
    name = open(hw + "/name").read().strip().lower() if os.path.exists(hw + "/name") else ""
    if "coretemp" in name or "k10temp" in name or "zenpower" in name or "cpu" in name:
        for pin in glob.glob(hw + "/power*_input"):
            try:
                cpu_power = int(open(pin).read().strip()) / 1e6
                break
            except:
                pass

disks = []
try:
    for line in os.popen("df -B1 -x tmpfs -x devtmpfs -x squashfs").read().strip().split("\n")[1:]:
        p = line.split()
        if len(p) >= 6:
            disks.append({"name": p[0], "total": int(p[1]), "used": int(p[2]), "avail": int(p[3]), "mount": p[5]})
except:
    pass

temps = []
for hw in glob.glob("/sys/class/hwmon/hwmon*"):
    name = open(hw + "/name").read().strip() if os.path.exists(hw + "/name") else os.path.basename(hw)
    for inp in glob.glob(hw + "/temp*_input"):
        lbl_p = inp.replace("_input", "_label")
        lbl = open(lbl_p).read().strip() if os.path.exists(lbl_p) else os.path.basename(inp)
        try:
            t = int(open(inp).read().strip()) / 1000.0
            if -40 <= t <= 150:
                temps.append({"chip": name, "label": lbl, "temp": t})
        except:
            pass

gpus = []
try:
    cmd = "nvidia-smi --query-gpu=name,temperature.gpu,utilization.gpu,memory.used,memory.total,clocks.current.graphics,clocks.current.memory,power.draw,power.limit --format=csv,noheader,nounits 2>/dev/null"
    for line in os.popen(cmd).read().strip().split("\n"):
        parts = [x.strip() for x in line.split(",") if x.strip()]
        if len(parts) >= 9:
            gpus.append({
                "name": parts[0],
                "temp": float(parts[1]) if parts[1] != "[N/A]" else None,
                "load": float(parts[2]) if parts[2] != "[N/A]" else None,
                "mem_used": int(float(parts[3])) if parts[3] != "[N/A]" else None,
                "mem_total": int(float(parts[4])) if parts[4] != "[N/A]" else None,
                "clock_mhz": int(float(parts[5])) if parts[5] != "[N/A]" else None,
                "mem_clock_mhz": int(float(parts[6])) if parts[6] != "[N/A]" else None,
                "power_watts": float(parts[7]) if parts[7] != "[N/A]" else None,
                "power_limit_watts": float(parts[8]) if parts[8] != "[N/A]" else None,
                "driver": "NVIDIA Proprietary"
            })
except:
    pass

if not gpus:
    for card in sorted(glob.glob("/sys/class/drm/card[0-9]*")):
        if "-" not in os.path.basename(card):
            vendor_p = card + "/device/vendor"
            v = open(vendor_p).read().strip() if os.path.exists(vendor_p) else ""
            v_name = "Intel Integrated Graphics" if v == "0x8086" else ("AMD Radeon Graphics" if v == "0x1002" else "Integrated GPU")
            act_freq = None
            for f in ["gt_act_freq_mhz", "gt_cur_freq_mhz"]:
                if os.path.exists(card + "/" + f):
                    try: act_freq = int(open(card + "/" + f).read().strip()); break
                    except: pass
            max_freq = None
            if os.path.exists(card + "/gt_max_freq_mhz"):
                try: max_freq = int(open(card + "/gt_max_freq_mhz").read().strip())
                except: pass
            load = None
            if act_freq and max_freq and max_freq > 0:
                load = round((act_freq / max_freq) * 100.0, 1)
            gpus.append({
                "name": v_name,
                "temp": None,
                "load": load,
                "mem_used": None,
                "mem_total": None,
                "clock_mhz": act_freq,
                "mem_clock_mhz": None,
                "power_watts": None,
                "power_limit_watts": None,
                "driver": "DRM / Linux Kernel"
            })
            break

ac_online = None
batteries = []
for ps in sorted(glob.glob("/sys/class/power_supply/*")):
    t = open(ps + "/type").read().strip().lower() if os.path.exists(ps + "/type") else ""
    if t in ["mains", "ac", "adp"] or "ac" in os.path.basename(ps).lower():
        if os.path.exists(ps + "/online"):
            try:
                ac_online = int(open(ps + "/online").read().strip()) == 1
            except:
                pass
    elif t == "battery" or "bat" in os.path.basename(ps).lower():
        present = open(ps + "/present").read().strip() == "1" if os.path.exists(ps + "/present") else True
        if not present:
            continue
        status = open(ps + "/status").read().strip() if os.path.exists(ps + "/status") else "Unknown"
        cap = float(open(ps + "/capacity").read().strip()) if os.path.exists(ps + "/capacity") else 0.0
        
        volt = None
        if os.path.exists(ps + "/voltage_now"):
            try: volt = round(int(open(ps + "/voltage_now").read().strip()) / 1e6, 2)
            except: pass
            
        p_watts = None
        if os.path.exists(ps + "/power_now"):
            try: p_watts = round(int(open(ps + "/power_now").read().strip()) / 1e6, 2)
            except: pass
        elif volt and os.path.exists(ps + "/current_now"):
            try:
                curr = int(open(ps + "/current_now").read().strip()) / 1e6
                p_watts = round(curr * volt, 2)
            except: pass
            
        energy_wh = None
        energy_full_wh = None
        energy_design_wh = None
        if os.path.exists(ps + "/energy_now"):
            try: energy_wh = round(int(open(ps + "/energy_now").read().strip()) / 1e6, 2)
            except: pass
        elif volt and os.path.exists(ps + "/charge_now"):
            try: energy_wh = round((int(open(ps + "/charge_now").read().strip()) / 1e6) * volt, 2)
            except: pass
            
        if os.path.exists(ps + "/energy_full"):
            try: energy_full_wh = round(int(open(ps + "/energy_full").read().strip()) / 1e6, 2)
            except: pass
        elif volt and os.path.exists(ps + "/charge_full"):
            try: energy_full_wh = round((int(open(ps + "/charge_full").read().strip()) / 1e6) * volt, 2)
            except: pass
            
        if os.path.exists(ps + "/energy_full_design"):
            try: energy_design_wh = round(int(open(ps + "/energy_full_design").read().strip()) / 1e6, 2)
            except: pass
        elif volt and os.path.exists(ps + "/charge_full_design"):
            try: energy_design_wh = round((int(open(ps + "/charge_full_design").read().strip()) / 1e6) * volt, 2)
            except: pass
            
        health = None
        if energy_full_wh and energy_design_wh and energy_design_wh > 0:
            health = round((energy_full_wh / energy_design_wh) * 100.0, 1)
        elif os.path.exists(ps + "/charge_full") and os.path.exists(ps + "/charge_full_design"):
            try:
                cf = int(open(ps + "/charge_full").read().strip())
                cd = int(open(ps + "/charge_full_design").read().strip())
                if cd > 0: health = round((cf / cd) * 100.0, 1)
            except: pass
            
        cycles = None
        if os.path.exists(ps + "/cycle_count"):
            try:
                c = int(open(ps + "/cycle_count").read().strip())
                if c > 0: cycles = c
            except: pass
            
        tech = open(ps + "/technology").read().strip() if os.path.exists(ps + "/technology") else ""
        model = open(ps + "/model_name").read().strip() if os.path.exists(ps + "/model_name") else ""
        mfg = open(ps + "/manufacturer").read().strip() if os.path.exists(ps + "/manufacturer") else ""
        
        batteries.append({
            "name": os.path.basename(ps),
            "status": status,
            "capacity_pct": cap,
            "power_watts": p_watts,
            "voltage_volts": volt,
            "energy_wh": energy_wh,
            "energy_full_wh": energy_full_wh,
            "energy_design_wh": energy_design_wh,
            "health_pct": health,
            "cycle_count": cycles,
            "technology": tech,
            "model_name": model,
            "manufacturer": mfg
        })

data = {
    "cpu_model": cpu_model,
    "cores": max(cores, 1),
    "cpu_freqs": cpu_freqs,
    "cpu_tdp": cpu_tdp,
    "cpu_power": cpu_power,
    "mem_total": mem.get("MemTotal", 0),
    "mem_avail": mem.get("MemAvailable", mem.get("MemFree", 0)),
    "swap_total": mem.get("SwapTotal", 0),
    "swap_free": mem.get("SwapFree", 0),
    "disks": disks,
    "temps": temps,
    "gpus": gpus,
    "power": {
        "ac_online": ac_online,
        "batteries": batteries
    }
}
print(json.dumps(data))
'
"#;

    let mut cmd = build_ssh_command(host, script);
    let out = cmd.output().map_err(|e| format!("SSH command execution error: {}", e))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Remote sensor query failed: {}", err.trim()));
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("Failed to parse remote sensor json: {}", e))?;

    let cpu_brand = parsed["cpu_model"].as_str().unwrap_or("Remote CPU").to_string();
    let cpu_cores = parsed["cores"].as_u64().unwrap_or(1) as usize;

    let mut cpu_frequencies_mhz = Vec::new();
    if let Some(arr) = parsed["cpu_freqs"].as_array() {
        for f in arr {
            if let Some(val) = f.as_u64() {
                cpu_frequencies_mhz.push(val);
            }
        }
    }
    let cpu_frequency_mhz = if !cpu_frequencies_mhz.is_empty() {
        cpu_frequencies_mhz.iter().sum::<u64>() / cpu_frequencies_mhz.len() as u64
    } else {
        0
    };

    let cpu_tdp_watts = parsed["cpu_tdp"].as_f64().map(|v| v as f32);
    let cpu_power_watts = parsed["cpu_power"].as_f64().map(|v| v as f32);

    let mem_total_bytes = parsed["mem_total"].as_u64().unwrap_or(0);
    let mem_avail = parsed["mem_avail"].as_u64().unwrap_or(0);
    let mem_used_bytes = mem_total_bytes.saturating_sub(mem_avail);
    let mem_usage_pct = if mem_total_bytes > 0 {
        (mem_used_bytes as f32 / mem_total_bytes as f32) * 100.0
    } else {
        0.0
    };

    let swap_total_bytes = parsed["swap_total"].as_u64().unwrap_or(0);
    let swap_free = parsed["swap_free"].as_u64().unwrap_or(0);
    let swap_used_bytes = swap_total_bytes.saturating_sub(swap_free);
    let swap_usage_pct = if swap_total_bytes > 0 {
        (swap_used_bytes as f32 / swap_total_bytes as f32) * 100.0
    } else {
        0.0
    };

    let mut disks = Vec::new();
    if let Some(arr) = parsed["disks"].as_array() {
        for d in arr {
            let total = d["total"].as_u64().unwrap_or(0);
            let used = d["used"].as_u64().unwrap_or(0);
            let avail = d["avail"].as_u64().unwrap_or(0);
            let pct = if total > 0 { (used as f32 / total as f32) * 100.0 } else { 0.0 };
            disks.push(DiskInfo {
                name: d["name"].as_str().unwrap_or("").to_string(),
                mount_point: d["mount"].as_str().unwrap_or("").to_string(),
                fs_type: String::new(),
                total_bytes: total,
                available_bytes: avail,
                used_bytes: used,
                usage_pct: pct,
            });
        }
    }

    let mut gpus = Vec::new();
    if let Some(arr) = parsed["gpus"].as_array() {
        for g in arr {
            gpus.push(GpuSensorInfo {
                name: g["name"].as_str().unwrap_or("Remote GPU").to_string(),
                load_pct: g["load"].as_f64().map(|v| v as f32),
                temp: g["temp"].as_f64().map(|v| v as f32),
                mem_used_mb: g["mem_used"].as_u64(),
                mem_total_mb: g["mem_total"].as_u64(),
                clock_mhz: g["clock_mhz"].as_u64().map(|v| v as u32),
                mem_clock_mhz: g["mem_clock_mhz"].as_u64().map(|v| v as u32),
                power_watts: g["power_watts"].as_f64().map(|v| v as f32),
                power_limit_watts: g["power_limit_watts"].as_f64().map(|v| v as f32),
                driver: g["driver"].as_str().unwrap_or("").to_string(),
            });
        }
    }

    let mut batteries = Vec::new();
    let mut ac_online = None;
    if let Some(p_obj) = parsed.get("power") {
        ac_online = p_obj["ac_online"].as_bool();
        if let Some(b_arr) = p_obj["batteries"].as_array() {
            for b in b_arr {
                batteries.push(BatteryInfo {
                    name: b["name"].as_str().unwrap_or("BAT").to_string(),
                    present: true,
                    status: b["status"].as_str().unwrap_or("Unknown").to_string(),
                    capacity_pct: b["capacity_pct"].as_f64().unwrap_or(0.0) as f32,
                    power_watts: b["power_watts"].as_f64().map(|v| v as f32),
                    voltage_volts: b["voltage_volts"].as_f64().map(|v| v as f32),
                    energy_wh: b["energy_wh"].as_f64().map(|v| v as f32),
                    energy_full_wh: b["energy_full_wh"].as_f64().map(|v| v as f32),
                    energy_design_wh: b["energy_design_wh"].as_f64().map(|v| v as f32),
                    health_pct: b["health_pct"].as_f64().map(|v| v as f32),
                    cycle_count: b["cycle_count"].as_u64().map(|v| v as u32),
                    technology: b["technology"].as_str().unwrap_or("").to_string(),
                    model_name: b["model_name"].as_str().unwrap_or("").to_string(),
                    manufacturer: b["manufacturer"].as_str().unwrap_or("").to_string(),
                });
            }
        }
    }

    let total_battery_pct = if !batteries.is_empty() {
        let sum: f32 = batteries.iter().map(|b| b.capacity_pct).sum();
        Some(sum / batteries.len() as f32)
    } else {
        None
    };

    let total_power_watts = {
        let sum: f32 = batteries.iter().filter_map(|b| b.power_watts).sum();
        if sum > 0.0 {
            Some(sum)
        } else {
            None
        }
    };

    let power = PowerSupplyInfo {
        ac_online,
        batteries,
        total_battery_pct,
        total_power_watts,
    };

    let mut temp_sensors = Vec::new();
    let mut cpu_temp = None;
    let mut ram_temp = None;
    let mut storage_temp = None;
    let mut gpu_temp = None;

    if let Some(arr) = parsed["temps"].as_array() {
        for t in arr {
            let chip = t["chip"].as_str().unwrap_or("").to_string();
            let label = t["label"].as_str().unwrap_or("").to_string();
            let temp = t["temp"].as_f64().unwrap_or(0.0) as f32;
            let cat = categorize_sensor(&label, &chip);

            match cat {
                SensorCategory::Cpu if cpu_temp.is_none() || label.contains("Package") || label == "CPU" => {
                    cpu_temp = Some(temp);
                }
                SensorCategory::Memory if ram_temp.is_none() || label.contains("SODIMM") => {
                    ram_temp = Some(temp);
                }
                SensorCategory::Storage if storage_temp.is_none() || label.contains("Composite") => {
                    storage_temp = Some(temp);
                }
                SensorCategory::Gpu if gpu_temp.is_none() => {
                    gpu_temp = Some(temp);
                }
                _ => {}
            }

            temp_sensors.push(TempSensor {
                name: label,
                chip,
                temp,
                max: None,
                crit: None,
                category: cat,
            });
        }
    }

    Ok(SystemSensorsSnapshot {
        cpu_brand,
        cpu_cores_physical: cpu_cores,
        cpu_cores_logical: cpu_cores,
        cpu_global_usage: 0.0,
        cpu_core_usages: vec![0.0; cpu_cores],
        cpu_frequency_mhz,
        cpu_frequencies_mhz,
        cpu_power_watts,
        cpu_tdp_watts,
        cpu_temp,
        mem_total_bytes,
        mem_used_bytes,
        mem_free_bytes: mem_avail,
        mem_usage_pct,
        swap_total_bytes,
        swap_used_bytes,
        swap_usage_pct,
        ram_temp,
        gpus,
        power,
        disks,
        storage_temp,
        networks: Vec::new(),
        total_rx_rate_bps: 0.0,
        total_tx_rate_bps: 0.0,
        total_rx_bytes: 0,
        total_tx_bytes: 0,
        temp_sensors,
        timestamp: Some(Instant::now()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1024 * 1024 * 5), "5.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024 * 16), "16.0 GB");
    }

    #[test]
    fn test_format_rate() {
        assert_eq!(format_rate(500.0), "500 B/s");
        assert_eq!(format_rate(1024.0 * 250.0), "250.0 KB/s");
        assert_eq!(format_rate(1024.0 * 1024.0 * 12.5), "12.5 MB/s");
    }

    #[test]
    fn test_sensor_categorization() {
        assert_eq!(categorize_sensor("Package id 0", "coretemp"), SensorCategory::Cpu);
        assert_eq!(categorize_sensor("SODIMM", "dell_smm"), SensorCategory::Memory);
        assert_eq!(categorize_sensor("Composite", "nvme"), SensorCategory::Storage);
        assert_eq!(categorize_sensor("amdgpu edge", "amdgpu"), SensorCategory::Gpu);
        assert_eq!(categorize_sensor("temp1", "acpitz"), SensorCategory::System);
    }

    #[test]
    fn test_sensors_collector_creation() {
        let mut collector = SystemSensorsCollector::new();
        let snap = collector.collect();
        assert!(!snap.cpu_brand.is_empty());
        assert!(snap.mem_total_bytes > 0);
        assert!(!snap.cpu_core_usages.is_empty());
        assert!(!snap.cpu_frequencies_mhz.is_empty() || snap.cpu_frequency_mhz > 0);
    }

    #[test]
    fn test_power_supply_collection() {
        let ps = collect_power_supply();
        // Just verify it runs without crashing and creates a valid struct
        if let Some(pct) = ps.total_battery_pct {
            assert!(pct >= 0.0 && pct <= 100.0);
        }
    }
}
