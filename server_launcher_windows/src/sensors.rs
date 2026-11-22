use std::collections::HashMap;
#[cfg(not(windows))]
use std::fs;
#[cfg(not(windows))]
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};
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

#[cfg(target_os = "linux")]
pub(crate) fn parse_proc_start_marker(stat: &str) -> Option<u64> {
    stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(all(test, target_os = "linux"))]
mod process_start_marker_tests {
    #[test]
    fn parses_process_name_with_spaces_and_parentheses() {
        let stat = format!("42 (worker ) task) S {} 123456 0", "0 ".repeat(18));
        assert_eq!(super::parse_proc_start_marker(&stat), Some(123456));
        assert_eq!(super::parse_proc_start_marker("42 (incomplete) S"), None);
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProcessSensorInfo {
    pub pid: u32,
    pub start_marker: Option<u64>,
    pub name: String,
    pub cpu_usage: f32,
    pub cpu_usage_available: bool,
    pub memory_bytes: u64,
    pub disk_read_bytes: u64,
    pub disk_written_bytes: u64,
    pub disk_read_rate_bps: f64,
    pub disk_write_rate_bps: f64,
    pub disk_rate_bps: f64,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    pub net_rate_bps: f64,
    pub status: String,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryHardwareInfo {
    pub topology: String,
    pub slot_summary: String,
    pub memory_type: String,
    pub module_spec: String,
    pub form_factor: String,
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
    pub disk_rates_available: bool,
    pub total_read_bytes: u64,
    pub total_write_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub struct PhysicalDiskInfo {
    pub name: String,
    pub model: String,
    pub bus_interface: String,
    pub smart_status: String,
    pub discard_supported: Option<bool>,
    pub total_bytes: u64,
    pub is_ssd: bool,
    pub partition_count: usize,
    pub partitions: Vec<DiskPartitionInfo>,
    pub read_rate_bps: f64,
    pub write_rate_bps: f64,
    pub disk_rates_available: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DiskPartitionInfo {
    pub name: String,
    pub size_bytes: u64,
    pub partition_num: u32,
}

#[derive(Debug, Clone, Default)]
pub struct NetInterfaceInfo {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_rate_bps: f64,
    pub tx_rate_bps: f64,
    pub is_wireless: bool,
}

#[derive(Debug, Clone, Default)]
pub struct GpuSensorInfo {
    pub device_path: String,
    pub engine_loads: Vec<(String, f32)>,
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
pub struct DisplayInfo {
    pub name: String,
    pub is_connected: bool,
    pub is_enabled: bool,
    pub resolution: String,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f32,
    pub bit_depth: String,
    pub color_space: String,
    pub connector_type: String,
    pub connector_id: Option<u32>,
    pub scaling: String,
    pub vrr_support: String,
    pub manufacturer: String,
    pub model: String,
}

#[derive(Debug, Clone, Default)]
pub struct BatteryInfo {
    pub name: String,
    pub present: bool,
    pub status: String,
    pub capacity_pct: Option<f32>,
    pub power_watts: Option<f32>,
    pub voltage_volts: Option<f32>,
    pub energy_wh: Option<f32>,
    pub energy_full_wh: Option<f32>,
    pub energy_design_wh: Option<f32>,
    pub energy_estimated: bool,
    pub health_pct: Option<f32>,
    pub cycle_count: Option<u32>,
    pub technology: String,
    pub model_name: String,
    pub manufacturer: String,
}

fn reported_battery_charge(batteries: &[BatteryInfo]) -> Option<f32> {
    if batteries.len() == 1 {
        return batteries[0].capacity_pct.filter(|value| value.is_finite() && (0.0..=100.0).contains(value));
    }
    let mut weighted = 0.0_f64;
    let mut capacity = 0.0_f64;
    for battery in batteries {
        let percent = battery.capacity_pct.filter(|value| value.is_finite() && (0.0..=100.0).contains(value))? as f64;
        let full = battery.energy_full_wh.filter(|value| value.is_finite() && *value > 0.0)? as f64;
        weighted += percent * full;
        capacity += full;
    }
    (capacity > 0.0).then(|| (weighted / capacity) as f32)
}

#[derive(Debug, Clone, Default)]
pub struct PowerSupplyInfo {
    pub ac_online: Option<bool>,
    pub batteries: Vec<BatteryInfo>,
    pub total_battery_pct: Option<f32>,
    pub total_power_watts: Option<f32>,
    pub backlight_ratio: Option<f32>,
    pub usb_power_watts: Option<(f32, usize)>,
    pub kbd_lights_watts: Option<f32>,
    pub charge_limit: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct SystemSensorsSnapshot {
    // CPU
    pub cpu_brand: String,
    pub cpu_cores_physical: usize,
    pub cpu_cores_logical: usize,
    pub cpu_global_usage: f32,
    pub cpu_usage_available: bool,
    pub cpu_core_usages: Vec<Option<f32>>,
    pub cpu_frequency_mhz: u64,
    pub cpu_frequencies_mhz: Vec<u64>,
    pub cpu_power_watts: Option<f32>,
    pub cpu_tdp_watts: Option<f32>,
    pub cpu_temp: Option<f32>,
    pub cpu_tjunction_max: Option<f32>,

    // Memory (RAM & Swap)
    pub mem_total_bytes: u64,
    pub mem_used_bytes: u64,
    pub mem_free_bytes: Option<u64>,
    pub mem_available_bytes: u64,
    pub mem_dirty_bytes: Option<u64>,
    pub mem_usage_pct: f32,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_usage_pct: f32,
    pub ram_temp: Option<f32>,
    pub memory_hw: Option<MemoryHardwareInfo>,

    // GPU & Displays
    pub gpus: Vec<GpuSensorInfo>,
    pub displays: Vec<DisplayInfo>,

    // Power & Battery
    pub power: PowerSupplyInfo,

    // Storage
    pub disks: Vec<DiskInfo>,
    pub physical_disks: Vec<PhysicalDiskInfo>,
    pub storage_temp: Option<f32>,
    pub total_disk_read_rate_bps: f64,
    pub total_disk_write_rate_bps: f64,
    pub total_disk_rates_available: bool,

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
    pub process_count: Option<usize>,
    pub processes_collected: bool,
    pub process_network_collected: bool,

    // OS info
    pub os_name: String,
    pub os_version: String,

    // CPU Governor
    pub cpu_governor: Option<String>,

    // System uptime & sockets
    pub uptime_secs: u64,
    pub socket_total: usize,
    pub socket_tcp: usize,
    pub socket_udp: usize,
    pub active_sockets: Vec<crate::wifi::WifiActiveSocket>,

    pub hardware_access_unlocked: bool,
    pub hardware_access_error: Option<String>,
    #[cfg(windows)]
    pub windows_telemetry_error: Option<String>,
    pub timestamp: Option<Instant>,
}

fn counter_rates(previous: Option<(u64, u64)>, current: (u64, u64), elapsed: f64) -> (f64, f64) {
    if !elapsed.is_finite() || elapsed <= 0.0 { return (0.0, 0.0); }
    match previous {
        Some((read, written)) => (
            current.0.saturating_sub(read) as f64 / elapsed,
            current.1.saturating_sub(written) as f64 / elapsed,
        ),
        None => (0.0, 0.0),
    }
}

#[cfg(test)]
mod process_disk_rate_tests {
    #[test]
    fn handles_first_sample_interval_and_counter_reset() {
        assert_eq!(super::counter_rates(None, (200, 400), 2.0), (0.0, 0.0));
        assert_eq!(super::counter_rates(Some((100, 200)), (300, 500), 2.0), (100.0, 150.0));
        assert_eq!(super::counter_rates(Some((300, 500)), (20, 600), 2.0), (0.0, 50.0));
        assert_eq!(super::counter_rates(Some((100, 200)), (300, 500), 0.0), (0.0, 0.0));
    }
}

pub struct SystemSensorsCollector {
    sys: System,
    disks: Disks,
    networks: Networks,
    components: Components,
    cached_physical_disks: Vec<PhysicalDiskInfo>,
    cached_displays: Vec<DisplayInfo>,
    last_hardware_scan: Instant,
    prev_net_rx: HashMap<String, u64>,
    prev_net_tx: HashMap<String, u64>,
    prev_net_time: Instant,
    prev_disk_stats: HashMap<String, (u64, u64)>,
    prev_disk_time: Instant,
    prev_proc_disk: HashMap<(u32, u64), (u64, u64)>,
    prev_proc_disk_time: Option<Instant>,
    prev_proc_net: HashMap<(u32, u64), (u64, u64)>,
    prev_proc_net_time: Instant,
    protected_data: Option<serde_json::Value>,
    last_protected_poll: Instant,
    #[cfg(windows)]
    windows_hardware: SystemSensorsSnapshot,
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
            cached_physical_disks: Vec::new(),
            cached_displays: Vec::new(),
            last_hardware_scan: now - Duration::from_secs(30),
            prev_net_rx: HashMap::new(),
            prev_net_tx: HashMap::new(),
            prev_net_time: now,
            prev_disk_stats: HashMap::new(),
            prev_disk_time: now,
            prev_proc_disk: HashMap::new(),
            prev_proc_disk_time: None,
            prev_proc_net: HashMap::new(),
            prev_proc_net_time: now,
            protected_data: None,
            last_protected_poll: now,
            #[cfg(windows)]
            windows_hardware: SystemSensorsSnapshot::default(),
        }
    }

    pub fn collect(&mut self) -> SystemSensorsSnapshot {
        self.collect_with_top_apps(true, true)
    }

    #[cfg(not(windows))]
    pub fn collect_with_top_apps(&mut self, include_top_apps: bool, include_process_network: bool) -> SystemSensorsSnapshot {
        // 1. Refresh System
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.disks.refresh();
        self.networks.refresh();
        self.components.refresh();

        let now = Instant::now();
        if self.last_hardware_scan.elapsed() >= Duration::from_secs(30) {
            self.cached_physical_disks = collect_physical_disks();
            self.cached_displays = detect_displays();
            self.last_hardware_scan = now;
        }
        let elapsed_secs = (now - self.prev_net_time).as_secs_f64().max(0.1);

        // 2. CPU
        let cpu_brand = self
            .sys
            .cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_else(|| "Unknown CPU".to_string());
        let cpu_cores_physical = self.sys.physical_core_count().unwrap_or(0);
        let cpu_cores_logical = self.sys.cpus().len();
        let cpu_global_usage = self.sys.global_cpu_usage();
        #[cfg(target_os = "linux")]
        let cpu_core_usages: Vec<Option<f32>> = {
            let mut values = Vec::new();
            for cpu in self.sys.cpus() {
                if let Some(index) = linux_cpu_index(cpu.name()) {
                    if values.len() <= index { values.resize(index + 1, None); }
                    values[index] = Some(cpu.cpu_usage());
                }
            }
            values
        };
        #[cfg(not(target_os = "linux"))]
        let cpu_core_usages: Vec<Option<f32>> = self.sys.cpus().iter().map(|cpu| Some(cpu.cpu_usage())).collect();
        let (mut cpu_frequencies_mhz, cpu_power_watts, cpu_tdp_watts) = collect_cpu_frequencies_and_power();
        if cpu_frequencies_mhz.is_empty() {
            #[cfg(target_os = "linux")]
            for cpu in self.sys.cpus() {
                if let Some(index) = linux_cpu_index(cpu.name()) { insert_cpu_frequency(&mut cpu_frequencies_mhz, index, cpu.frequency()); }
            }
            #[cfg(not(target_os = "linux"))]
            { cpu_frequencies_mhz = self.sys.cpus().iter().map(|cpu| cpu.frequency()).collect(); }
        }
        let cpu_frequency_mhz = average_cpu_frequency(&cpu_frequencies_mhz);

        // 3. Memory
        let mem_total_bytes = self.sys.total_memory();
        let mem_used_bytes = self.sys.used_memory();
        let mem_free_bytes = if cfg!(target_os = "linux") { Some(self.sys.free_memory()) } else { None };
        let mem_available_bytes = self.sys.available_memory();
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
        let disk_rates = disk_stat_rates(&self.prev_disk_stats, &current_diskstats, disk_elapsed_secs);
        self.prev_disk_stats = current_diskstats.clone();

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
            let clean_dev = fs::canonicalize(&raw_name).ok()
                .and_then(|p| p.file_name().map(|name| name.to_string_lossy().into_owned()))
                .unwrap_or_else(|| raw_name.trim_start_matches("/dev/").to_string());
            let fs_type = d.file_system().to_string_lossy().to_string();
            let mount_point = d.mount_point().to_string_lossy().to_string();

            // Filter out virtual, loop, snap squashfs, and pseudo filesystems
            if fs_type == "squashfs"
                || fs_type == "tmpfs"
                || fs_type == "devtmpfs"
                || fs_type == "efivarfs"
                || fs_type == "overlay"
                || fs_type == "iso9660"
                || fs_type == "cgroup2"
                || fs_type == "pstore"
                || fs_type == "bpf"
            {
                continue;
            }
            if raw_name.starts_with("/dev/loop") || raw_name == "none" || raw_name.is_empty() {
                continue;
            }

            // Deduplicate: if this block device was already registered, keep root "/" or shortest mount point
            if let Some(existing) = disks.iter_mut().find(|existing_d: &&mut DiskInfo| existing_d.name == raw_name) {
                if mount_point == "/" || (existing.mount_point != "/" && mount_point.len() < existing.mount_point.len()) {
                    existing.mount_point = mount_point;
                }
                continue;
            }

            let (read_bytes, write_bytes) = current_diskstats.get(&clean_dev).copied().unwrap_or_default();
            let (read_rate, write_rate) = disk_rates.get(&clean_dev).copied().unwrap_or_default();

            disks.push(DiskInfo {
                name: raw_name,
                mount_point,
                fs_type,
                total_bytes: total,
                available_bytes: avail,
                used_bytes: used,
                usage_pct: pct,
                read_rate_bps: read_rate,
                write_rate_bps: write_rate,
                disk_rates_available: disk_rates.contains_key(&clean_dev),
                total_read_bytes: read_bytes,
                total_write_bytes: write_bytes,
            });
        }
        // Deduplicate or sort by mount point
        disks.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));

        let mut physical_disks = self.cached_physical_disks.clone();
        // Physical and mounted views share the rates computed above, without consuming baselines.
        let total_disk_rates_available = if !physical_disks.is_empty() {
            for disk in &mut physical_disks {
                disk.disk_rates_available = false;
                if let Some(&(read, write)) = disk_rates.get(&disk.name) {
                    disk.disk_rates_available = true;
                    disk.read_rate_bps = read;
                    disk.write_rate_bps = write;
                    total_disk_read_rate_bps += read;
                    total_disk_write_rate_bps += write;
                }
            }
            physical_disks.iter().all(|disk| disk.disk_rates_available)
        } else {
            let mut seen = std::collections::HashSet::new();
            let mut available = true;
            for disk in &disks {
                let device = fs::canonicalize(&disk.name).unwrap_or_else(|_| std::path::PathBuf::from(&disk.name));
                if seen.insert(device) {
                    available &= disk.disk_rates_available;
                    total_disk_read_rate_bps += disk.read_rate_bps;
                    total_disk_write_rate_bps += disk.write_rate_bps;
                }
            }
            !seen.is_empty() && available
        };
        self.prev_disk_time = now;

        // 5. Networks
        let mut networks = Vec::new();

        for (name, net) in &self.networks {
            let rx = net.total_received();
            let tx = net.total_transmitted();

            let prev_rx = *self.prev_net_rx.get(name).unwrap_or(&rx);
            let prev_tx = *self.prev_net_tx.get(name).unwrap_or(&tx);

            let rx_rate = (rx.saturating_sub(prev_rx) as f64) / elapsed_secs;
            let tx_rate = (tx.saturating_sub(prev_tx) as f64) / elapsed_secs;

            self.prev_net_rx.insert(name.clone(), rx);
            self.prev_net_tx.insert(name.clone(), tx);

            let is_wireless = {
                #[cfg(target_os = "linux")]
                {
                    Path::new(&format!("/sys/class/net/{}/wireless", name)).exists()
                        || Path::new(&format!("/sys/class/net/{}/phy80211", name)).exists()
                }
                #[cfg(not(target_os = "linux"))]
                false
            };

            networks.push(NetInterfaceInfo {
                name: name.clone(),
                rx_bytes: rx,
                tx_bytes: tx,
                rx_rate_bps: rx_rate,
                tx_rate_bps: tx_rate,
                is_wireless,
            });
        }
        networks.sort_by(|a, b| a.name.cmp(&b.name));
        let (total_rx_bytes, total_tx_bytes, total_rx_rate_bps, total_tx_rate_bps) = network_totals(&networks);
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

        // 7. GPU & Displays detection (NVIDIA / Intel / AMD / DRM)
        let gpus = detect_gpus(gpu_temp);
        let displays = self.cached_displays.clone();

        // 8. Power Supply & Battery
        let power = collect_power_supply();

        // 9. Top Applications & Resource Consumers
        let mut top_apps = Vec::new();
        if include_top_apps {
            let proc_net_elapsed_secs = (now - self.prev_proc_net_time).as_secs_f64().max(0.1);
            let current_proc_net = if include_process_network { collect_process_tcp_counters() } else { HashMap::new() };
            self.sys.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::All,
                sysinfo::ProcessRefreshKind::new()
                    .with_cpu()
                    .with_memory()
                    .with_disk_usage()
                    .with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
            );
            let proc_disk_now = Instant::now();
            let proc_disk_elapsed = self.prev_proc_disk_time.map(|time| proc_disk_now.duration_since(time).as_secs_f64()).unwrap_or(0.0);
            let mut current_proc_disk = HashMap::new();
            let mut current_identified_net = HashMap::new();

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
                let read_b = disk_usage.total_read_bytes;
                let written_b = disk_usage.total_written_bytes;
                let identity = (pid_u32, proc.start_time());
                current_proc_disk.insert(identity, (read_b, written_b));
                let (r_rate, w_rate) = counter_rates(self.prev_proc_disk.get(&identity).copied(), (read_b, written_b), proc_disk_elapsed);
                let disk_rate_bps = r_rate + w_rate;

                let start_marker = {
                    #[cfg(target_os = "linux")]
                    { fs::read_to_string(format!("/proc/{pid_u32}/stat")).ok().and_then(|stat| parse_proc_start_marker(&stat)) }
                    #[cfg(not(target_os = "linux"))]
                    { Some(proc.start_time()) }
                };
                let (proc_net_rx, proc_net_tx) = current_proc_net.get(&pid_u32).copied().unwrap_or_default();
                let previous = start_marker.filter(|_| include_process_network).and_then(|start| {
                    current_identified_net.insert((pid_u32, start), (proc_net_rx, proc_net_tx));
                    self.prev_proc_net.get(&(pid_u32, start)).copied()
                });
                let (rx_rate, tx_rate) = counter_rates(previous, (proc_net_rx, proc_net_tx), proc_net_elapsed_secs);
                let net_rate_bps = rx_rate + tx_rate;

                let status = format!("{:?}", proc.status());

                top_apps.push(ProcessSensorInfo {
                    pid: pid_u32,
                    start_marker,
                    name,
                    cpu_usage,
                    cpu_usage_available: true,
                    memory_bytes,
                    disk_read_bytes: read_b,
                    disk_written_bytes: written_b,
                    disk_read_rate_bps: r_rate,
                    disk_write_rate_bps: w_rate,
                    disk_rate_bps,
                    net_rx_bytes: proc_net_rx,
                    net_tx_bytes: proc_net_tx,
                    net_rate_bps,
                    status,
                });
            }
            self.prev_proc_disk = current_proc_disk;
            self.prev_proc_disk_time = Some(proc_disk_now);
            self.prev_proc_net = current_identified_net;
            self.prev_proc_net_time = now;

            // Sort by CPU usage descending by default, then network usage, then memory
            let compare_apps = |a: &ProcessSensorInfo, b: &ProcessSensorInfo| {
                b.cpu_usage
                    .partial_cmp(&a.cpu_usage)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| (b.net_rx_bytes + b.net_tx_bytes).cmp(&(a.net_rx_bytes + a.net_tx_bytes)))
                    .then_with(|| b.memory_bytes.cmp(&a.memory_bytes))
            };
            top_apps.sort_unstable_by(compare_apps);
        }

