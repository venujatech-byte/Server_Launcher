use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use sysinfo::{Components, Disks, Networks, System};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SensorCategory {
    TopApps,
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
            SensorCategory::TopApps => "Top Apps",
            SensorCategory::Cpu => "CPU",
            SensorCategory::Memory => "Memory (RAM)",
            SensorCategory::Gpu => "GPU",
            SensorCategory::Storage => "Storage / NVMe",
            SensorCategory::System => "System / ACPI",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            SensorCategory::TopApps => crate::lucide::ZAP,
            SensorCategory::Cpu => crate::lucide::CPU,
            SensorCategory::Memory => crate::lucide::RAM,
            SensorCategory::Gpu => crate::lucide::GPU,
            SensorCategory::Storage => crate::lucide::STORAGE,
            SensorCategory::System => crate::lucide::SENSORS,
        }
    }

    pub fn emoji(&self) -> &'static str {
        self.icon()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProcessSensorInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
    pub disk_read_bytes: u64,
    pub disk_written_bytes: u64,
    pub disk_read_rate_bps: f64,
    pub disk_write_rate_bps: f64,
    pub disk_rate_bps: f64,
    pub status: String,
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
    pub read_rate_bps: f64,
    pub write_rate_bps: f64,
    pub total_read_bytes: u64,
    pub total_write_bytes: u64,
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
    pub total_disk_read_rate_bps: f64,
    pub total_disk_write_rate_bps: f64,

    // Network
    pub networks: Vec<NetInterfaceInfo>,
    pub total_rx_rate_bps: f64,
    pub total_tx_rate_bps: f64,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,

    // All temperature sensors
    pub temp_sensors: Vec<TempSensor>,

    // Top Applications & Processes
    pub top_apps: Vec<ProcessSensorInfo>,

    // OS info
    pub os_name: String,
    pub os_version: String,

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
    prev_disk_read: HashMap<String, u64>,
    prev_disk_write: HashMap<String, u64>,
    prev_disk_time: Instant,
    prev_proc_disk: HashMap<u32, (u64, u64)>,
}

impl Default for SystemSensorsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemSensorsCollector {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_all();
        sys.refresh_memory();
        let disks = Disks::new_with_refreshed_list();
        let networks = Networks::new_with_refreshed_list();
        let components = Components::new_with_refreshed_list();
        let now = Instant::now();

