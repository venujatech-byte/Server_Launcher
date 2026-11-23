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
    pub disk_rate_available: bool,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    pub net_counters_available: bool,
    pub net_rate_bps: f64,
    pub net_rate_available: bool,
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
pub struct FanSensorInfo {
    pub name: String,
    pub chip: String,
    pub rpm: f32,
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
    pub is_ssd: Option<bool>,
    pub partition_count: Option<usize>,
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
    pub is_wireless: Option<bool>,
    pub link_state: Option<String>,
    pub mac_address: Option<String>,
    pub mtu: Option<u32>,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub wifi_state: Option<String>,
    pub wifi_ssid: Option<String>,
    pub wifi_bssid: Option<String>,
    pub wifi_signal_pct: Option<u8>,
    pub wifi_channel: Option<u32>,
    pub wifi_rate: Option<String>,
    pub wifi_security: Option<String>,
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

pub(crate) fn single_gpu_load(gpus: &[GpuSensorInfo]) -> Option<f32> {
    let [gpu] = gpus else { return None; };
    gpu.load_pct.filter(|load| load.is_finite() && (0.0..=100.0).contains(load))
}

#[cfg(test)]
mod single_gpu_load_tests {
    use super::GpuSensorInfo;

    #[test]
    fn only_reports_a_valid_reading_for_one_adapter() {
        let gpu = GpuSensorInfo { load_pct: Some(42.0), ..Default::default() };
        assert_eq!(super::single_gpu_load(&[gpu.clone()]), Some(42.0));
        assert_eq!(super::single_gpu_load(&[gpu.clone(), gpu]), None);
        assert_eq!(super::single_gpu_load(&[GpuSensorInfo::default()]), None);
        assert_eq!(super::single_gpu_load(&[GpuSensorInfo { load_pct: Some(f32::NAN), ..Default::default() }]), None);
    }
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

fn edid_bit_depth(input: u8) -> &'static str {
    if input & 0x80 == 0 { return "Not reported"; }
    match (input >> 4) & 0x07 {
        1 => "6-bit RGB",
        2 => "8-bit RGB",
        3 => "10-bit RGB",
        4 => "12-bit RGB",
        5 => "14-bit RGB",
        6 => "16-bit RGB",
        _ => "Not reported",
    }
}

#[cfg(test)]
mod display_edid_tests {
    #[test]
    fn only_reports_defined_digital_color_depths() {
        assert_eq!(super::edid_bit_depth(0x90), "6-bit RGB");
        assert_eq!(super::edid_bit_depth(0xA0), "8-bit RGB");
        assert_eq!(super::edid_bit_depth(0xF0), "Not reported");
        assert_eq!(super::edid_bit_depth(0x00), "Not reported");
    }
}

#[derive(Debug, Clone, Default)]
pub struct BatteryInfo {
    pub device_id: String,
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
    pub cpu_power_cap_watts: Option<f32>,
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
    pub network_stats_available: bool,
    pub network_rates_available: bool,
    pub total_rx_rate_bps: f64,
    pub total_tx_rate_bps: f64,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,

    // All temperature sensors
    pub temp_sensors: Vec<TempSensor>,
    pub fan_sensors: Vec<FanSensorInfo>,

    // Top Applications & Processes
    pub top_apps: Vec<ProcessSensorInfo>,
    pub process_count: Option<usize>,
    pub processes_collected: bool,
    pub process_network_collected: bool,
    pub process_network_supported: bool,
    pub network_details_collected: bool,

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
    pub socket_stats_available: bool,
    pub active_sockets: Vec<crate::wifi::WifiActiveSocket>,
    pub active_sockets_available: bool,