        let cpu_governor = {
            #[cfg(target_os = "linux")]
            {
                fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
                    .ok()
                    .map(|s| s.trim().to_string())
            }
            #[cfg(not(target_os = "linux"))]
            None
        };
        let uptime_secs = {
            #[cfg(target_os = "linux")]
            {
                fs::read_to_string("/proc/uptime")
                    .ok()
                    .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()))
                    .map(|s| s as u64)
                    .unwrap_or(0)
            }
            #[cfg(not(target_os = "linux"))]
            0
        };
        let (socket_total, socket_tcp, socket_udp) = {
            let mut total = 0;
            let mut tcp = 0;
            let mut udp = 0;
            #[cfg(target_os = "linux")]
            if let Ok(content) = fs::read_to_string("/proc/net/sockstat") {
                for line in content.lines() {
                    if line.starts_with("sockets: used") {
                        if let Some(val) = line.split_whitespace().nth(2) {
                            total = val.parse::<usize>().unwrap_or(0);
                        }
                    } else if line.starts_with("TCP: inuse") {
                        if let Some(val) = line.split_whitespace().nth(2) {
                            tcp = val.parse::<usize>().unwrap_or(0);
                        }
                    } else if line.starts_with("UDP: inuse") {
                        if let Some(val) = line.split_whitespace().nth(2) {
                            udp = val.parse::<usize>().unwrap_or(0);
                        }
                    }
                }
            }
            (total.max(tcp + udp), tcp, udp)
        };

        let cpu_tjunction_max = detect_hardware_cpu_tjunction_max(&temp_sensors);
        let memory_hw = None;

        let mut mem_dirty_bytes = None;
        #[cfg(target_os = "linux")]
        {
            if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
                let mut dirty_kb = 0u64;
                let mut writeback_kb = 0u64;
                let mut found = false;
                for line in meminfo.lines() {
                    if line.starts_with("Dirty:") {
                        if let Some(val) = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()) {
                            dirty_kb = val;
                            found = true;
                        }
                    } else if line.starts_with("Writeback:") {
                        if let Some(val) = line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()) {
                            writeback_kb = val;
                            found = true;
                        }
                    }
                }
                if found {
                    mem_dirty_bytes = Some((dirty_kb + writeback_kb) * 1024);
                }
            }
        }

        SystemSensorsSnapshot {
            cpu_brand,
            cpu_cores_physical,
            cpu_cores_logical,
            cpu_global_usage,
            cpu_usage_available: true,
            cpu_core_usages,
            cpu_frequency_mhz,
            cpu_frequencies_mhz,
            cpu_power_watts,
            cpu_tdp_watts,
            cpu_temp,
            cpu_tjunction_max,
            mem_total_bytes,
            mem_used_bytes,
            mem_free_bytes,
            mem_available_bytes,
            mem_dirty_bytes,
            mem_usage_pct,
            swap_total_bytes,
            swap_used_bytes,
            swap_usage_pct,
            ram_temp,
            memory_hw,
            gpus,
            displays,
            power,
            disks,
            physical_disks,
            storage_temp,
            total_disk_read_rate_bps,
            total_disk_write_rate_bps,
            total_disk_rates_available,
            networks,
            total_rx_rate_bps,
            total_tx_rate_bps,
            total_rx_bytes,
            total_tx_bytes,
            temp_sensors,
            process_count: include_top_apps.then_some(top_apps.len()),
            top_apps,
            processes_collected: include_top_apps,
            process_network_collected: include_top_apps && include_process_network,
            os_name: System::name().unwrap_or_default(),
            os_version: System::os_version().unwrap_or_default(),
            cpu_governor,
            uptime_secs,
            socket_total,
            socket_tcp,
            socket_udp,
            active_sockets: Vec::new(),
            hardware_access_unlocked: false,
            hardware_access_error: None,
            #[cfg(windows)]
            windows_telemetry_error: None,
            timestamp: Some(now),
        }
    }
}

#[cfg_attr(windows, allow(unused_mut))]
fn disk_stat_rates(previous: &HashMap<String, (u64, u64)>, current: &HashMap<String, (u64, u64)>, elapsed: f64) -> HashMap<String, (f64, f64)> {
    current.iter().filter_map(|(device, &totals)| {
        let baseline = previous.get(device).copied()?;
        if !elapsed.is_finite() || elapsed <= 0.0 || totals.0 < baseline.0 || totals.1 < baseline.1 { return None; }
        Some((device.clone(), counter_rates(Some(baseline), totals, elapsed)))
    }).collect()
}

#[cfg(test)]
mod disk_stat_rate_tests {
    use std::collections::HashMap;

    #[test]
    fn rates_keep_distinct_device_baselines_and_handle_new_devices() {
        let previous = HashMap::from([("sda".into(), (100, 200)), ("sda1".into(), (50, 80)), ("sdaa".into(), (1, 1))]);
        let current = HashMap::from([("sda".into(), (300, 400)), ("sda1".into(), (150, 180)), ("sdaa".into(), (21, 11)), ("sdb".into(), (900, 900))]);
        let rates = super::disk_stat_rates(&previous, &current, 2.0);
        assert_eq!(rates["sda"], (100.0, 100.0));
        assert_eq!(rates["sda1"], (50.0, 50.0));
        assert_eq!(rates["sdaa"], (10.0, 5.0));
        assert!(!rates.contains_key("sdb"));
        assert!(!super::disk_stat_rates(&current, &previous, 2.0).contains_key("sda"));
        assert_eq!(super::disk_stat_rates(&current, &current, 2.0)["sda"], (0.0, 0.0));
    }
}

fn parse_cpu_usage(value: &serde_json::Value) -> Option<f32> {
    value.as_f64().filter(|value| value.is_finite() && (0.0..=100.0).contains(value)).map(|value| value as f32)
}

fn parse_process_cpu_usage(value: &serde_json::Value) -> Option<f32> {
    value.as_f64().filter(|value| value.is_finite() && *value >= 0.0 && *value <= f32::MAX as f64).map(|value| value as f32)
}

fn parse_cpu_core_usages(value: &serde_json::Value) -> Vec<Option<f32>> {
    value.as_array().into_iter().flatten().map(parse_cpu_usage).collect()
}

#[cfg(test)]
mod cpu_core_reading_tests {
    #[test]
    fn process_cpu_preserves_multicore_and_unavailable_values() {
        assert_eq!(super::parse_process_cpu_usage(&serde_json::json!(150.0)), Some(150.0));
        assert_eq!(super::parse_process_cpu_usage(&serde_json::json!(0.0)), Some(0.0));
        assert_eq!(super::parse_process_cpu_usage(&serde_json::Value::Null), None);
        assert_eq!(super::parse_process_cpu_usage(&serde_json::json!(-1.0)), None);
    }

    #[test]
    fn global_cpu_distinguishes_zero_from_unavailable() {
        assert_eq!(super::parse_cpu_usage(&serde_json::json!(0.0)), Some(0.0));
        assert_eq!(super::parse_cpu_usage(&serde_json::Value::Null), None);
        assert_eq!(super::parse_cpu_usage(&serde_json::json!(101.0)), None);
    }

    #[test]
    fn missing_core_data_is_not_replaced_by_global_usage() {
        assert!(super::parse_cpu_core_usages(&serde_json::Value::Null).is_empty());
        assert!(super::parse_cpu_core_usages(&serde_json::json!([])).is_empty());
        assert_eq!(super::parse_cpu_core_usages(&serde_json::json!([0.0, 42.5, 150.0])), vec![Some(0.0), Some(42.5), None]);
    }
}

fn network_totals(networks: &[NetInterfaceInfo]) -> (u64, u64, f64, f64) {
    networks.iter().fold((0_u64, 0_u64, 0.0, 0.0), |(rx, tx, rx_rate, tx_rate), net| (
        rx.saturating_add(net.rx_bytes), tx.saturating_add(net.tx_bytes),
        rx_rate + net.rx_rate_bps, tx_rate + net.tx_rate_bps,
    ))
}