        Self {
            sys,
            disks,
            networks,
            components,
            prev_net_rx: HashMap::new(),
            prev_net_tx: HashMap::new(),
            prev_net_time: now,
            prev_disk_read: HashMap::new(),
            prev_disk_write: HashMap::new(),
            prev_disk_time: now,
            prev_proc_disk: HashMap::new(),
        }
    }

    pub fn collect(&mut self) -> SystemSensorsSnapshot {
        self.collect_with_top_apps(true)
    }

    pub fn collect_with_top_apps(&mut self, include_top_apps: bool) -> SystemSensorsSnapshot {
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
        let (mut cpu_frequencies_mhz, mut cpu_power_watts, cpu_tdp_watts) = collect_cpu_frequencies_and_power();
        if cpu_frequencies_mhz.is_empty() {
            cpu_frequencies_mhz = self.sys.cpus().iter().map(|c| c.frequency()).collect();
        }
        let cpu_frequency_mhz = if !cpu_frequencies_mhz.is_empty() {
            (cpu_frequencies_mhz.iter().sum::<u64>() / cpu_frequencies_mhz.len() as u64).max(1)
        } else {
            self.sys.cpus().first().map(|c| c.frequency()).unwrap_or(0)
        };

        // Guarantee CPU power watts is always populated (hwmon/RAPL or dynamic TDP-usage model)
        if cpu_power_watts.is_none() {
            let tdp = cpu_tdp_watts.unwrap_or(match cpu_cores_physical {
                1..=2 => 15.0,
                3..=4 => 28.0,
                5..=8 => 45.0,
                _ => 65.0,
            });
            let idle_w = (tdp * 0.12).clamp(2.5, 12.0);
            let load_ratio = (cpu_global_usage / 100.0).clamp(0.0, 1.0);
            let p_est = idle_w + (tdp - idle_w) * (0.35 * load_ratio + 0.65 * load_ratio.powf(1.35));
            cpu_power_watts = Some((p_est * 10.0).round() / 10.0);
        }

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
        let disk_elapsed_secs = self.prev_disk_time.elapsed().as_secs_f64().max(0.001);
        let current_diskstats = collect_proc_diskstats();

        let mut disks = Vec::new();
        let mut total_disk_read_rate_bps = 0.0;
        let mut total_disk_write_rate_bps = 0.0;

        for d in &self.disks {
            let total = d.total_space();
            let avail = d.available_space();
            let used = total.saturating_sub(avail);
            let pct = if total > 0 {
                (used as f32 / total as f32) * 100.0
            } else {
                0.0
            };

            let raw_name = d.name().to_string_lossy().to_string();
            let clean_dev = raw_name.trim_start_matches("/dev/").to_string();

            // Match diskstats
            let (read_bytes, write_bytes) = current_diskstats
                .get(&clean_dev)
                .copied()
                .or_else(|| {
                    current_diskstats.iter().find_map(|(k, v)| {
                        if clean_dev.starts_with(k) || k.starts_with(&clean_dev) {
                            Some(*v)
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or((0, 0));

            let prev_read = *self.prev_disk_read.get(&clean_dev).unwrap_or(&read_bytes);
            let prev_write = *self.prev_disk_write.get(&clean_dev).unwrap_or(&write_bytes);

            let read_rate = (read_bytes.saturating_sub(prev_read) as f64) / disk_elapsed_secs;
            let write_rate = (write_bytes.saturating_sub(prev_write) as f64) / disk_elapsed_secs;

            self.prev_disk_read.insert(clean_dev.clone(), read_bytes);
            self.prev_disk_write.insert(clean_dev.clone(), write_bytes);

            disks.push(DiskInfo {
                name: raw_name,
                mount_point: d.mount_point().to_string_lossy().to_string(),
                fs_type: d.file_system().to_string_lossy().to_string(),
                total_bytes: total,
                available_bytes: avail,
                used_bytes: used,
                usage_pct: pct,
                read_rate_bps: read_rate,
                write_rate_bps: write_rate,
                total_read_bytes: read_bytes,
                total_write_bytes: write_bytes,
            });
        }
        // Deduplicate or sort by mount point
        disks.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));

        // Calculate total physical disk read/write throughput rates across base devices
        let mut base_matched = false;
        for (dev, &(r_bytes, w_bytes)) in &current_diskstats {
            if is_base_physical_disk(dev) {
                base_matched = true;
                let prev_r = *self.prev_disk_read.get(dev).unwrap_or(&r_bytes);
                let prev_w = *self.prev_disk_write.get(dev).unwrap_or(&w_bytes);
                total_disk_read_rate_bps += (r_bytes.saturating_sub(prev_r) as f64) / disk_elapsed_secs;
                total_disk_write_rate_bps += (w_bytes.saturating_sub(prev_w) as f64) / disk_elapsed_secs;
                self.prev_disk_read.insert(dev.clone(), r_bytes);
                self.prev_disk_write.insert(dev.clone(), w_bytes);
            }
        }
        if !base_matched {
            for d in &disks {
                total_disk_read_rate_bps += d.read_rate_bps;
                total_disk_write_rate_bps += d.write_rate_bps;
            }
        }
        self.prev_disk_time = now;

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

        // 9. Top Applications & Resource Consumers
        let mut top_apps = Vec::new();
        if include_top_apps {
            self.sys.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::All,
                sysinfo::ProcessRefreshKind::new()
                    .with_cpu()
                    .with_memory()
                    .with_disk_usage()
                    .with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
            );
            let mut current_proc_disk = HashMap::new();

            for (pid, proc) in self.sys.processes() {
                // Filter out OS-level threads so only actual processes are listed!
                // On Linux, sysinfo returns all kernel tasks/threads with thread_kind == Some(Userland).
                // Threads share the same address space (memory) and their CPU is already accounted for in the main process.
                if proc.thread_kind().is_some() {
                    continue;
                }

                let pid_u32 = pid.as_u32();
                let mut name = proc.name().to_string_lossy().to_string();
                if (name == "exe" || name.is_empty()) && proc.exe().is_some() {
                    if let Some(exe_file) = proc.exe().unwrap().file_name() {
                        name = exe_file.to_string_lossy().to_string();
                    }
                }
                let cpu_usage = proc.cpu_usage();
                let memory_bytes = proc.memory();
                let disk_usage = proc.disk_usage();
                let read_b = disk_usage.read_bytes;
                let written_b = disk_usage.written_bytes;

                current_proc_disk.insert(pid_u32, (read_b, written_b));

                let (prev_r, prev_w) = self.prev_proc_disk.get(&pid_u32).copied().unwrap_or((read_b, written_b));
                let r_rate = (read_b.saturating_sub(prev_r) as f64) / disk_elapsed_secs;
                let w_rate = (written_b.saturating_sub(prev_w) as f64) / disk_elapsed_secs;
                let disk_rate_bps = r_rate + w_rate;

                let status = format!("{:?}", proc.status());

                top_apps.push(ProcessSensorInfo {
                    pid: pid_u32,
                    name,
                    cpu_usage,
                    memory_bytes,
                    disk_read_bytes: read_b,
                    disk_written_bytes: written_b,
                    disk_read_rate_bps: r_rate,
                    disk_write_rate_bps: w_rate,
                    disk_rate_bps,
                    status,
                });
            }
            self.prev_proc_disk = current_proc_disk;

            // Sort by CPU usage descending by default, then memory
            top_apps.sort_by(|a, b| {
                b.cpu_usage
                    .partial_cmp(&a.cpu_usage)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.memory_bytes.cmp(&a.memory_bytes))
            });
            top_apps.truncate(150);
        }

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
            total_disk_read_rate_bps,
            total_disk_write_rate_bps,
            networks,
            total_rx_rate_bps,
            total_tx_rate_bps,
            total_rx_bytes,
            total_tx_bytes,
            temp_sensors,
            top_apps,
            os_name: System::name().unwrap_or_default(),
            os_version: System::os_version().unwrap_or_default(),
            timestamp: Some(now),
        }
    }
}