    pub hardware_access_unlocked: bool,
    pub hardware_access_error: Option<String>,
    #[cfg(windows)]
    pub windows_telemetry_error: Option<String>,
    #[cfg(windows)]
    pub windows_hardware_sampled_at: Option<Instant>,
    #[cfg(windows)]
    pub windows_inventory_sampled_at: Option<Instant>,
    #[cfg(windows)]
    pub windows_hardware_attempted_at: Option<Instant>,
    pub timestamp: Option<Instant>,
}

impl SystemSensorsSnapshot {
    pub fn hardware_history_timestamp(&self) -> Option<Instant> {
        #[cfg(windows)]
        { self.windows_hardware_attempted_at.or(self.timestamp) }
        #[cfg(not(windows))]
        { self.timestamp }
    }
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

fn process_cpu_sample_available(has_previous_sample: bool, usage: f32) -> bool {
    has_previous_sample && usage.is_finite()
}

fn process_rate_available(has_previous_sample: bool, elapsed_secs: f64) -> bool {
    has_previous_sample && elapsed_secs.is_finite() && elapsed_secs > 0.0
}

fn normalize_total_cpu_percent(total_usage: f32, logical_cpus: usize) -> Option<f32> {
    if logical_cpus == 0 || !total_usage.is_finite() || total_usage < 0.0 { return None; }
    let usage = total_usage / logical_cpus as f32;
    (usage <= 100.0).then_some(usage)
}

fn network_rate_sample_available(previous: Option<(u64, u64)>, current: (u64, u64)) -> bool {
    previous.is_some_and(|(rx, tx)| current.0 >= rx && current.1 >= tx)
}

fn parse_socket_stats(content: &str) -> Option<(usize, usize, usize)> {
    let value = |prefix: &str, key: &str| content.lines().find_map(|line| {
        if !line.starts_with(prefix) { return None; }
        let mut fields = line.split_whitespace().skip(1);
        while let (Some(field), Some(value)) = (fields.next(), fields.next()) {
            if field == key { return value.parse::<usize>().ok(); }
        }
        None
    });
    let (total, tcp, udp) = (value("sockets:", "used")?, value("TCP:", "inuse")?, value("UDP:", "inuse")?);
    Some((total.max(tcp.saturating_add(udp)), tcp, udp))
}

fn parse_active_sockets(value: &serde_json::Value) -> Option<Vec<crate::wifi::WifiActiveSocket>> {
    let rows = value.as_array()?;
    let parsed: Vec<_> = rows.iter().filter_map(|row| {
        let (Some(process), Some(proto), Some(port), Some(status)) = (
            row["process"].as_str().filter(|value| !value.trim().is_empty()),
            row["proto"].as_str().filter(|value| !value.trim().is_empty()),
            row["port"].as_str().filter(|value| !value.trim().is_empty()),
            row["status"].as_str().filter(|value| !value.trim().is_empty()),
        ) else { return None; };
        Some(crate::wifi::WifiActiveSocket {
            process: process.to_string(), proto: proto.to_string(), port: port.to_string(), status: status.to_string(),
        })
    }).collect();
    if !rows.is_empty() && parsed.is_empty() { None } else { Some(parsed) }
}

#[cfg(test)]
mod process_disk_rate_tests {
    #[test]
    fn handles_first_sample_interval_and_counter_reset() {
        assert_eq!(super::counter_rates(None, (200, 400), 2.0), (0.0, 0.0));
        assert_eq!(super::counter_rates(Some((100, 200)), (300, 500), 2.0), (100.0, 150.0));
        assert_eq!(super::counter_rates(Some((300, 500)), (20, 600), 2.0), (0.0, 50.0));
        assert_eq!(super::counter_rates(Some((100, 200)), (300, 500), 0.0), (0.0, 0.0));
        assert!(!super::process_rate_available(false, 2.0));
        assert!(!super::process_rate_available(true, 0.0));
        assert!(super::process_rate_available(true, 2.0));
    }
}

#[cfg(test)]
mod process_cpu_sample_tests {
    #[test]
    fn first_and_invalid_process_cpu_samples_are_unavailable() {
        assert!(!super::process_cpu_sample_available(false, 0.0));
        assert!(super::process_cpu_sample_available(true, 0.0));
        assert!(!super::process_cpu_sample_available(true, f32::NAN));
    }
}

#[cfg(test)]
mod total_cpu_percent_tests {
    #[test]
    fn converts_sysinfo_core_sum_to_system_percent() {
        assert_eq!(super::normalize_total_cpu_percent(100.0, 4), Some(25.0));
        assert_eq!(super::normalize_total_cpu_percent(400.0, 4), Some(100.0));
        assert_eq!(super::normalize_total_cpu_percent(10.0, 0), None);
        assert_eq!(super::normalize_total_cpu_percent(f32::NAN, 4), None);
    }
}

#[cfg(test)]
mod network_rate_availability_tests {
    #[test]
    fn first_and_reset_counters_are_unavailable() {
        assert!(!super::network_rate_sample_available(None, (100, 200)));
        assert!(super::network_rate_sample_available(Some((100, 200)), (110, 230)));
        assert!(!super::network_rate_sample_available(Some((100, 200)), (10, 230)));
        assert!(!super::network_rate_sample_available(Some((100, 200)), (110, 20)));
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
    prev_cpu_energy: HashMap<String, (u64, u64)>,
    prev_cpu_energy_at: Option<Instant>,
    protected_data: Option<serde_json::Value>,
    last_protected_poll: Instant,
    #[cfg(windows)]
    windows_hardware: SystemSensorsSnapshot,
    #[cfg(windows)]
    windows_network_adapters: serde_json::Value,
    #[cfg(windows)]
    last_windows_hardware_query: Instant,
    #[cfg(windows)]
    cached_windows_hardware: Option<Result<serde_json::Value, String>>,
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
        let prev_net_rx = networks.iter().map(|(name, net)| (name.clone(), net.total_received())).collect();
        let prev_net_tx = networks.iter().map(|(name, net)| (name.clone(), net.total_transmitted())).collect();

        Self {
            sys,
            disks,
            networks,
            components,
            cached_physical_disks: Vec::new(),
            cached_displays: Vec::new(),
            last_hardware_scan: now - Duration::from_secs(30),
            prev_net_rx,
            prev_net_tx,
            prev_net_time: now,
            prev_disk_stats: HashMap::new(),
            prev_disk_time: now,
            prev_proc_disk: HashMap::new(),
            prev_proc_disk_time: None,
            prev_proc_net: HashMap::new(),
            prev_proc_net_time: now,
            prev_cpu_energy: read_rapl_package_energy(),
            prev_cpu_energy_at: Some(now),
            protected_data: None,
            last_protected_poll: now,
            #[cfg(windows)]
            windows_hardware: SystemSensorsSnapshot::default(),
            #[cfg(windows)]
            windows_network_adapters: serde_json::Value::Null,
            #[cfg(windows)]
            last_windows_hardware_query: now - WINDOWS_HARDWARE_QUERY_INTERVAL,
            #[cfg(windows)]
            cached_windows_hardware: None,
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
            self.networks.refresh_list();
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
        let cpu_global_usage = normalize_total_cpu_percent(self.sys.global_cpu_usage(), cpu_cores_logical);
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
        let (mut cpu_frequencies_mhz, mut cpu_power_watts, cpu_power_cap_watts) = collect_cpu_frequencies_and_power();
        let energy = read_rapl_package_energy();
        let rapl_watts = self.prev_cpu_energy_at.and_then(|sampled_at| rapl_total_power_watts(&self.prev_cpu_energy, &energy, sampled_at.elapsed().as_secs_f64()));
        self.prev_cpu_energy = energy;
        self.prev_cpu_energy_at = (!self.prev_cpu_energy.is_empty()).then_some(now);
        if let Some(watts) = rapl_watts { cpu_power_watts.get_or_insert(watts); }
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
        let mut network_rates_available = !self.networks.is_empty();

        for (name, net) in &self.networks {
            let rx = net.total_received();
            let tx = net.total_transmitted();

            let previous = self.prev_net_rx.get(name).copied().zip(self.prev_net_tx.get(name).copied());
            network_rates_available &= network_rate_sample_available(previous, (rx, tx));
            let (prev_rx, prev_tx) = previous.unwrap_or((rx, tx));

            let rx_rate = (rx.saturating_sub(prev_rx) as f64) / elapsed_secs;
            let tx_rate = (tx.saturating_sub(prev_tx) as f64) / elapsed_secs;

            self.prev_net_rx.insert(name.clone(), rx);
            self.prev_net_tx.insert(name.clone(), tx);

            let is_wireless = {
                #[cfg(target_os = "linux")]
                {
                    Some(Path::new(&format!("/sys/class/net/{}/wireless", name)).exists()
                        || Path::new(&format!("/sys/class/net/{}/phy80211", name)).exists()
                    )
                }
                #[cfg(not(target_os = "linux"))]
                None
            };

            networks.push(NetInterfaceInfo {
                name: name.clone(),
                rx_bytes: rx,
                tx_bytes: tx,
                rx_rate_bps: rx_rate,
                tx_rate_bps: tx_rate,
                is_wireless,
                ..Default::default()
            });
        }
        self.prev_net_rx.retain(|name, _| self.networks.iter().any(|(current, _)| current == name));
        self.prev_net_tx.retain(|name, _| self.networks.iter().any(|(current, _)| current == name));
        networks.sort_by(|a, b| a.name.cmp(&b.name));
        let (total_rx_bytes, total_tx_bytes, total_rx_rate_bps, total_tx_rate_bps) = network_totals(&networks);
        let network_stats_available = !networks.is_empty();
        self.prev_net_time = now;

        // 6. Temperature Sensors (sysinfo + hwmon / thermal fallback)
        let mut temp_sensors = collect_hwmon_sensors();
        let fan_sensors = collect_hwmon_fans();

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

        for s in &temp_sensors {
            match s.category {
                SensorCategory::Cpu if cpu_temp.is_none() || s.name.contains("Package") || s.name == "CPU" => {
                    cpu_temp = Some(s.temp);
                }
                SensorCategory::Memory => ram_temp = hottest_temperature(ram_temp, s.temp),
                SensorCategory::Storage => storage_temp = hottest_temperature(storage_temp, s.temp),
                _ => {}
            }
        }

        // 7. GPU & Displays detection (NVIDIA / Intel / AMD / DRM)
        let gpus = detect_gpus();
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
                if name == "exe" || name.is_empty() {
                    if let Some(exe_file) = proc.exe().and_then(|exe| exe.file_name()) {
                        name = exe_file.to_string_lossy().to_string();
                    }
                }
                let cpu_usage = proc.cpu_usage();
                let memory_bytes = proc.memory();
                let disk_usage = proc.disk_usage();
                let read_b = disk_usage.total_read_bytes;
                let written_b = disk_usage.total_written_bytes;
                let identity = (pid_u32, proc.start_time());
                let cpu_usage_available = process_cpu_sample_available(self.prev_proc_disk.contains_key(&identity), cpu_usage);
                let disk_rate_available = process_rate_available(self.prev_proc_disk.contains_key(&identity), proc_disk_elapsed);
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
                let net_counters_available = include_process_network && current_proc_net.contains_key(&pid_u32);
                let previous = start_marker.filter(|_| include_process_network).and_then(|start| {
                    current_identified_net.insert((pid_u32, start), (proc_net_rx, proc_net_tx));
                    self.prev_proc_net.get(&(pid_u32, start)).copied()
                });
                let net_rate_available = process_rate_available(previous.is_some(), proc_net_elapsed_secs);
                let (rx_rate, tx_rate) = counter_rates(previous, (proc_net_rx, proc_net_tx), proc_net_elapsed_secs);
                let net_rate_bps = rx_rate + tx_rate;

                let status = format!("{:?}", proc.status());

                top_apps.push(ProcessSensorInfo {
                    pid: pid_u32,
                    start_marker,
                    name,
                    cpu_usage,
                    cpu_usage_available,
                    memory_bytes,
                    disk_read_bytes: read_b,
                    disk_written_bytes: written_b,
                    disk_read_rate_bps: r_rate,
                    disk_write_rate_bps: w_rate,
                    disk_rate_bps,
                    disk_rate_available,
                    net_rx_bytes: proc_net_rx,
                    net_tx_bytes: proc_net_tx,
                    net_counters_available,
                    net_rate_bps,
                    net_rate_available,
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
        let (socket_total, socket_tcp, socket_udp, socket_stats_available) = {
            #[cfg(target_os = "linux")]
            let stats = fs::read_to_string("/proc/net/sockstat").ok().and_then(|content| parse_socket_stats(&content));
            #[cfg(not(target_os = "linux"))]
            let stats: Option<(usize, usize, usize)> = None;
            match stats {
                Some((total, tcp, udp)) => (total.max(tcp.saturating_add(udp)), tcp, udp, true),
                None => (0, 0, 0, false),
            }
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
            cpu_global_usage: cpu_global_usage.unwrap_or(0.0),
            cpu_usage_available: cpu_global_usage.is_some(),
            cpu_core_usages,
            cpu_frequency_mhz,
            cpu_frequencies_mhz,
            cpu_power_watts,
            cpu_power_cap_watts,
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
            network_stats_available,
            network_rates_available,
            total_rx_rate_bps,
            total_tx_rate_bps,
            total_rx_bytes,
            total_tx_bytes,
            temp_sensors,
            fan_sensors,
            process_count: include_top_apps.then_some(top_apps.len()),
            top_apps,
            processes_collected: include_top_apps,
            process_network_collected: include_top_apps && include_process_network,
            process_network_supported: cfg!(target_os = "linux"),
            network_details_collected: false,
            os_name: System::name().unwrap_or_default(),
            os_version: System::os_version().unwrap_or_default(),
            cpu_governor,
            uptime_secs,
            socket_total,
            socket_tcp,
            socket_udp,
            socket_stats_available,
            active_sockets: Vec::new(),
            active_sockets_available: false,
            hardware_access_unlocked: false,
            hardware_access_error: None,
            #[cfg(windows)]
            windows_telemetry_error: None,
            #[cfg(windows)]
            windows_hardware_sampled_at: None,
            #[cfg(windows)]
            windows_inventory_sampled_at: None,
            #[cfg(windows)]
            windows_hardware_attempted_at: None,
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
    fn stacked_disks_are_not_physical_devices() {
        let root = std::env::temp_dir().join(format!("server-launcher-block-{}", std::process::id()));
        let slaves = root.join("slaves");
        std::fs::create_dir_all(&slaves).unwrap();
        assert!(super::disk_has_no_slaves(&root));
        std::fs::write(slaves.join("sda"), "").unwrap();
        assert!(!super::disk_has_no_slaves(&root));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(!super::disk_has_no_slaves(&root));
    }

    #[test]
    fn diskstats_ignore_malformed_counters_without_losing_following_devices() {
        let parsed = super::parse_proc_diskstats("8 0 sda 1 2 bad 4 5 6 7\n8 16 sdb 1 2 3 4 5 6 7\n");
        assert!(!parsed.contains_key("sda"));
        assert_eq!(parsed["sdb"], (3 * 512, 7 * 512));
    }

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

fn parse_network_address(value: &serde_json::Value, ipv4: bool) -> Option<String> {
    let text = value.as_str()?;
    let (address, prefix) = text.split_once('/')?;
    let prefix: u8 = prefix.parse().ok()?;
    if ipv4 {
        address.parse::<std::net::Ipv4Addr>().ok()?;
        if prefix > 32 { return None; }
    } else {
        address.parse::<std::net::Ipv6Addr>().ok()?;
        if prefix > 128 { return None; }
    }
    Some(text.to_owned())
}

fn parse_network_link_state(value: &serde_json::Value) -> Option<String> {
    match value.as_str()? {
        "up" => Some("Up".into()),
        "down" => Some("Down".into()),
        "dormant" => Some("Dormant".into()),
        "lowerlayerdown" => Some("Lower layer down".into()),
        "testing" => Some("Testing".into()),
        "unknown" => Some("Unknown".into()),
        "notpresent" => Some("Not present".into()),
        _ => None,
    }
}

fn parse_network_mac(value: &serde_json::Value) -> Option<String> {
    let address = value.as_str()?;
    let parts: Vec<_> = address.split(':').collect();
    (parts.len() == 6 && parts.iter().all(|part| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_hexdigit())))
        .then(|| address.to_owned())
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
    fn remote_interface_metadata_rejects_missing_or_invalid_values() {
        assert_eq!(super::parse_network_address(&serde_json::json!("192.0.2.1/24"), true).as_deref(), Some("192.0.2.1/24"));
        assert_eq!(super::parse_network_address(&serde_json::json!("2001:db8::1/64"), false).as_deref(), Some("2001:db8::1/64"));
        assert_eq!(super::parse_network_address(&serde_json::json!("192.0.2.1/33"), true), None);
        assert_eq!(super::parse_network_address(&serde_json::json!("not an address"), false), None);
        assert_eq!(super::parse_network_link_state(&serde_json::json!("up")).as_deref(), Some("Up"));
        assert_eq!(super::parse_network_link_state(&serde_json::json!("unknown")).as_deref(), Some("Unknown"));
        assert_eq!(super::parse_network_link_state(&serde_json::Value::Null), None);
        assert_eq!(super::parse_network_mac(&serde_json::json!("00:11:22:aa:bb:cc")).as_deref(), Some("00:11:22:aa:bb:cc"));
        assert_eq!(super::parse_network_mac(&serde_json::json!("bad address")), None);
        assert_eq!(super::parse_network_mac(&serde_json::Value::Null), None);
    }
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
        stats = parse_proc_diskstats(&content);
    }
    stats
}

fn parse_proc_diskstats(content: &str) -> HashMap<String, (u64, u64)> {
    content.lines().filter_map(|line| {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let name = *parts.get(2)?;
        let read = parts.get(5)?.parse::<u64>().ok()?.saturating_mul(512);
        let written = parts.get(9)?.parse::<u64>().ok()?.saturating_mul(512);
        Some((name.to_string(), (read, written)))
    }).collect()
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
fn disk_has_no_slaves(path: &std::path::Path) -> bool {
    std::fs::read_dir(path.join("slaves")).map(|mut entries| entries.next().is_none()).unwrap_or(false)
}

pub fn collect_physical_disks() -> Vec<PhysicalDiskInfo> {
    let mut physical_disks = Vec::new();
    #[cfg(target_os = "linux")]
    if let Ok(entries) = fs::read_dir("/sys/block") {
        let mut dev_names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| is_base_physical_disk(name) && disk_has_no_slaves(&std::path::Path::new("/sys/block").join(name)))
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
                .and_then(|s| match s.trim() { "0" => Some(true), "1" => Some(false), _ => None })
                .or_else(|| dev.starts_with("nvme").then_some(true));

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

            let partition_count = Some(partitions.len());
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

fn millidegrees_to_celsius(value: f32) -> f32 { value / 1000.0 }

fn parse_reported_temperature(value: &serde_json::Value) -> Option<f32> {
    value.as_f64().filter(|value| value.is_finite() && (-40.0..=150.0).contains(value)).map(|value| value as f32)
}

fn parse_battery_percentage(value: &serde_json::Value) -> Option<f32> {
    value.as_f64().filter(|value| value.is_finite() && (0.0..=100.0).contains(value)).map(|value| value as f32)
}

fn hottest_temperature(current: Option<f32>, candidate: f32) -> Option<f32> {
    Some(current.map_or(candidate, |value| value.max(candidate)))
}

fn apply_unambiguous_gpu_temperature(gpus: &mut [GpuSensorInfo], temperatures: &[f32]) {
    if let ([gpu], [temperature]) = (gpus, temperatures) {
        if gpu.temp.is_none() { gpu.temp = Some(*temperature); }
    }
}

#[cfg(test)]
mod gpu_temperature_matching_tests {
    use super::{apply_unambiguous_gpu_temperature, GpuSensorInfo};

    #[test]
    fn only_attaches_one_temperature_to_one_unreported_gpu() {
        let mut gpus = vec![GpuSensorInfo::default()];
        apply_unambiguous_gpu_temperature(&mut gpus, &[56.0]);
        assert_eq!(gpus[0].temp, Some(56.0));
    }

    #[test]
    fn leaves_multiple_gpus_or_temperatures_unmatched() {
        let mut gpus = vec![GpuSensorInfo::default(), GpuSensorInfo::default()];
        apply_unambiguous_gpu_temperature(&mut gpus, &[56.0]);
        assert!(gpus.iter().all(|gpu| gpu.temp.is_none()));
        apply_unambiguous_gpu_temperature(&mut gpus[..1], &[56.0, 60.0]);
        assert!(gpus[0].temp.is_none());
    }

    #[test]
    fn preserves_gpu_specific_temperature() {
        let mut gpus = vec![GpuSensorInfo { temp: Some(50.0), ..Default::default() }];
        apply_unambiguous_gpu_temperature(&mut gpus, &[56.0]);
        assert_eq!(gpus[0].temp, Some(50.0));
    }
}

fn categorize_sensor(label: &str, chip: &str) -> SensorCategory {
    let lower_label = label.to_lowercase();
    let lower_chip = chip.to_lowercase();

    if lower_label.contains("gpu")
        || lower_chip.contains("amdgpu")
        || lower_chip.contains("nouveau")
        || lower_chip.contains("nvidia")
        || lower_chip.contains("radeon")
    {
        SensorCategory::Gpu
    } else if lower_label.contains("cpu")
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
        || lower_chip.contains("dimm")
        || lower_chip.contains("dram")
        || lower_chip.contains("memory")
        || lower_chip.contains("ram")
    {
        SensorCategory::Memory
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
                                        let temp = millidegrees_to_celsius(raw_val);
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
                                    let temp = millidegrees_to_celsius(raw);
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

fn parse_fan_rpm(value: &str) -> Option<f32> {
    let rpm = value.trim().parse::<f32>().ok()?;
    (rpm.is_finite() && (0.0..=100_000.0).contains(&rpm)).then_some(rpm)
}

pub fn collect_hwmon_fans() -> Vec<FanSensorInfo> {
    let mut result = Vec::new();
    #[cfg(target_os = "linux")]
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        let mut paths: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            let chip = fs::read_to_string(path.join("name")).map(|name| name.trim().to_owned())
                .unwrap_or_else(|_| path.file_name().unwrap_or_default().to_string_lossy().into_owned());
            let Ok(files) = fs::read_dir(&path) else { continue; };
            let mut inputs: Vec<_> = files.flatten().map(|entry| entry.path()).collect();
            inputs.sort();
            for input in inputs {
                let file = input.file_name().unwrap_or_default().to_string_lossy();
                if !file.starts_with("fan") || !file.ends_with("_input") { continue; }
                let Some(rpm) = fs::read_to_string(&input).ok().and_then(|value| parse_fan_rpm(&value)) else { continue; };
                let prefix = file.trim_end_matches("_input");
                let name = fs::read_to_string(path.join(format!("{prefix}_label"))).map(|name| name.trim().to_owned())
                    .ok().filter(|name| !name.is_empty()).unwrap_or_else(|| prefix.to_owned());
                result.push(FanSensorInfo { name, chip: chip.clone(), rpm });
            }
        }
    }
    result
}

#[cfg(test)]
mod fan_sensor_tests {
    #[test]
    fn accepts_reported_fan_rpm_and_rejects_invalid_values() {
        assert_eq!(super::parse_fan_rpm("0"), Some(0.0));
        assert_eq!(super::parse_fan_rpm("1450"), Some(1450.0));
        assert_eq!(super::parse_fan_rpm("NaN"), None);
        assert_eq!(super::parse_fan_rpm("-1"), None);
        assert_eq!(super::parse_fan_rpm("100001"), None);
    }
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
                                        let val = millidegrees_to_celsius(raw);
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
                                                let val = millidegrees_to_celsius(raw);
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

#[cfg(any(target_os = "linux", test))]
fn parse_package_power(label: &str, microwatts: &str) -> Option<f32> {
    let label = label.trim().to_ascii_lowercase();
    if !label.starts_with("package") && !label.starts_with("cpu package") { return None; }
    let watts = microwatts.trim().parse::<f64>().ok()? / 1_000_000.0;
    (watts.is_finite() && watts >= 0.0 && watts <= f32::MAX as f64).then_some(watts as f32)
}

#[cfg(target_os = "linux")]
fn read_rapl_package_energy() -> HashMap<String, (u64, u64)> {
    let mut packages = HashMap::new();
    let Ok(entries) = fs::read_dir("/sys/class/powercap") else { return packages; };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(name) = fs::read_to_string(path.join("name")) else { continue; };
        let name = name.trim().to_ascii_lowercase();
        if !name.starts_with("package") && !name.starts_with("cpu package") { continue; }
        let energy = fs::read_to_string(path.join("energy_uj")).ok().and_then(|value| value.trim().parse().ok());
        let range = fs::read_to_string(path.join("max_energy_range_uj")).ok().and_then(|value| value.trim().parse().ok());
        let (Some(energy), Some(range)) = (energy, range) else { return HashMap::new(); };
        if range == 0 || energy > range { return HashMap::new(); }
        packages.insert(path.to_string_lossy().into_owned(), (energy, range));
    }
    packages
}

#[cfg(target_os = "linux")]
fn read_rapl_package_power_cap() -> Option<f32> {
    let entries = fs::read_dir("/sys/class/powercap").ok()?;
    let mut caps = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(name) = fs::read_to_string(path.join("name")) else { continue; };
        let name = name.trim();
        let lower = name.to_ascii_lowercase();
        if !lower.starts_with("package") && !lower.starts_with("cpu package") { continue; }
        let Ok(value) = fs::read_to_string(path.join("constraint_0_power_limit_uw")) else { return None; };
        let Some(watts) = parse_package_power(name, &value) else { return None; };
        caps.push(watts);
    }
    if caps.is_empty() { return None; }
    let total = caps.iter().map(|watts| *watts as f64).sum::<f64>();
    (total.is_finite() && total <= f32::MAX as f64).then_some(total as f32)
}

#[cfg(not(target_os = "linux"))]
fn read_rapl_package_power_cap() -> Option<f32> { None }

#[cfg(not(target_os = "linux"))]
fn read_rapl_package_energy() -> HashMap<String, (u64, u64)> { HashMap::new() }

fn rapl_power_watts(previous: u64, current: u64, range: u64, elapsed_secs: f64) -> Option<f32> {
    if range == 0 || previous > range || current > range || !elapsed_secs.is_finite() || elapsed_secs <= 0.0 { return None; }
    let delta = if current >= previous { current - previous } else { range - previous + current };
    let watts = delta as f64 / 1_000_000.0 / elapsed_secs;
    (watts.is_finite() && watts >= 0.0 && watts <= f32::MAX as f64).then_some(watts as f32)
}

fn rapl_total_power_watts(previous: &HashMap<String, (u64, u64)>, current: &HashMap<String, (u64, u64)>, elapsed_secs: f64) -> Option<f32> {
    if previous.is_empty() || previous.len() != current.len() { return None; }
    let total = current.iter().try_fold(0.0_f64, |total, (path, (energy, range))| {
        let (previous_energy, previous_range) = previous.get(path)?;
        Some(total + rapl_power_watts(*previous_energy, *energy, (*range).min(*previous_range), elapsed_secs)? as f64)
    })?;
    (total.is_finite() && total <= f32::MAX as f64).then_some(total as f32)
}

#[cfg(test)]
mod rapl_power_tests {
    #[test]
    fn converts_energy_deltas_and_rejects_invalid_samples() {
        assert_eq!(super::rapl_power_watts(1_000_000, 3_000_000, 10_000_000, 2.0), Some(1.0));
        assert_eq!(super::rapl_power_watts(9_000_000, 1_000_000, 10_000_000, 2.0), Some(1.0));
        assert_eq!(super::rapl_power_watts(1, 1, 0, 1.0), None);
        assert_eq!(super::rapl_power_watts(1, 1, 10, 0.0), None);
        assert_eq!(super::rapl_power_watts(1, 10_000_001, 10, 1.0), None);
        assert_eq!(super::rapl_power_watts(0, 1_250_000_000, 2_000_000_000, 1.0), Some(1250.0));
        let previous = [("socket0".into(), (0, 10_000_000)), ("socket1".into(), (0, 10_000_000))].into();
        let current = [("socket0".into(), (2_000_000, 10_000_000)), ("socket1".into(), (4_000_000, 10_000_000))].into();
        assert_eq!(super::rapl_total_power_watts(&previous, &current, 2.0), Some(3.0));
        let partial = [("socket0".into(), (2_000_000, 10_000_000))].into();
        assert_eq!(super::rapl_total_power_watts(&previous, &partial, 2.0), None);
    }
}

pub fn collect_cpu_frequencies_and_power() -> (Vec<u64>, Option<f32>, Option<f32>) {
    let mut freqs = Vec::new();
    let mut cpu_power_watts = None;
    let cpu_power_cap_watts;

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

        // 2. Read the reported long-term package power limit from the RAPL constraint.
        cpu_power_cap_watts = read_rapl_package_power_cap();

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
                                    let Some(prefix) = fname.strip_suffix("_input").or_else(|| fname.strip_suffix("_average")) else { continue; };
                                    if let (Ok(label), Ok(value)) = (
                                        fs::read_to_string(p.join(format!("{prefix}_label"))),
                                        fs::read_to_string(f.path()),
                                    ) {
                                        if let Some(watts) = parse_package_power(&label, &value) {
                                            cpu_power_watts = Some(watts);
                                            break;
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
    #[cfg(not(target_os = "linux"))]
    { cpu_power_cap_watts = None; }

    (freqs, cpu_power_watts, cpu_power_cap_watts)
}

#[cfg(test)]
mod package_power_tests {
    #[test]
    fn accepts_only_labeled_valid_package_power() {
        assert_eq!(super::parse_package_power("CPU Package", "3250000"), Some(3.25));
        assert_eq!(super::parse_package_power("package-0", "0"), Some(0.0));
        assert_eq!(super::parse_package_power("Core", "3250000"), None);
        assert_eq!(super::parse_package_power("", "3250000"), None);
        assert_eq!(super::parse_package_power("Package", "NaN"), None);
        assert_eq!(super::parse_package_power("Package", "-1"), None);
        assert_eq!(super::parse_package_power("Package", "1500000000"), Some(1500.0));
    }
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
#[cfg(target_os = "linux")]
fn linux_micro_value(value: &str) -> Option<f32> {
    let scaled = value.trim().parse::<f64>().ok()? / 1_000_000.0;
    (scaled.is_finite() && scaled.abs() <= f32::MAX as f64).then_some(scaled as f32)
}

#[cfg(target_os = "linux")]
fn linux_charge_to_wh(charge_micro_ah: &str, voltage_v: f32) -> Option<f32> {
    let amp_hours = charge_micro_ah.trim().parse::<f64>().ok()? / 1_000_000.0;
    if !amp_hours.is_finite() || amp_hours < 0.0 || !voltage_v.is_finite() || voltage_v <= 0.0 { return None; }
    let watt_hours = amp_hours * voltage_v as f64;
    (watt_hours.is_finite() && watt_hours <= f32::MAX as f64).then_some(watt_hours as f32)
}

#[cfg(all(test, target_os = "linux"))]
mod linux_power_supply_value_tests {
    #[test]
    fn rejects_non_finite_and_unrepresentable_micro_unit_values() {
        assert_eq!(super::linux_micro_value("12000000"), Some(12.0));
        assert_eq!(super::linux_micro_value("-500000"), Some(-0.5));
        assert_eq!(super::linux_micro_value("NaN"), None);
        assert_eq!(super::linux_micro_value("inf"), None);
        assert_eq!(super::linux_micro_value("1e100"), None);
        assert_eq!(super::linux_micro_value("unknown"), None);
        assert_eq!(super::linux_micro_value("3250000"), Some(3.25));
        assert_eq!(super::linux_charge_to_wh("2500000", 12.0), Some(30.0));
        assert_eq!(super::linux_charge_to_wh("-1", 12.0), None);
        assert_eq!(super::linux_charge_to_wh("1e100", 12.0), None);
        assert_eq!(super::linux_charge_to_wh("1000000", f32::INFINITY), None);
    }
}

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
                            .ok().and_then(|value| linux_micro_value(&value)).filter(|volts| *volts > 0.0);

                        let mut power_watts = fs::read_to_string(p.join("power_now"))
                            .ok().and_then(|value| linux_micro_value(&value));

                        if power_watts.is_none() {
                            if let (Some(v), Ok(c_str)) = (voltage_volts, fs::read_to_string(p.join("current_now"))) {
                                if let Some(amps) = linux_micro_value(&c_str) {
                                    let watts = amps * v;
                                    if watts.is_finite() { power_watts = Some(watts); }
                                }
                            }
                        }

                        let nominal_voltage = ["voltage_min_design", "voltage_design", "voltage_nominal"].iter()
                            .filter_map(|name| fs::read_to_string(p.join(name)).ok())
                            .filter_map(|value| linux_micro_value(&value))
                            .find(|value| *value > 0.0);
                        let mut energy_estimated = false;
                        let mut energy_wh = fs::read_to_string(p.join("energy_now"))
                            .ok().and_then(|value| linux_micro_value(&value)).filter(|value| *value >= 0.0);

                        if energy_wh.is_none() {
                            if let (Some(v), Ok(c_str)) = (nominal_voltage, fs::read_to_string(p.join("charge_now"))) {
                                if let Some(estimate) = linux_charge_to_wh(&c_str, v) {
                                    energy_wh = Some(estimate); energy_estimated = true;
                                }
                            }
                        }

                        let mut energy_full_wh = fs::read_to_string(p.join("energy_full"))
                            .ok().and_then(|value| linux_micro_value(&value)).filter(|value| *value >= 0.0);

                        if energy_full_wh.is_none() {
                            if let (Some(v), Ok(c_str)) = (nominal_voltage, fs::read_to_string(p.join("charge_full"))) {
                                if let Some(estimate) = linux_charge_to_wh(&c_str, v) {
                                    energy_full_wh = Some(estimate); energy_estimated = true;
                                }
                            }
                        }

                        let mut energy_design_wh = fs::read_to_string(p.join("energy_full_design"))
                            .ok().and_then(|value| linux_micro_value(&value)).filter(|value| *value >= 0.0);

                        if energy_design_wh.is_none() {
                            if let (Some(v), Ok(c_str)) = (nominal_voltage, fs::read_to_string(p.join("charge_full_design"))) {
                                if let Some(estimate) = linux_charge_to_wh(&c_str, v) {
                                    energy_design_wh = Some(estimate); energy_estimated = true;
                                }
                            }
                        }

                        let mut health_pct = if let (Some(full), Some(design)) = (energy_full_wh, energy_design_wh) {
                            if design > 0.0 {
                                let pct = (full / design) * 100.0;
                                pct.is_finite().then(|| pct.clamp(0.0, 100.0))
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
                                if let (Some(cf_val), Some(cd_val)) = (linux_micro_value(&cf), linux_micro_value(&cd)) {
                                    if cf_val >= 0.0 && cd_val > 0.0 {
                                        let pct = (cf_val / cd_val) * 100.0;
                                        if pct.is_finite() { health_pct = Some(pct.clamp(0.0, 100.0)); }
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
                            device_id: name.clone(),
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
        batteries.iter().map(|battery| battery.power_watts).sum::<Option<f32>>().filter(|watts| watts.is_finite())
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

static NVIDIA_SMI_LAST_FAILURE: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

fn nvidia_probe_due(last_failure: Option<Instant>, now: Instant) -> bool {
    last_failure.is_none_or(|last| now.saturating_duration_since(last) >= Duration::from_secs(30))
}

fn normalize_pci_address(address: &str) -> Option<String> {
    let (domain, rest) = address.trim().split_once(':')?;
    let (bus, slot) = rest.split_once(':')?;
    let (device, function) = slot.split_once('.')?;
    let (domain, bus, device, function) = (u16::from_str_radix(domain, 16).ok()?, u8::from_str_radix(bus, 16).ok()?, u8::from_str_radix(device, 16).ok()?, u8::from_str_radix(function, 16).ok()?);
    if device > 0x1f || function > 7 { return None; }
    Some(format!("{domain:04x}:{bus:02x}:{device:02x}.{function:x}"))
}

fn parse_nvidia_metric(value: &str, min: f32, max: f32) -> Option<f32> {
    value.parse::<f32>().ok().filter(|value| value.is_finite() && (min..=max).contains(value))
}

#[cfg(test)]
mod gpu_pci_address_tests {
    #[test]
    fn nvidia_probe_retries_after_a_cooldown() {
        let now = std::time::Instant::now();
        assert!(super::nvidia_probe_due(None, now));
        assert!(!super::nvidia_probe_due(Some(now), now + std::time::Duration::from_secs(29)));
        assert!(super::nvidia_probe_due(Some(now), now + std::time::Duration::from_secs(30)));
    }
    #[test]
    fn normalizes_nvidia_and_linux_addresses_without_conflating_devices() {
        assert_eq!(super::normalize_pci_address("00000000:01:00.0"), Some("0000:01:00.0".into()));
        assert_eq!(super::normalize_pci_address("0000:02:00.0"), Some("0000:02:00.0".into()));
        assert_eq!(super::normalize_pci_address("0000:01:20.0"), None);
        assert_eq!(super::normalize_pci_address("[N/A]"), None);
    }
    #[test]
    fn rejects_invalid_nvidia_values() {
        assert_eq!(super::parse_nvidia_metric("N/A", 0.0, 100.0), None);
        assert_eq!(super::parse_nvidia_metric("NaN", 0.0, 100.0), None);
        assert_eq!(super::parse_nvidia_metric("101", 0.0, 100.0), None);
        assert_eq!(super::parse_nvidia_metric("42.5", 0.0, 100.0), Some(42.5));
        assert_eq!(super::parse_nvidia_metric("12500", 0.0, f32::MAX), Some(12500.0));
    }

    #[test]
    fn accepts_only_valid_battery_percentages() {
        assert_eq!(super::parse_battery_percentage(&serde_json::json!(0)), Some(0.0));
        assert_eq!(super::parse_battery_percentage(&serde_json::json!(100)), Some(100.0));
        assert_eq!(super::parse_battery_percentage(&serde_json::json!(-1)), None);
        assert_eq!(super::parse_battery_percentage(&serde_json::json!(101)), None);
    }
}

fn detect_gpus() -> Vec<GpuSensorInfo> {
    let mut gpus = Vec::new();
    let mut nvidia_devices = std::collections::HashSet::new();

    // 1. Check NVIDIA via nvidia-smi if available
    if NVIDIA_SMI_LAST_FAILURE.lock().map(|last| nvidia_probe_due(*last, Instant::now())).unwrap_or(true) {
        let nvidia_out = Command::new("nvidia-smi")
            .args([
                "--query-gpu=name,temperature.gpu,utilization.gpu,utilization.memory,memory.used,memory.total,clocks.current.graphics,clocks.current.memory,power.draw,power.limit,pci.bus_id",
                "--format=csv,noheader,nounits"
            ])
            .output();

        match nvidia_out {
            Ok(out) if out.status.success() => {
                if let Ok(mut last) = NVIDIA_SMI_LAST_FAILURE.lock() { *last = None; }
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines() {
                    let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                    if parts.len() >= 6 {
                        let name = parts[0];
                        if name.is_empty() { continue; }
                        let temp = parse_nvidia_metric(parts[1], -40.0, 150.0);
                        let load_pct = parse_nvidia_metric(parts[2], 0.0, 100.0);
                        let memory = match (parts[4].parse::<u64>().ok(), parts[5].parse::<u64>().ok()) {
                            (Some(used), Some(total)) if total > 0 && used <= total => (Some(used), Some(total)),
                            _ => (None, None),
                        };
                        let clock_mhz = parts.get(6).and_then(|s| s.parse::<u32>().ok()).filter(|value| *value > 0);
                        let mem_clock_mhz = parts.get(7).and_then(|s| s.parse::<u32>().ok()).filter(|value| *value > 0);
        let power_watts = parts.get(8).and_then(|s| parse_nvidia_metric(s, 0.0, f32::MAX));
        let power_limit_watts = parts.get(9).and_then(|s| parse_nvidia_metric(s, 0.0, f32::MAX));

                        let pci = parts.get(10).and_then(|value| normalize_pci_address(value));
                        if let Some(address) = &pci { nvidia_devices.insert(address.clone()); }
                        gpus.push(GpuSensorInfo { device_path: pci.map(|address| format!("pci:{address}")).unwrap_or_default(), engine_loads: Vec::new(),
                            name: name.to_owned(),
                            load_pct,
                            temp,
                            mem_used_mb: memory.0,
                            mem_total_mb: memory.1,
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
                if let Ok(mut last) = NVIDIA_SMI_LAST_FAILURE.lock() { *last = Some(Instant::now()); }
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
                    
                    let pci_address = fs::canonicalize(&dev).ok().and_then(|path| path.file_name().and_then(|name| name.to_str()).and_then(normalize_pci_address));
                    if vendor == "0x10de" && pci_address.as_ref().is_some_and(|address| nvidia_devices.contains(address)) {
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
                            _ => if driver_name == "xe" { "Intel Arc / Xe" } else { "Intel Graphics" },
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
                        .and_then(|s| s.trim().parse::<f32>().ok())
                        .filter(|mhz| mhz.is_finite() && *mhz > 0.0 && *mhz <= u32::MAX as f32);

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
                                            let t = millidegrees_to_celsius(raw);
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
                                        power_watts = linux_micro_value(&s).filter(|watts| *watts >= 0.0);
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
                        temp: card_temp,
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
                                    temp: None,
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
                        "eDP".to_string()
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
                    let mut bit_depth = "Not reported".to_string();
                    let mut refresh_hz = 0.0f32;
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

                            bit_depth = edid_bit_depth(edid_bytes[20]).to_string();

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

                    let final_w = if mode_w > 0 { mode_w } else { edid_w };
                    let final_h = if mode_h > 0 { mode_h } else { edid_h };
                    let resolution = if final_w > 0 && final_h > 0 {
                        format!("{} \u{00D7} {}", final_w, final_h)
                    } else {
                        "Not reported".to_string()
                    };

                    displays.push(DisplayInfo {
                        name,
                        is_connected: true,
                        is_enabled: enabled,
                        resolution,
                        width: final_w,
                        height: final_h,
                        refresh_hz,
                        bit_depth,
                        color_space: "Not reported".to_string(),
                        connector_type,
                        connector_id,
                        scaling: "Not reported".to_string(),
                        vrr_support: "Not reported".to_string(),
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
                .find_map(|field| field.strip_prefix("bytes_sent:")?.parse::<u64>().ok());
            let received = line.split_whitespace()
                .find_map(|field| field.strip_prefix("bytes_received:")?.parse::<u64>().ok());
            let (Some(sent), Some(received)) = (sent, received) else { continue; };

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
fn remote_windows_sensor_command(include_processes: bool, include_network_details: bool, include_gpu_engines: bool, include_ram_modules: bool, include_network_rates: bool, include_cpu_cores: bool, include_disk_rates: bool, include_thermals: bool) -> String {
    use base64::Engine;

    let script = include_str!("../../shared/remote_windows_sensors.ps1")
        .replace("__COLLECT_NETWORK_DETAILS__", if include_network_details { "$true" } else { "$false" })
        .replace("__COLLECT_PROCESSES__", if include_processes { "$true" } else { "$false" })
        .replace("__COLLECT_GPU_ENGINES__", if include_gpu_engines { "$true" } else { "$false" })
        .replace("__COLLECT_RAM_MODULES__", if include_ram_modules { "$true" } else { "$false" })
        .replace("__COLLECT_NETWORK_RATES__", if include_network_rates { "$true" } else { "$false" })
        .replace("__COLLECT_CPU_CORES__", if include_cpu_cores { "$true" } else { "$false" })
        .replace("__COLLECT_DISK_RATES__", if include_disk_rates { "$true" } else { "$false" })
        .replace("__COLLECT_THERMALS__", if include_thermals { "$true" } else { "$false" });
    let utf16le: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16le);
    format!("powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {encoded}")
}

// shortcut: cache Windows by endpoint; a non-Windows response evicts it, while transient SSH errors keep it.
fn known_windows_remote_hosts() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static HOSTS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();
    HOSTS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

fn remote_sensor_host_key(host: &SshRemoteHost) -> String {
    format!("{}@{}:{}", host.user.trim(), host.host.trim(), host.port)
}

fn query_remote_windows_sensors(host: &SshRemoteHost, include_processes: bool, include_network_details: bool, include_gpu_engines: bool, include_ram_modules: bool, include_network_rates: bool, include_cpu_cores: bool, include_disk_rates: bool, include_thermals: bool) -> Result<Option<serde_json::Value>, String> {
    let mut host = host.clone();
    host.remote_cwd.clear();
    let mut cmd = build_ssh_command(&host, &remote_windows_sensor_command(include_processes, include_network_details, include_gpu_engines, include_ram_modules, include_network_rates, include_cpu_cores, include_disk_rates, include_thermals));
    let out = cmd.output().map_err(|error| format!("SSH command execution error: {error}"))?;
    if !out.status.success() {
        let error = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if remote_python_is_missing(&error) {
            return Ok(None);
        }
        return Err(if error.is_empty() { format!("Remote PowerShell exited with status {:?}", out.status.code()) } else { error });
    }

    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|error| format!("Failed to parse remote Windows sensor JSON: {error}"))?;
    if parsed["sensor_os"].as_str() == Some("not_windows") {
        return Ok(None);
    }
    Ok(Some(parsed))
}

fn remote_python_is_missing(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("command not found") || error.contains("not recognized as an internal or external command")
        || error.contains("not recognized as the name of a cmdlet") || error.contains("not recognized")
}

fn remote_sensor_command(script: &str, include_processes: bool, include_process_network: bool, include_network_details: bool) -> String {
    format!("{} {} {} {}", script.trim_end(), if include_processes { "1" } else { "0" }, if include_processes && include_process_network { "1" } else { "0" }, if include_network_details { "1" } else { "0" })
}

fn parse_remote_process_rows(parsed: &serde_json::Value, include_processes: bool) -> Result<Vec<ProcessSensorInfo>, String> {
    let process_rows: &[serde_json::Value] = if include_processes {
        parsed["top_apps"].as_array().map(Vec::as_slice).ok_or_else(|| "Remote sensor data has no process list".to_string())?
    } else {
        &[]
    };
    let mut top_apps = Vec::new();
    for process in process_rows {
        let (Some(pid), Some(name), Some(memory_bytes)) = (
            process["pid"].as_u64(), process["name"].as_str(), process["mem_bytes"].as_u64(),
        ) else { continue; };
        let Ok(pid) = u32::try_from(pid) else { continue; };
        if pid == 0 || name.trim().is_empty() { continue; }
        let counter = |key: &str| process[key].as_u64().unwrap_or(0);
        let rate = |key: &str| process[key].as_f64().filter(|rate| rate.is_finite() && *rate >= 0.0);
        let disk_read_rate_bps = rate("disk_read_rate");
        let disk_write_rate_bps = rate("disk_write_rate");
        let disk_rate_bps = rate("disk_rate");
        let net_rate_bps = rate("net_rate");
        let net_counters_available = process["net_counters_available"].as_bool() == Some(true)
            && process["net_rx_bytes"].as_u64().is_some() && process["net_tx_bytes"].as_u64().is_some();
        let cpu_reading = parse_process_cpu_usage(&process["cpu"]);
        top_apps.push(ProcessSensorInfo {
            pid,
            start_marker: process["start_marker"].as_u64(),
            name: name.to_string(),
            cpu_usage: cpu_reading.unwrap_or(0.0),
            cpu_usage_available: cpu_reading.is_some(),
            memory_bytes,
            disk_read_bytes: counter("disk_read_bytes"),
            disk_written_bytes: counter("disk_write_bytes"),
            disk_read_rate_bps: disk_read_rate_bps.unwrap_or(0.0),
            disk_write_rate_bps: disk_write_rate_bps.unwrap_or(0.0),
            disk_rate_bps: disk_rate_bps.unwrap_or(0.0),
            disk_rate_available: process["disk_rate_available"].as_bool() == Some(true)
                && disk_read_rate_bps.is_some() && disk_write_rate_bps.is_some() && disk_rate_bps.is_some(),
            net_rx_bytes: counter("net_rx_bytes"),
            net_tx_bytes: counter("net_tx_bytes"),
            net_counters_available,
            net_rate_bps: net_rate_bps.unwrap_or(0.0),
            net_rate_available: process["net_rate_available"].as_bool() == Some(true) && net_rate_bps.is_some()
                && net_counters_available,
            status: process["status"].as_str().unwrap_or("Unknown").to_string(),
        });
    }
    Ok(top_apps)
}

fn parse_remote_windows_memory_modules(modules: &serde_json::Value) -> Option<MemoryHardwareInfo> {
    let modules = modules.as_array().filter(|modules| !modules.is_empty())?;
    let types: std::collections::BTreeSet<_> = modules.iter().filter_map(|module| module["SMBIOSMemoryType"].as_u64().and_then(windows_memory_type)).collect();
    let forms: std::collections::BTreeSet<_> = modules.iter().filter_map(|module| module["FormFactor"].as_u64().and_then(windows_memory_form_factor)).collect();
    Some(MemoryHardwareInfo {
        slot_summary: format!("{} populated modules", modules.len()),
        memory_type: types.into_iter().collect::<Vec<_>>().join(" / "),
        form_factor: forms.into_iter().collect::<Vec<_>>().join(" / "),
        module_spec: modules.iter().map(|module| {
            let mut details = Vec::new();
            if let Some(value) = module["Manufacturer"].as_str().map(str::trim).filter(|value| !value.is_empty()) { details.push(value.to_string()); }
            if let Some(value) = module["SMBIOSMemoryType"].as_u64().and_then(windows_memory_type) { details.push(value.to_string()); }
            if let Some(speed) = module["ConfiguredClockSpeed"].as_u64().filter(|speed| *speed > 0) { details.push(format!("{speed} MHz")); }
            if let Some(capacity) = module["Capacity"].as_u64().filter(|capacity| *capacity > 0) { details.push(format_bytes(capacity)); }
            if let Some(value) = module["FormFactor"].as_u64().and_then(windows_memory_form_factor) { details.push(value.to_string()); }
            if let Some(value) = module["DeviceLocator"].as_str().map(str::trim).filter(|value| !value.is_empty()) { details.push(value.to_string()); }
            if details.is_empty() { "Module details unavailable".to_string() } else { details.join(" · ") }
        }).collect::<Vec<_>>().join("; "),
        topology: "Channel topology unavailable".into(),
        ..Default::default()
    })
}

// shortcut: netsh remains a fallback when the optional native WLAN query is unavailable.
fn apply_remote_windows_wlan(interface: &mut NetInterfaceInfo, text: Option<&str>, native: &serde_json::Value) {
    if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
        let mut sections = Vec::<std::collections::HashMap<String, String>>::new();
        let mut fields = std::collections::HashMap::new();
        for line in text.lines().chain(std::iter::once("")) {
            if line.trim().is_empty() {
                if !fields.is_empty() { sections.push(std::mem::take(&mut fields)); }
            } else if let Some((key, value)) = line.split_once(':') {
                fields.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }
        if let Some(fields) = sections.into_iter().find(|fields| {
            fields.get("name").is_some_and(|name| name.eq_ignore_ascii_case(&interface.name))
        }) {
            let get = |key: &str| fields.get(key).map(String::as_str);
            interface.wifi_state = get("state").filter(|state| matches!(state.to_ascii_lowercase().as_str(), "connected" | "disconnected")).map(str::to_ascii_uppercase);
            interface.wifi_ssid = get("ssid").filter(|value| !value.is_empty()).map(str::to_owned);
            interface.wifi_bssid = get("bssid").map(|value| value.replace('-', ":").to_ascii_lowercase()).filter(|value| parse_network_mac(&serde_json::json!(value)).is_some());
            interface.wifi_signal_pct = get("signal").and_then(|value| value.trim_end_matches('%').parse::<u8>().ok()).filter(|value| *value <= 100);
            interface.wifi_channel = get("channel").and_then(|value| value.parse::<u32>().ok()).filter(|value| *value > 0);
            let receive = get("receive rate (mbps)");
            let transmit = get("transmit rate (mbps)");
            interface.wifi_rate = (receive.is_some() || transmit.is_some()).then(|| format!("RX {} / TX {} Mbps", receive.unwrap_or("—"), transmit.unwrap_or("—")));
            interface.wifi_security = get("authentication").filter(|value| !value.is_empty()).map(str::to_owned);
        }
    }
    let state = native["state"].as_str().map(str::to_ascii_uppercase);
    if let Some(state) = state.filter(|state| matches!(state.as_str(), "CONNECTED" | "DISCONNECTED")) { interface.wifi_state = Some(state); }
    if let Some(ssid) = native["ssid"].as_str().filter(|value| !value.is_empty()) { interface.wifi_ssid = Some(ssid.to_owned()); }
    if let Some(bssid) = native["bssid"].as_str().filter(|value| parse_network_mac(&serde_json::json!(value)).is_some()) { interface.wifi_bssid = Some(bssid.replace('-', ":").to_ascii_lowercase()); }
    if let Some(signal) = native["signal_pct"].as_u64().and_then(|value| u8::try_from(value).ok()).filter(|value| *value <= 100) { interface.wifi_signal_pct = Some(signal); }
    if let Some(channel) = native["channel"].as_u64().and_then(|value| u32::try_from(value).ok()).filter(|value| *value > 0) { interface.wifi_channel = Some(channel); }
    if let Some(rate) = native["rate"].as_str().filter(|value| !value.is_empty()) { interface.wifi_rate = Some(rate.to_owned()); }
    if let Some(security) = native["security"].as_str().filter(|value| !value.is_empty()) { interface.wifi_security = Some(security.to_owned()); }
}

fn parse_remote_network_rows(parsed: &serde_json::Value) -> Result<(Vec<NetInterfaceInfo>, bool, bool), String> {
    let interfaces = parsed["networks"].as_array().ok_or_else(|| "Remote sensor data has no network interface list".to_string())?;
    let mut networks = Vec::new();
    let mut rates_available = true;
    for row in interfaces {
        let (Some(name), Some(rx), Some(tx)) = (row["name"].as_str(), row["rx_bytes"].as_u64(), row["tx_bytes"].as_u64()) else { continue; };
        if name.is_empty() { continue; }
        let rate = |key| row[key].as_f64().filter(|value| value.is_finite() && *value >= 0.0);
        let rx_rate = rate("rx_rate");
        let tx_rate = rate("tx_rate");
        rates_available &= rx_rate.is_some() && tx_rate.is_some();
        let mut network = NetInterfaceInfo {
            name: name.to_string(), rx_bytes: rx, tx_bytes: tx,
            rx_rate_bps: rx_rate.unwrap_or(0.0), tx_rate_bps: tx_rate.unwrap_or(0.0),
            is_wireless: row["is_wireless"].as_bool(),
            link_state: parse_network_link_state(&row["link_state"]),
            mac_address: parse_network_mac(&row["mac_address"]),
            mtu: row["mtu"].as_u64().and_then(|value| u32::try_from(value).ok()).filter(|value| *value > 0),
            ipv4: parse_network_address(&row["ipv4"], true),
            ipv6: parse_network_address(&row["ipv6"], false),
            wifi_state: row["wifi_state"].as_str().filter(|state| matches!(state.to_ascii_uppercase().as_str(), "CONNECTED" | "DISCONNECTED")).map(str::to_ascii_uppercase),
            ..Default::default()
        };
        if network.is_wireless == Some(true) { apply_remote_windows_wlan(&mut network, parsed["wlan_text"].as_str(), &row["wlan"]); }
        networks.push(network);
    }
    let complete = parsed["network_stats_available"].as_bool() == Some(true)
        && (interfaces.is_empty() || networks.len() == interfaces.len());
    let rates = parsed["network_rates_available"].as_bool() == Some(true) && rates_available && complete && !networks.is_empty();
    Ok((networks, complete, rates))
}

#[cfg(test)]
mod remote_sensor_command_tests {
    #[test]
    fn scan_flag_is_an_argument_to_the_python_command() {
        assert_eq!(super::remote_sensor_command("python3 -c 'pass'\n", true, true, false), "python3 -c 'pass' 1 1 0");
        assert_eq!(super::remote_sensor_command("python3 -c 'pass'\n", false, true, true), "python3 -c 'pass' 0 0 1");
    }

    #[test]
    fn windows_sensor_command_encodes_utf16_powershell_without_shell_payload() {
        use base64::Engine;
        let command = super::remote_windows_sensor_command(true, true, true, true, true, true, true, true);
        let encoded = command.strip_prefix("powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand ").unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap();
        let words = bytes.chunks_exact(2).map(|word| u16::from_le_bytes([word[0], word[1]])).collect::<Vec<_>>();
        let script = String::from_utf16(&words).unwrap();
        assert!(script.contains("$collectNetworkDetails = $true"));
        assert!(script.contains("$collectProcesses = $true"));
        assert!(script.contains("function Read-ProcessSample"));
        assert!(script.contains("process_network_supported = $false"));
        assert!(script.contains("$collectGpuEngines = $true"));
        assert!(script.contains("Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine"));
        assert!(script.contains("$collectRamModules = $true"));
        assert!(script.contains("Win32_PhysicalMemory"));
        assert!(script.contains("$collectNetworkRates = $true"));
        assert!(script.contains("$collectCpuCores = $true"));
        assert!(script.contains("NumberOfCores"));
        assert!(script.contains("physical_cores = $physicalCores"));
        assert!(script.contains("$collectDiskRates = $true"));
        assert!(script.contains("$collectThermals = $true"));
        assert!(script.contains("device_id = if ($battery.DeviceID)"));
        assert!(script.contains("SensorType -eq 'Fan'"));
        assert!(script.contains("fans = $fans"));
        assert!(script.contains("LibreHardwareMonitor"));
        assert!(script.contains("Win32_PerfFormattedData_PerfDisk_PhysicalDisk"));
        assert!(script.contains("Win32_PerfFormattedData_PerfDisk_LogicalDisk"));
        assert!(script.contains("read_rate = if ($null -ne $rates)"));
        assert!(script.contains("DiskReadBytesPersec"));
        assert!(script.contains("Win32_PerfFormattedData_PerfOS_Processor"));
        assert!(script.contains("$_.Name -eq '_Total'"));
        assert!(script.contains("$totalCpu.PercentProcessorTime"));
        assert!(script.contains("index = [int]$_.Name"));
        assert!(script.contains("$cpuCoreUsages[$row.index] = $row.usage"));
        assert!(script.contains("$rxRate = $null"));
        assert!(script.contains("[int]$adapter.ifType -eq 71"));
        assert!(script.contains("netsh wlan show interfaces"));
        assert!(script.contains("[ServerLauncherWlan]::Read"));
        assert!(script.contains("WlanQueryInterface"));
        assert!(script.contains("wlan_text = $wlanText"));
        assert!(script.contains("Get-NetIPInterface -InterfaceIndex $adapter.ifIndex"));
        assert!(script.contains("mtu = $mtu"));
        assert!(!command.contains("Get-CimInstance"));
    }

    #[test]
    fn remote_gpu_counter_collection_is_opt_in() {
        use base64::Engine;
        let command = super::remote_windows_sensor_command(false, false, false, false, false, false, false, false);
        let encoded = command.strip_prefix("powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand ").unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap();
        let words = bytes.chunks_exact(2).map(|word| u16::from_le_bytes([word[0], word[1]])).collect::<Vec<_>>();
        let script = String::from_utf16(&words).unwrap();
        assert!(script.contains("$collectGpuEngines = $false"));
        assert!(script.contains("$collectRamModules = $false"));
        assert!(script.contains("$collectNetworkRates = $false"));
        assert!(script.contains("$collectCpuCores = $false"));
        assert!(script.contains("$collectDiskRates = $false"));
        assert!(script.contains("$collectThermals = $false"));
    }

    #[test]
    fn only_missing_python_falls_back_to_the_windows_collector() {
        assert!(super::remote_python_is_missing("python3: command not found"));
        assert!(super::remote_python_is_missing("'python3' is not recognized as an internal or external command"));
        assert!(!super::remote_python_is_missing("Permission denied (publickey)"));
    }

    #[test]
    fn windows_process_rows_keep_cpu_and_memory_when_optional_counters_are_missing() {
        let parsed = serde_json::json!({"top_apps":[{
            "pid":42,"name":"worker","start_marker":1337,"cpu":7.5,"mem_bytes":2048,
            "status":"Running","net_rx_bytes":null,"net_tx_bytes":null,"net_rate":null,
            "net_rate_available":false,"disk_read_bytes":null,"disk_write_bytes":null,
            "disk_read_rate":null,"disk_write_rate":null,"disk_rate":null,"disk_rate_available":false
        }]});
        let rows = super::parse_remote_process_rows(&parsed, true).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].cpu_usage_available);
        assert_eq!(rows[0].cpu_usage, 7.5);
        assert_eq!(rows[0].memory_bytes, 2048);
        assert!(!rows[0].disk_rate_available && !rows[0].net_rate_available);
        assert!(!rows[0].net_counters_available);
    }

    #[test]
    fn remote_process_rows_preserve_reported_zero_network_counters() {
        let parsed = serde_json::json!({"top_apps":[{
            "pid":42,"name":"worker","mem_bytes":2048,"cpu":7.5,"status":"Running",
            "net_rx_bytes":0,"net_tx_bytes":0,"net_counters_available":true,
            "net_rate":0.0,"net_rate_available":true
        }]});
        let rows = super::parse_remote_process_rows(&parsed, true).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].net_counters_available && rows[0].net_rate_available);
        assert_eq!(rows[0].net_rx_bytes + rows[0].net_tx_bytes, 0);
    }

    #[test]
    fn remote_windows_wifi_details_are_matched_to_the_adapter_name() {
        let mut wifi = super::NetInterfaceInfo { name: "Wi-Fi".into(), is_wireless: Some(true), ..Default::default() };
        let text = "Name                   : Ethernet\nState                  : disconnected\n\nName                   : Wi-Fi\nState                  : connected\nSSID                   : Home\nBSSID                  : 00-11-22-33-44-55\nAuthentication         : WPA2-Personal\nChannel                : 36\nReceive rate (Mbps)    : 433.3\nTransmit rate (Mbps)   : 866.7\nSignal                 : 84%\n";
        super::apply_remote_windows_wlan(&mut wifi, Some(text), &serde_json::Value::Null);
        assert_eq!(wifi.wifi_state.as_deref(), Some("CONNECTED"));
        assert_eq!(wifi.wifi_ssid.as_deref(), Some("Home"));
        assert_eq!(wifi.wifi_bssid.as_deref(), Some("00:11:22:33:44:55"));
        assert_eq!(wifi.wifi_signal_pct, Some(84));
        assert_eq!(wifi.wifi_channel, Some(36));
        assert_eq!(wifi.wifi_rate.as_deref(), Some("RX 433.3 / TX 866.7 Mbps"));
        assert_eq!(wifi.wifi_security.as_deref(), Some("WPA2-Personal"));
    }

    #[test]
    fn localized_remote_windows_wifi_output_keeps_profile_connection_state() {
        let parsed = serde_json::json!({
            "network_stats_available": true,
            "network_rates_available": false,
            "networks": [{
                "name":"Wi-Fi", "rx_bytes":12, "tx_bytes":34,
                "rx_rate":null, "tx_rate":null, "is_wireless":true,
                "link_state":"up", "wifi_state":"CONNECTED"
            }],
            "wlan_text":"Name : Wi-Fi\nStatus : Verbunden\nSSID : Zuhause\n"
        });
        let (networks, complete, _) = super::parse_remote_network_rows(&parsed).unwrap();
        assert!(complete);
        assert_eq!(networks[0].wifi_state.as_deref(), Some("CONNECTED"));
        assert_eq!(networks[0].wifi_ssid, None);
        assert_eq!(networks[0].wifi_signal_pct, None);
    }

    #[test]
    fn remote_native_windows_wifi_fields_override_localized_fallback() {
        let parsed = serde_json::json!({
            "network_stats_available": true, "network_rates_available": false,
            "wlan_text":"Name : Wi-Fi\nState : disconnected\n",
            "networks":[{"name":"Wi-Fi","rx_bytes":1,"tx_bytes":2,"rx_rate":null,"tx_rate":null,"is_wireless":true,
                "link_state":"up","wlan":{"state":"CONNECTED","ssid":"Home","bssid":"00:11:22:33:44:55","signal_pct":84,
                    "channel":36,"rate":"RX 433.3 / TX 866.7 Mbps","security":"WPA2-Personal (CCMP)"}}]
        });
        let (networks, _, _) = super::parse_remote_network_rows(&parsed).unwrap();
        assert_eq!(networks[0].wifi_state.as_deref(), Some("CONNECTED"));
        assert_eq!(networks[0].wifi_ssid.as_deref(), Some("Home"));
        assert_eq!(networks[0].wifi_signal_pct, Some(84));
        assert_eq!(networks[0].wifi_channel, Some(36));
        assert_eq!(networks[0].wifi_rate.as_deref(), Some("RX 433.3 / TX 866.7 Mbps"));
        assert_eq!(networks[0].wifi_security.as_deref(), Some("WPA2-Personal (CCMP)"));
    }

    #[test]
    fn remote_windows_network_counters_survive_unavailable_rate_samples() {
        let parsed = serde_json::json!({
            "network_stats_available": true,
            "network_rates_available": false,
            "networks": [{
                "name": "Ethernet", "rx_bytes": 1234, "tx_bytes": 5678,
                "rx_rate": null, "tx_rate": null, "is_wireless": false,
                "link_state": "up", "mac_address": null, "mtu": null,
                "ipv4": null, "ipv6": null
            }]
        });
        let (rows, counters_available, rates_available) = super::parse_remote_network_rows(&parsed).unwrap();
        assert!(counters_available);
        assert!(!rates_available);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].rx_bytes, rows[0].tx_bytes), (1234, 5678));
        assert_eq!((rows[0].rx_rate_bps, rows[0].tx_rate_bps), (0.0, 0.0));
    }

    #[test]
    fn remote_windows_ram_inventory_uses_reported_module_metadata() {
        let memory = super::parse_remote_windows_memory_modules(&serde_json::json!([
            {"Manufacturer":"Vendor","Capacity":8589934592_u64,"ConfiguredClockSpeed":3200,"SMBIOSMemoryType":26,"FormFactor":12,"DeviceLocator":"DIMM 0"}
        ])).unwrap();
        assert_eq!(memory.slot_summary, "1 populated modules");
        assert_eq!(memory.memory_type, "DDR4");
        assert_eq!(memory.form_factor, "SODIMM");
        assert!(memory.module_spec.contains("3200 MHz") && memory.module_spec.contains("DIMM 0"));
        assert!(super::parse_remote_windows_memory_modules(&serde_json::json!([])).is_none());
    }
}

pub fn query_remote_sensors(host: &SshRemoteHost, include_processes: bool, include_process_network: bool, include_network_details: bool, include_gpu_engines: bool, include_ram_modules: bool, include_network_rates: bool, include_cpu_cores: bool, include_disk_rates: bool, include_thermals: bool) -> Result<SystemSensorsSnapshot, String> {
    let script = r#"python3 -c '
import os, glob, json, time, sys, math, subprocess, ipaddress, platform
remote_system = platform.system() or "unknown"
if remote_system != "Linux":
    print(json.dumps({"error": f"Remote sensor collection currently supports Linux hosts; detected {remote_system}."}))
    sys.exit(0)
collect_processes = len(sys.argv) > 1 and sys.argv[1] == "1"
collect_process_network = collect_processes and len(sys.argv) > 2 and sys.argv[2] == "1"
collect_network_details = len(sys.argv) > 3 and sys.argv[3] == "1"

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
                    try:
                        r_b = int(p[5]) * 512
                        w_b = int(p[9]) * 512
                        if r_b >= 0 and w_b >= 0:
                            d[p[2]] = (r_b, w_b)
                    except ValueError:
                        continue
    except OSError:
        pass
    return d

def read_net_dev():
    nets = {}
    valid = True
    try:
        with open("/proc/net/dev") as f:
            for line in f:
                if ":" in line:
                    ifname, counters = line.rsplit(":", 1)
                    fields = counters.split()
                    if not ifname.strip() or len(fields) < 9:
                        valid = False
                        continue
                    try:
                        rx = int(fields[0])
                        tx = int(fields[8])
                        if rx < 0 or tx < 0:
                            valid = False
                            continue
                        nets[ifname.strip()] = (rx, tx)
                    except ValueError:
                        valid = False
    except OSError:
        valid = False
    return nets, valid and bool(nets)

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
n1, n1_available = read_net_dev()
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
for p in sorted(set(glob.glob("/sys/class/powercap/*/energy_uj") + glob.glob("/sys/class/powercap/*/*/energy_uj"))):
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
if cores <= 0:
    cores = os.cpu_count() or 0

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

cpu_power_cap = None
cpu_power_caps = []
for p in glob.glob("/sys/class/powercap/*/constraint_0_power_limit_uw") + glob.glob("/sys/class/powercap/*/*/constraint_0_power_limit_uw"):
    try:
        name = open(os.path.join(os.path.dirname(p), "name")).read().strip().lower()
        if not (name.startswith("package") or name.startswith("cpu package")):
            continue
        v = int(open(p).read().strip()) / 1e6
        if math.isfinite(v) and v >= 0.0:
            cpu_power_caps.append(v)
    except:
        pass
if cpu_power_caps:
    total_cpu_power_cap = sum(cpu_power_caps)
    if math.isfinite(total_cpu_power_cap):
        cpu_power_cap = round(total_cpu_power_cap, 1)

cpu_power = None

disks = []
try:
    for line in os.popen("df -B1 -x tmpfs -x devtmpfs -x squashfs").read().strip().split("\n")[1:]:
        p = line.split()
        if len(p) >= 6:
            disks.append({"name": p[0], "total": int(p[1]), "used": int(p[2]), "avail": int(p[3]), "mount": p[5]})
except:
    pass

def read_optional_text(path):
    try:
        with open(path) as f:
            return f.read().strip()
    except (OSError, UnicodeError):
        return None

def millidegrees_to_celsius(value):
    return value / 1000.0

temps = []
fans = []
for hw in glob.glob("/sys/class/hwmon/hwmon*"):
    name = read_optional_text(hw + "/name") or os.path.basename(hw)
    for inp in glob.glob(hw + "/fan*_input"):
        try:
            rpm = float(open(inp).read().strip())
            if math.isfinite(rpm) and 0 <= rpm <= 100000:
                lbl = read_optional_text(inp.replace("_input", "_label")) or os.path.basename(inp).replace("_input", "")
                fans.append({"chip": name, "label": lbl, "rpm": rpm})
        except:
            pass
    for inp in glob.glob(hw + "/temp*_input"):
        lbl_p = inp.replace("_input", "_label")
        lbl = read_optional_text(lbl_p) or os.path.basename(inp)
        crit_p = inp.replace("_input", "_crit")
        crit = None
        if os.path.exists(crit_p):
            try:
                rc = float(open(crit_p).read().strip())
                crit = millidegrees_to_celsius(rc)
            except:
                pass
        max_p = inp.replace("_input", "_max")
        tmax = None
        if os.path.exists(max_p):
            try:
                rm = float(open(max_p).read().strip())
                tmax = millidegrees_to_celsius(rm)
            except:
                pass
        try:
            raw_t = float(open(inp).read().strip())
            t = millidegrees_to_celsius(raw_t)
            if -40 <= t <= 150:
                temps.append({"chip": name, "label": lbl, "temp": round(t, 1), "crit": crit, "max": tmax})
        except:
            pass
if len(temps) < 3:
    for tz in glob.glob("/sys/class/thermal/thermal_zone*"):
        try:
            type_lbl = open(tz + "/type").read().strip() if os.path.exists(tz + "/type") else os.path.basename(tz)
            raw_t = float(open(tz + "/temp").read().strip())
            t = millidegrees_to_celsius(raw_t)
            if -40 <= t <= 150:
                temps.append({"chip": "thermal_zone", "label": type_lbl, "temp": round(t, 1)})
        except:
            pass

def normalize_pci_address(address):
    try:
        domain, bus, slot = address.strip().split(":")
        device, function = slot.split(".")
        domain, bus, device, function = (int(value, 16) for value in (domain, bus, device, function))
        if not (0 <= domain <= 0xffff and 0 <= bus <= 0xff and 0 <= device <= 0x1f and 0 <= function <= 7):
            return None
        return f"{domain:04x}:{bus:02x}:{device:02x}.{function:x}"
    except (ValueError, AttributeError):
        return None

def nvidia_number(value, minimum=0.0, maximum=1e12, integer=False):
    try:
        number = float(value)
        if not math.isfinite(number) or not minimum <= number <= maximum:
            return None
        return int(number) if integer else number
    except (ValueError, TypeError, OverflowError):
        return None

gpus = []
nvidia_devices = set()
try:
    cmd = "nvidia-smi --query-gpu=name,temperature.gpu,utilization.gpu,memory.used,memory.total,clocks.current.graphics,clocks.current.memory,power.draw,power.limit,pci.bus_id --format=csv,noheader,nounits 2>/dev/null"
    for line in os.popen(cmd).read().strip().split("\n"):
        parts = [x.strip() for x in line.split(",")]
        if len(parts) >= 10:
            address = normalize_pci_address(parts[9])
            if address:
                nvidia_devices.add(address)
            gpus.append({
                "device_path": "pci:" + address if address else "",
                "name": parts[0],
                "temp": nvidia_number(parts[1], -40.0, 150.0),
                "load": nvidia_number(parts[2], 0.0, 100.0),
                "mem_used": nvidia_number(parts[3], integer=True),
                "mem_total": nvidia_number(parts[4], integer=True),
                "clock_mhz": nvidia_number(parts[5], maximum=4294967295, integer=True),
                "mem_clock_mhz": nvidia_number(parts[6], maximum=4294967295, integer=True),
                "power_watts": nvidia_number(parts[7]),
                "power_limit_watts": nvidia_number(parts[8]),
                "driver": "NVIDIA Proprietary"
            })
except:
    pass

for card in sorted(glob.glob("/sys/class/drm/card[0-9]*")):
    if "-" not in os.path.basename(card):
        vendor_p = card + "/device/vendor"
        v = (read_optional_text(vendor_p) or "").lower()
        address = normalize_pci_address(os.path.basename(os.path.realpath(card + "/device")))
        if v == "0x10de" and address in nvidia_devices:
            continue
        drv_p = card + "/device/driver"
        try:
            drv_name = os.path.basename(os.readlink(drv_p))
        except OSError:
            drv_name = ""
        v_name = "Intel Graphics" if v == "0x8086" else ("AMD Radeon Graphics" if v == "0x1002" else ("VirtIO GPU" if v == "0x1af4" else ("VMware SVGA II" if v == "0x15ad" else ("QEMU Standard VGA" if v == "0x1234" else ("ASPEED Graphics (BMC)" if v == "0x1a03" else ("Integrated / Linux GPU" if not drv_name else drv_name.upper() + " GPU"))))))
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
                    c_temp = round(millidegrees_to_celsius(raw_t), 1)
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
            "device_path": os.path.realpath(card + "/device"),
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

ac_online = None
ac_states = []
batteries = []
charge_limit = None
for ps in sorted(glob.glob("/sys/class/power_supply/*")):
    t = (read_optional_text(ps + "/type") or "").lower()
    name = os.path.basename(ps).lower()
    if t in ["mains", "ac", "adp"] or (t != "battery" and name.startswith("ac")):
        value = read_optional_text(ps + "/online")
        ac_states.append(True if value == "1" else False if value == "0" else None)
    elif t == "battery" or name.startswith("bat"):
        if read_optional_text(ps + "/present") == "0":
            continue
        status = read_optional_text(ps + "/status") or "Unknown"
        cap = None
        try:
            value = float(read_optional_text(ps + "/capacity"))
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
            
        tech = read_optional_text(ps + "/technology") or ""
        model = read_optional_text(ps + "/model_name") or ""
        mfg = read_optional_text(ps + "/manufacturer") or ""
        if os.path.exists(ps + "/charge_control_end_threshold"):
            try:
                cl = int(open(ps + "/charge_control_end_threshold").read().strip())
                if 1 <= cl <= 100: charge_limit = cl
            except: pass
        
        batteries.append({
            "device_id": os.path.basename(ps),
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
n2, n2_available = read_net_dev()
n_stats_available = n1_available and n2_available
n2_at = time.monotonic()
t_disk_elapsed = max(d2_at - d1_at, 0.001)
t_net_elapsed = max(n2_at - n1_at, 0.001)
p2_net = read_proc_net_map() if collect_process_network else {}
p2_net_at = time.monotonic()
proc_net_elapsed = max(p2_net_at - p1_net_at, 0.001)

def read_network_addresses():
    try:
        output = subprocess.run(["ip", "-j", "address", "show"], capture_output=True, text=True, timeout=2, check=True)
        entries = json.loads(output.stdout)
        if not isinstance(entries, list):
            return {}
    except (OSError, ValueError, subprocess.SubprocessError):
        return {}
    addresses = {}
    for entry in entries:
        if not isinstance(entry, dict) or not isinstance(entry.get("ifname"), str):
            continue
        found = {"inet": [], "inet6": []}
        items = entry.get("addr_info")
        if not isinstance(items, list):
            continue
        for item in items:
            if not isinstance(item, dict) or item.get("family") not in ("inet", "inet6"):
                continue
            try:
                interface = ipaddress.ip_interface(str(item["local"]) + "/" + str(item["prefixlen"]))
                if (item["family"] == "inet") != (interface.version == 4):
                    continue
                found[item["family"]].append((item.get("scope") != "global", str(interface)))
            except (KeyError, TypeError, ValueError):
                continue
        addresses[entry["ifname"]] = {family: min(values)[1] if values else None for family, values in found.items()}
    return addresses

network_addresses = read_network_addresses() if collect_network_details else {}
networks = []
network_rates_available = n_stats_available and bool(n2) and all(
    ifname in n1 and rx2 >= n1[ifname][0] and tx2 >= n1[ifname][1]
    for ifname, (rx2, tx2) in n2.items()
)
total_rx_rate = 0.0
total_tx_rate = 0.0
for ifname, (rx2, tx2) in n2.items():
    rx1, tx1 = n1.get(ifname, (rx2, tx2))
    rx_rate = max(0.0, (rx2 - rx1) / t_net_elapsed)
    tx_rate = max(0.0, (tx2 - tx1) / t_net_elapsed)
    total_rx_rate += rx_rate
    total_tx_rate += tx_rate
    interface_path = "/sys/class/net/" + ifname
    is_wl = os.path.exists(interface_path + "/wireless") or os.path.exists(interface_path + "/phy80211")
    mtu_text = read_optional_text(interface_path + "/mtu")
    try:
        mtu = int(mtu_text) if mtu_text is not None else None
        if mtu is not None and not 0 < mtu <= 4294967295:
            mtu = None
    except (TypeError, ValueError):
        mtu = None
    networks.append({
        "name": ifname,
        "rx_bytes": rx2,
        "tx_bytes": tx2,
        "rx_rate": rx_rate,
        "tx_rate": tx_rate,
        "is_wireless": is_wl,
        "link_state": read_optional_text(interface_path + "/operstate"),
        "mac_address": read_optional_text(interface_path + "/address"),
        "mtu": mtu,
        "ipv4": network_addresses.get(ifname, {}).get("inet"),
        "ipv6": network_addresses.get(ifname, {}).get("inet6")
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
            if not math.isfinite(watts) or watts < 0.0:
                break
            package_watts.append(watts)
        except:
            break
    if len(package_watts) == len(rapl_sources):
        cpu_power = round(sum(package_watts), 2)

if cpu_power is None and len(rapl_sources) <= 1:
    for hw in glob.glob("/sys/class/hwmon/hwmon*"):
        name = (read_optional_text(hw + "/name") or "").lower()
        if any(k in name for k in ["coretemp", "k10temp", "zenpower", "cpu", "rapl", "fam15h", "acpitz"]):
            for pin in glob.glob(hw + "/power*_input") + glob.glob(hw + "/power*_average"):
                try:
                    label_path = pin.rsplit("_", 1)[0] + "_label"
                    label = open(label_path).read().strip().lower()
                    if not (label.startswith("package") or label.startswith("cpu package")):
                        continue
                    v = int(open(pin).read().strip()) / 1e6
                    if math.isfinite(v) and v >= 0.0:
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
    if device.startswith(("loop", "ram", "zram", "dm-", "nbd", "sr")) or (device.startswith("mmcblk") and "boot" in device):
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

active_sockets = None
proc_ports = {}
try:
    socket_output = os.popen("ss -tulpn 2>/dev/null || ss -tunaep 2>/dev/null").read().strip()
    if socket_output:
        active_sockets = []
    for line in socket_output.split("\n")[1:40]:
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
                        net_rate_available = proc_net_elapsed > 0
                    else:
                        net_rate = 0.0
                        net_rate_available = False
                    dr_rate = 0.0
                    dw_rate = 0.0
                    disk_rate = 0.0
                    disk_rate_available = False
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
                        disk_rate_available = io_elapsed > 0

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
                        "net_counters_available": network is not None and network[0] == start_marker,
                        "net_rate": net_rate,
                        "net_rate_available": net_rate_available,
                        "disk_read_bytes": dr2,
                        "disk_write_bytes": dw2,
                        "disk_read_rate": dr_rate,
                        "disk_write_rate": dw_rate,
                        "disk_rate": disk_rate,
                        "disk_rate_available": disk_rate_available,
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

socket_stats = None
try:
    with open("/proc/net/sockstat") as f:
        values = {}
        for line in f:
            fields = line.split()
            if line.startswith("sockets: used"):
                values["total"] = int(fields[2])
            elif line.startswith("TCP: inuse"):
                values["tcp"] = int(fields[2])
            elif line.startswith("UDP: inuse"):
                values["udp"] = int(fields[2])
        if all(key in values for key in ("total", "tcp", "udp")):
            socket_stats = [max(values["total"], values["tcp"] + values["udp"]), values["tcp"], values["udp"]]
except:
    pass

cpu_governor = ""
for p in ["/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"]:
    if os.path.exists(p):
        try:
            cpu_governor = open(p).read().strip()
        except:
            pass

def disk_discard_supported(path):
    try:
        with open(path + "/queue/discard_max_bytes") as value:
            return int(value.read().strip()) > 0
    except (OSError, ValueError):
        return None

def disk_bus_interface(path):
    subsystem = path + "/device/subsystem"
    return os.path.basename(os.path.realpath(subsystem)).upper() if os.path.exists(subsystem) else ""

phys_disks = []
try:
    for dev in sorted(glob.glob("/sys/block/*")):
        dev_name = os.path.basename(dev)
        if not is_base_block_device(dev_name):
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
        rot = None
        try:
            rot = open(dev + "/queue/rotational").read().strip()
        except:
            pass
        is_ssd = False if rot == "1" else (True if rot == "0" or dev_name.startswith("nvme") else None)
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
            "bus_interface": disk_bus_interface(dev),
            "discard_supported": disk_discard_supported(dev),
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

data = {
    "cpu_model": cpu_model,
    "cores": max(cores, 1),
    "cpu_usage": cpu_usage,
    "cpu_core_usages": cpu_core_usages,
    "cpu_freqs": cpu_freqs,
    "cpu_power_cap": cpu_power_cap,
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
    "fans": fans,
    "gpus": gpus,
    "networks": networks,
    "network_stats_available": n_stats_available,
    "network_rates_available": network_rates_available,
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

    let host_key = remote_sensor_host_key(host);
    let known_windows = known_windows_remote_hosts().lock().unwrap().contains(&host_key);
    let parsed = if known_windows {
        match query_remote_windows_sensors(host, include_processes, include_network_details, include_gpu_engines, include_ram_modules, include_network_rates, include_cpu_cores, include_disk_rates, include_thermals)? {
            Some(data) => data,
            None => {
                known_windows_remote_hosts().lock().unwrap().remove(&host_key);
                return Err("Remote host no longer reports Windows through PowerShell".to_string());
            }
        }
    } else {
        let script = remote_sensor_command(script, include_processes, include_process_network, include_network_details);
        let mut sensor_host = host.clone();
        sensor_host.remote_cwd.clear();
        let mut cmd = build_ssh_command(&sensor_host, &script);
        let out = cmd.output().map_err(|e| format!("SSH command execution error: {}", e))?;
        let python_failure = format!("{} {}", String::from_utf8_lossy(&out.stderr).trim(), String::from_utf8_lossy(&out.stdout).trim());
        let mut python_error = if out.status.success() { None } else {
            Some(format!("Remote sensor query failed: {}", python_failure.trim()))
        };
        let python_data: serde_json::Value = if let Some(error) = &python_error {
            serde_json::json!({ "error": error })
        } else {
            match serde_json::from_slice(&out.stdout) {
                Ok(data) => data,
                Err(_error) if remote_python_is_missing(&python_failure) => {
                    python_error = Some(format!("Remote Python sensor collector unavailable: {}", python_failure.trim()));
                    serde_json::json!({ "error": python_error })
                }
                Err(error) => return Err(format!("Failed to parse remote sensor json: {}", error)),
            }
        };
        let windows_host = python_error.as_deref().is_some_and(remote_python_is_missing)
            || python_data["error"].as_str().is_some_and(|error| error.contains("detected Windows"));
        if python_error.is_some() && !windows_host {
            return Err(python_error.unwrap());
        }
        if windows_host {
            match query_remote_windows_sensors(host, include_processes, include_network_details, include_gpu_engines, include_ram_modules, include_network_rates, include_cpu_cores, include_disk_rates, include_thermals) {
                Ok(Some(windows_data)) => {
                    known_windows_remote_hosts().lock().unwrap().insert(host_key);
                    windows_data
                }
                Ok(None) => return Err(python_error.unwrap_or_else(|| python_data["error"].as_str().unwrap_or("Remote Windows sensor collection is unavailable").to_string())),
                Err(error) => return Err(if let Some(python_error) = python_error { format!("{python_error}; Windows collector: {error}") } else { error }),
            }
        } else {
            python_data
        }
    };
    if let Some(error) = parsed["error"].as_str() { return Err(error.to_string()); }

    let cpu_brand = parsed["cpu_model"].as_str().unwrap_or("Remote CPU").to_string();
    let cpu_cores = parsed["cores"].as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "Remote sensor data has no valid logical CPU count".to_string())?;
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

    let cpu_power_cap_watts = parsed["cpu_power_cap"].as_f64()
        .filter(|value| value.is_finite() && (0.0..=f32::MAX as f64).contains(value)).map(|value| value as f32);
    let cpu_power_watts = parsed["cpu_power"].as_f64().map(|v| v as f32)
        .filter(|value| value.is_finite() && *value >= 0.0);

    let mem_total_bytes = parsed["mem_total"].as_u64().filter(|total| *total > 0)
        .ok_or_else(|| "Remote sensor data has no valid total memory reading".to_string())?;
    let mem_avail = parsed["mem_avail"].as_u64().filter(|available| *available <= mem_total_bytes)
        .ok_or_else(|| "Remote sensor data has no valid available memory reading".to_string())?;
    let mem_used_bytes = mem_total_bytes.saturating_sub(mem_avail);
    let mem_usage_pct = if mem_total_bytes > 0 {
        (mem_used_bytes as f32 / mem_total_bytes as f32) * 100.0
    } else {
        0.0
    };

    let swap_total_bytes = parsed["swap_total"].as_u64()
        .ok_or_else(|| "Remote sensor data has no valid total swap reading".to_string())?;
    let swap_free = parsed["swap_free"].as_u64().filter(|free| *free <= swap_total_bytes)
        .ok_or_else(|| "Remote sensor data has no valid available swap reading".to_string())?;
    let swap_used_bytes = swap_total_bytes.saturating_sub(swap_free);
    let swap_usage_pct = if swap_total_bytes > 0 {
        (swap_used_bytes as f32 / swap_total_bytes as f32) * 100.0
    } else {
        0.0
    };

    let total_disk_read = parsed["total_disk_read_rate"].as_f64().filter(|rate| rate.is_finite() && *rate >= 0.0);
    let total_disk_write = parsed["total_disk_write_rate"].as_f64().filter(|rate| rate.is_finite() && *rate >= 0.0);
    let total_disk_rates_available = total_disk_read.is_some() && total_disk_write.is_some();
    let total_disk_read_rate_bps = total_disk_read.unwrap_or(0.0);
    let total_disk_write_rate_bps = total_disk_write.unwrap_or(0.0);

    let mut disks = Vec::new();
    if let Some(arr) = parsed["disks"].as_array() {
        for d in arr {
            let (Some(name), Some(mount_point), Some(total), Some(used), Some(avail)) = (
                d["name"].as_str(), d["mount"].as_str(), d["total"].as_u64(),
                d["used"].as_u64(), d["avail"].as_u64(),
            ) else { continue; };
            if name.is_empty() || mount_point.is_empty() || total == 0 || used > total || avail > total { continue; }
            let pct = (used as f32 / total as f32) * 100.0;
            let r_rate = d["read_rate"].as_f64().filter(|rate| rate.is_finite() && *rate >= 0.0);
            let w_rate = d["write_rate"].as_f64().filter(|rate| rate.is_finite() && *rate >= 0.0);
            let disk_rates_available = r_rate.is_some() && w_rate.is_some();
            let r_total = d["total_read"].as_u64().unwrap_or(0);
            let w_total = d["total_write"].as_u64().unwrap_or(0);

            disks.push(DiskInfo {
                name: name.to_string(),
                mount_point: mount_point.to_string(),
                fs_type: String::new(),
                total_bytes: total,
                available_bytes: avail,
                used_bytes: used,
                usage_pct: pct,
                read_rate_bps: r_rate.unwrap_or(0.0),
                write_rate_bps: w_rate.unwrap_or(0.0),
                disk_rates_available,
                total_read_bytes: r_total,
                total_write_bytes: w_total,
            });
        }
    }

    let mut gpus = Vec::new();
    if let Some(arr) = parsed["gpus"].as_array() {
        for g in arr {
            let mem_used_mb = g["mem_used"].as_u64();
            let mem_total_mb = g["mem_total"].as_u64();
            let memory = match (mem_used_mb, mem_total_mb) {
                (Some(used), Some(total)) if used > total => (None, None),
                values => values,
            };
            gpus.push(GpuSensorInfo { device_path: g["device_path"].as_str().unwrap_or("").to_string(), engine_loads: Vec::new(),
                name: g["name"].as_str().filter(|name| !name.trim().is_empty()).unwrap_or("Unknown GPU").to_string(),
                load_pct: g["load"].as_f64().filter(|v| v.is_finite() && (0.0..=100.0).contains(v)).map(|v| v as f32),
                temp: parse_reported_temperature(&g["temp"]),
                mem_used_mb: memory.0,
                mem_total_mb: memory.1,
                clock_mhz: g["clock_mhz"].as_u64().and_then(|v| u32::try_from(v).ok()),
                mem_clock_mhz: g["mem_clock_mhz"].as_u64().and_then(|v| u32::try_from(v).ok()),
                power_watts: g["power_watts"].as_f64().filter(|v| v.is_finite() && (0.0..=f32::MAX as f64).contains(v)).map(|v| v as f32),
                power_limit_watts: g["power_limit_watts"].as_f64().filter(|v| v.is_finite() && (0.0..=f32::MAX as f64).contains(v)).map(|v| v as f32),
                driver: g["driver"].as_str().unwrap_or("").to_string(),
            });
        }
    }

    if parsed["gpu_engines"].is_array() { apply_windows_gpu_engine_counters(&mut gpus, &parsed["gpu_engines"]); }

    let mut batteries = Vec::new();
    let mut ac_online = None;
    if let Some(p_obj) = parsed.get("power") {
        ac_online = p_obj["ac_online"].as_bool();
        if let Some(b_arr) = p_obj["batteries"].as_array() {
            for b in b_arr {
                let value = |key: &str| b[key].as_f64()
                    .filter(|value| value.is_finite() && *value >= 0.0 && *value <= f32::MAX as f64)
                    .map(|value| value as f32);
                batteries.push(BatteryInfo {
                    device_id: b["device_id"].as_str().filter(|id| !id.trim().is_empty())
                        .or_else(|| b["name"].as_str().filter(|name| !name.trim().is_empty()))
                        .unwrap_or("Unknown battery").to_string(),
                    name: b["name"].as_str().filter(|name| !name.trim().is_empty()).unwrap_or("Unknown battery").to_string(),
                    present: true,
                    status: b["status"].as_str().unwrap_or("Unknown").to_string(),
                    capacity_pct: parse_battery_percentage(&b["capacity_pct"]),
                    power_watts: value("power_watts"),
                    voltage_volts: value("voltage_volts"),
                    energy_wh: value("energy_wh"),
                    energy_full_wh: value("energy_full_wh"),
                    energy_design_wh: value("energy_design_wh"),
                    energy_estimated: b["energy_estimated"].as_bool().unwrap_or(false),
                    health_pct: parse_battery_percentage(&b["health_pct"]),
                    cycle_count: b["cycle_count"].as_u64().and_then(|value| u32::try_from(value).ok()),
                    technology: b["technology"].as_str().unwrap_or("").to_string(),
                    model_name: b["model_name"].as_str().unwrap_or("").to_string(),
                    manufacturer: b["manufacturer"].as_str().unwrap_or("").to_string(),
                });
            }
        }
    }

    let total_battery_pct = reported_battery_charge(&batteries);

    let total_power_watts = if batteries.is_empty() { None } else {
        batteries.iter().map(|battery| battery.power_watts).sum::<Option<f32>>().filter(|watts| watts.is_finite())
    };

    let charge_limit = parsed.get("power").and_then(|p| p.get("charge_limit")).and_then(|v| v.as_u64())
        .and_then(|value| u32::try_from(value).ok()).filter(|value| (1..=100).contains(value));
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
    let mut gpu_temps = Vec::new();

    if let Some(arr) = parsed["temps"].as_array() {
        for t in arr {
            let chip = t["chip"].as_str().unwrap_or("").to_string();
            let label = t["label"].as_str().unwrap_or("").to_string();
            let Some(temp) = parse_reported_temperature(&t["temp"]) else { continue; };
            let cat = categorize_sensor(&label, &chip);

            match cat {
                SensorCategory::Cpu if cpu_temp.is_none() || label.contains("Package") || label == "CPU" => {
                    cpu_temp = Some(temp);
                }
                SensorCategory::Memory => ram_temp = hottest_temperature(ram_temp, temp),
                SensorCategory::Storage => storage_temp = hottest_temperature(storage_temp, temp),
                SensorCategory::Gpu => gpu_temps.push(temp),
                _ => {}
            }

            let crit = parse_reported_temperature(&t["crit"]);
            let max = parse_reported_temperature(&t["max"]);

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
    apply_unambiguous_gpu_temperature(&mut gpus, &gpu_temps);

    let fan_sensors = parse_remote_fan_sensors(&parsed["fans"]);
    let (networks, network_stats_available, network_rates_available) = parse_remote_network_rows(&parsed)?;

    let (total_rx_bytes, total_tx_bytes, total_rx_rate_bps, total_tx_rate_bps) = network_totals(&networks);

    let top_apps = parse_remote_process_rows(&parsed, include_processes)?;

    let active_socket_rows = parse_active_sockets(&parsed["active_sockets"]);
    let active_sockets_available = active_socket_rows.is_some();
    let active_sockets = active_socket_rows.unwrap_or_default();

    let uptime_secs = parsed["uptime"].as_u64().unwrap_or(0);
    let socket_counts = parsed["socket_stats"].as_array().filter(|values| values.len() == 3).and_then(|values| {
        Some((
            usize::try_from(values[0].as_u64()?).ok()?,
            usize::try_from(values[1].as_u64()?).ok()?,
            usize::try_from(values[2].as_u64()?).ok()?,
        ))
    });
    let (socket_total, socket_tcp, socket_udp, socket_stats_available) = match socket_counts {
        Some((total, tcp, udp)) => (total.max(tcp.saturating_add(udp)), tcp, udp, true),
        None => (0, 0, 0, false),
    };
    let cpu_governor = parsed["cpu_governor"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string());

    let mut physical_disks = Vec::new();
    if let Some(arr) = parsed["physical_disks"].as_array() {
        for p in arr {
            let Some(name) = p["name"].as_str().filter(|name| !name.trim().is_empty()) else {
                continue;
            };
            let Some(total_bytes) = p["total_bytes"].as_u64().filter(|size| *size > 0) else {
                continue;
            };
            let is_ssd = p["is_ssd"].as_bool();
            let name = name.to_string();
            let model = p["model"]
                .as_str()
                .filter(|model| !model.trim().is_empty())
                .unwrap_or("Unknown disk")
                .to_string();
            let bus_interface = p["bus_interface"].as_str().unwrap_or("").to_string();
            let smart_status = p["smart_status"].as_str().unwrap_or("").to_string();
            let discard_supported = p["discard_supported"].as_bool();
            let mut partitions = Vec::new();
            if let Some(parts) = p["partitions"].as_array() {
                for part in parts {
                    let Some(part_name) = part["name"].as_str().filter(|name| !name.trim().is_empty()) else {
                        continue;
                    };
                    let Some(size_bytes) = part["size_bytes"].as_u64().filter(|size| *size > 0) else {
                        continue;
                    };
                    let Some(partition_num) = part["partition_num"]
                        .as_u64()
                        .and_then(|number| u32::try_from(number).ok())
                        .filter(|number| *number > 0)
                    else {
                        continue;
                    };
                    partitions.push(DiskPartitionInfo {
                        name: part_name.to_string(),
                        size_bytes,
                        partition_num,
                    });
                }
            }
            let partition_count = p["partition_count"].as_u64().and_then(|count| usize::try_from(count).ok());
            let read_rate = p["read_rate"].as_f64().filter(|rate| rate.is_finite() && *rate >= 0.0);
            let write_rate = p["write_rate"].as_f64().filter(|rate| rate.is_finite() && *rate >= 0.0);
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
                read_rate_bps: read_rate.unwrap_or(0.0),
                write_rate_bps: write_rate.unwrap_or(0.0),
                disk_rates_available: read_rate.is_some() && write_rate.is_some(),
            });
        }
    }

    let cpu_tjunction_max = detect_hardware_cpu_tjunction_max(&temp_sensors);

    Ok(SystemSensorsSnapshot {
        cpu_brand,
        cpu_cores_physical: parsed["physical_cores"].as_u64().and_then(|value| usize::try_from(value).ok()).unwrap_or(0),
        cpu_cores_logical: cpu_cores,
        cpu_global_usage,
        cpu_usage_available: cpu_usage.is_some(),
        cpu_core_usages,
        cpu_frequency_mhz,
        cpu_frequencies_mhz,
        cpu_power_watts,
        cpu_power_cap_watts,
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
        memory_hw: parse_remote_windows_memory_modules(&parsed["memory_modules"]),
        gpus,
        displays: parse_windows_displays(&parsed["gpus"]),
        power,
        disks,
        physical_disks,
        storage_temp,
        total_disk_read_rate_bps,
        total_disk_write_rate_bps,
        total_disk_rates_available,
        networks,
        network_stats_available,
        network_rates_available,
        total_rx_rate_bps,
        total_tx_rate_bps,
        total_rx_bytes,
        total_tx_bytes,
        temp_sensors,
        fan_sensors,
        process_count: parsed["process_count"].as_u64().and_then(|value| usize::try_from(value).ok()),
        top_apps,
        processes_collected: parsed["processes_collected"].as_bool().unwrap_or(include_processes),
        process_network_collected: parsed["process_network_collected"].as_bool().unwrap_or(include_processes && include_process_network),
        process_network_supported: parsed["process_network_supported"].as_bool()
            .unwrap_or(parsed["os_name"].as_str().is_some_and(|name| name.to_ascii_lowercase().contains("linux"))),
        network_details_collected: parsed["network_details_collected"].as_bool().unwrap_or(include_network_details),
        os_name: parsed["os_name"].as_str().unwrap_or("").to_string(),
        os_version: parsed["os_version"].as_str().unwrap_or("").to_string(),
        cpu_governor,
        uptime_secs,
        socket_total,
        socket_tcp,
        socket_udp,
        socket_stats_available,
        active_sockets,
        active_sockets_available,
        hardware_access_unlocked: false,
            hardware_access_error: None,
            #[cfg(windows)]
            windows_telemetry_error: None,
        #[cfg(windows)]
        windows_hardware_sampled_at: None,
        #[cfg(windows)]
        windows_inventory_sampled_at: None,
        #[cfg(windows)]
        windows_hardware_attempted_at: None,
            timestamp: Some(Instant::now()),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn memory_summary_uses_hottest_sensor_independent_of_order() {
        assert_eq!(super::hottest_temperature(Some(43.0), 51.0), Some(51.0));
        assert_eq!(super::hottest_temperature(Some(51.0), 43.0), Some(51.0));
        assert_eq!(super::hottest_temperature(None, 43.0), Some(43.0));
    }

    #[test]
    fn socket_stats_require_all_reported_counters() {
        assert_eq!(parse_socket_stats("sockets: used 20\nTCP: inuse 7 orphan 1\nUDP: inuse 2\n"), Some((20, 7, 2)));
        assert_eq!(parse_socket_stats("sockets: used 20\nTCP: inuse invalid\n"), None);
    }

    #[test]
    fn active_socket_rows_preserve_empty_success_and_drop_malformed_rows() {
        assert!(parse_active_sockets(&serde_json::json!([])).is_some_and(|rows| rows.is_empty()));
        let rows = parse_active_sockets(&serde_json::json!([
            {"process":"sshd", "proto":"TCP", "port":":22", "status":"LISTEN"},
            {"process":"", "proto":"TCP", "port":":22", "status":"LISTEN"}
        ])).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(parse_active_sockets(&serde_json::json!([{"process":""}])).is_none());
        assert!(parse_active_sockets(&serde_json::Value::Null).is_none());
    }

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
            "ESTAB 0 0 a b users:((\"idle\",pid=43,fd=5)) bytes_sent:0 bytes_received:0\n",
            "ESTAB 0 0 a b bytes_sent:999 bytes_received:999\n",
        );
        let counters = parse_process_tcp_counters(output);
        assert_eq!(counters.get(&42), Some(&(100, 150)));
        assert_eq!(counters.get(&43), Some(&(0, 0)));
        assert_eq!(counters.len(), 1);
    }

    #[test]
    fn test_sensor_categorization() {
        assert_eq!(categorize_sensor("Package id 0", "coretemp"), SensorCategory::Cpu);
        assert_eq!(categorize_sensor("GPU Core", "sysinfo"), SensorCategory::Gpu);
        assert_eq!(categorize_sensor("Core", "amdgpu"), SensorCategory::Gpu);
        assert_eq!(categorize_sensor("Core 0", "coretemp"), SensorCategory::Cpu);
        assert_eq!(millidegrees_to_celsius(100.0), 0.1);
        assert_eq!(millidegrees_to_celsius(-100.0), -0.1);
        assert_eq!(millidegrees_to_celsius(61000.0), 61.0);
        assert_eq!(parse_reported_temperature(&serde_json::json!(0)), Some(0.0));
        assert_eq!(parse_reported_temperature(&serde_json::Value::Null), None);
        assert_eq!(parse_reported_temperature(&serde_json::json!(999)), None);
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
    /// Native counters are sampled frequently; PowerShell hardware queries are throttled.
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
        }
        let hardware_query_due = self.cached_windows_hardware.is_none()
            || windows_hardware_query_due(self.last_windows_hardware_query, now);
        if hardware_query_due {
            let sample = collect_windows_hardware(inventory_due);
            let sampled_at = Instant::now();
            self.last_windows_hardware_query = sampled_at;
            self.windows_hardware.windows_hardware_attempted_at = Some(sampled_at);
            if sample.is_ok() {
                self.windows_hardware.windows_hardware_sampled_at = Some(sampled_at);
            }
            if inventory_due {
                match &sample {
                    Ok(data) => {
                        update_windows_inventory(&mut self.windows_hardware, data);
                        self.windows_network_adapters = data["network_adapters"].clone();
                        let errors: Vec<&str> = ["gpu_error", "ram_error", "disk_inventory_error"].iter().filter_map(|key| data[*key].as_str()).collect();
                        self.windows_hardware.windows_telemetry_error = (!errors.is_empty()).then(|| errors.join(" • "));
                        if errors.is_empty() {
                            self.windows_hardware.windows_inventory_sampled_at = Some(sampled_at);
                        }
                        self.last_hardware_scan = sampled_at;
                    }
                    Err(_) => {
                        self.windows_network_adapters = serde_json::Value::Null;
                        self.windows_hardware.windows_telemetry_error = Some("Hardware inventory refresh failed; previous inventory retained".into());
                    }
                }
            }
            self.cached_windows_hardware = Some(sample);
        }
        let sample = self.cached_windows_hardware.clone().unwrap_or_else(|| Err("Windows sensor query has not run".into()));
        let mut snap = self.windows_hardware.clone();
        match &sample {
            Ok(data) => {
                let live = parse_windows_hardware(data);
                snap.power = live.power;
                snap.cpu_temp = windows_cpu_package_metric(&data["sensor_hardware"], &data["hardware_sensors"], "Temperature", -40.0, 150.0);
                snap.cpu_power_watts = windows_cpu_package_metric(&data["sensor_hardware"], &data["hardware_sensors"], "Power", 0.0, f32::MAX as f64);
                snap.ram_temp = live.ram_temp;
                snap.storage_temp = live.storage_temp;
                snap.temp_sensors = live.temp_sensors;
                snap.fan_sensors = live.fan_sensors;
                snap.total_disk_read_rate_bps = live.total_disk_read_rate_bps;
                snap.total_disk_write_rate_bps = live.total_disk_write_rate_bps;
                snap.total_disk_rates_available = live.total_disk_rates_available;
                apply_windows_gpu_sensors(&mut snap.gpus, &data["sensor_hardware"], &data["hardware_sensors"]);
                apply_windows_gpu_engine_counters(&mut snap.gpus, &data["gpu_engines"]);
                let mut errors: Vec<String> = snap.windows_telemetry_error.take().into_iter().collect();
                errors.extend(["battery_error", "temp_error"].iter().filter_map(|key| data[*key].as_str()).map(str::to_owned));
                snap.windows_telemetry_error = (!errors.is_empty()).then(|| errors.join(" • "));
            }
            Err(error) => {
                clear_windows_live_hardware_readings(&mut snap);
                snap.windows_telemetry_error = Some(error.clone());
            }
        }
        snap.power.ac_online = windows_ac_online();
        snap.cpu_brand = self.sys.cpus().first().map(|c| c.brand().to_owned()).unwrap_or_default();
        snap.cpu_cores_physical = self.sys.physical_core_count().unwrap_or(0);
        snap.cpu_cores_logical = self.sys.cpus().len();
        let cpu_usage = normalize_total_cpu_percent(self.sys.global_cpu_usage(), snap.cpu_cores_logical);
        snap.cpu_global_usage = cpu_usage.unwrap_or(0.0);
        snap.cpu_usage_available = cpu_usage.is_some();
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
            let mount_point = d.mount_point().display().to_string();
            DiskInfo {
                // A volume GUID deduplicates mount paths; retain the path for display and drive counters.
                name: windows_volume_id(&mount_point).unwrap_or_else(|| mount_point.clone()),
                mount_point,
                fs_type: d.file_system().to_string_lossy().into_owned(),
                total_bytes: total, available_bytes: available,
                used_bytes: total.saturating_sub(available),
                usage_pct: percentage(total.saturating_sub(available), total),
                ..Default::default()
            }
        }).collect();
        if let Ok(data) = &sample {
            apply_windows_volume_rates(&mut snap.disks, &data["logical_disks"]);
        }
        let elapsed = now.duration_since(self.prev_net_time).as_secs_f64().max(0.001);
        let mut network_rates_available = !self.networks.is_empty();
        for (name, data) in &self.networks {
            let rx = data.total_received();
            let tx = data.total_transmitted();
            let previous = self.prev_net_rx.insert(name.clone(), rx).zip(self.prev_net_tx.insert(name.clone(), tx));
            network_rates_available &= network_rate_sample_available(previous, (rx, tx));
            let (prev_rx, prev_tx) = previous.unwrap_or((rx, tx));
            let rx_rate = rx.saturating_sub(prev_rx) as f64 / elapsed;
            let tx_rate = tx.saturating_sub(prev_tx) as f64 / elapsed;
            snap.networks.push(NetInterfaceInfo {
                name: name.clone(), rx_bytes: rx, tx_bytes: tx,
                rx_rate_bps: rx_rate, tx_rate_bps: tx_rate,
                is_wireless: windows_is_wireless_interface(name, &self.windows_network_adapters),
                ..Default::default()
            });
        }
        self.prev_net_rx.retain(|name, _| self.networks.iter().any(|(current, _)| current == name));
        self.prev_net_tx.retain(|name, _| self.networks.iter().any(|(current, _)| current == name));
        (snap.total_rx_bytes, snap.total_tx_bytes, snap.total_rx_rate_bps, snap.total_tx_rate_bps) = network_totals(&snap.networks);
        snap.network_stats_available = !snap.networks.is_empty();
        snap.network_rates_available = network_rates_available;
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
                    disk_rate_available: process_rate_available(self.prev_proc_disk.contains_key(&identity), elapsed),
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

#[cfg(windows)]
const WINDOWS_HARDWARE_QUERY_INTERVAL: Duration = Duration::from_secs(3);

#[cfg(any(windows, test))]
fn windows_hardware_query_due(last: Instant, now: Instant) -> bool {
    now.saturating_duration_since(last) >= Duration::from_secs(3)
}

#[cfg(windows)]
fn windows_volume_id(mount_point: &str) -> Option<String> {
    let mount_path = windows_volume_mount_path(mount_point)?;
    let mount_path: Vec<u16> = mount_path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut volume_name = [0u16; 261];
    let succeeded = unsafe {
        GetVolumeNameForVolumeMountPointW(mount_path.as_ptr(), volume_name.as_mut_ptr(), volume_name.len() as u32)
    };
    if succeeded == 0 { return None; }
    let len = volume_name.iter().position(|unit| *unit == 0).unwrap_or(volume_name.len());
    Some(String::from_utf16_lossy(&volume_name[..len]))
}

#[cfg(any(windows, test))]
fn windows_volume_mount_path(mount_point: &str) -> Option<String> {
    let path = mount_point.trim_end_matches(|ch| ch == '\\' || ch == '/');
    (!path.is_empty()).then(|| format!("{path}\\"))
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetVolumeNameForVolumeMountPointW(volume_mount_point: *const u16, volume_name: *mut u16, buffer_length: u32) -> i32;
}

#[cfg(test)]
mod windows_hardware_query_schedule_tests {
    use std::time::{Duration, Instant};

    #[test]
    fn throttles_hardware_queries_to_three_seconds() {
        let last = Instant::now();
        assert!(!super::windows_hardware_query_due(last, last + Duration::from_secs(2)));
        assert!(super::windows_hardware_query_due(last, last + Duration::from_secs(3)));
    }
}

#[cfg(test)]
mod windows_volume_mount_path_tests {
    #[test]
    fn adds_the_required_trailing_separator_to_volume_mount_paths() {
        assert_eq!(super::windows_volume_mount_path("C:"), Some("C:\\".into()));
        assert_eq!(super::windows_volume_mount_path("C:\\Data\\"), Some("C:\\Data\\".into()));
        assert_eq!(super::windows_volume_mount_path(""), None);
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
$result=@{gpu=@();ram=@();battery=@();temp=@();gpu_engines=@()};
if (__INVENTORY__) {
  try { $result.gpu=@(Get-CimInstance Win32_VideoController -ErrorAction Stop | Select-Object Name,DriverVersion,CurrentHorizontalResolution,CurrentVerticalResolution,CurrentRefreshRate) } catch { $result.gpu_error="GPU inventory query failed: $($_.Exception.Message)" }
  try { $result.ram=@(Get-CimInstance Win32_PhysicalMemory -ErrorAction Stop | Select-Object Manufacturer,Capacity,ConfiguredClockSpeed,SMBIOSMemoryType,FormFactor,DeviceLocator) } catch { $result.ram_error="RAM inventory query failed: $($_.Exception.Message)" }
  try { $result.physical_disks=@(Get-CimInstance Win32_DiskDrive -ErrorAction Stop | Select-Object DeviceID,Model,InterfaceType,Size,Partitions) } catch { $result.disk_inventory_error="Disk inventory query failed: $($_.Exception.Message)" }
  try { $result.network_adapters=@(Get-NetAdapter -IncludeHidden -ErrorAction Stop | Select-Object Name,ifType) } catch { $result.network_adapters=$null }
}
try { $result.disk=Get-CimInstance Win32_PerfFormattedData_PerfDisk_PhysicalDisk -Filter "Name='_Total'" -ErrorAction Stop | Select-Object -First 1 DiskReadBytesPerSec,DiskWriteBytesPerSec } catch { $result.disk=$null }
try { $result.gpu_engines=@(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -ErrorAction Stop | Select-Object Name,UtilizationPercentage) } catch { $result.gpu_engines=@() }
try { $result.logical_disks=@(Get-CimInstance Win32_PerfFormattedData_PerfDisk_LogicalDisk -ErrorAction Stop | Select-Object Name,DiskReadBytesPerSec,DiskWriteBytesPerSec) } catch { $result.logical_disks=@() }
try { $result.battery=@(Get-CimInstance Win32_Battery -ErrorAction Stop | Select-Object DeviceID,Name,EstimatedChargeRemaining,BatteryStatus,DesignVoltage,DesignCapacity,FullChargeCapacity) } catch { $result.battery_error="Battery query failed: $($_.Exception.Message)" }
try {
  $sensors=@(Get-CimInstance -Namespace root/LibreHardwareMonitor -ClassName Sensor -ErrorAction Stop | Select-Object Name,Value,Identifier,Parent,SensorType)
  $result.temp=@($sensors | Where-Object SensorType -eq Temperature)
  $result.hardware_sensors=$sensors
  $result.sensor_hardware=@(Get-CimInstance -Namespace root/LibreHardwareMonitor -ClassName Hardware -ErrorAction Stop | Where-Object { $_.HardwareType -like 'Gpu*' -or $_.HardwareType -eq 'Cpu' } | Select-Object Name,Identifier,HardwareType)
} catch { $result.temp_error="Temperature and CPU/GPU sensor query failed: $($_.Exception.Message)" }
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
fn windows_cpu_package_metric(hardware: &serde_json::Value, sensors: &serde_json::Value, kind: &str, min: f64, max: f64) -> Option<f32> {
    let hardware = hardware.as_array()?;
    let mut cpus = hardware.iter().filter(|device| device["HardwareType"].as_str() == Some("Cpu"));
    let cpu = cpus.next()?;
    if cpus.next().is_some() { return None; }
    let identifier = cpu["Identifier"].as_str().filter(|id| !id.is_empty())?;
    if hardware.iter().filter(|device| device["Identifier"].as_str() == Some(identifier)).count() != 1 { return None; }
    let mut readings = sensors.as_array()?.iter().filter(|sensor| {
        sensor["Parent"].as_str() == Some(identifier)
            && sensor["SensorType"].as_str().is_some_and(|value| value.eq_ignore_ascii_case(kind))
            && sensor["Name"].as_str().is_some_and(|value| value.to_ascii_lowercase().contains("package"))
    });
    let value = readings.next()?["Value"].as_f64()?;
    if readings.next().is_some() || !value.is_finite() || !(min..=max).contains(&value) { return None; }
    Some(value as f32)
}

#[cfg(any(windows, test))]
fn normalize_windows_hardware_name(name: &str) -> String {
    name.to_ascii_lowercase().replace("(r)", "").replace("(tm)", "")
        .chars().filter(|ch| ch.is_ascii_alphanumeric()).collect()
}

fn windows_memory_type(code: u64) -> Option<&'static str> {
    match code {
        18 => Some("DDR"),
        19 => Some("DDR2"),
        20 => Some("DDR2 FB-DIMM"),
        24 => Some("DDR3"),
        25 => Some("FBD2"),
        26 => Some("DDR4"),
        27 => Some("LPDDR"),
        28 => Some("LPDDR2"),
        29 => Some("LPDDR3"),
        30 => Some("LPDDR4"),
        32 => Some("HBM"),
        33 => Some("HBM2"),
        34 => Some("DDR5"),
        35 => Some("LPDDR5"),
        36 => Some("HBM3"),
        _ => None,
    }
}

fn windows_memory_form_factor(code: u64) -> Option<&'static str> {
    match code {
        7 => Some("SIMM"),
        8 => Some("DIMM"),
        11 => Some("RIMM"),
        12 => Some("SODIMM"),
        13 => Some("SRIMM"),
        _ => None,
    }
}

#[cfg(any(windows, test))]
fn apply_windows_gpu_sensors(gpus: &mut [GpuSensorInfo], hardware: &serde_json::Value, sensors: &serde_json::Value) {
    for gpu in gpus.iter_mut() {
        gpu.device_path.clear();
        gpu.load_pct = None;
        gpu.temp = None;
        gpu.clock_mhz = None;
        gpu.power_watts = None;
    }
    let Some(hardware) = hardware.as_array() else { return; };
    let Some(sensors) = sensors.as_array() else { return; };
    let names: Vec<_> = gpus.iter().map(|gpu| normalize_windows_hardware_name(&gpu.name)).collect();
    for gpu in gpus {
        let name = gpu.name.trim();
        let normalized_name = normalize_windows_hardware_name(name);
        if normalized_name.is_empty() || names.iter().filter(|candidate| **candidate == normalized_name).count() != 1 { continue; }
        let mut devices = hardware.iter().filter(|device| {
            device["Name"].as_str().is_some_and(|candidate| normalize_windows_hardware_name(candidate) == normalized_name)
                && device["HardwareType"].as_str().is_some_and(|kind| kind.starts_with("Gpu"))
        });
        let Some(device) = devices.next() else { continue; };
        if devices.next().is_some() { continue; }
        let Some(identifier) = device["Identifier"].as_str().filter(|id| !id.is_empty()) else { continue; };
        if hardware.iter().filter(|device| device["Identifier"].as_str() == Some(identifier)).count() != 1 { continue; }
        let metric = |kind: &str, label: &str, min: f64, max: f64| {
            let mut matching = sensors.iter().filter(|sensor| {
                sensor["Parent"].as_str() == Some(identifier)
                    && sensor["SensorType"].as_str().is_some_and(|value| value.eq_ignore_ascii_case(kind))
                    && sensor["Name"].as_str().is_some_and(|value| value.eq_ignore_ascii_case(label))
            });
            let value = matching.next()?["Value"].as_f64()?;
            if matching.next().is_some() || !value.is_finite() || !(min..=max).contains(&value) { return None; }
            Some(value)
        };
        gpu.device_path = identifier.to_owned();
        gpu.load_pct = metric("Load", "GPU Core", 0.0, 100.0).map(|value| value as f32);
        gpu.temp = metric("Temperature", "GPU Core", -40.0, 150.0)
            .or_else(|| metric("Temperature", "GPU Temperature", -40.0, 150.0))
            .map(|value| value as f32);
        gpu.clock_mhz = metric("Clock", "GPU Core", 0.0, u32::MAX as f64).map(|value| value.round() as u32);
        gpu.power_watts = metric("Power", "GPU Package", 0.0, f32::MAX as f64).map(|value| value as f32);
    }
}

fn apply_windows_gpu_engine_counters(gpus: &mut [GpuSensorInfo], counters: &serde_json::Value) {
    for gpu in gpus.iter_mut() { gpu.engine_loads.clear(); }
    // GPU Engine instance names contain adapter LUIDs, but Win32_VideoController does not expose a safe matching key.
    let [gpu] = gpus else { return; };
    let Some(counters) = counters.as_array() else { return; };
    let mut engines = std::collections::BTreeMap::<(String, String), f32>::new();
    for counter in counters {
        let Some(name) = counter["Name"].as_str() else { continue; };
        let Some((prefix, kind)) = name.rsplit_once("_engtype_") else { continue; };
        let Some(kind) = kind.split('_').next().filter(|kind| !kind.is_empty()) else { continue; };
        let Some((_, adapter_engine)) = prefix.split_once("_luid_") else { continue; };
        let Some((adapter, engine)) = adapter_engine.rsplit_once("_eng_") else { continue; };
        let Some(value) = counter["UtilizationPercentage"].as_f64()
            .filter(|value| value.is_finite() && (0.0..=100.0).contains(value)) else { continue; };
        let load = engines.entry((format!("{adapter}_eng_{engine}"), kind.to_owned())).or_default();
        *load = (*load + value as f32).min(100.0);
    }
    let mut peaks = std::collections::BTreeMap::<String, f32>::new();
    for ((_, kind), load) in engines {
        peaks.entry(kind).and_modify(|peak| *peak = peak.max(load)).or_insert(load);
    }
    gpu.engine_loads = peaks.into_iter().map(|(kind, load)| (format!("{kind} busiest engine"), load)).collect();
}

#[cfg(any(windows, test))]
fn windows_is_wireless_interface(name: &str, adapters: &serde_json::Value) -> Option<bool> {
    let mut matching = adapters.as_array()?.iter().filter(|adapter| {
        adapter["Name"].as_str().is_some_and(|alias| alias.eq_ignore_ascii_case(name))
    });
    let adapter = matching.next()?;
    // IF_TYPE_IEEE80211; missing or ambiguous metadata stays unknown.
    if matching.next().is_some() { return None; }
    adapter["ifType"].as_u64().map(|kind| kind == 71)
}

#[cfg(any(windows, test))]
fn windows_drive_letter(path: &str) -> Option<&str> {
    let drive = path.trim_end_matches(['\\', '/']);
    let bytes = drive.as_bytes();
    (bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':').then_some(drive)
}

#[cfg(any(windows, test))]
fn apply_windows_volume_rates(disks: &mut [DiskInfo], samples: &serde_json::Value) {
    for disk in disks {
        disk.disk_rates_available = false;
        let Some(drive) = windows_drive_letter(&disk.mount_point) else { continue; };
        let mut matches = samples.as_array().into_iter().flatten().filter(|sample| {
            sample["Name"].as_str().is_some_and(|name| name.eq_ignore_ascii_case(drive))
        });
        let Some(sample) = matches.next() else { continue; };
        if matches.next().is_some() { continue; }
        let read = sample["DiskReadBytesPerSec"].as_f64().filter(|value| value.is_finite() && *value >= 0.0);
        let write = sample["DiskWriteBytesPerSec"].as_f64().filter(|value| value.is_finite() && *value >= 0.0);
        if let (Some(read), Some(write)) = (read, write) {
            disk.read_rate_bps = read;
            disk.write_rate_bps = write;
            disk.disk_rates_available = true;
        }
    }
}

#[cfg(any(windows, test))]
fn update_windows_inventory(cached: &mut SystemSensorsSnapshot, data: &serde_json::Value) {
    let inventory = parse_windows_hardware(data);
    if data["gpu_error"].is_null() {
        cached.gpus = inventory.gpus;
        cached.displays = inventory.displays;
    }
    if data["ram_error"].is_null() { cached.memory_hw = inventory.memory_hw; }
    if data["disk_inventory_error"].is_null() { cached.physical_disks = inventory.physical_disks; }
}

#[cfg(any(windows, test))]
fn clear_windows_live_hardware_readings(snapshot: &mut SystemSensorsSnapshot) {
    snapshot.cpu_power_watts = None;
    snapshot.cpu_temp = None;
    snapshot.ram_temp = None;
    snapshot.storage_temp = None;
    snapshot.temp_sensors.clear();
    snapshot.fan_sensors.clear();
    snapshot.power = PowerSupplyInfo::default();
    snapshot.total_disk_read_rate_bps = 0.0;
    snapshot.total_disk_write_rate_bps = 0.0;
    snapshot.total_disk_rates_available = false;
    for gpu in &mut snapshot.gpus {
        gpu.load_pct = None;
        gpu.temp = None;
        gpu.mem_used_mb = None;
        gpu.mem_total_mb = None;
        gpu.clock_mhz = None;
        gpu.mem_clock_mhz = None;
        gpu.power_watts = None;
        gpu.power_limit_watts = None;
        gpu.engine_loads.clear();
    }
    for disk in &mut snapshot.physical_disks {
        disk.read_rate_bps = 0.0;
        disk.write_rate_bps = 0.0;
        disk.disk_rates_available = false;
    }
}

#[cfg(windows)]
#[repr(C)]
struct WindowsPowerStatus {
    ac_line_status: u8,
    battery_flag: u8,
    battery_life_percent: u8,
    reserved: u8,
    battery_life_time: u32,
    battery_full_life_time: u32,
}

#[cfg(windows)]
fn windows_ac_online() -> Option<bool> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemPowerStatus(status: *mut WindowsPowerStatus) -> i32;
    }
    let mut status = WindowsPowerStatus {
        ac_line_status: 255,
        battery_flag: 255,
        battery_life_percent: 255,
        reserved: 0,
        battery_life_time: u32::MAX,
        battery_full_life_time: u32::MAX,
    };
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 { return None; }
    windows_ac_line_status(status.ac_line_status)
}

#[cfg(any(windows, test))]
fn windows_ac_line_status(value: u8) -> Option<bool> {
    match value {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

#[cfg(any(windows, test))]
fn windows_battery_status(value: Option<u64>) -> &'static str {
    match value {
        Some(1) => "Discharging",
        Some(2) => "AC connected",
        Some(3) => "Fully charged",
        Some(4) => "Low",
        Some(5) => "Critical",
        Some(6..=9) => "Charging",
        Some(11) => "Partially charged",
        _ => "Unknown",
    }
}

#[cfg(any(windows, test))]
fn parse_windows_hardware(data: &serde_json::Value) -> SystemSensorsSnapshot {
    let mut snap = SystemSensorsSnapshot::default();
    if let Some(disks) = data["physical_disks"].as_array() {
        snap.physical_disks = disks.iter().filter_map(|disk| {
            let name = disk["DeviceID"].as_str().map(str::trim).filter(|name| !name.is_empty())?;
            let total_bytes = disk["Size"].as_str().and_then(|size| size.parse::<u64>().ok())
                .or_else(|| disk["Size"].as_u64()).filter(|size| *size > 0)?;
            Some(PhysicalDiskInfo {
                name: name.rsplit('\\').next().unwrap_or(name).to_owned(),
                model: disk["Model"].as_str().map(str::trim).filter(|model| !model.is_empty()).unwrap_or("Unknown disk").to_owned(),
                bus_interface: disk["InterfaceType"].as_str().unwrap_or_default().to_owned(),
                total_bytes,
                partition_count: disk["Partitions"].as_u64().and_then(|count| usize::try_from(count).ok()),
                // Win32_DiskDrive does not reliably distinguish SSDs from rotational media.
                is_ssd: None,
                ..Default::default()
            })
        }).collect();
    }
    if let (Some(read), Some(write)) = (
        data["disk"]["DiskReadBytesPerSec"].as_f64().filter(|value| value.is_finite() && *value >= 0.0),
        data["disk"]["DiskWriteBytesPerSec"].as_f64().filter(|value| value.is_finite() && *value >= 0.0),
    ) {
        snap.total_disk_read_rate_bps = read;
        snap.total_disk_write_rate_bps = write;
        snap.total_disk_rates_available = true;
    }
    if let Some(gpus) = data["gpu"].as_array() {
        snap.displays = parse_windows_displays(&data["gpu"]);
        snap.gpus = gpus.iter().filter_map(|g| {
            let name = g["Name"].as_str().map(str::trim).filter(|name| !name.is_empty())?;
            Some(GpuSensorInfo { device_path: String::new(), engine_loads: Vec::new(),
                name: name.into(),
                driver: g["DriverVersion"].as_str().unwrap_or_default().into(),
                ..Default::default()
            })
        }).collect();
    }
    if let Some(temps) = data["temp"].as_array() {
        for t in temps {
            let Some(temp) = parse_reported_temperature(&t["Value"]) else { continue; };
            let name = t["Name"].as_str().unwrap_or("Temperature");
            let id = t["Identifier"].as_str().unwrap_or_default();
            let category = categorize_sensor(name, id);
            match category {
                SensorCategory::Cpu => {},
                SensorCategory::Gpu => {},
                SensorCategory::Memory => snap.ram_temp = hottest_temperature(snap.ram_temp, temp as f32),
                SensorCategory::Storage => snap.storage_temp = hottest_temperature(snap.storage_temp, temp as f32),
                _ => {},
            }
            snap.temp_sensors.push(TempSensor { name: name.into(), chip: "LibreHardwareMonitor".into(),
                temp: temp as f32, category, ..Default::default() });
        }
    }
    snap.fan_sensors = parse_windows_fan_sensors(&data["hardware_sensors"]);
    if let Some(batteries) = data["battery"].as_array() {
        for b in batteries {
            let percent = b["EstimatedChargeRemaining"].as_f64().filter(|v| v.is_finite() && (0.0..=100.0).contains(v));
            let energy_full_wh = b["FullChargeCapacity"].as_u64().filter(|value| *value > 0).map(|value| value as f32 / 1000.0);
            let energy_design_wh = b["DesignCapacity"].as_u64().filter(|value| *value > 0).map(|value| value as f32 / 1000.0);
            let health_pct = energy_full_wh.zip(energy_design_wh)
                .filter(|(_, design)| *design > 0.0)
                .map(|(full, design)| ((full / design) * 100.0).clamp(0.0, 100.0));
            snap.power.batteries.push(BatteryInfo {
                device_id: b["DeviceID"].as_str().filter(|id| !id.trim().is_empty())
                    .or_else(|| b["Name"].as_str().filter(|name| !name.trim().is_empty()))
                    .unwrap_or("Battery").to_string(),
                name: b["Name"].as_str().unwrap_or("Battery").into(), present: true,
                capacity_pct: percent.map(|value| value as f32),
                status: windows_battery_status(b["BatteryStatus"].as_u64()).into(),
                voltage_volts: b["DesignVoltage"].as_u64().filter(|value| *value > 0).map(|value| value as f32 / 1000.0),
                energy_wh: percent.zip(energy_full_wh).map(|(percent, full)| percent as f32 * full / 100.0),
                energy_full_wh,
                energy_design_wh,
                energy_estimated: percent.is_some() && energy_full_wh.is_some(),
                health_pct,
                ..Default::default()
            });
        }
        if !snap.power.batteries.is_empty() {
            snap.power.total_battery_pct = reported_battery_charge(&snap.power.batteries);
        }
    }
    if let Some(ram) = data["ram"].as_array().filter(|r| !r.is_empty()) {
        let memory_types: std::collections::BTreeSet<_> = ram.iter()
            .filter_map(|module| module["SMBIOSMemoryType"].as_u64().and_then(windows_memory_type))
            .collect();
        let form_factors: std::collections::BTreeSet<_> = ram.iter()
            .filter_map(|module| module["FormFactor"].as_u64().and_then(windows_memory_form_factor))
            .collect();
        snap.memory_hw = Some(MemoryHardwareInfo {
            slot_summary: format!("{} populated modules", ram.len()),
            memory_type: memory_types.into_iter().collect::<Vec<_>>().join(" / "),
            form_factor: form_factors.into_iter().collect::<Vec<_>>().join(" / "),
            module_spec: ram.iter().map(|module| {
                let mut details = Vec::new();
                if let Some(manufacturer) = module["Manufacturer"].as_str().map(str::trim).filter(|value| !value.is_empty()) {
                    details.push(manufacturer.to_string());
                }
                if let Some(kind) = module["SMBIOSMemoryType"].as_u64().and_then(windows_memory_type) {
                    details.push(kind.to_string());
                }
                if let Some(speed) = module["ConfiguredClockSpeed"].as_u64().filter(|value| *value > 0) {
                    details.push(format!("{speed} MHz"));
                }
                if let Some(capacity) = module["Capacity"].as_u64().filter(|value| *value > 0) {
                    details.push(format_bytes(capacity));
                }
                if let Some(form) = module["FormFactor"].as_u64().and_then(windows_memory_form_factor) {
                    details.push(form.to_string());
                }
                if let Some(locator) = module["DeviceLocator"].as_str().map(str::trim).filter(|value| !value.is_empty()) {
                    details.push(locator.to_string());
                }
                if details.is_empty() { "Module details unavailable".to_string() } else { details.join(" · ") }
            }).collect::<Vec<_>>().join("; "),
            ..Default::default()
        });
    }
    snap
}

fn parse_windows_display(controller: &serde_json::Value) -> Option<DisplayInfo> {
    let dimension = |key: &str| controller[key].as_u64().and_then(|value| u32::try_from(value).ok()).filter(|value| *value > 0);
    let (width, height) = (dimension("CurrentHorizontalResolution")?, dimension("CurrentVerticalResolution")?);
    let refresh_hz = dimension("CurrentRefreshRate").map_or(0.0, |value| value as f32);
    Some(DisplayInfo {
        name: controller["Name"].as_str().unwrap_or("Display").to_string(),
        is_connected: true,
        is_enabled: true,
        resolution: format!("{width} × {height}"),
        width,
        height,
        refresh_hz,
        bit_depth: "Not reported".into(),
        color_space: "Not reported".into(),
        connector_type: "Not reported".into(),
        scaling: "Not reported".into(),
        vrr_support: "Not reported".into(),
        ..Default::default()
    })
}

fn parse_windows_displays(controllers: &serde_json::Value) -> Vec<DisplayInfo> {
    controllers.as_array().map(|items| items.iter().filter_map(parse_windows_display).collect()).unwrap_or_default()
}

#[cfg(test)]
mod windows_display_tests {
    #[test]
    fn only_reports_display_modes_returned_by_windows() {
        let display = super::parse_windows_display(&serde_json::json!({
            "Name": "Adapter", "CurrentHorizontalResolution": 1920,
            "CurrentVerticalResolution": 1080, "CurrentRefreshRate": 144
        })).unwrap();
        assert_eq!(display.resolution, "1920 × 1080");
        assert_eq!(display.refresh_hz, 144.0);
        assert_eq!(display.bit_depth, "Not reported");
        assert_eq!(display.connector_type, "Not reported");
        assert!(super::parse_windows_display(&serde_json::json!({"Name":"Adapter"})).is_none());
        assert_eq!(super::parse_windows_displays(&serde_json::json!([
            {"Name":"Display 1", "CurrentHorizontalResolution":1920, "CurrentVerticalResolution":1080},
            {"Name":"Display 2", "CurrentHorizontalResolution":2560, "CurrentVerticalResolution":1440}
        ])).len(), 2);
    }
}

#[cfg(any(windows, test))]
fn parse_windows_fan_sensors(sensors: &serde_json::Value) -> Vec<FanSensorInfo> {
    sensors.as_array().into_iter().flatten().filter_map(|sensor| {
        if !sensor["SensorType"].as_str().is_some_and(|kind| kind.eq_ignore_ascii_case("Fan")) { return None; }
        let rpm = sensor["Value"].as_f64().filter(|value| value.is_finite() && (0.0..=100_000.0).contains(value))? as f32;
        Some(FanSensorInfo {
            name: sensor["Name"].as_str().filter(|name| !name.trim().is_empty()).unwrap_or("Fan").to_owned(),
            chip: sensor["Parent"].as_str().unwrap_or("LibreHardwareMonitor").to_owned(),
            rpm,
        })
    }).collect()
}

fn parse_remote_fan_sensors(sensors: &serde_json::Value) -> Vec<FanSensorInfo> {
    sensors.as_array().into_iter().flatten().filter_map(|sensor| {
        let value = sensor.get("rpm").or_else(|| sensor.get("Value"))?.as_f64()
            .filter(|value| value.is_finite() && (0.0..=100_000.0).contains(value))? as f32;
        Some(FanSensorInfo {
            name: sensor.get("label").or_else(|| sensor.get("Name")).and_then(serde_json::Value::as_str)
                .filter(|name| !name.trim().is_empty()).unwrap_or("Fan").to_owned(),
            chip: sensor.get("chip").or_else(|| sensor.get("Parent")).and_then(serde_json::Value::as_str).unwrap_or("").to_owned(),
            rpm: value,
        })
    }).collect()
}

#[cfg(test)]
mod windows_sensor_tests {
    use super::*;
    #[test]
    fn windows_fan_parser_keeps_only_reported_fan_rpm() {
        let fans = parse_windows_fan_sensors(&serde_json::json!([
            {"SensorType":"Fan", "Name":"CPU Fan", "Parent":"CPU", "Value":1450.0},
            {"SensorType":"Temperature", "Name":"CPU", "Value":65.0},
            {"SensorType":"Fan", "Name":"Invalid", "Value":100001.0}
        ]));
        assert_eq!(fans.len(), 1);
        assert_eq!(fans[0].name, "CPU Fan");
        assert_eq!(fans[0].rpm, 1450.0);
    }
    #[test]
    fn windows_gpu_inventory_omits_unnamed_adapters() {
        let snapshot = parse_windows_hardware(&serde_json::json!({"gpu": [{"Name":"  "}, {"Name":"Intel Graphics"}]}));
        assert_eq!(snapshot.gpus.len(), 1);
        assert_eq!(snapshot.gpus[0].name, "Intel Graphics");
    }

    #[test]
    fn failed_inventory_does_not_erase_previous_devices() {
        let mut cached = parse_windows_hardware(&serde_json::json!({"gpu": [{"Name":"Existing GPU"}]}));
        update_windows_inventory(&mut cached, &serde_json::json!({"gpu_error":"unavailable"}));
        assert_eq!(cached.gpus[0].name, "Existing GPU");
        update_windows_inventory(&mut cached, &serde_json::json!({"gpu":[]}));
        assert!(cached.gpus.is_empty());
    }
    #[test]
    fn windows_disk_inventory_requires_identity_and_capacity() {
        let snapshot = parse_windows_hardware(&serde_json::json!({"physical_disks": [
            {"DeviceID":"\\\\.\\PHYSICALDRIVE0", "Model":"NVMe Drive", "InterfaceType":"SCSI", "Size":"1000", "Partitions":2},
            {"DeviceID":"", "Model":"No identity", "Size":"2000"},
            {"DeviceID":"\\\\.\\PHYSICALDRIVE2", "Size":"0"}
        ]}));
        assert_eq!(snapshot.physical_disks.len(), 1);
        assert_eq!(snapshot.physical_disks[0].name, "PHYSICALDRIVE0");
        assert_eq!(snapshot.physical_disks[0].total_bytes, 1000);
        assert_eq!(snapshot.physical_disks[0].partition_count, Some(2));
        assert_eq!(snapshot.physical_disks[0].is_ssd, None);
        let mut cached = snapshot.clone();
        update_windows_inventory(&mut cached, &serde_json::json!({"disk_inventory_error":"unavailable"}));
        assert_eq!(cached.physical_disks.len(), 1);
    }
    #[test]
    fn windows_disk_partition_count_preserves_missing_and_reported_zero() {
        let snapshot = parse_windows_hardware(&serde_json::json!({"physical_disks": [
            {"DeviceID":"\\\\.\\PHYSICALDRIVE0", "Size":"1000"},
            {"DeviceID":"\\\\.\\PHYSICALDRIVE1", "Size":"2000", "Partitions":0}
        ]}));
        assert_eq!(snapshot.physical_disks[0].partition_count, None);
        assert_eq!(snapshot.physical_disks[1].partition_count, Some(0));
    }
    #[test]
    fn failed_live_query_clears_measurements_but_keeps_device_inventory() {
        let mut snapshot = SystemSensorsSnapshot {
            cpu_temp: Some(65.0),
            cpu_power_watts: Some(12.0),
            power: PowerSupplyInfo { ac_online: Some(true), ..Default::default() },
            total_disk_rates_available: true,
            ..Default::default()
        };
        snapshot.gpus.push(GpuSensorInfo { name: "GPU".into(), load_pct: Some(40.0), temp: Some(70.0), ..Default::default() });
        snapshot.physical_disks.push(PhysicalDiskInfo { name: "Disk 0".into(), read_rate_bps: 10.0, disk_rates_available: true, ..Default::default() });
        snapshot.temp_sensors.push(TempSensor { name: "CPU Package".into(), temp: 65.0, ..Default::default() });

        clear_windows_live_hardware_readings(&mut snapshot);

        assert_eq!(snapshot.gpus[0].name, "GPU");
        assert_eq!(snapshot.physical_disks[0].name, "Disk 0");
        assert!(snapshot.cpu_temp.is_none() && snapshot.cpu_power_watts.is_none());
        assert!(snapshot.power.ac_online.is_none());
        assert!(snapshot.gpus[0].load_pct.is_none() && snapshot.gpus[0].temp.is_none());
        assert!(!snapshot.physical_disks[0].disk_rates_available);
        assert!(snapshot.temp_sensors.is_empty());
        assert!(!snapshot.total_disk_rates_available);
    }
    #[test]
    fn windows_ac_line_status_keeps_unknown_distinct_from_offline() {
        assert_eq!(windows_ac_line_status(0), Some(false));
        assert_eq!(windows_ac_line_status(1), Some(true));
        assert_eq!(windows_ac_line_status(255), None);
        assert_eq!(windows_ac_line_status(2), None);
    }
    #[test]
    fn windows_battery_status_preserves_ac_and_charge_states() {
        for code in 6..=9 { assert_eq!(windows_battery_status(Some(code)), "Charging"); }
        assert_eq!(windows_battery_status(Some(2)), "AC connected");
        assert_eq!(windows_battery_status(Some(4)), "Low");
        assert_eq!(windows_battery_status(Some(5)), "Critical");
        assert_eq!(windows_battery_status(Some(11)), "Partially charged");
        assert_eq!(windows_battery_status(Some(10)), "Unknown");
        assert_eq!(windows_battery_status(None), "Unknown");
    }

    #[test]
    fn cpu_package_readings_require_a_unique_cpu_and_matching_sensor() {
        let hardware = serde_json::json!([{"Identifier":"/cpu/0", "HardwareType":"Cpu"}]);
        let readings = serde_json::json!([
            {"Parent":"/cpu/0", "SensorType":"Power", "Name":"CPU Package", "Value":0},
            {"Parent":"/cpu/0", "SensorType":"Temperature", "Name":"CPU Package", "Value":65.5},
            {"Parent":"/cpu/1", "SensorType":"Power", "Name":"CPU Package", "Value":90}
        ]);
        assert_eq!(windows_cpu_package_metric(&hardware, &readings, "Power", 0.0, f32::MAX as f64), Some(0.0));
        assert_eq!(windows_cpu_package_metric(&hardware, &readings, "Temperature", -40.0, 150.0), Some(65.5));
        let invalid_temp = serde_json::json!([{"Parent":"/cpu/0", "SensorType":"Temperature", "Name":"CPU Package", "Value":190.0}]);
        assert_eq!(windows_cpu_package_metric(&hardware, &invalid_temp, "Temperature", -40.0, 150.0), None);
        let labeled_power = serde_json::json!([{"Parent":"/cpu/0", "SensorType":"Power", "Name":"CPU Package Power", "Value":8.25}]);
        assert_eq!(windows_cpu_package_metric(&hardware, &labeled_power, "Power", 0.0, f32::MAX as f64), Some(8.25));
        let high_power = serde_json::json!([{"Parent":"/cpu/0", "SensorType":"Power", "Name":"CPU Package", "Value":1250.0}]);
        assert_eq!(windows_cpu_package_metric(&hardware, &high_power, "Power", 0.0, f32::MAX as f64), Some(1250.0));
        let multi_cpu = serde_json::json!([hardware[0].clone(), {"Identifier":"/cpu/1", "HardwareType":"Cpu"}]);
        assert_eq!(windows_cpu_package_metric(&multi_cpu, &readings, "Power", 0.0, f32::MAX as f64), None);
        let duplicates = serde_json::json!([readings[0].clone(), readings[0].clone()]);
        assert_eq!(windows_cpu_package_metric(&hardware, &duplicates, "Power", 0.0, f32::MAX as f64), None);
        assert_eq!(windows_cpu_package_metric(&hardware, &serde_json::Value::Null, "Power", 0.0, f32::MAX as f64), None);
    }
    #[test]
    fn gpu_samples_require_unique_device_and_matching_sensor_parent() {
        let mut gpus = vec![GpuSensorInfo { name: "Example GPU".into(), ..Default::default() }];
        let hardware = serde_json::json!([{ "Name":"Example GPU", "Identifier":"/gpu/0", "HardwareType":"GpuIntel" }]);
        let sensors = serde_json::json!([
            {"Parent":"/gpu/0", "SensorType":"Load", "Name":"GPU Core", "Value":0.0},
            {"Parent":"/gpu/0", "SensorType":"Temperature", "Name":"GPU Core", "Value":60.5},
            {"Parent":"/gpu/0", "SensorType":"Clock", "Name":"GPU Core", "Value":500},
            {"Parent":"/gpu/1", "SensorType":"Power", "Name":"GPU Package", "Value":4.0}
        ]);
        apply_windows_gpu_sensors(&mut gpus, &hardware, &sensors);
        assert_eq!(gpus[0].load_pct, Some(0.0));
        assert_eq!(gpus[0].temp, Some(60.5));
        assert_eq!(gpus[0].clock_mhz, Some(500));
        assert_eq!(gpus[0].power_watts, None);
        apply_windows_gpu_sensors(&mut gpus, &serde_json::Value::Null, &sensors);
        assert!(gpus[0].load_pct.is_none() && gpus[0].temp.is_none() && gpus[0].clock_mhz.is_none());
        let mut duplicates = vec![GpuSensorInfo { name: "Example GPU".into(), ..Default::default() }; 2];
        apply_windows_gpu_sensors(&mut duplicates, &hardware, &sensors);
        assert!(duplicates.iter().all(|gpu| gpu.load_pct.is_none()));
        let mut gpu = vec![GpuSensorInfo { name: "Example GPU".into(), ..Default::default() }];
        let duplicate_hardware = serde_json::json!([hardware[0].clone(), hardware[0].clone()]);
        apply_windows_gpu_sensors(&mut gpu, &duplicate_hardware, &sensors);
        assert!(gpu[0].load_pct.is_none());
    }

    #[test]
    fn windows_gpu_engine_counters_require_one_adapter_and_valid_readings() {
        let counters = serde_json::json!([
            {"Name":"pid_1_luid_0x1_phys_0_eng_0_engtype_3D", "UtilizationPercentage":12.5},
            {"Name":"pid_2_luid_0x1_phys_0_eng_0_engtype_3D", "UtilizationPercentage":18.0},
            {"Name":"pid_3_luid_0x1_phys_0_eng_1_engtype_3D", "UtilizationPercentage":25.0},
            {"Name":"pid_4_luid_0x1_phys_0_eng_2_engtype_Copy", "UtilizationPercentage":80.0},
            {"Name":"pid_5_luid_0x1_phys_0_eng_2_engtype_Copy", "UtilizationPercentage":55.0},
            {"Name":"pid_1_luid_0x1_phys_0_eng_1_engtype_Copy", "UtilizationPercentage":101.0},
            {"Name":"bad-instance", "UtilizationPercentage":80.0}
        ]);
        let mut gpus = vec![GpuSensorInfo { name: "GPU".into(), load_pct: Some(90.0), engine_loads: vec![("stale".into(), 1.0)], ..Default::default() }];
        apply_windows_gpu_engine_counters(&mut gpus, &counters);
        assert_eq!(gpus[0].engine_loads, vec![("3D busiest engine".into(), 30.5), ("Copy busiest engine".into(), 100.0)]);
        assert_eq!(gpus[0].load_pct, Some(90.0));

        gpus.push(GpuSensorInfo { name: "Other GPU".into(), ..Default::default() });
        apply_windows_gpu_engine_counters(&mut gpus, &counters);
        assert!(gpus.iter().all(|gpu| gpu.engine_loads.is_empty()));
    }

    #[test]
    fn gpu_sensor_matching_ignores_windows_trademark_formatting() {
        let mut gpus = vec![GpuSensorInfo { name: "Intel(R) UHD Graphics 605".into(), ..Default::default() }];
        let hardware = serde_json::json!([{"Name":"Intel UHD Graphics 605", "Identifier":"/gpu/0", "HardwareType":"GpuIntel"}]);
        let sensors = serde_json::json!([{"Parent":"/gpu/0", "SensorType":"Load", "Name":"GPU Core", "Value":12.5}]);

        apply_windows_gpu_sensors(&mut gpus, &hardware, &sensors);

        assert_eq!(gpus[0].load_pct, Some(12.5));
        assert_eq!(gpus[0].device_path, "/gpu/0");
    }

    #[test]
    fn windows_gpu_temperature_accepts_common_provider_label_with_parent_match() {
        let mut gpus = vec![GpuSensorInfo { name: "Example GPU".into(), ..Default::default() }];
        let hardware = serde_json::json!([{"Name":"Example GPU", "Identifier":"/gpu/0", "HardwareType":"GpuIntel"}]);
        let sensors = serde_json::json!([
            {"Parent":"/gpu/0", "SensorType":"Temperature", "Name":"GPU Temperature", "Value":57.0},
            {"Parent":"/gpu/1", "SensorType":"Temperature", "Name":"GPU Temperature", "Value":99.0}
        ]);

        apply_windows_gpu_sensors(&mut gpus, &hardware, &sensors);

        assert_eq!(gpus[0].temp, Some(57.0));
    }

    #[test]
    fn windows_ram_temperature_uses_sensor_identity() {
        let snapshot = parse_windows_hardware(&serde_json::json!({"temp": [
            {"Name":"Temperature", "Identifier":"/ram/dimm/0", "Value":43.0}
        ]}));

        assert_eq!(snapshot.ram_temp, Some(43.0));
        assert_eq!(snapshot.temp_sensors[0].category, SensorCategory::Memory);
    }

    #[test]
    fn windows_battery_capacity_uses_reported_wmi_design_and_full_charge() {
        let snapshot = parse_windows_hardware(&serde_json::json!({"battery": [
            {"DeviceID":"ACPI\\PNP0C0A\\0", "Name":"Battery", "EstimatedChargeRemaining":50, "BatteryStatus":2, "DesignVoltage":12000,
             "DesignCapacity":50000, "FullChargeCapacity":40000}
        ]}));
        let battery = &snapshot.power.batteries[0];
        assert_eq!(battery.device_id, "ACPI\\PNP0C0A\\0");
        assert_eq!(battery.voltage_volts, Some(12.0));

        assert_eq!(battery.energy_design_wh, Some(50.0));
        assert_eq!(battery.energy_full_wh, Some(40.0));
        assert_eq!(battery.energy_wh, Some(20.0));
        assert_eq!(battery.health_pct, Some(80.0));
        assert!(battery.energy_estimated);
    }

    #[test]
    fn disk_rates_distinguish_idle_from_missing_or_invalid_data() {
        let idle = parse_windows_hardware(&serde_json::json!({"disk": {"DiskReadBytesPerSec": 0, "DiskWriteBytesPerSec": 0}}));
        assert!(idle.total_disk_rates_available);
        assert_eq!(idle.total_disk_read_rate_bps, 0.0);
        let busy = parse_windows_hardware(&serde_json::json!({"disk": {"DiskReadBytesPerSec": 512, "DiskWriteBytesPerSec": 1024}}));
        assert!(busy.total_disk_rates_available);
        assert_eq!(busy.total_disk_write_rate_bps, 1024.0);
        for disk in [serde_json::json!(null), serde_json::json!({"DiskReadBytesPerSec": 1}), serde_json::json!({"DiskReadBytesPerSec": -1, "DiskWriteBytesPerSec": 1})] {
            assert!(!parse_windows_hardware(&serde_json::json!({"disk": disk})).total_disk_rates_available);
        }
    }
    #[test]
    fn wireless_classification_uses_the_reported_type_and_unique_alias() {
        let adapters = serde_json::json!([
            {"Name":"Renamed adapter", "ifType":71},
            {"Name":"Wi-Fi cable", "ifType":6},
            {"Name":"Duplicate", "ifType":71},
            {"Name":"duplicate", "ifType":6}
        ]);
        assert_eq!(windows_is_wireless_interface("renamed adapter", &adapters), Some(true));
        assert_eq!(windows_is_wireless_interface("Wi-Fi cable", &adapters), Some(false));
        assert_eq!(windows_is_wireless_interface("Duplicate", &adapters), None);
        assert_eq!(windows_is_wireless_interface("Wi-Fi", &serde_json::Value::Null), None);
        assert_eq!(windows_is_wireless_interface("Missing type", &serde_json::json!([{"Name":"Missing type"}])), None);
    }
    #[test]
    fn logical_disk_rates_match_unique_drive_letters_only() {
        let mut disks = vec![
            DiskInfo { mount_point: "C:\\".into(), ..Default::default() },
            DiskInfo { mount_point: "d:/".into(), ..Default::default() },
            DiskInfo { mount_point: "C:\\data".into(), ..Default::default() },
        ];
        let samples = serde_json::json!([
            {"Name":"c:", "DiskReadBytesPerSec":0, "DiskWriteBytesPerSec":9},
            {"Name":"D:", "DiskReadBytesPerSec":5, "DiskWriteBytesPerSec":6},
            {"Name":"d:", "DiskReadBytesPerSec":5, "DiskWriteBytesPerSec":6},
            {"Name":"_Total", "DiskReadBytesPerSec":99, "DiskWriteBytesPerSec":99}
        ]);
        apply_windows_volume_rates(&mut disks, &samples);
        assert!(disks[0].disk_rates_available);
        assert_eq!((disks[0].read_rate_bps, disks[0].write_rate_bps), (0.0, 9.0));
        assert!(!disks[1].disk_rates_available && !disks[2].disk_rates_available);
        apply_windows_volume_rates(&mut disks, &serde_json::Value::Null);
        assert!(!disks[0].disk_rates_available);
    }
    #[test]
    fn memory_inventory_omits_missing_module_fields() {
        let snap = parse_windows_hardware(&serde_json::json!({"ram": [
            {"Manufacturer": "Example", "Speed": 9999, "ConfiguredClockSpeed": 3200, "SMBIOSMemoryType": 26, "FormFactor": 12, "DeviceLocator": "DIMM 0", "Capacity": 8589934592_u64},
            {"Manufacturer": "", "ConfiguredClockSpeed": null, "SMBIOSMemoryType": 0, "FormFactor": 0, "Capacity": 0}
        ]}));
        let memory = snap.memory_hw.unwrap();
        assert_eq!(memory.slot_summary, "2 populated modules");
        assert_eq!(memory.memory_type, "DDR4");
        assert_eq!(memory.form_factor, "SODIMM");
        assert_eq!(windows_memory_type(34), Some("DDR5"));
        assert_eq!(windows_memory_type(255), None);
        assert!(memory.module_spec.contains("Example · DDR4 · 3200 MHz · 8.0 GB · SODIMM · DIMM 0"));
        assert!(memory.module_spec.contains("Module details unavailable"));
        assert!(!memory.module_spec.contains("9999 MHz") && !memory.module_spec.contains("null MHz") && !memory.module_spec.contains("0 B"));
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
        assert_eq!(snap.cpu_temp, None);
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
for path in glob.glob('/sys/class/powercap/*'):
    try:
        name = open(path + '/name').read().strip().lower()
    except OSError:
        continue
    if not (name.startswith('package') or name.startswith('cpu package')):
        continue
    energy = read_number(path + '/energy_uj')
    maximum = read_number(path + '/max_energy_range_uj')
    if energy is not None:
        zones.append((path, energy, maximum, time.monotonic()))
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
        if not isinstance(samples, list):
            raise ValueError('unexpected GPU sample format')
        sample = next((sample for sample in reversed(samples) if isinstance(sample, dict) and isinstance(sample.get('engines'), dict)), None)
        if sample is None:
            raise ValueError('GPU engine readings unavailable')
        engines = {name: values['busy'] for name, values in sample['engines'].items() if isinstance(values, dict) and isinstance(values.get('busy'), (float, int)) and math.isfinite(values['busy']) and 0 <= values['busy'] <= 100}
        power = sample.get('power')
        gpu_power = power.get('GPU') if isinstance(power, dict) else None
        if not isinstance(gpu_power, (float, int)) or not math.isfinite(gpu_power) or gpu_power < 0:
            gpu_power = None
        if not engines and gpu_power is None:
            raise ValueError('GPU metrics unavailable')
        result['gpus'].append({'device': device, 'engines': engines, 'watts': gpu_power})
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        result['notes'].append(node + ': ' + str(error))

if time.monotonic() - started < 0.15:
    time.sleep(0.15)
watts = []
for path, previous, maximum, sampled_at in zones:
    current = read_number(path + '/energy_uj')
    elapsed = time.monotonic() - sampled_at
    if current is None or previous < 0 or current < 0 or elapsed <= 0:
        continue
    delta = current - previous
    if delta < 0 and maximum is not None and maximum > 0 and previous <= maximum and current <= maximum:
        delta += maximum
    if delta >= 0:
        watts.append(delta / 1e6 / elapsed)
if zones and not unreadable_zones and len(watts) == len(zones):
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

    pub fn refresh_protected_hardware(&mut self, snapshot: &mut SystemSensorsSnapshot) {
        if self.protected_data.is_none() { return; }
        if self.last_protected_poll.elapsed() >= Duration::from_secs(2) {
            self.last_protected_poll = Instant::now();
            match read_protected_hardware(None, false) {
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