#[cfg(test)]
mod network_total_tests {
    use super::NetInterfaceInfo;
    #[test]
    fn counters_and_rates_include_the_same_interfaces() {
        let networks = vec![
            NetInterfaceInfo { name: "wlan0".into(), rx_bytes: 100, tx_bytes: 200, rx_rate_bps: 10.0, tx_rate_bps: 20.0, ..Default::default() },
            NetInterfaceInfo { name: "lo".into(), rx_bytes: 30, tx_bytes: 30, rx_rate_bps: 3.0, tx_rate_bps: 3.0, ..Default::default() },
            NetInterfaceInfo { name: "vpn0".into(), rx_bytes: 20, tx_bytes: 10, rx_rate_bps: 2.0, tx_rate_bps: 1.0, ..Default::default() },
        ];
        assert_eq!(super::network_totals(&networks), (150, 240, 15.0, 24.0));
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
    if dev.starts_with("loop")
        || dev.starts_with("ram")
        || dev.starts_with("zram")
        || dev.starts_with("dm-")
        || dev.starts_with("nbd")
        || dev.starts_with("sr")
    {
        return false;
    }
    if dev.starts_with("sd") || dev.starts_with("vd") || dev.starts_with("hd") || dev.starts_with("xvd") {
        return !dev.chars().last().map(|c| c.is_ascii_digit()).unwrap_or(false);
    }
    if dev.starts_with("nvme") {
        let rest = if dev.len() > 4 { &dev[4..] } else { "" };
        return rest.contains('n') && !rest.contains('p');
    }
    if dev.starts_with("mmcblk") {
        let rest = if dev.len() > 6 { &dev[6..] } else { "" };
        return !rest.contains('p') && !rest.contains("boot");
    }
    if dev.starts_with("md") {
        return !dev.contains('p');
    }
    true
}

#[cfg_attr(windows, allow(unused_mut))]
pub fn collect_physical_disks() -> Vec<PhysicalDiskInfo> {
    let mut physical_disks = Vec::new();
    #[cfg(target_os = "linux")]
    if let Ok(entries) = fs::read_dir("/sys/block") {
        let mut dev_names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| is_base_physical_disk(name))
            .collect();
        dev_names.sort();

        for dev in dev_names {
            let p = format!("/sys/block/{}", dev);
            let size_bytes = fs::read_to_string(format!("{}/size", p))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(0)
                .saturating_mul(512);

            if size_bytes == 0 {
                continue;
            }

            let mut model = "Unknown Drive".to_string();
            for model_path in &[format!("{}/device/model", p), format!("{}/device/name", p)] {
                if let Ok(m) = fs::read_to_string(model_path) {
                    let trimmed = m.trim();
                    if !trimmed.is_empty() {
                        model = trimmed.to_string();
                        break;
                    }
                }
            }

            let is_ssd = fs::read_to_string(format!("{}/queue/rotational", p))
                .ok()
                .map(|s| s.trim() == "0")
                .unwrap_or_else(|| dev.starts_with("nvme"));

            let mut partitions = Vec::new();
            if let Ok(sub_entries) = fs::read_dir(&p) {
                let mut p_names: Vec<String> = sub_entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect();
                p_names.sort();

                for p_name in p_names {
                    let part_file = format!("{}/{}/partition", p, p_name);
                    if let Ok(num_str) = fs::read_to_string(&part_file) {
                        let num = num_str.trim().parse::<u32>().unwrap_or(0);
                        let part_size = fs::read_to_string(format!("{}/{}/size", p, p_name))
                            .ok()
                            .and_then(|s| s.trim().parse::<u64>().ok())
                            .unwrap_or(0)
                            .saturating_mul(512);
                        partitions.push(DiskPartitionInfo {
                            name: p_name,
                            size_bytes: part_size,
                            partition_num: num,
                        });
                    }
                }
            }
            partitions.sort_by_key(|p| p.partition_num);

            let bus_interface = Path::new(&format!("{}/device/subsystem", p))
                .canonicalize()
                .ok()
                .and_then(|path| path.file_name().map(|name| name.to_string_lossy().to_string()))
                .map(|bus| match bus.as_str() {
                    "nvme" => "NVMe".to_string(),
                    "scsi" => "SCSI".to_string(),
                    "virtio" => "VirtIO".to_string(),
                    other => other.to_uppercase(),
                })
                .unwrap_or_default();
            let discard_supported = fs::read_to_string(format!("{}/queue/discard_max_bytes", p))
                .ok()
                .and_then(|value| value.trim().parse::<u64>().ok())
                .map(|limit| limit > 0);
            let smart_status = Command::new("timeout")
                .args(["3s", "smartctl", "-H", "-j", "-n", "standby", &format!("/dev/{dev}")])
                .output()
                .ok()
                .and_then(|output| serde_json::from_slice::<serde_json::Value>(&output.stdout).ok())
                .and_then(|report| {
                    report["smart_status"]["passed"]
                        .as_bool()
                        .or_else(|| report["nvme_smart_health_information_log"]["critical_warning"].as_u64().map(|warning| warning == 0))
                })
                .map(|passed| if passed { "PASSED" } else { "WARNING" }.to_string())
                .unwrap_or_default();

            let partition_count = partitions.len();
            physical_disks.push(PhysicalDiskInfo {
                name: dev,
                model,
                bus_interface,
                smart_status,
                discard_supported,
                total_bytes: size_bytes,
                is_ssd,
                partition_count,
                partitions,
                read_rate_bps: 0.0,
                write_rate_bps: 0.0,
                disk_rates_available: false,
            });
        }
    }
    physical_disks
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

#[cfg_attr(windows, allow(unused_mut))]
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
                                        let temp = if raw_val.abs() > 200.0 { raw_val / 1000.0 } else { raw_val };
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
                                    let temp = if raw.abs() > 200.0 { raw / 1000.0 } else { raw };
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

/// Dynamically detects the CPU Tjunction Max / critical thermal limit for the specific PC hardware.
/// Reads from hwmon critical limit sensors (coretemp, k10temp, zenpower, cpu_thermal), thermal zone trip points,
/// or detected CPU temp sensor metadata.
pub fn detect_hardware_cpu_tjunction_max(temp_sensors: &[TempSensor]) -> Option<f32> {
    // 1. First inspect identified CPU sensors (prioritize Package / CPU / coretemp)
    for s in temp_sensors {
        if s.category == SensorCategory::Cpu && (s.name.contains("Package") || s.name == "CPU" || s.chip.contains("coretemp")) {
            if let Some(c) = s.crit.or(s.max) {
                if c >= 60.0 && c <= 125.0 {
                    return Some(c);
                }
            }
        }
    }
    // 2. Check any CPU sensor with a valid critical or max threshold
    for s in temp_sensors {
        if s.category == SensorCategory::Cpu {
            if let Some(c) = s.crit.or(s.max) {
                if c >= 60.0 && c <= 125.0 {
                    return Some(c);
                }
            }
        }
    }

    // 3. Directly query Linux hwmon sysfs for CPU drivers
    #[cfg(target_os = "linux")]
    {
        let hwmon_dir = Path::new("/sys/class/hwmon");
        if hwmon_dir.exists() {
            if let Ok(entries) = fs::read_dir(hwmon_dir) {
                let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
                paths.sort();

                for p in paths {
                    let chip_name = fs::read_to_string(p.join("name"))
                        .map(|s| s.trim().to_lowercase())
                        .unwrap_or_default();

                    let is_cpu_chip = chip_name.contains("coretemp")
                        || chip_name.contains("k10temp")
                        || chip_name.contains("zenpower")
                        || chip_name.contains("cpu")
                        || chip_name.contains("soc")
                        || chip_name.contains("fam15h");

                    if is_cpu_chip {
                        if let Ok(files) = fs::read_dir(&p) {
                            let mut flist: Vec<_> = files.flatten().map(|e| e.path()).collect();
                            flist.sort();

                            // Prioritize temp1_crit then any temp*_crit, then temp*_max
                            let mut crit_files: Vec<_> = flist.iter()
                                .filter(|f| {
                                    let name = f.file_name().unwrap_or_default().to_string_lossy();
                                    name.starts_with("temp") && (name.ends_with("_crit") || name.ends_with("_max"))
                                })
                                .collect();

                            crit_files.sort_by_key(|f| {
                                let name = f.file_name().unwrap_or_default().to_string_lossy();
                                if name.contains("temp1_crit") {
                                    0
                                } else if name.ends_with("_crit") {
                                    1
                                } else {
                                    2
                                }
                            });

                            for cf in crit_files {
                                if let Ok(content) = fs::read_to_string(cf) {
                                    if let Ok(raw) = content.trim().parse::<f32>() {
                                        let val = if raw.abs() > 200.0 { raw / 1000.0 } else { raw };
                                        if val >= 60.0 && val <= 125.0 {
                                            return Some(val);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 4. Check Linux thermal zones for critical trip points
        let thermal_dir = Path::new("/sys/class/thermal");
        if thermal_dir.exists() {
            if let Ok(entries) = fs::read_dir(thermal_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    if name.starts_with("thermal_zone") {
                        let type_str = fs::read_to_string(path.join("type")).unwrap_or_default().to_lowercase();
                        if type_str.contains("cpu") || type_str.contains("x86_pkg") || type_str.contains("soc") {
                            for trip in 0..8 {
                                let trip_type_path = path.join(format!("trip_point_{}_type", trip));
                                let trip_temp_path = path.join(format!("trip_point_{}_temp", trip));
                                if let Ok(ttype) = fs::read_to_string(trip_type_path) {
                                    if ttype.trim().eq_ignore_ascii_case("critical") {
                                        if let Ok(tstr) = fs::read_to_string(trip_temp_path) {
                                            if let Ok(raw) = tstr.trim().parse::<f32>() {
                                                let val = if raw.abs() > 200.0 { raw / 1000.0 } else { raw };
                                                if val >= 60.0 && val <= 125.0 {
                                                    return Some(val);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

const DROP_CACHES_COMMAND: &str = "sync && printf 3 > /proc/sys/vm/drop_caches";

/// Drop clean Linux page cache, dentries, and inodes after syncing dirty data.
pub fn drop_linux_caches(host: Option<&crate::config::SshRemoteHost>, password: Option<&str>) -> Result<String, String> {
    if let Some(host) = host {
        crate::systemd::remote_script(host, DROP_CACHES_COMMAND, false, true, password)?;
    } else {
        #[cfg(not(target_os = "linux"))]
        return Err("Cache dropping is only available on Linux hosts.".into());
        #[cfg(target_os = "linux")]
        crate::systemd::run_local("sh", &["-c", DROP_CACHES_COMMAND], true, password)?;
    }
    Ok("Clean page cache, dentries and inodes dropped.".into())
}

#[cfg(all(test, target_os = "linux"))]
mod drop_cache_tests {
    #[test]
    fn failed_write_is_reported() {
        let script = super::DROP_CACHES_COMMAND.replace("/proc/sys/vm/drop_caches", "/dev/null/nonexistent");
        let result = std::process::Command::new("sh").args(["-c", &script]).status().unwrap();
        assert!(!result.success());
    }
}

/// Detects if the current system has any active physical fan controllers in hwmon.
pub fn detect_active_fans() -> bool {
    #[cfg(target_os = "linux")]
    {
        let hwmon_dir = Path::new("/sys/class/hwmon");
        if hwmon_dir.exists() {
            if let Ok(entries) = fs::read_dir(hwmon_dir) {
                for entry in entries.flatten() {
                    if let Ok(files) = fs::read_dir(entry.path()) {
                        for f in files.flatten() {
                            let name = f.file_name().to_string_lossy().to_string();
                            if name.starts_with("fan") && name.ends_with("_input") {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

#[cfg_attr(windows, allow(unused_mut))]
fn average_cpu_frequency(frequencies: &[u64]) -> u64 {
    let (sum, count) = frequencies.iter().filter(|value| **value > 0)
        .fold((0_u128, 0_u128), |(sum, count), value| (sum + *value as u128, count + 1));
    if count == 0 { 0 } else { (sum / count) as u64 }
}

#[cfg(any(target_os = "linux", test))]
fn linux_cpu_index(name: &str) -> Option<usize> {
    let suffix = name.strip_prefix("cpu")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) { return None; }
    suffix.parse().ok()
}

#[cfg(any(target_os = "linux", test))]
fn insert_cpu_frequency(frequencies: &mut Vec<u64>, index: usize, mhz: u64) {
    if mhz == 0 { return; }
    if frequencies.len() <= index { frequencies.resize(index + 1, 0); }
    if frequencies[index] == 0 { frequencies[index] = mhz; }
}

#[cfg(test)]
mod cpu_frequency_tests {
    #[test]
    fn processor_slots_and_average_ignore_missing_readings() {
        let mut frequencies = Vec::new();
        super::insert_cpu_frequency(&mut frequencies, 10, 3000);
        super::insert_cpu_frequency(&mut frequencies, 2, 2000);
        super::insert_cpu_frequency(&mut frequencies, 0, 1000);
        super::insert_cpu_frequency(&mut frequencies, 2, 9000);
        assert_eq!(frequencies[2], 2000);
        assert_eq!(frequencies[1], 0);
        assert_eq!(frequencies[10], 3000);
        assert_eq!(super::average_cpu_frequency(&frequencies), 2000);
        assert_eq!(super::average_cpu_frequency(&[0, 0]), 0);
        assert_eq!(super::linux_cpu_index("cpu10"), Some(10));
        assert_eq!(super::linux_cpu_index("cpu"), None);
        assert_eq!(super::linux_cpu_index("cpuidle"), None);
    }
}

pub fn collect_cpu_frequencies_and_power() -> (Vec<u64>, Option<f32>, Option<f32>) {
    let mut freqs = Vec::new();
    let mut cpu_power_watts = None;
    let mut cpu_tdp_watts = None;

    #[cfg(target_os = "linux")]
    {
        // Keep Linux CPU numbers as indices, even when a frequency file is absent.
        if let Ok(entries) = fs::read_dir("/sys/devices/system/cpu") {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue; };
                let Some(index) = linux_cpu_index(name) else { continue; };
                let cpufreq = entry.path().join("cpufreq");
                let mhz = ["scaling_cur_freq", "cpuinfo_cur_freq"].iter()
                    .filter_map(|file| fs::read_to_string(cpufreq.join(file)).ok())
                    .filter_map(|value| value.trim().parse::<u64>().ok())
                    .map(|khz| khz / 1000).find(|mhz| *mhz > 0);
                if let Some(mhz) = mhz { insert_cpu_frequency(&mut freqs, index, mhz); }
            }
        }

        // Fill missing CPU slots from cpuinfo when it reports per-processor MHz.
        if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
            let mut processor = None;
            for line in cpuinfo.lines() {
                let Some((key, value)) = line.split_once(':') else { continue; };
                match key.trim() {
                    "processor" => processor = value.trim().parse::<usize>().ok(),
                    "cpu MHz" => {
                        if let (Some(index), Ok(mhz)) = (processor, value.trim().parse::<f64>()) {
                            if mhz.is_finite() && mhz > 0.0 && mhz <= u64::MAX as f64 {
                                insert_cpu_frequency(&mut freqs, index, mhz as u64);
                            }
                        }
                    }
                    _ => {}
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

#[cfg(any(target_os = "linux", test))]
fn combine_ac_states(states: &[Option<bool>]) -> Option<bool> {
    if states.contains(&Some(true)) { Some(true) }
    else if !states.is_empty() && states.iter().all(|state| *state == Some(false)) { Some(false) }
    else { None }
}

#[cfg(test)]
mod ac_state_tests {
    #[test]
    fn combines_connected_disconnected_and_unknown_adapters() {
        assert_eq!(super::combine_ac_states(&[]), None);
        assert_eq!(super::combine_ac_states(&[Some(false), Some(true)]), Some(true));
        assert_eq!(super::combine_ac_states(&[Some(false), Some(false)]), Some(false));
        assert_eq!(super::combine_ac_states(&[Some(false), None]), None);
        assert_eq!(super::combine_ac_states(&[Some(true), None]), Some(true));
    }
}

#[cfg(windows)]
pub fn collect_power_supply() -> PowerSupplyInfo { collect_windows_hardware(false).ok().map(|data| parse_windows_hardware(&data).power).unwrap_or_default() }

#[cfg(not(windows))]
pub fn collect_power_supply() -> PowerSupplyInfo {
    #[cfg(target_os = "linux")]
    let ac_online;
    #[cfg(not(target_os = "linux"))]
    let ac_online = None;
    let mut batteries = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let mut ac_states = Vec::new();
        let ps_dir = Path::new("/sys/class/power_supply");
        if ps_dir.exists() {
            if let Ok(entries) = fs::read_dir(ps_dir) {
                let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
                paths.sort();

                for p in paths {
                    let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let type_str = fs::read_to_string(p.join("type")).unwrap_or_default().trim().to_lowercase();

                    if type_str == "mains" || type_str == "ac" || type_str == "adp" || (type_str != "battery" && name.to_lowercase().starts_with("ac")) {
                        let state = fs::read_to_string(p.join("online")).ok().and_then(|value| match value.trim() {
                            "1" => Some(true),
                            "0" => Some(false),
                            _ => None,
                        });
                        ac_states.push(state);
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
                            .filter(|value| value.is_finite() && (0.0..=100.0).contains(value));

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

                        let nominal_voltage = ["voltage_min_design", "voltage_design", "voltage_nominal"].iter()
                            .filter_map(|name| fs::read_to_string(p.join(name)).ok())
                            .filter_map(|value| value.trim().parse::<f32>().ok())
                            .find(|value| value.is_finite() && *value > 0.0)
                            .map(|value| value / 1_000_000.0);
                        let mut energy_estimated = false;
                        let mut energy_wh = fs::read_to_string(p.join("energy_now"))
                            .ok()
                            .and_then(|s| s.trim().parse::<f32>().ok())
                            .map(|uwh| uwh / 1_000_000.0);

                        if energy_wh.is_none() {
                            if let (Some(v), Ok(c_str)) = (nominal_voltage, fs::read_to_string(p.join("charge_now"))) {
                                if let Ok(uah) = c_str.trim().parse::<f32>() {
                                    energy_wh = Some((uah / 1_000_000.0) * v);
                                    energy_estimated = true;
                                }
                            }
                        }

                        let mut energy_full_wh = fs::read_to_string(p.join("energy_full"))
                            .ok()
                            .and_then(|s| s.trim().parse::<f32>().ok())
                            .map(|uwh| uwh / 1_000_000.0);

                        if energy_full_wh.is_none() {
                            if let (Some(v), Ok(c_str)) = (nominal_voltage, fs::read_to_string(p.join("charge_full"))) {
                                if let Ok(uah) = c_str.trim().parse::<f32>() {
                                    energy_full_wh = Some((uah / 1_000_000.0) * v);
                                    energy_estimated = true;
                                }
                            }
                        }

                        let mut energy_design_wh = fs::read_to_string(p.join("energy_full_design"))
                            .ok()
                            .and_then(|s| s.trim().parse::<f32>().ok())
                            .map(|uwh| uwh / 1_000_000.0);

                        if energy_design_wh.is_none() {
                            if let (Some(v), Ok(c_str)) = (nominal_voltage, fs::read_to_string(p.join("charge_full_design"))) {
                                if let Ok(uah) = c_str.trim().parse::<f32>() {
                                    energy_design_wh = Some((uah / 1_000_000.0) * v);
                                    energy_estimated = true;
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
                            .and_then(|s| s.trim().parse::<u32>().ok());

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
                            energy_estimated,
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
        ac_online = combine_ac_states(&ac_states);
    }

    let total_battery_pct = reported_battery_charge(&batteries);

    let total_power_watts = if batteries.is_empty() { None } else {
        batteries.iter().map(|battery| battery.power_watts).sum::<Option<f32>>()
    };

    #[cfg(target_os = "linux")]
    let backlight_ratio = {
        let mut ratio = None;
        if let Ok(entries) = fs::read_dir("/sys/class/backlight") {
            for entry in entries.flatten() {
                let path = entry.path();
                let actual = fs::read_to_string(path.join("actual_brightness"))
                    .or_else(|_| fs::read_to_string(path.join("brightness")));
                let max = fs::read_to_string(path.join("max_brightness"));
                if let (Ok(a_str), Ok(m_str)) = (actual, max) {
                    if let (Ok(a_val), Ok(m_val)) = (a_str.trim().parse::<f32>(), m_str.trim().parse::<f32>()) {
                        if m_val > 0.0 {
                            ratio = Some((a_val / m_val).clamp(0.0, 1.0));
                            break;
                        }
                    }
                }
            }
        }
        ratio
    };
    #[cfg(not(target_os = "linux"))]
    let backlight_ratio = None;

    let usb_power_watts = None;
    let kbd_lights_watts = None;

    #[cfg(target_os = "linux")]
    let charge_limit = if batteries.len() == 1 {
        fs::read_to_string(format!("/sys/class/power_supply/{}/charge_control_end_threshold", batteries[0].name))
            .ok().and_then(|s| s.trim().parse::<u32>().ok()).filter(|v| (1..=100).contains(v))
    } else { None };
    #[cfg(not(target_os = "linux"))]
    let charge_limit = None;

    PowerSupplyInfo {
        ac_online,
        batteries,
        total_battery_pct,
        total_power_watts,
        backlight_ratio,
        usb_power_watts,
        kbd_lights_watts,
        charge_limit,
    }
}

static HAS_NVIDIA_SMI: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn detect_gpus(known_temp: Option<f32>) -> Vec<GpuSensorInfo> {
    #[cfg(windows)]
    let _ = known_temp;
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

                        gpus.push(GpuSensorInfo { device_path: String::new(), engine_loads: Vec::new(),
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

    // 2. Check Linux DRM cards (Intel, AMD, NVIDIA Nouveau, VirtIO, etc.)
    #[cfg(target_os = "linux")]
    {
        let drm_dir = Path::new("/sys/class/drm");
        if drm_dir.exists() {
            if let Ok(entries) = fs::read_dir(drm_dir) {
                let mut card_entries: Vec<_> = entries
                    .flatten()
                    .filter(|e| {
                        let fname = e.file_name().to_string_lossy().to_string();
                        fname.starts_with("card") && !fname.contains('-')
                    })
                    .collect();
                card_entries.sort_by_key(|e| e.file_name());

                for e in card_entries {
                    let dev = e.path().join("device");
                    let vendor = fs::read_to_string(dev.join("vendor")).unwrap_or_default().trim().to_lowercase();
                    
                    // If nvidia-smi already populated NVIDIA cards, avoid duplicating an NVIDIA card from DRM
                    if vendor == "0x10de" && !gpus.is_empty() && gpus.iter().any(|g| g.driver.contains("NVIDIA")) {
                        continue;
                    }

                    let driver_name = fs::read_link(dev.join("driver"))
                        .ok()
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                        .unwrap_or_default();

                    let device_id = fs::read_to_string(dev.join("device")).unwrap_or_default().trim().to_lowercase();

                    let vendor_name = match vendor.as_str() {
                        "0x8086" => match device_id.as_str() {
                            "0x3184" => "Intel UHD 605",
                            "0x3185" => "Intel UHD 600",
                            "0x3e92" | "0x3e98" | "0x3e9b" => "Intel UHD 630",
                            "0x5912" | "0x5916" | "0x5917" => "Intel HD 620",
                            "0x9a49" | "0x9a40" => "Intel Iris Xe",
                            _ => if driver_name == "xe" { "Intel Arc / Xe" } else { "Intel UHD Graphics" },
                        },
                        "0x1002" => "AMD Radeon",
                        "0x10de" => "NVIDIA GPU",
                        "0x1af4" => "VirtIO GPU",
                        "0x15ad" => "VMware SVGA",
                        "0x1ab8" => "Parallels GPU",
                        "0x1234" => "QEMU VGA",
                        "0x1a03" => "ASPEED BMC",
                        "0x13d3" | "0x14e4" => "Broadcom GPU",
                        "0x13b5" => "ARM Mali",
                        "0x5143" => "Adreno GPU",
                        _ => {
                            if !driver_name.is_empty() {
                                match driver_name.as_str() {
                                    "i915" | "xe" => "Intel Graphics",
                                    "amdgpu" | "radeon" => "AMD Radeon",
                                    "nouveau" => "NVIDIA Nouveau",
                                    "virtio-pci" | "virtio_gpu" => "VirtIO GPU",
                                    "v3d" | "vc4" => "Raspberry Pi GPU",
                                    "panfrost" | "lima" => "ARM Mali",
                                    _ => "Integrated GPU",
                                }
                            } else {
                                "Integrated GPU"
                            }
                        }
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

                    // Clock frequency is not a utilization counter.
                    let load_pct = fs::read_to_string(dev.join("gpu_busy_percent"))
                        .ok().and_then(|value| value.trim().parse::<f32>().ok())
                        .filter(|value| value.is_finite() && (0.0..=100.0).contains(value));

                    // DRM drivers expose VRAM counters when the device has a
                    // queryable local-memory pool (for example amdgpu).
                    let memory_mb = |names: &[&str]| {
                        names.iter().find_map(|name| {
                            fs::read_to_string(dev.join(name)).ok()?.trim().parse::<u64>().ok()
                        }).map(|bytes| bytes / (1024 * 1024))
                    };
                    let mem_used_mb = memory_mb(&["mem_info_vram_used", "mem_info_lmem_used"]);
                    let mem_total_mb = memory_mb(&["mem_info_vram_total", "mem_info_lmem_total"]);

                    let mut core_clock = freq_mhz.map(|f| f as u32);
                    let mut mem_clock = None;
                    let mut power_watts = None;
                    let mut card_temp = None;

                    let hwmon_p = dev.join("hwmon");
                    if hwmon_p.exists() {
                        if let Ok(entries) = fs::read_dir(&hwmon_p) {
                            for hwe in entries.flatten() {
                                let hp = hwe.path();
                                if card_temp.is_none() {
                                    if let Ok(s) = fs::read_to_string(hp.join("temp1_input")).or_else(|_| fs::read_to_string(hp.join("temp2_input"))) {
                                        if let Ok(raw) = s.trim().parse::<f32>() {
                                            let t = if raw.abs() > 200.0 { raw / 1000.0 } else { raw };
                                            if t >= -40.0 && t <= 150.0 {
                                                card_temp = Some(t);
                                            }
                                        }
                                    }
                                }
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

                    let driver_label = if !driver_name.is_empty() {
                        format!("Linux DRM ({})", driver_name)
                    } else {
                        "Direct Rendering Manager (DRM)".to_string()
                    };

                    gpus.push(GpuSensorInfo { device_path: fs::canonicalize(&dev).unwrap_or_else(|_| dev.clone()).to_string_lossy().into_owned(), engine_loads: Vec::new(),
                        name: vendor_name.to_string(),
                        load_pct,
                        temp: card_temp.or(known_temp),
                        mem_used_mb,
                        mem_total_mb,
                        clock_mhz: core_clock,
                        mem_clock_mhz: mem_clock,
                        power_watts,
                        power_limit_watts: None,
                        driver: driver_label,
                    });
                }
            }
        }

        // 3. PCI fallback if no GPUs detected yet (headless servers, minimal kernels without DRM sysfs)
        if gpus.is_empty() {
            let pci_dir = Path::new("/sys/bus/pci/devices");
            if pci_dir.exists() {
                if let Ok(entries) = fs::read_dir(pci_dir) {
                    for e in entries.flatten() {
                        let p = e.path();
                        if let Ok(class_str) = fs::read_to_string(p.join("class")) {
                            let class_hex = class_str.trim();
                            // PCI class 0x03xxxx = Display controller
                            if class_hex.starts_with("0x03") {
                                let vendor = fs::read_to_string(p.join("vendor")).unwrap_or_default().trim().to_lowercase();
                                let vendor_name = match vendor.as_str() {
                                    "0x8086" => "Intel Graphics Controller",
                                    "0x1002" => "AMD Radeon Display Controller",
                                    "0x10de" => "NVIDIA Display Controller",
                                    "0x1af4" => "VirtIO GPU Accelerator",
                                    "0x15ad" => "VMware SVGA II Adapter",
                                    "0x1234" => "QEMU Standard VGA",
                                    "0x1a03" => "ASPEED Graphics (BMC)",
                                    _ => "Standard PCI VGA/Display Controller",
                                };
                                let driver_name = fs::read_link(p.join("driver"))
                                    .ok()
                                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                                    .unwrap_or_else(|| "PCI Display Driver".to_string());

                                gpus.push(GpuSensorInfo { device_path: String::new(), engine_loads: Vec::new(),
                                    name: vendor_name.to_string(),
                                    load_pct: None,
                                    temp: known_temp,
                                    mem_used_mb: None,
                                    mem_total_mb: None,
                                    clock_mhz: None,
                                    mem_clock_mhz: None,
                                    power_watts: None,
                                    power_limit_watts: None,
                                    driver: format!("PCI Subsystem ({})", driver_name),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    gpus
}

#[cfg_attr(windows, allow(unused_mut))]
pub fn detect_displays() -> Vec<DisplayInfo> {
    let mut displays = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let drm_dir = Path::new("/sys/class/drm");
        if drm_dir.exists() {
            if let Ok(entries) = fs::read_dir(drm_dir) {
                let mut conn_entries: Vec<_> = entries
                    .flatten()
                    .filter(|e| {
                        let fname = e.file_name().to_string_lossy().to_string();
                        fname.starts_with("card") && fname.contains('-')
                    })
                    .collect();
                conn_entries.sort_by_key(|e| e.file_name());

                for e in conn_entries {
                    let p = e.path();
                    let raw_name = e.file_name().to_string_lossy().to_string();
                    let status_p = p.join("status");
                    let status = fs::read_to_string(&status_p).unwrap_or_default().trim().to_lowercase();
                    if status != "connected" {
                        continue;
                    }

                    let name = raw_name
                        .split_once('-')
                        .map(|(_, conn)| conn.to_string())
                        .unwrap_or(raw_name.clone());

                    let enabled = fs::read_to_string(p.join("enabled"))
                        .map(|s| s.trim() == "enabled")
                        .unwrap_or(true);

                    let connector_id = fs::read_to_string(p.join("connector_id"))
                        .ok()
                        .and_then(|s| s.trim().parse::<u32>().ok());

                    let connector_type = if name.starts_with("eDP") {
                        "eDP Pipe A".to_string()
                    } else if name.starts_with("HDMI") {
                        "HDMI".to_string()
                    } else if name.starts_with("DP") {
                        "DisplayPort".to_string()
                    } else if name.starts_with("DVI") {
                        "DVI".to_string()
                    } else if name.starts_with("VGA") {
                        "VGA".to_string()
                    } else {
                        name.clone()
                    };

                    let mut mode_w = 0u32;
                    let mut mode_h = 0u32;
                    if let Ok(modes_str) = fs::read_to_string(p.join("modes")) {
                        if let Some(first_line) = modes_str.lines().next() {
                            if let Some((w_s, h_s)) = first_line.trim().split_once('x') {
                                mode_w = w_s.trim().parse::<u32>().unwrap_or(0);
                                mode_h = h_s.trim().parse::<u32>().unwrap_or(0);
                            }
                        }
                    }

                    let mut edid_mfg = String::new();
                    let mut edid_prod = String::new();
                    let mut bit_depth = "8-bit RGB".to_string();
                    let mut refresh_hz = 60.0f32;
                    let mut edid_w = 0u32;
                    let mut edid_h = 0u32;

                    if let Ok(edid_bytes) = fs::read(p.join("edid")) {
                        if edid_bytes.len() >= 128 {
                            let c1 = ((edid_bytes[8] & 0x7c) >> 2) + b'@';
                            let c2 = (((edid_bytes[8] & 0x03) << 3) | ((edid_bytes[9] & 0xe0) >> 5)) + b'@';
                            let c3 = (edid_bytes[9] & 0x1f) + b'@';
                            if c1.is_ascii_alphabetic() && c2.is_ascii_alphabetic() && c3.is_ascii_alphabetic() {
                                edid_mfg = format!("{}{}{}", c1 as char, c2 as char, c3 as char);
                            }
                            let prod_code = (edid_bytes[10] as u16) | ((edid_bytes[11] as u16) << 8);
                            edid_prod = format!("{:#06x}", prod_code);

                            let b20 = edid_bytes[20];
                            if b20 & 0x80 != 0 {
                                bit_depth = match (b20 >> 4) & 0x07 {
                                    1 => "6-bit RGB".to_string(),
                                    2 => "8-bit RGB".to_string(),
                                    3 => "10-bit RGB".to_string(),
                                    4 => "12-bit RGB".to_string(),
                                    5 => "14-bit RGB".to_string(),
                                    6 => "16-bit RGB".to_string(),
                                    _ => "8-bit RGB".to_string(),
                                };
                            }

                            let pix_clock_10khz = (edid_bytes[54] as u32) | ((edid_bytes[55] as u32) << 8);
                            let h_act = (edid_bytes[56] as u32) | (((edid_bytes[58] as u32) & 0xf0) << 4);
                            let h_blank = (edid_bytes[57] as u32) | (((edid_bytes[58] as u32) & 0x0f) << 8);
                            let v_act = (edid_bytes[59] as u32) | (((edid_bytes[61] as u32) & 0xf0) << 4);
                            let v_blank = (edid_bytes[60] as u32) | (((edid_bytes[61] as u32) & 0x0f) << 8);
                            let h_tot = h_act + h_blank;
                            let v_tot = v_act + v_blank;
                            if pix_clock_10khz > 0 && h_tot > 0 && v_tot > 0 {
                                refresh_hz = ((pix_clock_10khz as f64 * 10_000.0) / (h_tot as f64 * v_tot as f64)) as f32;
                                refresh_hz = (refresh_hz * 10.0).round() / 10.0;
                            }
                            edid_w = h_act;
                            edid_h = v_act;
                        }
                    }

                    let final_w = if mode_w > 0 { mode_w } else if edid_w > 0 { edid_w } else { 1920 };
                    let final_h = if mode_h > 0 { mode_h } else if edid_h > 0 { edid_h } else { 1080 };
                    let resolution = format!("{} \u{00D7} {}", final_w, final_h);

                    displays.push(DisplayInfo {
                        name,
                        is_connected: true,
                        is_enabled: enabled,
                        resolution,
                        width: final_w,
                        height: final_h,
                        refresh_hz,
                        bit_depth,
                        color_space: "sRGB".to_string(),
                        connector_type,
                        connector_id,
                        scaling: "1.0x".to_string(),
                        vrr_support: "None".to_string(),
                        manufacturer: edid_mfg,
                        model: edid_prod,
                    });
                }
            }
        }
    }

    displays
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

/// Returns cumulative TCP payload counters for sockets whose owning PID is visible to this user.
fn collect_process_tcp_counters() -> HashMap<u32, (u64, u64)> {
    #[cfg(target_os = "linux")]
    {
        let Ok(output) = Command::new("ss").args(["-tinpHO"]).output() else {
            return HashMap::new();
        };
        if !output.status.success() {
            return HashMap::new();
        }

        parse_process_tcp_counters(&String::from_utf8_lossy(&output.stdout))
    }
    #[cfg(not(target_os = "linux"))]
    {
        HashMap::new()
    }
}

fn parse_process_tcp_counters(output: &str) -> HashMap<u32, (u64, u64)> {
    let mut counters = HashMap::new();
    for line in output.lines() {
            let sent = line.split_whitespace()
                .find_map(|field| field.strip_prefix("bytes_sent:")?.parse::<u64>().ok())
                .unwrap_or(0);
            let received = line.split_whitespace()
                .find_map(|field| field.strip_prefix("bytes_received:")?.parse::<u64>().ok())
                .unwrap_or(0);
            if sent == 0 && received == 0 {
                continue;
            }

            let mut remaining = line;
            let mut socket_pids = Vec::new();
            while let Some(offset) = remaining.find("pid=") {
                remaining = &remaining[offset + 4..];
                let digits = remaining.chars().take_while(char::is_ascii_digit).count();
                if digits == 0 {
                    break;
                }
                if let Ok(pid) = remaining[..digits].parse::<u32>() {
                    if !socket_pids.contains(&pid) {
                        socket_pids.push(pid);
                    }
                }
                remaining = &remaining[digits..];
            }

            for pid in socket_pids {
                let entry = counters.entry(pid).or_insert((0_u64, 0_u64));
                entry.0 = entry.0.saturating_add(received);
                entry.1 = entry.1.saturating_add(sent);
            }
    }
    counters
}

use crate::config::SshRemoteHost;
use crate::remote::build_ssh_command;
use egui::Color32;

pub fn temp_color(temp: f32) -> Color32 {
    if temp < 55.0 {
        crate::theme::current_gtk4_accent_color()
    } else if temp < 75.0 {
        Color32::from_rgb(148, 163, 184) // Slate grey
    } else {
        Color32::from_rgb(248, 250, 252) // Crisp white
    }
}

fn remote_sensor_command(script: &str, include_processes: bool, include_process_network: bool) -> String {
    format!("{} {} {}", script.trim_end(), if include_processes { "1" } else { "0" }, if include_processes && include_process_network { "1" } else { "0" })
}

#[cfg(test)]
mod remote_sensor_command_tests {
    #[test]
    fn scan_flag_is_an_argument_to_the_python_command() {
        assert_eq!(super::remote_sensor_command("python3 -c 'pass'\n", true, true), "python3 -c 'pass' 1 1");
        assert_eq!(super::remote_sensor_command("python3 -c 'pass'\n", false, true), "python3 -c 'pass' 0 0");
    }
}

pub fn query_remote_sensors(host: &SshRemoteHost, include_processes: bool, include_process_network: bool) -> Result<SystemSensorsSnapshot, String> {
    let script = r#"python3 -c '
import os, glob, json, time, sys
collect_processes = len(sys.argv) > 1 and sys.argv[1] == "1"
collect_process_network = collect_processes and len(sys.argv) > 2 and sys.argv[2] == "1"

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

def read_net_dev():
    nets = {}
    try:
        with open("/proc/net/dev") as f:
            for line in f:
                if ":" in line:
                    p = line.split(":")
                    ifname = p[0].strip()
                    fields = p[1].split()
                    if len(fields) >= 9:
                        rx = int(fields[0])
                        tx = int(fields[8])
                        nets[ifname] = (rx, tx)
    except:
        pass
    return nets

def read_proc_start_marker(pid):
    try:
        with open("/proc/" + str(pid) + "/stat") as stat_file:
            return int(stat_file.read().rsplit(")", 1)[1].split()[19])
    except (OSError, ValueError, IndexError):
        return None

def read_proc_cpu_sample(pid):
    try:
        with open("/proc/" + str(pid) + "/stat") as stat_file:
            fields = stat_file.read().rsplit(")", 1)[1].split()
        return (int(fields[19]), int(fields[11]) + int(fields[12]), time.monotonic(), fields[0])
    except (OSError, ValueError, IndexError):
        return None

def process_state_label(state):
    return {"R": "Running", "S": "Sleeping", "D": "Uninterruptible sleep",
            "T": "Stopped", "t": "Tracing stop", "Z": "Zombie",
            "X": "Dead", "x": "Dead", "I": "Idle", "P": "Parked"}.get(state, "Unknown")

def process_cpu_rate(previous, current, final_start, ticks_per_second):
    if (ticks_per_second is None or ticks_per_second <= 0 or previous is None or current is None or final_start is None
            or previous[0] != current[0] or current[0] != final_start
            or current[1] < previous[1] or current[2] <= previous[2]):
        return None
    return 100.0 * (current[1] - previous[1]) / ticks_per_second / (current[2] - previous[2])

def read_proc_io_map(pids):
    pio = {}
    for pid in pids:
        try:
            start = read_proc_start_marker(pid)
            with open("/proc/" + str(pid) + "/io") as f:
                dr = 0; dw = 0
                for l in f:
                    if l.startswith("read_bytes:"): dr = int(l.split()[1])
                    elif l.startswith("write_bytes:"): dw = int(l.split()[1])
            sampled_at = time.monotonic()
            if start is not None and start == read_proc_start_marker(pid):
                pio[pid] = (start, dr, dw, sampled_at)
        except:
            pass
    return pio

def read_proc_net_map():
    counters = {}
    try:
        for line in os.popen("ss -tinpHO 2>/dev/null").read().splitlines():
            fields = line.split()
            sent = next((int(f.split(":", 1)[1]) for f in fields if f.startswith("bytes_sent:")), 0)
            received = next((int(f.split(":", 1)[1]) for f in fields if f.startswith("bytes_received:")), 0)
            pids = set()
            rest = line
            while "pid=" in rest:
                rest = rest.split("pid=", 1)[1]
                digits = ""
                for c in rest:
                    if not c.isdigit():
                        break
                    digits += c
                if not digits:
                    break
                pids.add(int(digits))
                rest = rest[len(digits):]
            for pid in pids:
                start = read_proc_start_marker(pid)
                if start is None:
                    continue
                previous = counters.get(pid)
                if previous is not None and previous[0] != start:
                    counters.pop(pid, None)
                    continue
                old_rx, old_tx = previous[1:] if previous else (0, 0)
                counters[pid] = (start, old_rx + received, old_tx + sent)
    except:
        pass
    return counters

def count_visible_processes():
    try:
        with os.scandir("/proc") as entries:
            return sum(1 for entry in entries if entry.name.isdigit())
    except OSError:
        return None

process_count = count_visible_processes()
t_start = time.time()
c1 = read_cpu_stats()
d1 = read_disk_stats()
d1_at = time.monotonic()
n1 = read_net_dev()
n1_at = time.monotonic()
init_pids = []
if collect_processes:
    try:
        for line in os.popen("ps -eo pid --sort=-pcpu 2>/dev/null").read().strip().split("\n")[1:]:
            p = line.strip()
            if p.isdigit():
                init_pids.append(int(p))
    except:
        pass
p1_io = read_proc_io_map(init_pids)
p1_cpu = {}
for pid in init_pids:
    sample = read_proc_cpu_sample(pid)
    if sample is not None:
        p1_cpu[pid] = sample
try:
    cpu_ticks_per_second = os.sysconf("SC_CLK_TCK")
    if cpu_ticks_per_second <= 0:
        cpu_ticks_per_second = None
except (OSError, ValueError):
    cpu_ticks_per_second = None
p1_net = read_proc_net_map() if collect_process_network else {}
p1_net_at = time.monotonic()
rapl_sources = []
rapl_e1 = {}
for p in sorted(set(glob.glob("/sys/class/powercap/intel-rapl*/energy_uj") + glob.glob("/sys/class/powercap/intel-rapl*/*/energy_uj"))):
    try:
        name = open(os.path.join(os.path.dirname(p), "name")).read().strip().lower()
        if name != "package" and not name.startswith("package-"):
            continue
        rapl_sources.append(p)
        rapl_e1[p] = (int(open(p).read().strip()), time.monotonic())
    except:
        pass

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

def read_cpu_frequencies():
    by_cpu = {}
    for attribute in ("scaling_cur_freq", "cpuinfo_cur_freq"):
        for path in glob.glob("/sys/devices/system/cpu/cpu[0-9]*/cpufreq/" + attribute):
            try:
                name = path.split("/")[-3]
                if not name.startswith("cpu") or not name[3:].isdigit():
                    continue
                index = int(name[3:])
                if index in by_cpu:
                    continue
                mhz = int(open(path).read().strip()) // 1000
                if mhz > 0:
                    by_cpu[index] = mhz
            except (OSError, ValueError, IndexError):
                pass
    try:
        processor = None
        with open("/proc/cpuinfo") as f:
            for line in f:
                key, sep, value = line.partition(":")
                if not sep:
                    continue
                key, value = key.strip(), value.strip()
                if key == "processor":
                    processor = int(value) if value.isdigit() else None
                elif key == "cpu MHz" and processor is not None and processor not in by_cpu:
                    try:
                        mhz = int(float(value))
                        if mhz > 0:
                            by_cpu[processor] = mhz
                    except (ValueError, OverflowError):
                        pass
    except OSError:
        pass
    return [by_cpu.get(index, 0) for index in range(max(by_cpu, default=-1) + 1)]

cpu_freqs = read_cpu_frequencies()

cpu_tdp = None
for p in glob.glob("/sys/class/powercap/intel-rapl*/constraint_0_power_limit_uw") + glob.glob("/sys/class/powercap/intel-rapl*/*/constraint_0_power_limit_uw"):
    try:
        v = int(open(p).read().strip()) / 1e6
        if 2.0 < v < 1000.0:
            cpu_tdp = round(v, 1)
            break
    except:
        pass

cpu_power = None

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
        crit_p = inp.replace("_input", "_crit")
        crit = None
        if os.path.exists(crit_p):
            try:
                rc = float(open(crit_p).read().strip())
                crit = rc / 1000.0 if abs(rc) > 200.0 else rc
            except:
                pass
        max_p = inp.replace("_input", "_max")
        tmax = None
        if os.path.exists(max_p):
            try:
                rm = float(open(max_p).read().strip())
                tmax = rm / 1000.0 if abs(rm) > 200.0 else rm
            except:
                pass
        try:
            raw_t = float(open(inp).read().strip())
            t = raw_t / 1000.0 if abs(raw_t) > 200.0 else raw_t
            if -40 <= t <= 150:
                temps.append({"chip": name, "label": lbl, "temp": round(t, 1), "crit": crit, "max": tmax})
        except:
            pass
if len(temps) < 3:
    for tz in glob.glob("/sys/class/thermal/thermal_zone*"):
        try:
            type_lbl = open(tz + "/type").read().strip() if os.path.exists(tz + "/type") else os.path.basename(tz)
            raw_t = float(open(tz + "/temp").read().strip())
            t = raw_t / 1000.0 if abs(raw_t) > 200.0 else raw_t
            if -40 <= t <= 150:
                temps.append({"chip": "thermal_zone", "label": type_lbl, "temp": round(t, 1)})
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

for card in sorted(glob.glob("/sys/class/drm/card[0-9]*")):
    if "-" not in os.path.basename(card):
        vendor_p = card + "/device/vendor"
        v = open(vendor_p).read().strip().lower() if os.path.exists(vendor_p) else ""
        if v == "0x10de" and any("NVIDIA" in g.get("driver", "") for g in gpus):
            continue
        drv_p = card + "/device/driver"
        drv_name = os.path.basename(os.readlink(drv_p)) if os.path.islink(drv_p) else ""
        v_name = "Intel HD/UHD Graphics" if v == "0x8086" else ("AMD Radeon Graphics" if v == "0x1002" else ("VirtIO GPU" if v == "0x1af4" else ("VMware SVGA II" if v == "0x15ad" else ("QEMU Standard VGA" if v == "0x1234" else ("ASPEED Graphics (BMC)" if v == "0x1a03" else ("Integrated / Linux GPU" if not drv_name else drv_name.upper() + " GPU"))))))
        act_freq = None
        for f in ["gt_act_freq_mhz", "gt_cur_freq_mhz", "gt/gt0/rps_act_freq_mhz", "gt/gt0/rps_cur_freq_mhz"]:
            if os.path.exists(card + "/" + f):
                try: act_freq = int(open(card + "/" + f).read().strip()); break
                except: pass
        load = None
        try:
            value = float(open(card + "/device/gpu_busy_percent").read().strip())
            if 0 <= value <= 100: load = value
        except: pass

        def memory_mb(*names):
            for name in names:
                try:
                    return int(open(card + "/device/" + name).read().strip()) // (1024 * 1024)
                except:
                    pass
            return None
        mem_used = memory_mb("mem_info_vram_used", "mem_info_lmem_used")
        mem_total = memory_mb("mem_info_vram_total", "mem_info_lmem_total")

        p_watts = None
        mem_clk = None
        c_temp = None
        for hw in glob.glob(card + "/device/hwmon/hwmon*"):
            for tin in glob.glob(hw + "/temp1_input") + glob.glob(hw + "/temp2_input"):
                try:
                    raw_t = float(open(tin).read().strip())
                    c_temp = round(raw_t / 1000.0 if abs(raw_t) > 200.0 else raw_t, 1)
                    break
                except: pass
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
            "temp": c_temp,
            "load": load,
            "mem_used": mem_used,
            "mem_total": mem_total,
            "clock_mhz": act_freq,
            "mem_clock_mhz": mem_clk,
            "power_watts": p_watts,
            "power_limit_watts": None,
            "driver": "Linux DRM (" + (drv_name if drv_name else "Kernel") + ")"
        })

if not gpus:
    for pci in sorted(glob.glob("/sys/bus/pci/devices/*")):
        if os.path.exists(pci + "/class"):
            try:
                cls = open(pci + "/class").read().strip()
                if cls.startswith("0x03"):
                    v = open(pci + "/vendor").read().strip().lower() if os.path.exists(pci + "/vendor") else ""
                    v_name = "Intel Graphics" if v == "0x8086" else ("AMD Radeon" if v == "0x1002" else ("NVIDIA Controller" if v == "0x10de" else ("VirtIO GPU" if v == "0x1af4" else "PCI Display Controller")))
                    drv_name = os.path.basename(os.readlink(pci + "/driver")) if os.path.islink(pci + "/driver") else "PCI"
                    gpus.append({
                        "name": v_name, "temp": None, "load": None, "mem_used": None, "mem_total": None,
                        "clock_mhz": None, "mem_clock_mhz": None, "power_watts": None, "power_limit_watts": None,
                        "driver": "PCI Display (" + drv_name + ")"
                    })
            except: pass

def read_power_text(path):
    try:
        with open(path) as f:
            return f.read().strip()
    except (OSError, UnicodeError):
        return None

ac_online = None
ac_states = []
batteries = []
charge_limit = None
for ps in sorted(glob.glob("/sys/class/power_supply/*")):
    t = (read_power_text(ps + "/type") or "").lower()
    name = os.path.basename(ps).lower()
    if t in ["mains", "ac", "adp"] or (t != "battery" and name.startswith("ac")):
        value = read_power_text(ps + "/online")
        ac_states.append(True if value == "1" else False if value == "0" else None)
    elif t == "battery" or name.startswith("bat"):
        if read_power_text(ps + "/present") == "0":
            continue
        status = read_power_text(ps + "/status") or "Unknown"
        cap = None
        try:
            value = float(read_power_text(ps + "/capacity"))
            if 0.0 <= value <= 100.0:
                cap = value
        except (TypeError, ValueError, OverflowError):
            pass
        
        volt = None
        try:
            value = int(open(ps + "/voltage_now").read().strip()) / 1e6
            if value > 0:
                volt = value
        except:
            pass
        nominal_volt = None
        for vf in ["voltage_min_design", "voltage_design", "voltage_nominal"]:
            try:
                value = int(open(ps + "/" + vf).read().strip()) / 1e6
                if value > 0:
                    nominal_volt = value
                    break
            except:
                pass
            
        p_watts = None
        if os.path.exists(ps + "/power_now"):
            try: p_watts = round(int(open(ps + "/power_now").read().strip()) / 1e6, 2)
            except: pass
        elif volt is not None and os.path.exists(ps + "/current_now"):
            try:
                curr = int(open(ps + "/current_now").read().strip()) / 1e6
                p_watts = round(curr * volt, 2)
            except: pass
            
        energy_wh = None
        energy_full_wh = None
        energy_design_wh = None
        energy_estimated = False
        if os.path.exists(ps + "/energy_now"):
            try: energy_wh = round(int(open(ps + "/energy_now").read().strip()) / 1e6, 2)
            except: pass
        elif nominal_volt is not None and os.path.exists(ps + "/charge_now"):
            try:
                energy_wh = round((int(open(ps + "/charge_now").read().strip()) / 1e6) * nominal_volt, 2)
                energy_estimated = True
            except: pass
            
        if os.path.exists(ps + "/energy_full"):
            try: energy_full_wh = round(int(open(ps + "/energy_full").read().strip()) / 1e6, 2)
            except: pass
        elif nominal_volt is not None and os.path.exists(ps + "/charge_full"):
            try:
                energy_full_wh = round((int(open(ps + "/charge_full").read().strip()) / 1e6) * nominal_volt, 2)
                energy_estimated = True
            except: pass
            
        if os.path.exists(ps + "/energy_full_design"):
            try: energy_design_wh = round(int(open(ps + "/energy_full_design").read().strip()) / 1e6, 2)
            except: pass
        elif nominal_volt is not None and os.path.exists(ps + "/charge_full_design"):
            try:
                energy_design_wh = round((int(open(ps + "/charge_full_design").read().strip()) / 1e6) * nominal_volt, 2)
                energy_estimated = True
            except: pass

        health = None
        if energy_full_wh and energy_design_wh and energy_design_wh > 0:
            health = round(min(100.0, (energy_full_wh / energy_design_wh) * 100.0), 1)
        elif os.path.exists(ps + "/charge_full") and os.path.exists(ps + "/charge_full_design"):
            try:
                cf = int(open(ps + "/charge_full").read().strip())
                cd = int(open(ps + "/charge_full_design").read().strip())
                if cd > 0: health = round(min(100.0, (cf / cd) * 100.0), 1)
            except: pass
            
        cycles = None
        if os.path.exists(ps + "/cycle_count"):
            try:
                c = int(open(ps + "/cycle_count").read().strip())
                if c >= 0: cycles = c
            except: pass
            
        tech = read_power_text(ps + "/technology") or ""
        model = read_power_text(ps + "/model_name") or ""
        mfg = read_power_text(ps + "/manufacturer") or ""
        if os.path.exists(ps + "/charge_control_end_threshold"):
            try:
                cl = int(open(ps + "/charge_control_end_threshold").read().strip())
                if 1 <= cl <= 100: charge_limit = cl
            except: pass
        
        batteries.append({
            "name": os.path.basename(ps),
            "status": status,
            "capacity_pct": cap,
            "power_watts": p_watts,
            "voltage_volts": volt,
            "energy_wh": energy_wh,
            "energy_full_wh": energy_full_wh,
            "energy_design_wh": energy_design_wh,
            "energy_estimated": energy_estimated,
            "health_pct": health,
            "cycle_count": cycles,
            "technology": tech,
            "model_name": model,
            "manufacturer": mfg
        })


if True in ac_states:
    ac_online = True
elif ac_states and all(state is False for state in ac_states):
    ac_online = False

t_elapsed = time.time() - t_start
if t_elapsed < 0.12:
    time.sleep(0.12 - t_elapsed)
c2 = read_cpu_stats()
d2 = read_disk_stats()
d2_at = time.monotonic()
n2 = read_net_dev()
n2_at = time.monotonic()
t_disk_elapsed = max(d2_at - d1_at, 0.001)
t_net_elapsed = max(n2_at - n1_at, 0.001)
p2_net = read_proc_net_map() if collect_process_network else {}
p2_net_at = time.monotonic()
proc_net_elapsed = max(p2_net_at - p1_net_at, 0.001)

networks = []
total_rx_rate = 0.0
total_tx_rate = 0.0
for ifname, (rx2, tx2) in n2.items():
    rx1, tx1 = n1.get(ifname, (rx2, tx2))
    rx_rate = max(0.0, (rx2 - rx1) / t_net_elapsed)
    tx_rate = max(0.0, (tx2 - tx1) / t_net_elapsed)
    total_rx_rate += rx_rate
    total_tx_rate += tx_rate
    is_wl = os.path.exists("/sys/class/net/" + ifname + "/wireless") or os.path.exists("/sys/class/net/" + ifname + "/phy80211")
    networks.append({
        "name": ifname,
        "rx_bytes": rx2,
        "tx_bytes": tx2,
        "rx_rate": rx_rate,
        "tx_rate": tx_rate,
        "is_wireless": is_wl
    })

if rapl_sources and len(rapl_e1) == len(rapl_sources):
    package_watts = []
    for p, (e1, sampled_at) in rapl_e1.items():
        try:
            e2 = int(open(p).read().strip())
            dt = time.monotonic() - sampled_at
            if dt <= 0.05 or e2 < e1:
                break
            watts = (e2 - e1) / 1e6 / dt
            if not 0.0 <= watts <= 800.0:
                break
            package_watts.append(watts)
        except:
            break
    if len(package_watts) == len(rapl_sources):
        cpu_power = round(sum(package_watts), 2)

if cpu_power is None and len(rapl_sources) <= 1:
    for hw in glob.glob("/sys/class/hwmon/hwmon*"):
        name = open(hw + "/name").read().strip().lower() if os.path.exists(hw + "/name") else ""
        if any(k in name for k in ["coretemp", "k10temp", "zenpower", "cpu", "rapl", "fam15h", "acpitz"]):
            for pin in glob.glob(hw + "/power*_input") + glob.glob(hw + "/power*_average"):
                try:
                    label_path = pin.rsplit("_", 1)[0] + "_label"
                    label = open(label_path).read().strip().lower()
                    if not (label.startswith("package") or label.startswith("cpu package")):
                        continue
                    v = int(open(pin).read().strip()) / 1e6
                    if 0 <= v < 1000:
                        cpu_power = round(v, 2)
                        break
                except:
                    pass
            if cpu_power is not None:
                break

disk_rates = {}
for dev, (r2, w2) in d2.items():
    if dev in d1:
        r1, w1 = d1[dev]
        if r2 >= r1 and w2 >= w1 and t_disk_elapsed > 0:
            disk_rates[dev] = ((r2 - r1) / t_disk_elapsed, (w2 - w1) / t_disk_elapsed, r2, w2)

def disk_rate_for_source(source, rates):
    if not source.startswith("/dev/"):
        return None
    device = os.path.basename(os.path.realpath(source))
    return rates.get(device)

for d in disks:
    st = disk_rate_for_source(d["name"], disk_rates)
    d["read_rate"] = st[0] if st else None
    d["write_rate"] = st[1] if st else None
    d["total_read"] = st[2] if st else 0
    d["total_write"] = st[3] if st else 0

def is_base_block_device(device):
    if device.startswith(("loop", "ram", "zram", "dm-")):
        return False
    path = "/sys/class/block/" + device
    if not os.path.isdir(path) or os.path.exists(path + "/partition"):
        return False
    try:
        return not os.listdir(path + "/slaves")
    except OSError:
        return False

def mounted_disk_totals(mounts, rates):
    devices = {os.path.basename(os.path.realpath(mount["name"]))
               for mount in mounts if mount["name"].startswith("/dev/")}
    if not devices or not all(device in rates for device in devices):
        return None, None
    return (sum(rates[device][0] for device in devices),
            sum(rates[device][1] for device in devices))

base_devices = [dev for dev in d2 if is_base_block_device(dev)]
if base_devices:
    if all(dev in disk_rates for dev in base_devices):
        tot_disk_read_rate = sum(disk_rates[dev][0] for dev in base_devices)
        tot_disk_write_rate = sum(disk_rates[dev][1] for dev in base_devices)
    else:
        tot_disk_read_rate, tot_disk_write_rate = None, None
else:
    tot_disk_read_rate, tot_disk_write_rate = mounted_disk_totals(disks, disk_rates)

cpu_usage = None
if "cpu" in c1 and "cpu" in c2:
    td = c2["cpu"][0] - c1["cpu"][0]
    id_d = c2["cpu"][1] - c1["cpu"][1]
    if td > 0:
        cpu_usage = max(0.0, min(100.0, round(100.0 * (1.0 - id_d / td), 1)))

cpu_core_usages = []
core_indices = [int(key[3:]) for key in set(c1) | set(c2) if key.startswith("cpu") and key[3:].isdigit()]
for idx in range(max(core_indices, default=-1) + 1):
    key = f"cpu{idx}"
    if key not in c1 or key not in c2:
        cpu_core_usages.append(None)
        continue
    td = c2[key][0] - c1[key][0]
    id_d = c2[key][1] - c1[key][1]
    if td > 0:
        cpu_core_usages.append(max(0.0, min(100.0, round(100.0 * (1.0 - id_d / td), 1))))
    else:
        cpu_core_usages.append(None)

active_sockets = []
proc_ports = {}
try:
    for line in os.popen("ss -tulpn 2>/dev/null || ss -tunaep 2>/dev/null").read().strip().split("\n")[1:40]:
        parts = line.split()
        if len(parts) >= 5:
            proto = parts[0].upper()
            laddr = parts[4]
            port = ":" + laddr.rsplit(":", 1)[-1] if ":" in laddr else laddr
            status = parts[1] if len(parts) > 1 and parts[1].isupper() else "LISTEN"
            pname = "service"
            p_id = None
            if "users:((\"" in line:
                try:
                    pname = line.split("users:((\"")[1].split("\"")[0]
                except:
                    pass
            elif "users:((" in line:
                try:
                    pname = line.split("users:((")[1].split(",")[0].strip("\"").strip()
                except:
                    pass
            if "pid=" in line:
                try:
                    p_id = int(line.split("pid=")[1].split(",")[0].split(")")[0])
                    if p_id not in proc_ports and port:
                        proc_ports[p_id] = port
                except:
                    pass
            active_sockets.append({"process": pname, "proto": proto, "port": port, "status": status})
except:
    pass

top_apps = []
if collect_processes:
    try:
        for line in os.popen("ps -eo pid,pcpu,pmem,rss,comm --sort=-pcpu 2>/dev/null").read().strip().split("\n")[1:]:
            p = line.split()
            if len(p) >= 5:
                try:
                    pid = int(p[0])
                    cpu = None
                    rss_bytes = int(p[3]) * 1024
                    comm = " ".join(p[4:])

                    cpu_sample = read_proc_cpu_sample(pid)
                    start_marker = cpu_sample[0] if cpu_sample is not None else read_proc_start_marker(pid)
                    network = p2_net.get(pid)
                    rx2, tx2 = network[1:] if network and network[0] == start_marker else (0, 0)
                    dr2 = 0
                    dw2 = 0
                    io_sampled_at = None
                    try:
                        with open("/proc/" + str(pid) + "/io") as fio:
                            for ioline in fio:
                                if ioline.startswith("read_bytes:"): dr2 = int(ioline.split()[1])
                                elif ioline.startswith("write_bytes:"): dw2 = int(ioline.split()[1])
                        io_sampled_at = time.monotonic()
                    except:
                        pass

                    previous_network = p1_net.get(pid)
                    if (start_marker is not None and network and network[0] == start_marker
                            and previous_network and previous_network[0] == start_marker):
                        _, rx1, tx1 = previous_network
                        net_rate = (max(0, rx2 - rx1) + max(0, tx2 - tx1)) / proc_net_elapsed
                    else:
                        net_rate = 0.0
                    dr_rate = 0.0
                    dw_rate = 0.0
                    disk_rate = 0.0
                    current_start = read_proc_start_marker(pid)
                    cpu = process_cpu_rate(p1_cpu.get(pid), cpu_sample, current_start, cpu_ticks_per_second)
                    prior_io = p1_io.get(pid)
                    if (start_marker is not None and start_marker == current_start
                            and io_sampled_at is not None and prior_io is not None and prior_io[0] == start_marker):
                        _, dr1, dw1, previous_io_at = prior_io
                        io_elapsed = max(io_sampled_at - previous_io_at, 0.001)
                        dr_rate = max(0.0, (dr2 - dr1) / io_elapsed)
                        dw_rate = max(0.0, (dw2 - dw1) / io_elapsed)
                        disk_rate = dr_rate + dw_rate

                    port = proc_ports.get(pid)
                    top_apps.append({
                        "pid": pid,
                        "start_marker": start_marker,
                        "cpu": cpu,
                        "mem_bytes": rss_bytes,
                        "name": comm,
                        "status": process_state_label(cpu_sample[3]) if cpu_sample is not None and start_marker == current_start else "Unknown",
                        "net_rx_bytes": rx2,
                        "net_tx_bytes": tx2,
                        "net_rate": net_rate,
                        "disk_read_bytes": dr2,
                        "disk_write_bytes": dw2,
                        "disk_read_rate": dr_rate,
                        "disk_write_rate": dw_rate,
                        "disk_rate": disk_rate,
                        "port": port
                    })
                except:
                    pass
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

uptime = 0
try:
    with open("/proc/uptime") as f:
        uptime = int(float(f.read().split()[0]))
except:
    pass

socket_stats = [0, 0, 0]
try:
    with open("/proc/net/sockstat") as f:
        tot = 0
        tcp = 0
        udp = 0
        for line in f:
            if line.startswith("sockets: used"):
                tot = int(line.split()[2])
            elif line.startswith("TCP: inuse"):
                tcp = int(line.split()[2])
            elif line.startswith("UDP: inuse"):
                udp = int(line.split()[2])
        socket_stats = [max(tot, tcp + udp), tcp, udp]
except:
    pass

cpu_governor = ""
for p in ["/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"]:
    if os.path.exists(p):
        try:
            cpu_governor = open(p).read().strip()
        except:
            pass

phys_disks = []
try:
    for dev in sorted(glob.glob("/sys/block/*")):
        dev_name = os.path.basename(dev)
        if dev_name.startswith(("loop", "ram", "zram", "dm-")):
            continue
        if any(dev_name.startswith(pfx) for pfx in ["sd", "vd", "hd", "xvd"]):
            if dev_name[-1].isdigit():
                continue
        elif dev_name.startswith(("nvme", "mmcblk")):
            if "p" in dev_name:
                continue
        sz = 0
        try:
            sz = int(open(dev + "/size").read().strip()) * 512
        except:
            continue
        if sz == 0:
            continue
        model = "Disk Drive"
        for mp in [dev + "/device/model", dev + "/device/name"]:
            if os.path.exists(mp):
                try:
                    m = open(mp).read().strip()
                    if m:
                        model = m
                        break
                except:
                    pass
        rot = "1"
        try:
            rot = open(dev + "/queue/rotational").read().strip()
        except:
            pass
        is_ssd = (rot == "0") or dev_name.startswith("nvme")
        parts = []
        for part in sorted(glob.glob(dev + "/*/partition")):
            p_dir = os.path.dirname(part)
            p_name = os.path.basename(p_dir)
            try:
                p_num = int(open(part).read().strip())
                p_sz = int(open(p_dir + "/size").read().strip()) * 512
                parts.append({"name": p_name, "partition_num": p_num, "size_bytes": p_sz})
            except:
                pass
        st = disk_rates.get(dev_name)
        r_rate = st[0] if st else None
        w_rate = st[1] if st else None
        phys_disks.append({
            "name": dev_name,
            "model": model,
            "bus_interface": os.path.basename(os.path.realpath(dev + "/device/subsystem")).upper(),
            "discard_supported": (int(open(dev + "/queue/discard_max_bytes").read().strip()) > 0) if os.path.exists(dev + "/queue/discard_max_bytes") else None,
            "smart_status": "",
            "total_bytes": sz,
            "is_ssd": is_ssd,
            "partition_count": len(parts),
            "partitions": parts,
            "read_rate": r_rate,
            "write_rate": w_rate
        })
except:
    pass

if not phys_disks and disks:
    seen = set()
    for d in disks:
        nm = os.path.basename(d["name"])
        if nm not in seen:
            seen.add(nm)
            phys_disks.append({
                "name": nm,
                "model": "Storage Volume (" + d.get("mount", "/") + ")",
                "bus_interface": "",
                "discard_supported": None,
                "smart_status": "",
                "total_bytes": d.get("total", 0),
                "is_ssd": True,
                "partition_count": 1,
                "partitions": [{"name": nm, "partition_num": 1, "size_bytes": d.get("total", 0)}],
                "read_rate": d.get("read_rate"),
                "write_rate": d.get("write_rate")
            })

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
    "mem_free": mem.get("MemFree"),
    "swap_total": mem.get("SwapTotal", 0),
    "swap_free": mem.get("SwapFree", 0),
    "disks": disks,
    "total_disk_read_rate": tot_disk_read_rate,
    "total_disk_write_rate": tot_disk_write_rate,
    "temps": temps,
    "gpus": gpus,
    "networks": networks,
    "total_rx_rate": total_rx_rate,
    "total_tx_rate": total_tx_rate,
    "active_sockets": active_sockets,
    "power": {
        "ac_online": ac_online,
        "batteries": batteries,
        "charge_limit": charge_limit
    },
    "top_apps": top_apps,
    "process_count": process_count,
    "os_name": os_name,
    "os_version": os_version,
    "uptime": uptime,
    "socket_stats": socket_stats,
    "cpu_governor": cpu_governor,
    "physical_disks": phys_disks
}
print(json.dumps(data))
'
"#;

    let script = remote_sensor_command(script, include_processes, include_process_network);
    let mut cmd = build_ssh_command(host, &script);
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
    let cpu_usage = parse_cpu_usage(&parsed["cpu_usage"]);
    let cpu_global_usage = cpu_usage.unwrap_or(0.0);
    let cpu_core_usages = parse_cpu_core_usages(&parsed["cpu_core_usages"]);

    let mut cpu_frequencies_mhz = Vec::new();
    if let Some(arr) = parsed["cpu_freqs"].as_array() {
        for f in arr {
            if let Some(val) = f.as_u64() {
                cpu_frequencies_mhz.push(val);
            }
        }
    }
    let cpu_frequency_mhz = average_cpu_frequency(&cpu_frequencies_mhz);

    let cpu_tdp_watts = parsed["cpu_tdp"].as_f64().map(|v| v as f32);
    let cpu_power_watts = parsed["cpu_power"].as_f64().map(|v| v as f32)
        .filter(|value| value.is_finite() && *value >= 0.0);

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
    let total_disk_rates_available = parsed["total_disk_read_rate"].as_f64().is_some() && parsed["total_disk_write_rate"].as_f64().is_some();

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
                disk_rates_available: d["read_rate"].as_f64().is_some() && d["write_rate"].as_f64().is_some(),
                total_read_bytes: r_total,
                total_write_bytes: w_total,
            });
        }
    }

    let mut gpus = Vec::new();
    if let Some(arr) = parsed["gpus"].as_array() {
        for g in arr {
            gpus.push(GpuSensorInfo { device_path: String::new(), engine_loads: Vec::new(),
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
                    capacity_pct: b["capacity_pct"].as_f64().map(|value| value as f32).filter(|value| value.is_finite() && (0.0..=100.0).contains(value)),
                    power_watts: b["power_watts"].as_f64().map(|v| v as f32),
                    voltage_volts: b["voltage_volts"].as_f64().map(|v| v as f32),
                    energy_wh: b["energy_wh"].as_f64().map(|v| v as f32),
                    energy_full_wh: b["energy_full_wh"].as_f64().map(|v| v as f32),
                    energy_design_wh: b["energy_design_wh"].as_f64().map(|v| v as f32),
                    energy_estimated: b["energy_estimated"].as_bool().unwrap_or(false),
                    health_pct: b["health_pct"].as_f64().map(|v| v as f32),
                    cycle_count: b["cycle_count"].as_u64().map(|v| v as u32),
                    technology: b["technology"].as_str().unwrap_or("").to_string(),
                    model_name: b["model_name"].as_str().unwrap_or("").to_string(),
                    manufacturer: b["manufacturer"].as_str().unwrap_or("").to_string(),
                });
            }
        }
    }

    let total_battery_pct = reported_battery_charge(&batteries);

    let total_power_watts = if batteries.is_empty() { None } else {
        batteries.iter().map(|battery| battery.power_watts).sum::<Option<f32>>()
    };

    let charge_limit = parsed.get("power").and_then(|p| p.get("charge_limit")).and_then(|v| v.as_u64()).map(|v| v as u32);
    let power = PowerSupplyInfo {
        ac_online,
        batteries,
        total_battery_pct,
        total_power_watts,
        backlight_ratio: None,
        usb_power_watts: None,
        kbd_lights_watts: None,
        charge_limit,
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

            let crit = t.get("crit").and_then(|v| v.as_f64()).map(|v| v as f32);
            let max = t.get("max").and_then(|v| v.as_f64()).map(|v| v as f32);

            temp_sensors.push(TempSensor {
                name: label,
                chip,
                temp,
                max,
                crit,
                category: cat,
            });
        }
    }

    let mut networks = Vec::new();
    if let Some(arr) = parsed["networks"].as_array() {
        for n in arr {
            let name = n["name"].as_str().unwrap_or("").to_string();
            let rx = n["rx_bytes"].as_u64().unwrap_or(0);
            let tx = n["tx_bytes"].as_u64().unwrap_or(0);
            let rx_rate = n["rx_rate"].as_f64().unwrap_or(0.0);
            let tx_rate = n["tx_rate"].as_f64().unwrap_or(0.0);
            let is_wireless = n["is_wireless"].as_bool().unwrap_or(false);
            networks.push(NetInterfaceInfo {
                name,
                rx_bytes: rx,
                tx_bytes: tx,
                rx_rate_bps: rx_rate,
                tx_rate_bps: tx_rate,
                is_wireless,
            });
        }
    }

    let (total_rx_bytes, total_tx_bytes, total_rx_rate_bps, total_tx_rate_bps) = network_totals(&networks);

    let mut top_apps = Vec::new();
    if let Some(arr) = parsed.get("top_apps").and_then(|v| v.as_array()) {
        for a in arr {
            let pid = a["pid"].as_u64().unwrap_or(0) as u32;
            let name = a["name"].as_str().unwrap_or("").to_string();
            let cpu_reading = parse_process_cpu_usage(&a["cpu"]);
            let cpu_usage = cpu_reading.unwrap_or(0.0);
            let memory_bytes = a["mem_bytes"].as_u64().unwrap_or(0);
            let status = a["status"].as_str().unwrap_or("Unknown").to_string();
            let net_rx_bytes = a["net_rx_bytes"].as_u64().unwrap_or(0);
            let net_tx_bytes = a["net_tx_bytes"].as_u64().unwrap_or(0);
            let net_rate_bps = a["net_rate"].as_f64().unwrap_or(0.0);
            let disk_read_bytes = a["disk_read_bytes"].as_u64().unwrap_or(0);
            let disk_written_bytes = a["disk_write_bytes"].as_u64().unwrap_or(0);
            let disk_read_rate_bps = a["disk_read_rate"].as_f64().unwrap_or(0.0);
            let disk_write_rate_bps = a["disk_write_rate"].as_f64().unwrap_or(0.0);
            let disk_rate_bps = a["disk_rate"].as_f64().unwrap_or(0.0);
            top_apps.push(ProcessSensorInfo {
                pid,
                start_marker: a["start_marker"].as_u64(),
                name,
                cpu_usage,
                cpu_usage_available: cpu_reading.is_some(),
                memory_bytes,
                disk_read_bytes,
                disk_written_bytes,
                disk_read_rate_bps,
                disk_write_rate_bps,
                disk_rate_bps,
                net_rx_bytes,
                net_tx_bytes,
                net_rate_bps,
                status,
            });
        }
    }

    let mut active_sockets = Vec::new();
    if let Some(arr) = parsed.get("active_sockets").and_then(|v| v.as_array()) {
        for s in arr {
            active_sockets.push(crate::wifi::WifiActiveSocket {
                process: s["process"].as_str().unwrap_or("").to_string(),
                proto: s["proto"].as_str().unwrap_or("").to_string(),
                port: s["port"].as_str().unwrap_or("").to_string(),
                status: s["status"].as_str().unwrap_or("").to_string(),
            });
        }
    }

    let uptime_secs = parsed["uptime"].as_u64().unwrap_or(0);
    let socket_total = parsed["socket_stats"].get(0).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let socket_tcp = parsed["socket_stats"].get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let socket_udp = parsed["socket_stats"].get(2).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let cpu_governor = parsed["cpu_governor"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string());

    let mut physical_disks = Vec::new();
    if let Some(arr) = parsed["physical_disks"].as_array() {
        for p in arr {
            let name = p["name"].as_str().unwrap_or("").to_string();
            let model = p["model"].as_str().unwrap_or("Disk Drive").to_string();
            let bus_interface = p["bus_interface"].as_str().unwrap_or("").to_string();
            let smart_status = p["smart_status"].as_str().unwrap_or("").to_string();
            let discard_supported = p["discard_supported"].as_bool();
            let total_bytes = p["total_bytes"].as_u64().unwrap_or(0);
            let is_ssd = p["is_ssd"].as_bool().unwrap_or(false);
            let partition_count = p["partition_count"].as_u64().unwrap_or(0) as usize;
            let mut partitions = Vec::new();
            if let Some(parts) = p["partitions"].as_array() {
                for part in parts {
                    partitions.push(DiskPartitionInfo {
                        name: part["name"].as_str().unwrap_or("").to_string(),
                        size_bytes: part["size_bytes"].as_u64().unwrap_or(0),
                        partition_num: part["partition_num"].as_u64().unwrap_or(0) as u32,
                    });
                }
            }
            let read_rate_bps = p["read_rate"].as_f64().unwrap_or(0.0);
            let write_rate_bps = p["write_rate"].as_f64().unwrap_or(0.0);
            physical_disks.push(PhysicalDiskInfo {
                name,
                model,
                bus_interface,
                smart_status,
                discard_supported,
                total_bytes,
                is_ssd,
                partition_count,
                partitions,
                read_rate_bps,
                write_rate_bps,
                disk_rates_available: p["read_rate"].as_f64().is_some() && p["write_rate"].as_f64().is_some(),
            });
        }
    }

    if physical_disks.is_empty() && !disks.is_empty() {
        let mut seen = std::collections::HashSet::new();
        for d in &disks {
            let nm = d.name.trim_start_matches("/dev/").to_string();
            if seen.insert(nm.clone()) {
                let model = if !d.mount_point.is_empty() {
                    format!("Storage Volume ({})", d.mount_point)
                } else {
                    "Block Storage".to_string()
                };
                let r_rate = d.read_rate_bps;
                let w_rate = d.write_rate_bps;
                physical_disks.push(PhysicalDiskInfo {
                    name: nm.clone(),
                    model,
                    bus_interface: String::new(),
                    smart_status: String::new(),
                    discard_supported: None,
                    total_bytes: d.total_bytes,
                    is_ssd: true,
                    partition_count: 1,
                    partitions: vec![DiskPartitionInfo {
                        name: nm,
                        size_bytes: d.total_bytes,
                        partition_num: 1,
                    }],
                    read_rate_bps: r_rate,
                    write_rate_bps: w_rate,
                    disk_rates_available: d.disk_rates_available,
                });
            }
        }
    }

    let cpu_tjunction_max = detect_hardware_cpu_tjunction_max(&temp_sensors);

    Ok(SystemSensorsSnapshot {
        cpu_brand,
        cpu_cores_physical: 0,
        cpu_cores_logical: cpu_cores,
        cpu_global_usage,
        cpu_usage_available: cpu_usage.is_some(),
        cpu_core_usages,
        cpu_frequency_mhz,
        cpu_frequencies_mhz,
        cpu_power_watts,
        cpu_tdp_watts,
        cpu_temp,
        cpu_tjunction_max,
        mem_total_bytes,
        mem_used_bytes,
        mem_free_bytes: parsed["mem_free"].as_u64(),
        mem_available_bytes: mem_avail,
        mem_dirty_bytes: None,
        mem_usage_pct,
        swap_total_bytes,
        swap_used_bytes,
        swap_usage_pct,
        ram_temp,
        memory_hw: None,
        gpus,
        displays: Vec::new(),
        power,
        disks,
        physical_disks,
        storage_temp,
        total_disk_read_rate_bps,
        total_disk_write_rate_bps,
        total_disk_rates_available,
        networks,
        total_rx_rate_bps,
        total_tx_rate_bps,
        total_rx_bytes,
        total_tx_bytes,
        temp_sensors,
        process_count: parsed["process_count"].as_u64().and_then(|value| usize::try_from(value).ok()),
        top_apps,
        processes_collected: include_processes,
        process_network_collected: include_processes && include_process_network,
        os_name: parsed["os_name"].as_str().unwrap_or("").to_string(),
        os_version: parsed["os_version"].as_str().unwrap_or("").to_string(),
        cpu_governor,
        uptime_secs,
        socket_total,
        socket_tcp,
        socket_udp,
        active_sockets,
        hardware_access_unlocked: false,
            hardware_access_error: None,
            #[cfg(windows)]
            windows_telemetry_error: None,
            timestamp: Some(Instant::now()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unlock_reports_actual_metrics_instead_of_unconditional_success() {
        assert!(protected_hardware_summary(&serde_json::json!({"cpu_watts":null,"gpus":[],"memory":[],"notes":["No usable counters"]})).unwrap_err().contains("No usable counters"));
        assert!(protected_hardware_summary(&serde_json::json!({"cpu_watts":0.0,"gpus":[],"memory":[]})).unwrap().contains("CPU power available"));
        assert!(protected_hardware_summary(&serde_json::json!({"cpu_watts":null,"gpus":[],"memory":[{"kind":"DDR4"}]})).unwrap().contains("1 RAM module"));
    }


    #[test]
    fn protected_readings_match_devices_and_preserve_unknown_fields() {
        let mut snapshot = SystemSensorsSnapshot::default();
        snapshot.gpus.push(GpuSensorInfo { device_path: "/pci/intel".into(), ..Default::default() });
        snapshot.gpus.push(GpuSensorInfo { device_path: "/pci/other".into(), ..Default::default() });
        apply_protected_hardware(&mut snapshot, &serde_json::json!({
            "cpu_watts": 3.25,
            "gpus": [{"device":"/pci/intel", "engines":{"Render/3D":12.5,"Copy":0.0}, "watts":0.4}],
            "memory":[{"size":"8192 MB","kind":"DDR4","speed":"2400 MT/s","locator":"DIMM A"}]
        }));
        assert_eq!(snapshot.cpu_power_watts, Some(3.25));
        assert_eq!(snapshot.gpus[0].load_pct, None);
        assert!(snapshot.gpus[0].engine_loads.iter().any(|(name, load)| name == "Render/3D" && *load == 12.5));
        assert_eq!(snapshot.gpus[1].load_pct, None);
        assert_eq!(snapshot.gpus[0].mem_used_mb, None);
        let memory = snapshot.memory_hw.unwrap();
        assert_eq!(memory.slot_summary, "1 populated modules");
        assert_eq!(memory.memory_type, "DDR4");
        assert!(memory.topology.contains("unavailable"));
    }


    #[test]
    fn multiple_battery_charge_is_capacity_weighted() {
        let first = BatteryInfo { capacity_pct: Some(100.0), energy_full_wh: Some(80.0), ..Default::default() };
        let second = BatteryInfo { capacity_pct: Some(0.0), energy_full_wh: Some(20.0), ..Default::default() };
        assert_eq!(reported_battery_charge(&[first.clone(), second.clone()]), Some(80.0));
        assert_eq!(reported_battery_charge(&[first, BatteryInfo { energy_full_wh: None, ..second }]), None);
    }

    #[test]
    fn unknown_battery_charge_is_distinct_from_empty() {
        let mut battery = BatteryInfo::default();
        assert_eq!(reported_battery_charge(&[battery.clone()]), None);
        battery.capacity_pct = Some(0.0);
        assert_eq!(reported_battery_charge(&[battery.clone()]), Some(0.0));
        assert_eq!(reported_battery_charge(&[battery, BatteryInfo::default()]), None);
        assert_eq!(reported_battery_charge(&[]), None);
    }



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
    fn parses_and_aggregates_process_tcp_counters() {
        let output = concat!(
            "ESTAB 0 0 a b users:((\"app\",pid=42,fd=3)) bytes_sent:120 bytes_received:80\n",
            "ESTAB 0 0 a b users:((\"app\",pid=42,fd=4)) bytes_sent:30 bytes_received:20\n",
            "ESTAB 0 0 a b bytes_sent:999 bytes_received:999\n",
        );
        let counters = parse_process_tcp_counters(output);
        assert_eq!(counters.get(&42), Some(&(100, 150)));
        assert_eq!(counters.len(), 1);
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
        for pd in &snap.physical_disks {
            assert!(pd.read_rate_bps >= 0.0);
            assert!(pd.write_rate_bps >= 0.0);
        }
    }

    #[test]
    fn test_top_apps_collection_and_cpu_power() {
        let mut collector = SystemSensorsCollector::new();
        let snap = collector.collect();

        if let Some(watts) = snap.cpu_power_watts {
            assert!(watts.is_finite() && watts >= 0.0);
        }
        #[cfg(not(windows))]
        assert!(snap.memory_hw.is_none(), "Unknown module topology must not be guessed");

        // Top apps should have running processes
        assert!(!snap.top_apps.is_empty(), "top_apps should not be empty on a running system");
        for app in &snap.top_apps {
            #[cfg(not(windows))]
            assert!(app.pid > 0);
            assert!(app.cpu_usage >= 0.0);
            assert!(app.disk_read_rate_bps >= 0.0);
            assert!(app.disk_write_rate_bps >= 0.0);
            assert!(app.disk_rate_bps >= 0.0);
            assert!(app.net_rate_bps >= 0.0);
            assert!(!app.status.is_empty());
        }
    }
}

#[cfg(windows)]
impl SystemSensorsCollector {
    /// Native counters are sampled frequently; CIM hardware discovery is cached separately.
    pub fn collect_with_top_apps(&mut self, include_top_apps: bool, _include_process_network: bool) -> SystemSensorsSnapshot {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.disks.refresh();
        self.networks.refresh();
        let now = Instant::now();
        let inventory_due = self.last_hardware_scan.elapsed() >= Duration::from_secs(30);
        if inventory_due {
            self.disks.refresh_list();
            self.networks.refresh_list();
            self.last_hardware_scan = now;
        }
        let sample = collect_windows_hardware(inventory_due);
        if inventory_due {
            if let Ok(data) = &sample {
                update_windows_inventory(&mut self.windows_hardware, data);
                let errors: Vec<&str> = ["gpu_error", "ram_error"].iter().filter_map(|key| data[*key].as_str()).collect();
                self.windows_hardware.windows_telemetry_error = (!errors.is_empty()).then(|| errors.join(" • "));
            } else {
                self.windows_hardware.windows_telemetry_error = Some("Hardware inventory refresh failed; previous inventory retained".into());
            }
        }
        let mut snap = self.windows_hardware.clone();
        match sample {
            Ok(data) => {
                let live = parse_windows_hardware(&data);
                snap.power = live.power;
                snap.cpu_temp = live.cpu_temp;
                snap.storage_temp = live.storage_temp;
                snap.temp_sensors = live.temp_sensors;
                let mut errors: Vec<String> = snap.windows_telemetry_error.take().into_iter().collect();
                errors.extend(["battery_error", "temp_error"].iter().filter_map(|key| data[*key].as_str()).map(str::to_owned));
                snap.windows_telemetry_error = (!errors.is_empty()).then(|| errors.join(" • "));
            }
            Err(error) => { snap.windows_telemetry_error = Some(error); }
        }
        snap.cpu_brand = self.sys.cpus().first().map(|c| c.brand().to_owned()).unwrap_or_default();
        snap.cpu_cores_physical = self.sys.physical_core_count().unwrap_or(0);
        snap.cpu_cores_logical = self.sys.cpus().len();
        snap.cpu_global_usage = self.sys.global_cpu_usage();
        snap.cpu_usage_available = true;
        snap.cpu_core_usages = self.sys.cpus().iter().map(|c| Some(c.cpu_usage())).collect();
        snap.cpu_frequencies_mhz = self.sys.cpus().iter().map(|c| c.frequency()).collect();
        snap.cpu_frequency_mhz = average_cpu_frequency(&snap.cpu_frequencies_mhz);
        snap.mem_total_bytes = self.sys.total_memory();
        snap.mem_used_bytes = self.sys.used_memory();
        snap.mem_free_bytes = None;
        snap.mem_available_bytes = self.sys.available_memory();
        snap.mem_usage_pct = percentage(snap.mem_used_bytes, snap.mem_total_bytes);
        snap.swap_total_bytes = self.sys.total_swap();
        snap.swap_used_bytes = self.sys.used_swap();
        snap.swap_usage_pct = percentage(snap.swap_used_bytes, snap.swap_total_bytes);
        snap.disks = self.disks.iter().map(|d| {
            let total = d.total_space();
            let available = d.available_space();
            DiskInfo {
                name: d.name().to_string_lossy().into_owned(),
                mount_point: d.mount_point().display().to_string(),
                fs_type: d.file_system().to_string_lossy().into_owned(),
                total_bytes: total, available_bytes: available,
                used_bytes: total.saturating_sub(available),
                usage_pct: percentage(total.saturating_sub(available), total),
                ..Default::default()
            }
        }).collect();
        let elapsed = now.duration_since(self.prev_net_time).as_secs_f64().max(0.001);
        for (name, data) in &self.networks {
            let rx = data.total_received();
            let tx = data.total_transmitted();
            let prev_rx = self.prev_net_rx.insert(name.clone(), rx).unwrap_or(rx);
            let prev_tx = self.prev_net_tx.insert(name.clone(), tx).unwrap_or(tx);
            let rx_rate = rx.saturating_sub(prev_rx) as f64 / elapsed;
            let tx_rate = tx.saturating_sub(prev_tx) as f64 / elapsed;
            snap.networks.push(NetInterfaceInfo {
                name: name.clone(), rx_bytes: rx, tx_bytes: tx,
                rx_rate_bps: rx_rate, tx_rate_bps: tx_rate,
                is_wireless: name.to_ascii_lowercase().contains("wi-fi"),
            });
        }
        (snap.total_rx_bytes, snap.total_tx_bytes, snap.total_rx_rate_bps, snap.total_tx_rate_bps) = network_totals(&snap.networks);
        self.prev_net_time = now;
        if include_top_apps {
            self.sys.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All,
                sysinfo::ProcessRefreshKind::new().with_cpu().with_memory().with_disk_usage());
            let proc_disk_now = Instant::now();
            let elapsed = self.prev_proc_disk_time.map(|time| proc_disk_now.duration_since(time).as_secs_f64()).unwrap_or(0.0);
            let mut current_proc_disk = HashMap::new();
            for (pid, process) in self.sys.processes() {
                let disk = process.disk_usage();
                let identity = (pid.as_u32(), process.start_time());
                let current = (disk.total_read_bytes, disk.total_written_bytes);
                current_proc_disk.insert(identity, current);
                let (read, write) = counter_rates(self.prev_proc_disk.get(&identity).copied(), current, elapsed);
                snap.top_apps.push(ProcessSensorInfo {
                    pid: pid.as_u32(), start_marker: crate::procmgr::windows_process_start_marker(pid.as_u32()), name: process.name().to_string_lossy().into_owned(),
                    cpu_usage: process.cpu_usage(), cpu_usage_available: true, memory_bytes: process.memory(),
                    disk_read_bytes: disk.total_read_bytes, disk_written_bytes: disk.total_written_bytes,
                    disk_read_rate_bps: read, disk_write_rate_bps: write, disk_rate_bps: read + write,
                    status: format!("{:?}", process.status()), ..Default::default()
                });
            }
            self.prev_proc_disk = current_proc_disk;
            self.prev_proc_disk_time = Some(proc_disk_now);
            snap.top_apps.sort_unstable_by(|a,b| b.cpu_usage.total_cmp(&a.cpu_usage).then(b.memory_bytes.cmp(&a.memory_bytes)));
            snap.process_count = Some(snap.top_apps.len());
        }
        snap.os_name = System::name().unwrap_or_else(|| "Windows".into());
        snap.os_version = System::os_version().unwrap_or_default();
        snap.uptime_secs = System::uptime();
        snap.timestamp = Some(Instant::now());
        snap
    }
}

#[cfg(any(windows, test))]
fn percentage(used: u64, total: u64) -> f32 {
    if total == 0 { 0.0 } else { (used as f64 / total as f64 * 100.0) as f32 }
}

#[cfg(windows)]
fn collect_windows_hardware(inventory: bool) -> Result<serde_json::Value, String> {
    use std::os::windows::process::CommandExt;
    use std::io::Read;
    // shortcut: temperature telemetry needs LibreHardwareMonitor's WMI provider; add a native driver only if required.
    let script = r#"$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.UTF8Encoding]::new();
$result=@{gpu=@();ram=@();battery=@();temp=@()};
if (__INVENTORY__) {
  try { $result.gpu=@(Get-CimInstance Win32_VideoController -ErrorAction Stop | Select-Object Name,DriverVersion) } catch { $result.gpu_error='GPU inventory unavailable' }
  try { $result.ram=@(Get-CimInstance Win32_PhysicalMemory -ErrorAction Stop | Select-Object Manufacturer,Capacity,Speed,SMBIOSMemoryType,FormFactor) } catch { $result.ram_error='RAM inventory unavailable' }
}
try { $result.battery=@(Get-CimInstance Win32_Battery -ErrorAction Stop | Select-Object Name,EstimatedChargeRemaining,BatteryStatus) } catch { $result.battery_error='Battery telemetry unavailable' }
try { $result.temp=@(Get-CimInstance -Namespace root/LibreHardwareMonitor -ClassName Sensor -ErrorAction Stop | Where-Object SensorType -eq Temperature | Select-Object Name,Value,Identifier) } catch { $result.temp_error='Temperature readings require the LibreHardwareMonitor WMI provider' }
$result | ConvertTo-Json -Depth 4 -Compress"#;
    let script = script.replace("__INVENTORY__", if inventory { "$true" } else { "$false" });
    let mut child = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x08000000).stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn()
        .map_err(|error| format!("Could not start Windows sensor query: {error}"))?;
    // Drain output concurrently so a large inventory cannot block on a full pipe.
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(stdout) = stdout { let _ = stdout.take(1024 * 1024).read_to_end(&mut bytes); }
        bytes
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(status)) => return Err(format!("Windows sensor query failed ({status})")),
            Err(error) => return Err(format!("Windows sensor query failed: {error}")),
            _ if started.elapsed() > Duration::from_secs(8) => {
                let _ = child.kill(); let _ = child.wait();
                return Err("Windows sensor query timed out".into());
            }
            _ => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let bytes = reader.join().map_err(|_| "Could not read Windows sensor query output".to_string())?;
    serde_json::from_slice(&bytes).map_err(|error| format!("Invalid Windows sensor data: {error}"))
}

#[cfg(any(windows, test))]
fn update_windows_inventory(cached: &mut SystemSensorsSnapshot, data: &serde_json::Value) {
    let inventory = parse_windows_hardware(data);
    if data["gpu_error"].is_null() { cached.gpus = inventory.gpus; }
    if data["ram_error"].is_null() { cached.memory_hw = inventory.memory_hw; }
}

#[cfg(any(windows, test))]
fn parse_windows_hardware(data: &serde_json::Value) -> SystemSensorsSnapshot {
    let mut snap = SystemSensorsSnapshot::default();
    if let Some(gpus) = data["gpu"].as_array() {
        snap.gpus = gpus.iter().map(|g| GpuSensorInfo { device_path: String::new(), engine_loads: Vec::new(),
            name: g["Name"].as_str().unwrap_or_default().into(),
            driver: g["DriverVersion"].as_str().unwrap_or_default().into(),
            ..Default::default()
        }).collect();
    }
    if let Some(temps) = data["temp"].as_array() {
        for t in temps {
            let Some(temp) = t["Value"].as_f64().filter(|v| v.is_finite() && *v > -50.0 && *v < 200.0) else { continue; };
            let name = t["Name"].as_str().unwrap_or("Temperature");
            let id = t["Identifier"].as_str().unwrap_or_default();
            let category = if id.contains("cpu") { SensorCategory::Cpu }
                else if id.contains("gpu") { SensorCategory::Gpu }
                else if id.contains("hdd") || id.contains("nvme") { SensorCategory::Storage }
                else { SensorCategory::System };
            match category {
                SensorCategory::Cpu => snap.cpu_temp = Some(temp as f32),
                SensorCategory::Gpu => {},
                SensorCategory::Storage => snap.storage_temp = Some(temp as f32),
                _ => {},
            }
            snap.temp_sensors.push(TempSensor { name: name.into(), chip: "LibreHardwareMonitor".into(),
                temp: temp as f32, category, ..Default::default() });
        }
    }
    if let Some(batteries) = data["battery"].as_array() {
        for b in batteries {
            let percent = b["EstimatedChargeRemaining"].as_f64().filter(|v| v.is_finite() && (0.0..=100.0).contains(v));
            snap.power.batteries.push(BatteryInfo {
                name: b["Name"].as_str().unwrap_or("Battery").into(), present: true,
                capacity_pct: percent.map(|value| value as f32),
                status: match b["BatteryStatus"].as_u64() { Some(1) => "Discharging", Some(2) => "AC connected", Some(3) => "Fully charged", _ => "Unknown" }.into(),
                ..Default::default()
            });
        }
        if !snap.power.batteries.is_empty() {
            snap.power.total_battery_pct = reported_battery_charge(&snap.power.batteries);
        }
    }
    if let Some(ram) = data["ram"].as_array().filter(|r| !r.is_empty()) {
        snap.memory_hw = Some(MemoryHardwareInfo {
            slot_summary: format!("{} populated modules", ram.len()),
            module_spec: ram.iter().map(|r| format!("{} · {} MHz · {}", r["Manufacturer"].as_str().unwrap_or("Unknown"), r["Speed"], format_bytes(r["Capacity"].as_u64().unwrap_or(0)))).collect::<Vec<_>>().join("; "),
            ..Default::default()
        });
    }
    snap
}

#[cfg(test)]
mod windows_sensor_tests {
    use super::*;
    #[test]
    fn failed_inventory_does_not_erase_previous_devices() {
        let mut cached = parse_windows_hardware(&serde_json::json!({"gpu": [{"Name":"Existing GPU"}]}));
        update_windows_inventory(&mut cached, &serde_json::json!({"gpu_error":"unavailable"}));
        assert_eq!(cached.gpus[0].name, "Existing GPU");
        update_windows_inventory(&mut cached, &serde_json::json!({"gpu":[]}));
        assert!(cached.gpus.is_empty());
    }
    #[test]
    fn live_battery_samples_are_independent_of_inventory() {
        let cached = parse_windows_hardware(&serde_json::json!({"gpu": [{"Name":"GPU"}]}));
        let first = parse_windows_hardware(&serde_json::json!({"battery": [{"EstimatedChargeRemaining":72}]}));
        let second = parse_windows_hardware(&serde_json::json!({"battery": [{"EstimatedChargeRemaining":71}]}));
        assert_eq!(cached.gpus[0].name, "GPU");
        assert_eq!(first.power.total_battery_pct, Some(72.0));
        assert_eq!(second.power.total_battery_pct, Some(71.0));
    }
    #[test]
    fn missing_hardware_stays_unavailable_and_invalid_temperatures_are_rejected() {
        let empty = parse_windows_hardware(&serde_json::json!({}));
        assert!(empty.cpu_power_watts.is_none() && empty.cpu_temp.is_none());
        assert!(empty.power.total_battery_pct.is_none());
        let snap = parse_windows_hardware(&serde_json::json!({
            "temp": [{"Name":"Package", "Identifier":"/intelcpu/0", "Value":65.5},
                     {"Name":"Bad", "Value":999}],
            "battery": [{"Name":"Battery", "EstimatedChargeRemaining":72, "BatteryStatus":1}]
        }));
        assert_eq!(snap.cpu_temp, Some(65.5));
        assert_eq!(snap.temp_sensors.len(), 1);
        assert_eq!(snap.power.total_battery_pct, Some(72.0));
        assert_eq!(percentage(1, 0), 0.0);
        assert_eq!(percentage(25, 100), 25.0);
    }
}

const PROTECTED_HARDWARE_SCRIPT: &str = r##"import glob, json, math, os, subprocess, sys, time
os.environ['PATH'] = '/usr/sbin:/usr/bin:/sbin:/bin'

def read_number(path):
    try:
        value = float(open(path).read().strip())
        return value if math.isfinite(value) else None
    except (OSError, ValueError):
        return None

zones = []
unreadable_zones = []
for path in glob.glob('/sys/class/powercap/intel-rapl:*'):
    if os.path.basename(path).count(':') != 1:
        continue
    energy = read_number(path + '/energy_uj')
    maximum = read_number(path + '/max_energy_range_uj')
    if energy is not None:
        zones.append((path, energy, maximum))
    else:
        unreadable_zones.append(path)
started = time.monotonic()
result = {'cpu_watts': None, 'gpus': [], 'memory': None, 'notes': []}
if unreadable_zones:
    result['notes'].append('CPU energy counters are not readable: ' + ', '.join(unreadable_zones))
if len(sys.argv) > 1 and sys.argv[1] == 'inventory':
    result['memory'] = []
    try:
        output = subprocess.run(['dmidecode', '--type', '17'], capture_output=True, text=True, timeout=5)
        if output.returncode:
            result['notes'].append('RAM inventory: ' + output.stderr.strip())
        else:
            for block in output.stdout.split('Memory Device\n')[1:]:
                fields = dict(line.strip().split(': ', 1) for line in block.splitlines() if ': ' in line)
                size = fields.get('Size', '')
                if not size or size in ('No Module Installed', 'Unknown', 'Not Installed'):
                    continue
                values = {key: fields.get(source, '') for key, source in [('size', 'Size'), ('kind', 'Type'), ('speed', 'Configured Memory Speed'), ('form', 'Form Factor'), ('locator', 'Locator')]}
                values['speed'] = values['speed'] or fields.get('Speed', '')
                result['memory'].append(values)
    except (OSError, subprocess.TimeoutExpired) as error:
        result['notes'].append('RAM inventory: ' + str(error))

for card in sorted(glob.glob('/sys/class/drm/card[0-9]*')):
    if '-' in os.path.basename(card):
        continue
    device = os.path.realpath(card + '/device')
    driver = os.path.basename(os.path.realpath(card + '/device/driver'))
    if driver not in ('i915', 'xe'):
        continue
    node = '/dev/dri/' + os.path.basename(card)
    try:
        output = subprocess.run(['intel_gpu_top', '-J', '-s', '250', '-n', '2', '-m', '-d', 'drm:' + node, '-o', '-'], capture_output=True, text=True, timeout=3)
        if output.returncode:
            result['notes'].append(node + ': ' + output.stderr.strip())
            continue
        samples = json.loads(output.stdout)
        sample = next((sample for sample in reversed(samples) if sample.get('engines')), None)
        if sample is not None:
            engines = {name: values['busy'] for name, values in sample['engines'].items() if isinstance(values.get('busy'), (float, int)) and math.isfinite(values['busy']) and 0 <= values['busy'] <= 100}
            gpu_power = sample.get('power', {}).get('GPU')
            if not isinstance(gpu_power, (float, int)) or not math.isfinite(gpu_power) or gpu_power < 0:
                gpu_power = None
            result['gpus'].append({'device': device, 'engines': engines, 'watts': gpu_power})
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        result['notes'].append(node + ': ' + str(error))

if time.monotonic() - started < 0.15:
    time.sleep(0.15)
watts = []
for path, previous, maximum in zones:
    current = read_number(path + '/energy_uj')
    if current is None:
        continue
    delta = current - previous
    if delta < 0 and maximum is not None:
        delta += maximum
    if delta >= 0:
        watts.append(delta / 1e6 / (time.monotonic() - started))
if zones and len(watts) == len(zones):
    result['cpu_watts'] = sum(watts)
print(json.dumps(result, allow_nan=False))
"##;

// This fixed read-only probe runs only after the user unlocks protected sensor access.
pub fn read_protected_hardware(password: Option<&str>, inventory: bool) -> Result<serde_json::Value, String> {
    if !cfg!(target_os = "linux") { return Err("Protected Linux sensor access is unavailable on this platform.".into()); }
    use std::io::Write;
    let mut command = Command::new("sudo");
    let password = password.filter(|password| !password.is_empty());
    if password.is_some() { command.args(["-S", "-p", ""]); } else { command.arg("-n"); }
    command.args(["--", "python3", "-I", "-c", PROTECTED_HARDWARE_SCRIPT, if inventory { "inventory" } else { "sample" }]);
    command.env("LC_ALL", "C").stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    command.stdin(if password.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() });
    let mut child = command.spawn().map_err(|error| format!("Could not start hardware probe: {error}"))?;
    if let Some(password) = password {
        if let Some(mut stdin) = child.stdin.take() { let _ = writeln!(stdin, "{password}"); }
    }
    let output = child.wait_with_output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        if detail.to_ascii_lowercase().contains("no new privileges") {
            return Err("Hardware probe failed: the launch sandbox blocks privilege elevation (NoNewPrivs). Restart Server Launcher directly from a desktop shortcut or host terminal; a sudo password cannot override this restriction.".into());
        }
        return Err(format!("Hardware probe failed: {}", detail.trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("Invalid hardware probe response: {error}"))
}

fn apply_protected_hardware(snapshot: &mut SystemSensorsSnapshot, data: &serde_json::Value) {
    let reported = |value: &serde_json::Value| value.as_f64().filter(|value| value.is_finite() && *value >= 0.0).map(|value| value as f32);
    if let Some(watts) = reported(&data["cpu_watts"]) { snapshot.cpu_power_watts = Some(watts); }
    if let Some(readings) = data["gpus"].as_array() {
        for reading in readings {
            let Some(device) = reading["device"].as_str() else { continue; };
            if let Some(gpu) = snapshot.gpus.iter_mut().find(|gpu| !gpu.device_path.is_empty() && gpu.device_path == device) {
                gpu.engine_loads = reading["engines"].as_object().map(|engines| engines.iter().filter_map(|(name, value)|
                    reported(value).filter(|value| *value <= 100.0).map(|value| (name.clone(), value))).collect()).unwrap_or_default();
                if let Some(watts) = reported(&reading["watts"]) { gpu.power_watts = Some(watts); }
            }
        }
    }
    if let Some(modules) = data["memory"].as_array().filter(|modules| !modules.is_empty()) {
        let unique = |field: &str| modules.iter().filter_map(|module| module[field].as_str())
            .filter(|value| !value.is_empty() && *value != "Unknown").collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>().join(" / ");
        snapshot.memory_hw = Some(MemoryHardwareInfo {
            slot_summary: format!("{} populated modules", modules.len()),
            topology: "Channel topology unavailable".into(),
            memory_type: unique("kind"), form_factor: unique("form"),
            module_spec: modules.iter().map(|module| ["size", "kind", "speed", "locator"].iter()
                .filter_map(|field| module[*field].as_str()).filter(|value| !value.is_empty()).collect::<Vec<_>>().join(" · ")).collect::<Vec<_>>().join("; "),
        });
    }
}

fn protected_hardware_summary(data: &serde_json::Value) -> Result<String, String> {
    let cpu = data["cpu_watts"].as_f64().is_some_and(|value| value.is_finite() && value >= 0.0);
    let gpus = data["gpus"].as_array().map_or(0, Vec::len);
    let modules = data["memory"].as_array().map_or(0, Vec::len);
    if !cpu && gpus == 0 && modules == 0 {
        let notes = data["notes"].as_array().map(|notes| notes.iter().filter_map(|note| note.as_str()).collect::<Vec<_>>().join("; ")).unwrap_or_default();
        return Err(format!("Hardware probe failed: authorization completed, but no CPU power, Intel GPU counters or RAM module details could be read. {}", notes));
    }
    Ok(format!("Read-only hardware: CPU power {}; {} GPU adapter(s); {} RAM module(s).", if cpu { "available" } else { "unavailable" }, gpus, modules))
}

impl SystemSensorsCollector {
    pub fn unlock_hardware_access(&mut self, password: Option<&str>) -> Result<String, String> {
        let data = read_protected_hardware(password, true)?;
        let summary = protected_hardware_summary(&data)?;
        self.protected_data = Some(data);
        self.last_protected_poll = Instant::now();
        Ok(summary)
    }

    pub fn refresh_protected_hardware(&mut self, snapshot: &mut SystemSensorsSnapshot, password: Option<&str>) {
        if self.protected_data.is_none() { return; }
        if self.last_protected_poll.elapsed() >= Duration::from_secs(2) {
            self.last_protected_poll = Instant::now();
            match read_protected_hardware(password, false) {
                Ok(mut data) => {
                    data["memory"] = self.protected_data.as_ref().unwrap()["memory"].clone();
                    self.protected_data = Some(data);
                }
                Err(error) => {
                    // A failed refresh must not keep an old power/GPU sample looking live.
                    let data = self.protected_data.as_mut().unwrap();
                    data["cpu_watts"] = serde_json::Value::Null;
                    data["gpus"] = serde_json::json!([]);
                    data["notes"] = serde_json::json!([error]);
                }
            }
        }
        let data = self.protected_data.as_ref().unwrap();
        snapshot.hardware_access_unlocked = true;
        snapshot.hardware_access_error = data["notes"].as_array().filter(|notes| !notes.is_empty()).map(|notes| notes.iter().filter_map(|note| note.as_str()).collect::<Vec<_>>().join("; "));
        apply_protected_hardware(snapshot, data);
    }
}