fn collect_proc_diskstats() -> HashMap<String, (u64, u64)> {
    let mut stats = HashMap::new();
    #[cfg(target_os = "linux")]
    if let Ok(content) = fs::read_to_string("/proc/diskstats") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 10 {
                let dev = parts[2].to_string();
                let sectors_read = parts[5].parse::<u64>().unwrap_or(0);
                let sectors_written = parts[9].parse::<u64>().unwrap_or(0);
                let read_bytes = sectors_read.saturating_mul(512);
                let write_bytes = sectors_written.saturating_mul(512);
                stats.insert(dev, (read_bytes, write_bytes));
            }
        }
    }
    stats
}

fn is_base_physical_disk(dev: &str) -> bool {
    if dev.starts_with("loop") || dev.starts_with("ram") || dev.starts_with("zram") || dev.starts_with("dm-") {
        return false;
    }
    if dev.starts_with("sd") || dev.starts_with("vd") || dev.starts_with("hd") || dev.starts_with("xvd") {
        return !dev.chars().last().map(|c| c.is_ascii_digit()).unwrap_or(false);
    }
    if dev.starts_with("nvme") || dev.starts_with("mmcblk") {
        return !dev.contains('p');
    }
    true
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
        // 1. Read per-core frequencies from cpufreq (scaling_cur_freq or cpuinfo_cur_freq)
        let mut core_idx = 0;
        loop {
            let p_scaling = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", core_idx);
            let p_cur = format!("/sys/devices/system/cpu/cpu{}/cpufreq/cpuinfo_cur_freq", core_idx);

            let khz_opt = fs::read_to_string(&p_scaling)
                .or_else(|_| fs::read_to_string(&p_cur))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok());

            if let Some(khz) = khz_opt {
                freqs.push(khz / 1000);
            } else if core_idx > 0 || !Path::new(&format!("/sys/devices/system/cpu/cpu{}", core_idx)).exists() {
                break;
            }
            core_idx += 1;
            if core_idx > 128 {
                break;
            }
        }

        // Fallback: Read cpu MHz from /proc/cpuinfo if cpufreq is unavailable (VMs, VPS, containers)
        if freqs.is_empty() {
            if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
                for line in cpuinfo.lines() {
                    if line.starts_with("cpu MHz") {
                        if let Some(val_str) = line.split(':').nth(1) {
                            if let Ok(mhz) = val_str.trim().parse::<f64>() {
                                freqs.push(mhz as u64);
                            }
                        }
                    }
                }
            }
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

        // 3. Read power draw from hwmon if labeled CPU / coretemp / k10temp / rapl
        let hwmon_dir = Path::new("/sys/class/hwmon");
        if hwmon_dir.exists() {
            if let Ok(entries) = fs::read_dir(hwmon_dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    let name = fs::read_to_string(p.join("name")).unwrap_or_default().to_lowercase();
                    if name.contains("coretemp")
                        || name.contains("k10temp")
                        || name.contains("zenpower")
                        || name.contains("cpu")
                        || name.contains("rapl")
                        || name.contains("fam15h")
                    {
                        if let Ok(files) = fs::read_dir(&p) {
                            for f in files.flatten() {
                                let fname = f.file_name().to_string_lossy().to_string();
                                if fname.starts_with("power") && (fname.ends_with("_input") || fname.ends_with("_average")) {
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

static HAS_NVIDIA_SMI: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn detect_gpus(known_temp: Option<f32>) -> Vec<GpuSensorInfo> {
    let mut gpus = Vec::new();

    // 1. Check NVIDIA via nvidia-smi if available
    if HAS_NVIDIA_SMI.load(std::sync::atomic::Ordering::Relaxed) {
        let nvidia_out = Command::new("nvidia-smi")
            .args([
                "--query-gpu=name,temperature.gpu,utilization.gpu,utilization.memory,memory.used,memory.total,clocks.current.graphics,clocks.current.memory,power.draw,power.limit",
                "--format=csv,noheader,nounits"
            ])
            .output();

        match nvidia_out {
            Ok(out) if out.status.success() => {
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
            _ => {
                HAS_NVIDIA_SMI.store(false, std::sync::atomic::Ordering::Relaxed);
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

                        // Check frequency across all DRM / i915 / gt0 interfaces
                        let freq_mhz = fs::read_to_string(e.path().join("gt_act_freq_mhz"))
                            .or_else(|_| fs::read_to_string(e.path().join("gt/gt0/rps_act_freq_mhz")))
                            .or_else(|_| fs::read_to_string(e.path().join("gt_cur_freq_mhz")))
                            .or_else(|_| fs::read_to_string(e.path().join("gt/gt0/rps_cur_freq_mhz")))
                            .or_else(|_| fs::read_to_string(dev.join("gt_act_freq_mhz")))
                            .or_else(|_| fs::read_to_string(dev.join("gt_cur_freq_mhz")))
                            .ok()
                            .and_then(|s| s.trim().parse::<f32>().ok());

                        let max_freq_mhz = fs::read_to_string(e.path().join("gt_max_freq_mhz"))
                            .or_else(|_| fs::read_to_string(e.path().join("gt/gt0/rps_max_freq_mhz")))
                            .or_else(|_| fs::read_to_string(dev.join("gt_max_freq_mhz")))
                            .ok()
                            .and_then(|s| s.trim().parse::<f32>().ok())
                            .unwrap_or(1200.0);

                        let load_pct = freq_mhz.map(|f| ((f / max_freq_mhz) * 100.0).clamp(0.0, 100.0));

                        let mut core_clock = freq_mhz.map(|f| f as u32);
                        let mut mem_clock = None;
                        let mut power_watts = None;

                        let hwmon_p = dev.join("hwmon");
                        if hwmon_p.exists() {
                            if let Ok(entries) = fs::read_dir(&hwmon_p) {
                                for hwe in entries.flatten() {
                                    let hp = hwe.path();
                                    if power_watts.is_none() {
                                        if let Ok(s) = fs::read_to_string(hp.join("power1_average"))
                                            .or_else(|_| fs::read_to_string(hp.join("power1_input")))
                                        {
                                            if let Ok(uw) = s.trim().parse::<f32>() {
                                                power_watts = Some(uw / 1_000_000.0);
                                            }
                                        }
                                    }
                                    if core_clock.is_none() {
                                        if let Ok(s) = fs::read_to_string(hp.join("freq1_input")) {
                                            if let Ok(hz) = s.trim().parse::<u64>() {
                                                core_clock = Some((hz / 1_000_000) as u32);
                                            }
                                        }
                                    }
                                    if mem_clock.is_none() {
                                        if let Ok(s) = fs::read_to_string(hp.join("freq2_input")) {
                                            if let Ok(hz) = s.trim().parse::<u64>() {
                                                mem_clock = Some((hz / 1_000_000) as u32);
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Check AMD pp_dpm_sclk if still no core clock
                        if core_clock.is_none() {
                            if let Ok(s) = fs::read_to_string(dev.join("pp_dpm_sclk")) {
                                for line in s.lines() {
                                    if line.contains('*') {
                                        let parts: Vec<&str> = line.split_whitespace().collect();
                                        for p in parts {
                                            if p.ends_with("mhz") || p.ends_with("MHz") {
                                                let num_str = p.trim_end_matches(|c: char| !c.is_ascii_digit());
                                                if let Ok(val) = num_str.parse::<u32>() {
                                                    core_clock = Some(val);
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if power_watts.is_none() {
                            if let Some(clk) = core_clock {
                                let max_f = max_freq_mhz.max(400.0);
                                let clk_ratio = (clk as f32 / max_f).clamp(0.0, 1.0);
                                let load_ratio = (load_pct.unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
                                let p_est = 1.0 + 14.0 * (0.35 * clk_ratio + 0.65 * load_ratio);
                                power_watts = Some((p_est * 10.0).round() / 10.0);
                            }
                        }

                        gpus.push(GpuSensorInfo {
                            name: vendor_name.to_string(),
                            load_pct,
                            temp: known_temp,
                            mem_used_mb: None,
                            mem_total_mb: None,
                            clock_mhz: core_clock,
                            mem_clock_mhz: mem_clock,
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
import os, glob, json, time

def read_cpu_stats():
    cpus = {}
    try:
        with open("/proc/stat") as f:
            for line in f:
                if line.startswith("cpu"):
                    p = line.split()
                    name = p[0]
                    vals = [float(x) for x in p[1:]]
                    idle = vals[3] + (vals[4] if len(vals) > 4 else 0.0)
                    total = sum(vals)
                    cpus[name] = (total, idle)
    except:
        pass
    return cpus

def read_disk_stats():
    d = {}
    try:
        with open("/proc/diskstats") as f:
            for line in f:
                p = line.split()
                if len(p) >= 10:
                    dev = p[2]
                    r_b = int(p[5]) * 512
                    w_b = int(p[9]) * 512
                    d[dev] = (r_b, w_b)
    except:
        pass
    return d

t_start = time.time()
c1 = read_cpu_stats()
d1 = read_disk_stats()

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
if not cpu_freqs:
    for p in sorted(glob.glob("/sys/devices/system/cpu/cpu[0-9]*/cpufreq/cpuinfo_cur_freq")):
        try:
            cpu_freqs.append(int(open(p).read().strip()) // 1000)
        except:
            pass
if not cpu_freqs:
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if "cpu MHz" in line:
                    cpu_freqs.append(int(float(line.split(":")[1].strip())))
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
    if any(k in name for k in ["coretemp", "k10temp", "zenpower", "cpu", "rapl", "fam15h"]):
        for pin in glob.glob(hw + "/power*_input") + glob.glob(hw + "/power*_average"):
            try:
                v = int(open(pin).read().strip()) / 1e6
                if 0 < v < 1000:
                    cpu_power = v
                    break
            except:
                pass
        if cpu_power:
            break

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

            p_watts = None
            mem_clk = None
            for hw in glob.glob(card + "/device/hwmon/hwmon*"):
                for pin in glob.glob(hw + "/power1_average") + glob.glob(hw + "/power1_input"):
                    try:
                        p_watts = round(int(open(pin).read().strip()) / 1e6, 2)
                        break
                    except: pass
                if not act_freq and os.path.exists(hw + "/freq1_input"):
                    try: act_freq = int(open(hw + "/freq1_input").read().strip()) // 1000000
                    except: pass
                if os.path.exists(hw + "/freq2_input"):
                    try: mem_clk = int(open(hw + "/freq2_input").read().strip()) // 1000000
                    except: pass

            gpus.append({
                "name": v_name,
                "temp": None,
                "load": load,
                "mem_used": None,
                "mem_total": None,
                "clock_mhz": act_freq,
                "mem_clock_mhz": mem_clk,
                "power_watts": p_watts,
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

networks = []
try:
    with open("/proc/net/dev") as f:
        for line in f:
            if ":" in line:
                p = line.split(":")
                ifname = p[0].strip()
                if ifname == "lo":
                    continue
                fields = p[1].split()
                if len(fields) >= 9:
                    rx = int(fields[0])
                    tx = int(fields[8])
                    networks.append({"name": ifname, "rx_bytes": rx, "tx_bytes": tx})
except:
    pass

t_elapsed = time.time() - t_start
if t_elapsed < 0.12:
    time.sleep(0.12 - t_elapsed)
c2 = read_cpu_stats()
d2 = read_disk_stats()
t_disk_elapsed = max(time.time() - t_start, 0.05)

disk_rates = {}
for dev, (r2, w2) in d2.items():
    if dev in d1:
        r1, w1 = d1[dev]
        r_rate = max(0.0, (r2 - r1) / t_disk_elapsed)
        w_rate = max(0.0, (w2 - w1) / t_disk_elapsed)
        disk_rates[dev] = (r_rate, w_rate, r2, w2)
    else:
        disk_rates[dev] = (0.0, 0.0, r2, w2)

for d in disks:
    dev_name = os.path.basename(d["name"])
    st = disk_rates.get(dev_name)
    if not st:
        for k, v in disk_rates.items():
            if dev_name.startswith(k) or k.startswith(dev_name):
                st = v; break
    d["read_rate"] = st[0] if st else 0.0
    d["write_rate"] = st[1] if st else 0.0
    d["total_read"] = st[2] if st else 0
    d["total_write"] = st[3] if st else 0

tot_disk_read_rate = 0.0
tot_disk_write_rate = 0.0
base_found = False
for dev, (r_rate, w_rate, _, _) in disk_rates.items():
    if not (dev.startswith("loop") or dev.startswith("ram") or dev.startswith("zram") or dev.startswith("dm-")):
        is_base = False
        if any(dev.startswith(pfx) for pfx in ["sd", "vd", "hd", "xvd"]):
            is_base = not dev[-1].isdigit()
        elif dev.startswith("nvme") or dev.startswith("mmcblk"):
            is_base = "p" not in dev
        if is_base:
            base_found = True
            tot_disk_read_rate += r_rate
            tot_disk_write_rate += w_rate

if not base_found:
    for d in disks:
        tot_disk_read_rate += d.get("read_rate", 0.0)
        tot_disk_write_rate += d.get("write_rate", 0.0)

cpu_usage = 0.0
if "cpu" in c1 and "cpu" in c2:
    td = c2["cpu"][0] - c1["cpu"][0]
    id_d = c2["cpu"][1] - c1["cpu"][1]
    if td > 0:
        cpu_usage = max(0.0, min(100.0, round(100.0 * (1.0 - id_d / td), 1)))

cpu_core_usages = []
idx = 0
while f"cpu{idx}" in c1 and f"cpu{idx}" in c2:
    td = c2[f"cpu{idx}"][0] - c1[f"cpu{idx}"][0]
    id_d = c2[f"cpu{idx}"][1] - c1[f"cpu{idx}"][1]
    if td > 0:
        cpu_core_usages.append(max(0.0, min(100.0, round(100.0 * (1.0 - id_d / td), 1))))
    else:
        cpu_core_usages.append(0.0)
    idx += 1

if cpu_usage == 0.0 and os.path.exists("/proc/loadavg"):
    try:
        lavg = float(open("/proc/loadavg").read().split()[0])
        cpu_usage = max(0.0, min(100.0, round((lavg / max(cores, 1)) * 100.0, 1)))
    except:
        pass

top_apps = []
try:
    for line in os.popen("ps -eo pid,pcpu,pmem,rss,comm --sort=-pcpu 2>/dev/null").read().strip().split("\n")[1:31]:
        p = line.split()
        if len(p) >= 5:
            top_apps.append({
                "pid": int(p[0]),
                "cpu": float(p[1]),
                "mem_bytes": int(p[3]) * 1024,
                "name": " ".join(p[4:]),
                "status": "Running",
            })
except:
    pass

os_name = ""
os_version = ""
try:
    with open("/etc/os-release") as f:
        for line in f:
            if line.startswith("NAME="):
                os_name = line.split("=", 1)[1].strip().strip("\"")
            elif line.startswith("VERSION_ID="):
                os_version = line.split("=", 1)[1].strip().strip("\"")
except:
    pass
if not os_name:
    try:
        import platform
        os_name = platform.system()
        os_version = platform.release()
    except:
        pass

data = {
    "cpu_model": cpu_model,
    "cores": max(cores, 1),
    "cpu_usage": cpu_usage,
    "cpu_core_usages": cpu_core_usages,
    "cpu_freqs": cpu_freqs,
    "cpu_tdp": cpu_tdp,
    "cpu_power": cpu_power,
    "mem_total": mem.get("MemTotal", 0),
    "mem_avail": mem.get("MemAvailable", mem.get("MemFree", 0)),
    "swap_total": mem.get("SwapTotal", 0),
    "swap_free": mem.get("SwapFree", 0),
    "disks": disks,
    "total_disk_read_rate": tot_disk_read_rate,
    "total_disk_write_rate": tot_disk_write_rate,
    "temps": temps,
    "gpus": gpus,
    "networks": networks,
    "power": {
        "ac_online": ac_online,
        "batteries": batteries
    },
    "top_apps": top_apps,
    "os_name": os_name,
    "os_version": os_version
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
    let cpu_global_usage = parsed["cpu_usage"].as_f64().unwrap_or(0.0) as f32;
    let cpu_core_usages = if let Some(arr) = parsed["cpu_core_usages"].as_array() {
        let usages: Vec<f32> = arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect();
        if usages.is_empty() {
            vec![cpu_global_usage; cpu_cores]
        } else {
            usages
        }
    } else {
        vec![cpu_global_usage; cpu_cores]
    };

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

    let total_disk_read_rate_bps = parsed["total_disk_read_rate"].as_f64().unwrap_or(0.0);
    let total_disk_write_rate_bps = parsed["total_disk_write_rate"].as_f64().unwrap_or(0.0);

    let mut disks = Vec::new();
    if let Some(arr) = parsed["disks"].as_array() {
        for d in arr {
            let total = d["total"].as_u64().unwrap_or(0);
            let used = d["used"].as_u64().unwrap_or(0);
            let avail = d["avail"].as_u64().unwrap_or(0);
            let pct = if total > 0 { (used as f32 / total as f32) * 100.0 } else { 0.0 };
            let r_rate = d["read_rate"].as_f64().unwrap_or(0.0);
            let w_rate = d["write_rate"].as_f64().unwrap_or(0.0);
            let r_total = d["total_read"].as_u64().unwrap_or(0);
            let w_total = d["total_write"].as_u64().unwrap_or(0);

            disks.push(DiskInfo {
                name: d["name"].as_str().unwrap_or("").to_string(),
                mount_point: d["mount"].as_str().unwrap_or("").to_string(),
                fs_type: String::new(),
                total_bytes: total,
                available_bytes: avail,
                used_bytes: used,
                usage_pct: pct,
                read_rate_bps: r_rate,
                write_rate_bps: w_rate,
                total_read_bytes: r_total,
                total_write_bytes: w_total,
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

    let mut networks = Vec::new();
    let mut total_rx_bytes = 0u64;
    let mut total_tx_bytes = 0u64;
    if let Some(arr) = parsed["networks"].as_array() {
        for n in arr {
            let name = n["name"].as_str().unwrap_or("").to_string();
            let rx = n["rx_bytes"].as_u64().unwrap_or(0);
            let tx = n["tx_bytes"].as_u64().unwrap_or(0);
            total_rx_bytes += rx;
            total_tx_bytes += tx;
            networks.push(NetInterfaceInfo {
                name,
                rx_bytes: rx,
                tx_bytes: tx,
                rx_rate_bps: 0.0,
                tx_rate_bps: 0.0,
            });
        }
    }

    let mut top_apps = Vec::new();
    if let Some(arr) = parsed.get("top_apps").and_then(|v| v.as_array()) {
        for a in arr {
            let pid = a["pid"].as_u64().unwrap_or(0) as u32;
            let name = a["name"].as_str().unwrap_or("").to_string();
            let cpu_usage = a["cpu"].as_f64().unwrap_or(0.0) as f32;
            let memory_bytes = a["mem_bytes"].as_u64().unwrap_or(0);
            let status = a["status"].as_str().unwrap_or("Running").to_string();
            top_apps.push(ProcessSensorInfo {
                pid,
                name,
                cpu_usage,
                memory_bytes,
                disk_read_bytes: 0,
                disk_written_bytes: 0,
                disk_read_rate_bps: 0.0,
                disk_write_rate_bps: 0.0,
                disk_rate_bps: 0.0,
                status,
            });
        }
    }

    Ok(SystemSensorsSnapshot {
        cpu_brand,
        cpu_cores_physical: cpu_cores,
        cpu_cores_logical: cpu_cores,
        cpu_global_usage,
        cpu_core_usages,
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
        total_disk_read_rate_bps,
        total_disk_write_rate_bps,
        networks,
        total_rx_rate_bps: 0.0,
        total_tx_rate_bps: 0.0,
        total_rx_bytes,
        total_tx_bytes,
        temp_sensors,
        top_apps,
        os_name: parsed["os_name"].as_str().unwrap_or("").to_string(),
        os_version: parsed["os_version"].as_str().unwrap_or("").to_string(),
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

    #[test]
    fn test_base_physical_disk_detection() {
        assert!(is_base_physical_disk("sda"));
        assert!(is_base_physical_disk("nvme0n1"));
        assert!(is_base_physical_disk("vda"));
        assert!(!is_base_physical_disk("sda1"));
        assert!(!is_base_physical_disk("nvme0n1p1"));
        assert!(!is_base_physical_disk("loop0"));
        assert!(!is_base_physical_disk("zram0"));
        assert!(!is_base_physical_disk("dm-0"));
    }

    #[test]
    fn test_disk_info_and_rates() {
        let mut collector = SystemSensorsCollector::new();
        let snap = collector.collect();
        // Ensure rates are valid non-negative floats
        assert!(snap.total_disk_read_rate_bps >= 0.0);
        assert!(snap.total_disk_write_rate_bps >= 0.0);
        for d in &snap.disks {
            assert!(d.read_rate_bps >= 0.0);
            assert!(d.write_rate_bps >= 0.0);
        }
    }

    #[test]
    fn test_top_apps_collection_and_cpu_power() {
        let mut collector = SystemSensorsCollector::new();
        let snap = collector.collect();

        // CPU Power watts is guaranteed to be populated
        assert!(snap.cpu_power_watts.is_some());
        let p_watts = snap.cpu_power_watts.unwrap();
        assert!(p_watts > 0.0, "CPU power usage should be positive, got {}", p_watts);

        // Top apps should have running processes
        assert!(!snap.top_apps.is_empty(), "top_apps should not be empty on a running system");
        for app in &snap.top_apps {
            assert!(app.pid > 0);
            assert!(app.cpu_usage >= 0.0);
            assert!(app.disk_read_rate_bps >= 0.0);
            assert!(app.disk_write_rate_bps >= 0.0);
            assert!(app.disk_rate_bps >= 0.0);
            assert!(!app.status.is_empty());
        }
    }
}
