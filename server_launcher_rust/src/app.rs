use crate::theme::CompactTooltip;
mod workspace;
mod servers;
mod listeners;
mod server_detail;
mod scripts;
mod commands;
mod systemd;
mod settings;
mod ssh;
mod sensors;

use crate::config::{ConfigFile, CustomScript, SavedCommand, ServerConfig, SshRemoteHost};
use crate::modals::{
    render_add_edit_modal, render_command_modal, render_command_palette,
    render_close_confirmation_modal, render_new_service_modal, render_script_modal,
    render_ssh_host_modal, render_remote_start_server_modal, render_remote_reboot_modal, render_remote_shutdown_modal, render_sudo_modal, AddEditModalState, CloseConfirmChoice,
    CommandPaletteState, ModalAction, NewServiceModalState, PaletteAction,
    RemoteRebootModalState, RemoteShutdownModalState, RemoteStartServerModalState, SshHostModalState, SudoModalState,
};
use crate::scripts::{
    check_local_sudo_needs_password, execute_local_script, execute_remote_script,
    script_requires_sudo, ScriptExecution, ScriptModalState, ScriptRunStatus,
};
use crate::commands::{CommandModalState, TerminalCommandStatus, TerminalSession};
use crate::systemd::{
    control_local_service, control_remote_service, create_local_service, create_remote_service,
    delete_local_drop_in, fetch_local_journal, fetch_remote_journal, list_local_services,
    list_remote_services, write_local_unit_file, SystemdService,
};
use crate::remote::{
    build_remote_restart_cmd, build_remote_start_cmd,
    build_ssh_command,
    fetch_remote_listeners, guess_remote_start_cmd,
    import_from_ssh_config, launch_external_ssh_terminal, test_ssh_connection,
    RemoteListener, RemoteStoppedServer,
};
use crate::scanner::{get_lan_ip, SystemPortScanner};
use crate::service::{LogKind, Service, ServiceState};
use crate::theme::ThemeMode;
use eframe::egui;
use egui::{
    Color32, Context, Frame, Key, Modifiers, RichText, Rounding, ScrollArea, Stroke, Ui,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::System;

const MACHINE_RAIL_WIDTH: f32 = 52.0;
const WORKSPACE_SIDEBAR_WIDTH: f32 = 380.0;
const SERVERS_SIDEBAR_WIDTH: f32 = 340.0;
const SENSORS_SIDEBAR_WIDTH: f32 = 240.0;
const WORKSPACE_GAP: f32 = 8.0;
const WORKSPACE_HEADER_HEIGHT: f32 = 38.0;
const WORKSPACE_SIDEBAR_MARGIN: egui::Margin = egui::Margin {
    left: 14.0, right: 12.0, top: 0.0, bottom: 0.0,
};

#[derive(Clone, Debug)]
pub enum SlashAction {
    Start,
    Stop,
    Restart,
    Clear,
    Status,
    Edit,
    Help,
    Console,
    SendStdin(String),
    RunAction(String),
    OpenLink(String),
}

#[derive(Clone, Debug)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    pub category: &'static str,
    pub action: SlashAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Servers,
    Scripts,
    Commands,
    SystemMD,
    Sensors,
    Files,
    Apps,
}

impl SidebarTab {
    pub fn label(&self) -> &'static str {
        match self {
            SidebarTab::Servers => "\u{e153} Servers",
            SidebarTab::Scripts => "\u{e2ed} Scripts",
            SidebarTab::Commands => "\u{e181} Cmds",
            SidebarTab::SystemMD => "\u{e245} SystemD",
            SidebarTab::Files => "Files",
            SidebarTab::Apps => "Apps",
            SidebarTab::Sensors => "\u{e186} Sensors",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemdTab {
    Status,
    Files,
    Dependencies,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorsSubTab {
    Overview,
    TopApps,
    Cpu,
    Ram,
    Gpu,
    Storage,
    Wifi,
    DataUsage,
    Power,
    Bluetooth,
    Temperature,
}

/// A service operation retained for an authenticated retry on its original host.
#[derive(Clone, Debug)]
pub enum PendingSudoAction {
    UnlockSensorAccess,
    SetChargeLimit { battery: String, limit: u32 },
    ServiceAction { unit: String, action: &'static str, user_mode: bool },
    CreateService { def: crate::systemd::NewServiceDefinition },
    SaveUnitFile { path: String, content: String, user_mode: bool },
    CreateDropIn { path: String, content: String, user_mode: bool },
    DeleteDropIn { path: String, user_mode: bool },
    ReadUnitFile { path: String, user_mode: bool },
    ReadJournal { unit: String, user_mode: bool },
    QueryEnabled { unit: String, user_mode: bool },
    InspectUnit { unit: String, user_mode: bool },
}

impl PendingSudoAction {
    // A file may already be written when reload asks for sudo. Resume without recreating it.
    fn authenticated_retry(self, error: &str) -> Self {
        match &self {
            Self::CreateService { def } if error.starts_with("Unit file created,") || error.starts_with("Unit file saved,") || error.starts_with("File saved,") => {
                if let Ok(unit) = crate::systemd::service_unit_name(def) {
                    return Self::ServiceAction { unit, action: "finish-create", user_mode: def.is_user };
                }
            }
            Self::CreateDropIn { path, content, user_mode } if error.starts_with("Drop-in created,") || error.starts_with("File saved,") => {
                return Self::SaveUnitFile { path: path.clone(), content: content.clone(), user_mode: *user_mode };
            }
            _ => {}
        }
        self
    }

    fn key(&self, host_id: Option<&str>) -> String {
        let (kind, target, user_mode) = match self {
            Self::UnlockSensorAccess => ("hardware", "local", false),
            Self::SetChargeLimit { battery, .. } => ("charge-limit", battery.as_str(), false),
            Self::ServiceAction { unit, user_mode, .. } => ("control", unit.as_str(), *user_mode),
            Self::CreateService { def } => ("create", def.name.as_str(), def.is_user),
            Self::SaveUnitFile { path, user_mode, .. } => ("save", path.as_str(), *user_mode),
            Self::CreateDropIn { path, user_mode, .. } => ("create-dropin", path.as_str(), *user_mode),
            Self::DeleteDropIn { path, user_mode } => ("delete", path.as_str(), *user_mode),
            Self::ReadUnitFile { path, user_mode } => ("read", path.as_str(), *user_mode),
            Self::ReadJournal { unit, user_mode } => ("journal", unit.as_str(), *user_mode),
            Self::QueryEnabled { unit, user_mode } => ("enabled", unit.as_str(), *user_mode),
            Self::InspectUnit { unit, user_mode } => ("inspect", unit.as_str(), *user_mode),
        };
        format!("{}:{}:{}:{}", host_id.unwrap_or("local"), kind, target, user_mode)
    }
}

enum SystemdActionValue {
    Message(String),
    FileContent(String),
    Journal(String),
    Enabled(bool),
    Status(SystemdService),
}

struct SystemdActionResult {
    key: String,
    action: Option<PendingSudoAction>,
    host_id: Option<String>,
    host_lifetime: Option<Arc<()>>,
    from_popup: bool,
    result: Result<SystemdActionValue, String>,
}

fn remote_host_lifetime_matches(active: &HashMap<String, Arc<()>>, id: &str, lifetime: &Arc<()>) -> bool {
    active.get(id).is_some_and(|current| Arc::ptr_eq(current, lifetime))
}

fn remote_host_cache_key_matches(id: &str, key: &str) -> bool {
    key.split_once(':').is_some_and(|(key_id, _)| key_id == id)
}

#[cfg(test)]
mod remote_host_lifetime_tests {
    use super::*;

    #[test]
    fn late_completion_cannot_publish_after_host_id_reuse() {
        let mut active = HashMap::new();
        let old = Arc::new(());
        active.insert("host".to_owned(), old.clone());
        assert!(remote_host_lifetime_matches(&active, "host", &old));
        active.remove("host");
        let replacement = Arc::new(());
        active.insert("host".to_owned(), replacement.clone());
        assert!(!remote_host_lifetime_matches(&active, "host", &old));
        assert!(remote_host_lifetime_matches(&active, "host", &replacement));
        assert!(remote_host_cache_key_matches("host", "host:unit:system"));
        assert!(!remote_host_cache_key_matches("host", "host2:unit:system"));
    }
}

/// State for the sudo-password popup dialog.
#[derive(Clone, Debug)]
pub struct SudoPopupState {
    pub open: bool,
    pub pending: Option<PendingSudoAction>,
    pub host_id: Option<String>,
    pub password_input: String,
    pub remember_password: bool,
    pub error: Option<String>,
    pub just_opened: bool,
}

impl Default for SudoPopupState {
    fn default() -> Self {
        Self {
            open: false,
            pending: None,
            host_id: None,
            password_input: String::new(),
            remember_password: true,
            error: None,
            just_opened: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProcessGroupSummary {
    pub key: String,
    pub display_name: String,
    pub sum_cpu: f32,
    pub cpu_usage_available: bool,
    pub sum_mem: u64,
    pub sum_disk_rate: f64,
    pub disk_rate_available: bool,
    pub sum_net_rate: f64,
    pub net_rate_available: bool,
    pub sum_net_bytes: u64,
    pub sum_net_bytes_available: bool,
    pub sum_gpu: f32,
    pub sum_power_watts: f32,
    pub is_app: bool,
    pub pids: Vec<u32>,
    pub children: Vec<crate::sensors::ProcessSensorInfo>,
}

fn filtered_processes<'a>(apps: &'a [crate::sensors::ProcessSensorInfo], query: &str) -> Vec<&'a crate::sensors::ProcessSensorInfo> {
    apps.iter().filter(|app| query.is_empty() || app.name.to_lowercase().contains(query)
        || app.pid.to_string().contains(query) || app.status.to_lowercase().contains(query)).collect()
}

fn selected_process_present(apps: &[crate::sensors::ProcessSensorInfo], pid: u32, start: Option<u64>) -> bool {
    start.is_some() && apps.iter().any(|process| process.pid == pid && process.start_marker == start)
}

#[cfg(test)]
mod process_filter_tests {
    #[test]
    fn selection_requires_same_process_start() {
        let apps = [crate::sensors::ProcessSensorInfo { pid: 42, start_marker: Some(100), ..Default::default() }];
        assert!(super::selected_process_present(&apps, 42, Some(100)));
        assert!(!super::selected_process_present(&apps, 42, Some(99)));
        assert!(!super::selected_process_present(&apps, 42, None));
    }

    #[test]
    fn search_includes_processes_beyond_the_old_limit() {
        let apps: Vec<_> = (1..=300).map(|pid| crate::sensors::ProcessSensorInfo { pid, name: format!("process-{pid}"), ..Default::default() }).collect();
        let found = super::filtered_processes(&apps, "process-300");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].pid, 300);
        assert_eq!(super::filtered_processes(&apps, "").len(), 300);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Appearance,
    Autostart,
    Backup,
    Shortcuts,
    About,
}

impl SettingsTab {
    pub fn all() -> &'static [SettingsTab] {
        &[
            SettingsTab::Appearance,
            SettingsTab::Autostart,
            SettingsTab::Backup,
            SettingsTab::Shortcuts,
            SettingsTab::About,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            SettingsTab::Appearance => "Appearance",
            SettingsTab::Autostart => "Startup",
            SettingsTab::Backup => "Data & Backup",
            SettingsTab::Shortcuts => "Shortcuts",
            SettingsTab::About => "About",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            SettingsTab::Appearance => crate::lucide::PALETTE,
            SettingsTab::Autostart => crate::lucide::ROCKET,
            SettingsTab::Backup => crate::lucide::PACKAGE,
            SettingsTab::Shortcuts => crate::lucide::KEYBOARD,
            SettingsTab::About => crate::lucide::INFO,
        }
    }
}

const REMOTE_LISTENER_REFRESH: &str = "remote_listener_action_refresh";

#[derive(Clone, Default)]
struct RemoteListenerSnapshot {
    rows: Arc<Vec<RemoteListener>>,
    error: Option<Arc<str>>,
}

#[derive(Default)]
struct RemoteCommandActivity {
    count: usize,
    cancel_generation: u64,
    inactive_since: Option<Instant>,
    timeout: Option<std::time::Duration>,
}

#[allow(dead_code)]
pub struct LauncherApp {
    pub active_sidebar_tab: SidebarTab,
    sidebar_tab_machine: String,
    machine_sidebar_tabs: HashMap<String, SidebarTab>,
    pub scripts: Vec<CustomScript>,
    pub saved_commands: Vec<SavedCommand>,
    command_catalog_view: Option<commands::CommandCatalogView>,
    config_path: PathBuf,
    services: Vec<Service>,
    active_tab: String, // "overview" or service.key
    server_tab_order: Vec<String>,
    active_group_filter: String, // "All" or group name
    lan_ip: String,

    // System monitor and network listener scanner
    sys: System,
    scanner: SystemPortScanner,
    last_poll: Instant,

    // Modals
    add_edit_modal: AddEditModalState,
    palette_modal: CommandPaletteState,

    // Log search state per service key
    search_queries: HashMap<String, String>,
    search_open: HashMap<String, bool>,
    filter_lines: HashMap<String, bool>,

    // Command input state per service key
    input_texts: HashMap<String, String>,
    #[allow(dead_code)]
    input_histories: HashMap<String, Vec<String>>,
    #[allow(dead_code)]
    history_indices: HashMap<String, usize>,

    // Saved stopped external listeners (port -> (name, command, cwd))
    stopped_external: HashMap<u16, (String, String, String)>,

    // Search query specifically for the Server Overview tab
    overview_search_query: String,
    focus_overview_search: bool,

    // Autocomplete navigation state
    autocomplete_selected: usize,

    // Expanded view state for external listening processes
    expanded_listeners: BTreeSet<u16>,

    // Only cards explicitly expanded by the user are kept open.
    pub expanded_server_cards: BTreeSet<String>,

    // App header icon texture
    #[allow(dead_code)]
    icon_texture: Option<egui::TextureHandle>,

    // External listeners resource monitoring toggle (CPU & RAM)
    show_other_process_metrics: bool,
    other_metrics: HashMap<u32, (f32, f32)>,
    last_other_poll: Instant,

    // Inline terminal state for PC listening processes
    open_inline_terminals: BTreeSet<u16>,
    inline_terminal_sessions: HashMap<u16, ExternalTerminalSession>,
    inline_terminal_inputs: HashMap<u16, String>,

    // Navigation & Remote SSH machines
    selected_machine: String, // "local" or host.id
    ssh_hosts: Vec<SshRemoteHost>,
    remote_host_lifetimes: Arc<Mutex<HashMap<String, Arc<()>>>>,
    saved_ssh_refs: BTreeSet<String>,
    ssh_host_modal: SshHostModalState,
    remote_listeners: Arc<Mutex<HashMap<String, RemoteListenerSnapshot>>>,
    remote_scanning: Arc<Mutex<HashMap<String, Option<(Instant, u32)>>>>,
    remote_terminal_logs: Arc<Mutex<HashMap<String, Arc<Mutex<VecDeque<crate::service::LogEntry>>>>>>,
    remote_command_activity: Arc<Mutex<HashMap<String, RemoteCommandActivity>>>,
    ssh_idle: crate::config::SshIdleSettings,
    ssh_probe_timeouts: crate::config::SshProbeTimeouts,
    remote_terminal_inputs: HashMap<String, String>,
    remote_connection_status: Arc<Mutex<HashMap<String, Result<String, String>>>>,
    remote_connecting: Arc<Mutex<BTreeSet<String>>>,
    pub remote_stopped_servers: Arc<Mutex<HashMap<String, Vec<RemoteStoppedServer>>>>,
    pub remote_start_server_modal: RemoteStartServerModalState,
    pub remote_reboot_modal: RemoteRebootModalState,
    pub remote_shutdown_modal: RemoteShutdownModalState,
    pub theme: ThemeMode,
    pub custom_accent_color: Option<String>,
    pub settings_active_tab: SettingsTab,
    pub settings_custom_hex_input: String,
    pub last_applied_palette: Option<crate::theme::ThemePalette>,
    pub app_autostart: bool,
    pub auto_scroll: HashMap<String, bool>,
    proc_info_cache: HashMap<u32, (Option<String>, Option<String>, Instant)>,
    remote_search_query: HashMap<String, String>,
    // Close confirmation dialog state
    close_confirm_open: bool,
    close_confirmed: bool,
    close_waiting_for_ssh_commands: bool,

    // ADB / Android Device Cockpit State
    pub adb_devices: Vec<crate::config::AdbDeviceConfig>,
    pub adb_tauri_opened: HashSet<String>,
    pub add_device_selector_modal_open: bool,
    pub adb_add_modal_open: bool,
    pub adb_detected_devices: Vec<crate::adb::AdbDeviceInfo>,
    pub adb_wireless_ip_input: String,
    pub adb_wireless_port_input: String,
    pub adb_device_name_input: String,
    pub adb_active_subtab: AdbSubTab,
    pub adb_telemetry_cache: HashMap<String, crate::adb::AdbDeviceTelemetry>,
    pub adb_telemetry_pending: HashSet<String>,
    pub adb_telemetry_results: Arc<Mutex<HashMap<String, crate::adb::AdbDeviceTelemetry>>>,
    pub adb_data_serial: String,
    pub adb_queries_loaded: HashSet<String>,
    pub adb_status_msg: Option<(String, bool, Instant)>,
    pub adb_input_text_field: String,
    pub adb_clipboard_cache: HashMap<String, String>,
    pub adb_clipboard_send_text: String,
    pub adb_shell_cmd_input: String,
    pub adb_shell_history: Vec<(String, String, bool)>,
    pub adb_file_current_path: String,
    pub adb_file_entries: Vec<crate::adb::AdbFileEntry>,
    pub adb_fastboot_partition: String,
    pub adb_package_filter: String,
    pub adb_package_is_system: bool,
    pub adb_packages_cache: Vec<crate::adb::AdbPackageInfo>,
    pub adb_scrcpy_max_fps: u32,
    pub adb_scrcpy_bitrate_mbps: u32,
    pub adb_scrcpy_stay_awake: bool,
    pub adb_scrcpy_screen_off: bool,
    pub adb_scrcpy_no_audio: bool,
    pub adb_scrcpy_pids: HashMap<String, u32>,
    pub adb_mirror_mode: AdbScreenMirrorMode,

    // Built-in Screen Mirroring State (Native in-app screen share)
    pub adb_mirror_active: bool,
    pub adb_mirror_texture: Option<egui::TextureHandle>,
    pub adb_mirror_frame_rx: Option<std::sync::mpsc::Receiver<(Vec<u8>, usize, usize)>>,
    pub adb_mirror_stop_tx: Option<std::sync::mpsc::Sender<()>>,
    pub adb_mirror_last_frame_time: Option<Instant>,
    pub adb_mirror_fps: f32,
    pub adb_mirror_device_res: (usize, usize),
    pub adb_mirror_drag_start: Option<egui::Pos2>,
    pub adb_screenshare_text_input: String,
    pub adb_file_filter: String,

    // Phone Link Suite State
    pub adb_notifications: Vec<crate::adb::AdbNotification>,
    pub adb_call_logs: Vec<crate::adb::AdbCallLogEntry>,
    pub adb_dialer_input: String,
    pub adb_sms_messages: Vec<crate::adb::AdbSmsMessage>,
    pub adb_sms_selected_contact: Option<String>,
    pub adb_sms_input: String,
    pub adb_photos: Vec<crate::adb::AdbPhotoItem>,
    pub adb_selected_photo: Option<crate::adb::AdbPhotoItem>,
    pub adb_photo_preview: Option<(String, Result<egui::TextureHandle, String>)>,
    pub adb_dnd_enabled: bool,
    pub adb_audio_muted: bool,
    pub adb_media_playing: bool,
    pub adb_notifications_filter: String,
    pub adb_call_search: String,
    pub adb_sms_search: String,
    pub adb_quick_call_number: String,

    // Scripts State
    pub script_executions: HashMap<String, ScriptExecution>,
    pub script_modal: ScriptModalState,
    pub sudo_modal: SudoModalState,
    pub selected_script_id: Option<String>,
    pub script_search_query: String,

    // Commands State
    pub command_modal: CommandModalState,
    pub local_terminal: TerminalSession,
    pub remote_terminals: HashMap<String, TerminalSession>,
    pub saved_command_search: String,
    pub saved_command_category: String,
    pub saved_command_status_msg: Option<(String, bool, Instant)>,

    // SystemMD State
    pub systemd_modal: NewServiceModalState,
    pub local_services: Vec<SystemdService>,
    pub local_services_user: bool,
    pub local_services_loaded: bool,
    pub local_services_cache: Arc<Mutex<HashMap<bool, Vec<SystemdService>>>>,
    pub local_services_in_flight: Arc<Mutex<HashSet<bool>>>,
    pub remote_services: Arc<Mutex<HashMap<String, Vec<SystemdService>>>>,
    pub remote_services_in_flight: Arc<Mutex<HashSet<String>>>,
    pub remote_services_errors: Arc<Mutex<HashMap<String, String>>>,
    pub remote_services_user: HashMap<String, bool>,
    pub selected_service: Option<String>,
    pub local_journal_logs: Arc<Mutex<HashMap<String, String>>>,
    pub remote_journal_logs: Arc<Mutex<HashMap<String, String>>>,
    pub systemd_actions_in_flight: Arc<Mutex<HashSet<String>>>,
    systemd_action_results: Arc<Mutex<VecDeque<SystemdActionResult>>>,
    remote_sudo_passwords: HashMap<String, String>,
    service_details_cache: HashMap<String, (SystemdService, Instant)>,
    service_query_errors: HashMap<String, Instant>,
    pub remote_service_action_msg: Arc<Mutex<Option<String>>>,
    pub service_search_query: String,
    pub service_action_msg: Option<String>,
    pub service_enabled_cache: HashMap<String, (bool, Instant)>,
    pub journal_refresh_interval: u32,
    pub last_journal_refresh: Instant,

    // SystemMD Tabs & Files / Dependencies state:
    pub systemd_tab: SystemdTab,
    pub systemd_files_cache: Arc<Mutex<HashMap<String, (crate::systemd::UnitFilesInfo, Instant)>>>,
    pub systemd_files_in_flight: Arc<Mutex<HashSet<String>>>,
    pub systemd_file_content_cache: Arc<Mutex<HashMap<String, Result<String, String>>>>,
    pub systemd_selected_file: Option<String>,
    pub systemd_file_content: String,
    pub systemd_file_original: String,
    systemd_editor_context: Option<(String, bool)>,
    pub systemd_file_status_msg: Option<(String, bool)>,
    pub systemd_new_dropin_open: bool,
    pub systemd_new_dropin_name: String,
    pub systemd_new_dropin_template: usize,
    systemd_new_dropin_content: String,
    pub systemd_deps_cache: Arc<Mutex<HashMap<String, (crate::systemd::UnitDependenciesInfo, Instant)>>>,
    pub systemd_deps_in_flight: Arc<Mutex<HashSet<String>>>,
    pub systemd_deps_history: Vec<String>,
    pub systemd_deps_filter: String,
    pub systemd_deps_view_mode: usize,

    // Sensors State (Asynchronous background polling for zero UI lag)
    pub sensors_collector: Arc<Mutex<crate::sensors::SystemSensorsCollector>>,
    pub sensors_in_flight: Arc<std::sync::atomic::AtomicBool>,
    pub sensors_shared_snapshot: Arc<Mutex<Arc<crate::sensors::SystemSensorsSnapshot>>>,
    pub sensors_snapshot: Arc<crate::sensors::SystemSensorsSnapshot>,
    pub sensors_refresh_interval: u32,
    pub last_sensors_refresh: Instant,
    pub remote_sensors_snapshots: Arc<Mutex<HashMap<String, Arc<crate::sensors::SystemSensorsSnapshot>>>>,
    pub remote_sensors_in_flight: Arc<Mutex<HashSet<String>>>,
    pub remote_sensors_last_fetch: Arc<Mutex<HashMap<String, Instant>>>,
    pub remote_sensors_errors: Arc<Mutex<HashMap<String, String>>>,
    pub remote_bluetooth_snapshots: Arc<Mutex<HashMap<String, crate::bluetooth::BluetoothSnapshot>>>,
    pub remote_bluetooth_in_flight: Arc<Mutex<HashSet<String>>>,
    pub remote_bluetooth_last_fetch: Arc<Mutex<HashMap<String, Instant>>>,
    #[allow(dead_code)]
    pub sensors_search_query: String,
    #[allow(dead_code)]
    pub sensors_category_filter: String,
    pub sensors_top_apps_sort: String,
    pub sensors_top_apps_sort_desc: bool,
    #[allow(dead_code)]
    pub sensors_top_apps_group_by_app: bool,
    pub sensors_top_apps_expanded_groups: HashSet<String>,
    pub sensors_top_apps_selected_pid: Option<u32>,
    pub sensors_top_apps_selected_start: Option<u64>,
    #[allow(dead_code)]
    pub sensors_top_apps_normalize_cpu: bool,
    pub sensors_top_apps_status_msg: Option<(String, bool, Instant)>,
    pub remote_process_results: Arc<Mutex<VecDeque<(String, bool)>>>,
    pub sensors_subtab: SensorsSubTab,
    pub sensors_top_apps_visited: bool,
    pub sensors_top_apps_section_apps_expanded: bool,
    pub sensors_top_apps_section_bg_expanded: bool,
    pub current_sensors_host: Option<String>,
    pub cpu_history: VecDeque<f32>,
    pub cpu_core_histories: Vec<VecDeque<f32>>,
    pub cpu_peak_usage: f32,
    pub ram_history: VecDeque<(f32, f32)>,
    pub disk_write_history: VecDeque<f32>,
    pub disk_read_history: VecDeque<f32>,
    pub net_tx_history: VecDeque<f32>,
    pub net_rx_history: VecDeque<f32>,
    pub net_peak_mb: f32,
    pub gpu_mode: usize,
    pub selected_gpu_idx: usize,
    pub selected_disk_idx: usize,
    pub per_disk_write_history: std::collections::HashMap<String, VecDeque<f32>>,
    pub per_disk_read_history: std::collections::HashMap<String, VecDeque<f32>>,
    pub last_telemetry_sample: Option<Instant>,

    // Power & Battery State (matching Image 1)
    pub power_ceiling: f32,
    pub cpu_tdp_profile: crate::power::CpuTdpProfile,

    // Bluetooth Control State (matching Image 3)
    pub bt_radio_on: bool,
    pub bt_discoverable: bool,
    pub bluetooth_snapshot: Arc<Mutex<crate::bluetooth::BluetoothSnapshot>>,
    pub bluetooth_in_flight: Arc<std::sync::atomic::AtomicBool>,
    pub bluetooth_scanning: Arc<std::sync::atomic::AtomicBool>,
    pub bluetooth_last_query: Instant,

    // Wi-Fi Control State (matching Image 4)
    pub wifi_snapshot: Arc<Mutex<crate::wifi::WifiSnapshot>>,
    pub wifi_selected_interface: Option<String>,
    pub wifi_in_flight: Arc<std::sync::atomic::AtomicBool>,
    pub wifi_last_query: Instant,
    pub wifi_status_msg: Option<(String, bool, Instant)>,
    pub wifi_action_in_flight: Arc<std::sync::atomic::AtomicBool>,
    pub wifi_action_results: Arc<Mutex<VecDeque<(String, bool, bool)>>>,
    // Power Subsystem Status Toast
    pub power_status_msg: Option<(String, bool, Instant)>,

    // Cached grouped processes for Top Apps tab (prevents re-filtering and sorting every frame)
    pub sensors_cached_apps_groups: Vec<ProcessGroupSummary>,
    pub sensors_cached_bg_groups: Vec<ProcessGroupSummary>,
    pub sensors_cached_snap_timestamp: Option<Instant>,
    pub sensors_cached_search: String,
    pub sensors_cached_sort: String,
    pub sensors_cached_sort_desc: bool,

    // Sudo popup for local system-level systemd operations
    pub sudo_popup: SudoPopupState,
    pub local_sudo_password: String, // remembered for the session

    // Import status notification: (message, is_error, timestamp)
    pub import_status_msg: Option<(String, bool, Instant)>,

    // Settings window open state
    pub settings_modal_open: bool,

    // Process Diagnostics & Control Modals (Properties, Memory Maps, Open Files, Set Affinity)
    pub proc_properties_modal_open: bool,
    pub proc_properties_pid: u32,
    pub proc_properties_name: String,
    pub proc_properties_data: Option<crate::procmgr::ProcessDetails>,
    pub proc_properties_error: Option<String>,
    pub proc_properties_loading: bool,
    pub proc_details_receiver: Arc<Mutex<Option<(u32, Result<crate::procmgr::ProcessDetails, String>)>>>,

    pub proc_maps_modal_open: bool,
    pub proc_maps_pid: u32,
    pub proc_maps_name: String,
    pub proc_maps_data: Option<Vec<crate::procmgr::MemoryMapEntry>>,
    pub proc_maps_error: Option<String>,
    pub proc_maps_search: String,
    pub proc_maps_loading: bool,
    pub proc_maps_receiver: Arc<Mutex<Option<(u32, Result<Vec<crate::procmgr::MemoryMapEntry>, String>)>>>,

    pub proc_open_files_modal_open: bool,
    pub proc_open_files_pid: u32,
    pub proc_open_files_name: String,
    pub proc_open_files_data: Option<Vec<crate::procmgr::OpenFileEntry>>,
    pub proc_open_files_error: Option<String>,
    pub proc_open_files_search: String,
    pub proc_open_files_loading: bool,
    pub proc_open_files_receiver: Arc<Mutex<Option<(u32, Result<Vec<crate::procmgr::OpenFileEntry>, String>)>>>,

    pub proc_affinity_modal_open: bool,
    pub proc_affinity_pid: u32,
    pub proc_affinity_start_marker: Option<u64>,
    pub proc_affinity_name: String,
    pub proc_affinity_host_id: Option<String>,
    pub proc_affinity_cores: Vec<bool>,
    pub proc_affinity_total_cores: usize,
    pub proc_affinity_error: Option<String>,
    pub proc_affinity_loading: bool,
    pub proc_affinity_receiver: Arc<Mutex<Option<(u32, Result<Vec<usize>, String>)>>>,

    // Local Host Apps Manager State
    pub apps_manager: crate::apps::AppsManagerState,
    remote_apps_managers: std::collections::HashMap<String, crate::apps::AppsManagerState>,
}

pub struct ExternalTerminalSession {
    pub logs: Arc<Mutex<VecDeque<crate::service::LogEntry>>>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum WindowButtonKind {
    Minimize,
    Maximize(bool),
    Close,
}

fn render_window_button(
    ui: &mut egui::Ui,
    kind: WindowButtonKind,
    palette: &crate::theme::ThemePalette,
) -> bool {
    let size = egui::vec2(34.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let hovered = resp.hovered();

    let (bg_color, stroke_color) = match kind {
        WindowButtonKind::Close => {
            if hovered {
                (Color32::from_rgb(232, 17, 35), Color32::WHITE)
            } else {
                (Color32::TRANSPARENT, palette.text_secondary)
            }
        }
        _ => {
            if hovered {
                (
                    if palette.is_dark {
                        Color32::from_rgba_unmultiplied(255, 255, 255, 22)
                    } else {
                        Color32::from_rgba_unmultiplied(0, 0, 0, 18)
                    },
                    palette.text_primary,
                )
            } else {
                (Color32::TRANSPARENT, palette.text_secondary)
            }
        }
    };

    if bg_color != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, Rounding::ZERO, bg_color);
    }

    let painter = ui.painter();
    let center = rect.center();
    let stroke = Stroke::new(1.0_f32, stroke_color);

    match kind {
        WindowButtonKind::Minimize => {
            let y = center.y + 1.0;
            painter.line_segment(
                [egui::pos2(center.x - 4.0, y), egui::pos2(center.x + 4.0, y)],
                stroke,
            );
        }
        WindowButtonKind::Maximize(maximized) => {
            if maximized {
                // Restore icon: two overlapping squares
                let back_top_left = egui::pos2(center.x - 2.5, center.y - 3.5);
                let back_top_right = egui::pos2(center.x + 3.5, center.y - 3.5);
                let back_bottom_right = egui::pos2(center.x + 3.5, center.y + 2.5);
                painter.line_segment([back_top_left, back_top_right], stroke);
                painter.line_segment([back_top_right, back_bottom_right], stroke);

                let front_rect = egui::Rect::from_min_size(
                    egui::pos2(center.x - 3.5, center.y - 2.5),
                    egui::vec2(6.0, 6.0),
                );
                painter.rect_stroke(front_rect, Rounding::ZERO, stroke);
            } else {
                // Maximize icon: crisp square outline
                let square = egui::Rect::from_center_size(center, egui::vec2(8.0, 8.0));
                painter.rect_stroke(square, Rounding::ZERO, stroke);
            }
        }
        WindowButtonKind::Close => {
            let d = 3.5;
            painter.line_segment(
                [egui::pos2(center.x - d, center.y - d), egui::pos2(center.x + d, center.y + d)],
                stroke,
            );
            painter.line_segment(
                [egui::pos2(center.x + d, center.y - d), egui::pos2(center.x - d, center.y + d)],
                stroke,
            );
        }
    }

    resp.hover_tip(match kind {
        WindowButtonKind::Minimize => "Minimize",
        WindowButtonKind::Maximize(true) => "Restore",
        WindowButtonKind::Maximize(false) => "Maximize",
        WindowButtonKind::Close => "Close",
    })
    .clicked()
}

fn ssh_identity_link(ui: &mut Ui, text: RichText, full_text: &str, hover_fill: Color32) -> bool {
    let background = ui.painter().add(egui::Shape::Noop);
    let response = ui.add(egui::Label::new(text).truncate().sense(egui::Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter().set(background, egui::Shape::rect_filled(
            response.rect.expand2(egui::vec2(3.0, 1.0)), Rounding::same(4.0), hover_fill,
        ));
    }
    response.on_hover_ui(|ui| {
        ui.label(RichText::new("Edit SSH host").size(11.0).strong());
        ui.label(RichText::new(full_text).size(10.5).weak());
    }).clicked()
}

impl LauncherApp {
    fn workspace_sidebar_width(&self) -> f32 {
        match self.active_sidebar_tab {
            SidebarTab::Servers => SERVERS_SIDEBAR_WIDTH,
            SidebarTab::Sensors => SENSORS_SIDEBAR_WIDTH,
            _ => WORKSPACE_SIDEBAR_WIDTH,
        }
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config_path = ConfigFile::default_path();
        let config_file = ConfigFile::load_from_file(&config_path);

        let mut services: Vec<Service> = config_file
            .servers
            .into_iter()
            .map(Service::new)
            .collect();

        // 🚀 Detect previously running servers and reconnect by verified process identity.
        crate::service::reconnect_services(&mut services);
        for service in &mut services {
            if service.state == ServiceState::Stopped && service.config.autostart {
                service.start();
            }
        }

        // 🚀 App autostart check and synchronization with system
        let sys_autostart = crate::autostart::is_app_autostart_enabled();
        let app_autostart = config_file.app_autostart || sys_autostart;
        if config_file.app_autostart && !sys_autostart {
            let _ = crate::autostart::set_app_autostart(true);
        }

        let scanner = SystemPortScanner::new();
        scanner.trigger_scan();

        let icon_texture = {
            let png_bytes = include_bytes!("../assets/icon_96.png");
            if let Ok(img) = image::load_from_memory(png_bytes) {
                let rgba = img.to_rgba8();
                let (w, h) = (rgba.width() as usize, rgba.height() as usize);
                let color_image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba.into_raw());
                Some(cc.egui_ctx.load_texture(
                    "app_header_icon",
                    color_image,
                    egui::TextureOptions {
                        magnification: egui::TextureFilter::Linear,
                        minification: egui::TextureFilter::Linear,
                        wrap_mode: egui::TextureWrapMode::ClampToEdge,
                        mipmap_mode: Some(egui::TextureFilter::Linear),
                    },
                ))
            } else {
                None
            }
        };

        let selected_machine = "local".to_string();
        let mut ssh_hosts = config_file.ssh_hosts;
        let mut credential_warning = None;
        for host in &mut ssh_hosts {
            if host.password.is_empty() {
                if let Some(reference) = &host.credential_ref {
                    match crate::ssh_credentials::load(reference, &crate::ssh_credentials::binding(host)) {
                        Ok(password) => host.password = password,
                        Err(error) => credential_warning = Some((format!("Could not unlock the SSH credential for {}: {error}", host.name), true, Instant::now())),
                    }
                }
            }
        }
        let saved_ssh_refs = ssh_hosts.iter().filter_map(|host| host.credential_ref.clone()).collect();
        let migrate_credentials = ssh_hosts.iter().any(|host| !host.password.is_empty() && host.credential_ref.is_none());
        let adb_devices = config_file.adb_devices;
        let scripts = config_file.scripts;
        let saved_commands = config_file.saved_commands;
        let ssh_host_modal = SshHostModalState::default();
        let remote_listeners = Arc::new(Mutex::new(HashMap::new()));
        let remote_scanning = Arc::default();
        let remote_terminal_logs = Arc::new(Mutex::new(HashMap::new()));
        let remote_terminal_inputs = HashMap::new();
        let remote_connection_status = Arc::new(Mutex::new(HashMap::new()));
        let remote_connecting = Arc::new(Mutex::new(BTreeSet::new()));
        let theme = config_file
            .theme
            .as_deref()
            .map(ThemeMode::from_id_str)
            .unwrap_or_default();
        let custom_accent_color = config_file.accent_color.clone();

        let mut script_executions = HashMap::new();
        for sc in &scripts {
            if sc.autostart && sc.host_id.is_none() {
                let mut exec = ScriptExecution::new(sc.id.clone());
                execute_local_script(sc, &mut exec, None);
                script_executions.insert(sc.id.clone(), exec);
            }
        }

        let cpu_tdp_profile = crate::power::detect_cpu_tdp_capability();

        let mut app = Self {
            active_sidebar_tab: SidebarTab::Servers,
            sidebar_tab_machine: "local".into(),
            machine_sidebar_tabs: HashMap::new(),
            scripts,
            saved_commands,
            command_catalog_view: None,
            config_path,
            services,
            active_tab: "overview".to_string(),
            server_tab_order: config_file.server_tab_order,
            active_group_filter: "All".to_string(),
            lan_ip: get_lan_ip(),
            sys: System::new(),
            scanner,
            last_poll: Instant::now() - std::time::Duration::from_secs(2),
            add_edit_modal: AddEditModalState::default(),
            palette_modal: CommandPaletteState::default(),
            search_queries: HashMap::new(),
            search_open: HashMap::new(),
            filter_lines: HashMap::new(),
            input_texts: HashMap::new(),
            input_histories: HashMap::new(),
            history_indices: HashMap::new(),
            stopped_external: HashMap::new(),
            overview_search_query: String::new(),
            focus_overview_search: false,
            autocomplete_selected: 0,
            expanded_listeners: BTreeSet::new(),
            expanded_server_cards: BTreeSet::new(),
            icon_texture,
            show_other_process_metrics: false,
            other_metrics: HashMap::new(),
            last_other_poll: Instant::now(),
            open_inline_terminals: BTreeSet::new(),
            inline_terminal_sessions: HashMap::new(),
            inline_terminal_inputs: HashMap::new(),
            selected_machine,
            ssh_hosts,
            remote_host_lifetimes: Arc::new(Mutex::new(HashMap::new())),
            saved_ssh_refs,
            ssh_host_modal,
            remote_listeners,
            remote_scanning,
            remote_terminal_logs,
            remote_command_activity: Arc::default(),
            ssh_idle: config_file.ssh_idle,
            ssh_probe_timeouts: config_file.ssh_probe_timeouts,
            remote_terminal_inputs,
            remote_connection_status,
            remote_connecting,
            remote_stopped_servers: Arc::new(Mutex::new(HashMap::new())),
            remote_start_server_modal: RemoteStartServerModalState::default(),
            remote_reboot_modal: RemoteRebootModalState::default(),
            remote_shutdown_modal: RemoteShutdownModalState::default(),
            theme,
            custom_accent_color: custom_accent_color.clone(),
            settings_active_tab: SettingsTab::default(),
            settings_custom_hex_input: custom_accent_color.unwrap_or_else(|| "#3584e4".to_string()),
            last_applied_palette: None,
            app_autostart,
            auto_scroll: HashMap::new(),
            proc_info_cache: HashMap::new(),
            remote_search_query: HashMap::new(),
            close_confirm_open: false,
            close_confirmed: false,
            close_waiting_for_ssh_commands: false,

            script_executions,
            script_modal: ScriptModalState::default(),
            sudo_modal: SudoModalState::default(),
            selected_script_id: None,
            script_search_query: String::new(),

            command_modal: CommandModalState::default(),
            local_terminal: TerminalSession::default(),
            remote_terminals: HashMap::new(),
            saved_command_search: String::new(),
            saved_command_category: "All".to_string(),
            saved_command_status_msg: None,

            systemd_modal: NewServiceModalState::default(),
            local_services: Vec::new(),
            local_services_user: true,
            local_services_loaded: false,
            local_services_cache: Arc::new(Mutex::new(HashMap::new())),
            local_services_in_flight: Arc::new(Mutex::new(HashSet::new())),
            remote_services: Arc::new(Mutex::new(HashMap::new())),
            remote_services_in_flight: Arc::new(Mutex::new(HashSet::new())),
            remote_services_errors: Arc::new(Mutex::new(HashMap::new())),
            remote_services_user: HashMap::new(),
            selected_service: None,
            local_journal_logs: Arc::new(Mutex::new(HashMap::new())),
            remote_journal_logs: Arc::new(Mutex::new(HashMap::new())),
            systemd_actions_in_flight: Arc::new(Mutex::new(HashSet::new())),
            systemd_action_results: Arc::new(Mutex::new(VecDeque::new())),
            remote_sudo_passwords: HashMap::new(),
            service_details_cache: HashMap::new(),
            service_query_errors: HashMap::new(),
            remote_service_action_msg: Arc::new(Mutex::new(None)),
            service_search_query: String::new(),
            service_action_msg: None,
            service_enabled_cache: HashMap::new(),
            journal_refresh_interval: config_file.journal_refresh_interval.unwrap_or(0),
            last_journal_refresh: Instant::now(),

            systemd_tab: SystemdTab::Status,
            systemd_files_cache: Arc::new(Mutex::new(HashMap::new())),
            systemd_files_in_flight: Arc::new(Mutex::new(HashSet::new())),
            systemd_file_content_cache: Arc::new(Mutex::new(HashMap::new())),
            systemd_selected_file: None,
            systemd_file_content: String::new(),
            systemd_file_original: String::new(),
            systemd_editor_context: None,
            systemd_file_status_msg: None,
            systemd_new_dropin_open: false,
            systemd_new_dropin_name: "override.conf".to_string(),
            systemd_new_dropin_template: 0,
            systemd_new_dropin_content: "[Service]\n".into(),
            systemd_deps_cache: Arc::new(Mutex::new(HashMap::new())),
            systemd_deps_in_flight: Arc::new(Mutex::new(HashSet::new())),
            systemd_deps_history: Vec::new(),
            systemd_deps_filter: String::new(),
            systemd_deps_view_mode: 0,

            sensors_collector: Arc::new(Mutex::new(crate::sensors::SystemSensorsCollector::new())),
            sensors_in_flight: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            sensors_shared_snapshot: Arc::new(Mutex::new(Arc::new(crate::sensors::SystemSensorsSnapshot::default()))),
            sensors_snapshot: Arc::new(crate::sensors::SystemSensorsSnapshot::default()),
            sensors_refresh_interval: 1,
            last_sensors_refresh: Instant::now() - Duration::from_secs(10),
            remote_sensors_snapshots: Arc::new(Mutex::new(HashMap::new())),
            remote_sensors_in_flight: Arc::new(Mutex::new(HashSet::new())),
            remote_sensors_last_fetch: Arc::new(Mutex::new(HashMap::new())),
            remote_sensors_errors: Arc::new(Mutex::new(HashMap::new())),
            remote_bluetooth_snapshots: Arc::new(Mutex::new(HashMap::new())),
            remote_bluetooth_in_flight: Arc::new(Mutex::new(HashSet::new())),
            remote_bluetooth_last_fetch: Arc::new(Mutex::new(HashMap::new())),
            sensors_search_query: String::new(),
            sensors_category_filter: "All".to_string(),
            sensors_top_apps_sort: "cpu".to_string(),
            sensors_top_apps_sort_desc: true,
            sensors_top_apps_group_by_app: true,
            sensors_top_apps_expanded_groups: HashSet::new(),
            sensors_top_apps_selected_pid: None,
            sensors_top_apps_selected_start: None,
            sensors_top_apps_normalize_cpu: true,
            sensors_top_apps_status_msg: None,
            remote_process_results: Arc::new(Mutex::new(VecDeque::new())),
            sensors_subtab: SensorsSubTab::Overview,
            sensors_top_apps_visited: false,
            sensors_top_apps_section_apps_expanded: true,
            sensors_top_apps_section_bg_expanded: true,
            current_sensors_host: None,
            cpu_history: VecDeque::with_capacity(60),
            cpu_core_histories: Vec::new(),
            cpu_peak_usage: 0.0,
            ram_history: VecDeque::with_capacity(60),
            disk_write_history: VecDeque::with_capacity(60),
            disk_read_history: VecDeque::with_capacity(60),
            net_tx_history: VecDeque::with_capacity(60),
            net_rx_history: VecDeque::with_capacity(60),
            net_peak_mb: 0.0,
            gpu_mode: 1, // Balanced
            selected_gpu_idx: 0,
            selected_disk_idx: 0,
            per_disk_write_history: std::collections::HashMap::new(),
            per_disk_read_history: std::collections::HashMap::new(),
            last_telemetry_sample: None,
            power_ceiling: 80.0,
            cpu_tdp_profile,
            bt_radio_on: false,
            bt_discoverable: false,
            bluetooth_snapshot: Arc::new(Mutex::new(crate::bluetooth::BluetoothSnapshot::default())),
            bluetooth_in_flight: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            bluetooth_scanning: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            bluetooth_last_query: Instant::now() - std::time::Duration::from_secs(10), // trigger immediately
            wifi_snapshot: Arc::new(Mutex::new(crate::wifi::WifiSnapshot::default())),
            wifi_selected_interface: None,
            wifi_in_flight: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            wifi_last_query: Instant::now() - std::time::Duration::from_secs(10), // trigger immediately
            wifi_status_msg: None,
            wifi_action_in_flight: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            wifi_action_results: Arc::new(Mutex::new(VecDeque::new())),
            power_status_msg: None,
            sensors_cached_apps_groups: Vec::new(),
            sensors_cached_bg_groups: Vec::new(),
            sensors_cached_snap_timestamp: None,
            sensors_cached_search: String::new(),
            sensors_cached_sort: String::new(),
            sensors_cached_sort_desc: false,
            sudo_popup: SudoPopupState::default(),
            local_sudo_password: String::new(),
            import_status_msg: credential_warning,
            settings_modal_open: false,

            proc_properties_modal_open: false,
            proc_properties_pid: 0,
            proc_properties_name: String::new(),
            proc_properties_data: None,
            proc_properties_error: None,
            proc_properties_loading: false,
            proc_details_receiver: Arc::new(Mutex::new(None)),

            proc_maps_modal_open: false,
            proc_maps_pid: 0,
            proc_maps_name: String::new(),
            proc_maps_data: None,
            proc_maps_error: None,
            proc_maps_search: String::new(),
            proc_maps_loading: false,
            proc_maps_receiver: Arc::new(Mutex::new(None)),

            proc_open_files_modal_open: false,
            proc_open_files_pid: 0,
            proc_open_files_name: String::new(),
            proc_open_files_data: None,
            proc_open_files_error: None,
            proc_open_files_search: String::new(),
            proc_open_files_loading: false,
            proc_open_files_receiver: Arc::new(Mutex::new(None)),

            proc_affinity_modal_open: false,
            proc_affinity_pid: 0,
            proc_affinity_start_marker: None,
            proc_affinity_name: String::new(),
            proc_affinity_host_id: None,
            proc_affinity_cores: Vec::new(),
            proc_affinity_total_cores: 0,
            proc_affinity_error: None,
            proc_affinity_loading: false,
            proc_affinity_receiver: Arc::new(Mutex::new(None)),

            apps_manager: crate::apps::AppsManagerState::default(),
            remote_apps_managers: std::collections::HashMap::new(),

            // ADB / Android Device Cockpit State
            adb_devices,
            adb_tauri_opened: HashSet::new(),
            add_device_selector_modal_open: false,
            adb_add_modal_open: false,
            adb_detected_devices: Vec::new(),
            adb_wireless_ip_input: String::new(),
            adb_wireless_port_input: "5555".to_string(),
            adb_device_name_input: String::new(),
            adb_active_subtab: AdbSubTab::Overview,
            adb_telemetry_cache: HashMap::new(),
            adb_telemetry_pending: HashSet::new(),
            adb_telemetry_results: Arc::new(Mutex::new(HashMap::new())),
            adb_data_serial: String::new(),
            adb_queries_loaded: HashSet::new(),
            adb_status_msg: None,
            adb_input_text_field: String::new(),
            adb_clipboard_cache: HashMap::new(),
            adb_clipboard_send_text: String::new(),
            adb_shell_cmd_input: String::new(),
            adb_shell_history: Vec::new(),
            adb_file_current_path: "/sdcard".to_string(),
            adb_file_entries: Vec::new(),
            adb_fastboot_partition: "boot".to_string(),
            adb_package_filter: String::new(),
            adb_package_is_system: false,
            adb_packages_cache: Vec::new(),
            adb_scrcpy_max_fps: 60,
            adb_scrcpy_bitrate_mbps: 8,
            adb_scrcpy_stay_awake: true,
            adb_scrcpy_screen_off: false,
            adb_scrcpy_no_audio: false,
            adb_scrcpy_pids: HashMap::new(),
            adb_mirror_mode: AdbScreenMirrorMode::BuiltIn,
            adb_mirror_active: false,
            adb_mirror_texture: None,
            adb_mirror_frame_rx: None,
            adb_mirror_stop_tx: None,
            adb_mirror_last_frame_time: None,
            adb_mirror_fps: 0.0,
            adb_mirror_device_res: (800, 1280),
            adb_mirror_drag_start: None,
            adb_screenshare_text_input: String::new(),
            adb_file_filter: String::new(),

            // Phone Link Suite State Defaults
            adb_notifications: Vec::new(),
            adb_call_logs: Vec::new(),
            adb_dialer_input: String::new(),
            adb_sms_messages: Vec::new(),
            adb_sms_selected_contact: None,
            adb_sms_input: String::new(),
            adb_photos: Vec::new(),
            adb_selected_photo: None,
            adb_photo_preview: None,
            adb_dnd_enabled: false,
            adb_audio_muted: false,
            adb_media_playing: false,
            adb_notifications_filter: String::new(),
            adb_call_search: String::new(),
            adb_sms_search: String::new(),
            adb_quick_call_number: String::new(),
        };
        if migrate_credentials {
            app.persist_config();
        }
        app
    }

    pub fn trigger_remote_sensors_query(&self, host: SshRemoteHost) {
        let Some(lifetime) = self.remote_host_lifetime(&host) else { return };
        let hid = host.id.clone();
        {
            let mut in_flight = self.remote_sensors_in_flight.lock().unwrap();
            if in_flight.contains(&hid) {
                return;
            }
            in_flight.insert(hid.clone());
        }

        let snapshots_arc = self.remote_sensors_snapshots.clone();
        let include_processes = matches!(self.sensors_subtab, SensorsSubTab::TopApps | SensorsSubTab::Ram);
        let include_process_network = self.sensors_subtab == SensorsSubTab::TopApps;
        let include_network_details = self.sensors_subtab == SensorsSubTab::Wifi;
        let include_gpu_engines = self.sensors_subtab == SensorsSubTab::Gpu;
        let include_ram_modules = self.sensors_subtab == SensorsSubTab::Ram;
        let include_network_rates = matches!(self.sensors_subtab, SensorsSubTab::Overview | SensorsSubTab::Wifi | SensorsSubTab::DataUsage);
        let include_cpu_cores = self.sensors_subtab == SensorsSubTab::Cpu;
        let include_disk_rates = matches!(self.sensors_subtab, SensorsSubTab::Overview | SensorsSubTab::Storage);
        let include_thermals = !matches!(self.sensors_subtab, SensorsSubTab::TopApps | SensorsSubTab::Wifi | SensorsSubTab::DataUsage | SensorsSubTab::Bluetooth);
        let in_flight_arc = self.remote_sensors_in_flight.clone();
        let last_fetch_arc = self.remote_sensors_last_fetch.clone();
        let errors_arc = self.remote_sensors_errors.clone();
        let lifetimes_arc = self.remote_host_lifetimes.clone();

        std::thread::spawn(move || {
            let result = crate::sensors::query_remote_sensors(&host, include_processes, include_process_network, include_network_details, include_gpu_engines, include_ram_modules, include_network_rates, include_cpu_cores, include_disk_rates, include_thermals);
            let lifetimes = lifetimes_arc.lock().unwrap();
            if !remote_host_lifetime_matches(&lifetimes, &hid, &lifetime) { return; }
            match result {
                Ok(snap) => {
                    snapshots_arc.lock().unwrap().insert(hid.clone(), Arc::new(snap));
                    last_fetch_arc.lock().unwrap().insert(hid.clone(), Instant::now());
                    errors_arc.lock().unwrap().remove(&hid);
                }
                Err(e) => {
                    errors_arc.lock().unwrap().insert(hid.clone(), e);
                    last_fetch_arc.lock().unwrap().insert(hid.clone(), Instant::now());
                }
            }
            in_flight_arc.lock().unwrap().remove(&hid);
        });
    }

    fn trigger_remote_bluetooth_query(&self, host: SshRemoteHost, windows_host: bool) {
        let Some(lifetime) = self.remote_host_lifetime(&host) else { return };
        let hid = host.id.clone();
        {
            let mut in_flight = self.remote_bluetooth_in_flight.lock().unwrap();
            if !in_flight.insert(hid.clone()) { return; }
        }
        let snapshots = self.remote_bluetooth_snapshots.clone();
        let in_flight = self.remote_bluetooth_in_flight.clone();
        let last_fetch = self.remote_bluetooth_last_fetch.clone();
        let lifetimes = self.remote_host_lifetimes.clone();
        std::thread::spawn(move || {
            let result = if windows_host {
                crate::bluetooth::query_remote_windows_bluetooth_status(&host)
            } else {
                crate::bluetooth::query_remote_bluetooth_status(&host)
            };
            let snapshot = result.unwrap_or_else(|error| {
                let mut snapshot = crate::bluetooth::BluetoothSnapshot::default();
                snapshot.status_msg = Some((error, false, Instant::now()));
                snapshot
            });
            let lifetimes = lifetimes.lock().unwrap();
            if !remote_host_lifetime_matches(&lifetimes, &hid, &lifetime) { return; }
            snapshots.lock().unwrap().insert(hid.clone(), snapshot);
            last_fetch.lock().unwrap().insert(hid.clone(), Instant::now());
            in_flight.lock().unwrap().remove(&hid);
        });
    }

    pub fn trigger_local_sensors_query(&self, include_top: bool, ctx: Option<egui::Context>) {
        if self.sensors_in_flight.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }

        let collector_arc = Arc::clone(&self.sensors_collector);
        let include_process_network = include_top && self.sensors_subtab == SensorsSubTab::TopApps;
        let in_flight_arc = Arc::clone(&self.sensors_in_flight);
        let shared_snap_arc = Arc::clone(&self.sensors_shared_snapshot);

        std::thread::spawn(move || {
            let snap = if let Ok(mut coll) = collector_arc.lock() {
                let mut snapshot = coll.collect_with_top_apps(include_top, include_process_network);
                coll.apply_protected_hardware(&mut snapshot);
                snapshot
            } else {
                crate::sensors::SystemSensorsSnapshot::default()
            };
            if let Ok(mut shared) = shared_snap_arc.lock() {
                *shared = Arc::new(snap);
            }
            in_flight_arc.store(false, std::sync::atomic::Ordering::SeqCst);
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    pub fn trigger_bluetooth_query(&mut self, ctx: Option<egui::Context>) {
        if self.bluetooth_in_flight.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }

        self.bluetooth_last_query = Instant::now();
        let snap_arc = Arc::clone(&self.bluetooth_snapshot);
        let in_flight_arc = Arc::clone(&self.bluetooth_in_flight);

        std::thread::spawn(move || {
            let snap = crate::bluetooth::query_bluetooth_status();
            if let Ok(mut lock) = snap_arc.lock() {
                let old_msg = lock.status_msg.clone();
                *lock = snap;
                if let Some((msg, is_err, time)) = old_msg {
                    if time.elapsed().as_secs() < 6 {
                        lock.status_msg = Some((msg, is_err, time));
                    }
                }
            }
            in_flight_arc.store(false, std::sync::atomic::Ordering::SeqCst);
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    pub fn trigger_wifi_query(&mut self, ctx: Option<egui::Context>) {
        if self.wifi_in_flight.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }

        self.wifi_last_query = Instant::now();
        let snap_arc = Arc::clone(&self.wifi_snapshot);
        let in_flight_arc = Arc::clone(&self.wifi_in_flight);
        let selected = self.wifi_selected_interface.clone();

        std::thread::spawn(move || {
            let snap = crate::wifi::query_wifi_status(selected.as_deref());
            if let Ok(mut lock) = snap_arc.lock() {
                *lock = snap;
            }
            in_flight_arc.store(false, std::sync::atomic::Ordering::SeqCst);
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    fn queue_wifi_action(&mut self, interface: Option<String>, ctx: egui::Context) {
        if self.wifi_action_in_flight.swap(true, std::sync::atomic::Ordering::SeqCst) { return; }
        self.wifi_status_msg = Some((if interface.is_some() { "Requesting Wi-Fi scan…" } else { "Flushing DNS cache…" }.into(), false, Instant::now()));
        let in_flight = Arc::clone(&self.wifi_action_in_flight);
        let results = Arc::clone(&self.wifi_action_results);
        std::thread::spawn(move || {
            let refresh = interface.is_some();
            let result = if let Some(interface) = interface { crate::wifi::scan_wifi(&interface) } else { crate::wifi::flush_dns() };
            let (message, error) = match result { Ok(message) => (message, false), Err(error) => (error, true) };
            if let Ok(mut results) = results.lock() { results.push_back((message, error, refresh && !error)); }
            in_flight.store(false, std::sync::atomic::Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    fn build_config_snapshot(&self, ssh_hosts: Vec<SshRemoteHost>) -> ConfigFile {
        ConfigFile {
            servers: self.services.iter().map(|s| s.config.clone()).collect(),
            server_tab_order: self.server_tab_order.clone(),
            ssh_hosts,
            adb_devices: self.adb_devices.clone(),
            scripts: self.scripts.clone(),
            saved_commands: self.saved_commands.clone(),
            theme: Some(self.theme.id_str().to_string()),
            accent_color: self.custom_accent_color.clone(),
            app_autostart: self.app_autostart,
            journal_refresh_interval: Some(self.journal_refresh_interval),
            ssh_idle: self.ssh_idle,
            ssh_probe_timeouts: self.ssh_probe_timeouts,
        }
    }

    fn persist_config(&mut self) {
        let mut ssh_hosts = self.ssh_hosts.clone();
        let mut staged_refs = Vec::new();
        for host in &mut ssh_hosts {
            if !host.password.is_empty() && host.credential_ref.is_none() {
                match crate::ssh_credentials::protect(&host.password, &crate::ssh_credentials::binding(host)) {
                    Ok(reference) => {
                        staged_refs.push(reference.clone());
                        host.credential_ref = Some(reference);
                    }
                    Err(error) => {
                        crate::ssh_credentials::discard(staged_refs);
                        self.import_status_msg = Some((format!("Could not protect the SSH password for {}: {error}", host.name), true, Instant::now()));
                        return;
                    }
                }
            }
            host.password.clear();
        }
        let cfg = self.build_config_snapshot(ssh_hosts);
        if let Err(error) = cfg.save_to_file(&self.config_path) {
            crate::ssh_credentials::discard(staged_refs);
            eprintln!("Configuration save failed: {error}");
            self.import_status_msg = Some((format!("Could not save settings: {error}"), true, Instant::now()));
        } else {
            let saved_refs = cfg.ssh_hosts.iter().filter_map(|host| host.credential_ref.clone()).collect();
            let removed = self.saved_ssh_refs.difference(&saved_refs).cloned().collect();
            self.saved_ssh_refs = saved_refs;
            crate::ssh_credentials::discard(removed);
            for (host, stored) in self.ssh_hosts.iter_mut().zip(cfg.ssh_hosts) {
                host.credential_ref = stored.credential_ref;
            }
        }
    }

    pub fn import_servers_from_file_dialog(&mut self) {
        let file_opt = rfd::FileDialog::new()
            .set_title("Import servers.json Configuration")
            .add_filter("JSON Configuration (*.json)", &["json"])
            .pick_file();

        if let Some(path) = file_opt {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    match self.import_from_json_str(&content) {
                        Ok((imported_count, updated_count)) => {
                            self.persist_config();
                            self.import_status_msg = Some((
                                format!("{} Imported {} new, {} updated server(s)", crate::lucide::CHECK, imported_count, updated_count),
                                false,
                                Instant::now(),
                            ));
                        }
                        Err(err) => {
                            self.import_status_msg = Some((
                                format!("Import failed: {}", err),
                                true,
                                Instant::now(),
                            ));
                        }
                    }
                }
                Err(err) => {
                    self.import_status_msg = Some((
                        format!("Failed to read file: {}", err),
                        true,
                        Instant::now(),
                    ));
                }
            }
        }
    }

    pub fn export_servers_to_file_dialog(&mut self) {
        let file_opt = rfd::FileDialog::new()
            .set_title("Export Full Backup (servers.json)")
            .set_file_name("server_launcher_backup.json")
            .add_filter("JSON Configuration (*.json)", &["json"])
            .save_file();

        if let Some(path) = file_opt {
            self.persist_config();
            let mut ssh_hosts = self.ssh_hosts.clone();
            for h in &mut ssh_hosts {
                h.password.clear();
            }
            let cfg = self.build_config_snapshot(ssh_hosts);
            match serde_json::to_string_pretty(&cfg) {
                Ok(json_str) => {
                    match std::fs::write(&path, json_str) {
                        Ok(_) => {
                            let fname = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "backup.json".to_string());
                            self.import_status_msg = Some((
                                format!(
                                    "{} Exported full backup ({} servers, {} SSH hosts, {} ADB devices, {} scripts, {} commands) to {}",
                                    crate::lucide::CHECK,
                                    cfg.servers.len(),
                                    cfg.ssh_hosts.len(),
                                    cfg.adb_devices.len(),
                                    cfg.scripts.len(),
                                    cfg.saved_commands.len(),
                                    fname
                                ),
                                false,
                                Instant::now(),
                            ));
                        }
                        Err(e) => {
                            self.import_status_msg = Some((
                                format!("Export error: {}", e),
                                true,
                                Instant::now(),
                            ));
                        }
                    }
                }
                Err(e) => {
                    self.import_status_msg = Some((
                        format!("Serialization error: {}", e),
                        true,
                        Instant::now(),
                    ));
                }
            }
        }
    }

    pub fn export_commands_to_file_dialog(&mut self, host_id: Option<&str>) {
        let file_opt = rfd::FileDialog::new()
            .set_title("Export Saved Commands")
            .set_file_name("saved_commands.json")
            .add_filter("JSON Files (*.json)", &["json"])
            .save_file();

        if let Some(path) = file_opt {
            let to_export: Vec<SavedCommand> = if let Some(hid) = host_id {
                let filtered: Vec<_> = self.saved_commands.iter().filter(|c| c.host_id.as_deref() == Some(hid)).cloned().collect();
                if filtered.is_empty() { self.saved_commands.clone() } else { filtered }
            } else {
                let filtered: Vec<_> = self.saved_commands.iter().filter(|c| c.host_id.is_none()).cloned().collect();
                if filtered.is_empty() { self.saved_commands.clone() } else { filtered }
            };

            match serde_json::to_string_pretty(&to_export) {
                Ok(json_str) => match std::fs::write(&path, json_str) {
                    Ok(_) => {
                        let fname = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "saved_commands.json".to_string());
                        self.saved_command_status_msg = Some((
                            format!("{} Exported {} command(s) to {}", crate::lucide::CHECK, to_export.len(), fname),
                            false,
                            Instant::now(),
                        ));
                    }
                    Err(e) => {
                        self.saved_command_status_msg = Some((
                            format!("Export error: {}", e),
                            true,
                            Instant::now(),
                        ));
                    }
                },
                Err(e) => {
                    self.saved_command_status_msg = Some((
                        format!("Serialization error: {}", e),
                        true,
                        Instant::now(),
                    ));
                }
            }
        }
    }

    pub fn import_commands_from_file_dialog(&mut self, host_id: Option<&str>) {
        let file_opt = rfd::FileDialog::new()
            .set_title("Import Saved Commands")
            .add_filter("JSON Files (*.json)", &["json"])
            .pick_file();

        if let Some(path) = file_opt {
            match std::fs::read_to_string(&path) {
                Ok(content) => match self.import_commands_from_json_str(&content, host_id) {
                    Ok((imported, updated)) => {
                        self.persist_config();
                        self.saved_command_status_msg = Some((
                            format!("{} Imported {} new, {} updated command(s)", crate::lucide::CHECK, imported, updated),
                            false,
                            Instant::now(),
                        ));
                    }
                    Err(err) => {
                        self.saved_command_status_msg = Some((
                            format!("Import error: {}", err),
                            true,
                            Instant::now(),
                        ));
                    }
                },
                Err(err) => {
                    self.saved_command_status_msg = Some((
                        format!("Failed to read file: {}", err),
                        true,
                        Instant::now(),
                    ));
                }
            }
        }
    }

    pub fn import_commands_from_json_str(&mut self, content: &str, target_host: Option<&str>) -> Result<(usize, usize), String> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err("File is empty".to_string());
        }

        let parsed_commands: Vec<SavedCommand> = if let Ok(cmds) = serde_json::from_str::<Vec<SavedCommand>>(trimmed) {
            cmds
        } else if let Ok(wrapper) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(arr) = wrapper.get("commands").or_else(|| wrapper.get("saved_commands")) {
                serde_json::from_value(arr.clone()).map_err(|e| format!("Failed to parse commands array: {}", e))?
            } else {
                return Err("Expected JSON array of commands or object with 'commands' key".to_string());
            }
        } else {
            return Err("Invalid JSON structure".to_string());
        };

        if parsed_commands.is_empty() {
            return Err("No valid commands found in file".to_string());
        }

        let mut imported_count = 0;
        let mut updated_count = 0;

        for mut cmd in parsed_commands {
            if cmd.id.trim().is_empty() {
                cmd.id = format!("cmd_{}_{}", chrono::Utc::now().timestamp_millis(), imported_count + updated_count);
            }
            if cmd.host_id.is_none() && target_host.is_some() {
                cmd.host_id = target_host.map(|s| s.to_string());
            }

            if let Some(idx) = self.saved_commands.iter().position(|c| c.id == cmd.id || (c.name == cmd.name && c.command == cmd.command && c.host_id == cmd.host_id)) {
                self.saved_commands[idx] = cmd;
                updated_count += 1;
            } else {
                self.saved_commands.push(cmd);
                imported_count += 1;
            }
        }

        if imported_count + updated_count > 0 {
            self.command_catalog_view = None;
        }
        Ok((imported_count, updated_count))
    }

    pub fn import_from_json_str(&mut self, content: &str) -> Result<(usize, usize), String> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err("File is empty".to_string());
        }

        let mut imported = 0;
        let mut updated = 0;

        fn add_server(
            srv_cfg: ServerConfig,
            services: &mut Vec<Service>,
            imported: &mut usize,
            updated: &mut usize,
        ) {
            let key = if srv_cfg.key.trim().is_empty() {
                srv_cfg.name.to_lowercase().replace(' ', "_")
            } else {
                srv_cfg.key.clone()
            };
            let mut final_cfg = srv_cfg;
            final_cfg.key = key;

            if let Some(existing) = services.iter_mut().find(|s| s.config.key == final_cfg.key) {
                existing.config = final_cfg;
                *updated += 1;
            } else {
                services.push(Service::new(final_cfg));
                *imported += 1;
            }
        }

        // 1. Try parsing full ConfigFile (when JSON object contains ConfigFile keys)
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(obj) = val.as_object() {
                let is_full_config = [
                    "ssh_hosts",
                    "adb_devices",
                    "scripts",
                    "saved_commands",
                    "server_tab_order",
                    "theme",
                    "accent_color",
                    "ssh_idle",
                    "ssh_probe_timeouts",
                ]
                .iter()
                .any(|k| obj.contains_key(*k));

                if is_full_config {
                    if let Ok(cfg) = serde_json::from_value::<ConfigFile>(val.clone()) {
                        for s in cfg.servers {
                            add_server(s, &mut self.services, &mut imported, &mut updated);
                        }
                        for key in cfg.server_tab_order {
                            if !self.server_tab_order.contains(&key) {
                                self.server_tab_order.push(key);
                            }
                        }
                        for h in cfg.ssh_hosts {
                            if let Some(existing) = self.ssh_hosts.iter_mut().find(|e| e.id == h.id) {
                                let kept_ref = existing.credential_ref.clone();
                                *existing = h;
                                if existing.credential_ref.is_none() {
                                    existing.credential_ref = kept_ref;
                                }
                                updated += 1;
                            } else {
                                self.ssh_hosts.push(h);
                                imported += 1;
                            }
                        }
                        for dev in cfg.adb_devices {
                            if let Some(existing) = self.adb_devices.iter_mut().find(|e| e.id == dev.id) {
                                *existing = dev;
                                updated += 1;
                            } else {
                                self.adb_devices.push(dev);
                                imported += 1;
                            }
                        }
                        for sc in cfg.scripts {
                            if let Some(existing) = self.scripts.iter_mut().find(|e| e.id == sc.id) {
                                *existing = sc;
                                updated += 1;
                            } else {
                                self.scripts.push(sc);
                                imported += 1;
                            }
                        }
                        for cmd in cfg.saved_commands {
                            if let Some(existing) = self.saved_commands.iter_mut().find(|e| e.id == cmd.id) {
                                *existing = cmd;
                                updated += 1;
                            } else {
                                self.saved_commands.push(cmd);
                                imported += 1;
                            }
                            self.command_catalog_view = None;
                        }
                        if let Some(theme_id) = cfg.theme {
                            self.theme = ThemeMode::from_id_str(&theme_id);
                        }
                        if cfg.accent_color.is_some() {
                            self.custom_accent_color = cfg.accent_color.clone();
                            if let Some(ref hex) = self.custom_accent_color {
                                self.settings_custom_hex_input = hex.clone();
                            }
                        }
                        self.app_autostart = cfg.app_autostart;
                        if let Some(interval) = cfg.journal_refresh_interval {
                            self.journal_refresh_interval = interval;
                        }
                        self.ssh_idle = cfg.ssh_idle;
                        self.ssh_probe_timeouts = cfg.ssh_probe_timeouts;
                        return Ok((imported, updated));
                    }
                }
            }
        }

        // 2. Try parsing as Vec<ServerConfig> array
        if let Ok(servers) = serde_json::from_str::<Vec<ServerConfig>>(trimmed) {
            if !servers.is_empty() {
                for s in servers {
                    add_server(s, &mut self.services, &mut imported, &mut updated);
                }
                return Ok((imported, updated));
            }
        }

        // 3. Try parsing generic JSON object with a "servers" or "mcpServers" field, or direct map
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(servers_val) = val.get("servers") {
                if let Ok(servers) = serde_json::from_value::<Vec<ServerConfig>>(servers_val.clone()) {
                    if !servers.is_empty() {
                        for s in servers {
                            add_server(s, &mut self.services, &mut imported, &mut updated);
                        }
                        return Ok((imported, updated));
                    }
                } else if let Some(map) = servers_val.as_object() {
                    for (key, srv_val) in map {
                        if let Some(srv_cfg) = crate::config::parse_server_json_value(key, srv_val) {
                            add_server(srv_cfg, &mut self.services, &mut imported, &mut updated);
                        }
                    }
                    if imported > 0 || updated > 0 {
                        return Ok((imported, updated));
                    }
                }
            }

            // MCP servers format (Claude Desktop / Gemini CLI / Cursor configs)
            if let Some(mcp_map) = val.get("mcpServers").and_then(|v| v.as_object()) {
                for (key, srv_val) in mcp_map {
                    if let Some(srv_cfg) = crate::config::parse_server_json_value(key, srv_val) {
                        add_server(srv_cfg, &mut self.services, &mut imported, &mut updated);
                    }
                }
                if imported > 0 || updated > 0 {
                    return Ok((imported, updated));
                }
            }

            // Dictionary format: { "stitch": { "type": "http", ... } }
            if let Some(map) = val.as_object() {
                let mut map_imported = false;
                for (key, srv_val) in map {
                    if let Some(srv_cfg) = crate::config::parse_server_json_value(key, srv_val) {
                        add_server(srv_cfg, &mut self.services, &mut imported, &mut updated);
                        map_imported = true;
                    }
                }
                if map_imported && (imported > 0 || updated > 0) {
                    return Ok((imported, updated));
                }
            }

            // Single server object
            if let Some(single) = crate::config::parse_server_json_value("imported", &val) {
                add_server(single, &mut self.services, &mut imported, &mut updated);
                return Ok((imported, updated));
            }
        }

        Err("Invalid servers JSON format. Expected a configuration containing a 'servers' list or an array of server objects.".to_string())
    }

    fn start_all(&mut self) {
        for s in &mut self.services {
            s.start();
        }
    }

    fn stop_all(&mut self) {
        for s in &mut self.services {
            s.stop();
        }
    }

    fn start_group(&mut self, group: &str) {
        for s in &mut self.services {
            if s.config.group == group {
                s.start();
            }
        }
    }

    fn stop_group(&mut self, group: &str) {
        for s in &mut self.services {
            if s.config.group == group {
                s.stop();
            }
        }
    }

    fn find_service_mut(&mut self, key: &str) -> Option<&mut Service> {
        self.services.iter_mut().find(|s| s.config.key == key)
    }

    fn find_service(&self, key: &str) -> Option<&Service> {
        self.services.iter().find(|s| s.config.key == key)
    }

    fn run_service_action(&mut self, key: &str, command: &str) {
        if let Some(s) = self.find_service_mut(key) {
            s.append_log(format!("[action] Executing: {}", command), LogKind::Launcher);
            let cwd = s.config.cwd.clone();
            let env_vars = s.config.env.clone();
            let cmd_str = command.to_string();
            let logs_clone = s.logs.clone();
            std::thread::spawn(move || {
                let current_dir = if cwd.is_empty() { ".".to_string() } else { cwd };
                #[cfg(target_os = "windows")]
                let mut cmd = std::process::Command::new("cmd.exe");
                #[cfg(target_os = "windows")]
                cmd.args(&["/C", &cmd_str]);

                #[cfg(not(target_os = "windows"))]
                let mut cmd = std::process::Command::new("sh");
                #[cfg(not(target_os = "windows"))]
                cmd.args(&["-c", &cmd_str]);

                cmd.current_dir(&current_dir);
                crate::service::prepare_environment(&mut cmd, &current_dir, &env_vars);

                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());

                match cmd.spawn() {
                    Ok(mut child) => {
                        if let Some(stdout) = child.stdout.take() {
                            crate::service::spawn_stream_reader(
                                stdout,
                                logs_clone.clone(),
                                3000,
                                crate::service::LogKind::Stdout,
                            );
                        }
                        if let Some(stderr) = child.stderr.take() {
                            crate::service::spawn_stream_reader(
                                stderr,
                                logs_clone.clone(),
                                3000,
                                crate::service::LogKind::Stderr,
                            );
                        }

                        match child.wait() {
                            Ok(status) => {
                                if status.success() {
                                    crate::service::push_log_line(
                                        "[action completed successfully]",
                                        &logs_clone,
                                        3000,
                                        crate::service::LogKind::Launcher,
                                    );
                                } else {
                                    let code = status
                                        .code()
                                        .map(|c| c.to_string())
                                        .unwrap_or_else(|| "signal".to_string());
                                    crate::service::push_log_line(
                                        &format!("[action exited with error code {}]", code),
                                        &logs_clone,
                                        3000,
                                        crate::service::LogKind::Warning,
                                    );
                                }
                            }
                            Err(e) => {
                                crate::service::push_log_line(
                                    &format!("[action error] Failed while waiting: {}", e),
                                    &logs_clone,
                                    3000,
                                    crate::service::LogKind::Error,
                                );
                            }
                        }
                    }
                    Err(e) => {
                        let entry = crate::service::LogEntry::new(
                            format!("[action error] Failed to execute: {}", e),
                            crate::service::LogKind::Error,
                        );
                        if let Ok(mut l) = logs_clone.lock() {
                            l.push_back(entry);
                        }
                    }
                }
            });
        }
    }

    fn ensure_inline_session(
        &mut self,
        port: u16,
        name: &str,
        pid: Option<u32>,
        cmd_str: &str,
        cwd_str: &str,
    ) {
        if !self.inline_terminal_sessions.contains_key(&port) {
            let logs = Arc::new(Mutex::new(VecDeque::with_capacity(1000)));
            let pid_str = pid.map(|p| p.to_string()).unwrap_or_else(|| "unavailable".to_string());
            let entry_welcome = crate::service::LogEntry::new(
                format!("[console] Opened for {} (Port :{}, PID {})", name, port, pid_str),
                crate::service::LogKind::Launcher,
            );
            if let Ok(mut l) = logs.lock() {
                l.push_back(entry_welcome);
                if !cwd_str.is_empty() {
                    l.push_back(crate::service::LogEntry::new(
                        format!("[console] Command directory: {}", cwd_str),
                        crate::service::LogKind::Launcher,
                    ));
                }
                if !cmd_str.is_empty() {
                    l.push_back(crate::service::LogEntry::new(
                        format!("[console] Process command: {}", cmd_str),
                        crate::service::LogKind::Launcher,
                    ));
                }
            }

            self.inline_terminal_sessions.insert(
                port,
                ExternalTerminalSession {
                    logs,
                },
            );

            // Fetch initial journald or process output in background thread
            self.refresh_inline_logs(port, pid, cwd_str);
        }
    }

    fn refresh_inline_logs(&self, port: u16, pid: Option<u32>, cwd: &str) {
        if let Some(session) = self.inline_terminal_sessions.get(&port) {
            let logs_clone = session.logs.clone();
            let _ = cwd;
            std::thread::spawn(move || {
                if let Some(pid_val) = pid {
                    // 1. Try reading journalctl logs for this PID
                    let output = std::process::Command::new("journalctl")
                        .args(&["--no-pager", "-n", "35", &format!("_PID={}", pid_val)])
                        .output();

                    if let Ok(out) = output {
                        let text = String::from_utf8_lossy(&out.stdout);
                        let mut has_lines = false;
                        for line in text.lines() {
                            if !line.trim().is_empty() && !line.starts_with("-- No entries --") {
                                has_lines = true;
                                crate::service::push_log_line(line, &logs_clone, 1000, crate::service::LogKind::Stdout);
                            }
                        }
                        if has_lines {
                            return;
                        }
                    }

                    // 2. Check if /proc/<pid>/fd/1 is a log file
                    #[cfg(target_os = "linux")]
                    {
                        let fd1_path = format!("/proc/{}/fd/1", pid_val);
                        if let Ok(target) = std::fs::read_link(&fd1_path) {
                            if target.is_file() {
                                let output = std::process::Command::new("tail")
                                    .args(&["-n", "30", &target.to_string_lossy()])
                                    .output();
                                if let Ok(out) = output {
                                    let text = String::from_utf8_lossy(&out.stdout);
                                    for line in text.lines() {
                                        crate::service::push_log_line(line, &logs_clone, 1000, crate::service::LogKind::Stdout);
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }
    }

    fn run_inline_command(&self, port: u16, cmd: &str, cwd: &str) {
        if let Some(session) = self.inline_terminal_sessions.get(&port) {
            let logs_clone = session.logs.clone();
            let cmd_str = cmd.to_string();
            let cwd_owned = if cwd.is_empty() { ".".to_string() } else { cwd.to_string() };

            crate::service::push_log_line(&format!("> {}", cmd), &logs_clone, 1000, crate::service::LogKind::Stdin);

            std::thread::spawn(move || {
                #[cfg(target_os = "windows")]
                let mut c = std::process::Command::new("cmd.exe");
                #[cfg(target_os = "windows")]
                c.args(&["/C", &cmd_str]);

                #[cfg(not(target_os = "windows"))]
                let mut c = std::process::Command::new("sh");
                #[cfg(not(target_os = "windows"))]
                c.args(&["-c", &cmd_str]);

                c.current_dir(&cwd_owned);
                let empty_env = std::collections::HashMap::new();
                crate::service::prepare_environment(&mut c, &cwd_owned, &empty_env);

                c.stdout(std::process::Stdio::piped());
                c.stderr(std::process::Stdio::piped());

                match c.spawn() {
                    Ok(mut child) => {
                        if let Some(stdout) = child.stdout.take() {
                            crate::service::spawn_stream_reader(stdout, logs_clone.clone(), 1000, crate::service::LogKind::Stdout);
                        }
                        if let Some(stderr) = child.stderr.take() {
                            crate::service::spawn_stream_reader(stderr, logs_clone.clone(), 1000, crate::service::LogKind::Stderr);
                        }
                        let _ = child.wait();
                    }
                    Err(e) => {
                        crate::service::push_log_line(&format!("[error] Failed to execute: {}", e), &logs_clone, 1000, crate::service::LogKind::Error);
                    }
                }
            });
        }
    }

    fn clear_inline_logs(&self, port: u16) {
        if let Some(session) = self.inline_terminal_sessions.get(&port) {
            if let Ok(mut l) = session.logs.lock() {
                l.clear();
            }
        }
    }


    fn collect_palette_actions(&self) -> Vec<(String, PaletteAction)> {
        let mut items = Vec::new();

        // 1. Server Start / Stop / Restart & View Logs
        for s in &self.services {
            let grp_tag = if s.config.group.is_empty() {
                String::new()
            } else {
                format!(" [{}]", s.config.group)
            };

            if s.state == ServiceState::Running || s.state == ServiceState::Starting {
                items.push((
                    format!("{}  Stop {}{}", crate::lucide::STOP, s.config.name, grp_tag),
                    PaletteAction::StopServer(s.config.key.clone()),
                ));
                items.push((
                    format!("{}  Restart {}{}", crate::lucide::REFRESH_CW, s.config.name, grp_tag),
                    PaletteAction::RestartServer(s.config.key.clone()),
                ));
            } else {
                items.push((
                    format!("{}  Start {}{}", crate::lucide::PLAY, s.config.name, grp_tag),
                    PaletteAction::StartServer(s.config.key.clone()),
                ));
            }
            items.push((
                format!("{}  View logs: {}", crate::lucide::SCRIPT, s.config.name),
                PaletteAction::SelectTab(s.config.key.clone()),
            ));

            // Custom actions
            for act in &s.config.actions {
                items.push((
                    format!("{}  {} [{}]", crate::lucide::ZAP, act.label, s.config.name),
                    PaletteAction::RunAction {
                        key: s.config.key.clone(),
                        command: act.command.clone(),
                    },
                ));
            }

            // Links
            for lnk in &s.config.links {
                items.push((
                    format!("{}  Open {} ({})", crate::lucide::LINK, lnk.label, lnk.url),
                    PaletteAction::OpenLink(lnk.url.clone()),
                ));
            }
        }

        // 2. Group actions
        let mut groups = BTreeSet::new();
        for s in &self.services {
            groups.insert(s.config.group.clone());
        }
        for g in groups {
            items.push((
                format!("{}  Start Group: {}", crate::lucide::PLAY, g),
                PaletteAction::StartGroup(g.clone()),
            ));
            items.push((
                format!("{}  Stop Group: {}", crate::lucide::STOP, g),
                PaletteAction::StopGroup(g.clone()),
            ));
        }

        // 3. Global actions
        items.push((
            format!("{}  Start All Servers", crate::lucide::PLAY),
            PaletteAction::StartAll,
        ));
        items.push((
            format!("{}  Stop All Servers", crate::lucide::STOP),
            PaletteAction::StopAll,
        ));

        // 4. Color Themes
        for thm in ThemeMode::all() {
            items.push((
                format!("{}  Switch Theme: {} {}", crate::lucide::PALETTE, thm.emoji(), thm.name()),
                PaletteAction::SetTheme(thm.id_str().to_string()),
            ));
        }

        // 5. Autostart Actions
        let app_auto = crate::autostart::is_app_autostart_enabled();
        items.push((
            format!("{}  App Autostart on System Login: {}", crate::lucide::ROCKET, if app_auto { "Disable" } else { "Enable" }),
            PaletteAction::ToggleAppAutostart,
        ));

        for s in &self.services {
            items.push((
                format!(
                    "{}  Server Autostart on Launch: {} ({})",
                    crate::lucide::ROCKET,
                    s.config.name,
                    if s.config.autostart { "Disable" } else { "Enable" }
                ),
                PaletteAction::ToggleServerAutostart(s.config.key.clone()),
            ));
        }

        items
    }

    fn handle_global_shortcuts(&mut self, ctx: &Context) {
        // Ctrl+P / Ctrl+K: Quick Launch & Command Palette
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::P))
            || ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::P))
            || ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::K))
            || ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::K))
        {
            self.palette_modal.open = !self.palette_modal.open;
            self.palette_modal.query.clear();
            self.palette_modal.selected_idx = 0;
        }

        // Ctrl+F: Search in Overview or Find in server logs
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F))
            || ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::F))
        {
            if self.active_tab == "overview" {
                self.focus_overview_search = true;
            } else {
                let current = self.search_open.get(&self.active_tab).copied().unwrap_or(false);
                self.search_open.insert(self.active_tab.clone(), !current);
            }
        }

        // Ctrl+A: Start all
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::A))
            || ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::A))
        {
            self.start_all();
        }

        // Ctrl+X: Stop all
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::X))
            || ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::X))
        {
            self.stop_all();
        }

        // Ctrl+,: Settings
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Comma))
            || ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Comma))
        {
            self.settings_modal_open = !self.settings_modal_open;
        }
    }




    fn trigger_remote_services_list(&self, host: &SshRemoteHost, user_mode: bool) {
        let Some(lifetime) = self.remote_host_lifetime(host) else { return };
        let key = format!("{}:{}", host.id, if user_mode { "user" } else { "system" });
        {
            let mut in_flight = self.remote_services_in_flight.lock().unwrap();
            if in_flight.contains(&key) {
                return;
            }
            in_flight.insert(key.clone());
        }

        let host_clone = host.clone();
        let key_clone = key.clone();
        let services_arc = self.remote_services.clone();
        let in_flight_arc = self.remote_services_in_flight.clone();
        let errors_arc = self.remote_services_errors.clone();
        let lifetimes_arc = self.remote_host_lifetimes.clone();

        std::thread::spawn(move || {
            let result = list_remote_services(&host_clone, user_mode);
            let lifetimes = lifetimes_arc.lock().unwrap();
            if !remote_host_lifetime_matches(&lifetimes, &host_clone.id, &lifetime) { return; }
            match result {
                Ok(services) => {
                    if let Ok(mut map) = services_arc.lock() {
                        map.insert(key_clone.clone(), services);
                    }
                    if let Ok(mut errs) = errors_arc.lock() {
                        errs.remove(&key_clone);
                    }
                }
                Err(err) => {
                    if let Ok(mut errs) = errors_arc.lock() {
                        errs.insert(key_clone.clone(), err);
                    }
                    if let Ok(mut map) = services_arc.lock() {
                        map.entry(key_clone.clone()).or_insert_with(Vec::new);
                    }
                }
            }
            if let Ok(mut in_flight) = in_flight_arc.lock() {
                in_flight.remove(&key_clone);
            }
        });
    }

    pub fn trigger_remote_journal_fetch(&self, host: &SshRemoteHost, unit: &str, user_mode: bool) {
        self.queue_systemd_action(PendingSudoAction::ReadJournal { unit: unit.to_string(), user_mode }, Some(host), None);
    }

    pub fn trigger_local_services_list(&self, user_mode: bool) {
        {
            let mut in_flight = self.local_services_in_flight.lock().unwrap();
            if in_flight.contains(&user_mode) {
                return;
            }
            in_flight.insert(user_mode);
        }

        let cache_arc = self.local_services_cache.clone();
        let in_flight_arc = self.local_services_in_flight.clone();

        let results = self.systemd_action_results.clone();
        std::thread::spawn(move || {
            match list_local_services(user_mode) {
                Ok(services) => { cache_arc.lock().unwrap().insert(user_mode, services); }
                Err(error) => { results.lock().unwrap().push_back(SystemdActionResult { key: String::new(), action: None, host_id: None, host_lifetime: None, from_popup: false, result: Err(error) }); }
            }
            if let Ok(mut in_flight) = in_flight_arc.lock() {
                in_flight.remove(&user_mode);
            }
        });
    }

    pub fn trigger_local_journal_fetch(&self, unit: &str, user_mode: bool) {
        self.queue_systemd_action(PendingSudoAction::ReadJournal { unit: unit.to_string(), user_mode }, None, None);
    }

    pub fn trigger_systemd_unit_files_fetch(&self, unit: &str, user_mode: bool, host_opt: Option<&SshRemoteHost>) {
        let host_lifetime = host_opt.and_then(|host| self.remote_host_lifetime(host));
        if host_opt.is_some() && host_lifetime.is_none() { return; }
        let cache_key = format!("{}:{}:{}", host_opt.map(|h| h.id.as_str()).unwrap_or("local"), unit, user_mode);
        {
            let mut in_flight = self.systemd_files_in_flight.lock().unwrap();
            if in_flight.contains(&cache_key) {
                return;
            }
            in_flight.insert(cache_key.clone());
        }

        let unit_clone = unit.to_string();
        let host_clone = host_opt.cloned();
        let cache_key_clone = cache_key.clone();
        let files_arc = self.systemd_files_cache.clone();
        let in_flight_arc = self.systemd_files_in_flight.clone();
        let lifetimes_arc = self.remote_host_lifetimes.clone();

        let results = self.systemd_action_results.clone();
        std::thread::spawn(move || {
            let info = if let Some(host) = &host_clone {
                crate::systemd::get_remote_unit_files(host, &unit_clone, user_mode)
            } else {
                crate::systemd::get_local_unit_files(&unit_clone, user_mode)
            };
            let info = match info {
                Ok(value) => value,
                Err(error) => {
                    results.lock().unwrap().push_back(SystemdActionResult { key: String::new(), action: None, host_id: host_clone.as_ref().map(|host| host.id.clone()), host_lifetime: host_lifetime.clone(), from_popup: false, result: Err(error) });
                    Default::default()
                }
            };
            let lifetimes = lifetimes_arc.lock().unwrap();
            if host_clone.as_ref().is_some_and(|host| !lifetimes.get(&host.id).is_some_and(|current| host_lifetime.as_ref().is_some_and(|token| Arc::ptr_eq(current, token)))) { return; }
            if let Ok(mut map) = files_arc.lock() {
                map.insert(cache_key_clone.clone(), (info, Instant::now()));
            }
            if let Ok(mut in_flight) = in_flight_arc.lock() {
                in_flight.remove(&cache_key_clone);
            }
        });
    }

    pub fn trigger_systemd_file_read(&self, path: &str, user_mode: bool, host_opt: Option<&SshRemoteHost>) {
        self.queue_systemd_action(PendingSudoAction::ReadUnitFile { path: path.to_string(), user_mode }, host_opt, None);
    }

    pub fn trigger_systemd_unit_dependencies_fetch(&self, unit: &str, user_mode: bool, host_opt: Option<&SshRemoteHost>) {
        let host_lifetime = host_opt.and_then(|host| self.remote_host_lifetime(host));
        if host_opt.is_some() && host_lifetime.is_none() { return; }
        let cache_key = format!("{}:{}:{}", host_opt.map(|h| h.id.as_str()).unwrap_or("local"), unit, user_mode);
        {
            let mut in_flight = self.systemd_deps_in_flight.lock().unwrap();
            if in_flight.contains(&cache_key) {
                return;
            }
            in_flight.insert(cache_key.clone());
        }

        let unit_clone = unit.to_string();
        let host_clone = host_opt.cloned();
        let cache_key_clone = cache_key.clone();
        let deps_arc = self.systemd_deps_cache.clone();
        let in_flight_arc = self.systemd_deps_in_flight.clone();
        let lifetimes_arc = self.remote_host_lifetimes.clone();

        let results = self.systemd_action_results.clone();
        std::thread::spawn(move || {
            let deps = if let Some(host) = &host_clone {
                crate::systemd::get_remote_unit_dependencies(host, &unit_clone, user_mode)
            } else {
                crate::systemd::get_local_unit_dependencies(&unit_clone, user_mode)
            };
            let deps = match deps {
                Ok(value) => value,
                Err(error) => {
                    results.lock().unwrap().push_back(SystemdActionResult { key: String::new(), action: None, host_id: host_clone.as_ref().map(|host| host.id.clone()), host_lifetime: host_lifetime.clone(), from_popup: false, result: Err(error) });
                    Default::default()
                }
            };
            let lifetimes = lifetimes_arc.lock().unwrap();
            if host_clone.as_ref().is_some_and(|host| !lifetimes.get(&host.id).is_some_and(|current| host_lifetime.as_ref().is_some_and(|token| Arc::ptr_eq(current, token)))) { return; }
            if let Ok(mut map) = deps_arc.lock() {
                map.insert(cache_key_clone.clone(), (deps, Instant::now()));
            }
            if let Ok(mut in_flight) = in_flight_arc.lock() {
                in_flight.remove(&cache_key_clone);
            }
        });
    }

    pub fn select_systemd_unit(&mut self, unit: &str, user_mode: bool, host_opt: Option<&SshRemoteHost>) {
        if self.systemd_file_content != self.systemd_file_original {
            self.systemd_file_status_msg = Some(("Save or revert the current unit file before selecting another unit.".into(), true));
            return;
        }
        self.selected_service = Some(unit.to_string());
        self.queue_systemd_action(PendingSudoAction::InspectUnit { unit: unit.to_string(), user_mode }, host_opt, None);
        self.service_action_msg = None;
        self.systemd_selected_file = None;
        self.systemd_file_content.clear();
        self.systemd_file_original.clear();
        self.systemd_file_status_msg = None;
        self.last_journal_refresh = Instant::now();

        // 1. Asynchronously fetch journalctl logs
        if let Some(host) = host_opt {
            self.trigger_remote_journal_fetch(host, unit, user_mode);
        } else {
            self.trigger_local_journal_fetch(unit, user_mode);
        }

        // 2. Pre-fetch unit files & dependencies in the background for zero-lag tab switches!
        self.trigger_systemd_unit_files_fetch(unit, user_mode, host_opt);
        self.trigger_systemd_unit_dependencies_fetch(unit, user_mode, host_opt);
    }

    fn get_or_query_service_enabled(&mut self, unit: &str, user_mode: bool, host_opt: Option<&SshRemoteHost>) -> Option<bool> {
        let key = format!("{}:{}:{}", host_opt.map(|host| host.id.as_str()).unwrap_or("local"), unit, if user_mode { "user" } else { "system" });
        let cached = self.service_enabled_cache.get(&key).copied();
        if cached.is_none_or(|(_, time)| time.elapsed() >= Duration::from_secs(6)) {
            self.queue_systemd_action(PendingSudoAction::QueryEnabled { unit: unit.to_string(), user_mode }, host_opt, None);
        }
        cached.map(|(enabled, _)| enabled)
    }









}

// ─────────────────────────────────────────────────────────────────────────────
// Helper rendering functions for crisp UI elements
// ─────────────────────────────────────────────────────────────────────────────

fn draw_status_dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 6.0, color.gamma_multiply(0.25));
    ui.painter().circle_filled(rect.center(), 3.5, color);
}

#[allow(dead_code)]
fn render_m3_status_chip(ui: &mut Ui, label: &str, dot_color: Color32, container_color: Color32, text_color: Color32) {
    Frame::none()
        .fill(container_color)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(999.0))
        .inner_margin(egui::Margin::symmetric(10.0, 3.5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                let (rect, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 3.5, dot_color);
                ui.label(
                    RichText::new(label)
                        .size(11.0)
                        .strong()
                        .color(text_color),
                );
            });
        });
}

#[allow(dead_code)]
fn render_group_badge(ui: &mut Ui, text: &str) {
    let visuals = ui.visuals();
    let bg = visuals.faint_bg_color;
    let text_col = visuals.selection.bg_fill;
    Frame::none()
        .fill(bg)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(999.0))
        .inner_margin(egui::Margin::symmetric(8.0, 2.5))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .size(10.5)
                    .color(text_col)
                    .strong(),
            );
        });
}

/// Renders the official Server Launcher brand logo with subpixel vector precision.
/// Renders with mathematical crispness at any DPI scale factor (1x, 1.25x, 1.5x, 2x, 4x)
/// with zero pixelation, texture aliasing, or edge artifacts.
pub fn render_app_logo(ui: &mut Ui, size: f32, bg_mask: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return resp;
    }

    let painter = ui.painter();
    let min = rect.min;
    let s = rect.width();

    // 1. Base Squircle Container
    // Deep nocturnal slate/indigo dark body
    let card_rounding = Rounding::same(s * 0.22);
    painter.rect_filled(
        rect,
        card_rounding,
        Color32::from_rgb(17, 22, 44),
    );
    // Subtle inner depth fill
    painter.rect_filled(
        egui::Rect::from_min_max(
            rect.min + egui::vec2(1.0, 1.0),
            rect.max - egui::vec2(1.0, 1.0),
        ),
        Rounding::same(s * 0.20),
        Color32::from_rgb(20, 26, 52),
    );
    // Crisp subtle border stroke
    painter.rect_stroke(
        rect,
        card_rounding,
        Stroke::new(1.0_f32, Color32::from_rgb(33, 44, 76)),
    );

    // 2. Three Server Blade Slots
    let blade_pad_x = s * 0.14;
    let blade_w = s * 0.72;
    let blade_y_start = s * 0.16;
    let total_blade_span = s * 0.60;
    let gap = s * 0.045;
    let blade_h = (total_blade_span - 2.0 * gap) / 3.0;
    let blade_rounding = Rounding::same((blade_h * 0.35).max(1.5));

    for i in 0..3 {
        let b_top = min.y + blade_y_start + i as f32 * (blade_h + gap);
        let b_rect = egui::Rect::from_min_size(
            egui::pos2(min.x + blade_pad_x, b_top),
            egui::vec2(blade_w, blade_h),
        );

        // Blade chassis card
        painter.rect_filled(
            b_rect,
            blade_rounding,
            Color32::from_rgb(31, 40, 64),
        );
        painter.rect_stroke(
            b_rect,
            blade_rounding,
            Stroke::new(0.5_f32, Color32::from_rgb(46, 59, 90)),
        );

        let cy = b_rect.center().y;
        let led_radius = (blade_h * 0.22).max(1.1);

        // Green status LED (Left dot)
        let led1_x = b_rect.min.x + blade_h * 0.48;
        painter.circle_filled(
            egui::pos2(led1_x, cy),
            led_radius,
            Color32::from_rgb(16, 185, 129),
        );

        // Second status LED
        let led2_x = led1_x + led_radius * 2.6;
        let led2_col = if i < 2 {
            Color32::from_rgb(56, 189, 248)
        } else {
            Color32::from_rgb(100, 116, 139)
        };
        painter.circle_filled(
            egui::pos2(led2_x, cy),
            led_radius,
            led2_col,
        );

        // Activity vent lines (Right side)
        let line_x_start = led2_x + led_radius * 2.8;
        let line_w1 = (b_rect.max.x - line_x_start - blade_h * 0.3).max(2.0);
        let line_h = (blade_h * 0.16).max(1.0);
        let line_y = cy;

        // Top line
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(line_x_start, line_y - line_h * 0.5),
                egui::vec2(line_w1, line_h),
            ),
            Rounding::same(line_h * 0.5),
            Color32::from_rgba_premultiplied(148, 163, 184, 90),
        );
    }

    // 3. Floating Play Badge (Bottom-Right)
    let badge_center = egui::pos2(min.x + s * 0.72, min.y + s * 0.72);
    let badge_radius = s * 0.22;
    let cutout_radius = badge_radius + (s * 0.045).max(1.2);

    // Dark cutout rim (cuts cleanly through squircle and blades, matching header background)
    painter.circle_filled(
        badge_center,
        cutout_radius,
        bg_mask,
    );

    // Vibrant Cyan / Azure Play Circle
    painter.circle_filled(
        badge_center,
        badge_radius,
        Color32::from_rgb(14, 165, 233),
    );

    // Inner subtle gradient ring / depth stroke
    painter.circle_stroke(
        badge_center,
        badge_radius,
        Stroke::new(0.5_f32, Color32::from_rgb(56, 189, 248)),
    );

    // Sharp, centered white equilateral play triangle
    let tri_size = badge_radius * 0.72;
    let tri_cx = badge_center.x + tri_size * 0.10; // optical center shift to the right
    let tri_cy = badge_center.y;

    let p1 = egui::pos2(tri_cx - tri_size * 0.42, tri_cy - tri_size * 0.50);
    let p2 = egui::pos2(tri_cx - tri_size * 0.42, tri_cy + tri_size * 0.50);
    let p3 = egui::pos2(tri_cx + tri_size * 0.55, tri_cy);

    painter.add(egui::epaint::PathShape::convex_polygon(
        vec![p1, p2, p3],
        Color32::WHITE,
        Stroke::NONE,
    ));

    resp
}

#[allow(dead_code)]
fn render_metric_pill(ui: &mut Ui, icon: &str, label: &str, text_color: Color32) {
    let bg_color = ui.visuals().widgets.inactive.bg_fill;
    render_metric_pill_custom(ui, icon, label, text_color, bg_color);
}

#[allow(dead_code)]
fn render_metric_pill_custom(ui: &mut Ui, icon: &str, label: &str, text_color: Color32, bg_color: Color32) {
    Frame::none()
        .fill(bg_color)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(999.0))
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if !icon.is_empty() {
                    ui.label(RichText::new(icon).size(10.5));
                }
                ui.label(RichText::new(label).size(11.0).monospace().color(text_color).strong());
            });
        });
}

fn read_proc_cmd_and_cwd(pid: u32) -> (Option<String>, Option<String>) {
    #[cfg(target_os = "linux")]
    {
        let cmd = std::fs::read(format!("/proc/{}/cmdline", pid)).ok().and_then(|bytes| {
            let args: Vec<String> = bytes
                .split(|&b| b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).to_string())
                .collect();
            if args.is_empty() {
                None
            } else {
                Some(args.join(" "))
            }
        });
        let cwd = std::fs::read_link(format!("/proc/{}/cwd", pid))
            .ok()
            .map(|p| p.to_string_lossy().to_string());
        (cmd, cwd)
    }
    #[cfg(not(target_os = "linux"))]
    {
        (None, None)
    }
}

impl eframe::App for LauncherApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.palette().bg_header.to_normalized_gamma_f32()
    }


    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let refresh_hosts = ctx.data_mut(|data| data.remove_temp::<BTreeSet<String>>(egui::Id::new(REMOTE_LISTENER_REFRESH)));
        if let Some(refresh_hosts) = refresh_hosts {
            for host in self.ssh_hosts.iter().filter(|host| refresh_hosts.contains(&host.id)) {
                self.schedule_remote_scan(host, 1500, ctx.clone());
            }
        }
        self.apply_systemd_results();
        // Sync background sensors snapshot if newly available
        if let Ok(shared) = self.sensors_shared_snapshot.try_lock() {
            if shared.timestamp != self.sensors_snapshot.timestamp && shared.timestamp.is_some() {
                self.sensors_snapshot = Arc::clone(&shared);
            }
        }

        crate::updater::close_for_update(ctx);

        // ── Close Confirmation ──────────────────────────────────────────────
        let any_running = self.services.iter().any(|s| {
            s.state == ServiceState::Running || s.state == ServiceState::Starting || s.restart_at.is_some()
        });

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.close_confirmed {
                // User already approved exit - let egui close
            } else if any_running || self.close_confirm_open {
                // Block close and show dialog
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_confirm_open = true;
            } else if self.remote_commands_active() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_confirmed = true;
                self.close_waiting_for_ssh_commands = true;
                self.cancel_all_remote_commands();
            } else {
                // No in-app SSH child needs cleanup before the window closes.
            }
        }

        if self.close_confirm_open {
            let running_info: Vec<(String, Option<u16>, Option<u32>)> = self.services.iter()
                .filter(|s| s.state == ServiceState::Running || s.state == ServiceState::Starting)
                .map(|s| (s.config.name.clone(), if s.config.port > 0 { Some(s.config.port) } else { None }, s.pid))
                .collect();
            let p = self.palette();
            let choice = render_close_confirmation_modal(ctx, &mut self.close_confirm_open, &running_info, &p);
            match choice {
                CloseConfirmChoice::StopAndExit => {
                    self.cancel_all_remote_commands();
                    self.close_confirmed = true;
                    self.close_confirm_open = false;
                    self.stop_all();
                    self.close_waiting_for_ssh_commands = self.remote_commands_active();
                    if !self.close_waiting_for_ssh_commands {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                CloseConfirmChoice::KeepRunningAndExit => {
                    self.cancel_all_remote_commands();
                    self.close_confirmed = true;
                    self.close_confirm_open = false;
                    self.close_waiting_for_ssh_commands = self.remote_commands_active();
                    if !self.close_waiting_for_ssh_commands {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                CloseConfirmChoice::Cancel => {
                    crate::updater::cancel_install(ctx);
                    self.close_confirm_open = false;
                }
                CloseConfirmChoice::None => {}
            }
        }

        if self.close_waiting_for_ssh_commands {
            if self.remote_commands_active() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            } else {
                self.close_waiting_for_ssh_commands = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // Sample all process CPU counters once per interval; every consumer reads this snapshot.
        let show_other_metrics = self.show_other_process_metrics && self.active_tab == "overview";
        if (any_running || show_other_metrics) && self.last_poll.elapsed() >= std::time::Duration::from_millis(2000) {
            crate::service::refresh_process_metrics(&mut self.sys);
            for s in &mut self.services {
                s.poll_status(&self.sys);
            }
            self.last_poll = Instant::now();
        }

        // Read the shared sample for other listening processes when metrics are enabled (every 2s)
        if show_other_metrics {
            ctx.request_repaint_after(std::time::Duration::from_millis(2000));
            if self.other_metrics.is_empty() || self.last_other_poll.elapsed() >= std::time::Duration::from_millis(2000) {
                let listeners = self.scanner.get_listeners();
                let pids: Vec<u32> = listeners.iter().filter_map(|l| l.pid).collect();
                if !pids.is_empty() {
                    for &pid in &pids {
                        let s_pid = sysinfo::Pid::from_u32(pid);
                        let cpu = self.sys.process(s_pid).map(|p| p.cpu_usage()).unwrap_or(0.0);
                        #[cfg(target_os = "linux")]
                        let mem = (crate::service::get_proc_rss_kb_linux(pid) as f32) / 1024.0;
                        #[cfg(not(target_os = "linux"))]
                        let mem = self.sys.process(s_pid).map(|p| (p.memory() as f32) / (1024.0 * 1024.0)).unwrap_or(0.0);
                        self.other_metrics.insert(pid, (cpu, mem));
                    }
                }
                self.last_other_poll = Instant::now();
            }
        } else if !self.show_other_process_metrics && !self.other_metrics.is_empty() {
            self.other_metrics.clear();
        }

        // GTK4 / System theme: live-follow desktop color scheme + accent changes
        if self.theme == ThemeMode::Gtk4 {
            let p = self.palette();
            if self.last_applied_palette.as_ref() != Some(&p) {
                ctx.set_visuals(p.egui_visuals());
                self.last_applied_palette = Some(p);
            }
        }

        let visible_ssh_host = (self.active_sidebar_tab == SidebarTab::Servers)
            .then(|| self.ssh_hosts.iter().find(|host| host.id == self.selected_machine))
            .flatten().map(|host| host.id.clone());
        let view_id = egui::Id::new("ssh_listener_view");
        let listener_dialog_open = ctx.data_mut(|data| {
            let previous = data.get_temp::<String>(view_id);
            if previous != visible_ssh_host {
                if let Some(previous) = previous {
                    let id = egui::Id::new(("ssh_listener_confirmation", previous));
                    data.remove::<(crate::remote::RemoteListener, bool)>(id);
                    data.remove::<bool>(id.with("focused"));
                }
                data.remove::<String>(view_id);
                if let Some(host_id) = &visible_ssh_host {
                    data.insert_temp(view_id, host_id.clone());
                }
            }
            visible_ssh_host.as_ref().is_some_and(|id| {
                data.get_temp::<(crate::remote::RemoteListener, bool)>(
                    egui::Id::new(("ssh_listener_confirmation", id))
                ).is_some()
            })
        });
        if !listener_dialog_open {
            self.handle_global_shortcuts(ctx);
        }
        let p = self.palette();

        // ═════════════════════════════════════════════════════════════════════
        // 0. CUSTOM TITLE BAR
        // ═════════════════════════════════════════════════════════════════════
        egui::TopBottomPanel::top("custom_titlebar")
            .show_separator_line(false)
            .exact_height(24.0)
            .frame(
                Frame::none()
                    .fill(p.bg_header)
                    .inner_margin(egui::Margin {
                        left: 10.0,
                        right: 0.0,
                        top: 0.0,
                        bottom: 0.0,
                    }),
            )
            .show(ctx, |ui| {
                let maximized = ctx.input(|i| i.viewport().maximized).unwrap_or(false);

                // Entire titlebar area is draggable to move window; double-click toggles maximize
                let titlebar_drag = ui.interact(
                    ui.max_rect(),
                    ui.id().with("custom_titlebar_drag"),
                    egui::Sense::click_and_drag(),
                );
                if titlebar_drag.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if titlebar_drag.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    // Window caption stays neutral; server state is shown in the server tabs.
                    ui.label(
                        RichText::new("Server Launcher")
                            .size(11.5)
                            .color(p.text_muted),
                    );

                    // Right window controls: only Minimize, Maximize, and Close
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;

                        // 1. Close (far right)
                        if render_window_button(ui, WindowButtonKind::Close, &p) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }

                        // 2. Maximize / Restore (middle)
                        if render_window_button(ui, WindowButtonKind::Maximize(maximized), &p) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                        }

                        // 3. Minimize (left)
                        if render_window_button(ui, WindowButtonKind::Minimize, &p) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                });
            });


        // ═════════════════════════════════════════════════════════════════════
        // TOP HEADER BAR (App Toolbar: Logo, search, action buttons)
        // ═════════════════════════════════════════════════════════════════════
        let header_fill = if p.bg_header == p.bg_main { p.bg_card } else { p.bg_header };
        egui::TopBottomPanel::top("top_header")
            .frame(
                Frame::none()
                    .fill(header_fill)
                    .inner_margin(egui::Margin {
                        left: 14.0,
                        right: 14.0,
                        top: 6.0,
                        bottom: 6.0,
                    }),
            )
            .show(ctx, |ui| {
                if listener_dialog_open { ui.disable(); }
                let header_center = ui.available_rect_before_wrap().center().x;
                let mut tabs_bounds = None;
                let header = ui.horizontal(|ui| {
                    let remote_host_opt = if self.selected_machine != "local" {
                        self.ssh_hosts.iter().find(|h| h.id == self.selected_machine).cloned()
                    } else {
                        None
                    };

                    // Left: App Icon + Server Launcher / Host title & status line
                    render_app_logo(ui, 30.0, header_fill);
                    ui.add_space(8.0);

                    if let Some(ref host) = remote_host_opt {
                        let (status, is_connecting) = {
                            let s = self.remote_connection_status.lock().unwrap().get(&host.id).cloned();
                            let c = self.remote_connecting.lock().unwrap().contains(&host.id);
                            (s, c)
                        };

                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                let name_width = ui.available_width().min(220.0);
                                let edit = ui.scope(|ui| {
                                    ui.set_max_width(name_width);
                                    ssh_identity_link(ui, RichText::new(&host.name).size(16.5).strong().color(p.text_primary),
                                        &host.name, p.bg_button_hover)
                                }).inner;
                                if edit {
                                    self.ssh_host_modal.open_edit(host);
                                }
                                ui.add_space(5.0);
                                if is_connecting {
                                    Frame::none()
                                        .fill(p.warning_subtle)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new("○ Connecting...").size(10.0).color(p.warning));
                                        });
                                } else if let Some(Ok(ref info)) = status {
                                    Frame::none()
                                        .fill(p.success_subtle)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                        .show(ui, |ui| {
                                            let badge_lbl = ui.label(
                                                RichText::new("● Online").size(10.0).strong().color(p.success),
                                            );
                                            badge_lbl.hover_tip(format!("Online: {}", info));
                                        });
                                } else if let Some(Err(ref err)) = status {
                                    Frame::none()
                                        .fill(p.danger_subtle)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                        .show(ui, |ui| {
                                            let err_chars = err.chars().count();
                                            let err_short = if err_chars > 24 {
                                                format!("{}...", err.chars().take(21).collect::<String>())
                                            } else {
                                                err.clone()
                                            };
                                            let badge_lbl = ui.label(
                                                RichText::new(format!("● Error: {}", err_short)).size(10.0).color(p.danger),
                                            );
                                            badge_lbl.hover_tip(format!("Connection Error:\n{}", err));
                                        });
                                } else {
                                    Frame::none()
                                        .fill(p.bg_subtle)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new("○ Idle").size(10.0).color(p.text_muted));
                                        });
                                }
                            });
                            ui.add_space(1.0);
                            let target_desc = format!(
                                "ssh://{}@{}:{} {}",
                                host.user,
                                host.host,
                                host.port,
                                if !host.remote_cwd.is_empty() {
                                    format!("(cwd: {})", host.remote_cwd)
                                } else {
                                    String::new()
                                }
                            );
                            ui.scope(|ui| {
                                ui.spacing_mut().interact_size.y = 0.0;
                                ui.spacing_mut().button_padding.y = 0.0;
                                ui.horizontal(|ui| {
                                    if ssh_identity_link(ui, RichText::new(&target_desc).size(11.5).color(p.text_muted),
                                        &target_desc, p.bg_button_hover)
                                    {
                                        self.ssh_host_modal.open_edit(host);
                                    }
                                    let reconnect = egui::Button::new(RichText::new(crate::lucide::REFRESH_CW).size(10.0))
                                        .small()
                                        .frame(false);
                                    if ui.add(reconnect).hover_tip("Reconnect SSH").clicked() {
                                        self.test_remote_host(host);
                                        self.trigger_remote_scan(host);
                                    }
                                });
                            });
                        });
                    } else {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("Server Launcher")
                                    .size(16.5)
                                    .strong()
                                    .color(p.text_primary),
                            );
                            ui.add_space(1.0);
                            ui.label(
                                RichText::new(format!(
                                    "This PC · {} · {} servers",
                                    self.lan_ip,
                                    self.services.len()
                                ))
                                .size(11.5)
                                .color(p.text_muted),
                            );

                            if let Some((ref msg, is_err, time)) = self.import_status_msg {
                                if time.elapsed() < Duration::from_secs(6) {
                                    ui.add_space(2.0);
                                    let color = if is_err { p.danger } else { Color32::from_rgb(16, 185, 129) };
                                    Frame::none()
                                        .fill(p.bg_subtle)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(msg).size(10.5).color(color));
                                        });
                                } else {
                                    self.import_status_msg = None;
                                }
                            }
                        });
                    }

                    let left_edge = ui.cursor().left() + 8.0;

                    // Right: Actions buttons in modern pill layout
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.spacing_mut().button_padding = egui::vec2(6.0, 2.0);

                        // Settings uses the same height as the adjacent actions.
                        let btn_settings = egui::Button::new(
                            RichText::new(crate::lucide::SETTINGS).size(10.5).color(p.text_secondary),
                        )
                        .fill(p.bg_button)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(6.0))
                        .min_size(egui::vec2(22.0, 22.0));
                        if ui.add(btn_settings).hover_tip("Settings & Preferences (Ctrl+,)").clicked() {
                            self.settings_modal_open = true;
                        }

                        if let Some(ref host) = remote_host_opt {
                            // REMOTE SSH PC CONTROLS (Moved up here)
                            let btn_ext = egui::Button::new(
                                RichText::new(format!("{} Terminal", crate::lucide::EXTERNAL_LINK))
                                    .size(10.5)
                                    .strong()
                                    .color(p.info),
                            )
                            .fill(p.bg_button)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(76.0, 22.0));
                            if ui.add(btn_ext).hover_tip("Open desktop terminal connected via SSH").clicked() {
                                let _ = launch_external_ssh_terminal(host);
                            }

                            // Remote Host Controls: Shut Down & Restart Device
                            // In right-to-left layout: adding shutdown first then reboot renders: [Restart Device] [Shut Down]
                            let btn_shutdown = egui::Button::new(
                                RichText::new(format!("{} Shut Down", crate::lucide::POWER))
                                    .size(10.5)
                                    .strong()
                                    .color(p.danger),
                            )
                            .fill(p.bg_button)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(86.0, 22.0));
                            if ui.add(btn_shutdown).hover_tip("Shut down remote device (requires confirmation & sudo password)").clicked() {
                                self.remote_shutdown_modal.open_for_host(host);
                            }

                            let btn_reboot = egui::Button::new(
                                RichText::new(format!("{} Restart", crate::lucide::REFRESH_CW))
                                    .size(10.5)
                                    .strong()
                                    .color(p.warning),
                            )
                            .fill(p.bg_button)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(0.0, 22.0));
                            if ui.add(btn_reboot).hover_tip("Restart remote device (requires confirmation & sudo password)").clicked() {
                                self.remote_reboot_modal.open_for_host(host);
                            }
                        } else {
                            // Local actions share one height; shortcuts live in tooltips.
                            let add = egui::Button::new(RichText::new(format!("{} Add Server", crate::lucide::PLUS))
                                .size(11.0).strong().color(p.accent_text))
                                .fill(p.accent).stroke(Stroke::NONE).rounding(Rounding::same(6.0))
                                .min_size(egui::vec2(90.0, 22.0));
                            if ui.add(add).hover_tip("Configure a new server").clicked() {
                                self.add_edit_modal.open_new();
                            }
                            let compact_actions = ui.available_width() < 320.0;
                            let quick_label = if compact_actions { crate::lucide::ZAP.to_string() } else { format!("{} Quick Launch", crate::lucide::ZAP) };
                            let quick = egui::Button::new(RichText::new(quick_label)
                                .size(11.0).color(p.text_secondary))
                                .fill(p.bg_button).stroke(Stroke::NONE).rounding(Rounding::same(6.0))
                                .min_size(egui::vec2(if compact_actions { 22.0 } else { 92.0 }, 22.0));
                            if ui.add(quick).hover_tip("Quick Launch · Ctrl+P or Ctrl+K").clicked() {
                                self.palette_modal.open = true;
                                self.palette_modal.query.clear();
                            }

                        }
                        if remote_host_opt.is_some() || self.selected_machine == "local" {
                            tabs_bounds = Some((left_edge, ui.cursor().right() - 8.0, remote_host_opt.is_some()));
                        }
                    });
                });
                if let Some((left, right, remote)) = tabs_bounds {
                    let tabs_width = 486.0;
                    let x = header_center - tabs_width * 0.5;
                    if x >= left && x + tabs_width <= right {
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(x, header.response.rect.center().y - 16.0),
                            egui::vec2(tabs_width, 32.0),
                        );
                        let mut tabs = ui.new_child(egui::UiBuilder::new()
                            .id_salt("header_workspace_tabs")
                            .max_rect(rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)));
                        self.render_sidebar_tabs_bar(&mut tabs, remote);
                    } else {
                        let width = ui.available_width().min(tabs_width);
                        ui.horizontal(|ui| {
                            ui.add_space(((ui.available_width() - width) * 0.5).max(0.0));
                            ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
                                ui.set_max_width(width);
                                self.render_sidebar_tabs_bar(ui, remote);
                            });
                        });
                    }
                }
            });

        // ═════════════════════════════════════════════════════════════════════
        // LEFT MACHINES NAV BAR (Switch between This PC and SSH Remotes)
        // ═════════════════════════════════════════════════════════════════════
        egui::SidePanel::left("machines_nav_bar")
            .resizable(false)
            .exact_width(MACHINE_RAIL_WIDTH)
            .show_separator_line(false)
            .frame(
                Frame::none()
                    .fill(p.bg_machines_nav)
                    .inner_margin(egui::Margin {
                        left: 6.0,
                        right: 6.0,
                        top: 10.0,
                        bottom: 10.0,
                    }),
            )
            .show(ctx, |ui| {
                if listener_dialog_open { ui.disable(); }
                ui.set_width(ui.available_width());
                ui.set_min_width(ui.available_width());
                self.render_machines_nav_bar(ui);
            });

        if self.sidebar_tab_machine != self.selected_machine {
            let previous = std::mem::replace(&mut self.sidebar_tab_machine, self.selected_machine.clone());
            if previous == "local" || self.ssh_hosts.iter().any(|host| host.id == previous) {
                self.machine_sidebar_tabs.insert(previous, self.active_sidebar_tab);
            }
            self.active_sidebar_tab = self.machine_sidebar_tabs.get(&self.selected_machine)
                .copied().unwrap_or(SidebarTab::Servers);
        }

        if self.selected_machine == "local" {
            // ═════════════════════════════════════════════════════════════════
            // LOCAL PC VIEW: Left Sidebar + Right Main Area (Remaining)
            // ═════════════════════════════════════════════════════════════════
            egui::CentralPanel::default()
                .frame(
                    Frame::none()
                        .fill(p.bg_main)
                        .stroke(Stroke::NONE)
                        .inner_margin(egui::Margin {
                            left: 0.0,
                            right: 16.0,
                            top: 4.0,
                            bottom: 6.0,
                        }),
                )
                .show(ctx, |ui| {
                    if self.active_sidebar_tab == SidebarTab::Apps {
                        let p = self.palette();
                        crate::apps::ui::render_apps_page(&mut self.apps_manager, ui, &p, None);
                        return;
                    }
                    let header_rect = ui.available_rect_before_wrap();
                    let total_h = ui.available_height().max(0.0);
                    let left_w = self.workspace_sidebar_width();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.set_height(total_h);
                        ui.set_min_height(total_h);

                        // LEFT SIDEBAR
                        Frame::none()
                            .fill(p.bg_sidebar)
                            .stroke(Stroke::NONE)
                            .inner_margin(WORKSPACE_SIDEBAR_MARGIN)
                            .show(ui, |ui| {
                                ui.set_width(left_w);
                                ui.set_min_width(left_w);
                                ui.set_max_width(left_w);
                                ui.set_height(total_h);
                                ui.set_min_height(total_h);
                                ui.spacing_mut().item_spacing.x = ui.ctx().style().spacing.item_spacing.x;
                                let mut body_ui = workspace_sidebar_body(ui, false);
                                self.render_left_panel(&mut body_ui);
                            });

                        ui.add_space(WORKSPACE_GAP);

                        // RIGHT MAIN AREA: Fills 100% of remaining width
                        let right_w = ui.available_width().max(0.0);
                        ui.allocate_ui_with_layout(egui::vec2(right_w, total_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.spacing_mut().item_spacing.x = ui.ctx().style().spacing.item_spacing.x;
                            ui.set_width(right_w);
                            ui.set_max_width(right_w);
                            ui.set_height(total_h);
                            ui.set_min_height(total_h);
                            if self.active_sidebar_tab == SidebarTab::Sensors {
                                ui.add_space(WORKSPACE_HEADER_HEIGHT);
                            }
                            self.render_right_panel(ui);
                        });
                    });
                    self.render_workspace_header(ui, header_rect, None);
                });
        } else if let Some(adb_dev) = self.adb_devices.iter().find(|d| d.id == self.selected_machine).cloned() {
            // ═════════════════════════════════════════════════════════════════
            // ADB ANDROID DEVICE COCKPIT
            // ═════════════════════════════════════════════════════════════════
            self.render_adb_tauri_handoff(ctx, &adb_dev);
        } else {
            // ═════════════════════════════════════════════════════════════════
            // SSH REMOTE PC DASHBOARD
            // ═════════════════════════════════════════════════════════════════
            egui::CentralPanel::default()
                .frame(
                    Frame::none()
                        .fill(p.bg_main)
                        .inner_margin(egui::Margin {
                            left: 0.0,
                            right: 16.0,
                            top: 4.0,
                            bottom: 6.0,
                        }),
                )
                .show(ctx, |ui| {
                    let machine_id = self.selected_machine.clone();
                    ui.push_id(("ssh_workspace", &machine_id), |ui| self.render_remote_pc_dashboard(ui, &machine_id));
                });
        }

        // ═════════════════════════════════════════════════════════════════════
        // MODALS
        // ═════════════════════════════════════════════════════════════════════
        let modal_action = render_add_edit_modal(ctx, &mut self.add_edit_modal, &p);
        match modal_action {
            ModalAction::SaveServer(new_cfg, old_key) => {
                self.add_edit_modal.open = false;
                if let Some(old_k) = old_key {
                    if let Some(s) = self.find_service_mut(&old_k) {
                        s.config = new_cfg;
                    }
                } else {
                    self.services.push(Service::new(new_cfg));
                }
                self.persist_config();
            }
            ModalAction::DeleteServer(k) => {
                self.add_edit_modal.open = false;
                if let Some(idx) = self.services.iter().position(|s| s.config.key == k) {
                    let mut s = self.services.remove(idx);
                    s.stop();
                    self.persist_config();
                    if self.active_tab == k {
                        self.active_tab = "overview".to_string();
                    }
                }
            }
            _ => {}
        }

        let palette_items = self.collect_palette_actions();
        let palette_action = render_command_palette(ctx, &mut self.palette_modal, &palette_items, &p);
        match palette_action {
            ModalAction::ExecutePaletteAction(PaletteAction::StartServer(k)) => {
                if let Some(s) = self.find_service_mut(&k) {
                    s.start();
                }
            }
            ModalAction::ExecutePaletteAction(PaletteAction::StopServer(k)) => {
                if let Some(s) = self.find_service_mut(&k) {
                    s.stop();
                }
            }
            ModalAction::ExecutePaletteAction(PaletteAction::RestartServer(k)) => {
                if let Some(s) = self.find_service_mut(&k) {
                    s.restart();
                }
            }
            ModalAction::ExecutePaletteAction(PaletteAction::SelectTab(k)) => {
                self.active_tab = k;
            }
            ModalAction::ExecutePaletteAction(PaletteAction::RunAction { key, command }) => {
                self.run_service_action(&key, &command);
            }
            ModalAction::ExecutePaletteAction(PaletteAction::OpenLink(url)) => {
                ctx.open_url(egui::OpenUrl::new_tab(&url));
            }
            ModalAction::ExecutePaletteAction(PaletteAction::StartGroup(g)) => {
                self.start_group(&g);
            }
            ModalAction::ExecutePaletteAction(PaletteAction::StopGroup(g)) => {
                self.stop_group(&g);
            }
            ModalAction::ExecutePaletteAction(PaletteAction::StartAll) => {
                self.start_all();
            }
            ModalAction::ExecutePaletteAction(PaletteAction::StopAll) => {
                self.stop_all();
            }
            ModalAction::ExecutePaletteAction(PaletteAction::SetTheme(thm_id)) => {
                let thm = ThemeMode::from_id_str(&thm_id);
                self.theme = thm;
                let pal = thm.palette();
                ctx.set_visuals(pal.egui_visuals());
                self.last_applied_palette = Some(pal);
                self.persist_config();
            }
            ModalAction::ExecutePaletteAction(PaletteAction::ToggleAppAutostart) => {
                let current = crate::autostart::is_app_autostart_enabled();
                let _ = crate::autostart::set_app_autostart(!current);
                self.app_autostart = !current;
                self.persist_config();
            }
            ModalAction::ExecutePaletteAction(PaletteAction::ToggleServerAutostart(k)) => {
                if let Some(s) = self.find_service_mut(&k) {
                    s.config.autostart = !s.config.autostart;
                    self.persist_config();
                }
            }
            _ => {}
        }

        let ssh_modal_action = render_ssh_host_modal(ctx, &mut self.ssh_host_modal, &p, self.ssh_probe_timeouts.connection_test_seconds);
        match ssh_modal_action {
            ModalAction::SaveSshHost(mut new_host, old_id) => {
                self.ssh_host_modal.open = false;
                if let Some(old) = old_id {
                    if let Some(pos) = self.ssh_hosts.iter().position(|h| h.id == old) {
                        let previous = &self.ssh_hosts[pos];
                        let host_changed = new_host != *previous;
                        if new_host.password == previous.password
                            && crate::ssh_credentials::binding(&new_host) == crate::ssh_credentials::binding(previous)
                        {
                            new_host.credential_ref.clone_from(&previous.credential_ref);
                        }
                        if host_changed {
                            self.cancel_remote_commands(&old);
                            self.invalidate_remote_host(&old);
                        }
                        self.ssh_hosts[pos] = new_host.clone();
                        self.remote_host_lifetime(&new_host);
                    }
                    if self.selected_machine == old {
                        self.selected_machine = new_host.id.clone();
                    }
                } else {
                    self.selected_machine = new_host.id.clone();
                    self.ssh_hosts.push(new_host);
                }
                self.persist_config();
            }
            ModalAction::DeleteSshHost(id) => {
                self.ssh_host_modal.open = false;
                self.delete_ssh_host(&id);
            }
            _ => {}
        }

        if let Some(script) = render_script_modal(ctx, &mut self.script_modal, &p) {
            self.script_modal.open = false;
            if let Some(existing) = self.scripts.iter_mut().find(|s| s.id == script.id) {
                *existing = script.clone();
            } else {
                self.scripts.push(script.clone());
            }
            self.persist_config();
        }

        if let Some((script, host_id, password)) = render_sudo_modal(ctx, &mut self.sudo_modal, &p) {
            self.sudo_modal.close();
            self.start_script_execution(&script, host_id.as_deref(), Some(&password));
        }

        if let Some(cmd) = render_command_modal(ctx, &mut self.command_modal, &p) {
            self.command_modal.open = false;
            if let Some(existing) = self.saved_commands.iter_mut().find(|c| c.id == cmd.id) {
                *existing = cmd.clone();
            } else {
                self.saved_commands.push(cmd.clone());
            }
            self.command_catalog_view = None;
            self.persist_config();
        }

        if let Some(service_def) = render_new_service_modal(ctx, &mut self.systemd_modal, &p) {
            self.systemd_modal.open = false;
            let host = self.ssh_hosts.iter().find(|host| host.id == self.selected_machine).cloned();
            if self.selected_machine == "local" || host.is_some() {
                self.queue_systemd_action(PendingSudoAction::CreateService { def: service_def }, host.as_ref(), None);
            }
        }

        if let Some(req) = render_remote_start_server_modal(ctx, &mut self.remote_start_server_modal, &p) {
            self.remote_start_server_modal.open = false;
            if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == req.host_id).cloned() {
                let full_cmd = build_remote_start_cmd(&req.name, req.port.unwrap_or_default(), Some(&req.command), &req.cwd);
                if !self.run_remote_ssh_command(&host, &full_cmd, Some(ctx.clone()), true) {
                    self.remote_start_server_modal.open = true;
                } else {
                    if let Some(port) = req.port {
                        if let Ok(mut map) = self.remote_stopped_servers.lock() {
                            let list = map.entry(host.id.clone()).or_default();
                            if !list.iter().any(|s| s.port == port) {
                                list.push(RemoteStoppedServer {
                                    name: req.name.clone(),
                                    port,
                                    proto: "TCP".to_string(),
                                    last_pid: None,
                                    cmd: req.command.clone(),
                                    start_cmd: req.command.clone(),
                                    cwd: req.cwd.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }

        if let Some((host, password)) = render_remote_reboot_modal(ctx, &mut self.remote_reboot_modal, &p) {
            let host_clone = host.clone();
            let pass_clone = password.clone();
            let ctx_clone = ctx.clone();
            let rem_status = self.remote_connection_status.clone();

            std::thread::spawn(move || {
                match crate::remote::reboot_remote_device(&host_clone, &pass_clone) {
                    Ok(msg) => {
                        println!("[reboot] {}", msg);
                        if let Some(status) = rem_status.lock().unwrap().get_mut(&host_clone.id) {
                            *status = Err("Rebooting device...".to_string());
                        }
                    }
                    Err(err) => {
                        eprintln!("[reboot error] {}", err);
                        if let Some(status) = rem_status.lock().unwrap().get_mut(&host_clone.id) {
                            *status = Err(format!("Reboot failed: {}", err));
                        }
                    }
                }
                ctx_clone.request_repaint();
            });

            self.import_status_msg = Some((
                format!("{} Reboot signal sent to {}", crate::lucide::REFRESH_CW, host.name),
                false,
                Instant::now(),
            ));
        }

        if let Some((host, password)) = render_remote_shutdown_modal(ctx, &mut self.remote_shutdown_modal, &p) {
            let host_clone = host.clone();
            let pass_clone = password.clone();
            let ctx_clone = ctx.clone();
            let rem_status = self.remote_connection_status.clone();

            std::thread::spawn(move || {
                match crate::remote::shutdown_remote_device(&host_clone, &pass_clone) {
                    Ok(msg) => {
                        println!("[shutdown] {}", msg);
                        if let Some(status) = rem_status.lock().unwrap().get_mut(&host_clone.id) {
                            *status = Err("Shutting down device...".to_string());
                        }
                    }
                    Err(err) => {
                        eprintln!("[shutdown error] {}", err);
                        if let Some(status) = rem_status.lock().unwrap().get_mut(&host_clone.id) {
                            *status = Err(format!("Shutdown failed: {}", err));
                        }
                    }
                }
                ctx_clone.request_repaint();
            });

            self.import_status_msg = Some((
                format!("{} Shutdown signal sent to {}", crate::lucide::POWER, host.name),
                false,
                Instant::now(),
            ));
        }

        self.render_sudo_popup(ctx, &p);
        self.render_settings_modal(ctx, &p);
        self.render_proc_properties_modal(ctx, &p);
        self.render_proc_memory_maps_modal(ctx, &p);
        self.render_proc_open_files_modal(ctx, &p);
        self.render_proc_affinity_modal(ctx, &p);
        self.render_add_device_selector_modal(ctx);
        self.render_adb_add_modal(ctx);

        let is_scanning = self.scanner.is_scanning.lock().map(|s| *s).unwrap_or(false);
        {
            use crate::config::SshIdleMode;
            let focused = ctx.input(|input| input.focused && input.viewport().minimized != Some(true));
            let on_servers = matches!(self.active_sidebar_tab, SidebarTab::Servers);
            let now = Instant::now();
            for (host, activity) in self.remote_command_activity.lock().unwrap().iter_mut() {
                let inactive = match self.ssh_idle.mode {
                    SshIdleMode::PageOrWindowInactive => !focused || !on_servers || self.selected_machine != *host,
                    SshIdleMode::WindowInactive => !focused,
                    SshIdleMode::Never => false,
                };
                if inactive {
                    activity.inactive_since.get_or_insert(now);
                    activity.timeout = Some(std::time::Duration::from_secs(u64::from(self.ssh_idle.minutes.clamp(1, 120)) * 60));
                } else {
                    activity.inactive_since = None;
                    activity.timeout = None;
                }
            }
        }
        let has_remote_bg_work = {
            self.remote_connecting.lock().map(|s| !s.is_empty()).unwrap_or(false)
                || self.remote_scanning.lock().map(|s| !s.is_empty()).unwrap_or(false)
                || self.remote_services_in_flight.lock().map(|s| !s.is_empty()).unwrap_or(false)
                || self.systemd_actions_in_flight.lock().map(|s| !s.is_empty()).unwrap_or(false)
                || self.remote_sensors_in_flight.lock().map(|s| !s.is_empty()).unwrap_or(false)
        };
        if self.close_confirm_open {
            ctx.request_repaint_after(std::time::Duration::from_millis(1000));
        } else if is_scanning || has_remote_bg_work {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        } else if self.active_sidebar_tab == SidebarTab::Apps
            && (self.apps_manager.loading
                || self.apps_manager.checking_updates
                || self.apps_manager.task_running)
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        } else if self.active_sidebar_tab == SidebarTab::Sensors && self.sensors_refresh_interval > 0 {
            // Repaint at 1-second intervals on Sensors tab so real-time waveforms and second-by-second telemetry scroll smoothly
            ctx.request_repaint_after(std::time::Duration::from_millis(1000));
        } else if any_running {
            // When servers are running, align repaint with the 2.0s polling cycle
            ctx.request_repaint_after(std::time::Duration::from_millis(2000));
        } else {
            // When fully idle, repaint reactively on user interaction only (4s heartbeat)
            ctx.request_repaint_after(std::time::Duration::from_secs(4));
        }
    }
}

#[derive(Debug, Clone)]
enum ProcAction {
    Properties { pid: u32, name: String, cpu: f32, mem: u64 },
    MemoryMaps { pid: u32, name: String },
    OpenFiles { pid: u32, name: String },
    SetPriority { pid: u32, nice: i32 },
    SetAffinity { pid: u32, name: String },
    Stop { pid: u32 },
    Resume { pid: u32 },
    Terminate { pid: u32 },
    ForceStop { pid: u32 },
}

fn render_process_context_menu(
    ui: &mut egui::Ui,
    pid: u32,
    name: &str,
    cpu: f32,
    mem: u64,
    proc_action: &mut Option<ProcAction>,
    p: &crate::theme::ThemePalette,
    windows_process_host: bool,
) {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.bg_fill = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.hovered.bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 18);
        ui.visuals_mut().widgets.hovered.rounding = Rounding::same(4.0);
        ui.visuals_mut().widgets.active.bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 28);
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.0);

        let item = |ui: &mut egui::Ui, label: &str, shortcut: &str| -> egui::Response {
            let desired_size = egui::vec2(210.0, 24.0);
            let (rect, resp) = ui.allocate_exact_size(desired_size, egui::Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(rect, Rounding::same(4.0), Color32::from_rgba_unmultiplied(255, 255, 255, 18));
            }
            ui.painter().text(
                egui::pos2(rect.left() + 10.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(12.0),
                p.text_primary,
            );
            if !shortcut.is_empty() {
                ui.painter().text(
                    egui::pos2(rect.right() - 10.0, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    shortcut,
                    egui::FontId::proportional(11.0),
                    p.text_muted,
                );
            }
            resp
        };

        if windows_process_host {
            if item(ui, "End task", "Ctrl+T").clicked() {
                *proc_action = Some(ProcAction::Terminate { pid });
                ui.close_menu();
            }
            return;
        }

        // 1. Properties (Alt+Return)
        if item(ui, "Properties", "Alt+Return").clicked() {
            *proc_action = Some(ProcAction::Properties {
                pid,
                name: name.to_string(),
                cpu,
                mem,
            });
            ui.close_menu();
        }

        ui.add_space(2.0);
        ui.separator();
        ui.add_space(2.0);

        // 2. Memory Maps (Ctrl+M)
        if item(ui, "Memory Maps", "Ctrl+M").clicked() {
            *proc_action = Some(ProcAction::MemoryMaps {
                pid,
                name: name.to_string(),
            });
            ui.close_menu();
        }

        // 3. Open Files (Ctrl+O)
        if item(ui, "Open Files", "Ctrl+O").clicked() {
            *proc_action = Some(ProcAction::OpenFiles {
                pid,
                name: name.to_string(),
            });
            ui.close_menu();
        }

        ui.add_space(2.0);
        ui.separator();
        ui.add_space(2.0);

        // 4. Change Priority (Submenu)
        ui.menu_button(RichText::new("Change Priority").size(12.0).color(p.text_primary), |ui| {
            ui.set_min_width(170.0);
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.0);
            let prios = [
                ("Realtime (-20)", -20),
                ("High (-10)", -10),
                ("Above Normal (-5)", -5),
                ("Normal (0)", 0),
                ("Below Normal (5)", 5),
                ("Low / Idle (19)", 19),
            ];
            for (p_label, nice) in prios {
                if item(ui, p_label, "").clicked() {
                    *proc_action = Some(ProcAction::SetPriority { pid, nice });
                    ui.close_menu();
                }
            }
        });

        // 5. Set Affinity... (Alt+S)
        if item(ui, "Set Affinity...", "Alt+S").clicked() {
            *proc_action = Some(ProcAction::SetAffinity {
                pid,
                name: name.to_string(),
            });
            ui.close_menu();
        }

        ui.add_space(2.0);
        ui.separator();
        ui.add_space(2.0);

        // 6. Stop... (Ctrl+S)
        if item(ui, "Stop...", "Ctrl+S").clicked() {
            *proc_action = Some(ProcAction::Stop { pid });
            ui.close_menu();
        }

        // 7. Resume (Ctrl+C)
        if item(ui, "Resume", "Ctrl+C").clicked() {
            *proc_action = Some(ProcAction::Resume { pid });
            ui.close_menu();
        }

        // 8. Terminate... (Ctrl+T)
        if item(ui, "Terminate...", "Ctrl+T").clicked() {
            *proc_action = Some(ProcAction::Terminate { pid });
            ui.close_menu();
        }

        // 9. Force Stop... (Ctrl+K)
        if item(ui, "Force Stop...", "Ctrl+K").clicked() {
            *proc_action = Some(ProcAction::ForceStop { pid });
            ui.close_menu();
        }
    });
}

fn get_system_uptime() -> u64 {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()))
        .map(|s| s as u64)
        .unwrap_or(0)
}

fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    if days > 0 {
        format!("{}d {}h {}m", days, hours, mins)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

fn format_uptime_hms(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{}:{:02}:{:02}", hours, mins, s)
}

fn sensor_network_rate(snap: &crate::sensors::SystemSensorsSnapshot, value: f64) -> String {
    if snap.network_rates_available { crate::sensors::format_rate(value) } else { "—".to_string() }
}

fn sensor_network_bytes(snap: &crate::sensors::SystemSensorsSnapshot, value: u64) -> String {
    if snap.network_stats_available { crate::sensors::format_bytes(value) } else { "—".to_string() }
}

fn wifi_channel_label(channel: u32, frequency_mhz: u32) -> String {
    match (channel, frequency_mhz) {
        (channel, frequency) if channel > 0 && frequency > 0 => format!("Channel {channel} ({frequency} MHz)"),
        (channel, _) if channel > 0 => format!("Channel {channel}"),
        (_, frequency) if frequency > 0 => format!("{frequency} MHz"),
        _ => "Channel unavailable".into(),
    }
}

#[inline]
fn accent_with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

fn draw_waveform(ui: &mut Ui, height: f32, values: &VecDeque<f32>, max_val: f32, stroke_color: Color32, fill_color: Color32) {
    let (response, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let rect = response.rect;

    // Dark canvas background
    painter.rect_filled(rect, Rounding::same(6.0), Color32::from_rgb(10, 15, 29));

    // Grid lines with labels at 100%, 75%, 50%, 25%, 0%
    let levels = [(1.00_f32, "100%"), (0.75_f32, "75%"), (0.50_f32, "50%"), (0.25_f32, "25%"), (0.00_f32, "0%")];
    for (pct, label) in levels {
        let y = rect.bottom() - 6.0 - (rect.height() - 12.0) * pct;
        painter.line_segment(
            [egui::pos2(rect.left() + 8.0, y), egui::pos2(rect.right() - 36.0, y)],
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 10)),
        );
        painter.text(
            egui::pos2(rect.right() - 8.0, y),
            egui::Align2::RIGHT_CENTER,
            label,
            egui::FontId::monospace(8.5),
            Color32::from_rgb(100, 116, 139),
        );
    }

    if values.len() < 2 {
        return;
    }

    let n = values.len();
    let graph_w = (rect.width() - 44.0).max(20.0);
    let step_x = graph_w / (n - 1) as f32;

    let points: Vec<egui::Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = rect.left() + 8.0 + i as f32 * step_x;
            let norm = (v / max_val).clamp(0.0, 1.0);
            let y = rect.bottom() - 6.0 - norm * (rect.height() - 14.0);
            egui::pos2(x, y)
        })
        .collect();

    let top_fill = fill_color;
    let bot_fill = Color32::from_rgba_unmultiplied(stroke_color.r(), stroke_color.g(), stroke_color.b(), 3);
    let mut mesh = egui::Mesh::default();
    for i in 0..(points.len() - 1) {
        let p1 = points[i];
        let p2 = points[i + 1];
        let b1 = egui::pos2(p1.x, rect.bottom() - 6.0);
        let b2 = egui::pos2(p2.x, rect.bottom() - 6.0);

        let idx = mesh.vertices.len() as u32;
        mesh.vertices.push(egui::epaint::Vertex { pos: p1, uv: egui::epaint::WHITE_UV, color: top_fill });
        mesh.vertices.push(egui::epaint::Vertex { pos: p2, uv: egui::epaint::WHITE_UV, color: top_fill });
        mesh.vertices.push(egui::epaint::Vertex { pos: b2, uv: egui::epaint::WHITE_UV, color: bot_fill });
        mesh.vertices.push(egui::epaint::Vertex { pos: b1, uv: egui::epaint::WHITE_UV, color: bot_fill });

        mesh.indices.extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
    }
    painter.add(egui::Shape::mesh(mesh));

    for i in 0..(points.len() - 1) {
        painter.line_segment([points[i], points[i + 1]], Stroke::new(1.8_f32, stroke_color));
    }

    if let Some(&last_pt) = points.last() {
        painter.circle_filled(last_pt, 5.0, stroke_color.gamma_multiply(0.4));
        painter.circle_filled(last_pt, 2.5, stroke_color);
    }
}

fn draw_core_waveform(ui: &mut Ui, width: f32, height: f32, values: &[f32], stroke_color: Color32) {
    let (response, painter) = ui.allocate_painter(egui::vec2(width, height), egui::Sense::hover());
    let rect = response.rect;

    if values.len() < 2 { return; }

    let n = values.len();
    let step_x = rect.width() / (n - 1) as f32;

    let points: Vec<Option<egui::Pos2>> = values
        .iter()
        .enumerate()
        .map(|(i, &v)| v.is_finite().then(|| {
            let x = rect.left() + i as f32 * step_x;
            let norm = (v / 100.0).clamp(0.03, 0.97);
            let y = rect.bottom() - 2.0 - norm * (rect.height() - 4.0);
            egui::pos2(x, y)
        }))
        .collect();

    let top_fill = Color32::from_rgba_unmultiplied(stroke_color.r(), stroke_color.g(), stroke_color.b(), 35);
    let bot_fill = Color32::from_rgba_unmultiplied(stroke_color.r(), stroke_color.g(), stroke_color.b(), 4);

    let mut mesh = egui::Mesh::default();
    for i in 0..(points.len() - 1) {
        let (Some(p1), Some(p2)) = (points[i], points[i + 1]) else { continue; };
        let b1 = egui::pos2(p1.x, rect.bottom());
        let b2 = egui::pos2(p2.x, rect.bottom());

        let idx = mesh.vertices.len() as u32;
        mesh.vertices.push(egui::epaint::Vertex { pos: p1, uv: egui::epaint::WHITE_UV, color: top_fill });
        mesh.vertices.push(egui::epaint::Vertex { pos: p2, uv: egui::epaint::WHITE_UV, color: top_fill });
        mesh.vertices.push(egui::epaint::Vertex { pos: b2, uv: egui::epaint::WHITE_UV, color: bot_fill });
        mesh.vertices.push(egui::epaint::Vertex { pos: b1, uv: egui::epaint::WHITE_UV, color: bot_fill });

        mesh.indices.extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
    }
    painter.add(egui::Shape::mesh(mesh));

    for i in 0..(points.len() - 1) {
        if let (Some(p1), Some(p2)) = (points[i], points[i + 1]) {
            painter.line_segment([p1, p2], Stroke::new(1.6_f32, stroke_color));
        }
    }

    if let Some(last_pt) = points.iter().rev().find_map(|point| *point) {
        painter.circle_filled(last_pt, 3.5, stroke_color.gamma_multiply(0.4));
        painter.circle_filled(last_pt, 2.0, stroke_color);
    }
}

#[allow(dead_code)]
fn draw_sparkline(ui: &mut Ui, width: f32, height: f32, usage: f32, stroke_color: Color32) {
    let (response, painter) = ui.allocate_painter(egui::vec2(width, height), egui::Sense::hover());
    let rect = response.rect;
    painter.rect_filled(rect, Rounding::same(4.0), Color32::from_rgb(10, 15, 29));
    let norm = (usage / 100.0).clamp(0.0, 1.0);
    let bar_w = rect.width() * norm;
    if bar_w > 0.0 {
        let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(bar_w, rect.height()));
        painter.rect_filled(fill_rect, Rounding::same(4.0), stroke_color.gamma_multiply(0.2));
    }
    let p1 = egui::pos2(rect.left(), rect.bottom() - 2.0);
    let p2 = egui::pos2(rect.left() + rect.width() * 0.35, rect.bottom() - (rect.height() * norm * 0.5) - 2.0);
    let p3 = egui::pos2(rect.left() + rect.width() * 0.7, rect.bottom() - (rect.height() * norm * 0.8) - 2.0);
    let p4 = egui::pos2(rect.right(), rect.bottom() - (rect.height() * norm) - 2.0);
    painter.line_segment([p1, p2], Stroke::new(1.5_f32, stroke_color));
    painter.line_segment([p2, p3], Stroke::new(1.5_f32, stroke_color));
    painter.line_segment([p3, p4], Stroke::new(1.5_f32, stroke_color));
}

fn draw_ram_waveform(
    ui: &mut Ui,
    height: f32,
    history: &VecDeque<(f32, f32)>,
    total_str: &str,
    accent: Color32,
) {
    let (response, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let rect = response.rect;

    painter.rect_filled(rect, Rounding::same(6.0), Color32::from_rgb(10, 15, 29));

    // Grid line at top
    let top_y = rect.top() + 18.0;
    painter.line_segment(
        [egui::pos2(rect.left() + 8.0, top_y), egui::pos2(rect.right() - 8.0, top_y)],
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 8)),
    );
    painter.text(
        egui::pos2(rect.left() + 12.0, rect.top() + 10.0),
        egui::Align2::LEFT_CENTER,
        format!("{} (100%)", total_str),
        egui::FontId::monospace(8.5),
        Color32::from_rgb(100, 116, 139),
    );

    if history.len() < 2 {
        return;
    }

    let n = history.len();
    let graph_w = (rect.width() - 16.0).max(20.0);
    let step_x = graph_w / (n - 1) as f32;

    // Swap is an independent metric and may be unavailable when no swap is configured.
    let swap_pts: Vec<Option<egui::Pos2>> = history
        .iter()
        .enumerate()
        .map(|(i, &(_, swap_pct))| {
            if !swap_pct.is_finite() { return None; }
            let x = rect.left() + 8.0 + i as f32 * step_x;
            let norm = (swap_pct / 100.0).clamp(0.04, 0.96);
            let y = rect.bottom() - 18.0 - norm * (rect.height() - 36.0);
            Some(egui::pos2(x, y))
        })
        .collect();

    for (i, pair) in swap_pts.windows(2).enumerate() {
        if i % 2 == 0 {
            if let [Some(first), Some(second)] = pair {
                painter.line_segment([*first, *second], Stroke::new(1.4_f32, Color32::from_rgb(96, 165, 250)));
            }
        }
    }

    // 2. Committed Active wave (cyan) with gradient fill
    let ram_pts: Vec<egui::Pos2> = history
        .iter()
        .enumerate()
        .map(|(i, &(ram_pct, _))| {
            let x = rect.left() + 8.0 + i as f32 * step_x;
            let norm = (ram_pct / 100.0).clamp(0.04, 0.96);
            let y = rect.bottom() - 18.0 - norm * (rect.height() - 36.0);
            egui::pos2(x, y)
        })
        .collect();

    let top_fill = accent_with_alpha(accent, 55);
    let bot_fill = accent_with_alpha(accent, 3);
    let mut mesh = egui::Mesh::default();
    for i in 0..(ram_pts.len() - 1) {
        let p1 = ram_pts[i];
        let p2 = ram_pts[i + 1];
        let b1 = egui::pos2(p1.x, rect.bottom() - 6.0);
        let b2 = egui::pos2(p2.x, rect.bottom() - 6.0);

        let idx = mesh.vertices.len() as u32;
        mesh.vertices.push(egui::epaint::Vertex { pos: p1, uv: egui::epaint::WHITE_UV, color: top_fill });
        mesh.vertices.push(egui::epaint::Vertex { pos: p2, uv: egui::epaint::WHITE_UV, color: top_fill });
        mesh.vertices.push(egui::epaint::Vertex { pos: b2, uv: egui::epaint::WHITE_UV, color: bot_fill });
        mesh.vertices.push(egui::epaint::Vertex { pos: b1, uv: egui::epaint::WHITE_UV, color: bot_fill });

        mesh.indices.extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
    }
    painter.add(egui::Shape::mesh(mesh));

    for i in 0..(ram_pts.len() - 1) {
        painter.line_segment([ram_pts[i], ram_pts[i + 1]], Stroke::new(2.0_f32, accent));
    }

    if let Some(&last_pt) = ram_pts.last() {
        painter.circle_filled(last_pt, 5.0, accent_with_alpha(accent, 90));
        painter.circle_filled(last_pt, 2.8, accent);
    }
}

fn draw_disk_waveform(
    ui: &mut Ui,
    height: f32,
    write_hist: &VecDeque<f32>,
    read_hist: &VecDeque<f32>,
    accent: Color32,
) {
    let (response, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let rect = response.rect;

    painter.rect_filled(rect, Rounding::same(6.0), Color32::from_rgb(10, 15, 29));

    // Grid lines: 100 MB/s, 50 MB/s, 0 MB/s
    let levels = [(1.0_f32, "100 MB/s", ""), (0.5_f32, "50 MB/s", ""), (0.0_f32, "0 MB/s", "Now")];
    for (pct, left_label, right_label) in levels {
        let y = rect.bottom() - 18.0 - (rect.height() - 32.0) * pct;
        painter.line_segment(
            [egui::pos2(rect.left() + 50.0, y), egui::pos2(rect.right() - 40.0, y)],
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 8)),
        );
        painter.text(
            egui::pos2(rect.left() + 10.0, y),
            egui::Align2::LEFT_CENTER,
            left_label,
            egui::FontId::monospace(8.5),
            Color32::from_rgb(100, 116, 139),
        );
        painter.text(
            egui::pos2(rect.right() - 10.0, y),
            egui::Align2::RIGHT_CENTER,
            right_label,
            egui::FontId::monospace(8.5),
            Color32::from_rgb(100, 116, 139),
        );
    }

    let n = write_hist.len().max(read_hist.len());
    if n < 2 {
        return;
    }

    let graph_w = (rect.width() - 95.0).max(20.0);
    let step_x = graph_w / (n - 1) as f32;
    let max_val = 100.0_f32;

    // Read curve (slate / white)
    let read_pts: Vec<egui::Pos2> = read_hist
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = rect.left() + 50.0 + i as f32 * step_x;
            let norm = (v / max_val).clamp(0.03, 0.97);
            let y = rect.bottom() - 18.0 - norm * (rect.height() - 32.0);
            egui::pos2(x, y)
        })
        .collect();

    let mut mesh_read = egui::Mesh::default();
    for i in 0..(read_pts.len() - 1) {
        let p1 = read_pts[i];
        let p2 = read_pts[i + 1];
        let b1 = egui::pos2(p1.x, rect.bottom() - 6.0);
        let b2 = egui::pos2(p2.x, rect.bottom() - 6.0);
        let idx = mesh_read.vertices.len() as u32;
        let c_top = Color32::from_rgba_unmultiplied(203, 213, 225, 25);
        let c_bot = Color32::from_rgba_unmultiplied(203, 213, 225, 2);
        mesh_read.vertices.push(egui::epaint::Vertex { pos: p1, uv: egui::epaint::WHITE_UV, color: c_top });
        mesh_read.vertices.push(egui::epaint::Vertex { pos: p2, uv: egui::epaint::WHITE_UV, color: c_top });
        mesh_read.vertices.push(egui::epaint::Vertex { pos: b2, uv: egui::epaint::WHITE_UV, color: c_bot });
        mesh_read.vertices.push(egui::epaint::Vertex { pos: b1, uv: egui::epaint::WHITE_UV, color: c_bot });
        mesh_read.indices.extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
    }
    painter.add(egui::Shape::mesh(mesh_read));

    for i in 0..(read_pts.len() - 1) {
        painter.line_segment([read_pts[i], read_pts[i + 1]], Stroke::new(1.6_f32, Color32::from_rgb(203, 213, 225)));
    }

    // Write curve (cyan)
    let write_pts: Vec<egui::Pos2> = write_hist
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = rect.left() + 50.0 + i as f32 * step_x;
            let norm = (v / max_val).clamp(0.03, 0.97);
            let y = rect.bottom() - 18.0 - norm * (rect.height() - 32.0);
            egui::pos2(x, y)
        })
        .collect();

    let mut mesh_write = egui::Mesh::default();
    for i in 0..(write_pts.len() - 1) {
        let p1 = write_pts[i];
        let p2 = write_pts[i + 1];
        let b1 = egui::pos2(p1.x, rect.bottom() - 6.0);
        let b2 = egui::pos2(p2.x, rect.bottom() - 6.0);
        let idx = mesh_write.vertices.len() as u32;
        let c_top = accent_with_alpha(accent, 45);
        let c_bot = accent_with_alpha(accent, 3);
        mesh_write.vertices.push(egui::epaint::Vertex { pos: p1, uv: egui::epaint::WHITE_UV, color: c_top });
        mesh_write.vertices.push(egui::epaint::Vertex { pos: p2, uv: egui::epaint::WHITE_UV, color: c_top });
        mesh_write.vertices.push(egui::epaint::Vertex { pos: b2, uv: egui::epaint::WHITE_UV, color: c_bot });
        mesh_write.vertices.push(egui::epaint::Vertex { pos: b1, uv: egui::epaint::WHITE_UV, color: c_bot });
        mesh_write.indices.extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
    }
    painter.add(egui::Shape::mesh(mesh_write));

    for i in 0..(write_pts.len() - 1) {
        painter.line_segment([write_pts[i], write_pts[i + 1]], Stroke::new(1.8_f32, accent));
    }

    if let Some(&last_pt) = write_pts.last() {
        painter.circle_filled(last_pt, 4.5, accent_with_alpha(accent, 80));
        painter.circle_filled(last_pt, 2.5, accent);
    }
}


fn draw_dotted_hline(
    painter: &egui::Painter,
    y: f32,
    x_start: f32,
    x_end: f32,
    stroke: Stroke,
    dash_len: f32,
    gap_len: f32,
) {
    let mut x = x_start;
    while x < x_end {
        let seg_end = (x + dash_len).min(x_end);
        painter.line_segment([egui::pos2(x, y), egui::pos2(seg_end, y)], stroke);
        x += dash_len + gap_len;
    }
}

fn draw_estimated_power_waveform(ui: &mut Ui, height: f32, watts: f32, accent: Color32) {
    let (response, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let rect = response.rect;
    if !ui.is_rect_visible(rect) { return; }
    ui.ctx().request_repaint_after(Duration::from_millis(100));

    painter.rect_filled(rect, Rounding::same(6.0), ui.visuals().extreme_bg_color);

    let y_bot = rect.bottom() - 12.0;
    let y_top = rect.top() + 12.0;
    let plot_h = y_bot - y_top;

    // Dotted guide grid lines at 25%, 50%, 75%
    for ratio in [0.25_f32, 0.50_f32, 0.75_f32] {
        let y = y_bot - ratio * plot_h;
        draw_dotted_hline(&painter, y, rect.left() + 10.0, rect.right() - 10.0, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 12)), 4.0, 4.0);
    }

    let n = 48;
    let step_x = (rect.width() - 20.0) / (n - 1) as f32;
    let time_sec = ui.input(|i| i.time as f32);

    let max_watts = if watts > 60.0 { 150.0_f32 } else if watts > 30.0 { 65.0_f32 } else { 35.0_f32 };
    let to_y = |w: f32| -> f32 {
        let norm = (w / max_watts).clamp(0.08, 0.92);
        y_bot - norm * plot_h
    };

    let pts: Vec<egui::Pos2> = (0..n).map(|i| {
        let x = rect.left() + 10.0 + i as f32 * step_x;
        let progress = i as f32 / (n - 1) as f32;
        // Animated smooth ripple
        let a = progress * std::f32::consts::TAU * 1.5 - time_sec * 1.8;
        let ripple = (a.sin() * 0.12 + (a * 2.3).cos() * 0.05) * watts;
        // Center bump / natural telemetry crest
        let bell = (-(progress - 0.65).powi(2) / 0.08).exp() * (watts * 0.28);
        let current_w = (watts * 0.85 + bell + ripple).max(1.0);
        egui::pos2(x, to_y(current_w))
    }).collect();

    // Gradient Mesh Fill under curve
    let mut mesh = egui::Mesh::default();
    let c_top = accent_with_alpha(accent, 38);
    let c_bot = accent_with_alpha(accent, 2);
    for i in 0..(pts.len() - 1) {
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let b1 = egui::pos2(p1.x, y_bot);
        let b2 = egui::pos2(p2.x, y_bot);
        let idx = mesh.vertices.len() as u32;
        mesh.vertices.push(egui::epaint::Vertex { pos: p1, uv: egui::epaint::WHITE_UV, color: c_top });
        mesh.vertices.push(egui::epaint::Vertex { pos: p2, uv: egui::epaint::WHITE_UV, color: c_top });
        mesh.vertices.push(egui::epaint::Vertex { pos: b2, uv: egui::epaint::WHITE_UV, color: c_bot });
        mesh.vertices.push(egui::epaint::Vertex { pos: b1, uv: egui::epaint::WHITE_UV, color: c_bot });
        mesh.indices.extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
    }
    painter.add(egui::Shape::mesh(mesh));

    // Outer glow line segment
    for i in 0..(pts.len() - 1) {
        painter.line_segment([pts[i], pts[i + 1]], Stroke::new(2.4_f32, accent));
    }

    // Glowing telemetry head on the rightmost point
    if let Some(&last_pt) = pts.last() {
        painter.circle_filled(last_pt, 6.0, accent_with_alpha(accent, 80));
        painter.circle_filled(last_pt, 3.0, ui.visuals().text_color());

        // Floating readout chip
        let tag_w = 72.0_f32;
        let tag_h = 16.0_f32;
        let tag_rect = egui::Rect::from_min_size(egui::pos2(last_pt.x - tag_w - 6.0, (last_pt.y - tag_h - 4.0).max(rect.top() + 4.0)), egui::vec2(tag_w, tag_h));
        painter.rect_filled(tag_rect, Rounding::same(3.0), ui.visuals().widgets.noninteractive.bg_fill);
        painter.rect_stroke(tag_rect, Rounding::same(3.0), Stroke::new(1.0_f32, accent));
        let label_str = format!("{:.1} W EST.", watts);
        painter.text(tag_rect.center(), egui::Align2::CENTER_CENTER, label_str, egui::FontId::monospace(8.5), ui.visuals().text_color());
    }
}


fn all_batteries_have_status(batteries: &[crate::sensors::BatteryInfo], status: &str) -> bool {
    !batteries.is_empty() && batteries.iter().all(|battery| battery.status.trim().eq_ignore_ascii_case(status))
}

fn battery_identity(battery: &crate::sensors::BatteryInfo) -> &str {
    if battery.device_id.trim().is_empty() { &battery.name } else { &battery.device_id }
}

fn battery_device_key(batteries: &[crate::sensors::BatteryInfo], index: usize) -> String {
    let identity = battery_identity(&batteries[index]);
    if batteries.iter().filter(|item| battery_identity(item) == identity).count() > 1 {
        let ordinal = batteries[..index].iter().filter(|item| battery_identity(item) == identity).count() + 1;
        format!("{identity}\u{1f}{ordinal}")
    } else {
        identity.to_owned()
    }
}

fn battery_selector_label(batteries: &[crate::sensors::BatteryInfo], index: usize) -> String {
    let battery = &batteries[index];
    if batteries.iter().filter(|item| item.name == battery.name).count() > 1 {
        let ordinal = batteries[..index].iter().filter(|item| item.name == battery.name).count() + 1;
        format!("{} · {}", battery.name, ordinal)
    } else {
        battery.name.clone()
    }
}

fn battery_capacity_retention(battery: &crate::sensors::BatteryInfo) -> Option<f32> {
    battery.health_pct.filter(|value| value.is_finite() && (0.0..=100.0).contains(value)).or_else(|| {
        let full = battery.energy_full_wh?;
        let design = battery.energy_design_wh?;
        (full.is_finite() && design.is_finite() && full >= 0.0 && design > 0.0)
            .then(|| (full / design * 100.0).clamp(0.0, 100.0))
    })
}

fn reported_sensor_value(value: Option<f32>, decimals: usize) -> String {
    value.filter(|value| value.is_finite()).map(|value| format!("{value:.decimals$}"))
        .unwrap_or_else(|| "—".to_string())
}

fn reported_swap_usage(total_bytes: u64, usage_pct: f32) -> Option<f32> {
    (total_bytes > 0 && usage_pct.is_finite() && (0.0..=100.0).contains(&usage_pct)).then_some(usage_pct)
}

fn record_sensor_sample(history: &mut VecDeque<(Instant, Option<f32>)>, timestamp: Instant, value: Option<f32>) {
    while history.front().is_some_and(|sample| timestamp.saturating_duration_since(sample.0) > Duration::from_secs(60)) {
        history.pop_front();
    }
    if history.back().is_some_and(|sample| sample.0 >= timestamp) { return; }
    history.push_back((timestamp, value.filter(|value| value.is_finite())));
    if history.len() > 120 { history.pop_front(); }
}

fn sensor_metric_history(ui: &Ui, host_id: Option<&str>, metric: &str, timestamp: Option<Instant>, value: Option<f32>) -> Vec<f32> {
    let id = egui::Id::new(("reported_sensor_history", host_id, metric));
    ui.ctx().data_mut(|data| {
        let history = data.get_temp_mut_or_default::<VecDeque<(Instant, Option<f32>)>>(id);
        update_sensor_history(history, timestamp, value);
        history.iter().map(|sample| sample.1.unwrap_or(f32::NAN)).collect()
    })
}

fn update_sensor_history(history: &mut VecDeque<(Instant, Option<f32>)>, timestamp: Option<Instant>, value: Option<f32>) {
    if let Some(timestamp) = timestamp { record_sensor_sample(history, timestamp, value); } else { history.clear(); }
}

#[cfg(test)]
mod sensor_history_tests {
    use super::*;

    #[test]
    fn missing_snapshot_clears_old_metric_samples() {
        let mut history = VecDeque::from([(Instant::now(), Some(42.0))]);
        update_sensor_history(&mut history, None, None);
        assert!(history.is_empty());
    }
}

fn sensor_freshness(age: Option<Duration>, interval_secs: u32, failed: bool) -> &'static str {
    if failed { return "ERROR"; }
    if age.is_none() { return "LOADING"; }
    if interval_secs == 0 { return "PAUSED"; }
    if age.unwrap_or_default() > Duration::from_secs((interval_secs as u64 * 3).max(5)) { "STALE" } else { "LIVE" }
}

fn remote_sensor_due(age: Option<Duration>, interval_secs: u32, failed: bool) -> bool {
    match age {
        None => true,
        Some(_) if interval_secs == 0 => false,
        Some(age) => age >= Duration::from_secs(if failed { (interval_secs as u64).max(5) } else { interval_secs as u64 }),
    }
}

#[cfg(test)]
mod sensor_freshness_tests {
    use std::time::Duration;
    #[test]
    fn distinguishes_paused_failed_stale_and_retries() {
        assert_eq!(super::sensor_freshness(None, 1, false), "LOADING");
        assert_eq!(super::sensor_freshness(Some(Duration::from_secs(1)), 0, false), "PAUSED");
        assert_eq!(super::sensor_freshness(Some(Duration::from_secs(6)), 1, false), "STALE");
        assert_eq!(super::sensor_freshness(Some(Duration::from_secs(1)), 1, true), "ERROR");
        assert!(!super::remote_sensor_due(Some(Duration::from_secs(2)), 1, true));
        assert!(super::remote_sensor_due(Some(Duration::from_secs(5)), 1, true));
        assert!(!super::remote_sensor_due(Some(Duration::from_secs(50)), 0, true));
    }
}

fn memory_reclaimable_estimate(available: u64, free: Option<u64>) -> Option<u64> {
    free.map(|free| available.saturating_sub(free))
}

#[cfg(test)]
mod memory_quantity_tests {
    #[test]
    fn reclaimable_is_available_beyond_unallocated_memory() {
        assert_eq!(super::memory_reclaimable_estimate(400, Some(100)), Some(300));
        assert_eq!(super::memory_reclaimable_estimate(100, Some(200)), Some(0));
        assert_eq!(super::memory_reclaimable_estimate(400, None), None);
    }
}

fn gpu_display_load(gpu: Option<&crate::sensors::GpuSensorInfo>) -> (Option<f32>, bool) {
    let Some(gpu) = gpu else { return (None, false); };
    if let Some(load) = gpu.load_pct.filter(|value| value.is_finite() && (0.0..=100.0).contains(value)) {
        return (Some(load), false);
    }
    let busiest = gpu.engine_loads.iter().map(|(_, load)| *load)
        .filter(|load| load.is_finite() && (0.0..=100.0).contains(load)).reduce(f32::max);
    (busiest, busiest.is_some())
}

#[cfg(test)]
mod gpu_display_tests {
    #[test]
    fn distinguishes_overall_load_from_busiest_engine() {
        let mut gpu = crate::sensors::GpuSensorInfo::default();
        assert_eq!(super::gpu_display_load(Some(&gpu)), (None, false));
        gpu.engine_loads = vec![("Render".into(), 12.5), ("Copy".into(), 30.0)];
        assert_eq!(super::gpu_display_load(Some(&gpu)), (Some(30.0), true));
        gpu.load_pct = Some(21.0);
        assert_eq!(super::gpu_display_load(Some(&gpu)), (Some(21.0), false));
    }
}

fn matching_temperature_limit(sensors: &[crate::sensors::TempSensor], category: crate::sensors::SensorCategory, value: f32) -> Option<f32> {
    if !value.is_finite() { return None; }
    let mut matches = sensors.iter().filter(|sensor| sensor.category == category && sensor.temp == value);
    let sensor = matches.next()?;
    if matches.next().is_some() { return None; }
    reported_temperature_limit(sensor.crit, sensor.max)
}

fn reported_temperature_limit(crit: Option<f32>, max: Option<f32>) -> Option<f32> {
    crit.filter(|limit| limit.is_finite() && *limit > 0.0 && *limit <= 150.0)
        .or_else(|| max.filter(|limit| limit.is_finite() && *limit > 0.0 && *limit <= 150.0))
}

fn hottest_temperature_for_category(sensors: &[crate::sensors::TempSensor], category: crate::sensors::SensorCategory) -> Option<f32> {
    sensors.iter().filter(|sensor| sensor.category == category).map(|sensor| sensor.temp).filter(|temp| temp.is_finite()).reduce(f32::max)
}

#[cfg(test)]
mod temperature_limit_tests {
    use crate::sensors::{SensorCategory, TempSensor};
    #[test]
    fn only_uses_a_limit_from_the_unambiguous_matching_reading() {
        let sensor = TempSensor { category: SensorCategory::Storage, temp: 40.0, crit: Some(90.0), ..Default::default() };
        assert_eq!(super::matching_temperature_limit(&[sensor.clone()], SensorCategory::Storage, 40.0), Some(90.0));
        assert_eq!(super::matching_temperature_limit(&[sensor.clone()], SensorCategory::Cpu, 40.0), None);
        assert_eq!(super::matching_temperature_limit(&[sensor.clone(), sensor], SensorCategory::Storage, 40.0), None);
        let invalid_limit = TempSensor { category: SensorCategory::Storage, temp: 40.0, crit: Some(999.0), max: Some(200.0), ..Default::default() };
        assert_eq!(super::matching_temperature_limit(&[invalid_limit], SensorCategory::Storage, 40.0), None);
        assert_eq!(super::reported_temperature_limit(Some(999.0), Some(100.0)), Some(100.0));
        assert_eq!(super::reported_temperature_limit(Some(0.0), None), None);
    }
}

#[cfg(test)]
mod temperature_category_tests {
    use crate::sensors::{SensorCategory, TempSensor};

    #[test]
    fn uses_sensor_category_instead_of_incidental_label_words() {
        let sensors = [
            TempSensor { name: "GPU Memory".into(), category: SensorCategory::Gpu, temp: 70.0, ..Default::default() },
            TempSensor { name: "GPU Hotspot".into(), category: SensorCategory::Gpu, temp: 72.0, ..Default::default() },
            TempSensor { name: "DIMM 0".into(), category: SensorCategory::Memory, temp: 43.0, ..Default::default() },
            TempSensor { name: "DIMM 1".into(), category: SensorCategory::Memory, temp: 51.0, ..Default::default() },
            TempSensor { name: "NVMe Composite".into(), category: SensorCategory::Storage, temp: 52.0, ..Default::default() },
            TempSensor { name: "Drive 2".into(), category: SensorCategory::Storage, temp: 60.0, ..Default::default() },
        ];
        assert_eq!(super::hottest_temperature_for_category(&sensors, SensorCategory::Gpu), Some(72.0));
        assert_eq!(super::hottest_temperature_for_category(&sensors, SensorCategory::Memory), Some(51.0));
        assert_eq!(super::hottest_temperature_for_category(&sensors, SensorCategory::Storage), Some(60.0));
        let cpu_sensors = [
            TempSensor { name: "CPU Core 0".into(), category: SensorCategory::Cpu, temp: 62.0, ..Default::default() },
            TempSensor { name: "CPU Package".into(), category: SensorCategory::Cpu, temp: 68.0, ..Default::default() },
        ];
        assert_eq!(super::hottest_temperature_for_category(&cpu_sensors, SensorCategory::Cpu), Some(68.0));
        let cpu_sensors = [
            TempSensor { name: "CPU Core 0".into(), category: SensorCategory::Cpu, temp: 62.0, ..Default::default() },
            TempSensor { name: "CPU Package".into(), category: SensorCategory::Cpu, temp: 68.0, ..Default::default() },
        ];
        assert_eq!(super::hottest_temperature_for_category(&cpu_sensors, SensorCategory::Cpu), Some(68.0));
    }
}

fn disk_owns_mount(device: &str, partitions: &[crate::sensors::DiskPartitionInfo], source: &str) -> bool {
    let device = device.strip_prefix("/dev/").unwrap_or(device);
    let source = source.strip_prefix("/dev/").unwrap_or(source);
    source == device || partitions.iter().any(|partition| source == partition.name.strip_prefix("/dev/").unwrap_or(&partition.name))
}

fn mounted_storage_totals(disks: &[crate::sensors::DiskInfo]) -> (u64, u64, u64) {
    let mut seen_devices = std::collections::HashSet::new();
    disks.iter().filter(|disk| {
        if disk.name.starts_with("/dev/") { seen_devices.insert(disk.name.clone()) }
        else if disk.name.to_ascii_lowercase().starts_with(r"\\?\volume{") { seen_devices.insert(disk.name.to_ascii_lowercase()) }
        else { true }
    })
        .fold((0_u64, 0_u64, 0_u64), |(total, used, available), disk| (
            total.saturating_add(disk.total_bytes), used.saturating_add(disk.used_bytes),
            available.saturating_add(disk.available_bytes),
        ))
}

#[cfg(test)]
mod mounted_storage_tests {
    use crate::sensors::DiskInfo;
    #[test]
    fn mounts_match_only_the_device_or_reported_partitions() {
        use crate::sensors::DiskPartitionInfo;
        let parts = [DiskPartitionInfo { name: "sda1".into(), ..Default::default() }];
        assert!(super::disk_owns_mount("sda", &parts, "/dev/sda"));
        assert!(super::disk_owns_mount("sda", &parts, "/dev/sda1"));
        assert!(!super::disk_owns_mount("sda", &parts, "/dev/sdaa"));
        assert!(!super::disk_owns_mount("sda", &parts, "/dev/sdaa1"));
        assert!(!super::disk_owns_mount("sda", &parts, "/dev/mapper/root"));
        assert!(super::disk_owns_mount("mapper/root", &[], "/dev/mapper/root"));
    }

    #[test]
    fn totals_use_mounted_filesystems_without_duplicate_devices() {
        let disk = DiskInfo { name: "/dev/sda1".into(), total_bytes: 100, used_bytes: 60, available_bytes: 30, ..Default::default() };
        let other = DiskInfo { name: "/dev/sdb1".into(), total_bytes: 200, used_bytes: 50, available_bytes: 140, ..Default::default() };
        assert_eq!(super::mounted_storage_totals(&[disk.clone(), disk, other]), (300, 110, 170));
        assert_eq!(super::mounted_storage_totals(&[]), (0, 0, 0));
    }

    #[test]
    fn counts_a_windows_volume_once_across_mount_paths() {
        let volume = r"\\?\Volume{abc123}\";
        let mounts = [
            DiskInfo { name: volume.into(), mount_point: "C:".into(), total_bytes: 100, used_bytes: 60, available_bytes: 40, ..Default::default() },
            DiskInfo { name: r"\\?\VOLUME{ABC123}\".into(), mount_point: "C:\\Data".into(), total_bytes: 100, used_bytes: 60, available_bytes: 40, ..Default::default() },
        ];
        assert_eq!(super::mounted_storage_totals(&mounts), (100, 60, 40));
    }
}

fn select_remote_interface<'a>(interfaces: &'a [crate::sensors::NetInterfaceInfo], selected: Option<&str>) -> Option<&'a crate::sensors::NetInterfaceInfo> {
    selected.and_then(|name| interfaces.iter().find(|interface| interface.name != "lo" && interface.name == name))
        .or_else(|| interfaces.iter().find(|interface| interface.name != "lo" && interface.is_wireless == Some(true)))
        .or_else(|| interfaces.iter().find(|interface| interface.name != "lo"))
}

#[cfg(test)]
mod remote_interface_selection_tests {
    use crate::sensors::NetInterfaceInfo;
    #[test]
    fn prefers_selected_then_wifi_and_handles_removed_adapters() {
        let networks = vec![
            NetInterfaceInfo { name: "lo".into(), ..Default::default() },
            NetInterfaceInfo { name: "eth0".into(), ..Default::default() },
            NetInterfaceInfo { name: "wlan0".into(), is_wireless: Some(true), ..Default::default() },
        ];
        assert_eq!(super::select_remote_interface(&networks, None).unwrap().name, "wlan0");
        assert_eq!(super::select_remote_interface(&networks, Some("eth0")).unwrap().name, "eth0");
        assert_eq!(super::select_remote_interface(&networks, Some("removed")).unwrap().name, "wlan0");
        assert!(super::select_remote_interface(&networks[..1], None).is_none());
    }

}

fn telemetry_capacity(interval_secs: u32) -> usize {
    if interval_secs == 0 { 60 } else { (60 / interval_secs as usize).max(1) }
}

fn push_telemetry<T>(history: &mut VecDeque<T>, value: T, capacity: usize) {
    while history.len() >= capacity { history.pop_front(); }
    history.push_back(value);
}

fn card_sparkline_empty_state(history: &[f32]) -> Option<&'static str> {
    let samples = history.iter().filter(|value| value.is_finite()).count();
    match (history.is_empty(), samples) {
        (true, _) => Some("No history"),
        (false, 0) => Some("Unavailable"),
        (false, 1) => Some("Collecting history…"),
        _ => None,
    }
}

#[cfg(test)]
mod card_sparkline_state_tests {
    #[test]
    fn missing_samples_are_not_rendered_as_empty_or_zero_history() {
        assert_eq!(super::card_sparkline_empty_state(&[]), Some("No history"));
        assert_eq!(super::card_sparkline_empty_state(&[f32::NAN, f32::NAN]), Some("Unavailable"));
        assert_eq!(super::card_sparkline_empty_state(&[f32::NAN, 0.0]), Some("Collecting history…"));
        assert_eq!(super::card_sparkline_empty_state(&[0.0, 1.0]), None);
    }
}

fn draw_card_sparkline(
    ui: &mut Ui,
    height: f32,
    history: &[f32],
    max_val: f32,
    dotted_ratio: Option<f32>,
    color: Color32,
) {
    let (response, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let rect = response.rect;

    if let Some(state) = card_sparkline_empty_state(history) {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, state, egui::FontId::monospace(9.0), ui.visuals().weak_text_color());
        return;
    }

    if let Some(ratio) = dotted_ratio {
        let y = rect.bottom() - rect.height() * ratio.clamp(0.05, 0.95);
        draw_dotted_hline(
            &painter,
            y,
            rect.left(),
            rect.right(),
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(100, 116, 139, 70)),
            4.0,
            3.0,
        );
    }

    let n = history.len();

    let step_x = rect.width() / (n - 1) as f32;
    let pts: Vec<Option<egui::Pos2>> = history
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            if !v.is_finite() { return None; }
            let x = rect.left() + i as f32 * step_x;
            let norm = (v / max_val.max(1.0)).clamp(0.02, 0.98);
            let y = rect.bottom() - norm * (rect.height() - 4.0);
            Some(egui::pos2(x, y))
        })
        .collect();

    let mut mesh = egui::Mesh::default();
    let c_top = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 40);
    let c_bot = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 2);
    for i in 0..(pts.len() - 1) {
        let (Some(p1), Some(p2)) = (pts[i], pts[i + 1]) else { continue; };
        let b1 = egui::pos2(p1.x, rect.bottom());
        let b2 = egui::pos2(p2.x, rect.bottom());
        let idx = mesh.vertices.len() as u32;
        mesh.vertices.push(egui::epaint::Vertex { pos: p1, uv: egui::epaint::WHITE_UV, color: c_top });
        mesh.vertices.push(egui::epaint::Vertex { pos: p2, uv: egui::epaint::WHITE_UV, color: c_top });
        mesh.vertices.push(egui::epaint::Vertex { pos: b2, uv: egui::epaint::WHITE_UV, color: c_bot });
        mesh.vertices.push(egui::epaint::Vertex { pos: b1, uv: egui::epaint::WHITE_UV, color: c_bot });
        mesh.indices.extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
    }
    painter.add(egui::Shape::mesh(mesh));

    for i in 0..(pts.len() - 1) {
        if let (Some(first), Some(second)) = (pts[i], pts[i + 1]) {
            painter.line_segment([first, second], Stroke::new(1.8_f32, color));
        }
    }
}

// Sidebar frames inherit the horizontal split layout; give their content a vertical, clipped body.
fn workspace_sidebar_body(ui: &mut Ui, reserve_header: bool) -> Ui {
    let mut rect = ui.max_rect();
    if reserve_header {
        rect.min.y = (rect.top() + WORKSPACE_HEADER_HEIGHT).min(rect.bottom());
    }
    let mut body = ui.new_child(egui::UiBuilder::new()
        .id_salt("workspace_sidebar_body")
        .max_rect(rect)
        .layout(egui::Layout::top_down(egui::Align::Min)));
    body.set_clip_rect(ui.clip_rect().intersect(rect));
    body
}

fn sensor_grid_columns(available: f32, gap: f32, count: usize) -> usize {
    if count == 0 { return 0; }
    let max_columns = (((available + gap) / (180.0 + gap)).floor() as usize).clamp(1, count);
    let rows = count.div_ceil(max_columns);
    count.div_ceil(rows)
}

// Keep cards readable and distribute a final short row instead of leaving a single card alone.
fn sensor_card_columns(ui: &mut Ui, count: usize, mut render: impl FnMut(&mut Ui, usize)) {
    let gap = ui.spacing().item_spacing.x;
    let available = ui.available_width();
    let columns = sensor_grid_columns(available, gap, count);
    if columns == 0 { return; }
    let width = ((available - gap * (columns - 1) as f32) / columns as f32).max(0.0);
    ui.allocate_ui_with_layout(egui::vec2(available, 0.0), egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
        for index in 0..count {
            ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(width);
                ui.set_max_width(width);
                render(ui, index);
            });
        }
    });
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn sensor_cards_balance_rows_at_breakpoints() {
        assert_eq!(sensor_grid_columns(800.0, 8.0, 5), 3);
        assert_eq!(sensor_grid_columns(800.0, 8.0, 6), 3);
        assert_eq!(sensor_grid_columns(1_400.0, 8.0, 5), 5);
        assert_eq!(sensor_grid_columns(800.0, 8.0, 0), 0);
    }

    #[test]
    fn battery_flow_requires_a_uniform_reported_status() {
        let charging = crate::sensors::BatteryInfo { status: "Charging".into(), ..Default::default() };
        let discharging = crate::sensors::BatteryInfo { status: "Discharging".into(), ..Default::default() };
        assert!(!all_batteries_have_status(&[], "charging"));
        assert!(all_batteries_have_status(&[charging.clone()], "charging"));
        assert!(!all_batteries_have_status(&[charging, discharging], "charging"));
        assert!(!all_batteries_have_status(&[crate::sensors::BatteryInfo::default()], "discharging"));
    }

    #[test]
    fn duplicate_battery_names_keep_distinct_selector_identity() {
        let batteries = vec![
            crate::sensors::BatteryInfo { device_id: "BAT0".into(), name: "Battery".into(), ..Default::default() },
            crate::sensors::BatteryInfo { device_id: "ACPI\\BAT1".into(), name: "Battery".into(), ..Default::default() },
        ];
        assert_ne!(battery_device_key(&batteries, 0), battery_device_key(&batteries, 1));
        assert_eq!(battery_selector_label(&batteries, 0), "Battery · 1");
        assert_eq!(battery_selector_label(&batteries, 1), "Battery · 2");
        let unknown_ids = vec![crate::sensors::BatteryInfo { name: "Battery".into(), ..Default::default() }; 2];
        assert_ne!(battery_device_key(&unknown_ids, 0), battery_device_key(&unknown_ids, 1));
    }

    #[test]
    fn battery_capacity_retention_does_not_infer_new_or_healthy_batteries() {
        let mut battery = crate::sensors::BatteryInfo::default();
        assert_eq!(battery_capacity_retention(&battery), None);
        battery.energy_full_wh = Some(23.0);
        battery.energy_design_wh = Some(47.5);
        assert!((battery_capacity_retention(&battery).unwrap() - 48.421).abs() < 0.01);
        assert_eq!(battery.cycle_count, None);
        battery.energy_design_wh = Some(0.0);
        assert_eq!(battery_capacity_retention(&battery), None);
        battery.health_pct = Some(f32::NAN);
        assert_eq!(battery_capacity_retention(&battery), None);
    }


    #[test]
    fn reported_sensor_history_preserves_missing_samples_and_real_zeroes() {
        assert_eq!(reported_swap_usage(0, 0.0), None);
        assert_eq!(reported_swap_usage(100, 0.0), Some(0.0));
        assert_eq!(reported_swap_usage(100, 101.0), None);
        let now = Instant::now();
        let mut history = VecDeque::new();
        record_sensor_sample(&mut history, now, Some(0.0));
        record_sensor_sample(&mut history, now, Some(99.0));
        record_sensor_sample(&mut history, now + Duration::from_secs(1), Some(f32::NAN));
        record_sensor_sample(&mut history, now + Duration::from_secs(2), Some(12.0));
        assert_eq!(history.iter().map(|sample| sample.1).collect::<Vec<_>>(), vec![Some(0.0), None, Some(12.0)]);
        record_sensor_sample(&mut history, now + Duration::from_secs(63), Some(8.0));
        assert_eq!(history.iter().map(|sample| sample.1).collect::<Vec<_>>(), vec![Some(8.0)]);
        assert_eq!(reported_sensor_value(None, 1), "—");
        assert_eq!(reported_sensor_value(Some(0.0), 1), "0.0");
    }


    #[test]
    fn sudo_retries_resume_after_writes_instead_of_recreating_files() {
        let action = PendingSudoAction::CreateService { def: crate::systemd::NewServiceDefinition::default() };
        assert!(matches!(action.clone().authenticated_retry("Unit file created, but daemon reload failed: sudo: a password is required"), PendingSudoAction::ServiceAction { action: "finish-create", .. }));
        assert!(matches!(action.authenticated_retry("sudo: a password is required"), PendingSudoAction::CreateService { .. }));
        let dropin = PendingSudoAction::CreateDropIn { path: "/x.service.d/override.conf".into(), content: "[Service]".into(), user_mode: true };
        assert!(matches!(dropin.authenticated_retry("Drop-in created, but daemon reload failed: sudo: a password is required"), PendingSudoAction::SaveUnitFile { user_mode: true, .. }));
    }



    #[test]
    fn systemd_work_is_isolated_by_host_scope_and_operation() {
        let read = PendingSudoAction::ReadUnitFile { path: "/etc/systemd/system/app.service".into(), user_mode: false };
        let user_read = PendingSudoAction::ReadUnitFile { path: "/etc/systemd/system/app.service".into(), user_mode: true };
        let save = PendingSudoAction::SaveUnitFile { path: "/etc/systemd/system/app.service".into(), content: "draft".into(), user_mode: false };
        assert_ne!(read.key(None), read.key(Some("remote")));
        assert_ne!(read.key(None), user_read.key(None));
        assert_ne!(read.key(None), save.key(None));
        let start = PendingSudoAction::ServiceAction { unit: "app.service".into(), action: "start", user_mode: false };
        let stop = PendingSudoAction::ServiceAction { unit: "app.service".into(), action: "stop", user_mode: false };
        assert_eq!(start.key(None), stop.key(None)); // Opposing controls cannot run together.
    }


    #[test]
    fn other_listeners_are_filtered_by_process_ownership_not_port() {
        let pid = std::process::id();
        let mut sys = System::new();
        sys.refresh_processes_specifics(sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]), false, sysinfo::ProcessRefreshKind::nothing());
        let listener = crate::scanner::ListenerInfo { proto: "TCP".into(), port: 5173, pid: Some(pid), start_marker: None, name: "node".into() };
        assert!(!listener_is_managed(&listener, &BTreeSet::new(), &sys));
        assert!(listener_is_managed(&listener, &BTreeSet::from([pid]), &sys));
        assert!(!listener_is_managed(&listener, &BTreeSet::from([u32::MAX]), &sys));
        let unknown = crate::scanner::ListenerInfo { pid: None, ..listener.clone() };
        assert!(!listener_is_managed(&unknown, &BTreeSet::from([pid]), &sys));
        let parent = sys.process(sysinfo::Pid::from_u32(pid)).unwrap().parent().unwrap().as_u32();
        assert!(listener_is_managed(&listener, &BTreeSet::from([parent]), &sys));
    }

    #[test]
    fn offscreen_rows_reuse_only_matching_measured_heights() {
        let ctx = Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let id = ui.make_persistent_id("measured_row_test");
                let layout = ProcessRowLayout {
                    width: ui.available_width().to_bits(), scale: 1.0_f32.to_bits(),
                    expanded: true, console: true, metrics: None,
                };
                ui.ctx().data_mut(|data| data.insert_temp(id, MeasuredProcessRow { layout, height: 240.0 }));
                let start = ui.cursor().min;
                ui.set_clip_rect(egui::Rect::from_min_size(start + egui::vec2(0.0, 600.0), egui::vec2(400.0, 100.0)));
                assert!(!skip_measured_process_row(ui, id, ProcessRowLayout { expanded: false, ..layout }));
                assert!(!skip_measured_process_row(ui, id, ProcessRowLayout { console: false, ..layout }));
                assert!(!skip_measured_process_row(ui, id, ProcessRowLayout { width: 200.0_f32.to_bits(), ..layout }));
                assert_eq!(ui.cursor().min, start);
                assert!(skip_measured_process_row(ui, id, layout));
                assert!((ui.cursor().min.y - start.y - 240.0 - ui.spacing().item_spacing.y).abs() < 0.5);
                ui.set_clip_rect(ui.max_rect());
                assert!(!skip_measured_process_row(ui, id, layout));
            });
        });
    }

    #[test]
    fn console_layout_cache_refreshes_for_content_theme_and_width() {
        let ctx = Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut p = crate::theme::ThemeMode::Dark.palette();
                let mut logs = VecDeque::from([
                    crate::service::LogEntry { timestamp: "12:00:01".into(), text: "first".into(), kind: LogKind::Stdout },
                    crate::service::LogEntry { timestamp: "12:00:02".into(), text: "middle ".repeat(100).into(), kind: LogKind::Stdout },
                    crate::service::LogEntry { timestamp: "12:00:03".into(), text: "last".into(), kind: LogKind::Stdout },
                ]);
                let first = cached_inline_console_galley(ui, 8000, &logs, &p);
                assert!(Arc::ptr_eq(&first, &cached_inline_console_galley(ui, 8000, &logs, &p)));
                logs[1].text = "updated ".repeat(100).into();
                let updated = cached_inline_console_galley(ui, 8000, &logs, &p);
                assert!(!Arc::ptr_eq(&first, &updated));
                assert!(updated.job.text.contains("updated"));
                p.log_stdout = Color32::RED;
                let themed = cached_inline_console_galley(ui, 8000, &logs, &p);
                assert!(!Arc::ptr_eq(&updated, &themed));
                ui.set_width(120.0);
                let narrow = cached_inline_console_galley(ui, 8000, &logs, &p);
                assert!(!Arc::ptr_eq(&themed, &narrow));
                assert!(narrow.rows.len() > themed.rows.len());
            });
        });
    }

    #[test]
    fn long_process_details_do_not_widen_the_card() {
        for width in [260.0, 720.0] {
            let ctx = Context::default();
            let p = crate::theme::ThemeMode::Dark.palette();
            let long_command = "/very/long/process/path/".repeat(80);
            let mut card = egui::Rect::NOTHING;
            let _ = ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 300.0))),
                ..Default::default()
            }, |ctx| {
                egui::CentralPanel::default().frame(Frame::none()).show(ctx, |ui| {
                    card = Frame::none().inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Command:");
                            process_detail_value(ui, &long_command, p.text_secondary);
                        });
                    }).response.rect;
                });
            });
            assert!(card.right() <= width + 0.5);
        }
    }

    #[test]
    fn console_viewport_keeps_list_height_stable_while_scrolling() {
        let p = crate::theme::ThemeMode::Dark.palette();
        let logs = VecDeque::from([crate::service::LogEntry {
            timestamp: "12:00:00".into(), text: "A long console message ".repeat(100).into(), kind: LogKind::Stdout,
        }]);
        let mut heights = Vec::new();
        for offset in [0.0, 180.0, 400.0] {
            let ctx = Context::default();
            let mut height = 0.0;
            let _ = ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 300.0))),
                ..Default::default()
            }, |ctx| {
                egui::CentralPanel::default().frame(Frame::none()).show(ctx, |ui| {
                    let outer = ScrollArea::vertical().id_salt("console_list_test")
                        .scroll_offset(egui::vec2(0.0, offset)).show(ui, |ui| {
                            for port in [8000, 8001, 8002] {
                                let rect = render_inline_console_output(ui, port, Some(&logs), &p);
                                assert!((rect.height() - 216.0).abs() < 0.5);
                            }
                        });
                    height = outer.content_size.y;
                });
            });
            heights.push(height);
        }
        assert!(heights.iter().all(|height| (height - heights[0]).abs() < 0.5));
    }


    #[test]
    fn console_output_preserves_copy_text_and_separates_timestamp_colours() {
        let logs = VecDeque::from([
            crate::service::LogEntry { timestamp: "12:00:01".into(), text: "ready ✓\ncontinued".into(), kind: LogKind::Stdout },
            crate::service::LogEntry { timestamp: "12:00:02".into(), text: "failed".into(), kind: LogKind::Error },
            crate::service::LogEntry { timestamp: "12:00:03".into(), text: "warning".into(), kind: LogKind::Warning },
        ]);
        for mode in crate::theme::ThemeMode::all() {
            let p = mode.palette();
            let job = inline_console_log_layout(&logs, &p);
            assert_eq!(job.text, "[12:00:01] ready ✓\ncontinued\n[12:00:02] failed\n[12:00:03] warning");
            for (index, message_colour) in [p.log_stdout, p.log_stderr, p.log_warning].into_iter().enumerate() {
                assert_eq!(job.sections[index * 2].format.color, p.text_muted);
                assert_eq!(job.sections[index * 2 + 1].format.color, message_colour);
            }
        }
        assert!(inline_console_log_layout(&VecDeque::new(), &crate::theme::ThemeMode::Dark.palette()).text.is_empty());
    }


    #[test]
    fn notebook_tabs_keep_equal_width_and_one_baseline_as_count_grows() {
        let p = crate::theme::ThemeMode::Dark.palette();
        for available in [320.0, 800.0, 1200.0] {
            for count in [1, 4, 8, 20, 60] {
                for active_index in [0, count - 1] {
                    let ctx = Context::default();
                    let mut rects = Vec::new();
                    let _ = ctx.run(egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(available, 100.0))),
                        ..Default::default()
                    }, |ctx| {
                        egui::CentralPanel::default().frame(Frame::none()).show(ctx, |ui| {
                            let (width, gap) = notebook_tab_metrics(ui.available_width(), count);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                for index in 0..count {
                                    let mut label = egui::text::LayoutJob::default();
                                    label.append("A long server name that must truncate", 0.0, egui::TextFormat {
                                        font_id: egui::FontId::proportional(12.0), color: p.text_primary,
                                        ..Default::default()
                                    });
                                    rects.push(ui.push_id(index, |ui| render_notebook_tab(ui, label, width, index == active_index, &p)).inner.rect);
                                }
                            });
                        });
                    });
                    assert_eq!(rects.len(), count);
                    for rect in &rects {
                        assert!((rect.width() - rects[0].width()).abs() < 0.1);
                        assert!((rect.top() - rects[0].top()).abs() < 0.1);
                        assert!((rect.height() - 28.0).abs() < 0.1);
                        assert!(rect.left() >= -0.1 && rect.right() <= available + 0.5);
                    }
                    assert!((rects.last().unwrap().right() - available).abs() < 0.5);
                }
            }
        }
    }


    #[test]
    fn server_tab_order_handles_both_directions_and_no_op_drops() {
        let keys = || vec!["a".to_string(), "b".to_string(), "c".to_string(), "d".to_string()];
        let mut order = keys();
        assert!(move_server_tab(&mut order, "a", "d", true));
        assert_eq!(order, vec!["b", "c", "d", "a"]);
        assert!(move_server_tab(&mut order, "a", "b", false));
        assert_eq!(order, keys());
        assert!(move_server_tab(&mut order, "d", "b", true));
        assert_eq!(order, vec!["a", "b", "d", "c"]);
        let unchanged = order.clone();
        assert!(!move_server_tab(&mut order, "b", "d", false));
        assert!(!move_server_tab(&mut order, "b", "a", true));
        assert!(!move_server_tab(&mut order, "b", "b", true));
        assert!(!move_server_tab(&mut order, "missing", "a", false));
        assert!(!move_server_tab(&mut order, "a", "missing", false));
        assert_eq!(order, unchanged);
    }

    #[test]
    fn saved_tab_order_removes_stale_duplicates_and_appends_new_servers() {
        let mut order = vec!["b".to_string(), "deleted".to_string(), "b".to_string()];
        let keys = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        sync_server_tab_order(&mut order, &keys);
        assert_eq!(order, vec!["b", "a", "c"]);
        sync_server_tab_order(&mut order, &keys);
        assert_eq!(order, vec!["b", "a", "c"]);
        sync_server_tab_order(&mut order, &[]);
        assert!(order.is_empty());
    }

    #[test]
    fn tab_order_is_backward_compatible_and_round_trips() {
        let mut config: ConfigFile = serde_json::from_str(r#"{"servers":[]}"#).unwrap();
        assert!(config.server_tab_order.is_empty());
        config.server_tab_order = vec!["b".into(), "a".into()];
        let loaded: ConfigFile = serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(loaded.server_tab_order, vec!["b", "a"]);
        assert!(loaded.servers.is_empty());
    }


    #[test]
    fn compact_server_name_aligns_left_with_a_full_selection_target() {
        let ctx = Context::default();
        let palette = crate::theme::ThemeMode::Dark.palette();
        let mut title_rect = egui::Rect::NOTHING;
        let output = ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 150.0))),
            ..Default::default()
        }, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                title_rect = render_server_card_title(ui, "Portfolio", 200.0, 24.0, false, &palette).rect;
            });
        });
        let text_x = output.shapes.iter().find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                if text.galley.job.text == "Portfolio" { return Some(text.pos.x); }
            }
            None
        }).expect("Server name should be painted");
        assert!((text_x - title_rect.left()).abs() < 1.0);
        assert_eq!(title_rect.width(), 200.0);
    }


    #[test]
    fn server_card_switch_supports_keyboard_in_every_theme() {
        for mode in crate::theme::ThemeMode::all() {
            let ctx = Context::default();
            let palette = mode.palette();
            ctx.set_visuals(palette.egui_visuals());
            let mut on = false;
            for frame in 0..3 {
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))),
                    ..Default::default()
                };
                if frame == 2 {
                    input.events.push(egui::Event::Key {
                        key: Key::Space, physical_key: Some(Key::Space), pressed: true,
                        repeat: false, modifiers: egui::Modifiers::NONE,
                    });
                }
                let _ = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if frame == 1 {
                            let id = ui.next_auto_id();
                            ui.memory_mut(|memory| memory.request_focus(id));
                        }
                        let changed = render_modern_toggle_switch(ui, &mut on, &palette, "Start automatically");
                        if frame == 2 { assert!(changed && on, "Keyboard activation failed for {:?}", mode); }
                    });
                });
            }
        }
    }


    #[test]
    fn sidebar_content_starts_below_tabs_without_moving_right() {
        let ctx = Context::default();
        let _ = ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            ..Default::default()
        }, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.set_width(WORKSPACE_SIDEBAR_WIDTH);
                    ui.set_height(400.0);
                    let parent = ui.max_rect();
                    let mut body = workspace_sidebar_body(ui, true);
                    let first = body.label("Group filter").rect;
                    let second = body.label("Server list").rect;
                    assert_eq!(first.left(), parent.left());
                    assert!(first.top() >= parent.top() + WORKSPACE_HEADER_HEIGHT);
                    assert!(second.top() >= first.bottom());
                    assert!(body.clip_rect().top() >= parent.top() + WORKSPACE_HEADER_HEIGHT);
                });
            });
        });
    }


    #[test]
    fn sensor_cards_wrap_without_leaving_the_content_column() {
        for (width, expected_rows) in [(340.0, 4), (740.0, 2), (840.0, 1), (1000.0, 1)] {
            let ctx = Context::default();
            let mut cards = Vec::new();
            let _ = ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 800.0))),
                ..Default::default()
            }, |ctx| {
                egui::CentralPanel::default().frame(Frame::none()).show(ctx, |ui| {
                    sensor_card_columns(ui, 4, |ui, index| {
                        cards.push(ui.allocate_exact_size(egui::vec2(ui.available_width(), 60.0 + index as f32 * 20.0), egui::Sense::hover()).0);
                    });
                });
            });
            assert_eq!(cards.len(), 4);
            if expected_rows < 4 { assert_eq!(cards[0].top(), cards[1].top()); }
            let mut rows: Vec<_> = cards.iter().map(|r| r.top()).collect();
            rows.dedup();
            assert_eq!(rows.len(), expected_rows);
            for card in cards {
                assert!(card.left() >= 0.0 && card.right() <= width + 1.0);
            }
        }
    }
}

fn render_big_stat_card(
    ui: &mut Ui,
    icon: &str,
    title: &str,
    hero_val: &str,
    hero_unit: &str,
    subtitle: &str,
    left_stat: &str,
    right_stat: &str,
    sparkline_data: &[f32],
    sparkline_max: f32,
    dotted_ratio: Option<f32>,
    accent_color: Color32,
    bg_card: Color32,
    text_muted: Color32,
) {
    Frame::none()
        .fill(bg_card)
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::symmetric(18.0, 15.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(RichText::new(icon).size(15.0).color(accent_color));
                ui.add(egui::Label::new(RichText::new(title).size(11.0).monospace().strong().color(text_muted)).truncate()).hover_tip(title);
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.add(egui::Label::new(RichText::new(hero_val).size(if hero_val.len() > 12 { 24.0 } else { 34.0 }).strong().color(ui.visuals().text_color())).truncate()).hover_tip(hero_val);
                if !hero_unit.is_empty() {
                    ui.add(egui::Label::new(RichText::new(hero_unit).size(12.0).color(text_muted)).truncate()).hover_tip(hero_unit);
                }
            });
            ui.add_space(2.0);
            ui.add(egui::Label::new(RichText::new(subtitle).size(11.0).monospace().color(text_muted)).truncate()).hover_tip(subtitle);
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                let width = ((ui.available_width() - ui.spacing().item_spacing.x) * 0.5).max(0.0);
                ui.add_sized(egui::vec2(width, 17.0), egui::Label::new(RichText::new(left_stat).size(10.0).monospace().color(text_muted)).truncate()).hover_tip(left_stat);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_sized(egui::vec2(width, 17.0), egui::Label::new(RichText::new(right_stat).size(10.0).monospace().color(text_muted)).truncate()).hover_tip(right_stat);
                });
            });
            ui.add_space(5.0);
            draw_card_sparkline(ui, 44.0, sparkline_data, sparkline_max, dotted_ratio, accent_color);
        });
}

fn draw_dual_network_chart(
    ui: &mut Ui,
    height: f32,
    tx_hist: &VecDeque<f32>,
    rx_hist: &VecDeque<f32>,
    peak_val: f32,
    accent: Color32,
) {
    let (response, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let rect = response.rect;

    painter.rect_filled(rect, Rounding::same(6.0), Color32::from_rgb(10, 14, 22));

    let y_base = rect.bottom() - 12.0;
    painter.line_segment(
        [egui::pos2(rect.left() + 10.0, y_base), egui::pos2(rect.right() - 10.0, y_base)],
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 8)),
    );

    let y_peak = rect.top() + rect.height() * 0.28;
    draw_dotted_hline(
        &painter,
        y_peak,
        rect.left() + 10.0,
        rect.right() - 10.0,
        Stroke::new(1.2_f32, accent_with_alpha(accent, 120)),
        6.0,
        4.0,
    );

    let n = tx_hist.len().max(rx_hist.len());
    if n < 2 {
        return;
    }

    let graph_w = (rect.width() - 20.0).max(20.0);
    let step_x = graph_w / (n - 1) as f32;
    let max_v = peak_val.max(1.0);

    let rx_pts: Vec<egui::Pos2> = rx_hist
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = rect.left() + 10.0 + i as f32 * step_x;
            let norm = (v / max_v).clamp(0.02, 0.92);
            let y = y_base - norm * (y_base - rect.top() - 10.0);
            egui::pos2(x, y)
        })
        .collect();

    for i in 0..(rx_pts.len() - 1) {
        painter.line_segment([rx_pts[i], rx_pts[i + 1]], Stroke::new(1.8_f32, Color32::from_rgb(148, 163, 184)));
    }

    let tx_pts: Vec<egui::Pos2> = tx_hist
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = rect.left() + 10.0 + i as f32 * step_x;
            let norm = (v / max_v).clamp(0.02, 0.92);
            let y = y_base - norm * (y_base - rect.top() - 10.0);
            egui::pos2(x, y)
        })
        .collect();

    let mut mesh = egui::Mesh::default();
    let c_top = accent_with_alpha(accent, 30);
    let c_bot = accent_with_alpha(accent, 2);
    for i in 0..(tx_pts.len() - 1) {
        let p1 = tx_pts[i];
        let p2 = tx_pts[i + 1];
        let b1 = egui::pos2(p1.x, y_base);
        let b2 = egui::pos2(p2.x, y_base);
        let idx = mesh.vertices.len() as u32;
        mesh.vertices.push(egui::epaint::Vertex { pos: p1, uv: egui::epaint::WHITE_UV, color: c_top });
        mesh.vertices.push(egui::epaint::Vertex { pos: p2, uv: egui::epaint::WHITE_UV, color: c_top });
        mesh.vertices.push(egui::epaint::Vertex { pos: b2, uv: egui::epaint::WHITE_UV, color: c_bot });
        mesh.vertices.push(egui::epaint::Vertex { pos: b1, uv: egui::epaint::WHITE_UV, color: c_bot });
        mesh.indices.extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
    }
    painter.add(egui::Shape::mesh(mesh));

    for i in 0..(tx_pts.len() - 1) {
        painter.line_segment([tx_pts[i], tx_pts[i + 1]], Stroke::new(2.0_f32, accent));
    }
}

fn render_custom_toggle_switch(ui: &mut Ui, on: &mut bool, accent: Color32) -> bool {
    let desired_size = egui::vec2(34.0, 18.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let how_on = if *on { 1.0_f32 } else { 0.0_f32 };
        let bg = if *on {
            accent
        } else {
            Color32::from_rgb(30, 41, 59)
        };
        ui.painter().rect_filled(rect, Rounding::same(9.0), bg);

        let circle_x = egui::lerp((rect.left() + 9.0)..=(rect.right() - 9.0), how_on);
        let center = egui::pos2(circle_x, rect.center().y);
        let thumb_color = if *on {
            Color32::from_rgb(10, 15, 29)
        } else {
            Color32::from_rgb(148, 163, 184)
        };
        ui.painter().circle_filled(center, 6.0, thumb_color);
    }
    response.changed()
}

#[derive(Clone)]
struct ServerTabDrag {
    key: String,
}

fn sync_server_tab_order(order: &mut Vec<String>, keys: &[String]) {
    let mut seen = HashSet::new();
    order.retain(|key| keys.contains(key) && seen.insert(key.clone()));
    for key in keys {
        if !order.contains(key) { order.push(key.clone()); }
    }
}

fn move_server_tab(order: &mut Vec<String>, source: &str, target: &str, after: bool) -> bool {
    let Some(from) = order.iter().position(|key| key == source) else { return false; };
    let Some(to) = order.iter().position(|key| key == target) else { return false; };
    if from == to { return false; }
    let insertion = to + usize::from(after);
    let insertion = insertion - usize::from(from < insertion);
    if insertion == from { return false; }
    let key = order.remove(from);
    order.insert(insertion, key);
    true
}

fn notebook_tab_metrics(available: f32, count: usize) -> (f32, f32) {
    let count = count.max(1) as f32;
    let available = available.max(0.0);
    let gap = (available / count / 8.0).min(4.0);
    ((available - gap * (count - 1.0)) / count, gap)
}

fn render_notebook_tab(ui: &mut Ui, mut label: egui::text::LayoutJob, width: f32, active: bool, p: &crate::theme::ThemePalette) -> egui::Response {
    // Exact geometry prevents text padding or ellipses from pushing narrow tabs out of the row.
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, &label.text));
    let fill = if response.is_pointer_button_down_on() { p.accent_subtle }
        else if response.hovered() { p.bg_button_hover }
        else if active { p.bg_subtle } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(rect, Rounding::same(6.0), fill);
    let padding = (width / 8.0).min(10.0);
    label.wrap = egui::text::TextWrapping::truncate_at_width((width - padding * 2.0).max(0.0));
    let galley = ui.fonts(|fonts| fonts.layout_job(label));
    let pos = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).galley(pos, galley, p.text_primary);
    if active {
        let inset = (width * 0.12).min(12.0);
        ui.painter().rect_filled(egui::Rect::from_min_max(
            egui::pos2(rect.left() + inset, rect.bottom() - 2.0),
            egui::pos2(rect.right() - inset, rect.bottom()),
        ), Rounding::same(1.0), p.accent);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(rect, Rounding::same(6.0), Stroke::new(1.0_f32, p.accent));
    }
    response
}

// Only hide a listener when its PID belongs to a managed process tree, never just its configured port.
fn listener_is_managed(listener: &crate::scanner::ListenerInfo, managed_pids: &BTreeSet<u32>, sys: &System) -> bool {
    if managed_pids.is_empty() { return false; }
    let Some(mut pid) = listener.pid else { return false; };
    // Use the existing process cache; scrolling must not query the OS for each row.
    for _ in 0..=sys.processes().len() {
        if managed_pids.contains(&pid) { return true; }
        let Some(parent) = sys.process(sysinfo::Pid::from_u32(pid)).and_then(|process| process.parent()) else { return false; };
        pid = parent.as_u32();
    }
    false
}

#[derive(Clone, Copy, PartialEq)]
struct ProcessRowLayout {
    width: u32,
    scale: u32,
    expanded: bool,
    console: bool,
    metrics: Option<(u32, u32)>,
}

#[derive(Clone, Copy)]
struct MeasuredProcessRow {
    layout: ProcessRowLayout,
    height: f32,
}

// Reuse actual heights so skipping offscreen cards does not shift the scroll position.
fn skip_measured_process_row(ui: &mut Ui, id: egui::Id, layout: ProcessRowLayout) -> bool {
    if let Some(measured) = ui.ctx().data_mut(|data| data.get_temp::<MeasuredProcessRow>(id)) {
        if measured.layout == layout && measured.height.is_finite() && measured.height > 0.0 {
            let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), measured.height));
            if !ui.clip_rect().expand(80.0).intersects(rect) {
                ui.allocate_space(rect.size());
                return true;
            }
        }
    }
    false
}

#[derive(Clone)]
struct CachedConsoleGalley {
    fingerprint: u64,
    galley: Arc<egui::Galley>,
}

fn cached_inline_console_galley(ui: &Ui, port: u16, logs: &VecDeque<crate::service::LogEntry>, p: &crate::theme::ThemePalette) -> Arc<egui::Galley> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ui.available_width().to_bits().hash(&mut hasher);
    ui.ctx().pixels_per_point().to_bits().hash(&mut hasher);
    [p.text_muted, p.log_stdout, p.log_stderr, p.log_warning, p.log_launcher, p.log_stdin].hash(&mut hasher);
    logs.len().hash(&mut hasher);
    for entry in logs {
        entry.timestamp.hash(&mut hasher);
        entry.text.hash(&mut hasher);
        std::mem::discriminant(&entry.kind).hash(&mut hasher);
    }
    let fingerprint = hasher.finish();
    let id = ui.make_persistent_id(("console_text_layout", port));
    if let Some(cached) = ui.ctx().data_mut(|data| data.get_temp::<CachedConsoleGalley>(id)) {
        if cached.fingerprint == fingerprint { return cached.galley; }
    }
    let mut job = inline_console_log_layout(logs, p);
    job.wrap.max_width = ui.available_width().max(0.0);
    let galley = ui.fonts(|fonts| fonts.layout_job(job));
    ui.ctx().data_mut(|data| data.insert_temp(id, CachedConsoleGalley { fingerprint, galley: galley.clone() }));
    galley
}

fn process_detail_value(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    ui.add(egui::Label::new(RichText::new(text).size(10.5).monospace().color(color)).truncate())
        .hover_tip(text)
}

fn render_inline_console_output(ui: &mut Ui, port: u16, logs: Option<&VecDeque<crate::service::LogEntry>>, p: &crate::theme::ThemePalette) -> egui::Rect {
    Frame::none().fill(p.terminal_bg).rounding(Rounding::same(8.0))
        .inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            // A fixed viewport keeps the surrounding list height stable as it scrolls past the console.
            ScrollArea::vertical().id_salt(("inline_console_output", port))
                .min_scrolled_height(200.0).max_height(200.0).auto_shrink([false, false])
                .drag_to_scroll(false).stick_to_bottom(true).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some(logs) = logs.filter(|logs| !logs.is_empty()) {
                        // One selectable label allows copying across log-entry boundaries.
                        ui.add(egui::Label::new(cached_inline_console_galley(ui, port, logs, p)).selectable(true));
                    } else {
                        ui.label(RichText::new("No captured output yet. Run a command below or use Refresh Logs for available process logs.")
                            .size(10.5).color(p.text_muted));
                    }
                });
        }).response.rect
}

fn inline_console_log_layout(logs: &VecDeque<crate::service::LogEntry>, p: &crate::theme::ThemePalette) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for (index, entry) in logs.iter().enumerate() {
        let color = match entry.kind {
            LogKind::Stderr | LogKind::Error => p.log_stderr,
            LogKind::Warning => p.log_warning,
            LogKind::Launcher => p.log_launcher,
            LogKind::Stdin => p.log_stdin,
            LogKind::Stdout => p.log_stdout,
        };
        job.append(&format!("{}[{}] ", if index == 0 { "" } else { "\n" }, entry.timestamp), 0.0, egui::TextFormat {
            font_id: egui::FontId::monospace(10.5), color: p.text_muted, line_height: Some(14.0),
            ..Default::default()
        });
        job.append(&entry.text, 0.0, egui::TextFormat {
            font_id: egui::FontId::monospace(10.5), color, line_height: Some(14.0),
            ..Default::default()
        });
    }
    job
}

fn render_server_card_title(ui: &mut Ui, name: &str, width: f32, height: f32, selected: bool, p: &crate::theme::ThemePalette) -> egui::Response {
    let row = ui.allocate_ui_with_layout(egui::vec2(width, height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_width(width);
        ui.add(egui::Label::new(RichText::new(name).size(12.5).strong()
            .color(if selected { p.accent } else { p.text_primary }))
            .truncate().halign(egui::Align::Min));
    }).response;
    let response = ui.interact(row.rect, row.id.with("select_output"), egui::Sense::click());
    let hint = format!("Show output for {}", name);
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, &hint));
    if response.has_focus() {
        ui.painter().rect_stroke(row.rect, Rounding::same(4.0), Stroke::new(1.0_f32, p.accent));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand).hover_tip(hint)
}

fn render_modern_toggle_switch(ui: &mut Ui, on: &mut bool, p: &crate::theme::ThemePalette, label: &str) -> bool {
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(30.0, 16.0), egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, label));
    let how_on = ui.ctx().animate_bool(response.id, *on);
    let rounding = Rounding::same(8.0);
    let fill = if *on { p.accent } else if response.hovered() { p.bg_button_hover } else { p.bg_button };
    ui.painter().rect_filled(rect, rounding, fill);
    if !*on {
        ui.painter().rect_stroke(rect, rounding, Stroke::new(1.0_f32, p.text_muted));
    }
    if response.has_focus() {
        ui.painter().rect_stroke(rect.expand(2.0), rounding, Stroke::new(1.5_f32, p.accent));
    }
    let circle_x = egui::lerp((rect.left() + 8.0)..=(rect.right() - 8.0), how_on);
    let thumb = if *on { p.accent_text } else { p.text_secondary };
    ui.painter().circle_filled(egui::pos2(circle_x, rect.center().y), 5.0, thumb);
    let changed = response.changed();
    response.hover_tip(label);
    changed
}


fn draw_wifi_realtime_waveform(ui: &mut Ui, height: f32, rx_history: &VecDeque<f32>, tx_history: &VecDeque<f32>, accent: Color32) {
    let (response, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let rect = response.rect;

    painter.rect_filled(rect, Rounding::same(6.0), Color32::from_rgb(10, 15, 29));

    let n = rx_history.len().max(tx_history.len());
    if n < 2 {
        return;
    }

    // Dynamic scale ceiling calculation from current throughput telemetry (values are in MB/s)
    let observed_max = rx_history.iter().chain(tx_history.iter()).copied().fold(0.0_f32, f32::max);
    let scale_max = if observed_max <= 0.5 {
        1.0_f32
    } else if observed_max <= 2.0 {
        2.5_f32
    } else if observed_max <= 5.0 {
        5.0_f32
    } else if observed_max <= 10.0 {
        10.0_f32
    } else if observed_max <= 25.0 {
        25.0_f32
    } else if observed_max <= 50.0 {
        50.0_f32
    } else if observed_max <= 100.0 {
        100.0_f32
    } else {
        (observed_max * 1.25).ceil()
    };

    let left_offset = 55.0_f32;
    let right_offset = 35.0_f32;
    let y_top = rect.top() + 14.0;
    let y_base = rect.bottom() - 14.0;
    let plot_h = (y_base - y_top).max(20.0);

    // Subtle horizontal grid lines at 100%, 50%, 0%
    let levels = [
        (1.0_f32, format!("{:.1} MB/s", scale_max), ""),
        (0.5_f32, format!("{:.1} MB/s", scale_max * 0.5), ""),
        (0.0_f32, "0 B/s".to_string(), "Now"),
    ];

    for (pct, left_label, right_label) in levels {
        let y = y_base - plot_h * pct;
        painter.line_segment(
            [egui::pos2(rect.left() + left_offset, y), egui::pos2(rect.right() - right_offset, y)],
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 8)),
        );
        painter.text(
            egui::pos2(rect.left() + 8.0, y),
            egui::Align2::LEFT_CENTER,
            left_label,
            egui::FontId::monospace(8.0),
            Color32::from_rgb(100, 116, 139),
        );
        painter.text(
            egui::pos2(rect.right() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            right_label,
            egui::FontId::monospace(8.0),
            Color32::from_rgb(100, 116, 139),
        );
    }

    let graph_w = (rect.width() - left_offset - right_offset).max(20.0);
    let step_x = graph_w / (n - 1) as f32;

    // 1. RX Points (Electric Cyan with gradient mesh underneath)
    let rx_pts: Vec<egui::Pos2> = rx_history
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = rect.left() + left_offset + i as f32 * step_x;
            let norm = (v / scale_max).clamp(0.0, 1.0);
            let y = y_base - norm * plot_h;
            egui::pos2(x, y)
        })
        .collect();

    let mut mesh_rx = egui::Mesh::default();
    for i in 0..(rx_pts.len() - 1) {
        let p1 = rx_pts[i];
        let p2 = rx_pts[i + 1];
        let b1 = egui::pos2(p1.x, y_base);
        let b2 = egui::pos2(p2.x, y_base);
        let idx = mesh_rx.vertices.len() as u32;
        let c_top = accent_with_alpha(accent, 35);
        let c_bot = accent_with_alpha(accent, 2);
        mesh_rx.vertices.push(egui::epaint::Vertex { pos: p1, uv: egui::epaint::WHITE_UV, color: c_top });
        mesh_rx.vertices.push(egui::epaint::Vertex { pos: p2, uv: egui::epaint::WHITE_UV, color: c_top });
        mesh_rx.vertices.push(egui::epaint::Vertex { pos: b2, uv: egui::epaint::WHITE_UV, color: c_bot });
        mesh_rx.vertices.push(egui::epaint::Vertex { pos: b1, uv: egui::epaint::WHITE_UV, color: c_bot });
        mesh_rx.indices.extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
    }
    painter.add(egui::Shape::mesh(mesh_rx));

    for i in 0..(rx_pts.len() - 1) {
        painter.line_segment([rx_pts[i], rx_pts[i + 1]], Stroke::new(2.0_f32, accent));
    }

    // 2. TX Points (Slate / Light Gray line)
    let tx_pts: Vec<egui::Pos2> = tx_history
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = rect.left() + left_offset + i as f32 * step_x;
            let norm = (v / scale_max).clamp(0.0, 1.0);
            let y = y_base - norm * plot_h;
            egui::pos2(x, y)
        })
        .collect();

    for i in 0..(tx_pts.len() - 1) {
        painter.line_segment([tx_pts[i], tx_pts[i + 1]], Stroke::new(1.6_f32, Color32::from_rgb(203, 213, 225)));
    }

    // Glowing endpoint indicators at "Now"
    if let Some(&last_tx) = tx_pts.last() {
        painter.circle_filled(last_tx, 3.5, Color32::from_rgb(203, 213, 225));
    }
    if let Some(&last_rx) = rx_pts.last() {
        painter.circle_filled(last_rx, 6.0, accent_with_alpha(accent, 90));
        painter.circle_filled(last_rx, 3.5, Color32::from_rgb(241, 245, 249));
    }
}

fn thermal_samples_connected(previous: Instant, current: Instant, interval_secs: u32) -> bool {
    interval_secs > 0 && current.checked_duration_since(previous)
        .is_some_and(|elapsed| elapsed <= Duration::from_secs((interval_secs as u64 * 3).max(5)))
}

#[cfg(test)]
mod thermal_history_gap_tests {
    use super::*;
    #[test]
    fn missed_and_manual_samples_do_not_form_continuous_lines() {
        let now = Instant::now();
        assert!(thermal_samples_connected(now, now + Duration::from_secs(2), 1));
        assert!(!thermal_samples_connected(now, now + Duration::from_secs(6), 1));
        assert!(!thermal_samples_connected(now, now + Duration::from_secs(2), 0));
        assert!(!thermal_samples_connected(now, now - Duration::from_secs(1), 1));
    }
}

fn draw_thermal_dual_waveform(ui: &mut Ui, height: f32, samples: &VecDeque<(Instant, f32, f32)>, accent: Color32, interval_secs: u32) {
    let (response, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let rect = response.rect;
    painter.rect_filled(rect, Rounding::same(6.0), Color32::from_rgb(10, 14, 22));
    let plot = rect.shrink2(egui::vec2(12.0, 14.0));
    for ratio in [0.25_f32, 0.5, 0.75] {
        let y = plot.bottom() - ratio * plot.height();
        draw_dotted_hline(&painter, y, plot.left(), plot.right(), Stroke::new(1.0_f32, Color32::from_white_alpha(12)), 4.0, 4.0);
    }
    let Some(latest) = samples.back().map(|sample| sample.0) else {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Waiting for temperature samples", egui::FontId::proportional(11.0), Color32::from_rgb(148, 163, 184));
        return;
    };
    let ceiling = samples.iter().flat_map(|sample| [sample.1, sample.2]).filter(|value| value.is_finite()).fold(100.0_f32, f32::max) + 5.0;
    let mut any_reading = false;
    for (channel, color) in [(0, accent), (1, Color32::from_rgb(148, 163, 184))] {
        let mut previous: Option<(Instant, egui::Pos2)> = None;
        for &(timestamp, cpu, gpu) in samples {
            let value = if channel == 0 { cpu } else { gpu };
            if !value.is_finite() {
                previous = None;
                continue;
            }
            any_reading = true;
            let age = latest.saturating_duration_since(timestamp).as_secs_f32();
            let point = egui::pos2(plot.right() - (age / 60.0).clamp(0.0, 1.0) * plot.width(), plot.bottom() - (value / ceiling).clamp(0.0, 1.0) * plot.height());
            if let Some((_, last)) = previous.filter(|(last_time, _)| thermal_samples_connected(*last_time, timestamp, interval_secs)) {
                if channel == 0 {
                    let mut mesh = egui::Mesh::default();
                    for (pos, color) in [(last, accent_with_alpha(color, 40)), (point, accent_with_alpha(color, 40)), (egui::pos2(point.x, plot.bottom()), accent_with_alpha(color, 2)), (egui::pos2(last.x, plot.bottom()), accent_with_alpha(color, 2))] {
                        mesh.vertices.push(egui::epaint::Vertex { pos, uv: egui::epaint::WHITE_UV, color });
                    }
                    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                    painter.add(egui::Shape::mesh(mesh));
                }
                painter.line_segment([last, point], Stroke::new(if channel == 0 { 2.2_f32 } else { 1.8_f32 }, color));
            }
            previous = Some((timestamp, point));
        }
        if let Some((_, point)) = previous {
            painter.circle_filled(point, 3.0, color);
        }
    }
    if !any_reading {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Temperature readings unavailable", egui::FontId::proportional(11.0), Color32::from_rgb(148, 163, 184));
    }
}


impl LauncherApp {
    pub fn refresh_server_list(&mut self) {
        self.scanner.trigger_scan();
        crate::service::reconnect_services(&mut self.services);
        for service in &mut self.services {
            service.poll_status(&self.sys);
        }
    }

    fn get_proc_cmd_and_cwd(&mut self, pid: u32) -> (Option<String>, Option<String>) {
        if let Some((cmd, cwd, last_read)) = self.proc_info_cache.get(&pid) {
            if last_read.elapsed() < std::time::Duration::from_secs(5) {
                return (cmd.clone(), cwd.clone());
            }
        }
        let (cmd, cwd) = read_proc_cmd_and_cwd(pid);
        self.proc_info_cache.insert(pid, (cmd.clone(), cwd.clone(), Instant::now()));
        (cmd, cwd)
    }





    // ═════════════════════════════════════════════════════════════════════════
    // SCRIPTS TAB: Left Sidebar (List & Create) + Right Console Output
    // ═════════════════════════════════════════════════════════════════════════
    fn request_run_script(&mut self, script: &CustomScript, host_id: Option<String>) {
        self.selected_script_id = Some(script.id.clone());
        let needs_sudo = script_requires_sudo(&script.script);

        if needs_sudo {
            if let Some(ref hid) = host_id {
                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == *hid).cloned() {
                    if !host.password.trim().is_empty() {
                        self.start_script_execution(script, Some(hid), Some(&host.password));
                    } else {
                        self.sudo_modal.open_for_script(script.clone(), Some(&host));
                    }
                } else {
                    self.start_script_execution(script, host_id.as_deref(), None);
                }
            } else {
                if check_local_sudo_needs_password() {
                    self.sudo_modal.open_for_script(script.clone(), None);
                } else {
                    self.start_script_execution(script, None, None);
                }
            }
        } else {
            self.start_script_execution(script, host_id.as_deref(), None);
        }
    }

    fn start_script_execution(
        &mut self,
        script: &CustomScript,
        host_id: Option<&str>,
        sudo_password: Option<&str>,
    ) {
        let mut exec = self
            .script_executions
            .remove(&script.id)
            .unwrap_or_else(|| ScriptExecution::new(script.id.clone()));

        if let Some(hid) = host_id {
            if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                execute_remote_script(&host, script, &mut exec, sudo_password);
            }
        } else {
            execute_local_script(script, &mut exec, sudo_password);
        }
        self.script_executions.insert(script.id.clone(), exec);
    }







    fn load_systemd_unit_files(&mut self, unit: &str, user_mode: bool, host_opt: Option<&SshRemoteHost>) -> (crate::systemd::UnitFilesInfo, bool) {
        let cache_key = format!("{}:{}:{}", host_opt.map(|h| h.id.as_str()).unwrap_or("local"), unit, user_mode);
        let cached = self.systemd_files_cache.lock().ok().and_then(|c| c.get(&cache_key).cloned());
        let in_flight = self.systemd_files_in_flight.lock().map(|s| s.contains(&cache_key)).unwrap_or(false);
        if let Some((info, time)) = cached {
            if time.elapsed() < Duration::from_secs(30) {
                return (info, false);
            }
            if !in_flight {
                self.trigger_systemd_unit_files_fetch(unit, user_mode, host_opt);
            }
            return (info, true);
        }
        if !in_flight {
            self.trigger_systemd_unit_files_fetch(unit, user_mode, host_opt);
        }
        (crate::systemd::UnitFilesInfo::default(), true)
    }

    fn load_systemd_unit_dependencies(&mut self, unit: &str, user_mode: bool, host_opt: Option<&SshRemoteHost>) -> (crate::systemd::UnitDependenciesInfo, bool) {
        let cache_key = format!("{}:{}:{}", host_opt.map(|h| h.id.as_str()).unwrap_or("local"), unit, user_mode);
        let cached = self.systemd_deps_cache.lock().ok().and_then(|c| c.get(&cache_key).cloned());
        let in_flight = self.systemd_deps_in_flight.lock().map(|s| s.contains(&cache_key)).unwrap_or(false);
        if let Some((deps, time)) = cached {
            if time.elapsed() < Duration::from_secs(45) {
                return (deps, false);
            }
            if !in_flight {
                self.trigger_systemd_unit_dependencies_fetch(unit, user_mode, host_opt);
            }
            return (deps, true);
        }
        if !in_flight {
            self.trigger_systemd_unit_dependencies_fetch(unit, user_mode, host_opt);
        }
        (crate::systemd::UnitDependenciesInfo::default(), true)
    }



    fn queue_systemd_action(&self, action: PendingSudoAction, host: Option<&SshRemoteHost>, password: Option<String>) {
        let host_lifetime = host.and_then(|host| self.remote_host_lifetime(host));
        if host.is_some() && host_lifetime.is_none() { return; }
        let key = action.key(host.map(|host| host.id.as_str()));
        if password.is_none() && self.sudo_popup.open && self.sudo_popup.pending.as_ref().is_some_and(|pending| pending.key(self.sudo_popup.host_id.as_deref()) == key) { return; }
        if password.is_none() && matches!(action, PendingSudoAction::InspectUnit { .. } | PendingSudoAction::QueryEnabled { .. })
            && self.service_query_errors.get(&key).is_some_and(|time| time.elapsed() < Duration::from_secs(5)) { return; }
        if !self.systemd_actions_in_flight.lock().unwrap().insert(key.clone()) { return; }
        let from_popup = password.is_some();
        let host = host.cloned();
        let password = if matches!(action, PendingSudoAction::UnlockSensorAccess) {
            None
        } else {
            password.or_else(|| {
                host.as_ref().and_then(|host| self.remote_sudo_passwords.get(&host.id).cloned())
                    .or_else(|| host.is_none().then(|| self.local_sudo_password.clone()))
            })
        };
        let sensor_collector = self.sensors_collector.clone();
        let results = self.systemd_action_results.clone();
        std::thread::spawn(move || {
            let pass = password.as_deref().filter(|password| !password.is_empty());
            let result = match &action {
                PendingSudoAction::UnlockSensorAccess => {
                    if host.is_some() { Err("Protected sensor access applies to the local host only.".into()) }
                    else {
                        crate::sensors::read_protected_hardware().and_then(|data| {
                            sensor_collector.lock().unwrap().store_protected_hardware(data)
                        }).map(SystemdActionValue::Message)
                    }
                },
                PendingSudoAction::SetChargeLimit { battery, limit } => crate::power::set_charge_limit(battery, *limit, host.as_ref(), pass).map(SystemdActionValue::Message),
                PendingSudoAction::ServiceAction { unit, action, user_mode } => {
                    if let Some(host) = &host { control_remote_service(host, unit, action, *user_mode, pass) }
                    else { control_local_service(unit, action, *user_mode, pass) }.map(SystemdActionValue::Message)
                }
                PendingSudoAction::CreateService { def } => {
                    if let Some(host) = &host { create_remote_service(host, def, pass) }
                    else { create_local_service(def, pass) }.map(SystemdActionValue::Message)
                }
                PendingSudoAction::SaveUnitFile { path, content, user_mode } => {
                    if let Some(host) = &host { crate::systemd::write_remote_unit_file(host, path, content, *user_mode, pass) }
                    else { write_local_unit_file(path, content, *user_mode, pass) }.map(SystemdActionValue::Message)
                }
                PendingSudoAction::CreateDropIn { path, content, user_mode } => {
                    if let Some(host) = &host { crate::systemd::create_remote_drop_in(host, path, content, *user_mode, pass) }
                    else { crate::systemd::create_local_drop_in(path, content, *user_mode, pass) }.map(SystemdActionValue::Message)
                }
                PendingSudoAction::DeleteDropIn { path, user_mode } => {
                    if let Some(host) = &host { crate::systemd::delete_remote_drop_in(host, path, *user_mode, pass) }
                    else { delete_local_drop_in(path, *user_mode, pass) }.map(SystemdActionValue::Message)
                }
                PendingSudoAction::ReadUnitFile { path, .. } => {
                    if let Some(host) = &host { crate::systemd::read_remote_unit_file(host, path, pass) }
                    else { crate::systemd::read_local_unit_file(path, pass) }.map(SystemdActionValue::FileContent)
                }
                PendingSudoAction::ReadJournal { unit, user_mode } => {
                    if let Some(host) = &host { fetch_remote_journal(host, unit, *user_mode, 100, pass) }
                    else { fetch_local_journal(unit, *user_mode, 100, pass) }.map(SystemdActionValue::Journal)
                }
                PendingSudoAction::QueryEnabled { unit, user_mode } => {
                    if let Some(host) = &host { crate::systemd::is_remote_service_enabled(host, unit, *user_mode) }
                    else { crate::systemd::is_local_service_enabled(unit, *user_mode) }.map(SystemdActionValue::Enabled)
                }
                PendingSudoAction::InspectUnit { unit, user_mode } => {
                    if let Some(host) = &host { crate::systemd::get_remote_unit_status(host, unit, *user_mode) }
                    else { crate::systemd::get_local_unit_status(unit, *user_mode) }.map(SystemdActionValue::Status)
                }
            };
            if let Ok(mut results) = results.lock() {
                results.push_back(SystemdActionResult { key, action: Some(action), host_id: host.map(|host| host.id), host_lifetime, from_popup, result });
            }
        });
    }

    fn apply_systemd_results(&mut self) {
        let results: Vec<_> = self.systemd_action_results.lock().unwrap().drain(..).collect();
        for result in results {
            if result.host_id.as_ref().is_some_and(|id| !self.remote_host_lifetimes.lock().unwrap().get(id).is_some_and(|current| result.host_lifetime.as_ref().is_some_and(|token| Arc::ptr_eq(current, token)))) { continue; }
            self.systemd_actions_in_flight.lock().unwrap().remove(&result.key);
            let host = result.host_id.as_ref().and_then(|id| self.ssh_hosts.iter().find(|host| &host.id == id)).cloned();
            let host_key = result.host_id.as_deref().unwrap_or("local");
            let current_host = self.selected_machine == host_key;
            let matching_popup = self.sudo_popup.pending.as_ref().is_some_and(|pending| pending.key(self.sudo_popup.host_id.as_deref()) == result.key);
            let Some(action) = result.action else {
                if let Err(error) = result.result {
                    if result.host_id.is_some() { *self.remote_service_action_msg.lock().unwrap() = Some(error); }
                    else { self.service_action_msg = Some(error); }
                }
                continue;
            };
            let action_scope = match &action {
                PendingSudoAction::UnlockSensorAccess | PendingSudoAction::SetChargeLimit { .. } => false,
                PendingSudoAction::CreateService { def } => def.is_user,
                PendingSudoAction::ServiceAction { user_mode, .. } | PendingSudoAction::SaveUnitFile { user_mode, .. }
                | PendingSudoAction::CreateDropIn { user_mode, .. } | PendingSudoAction::DeleteDropIn { user_mode, .. }
                | PendingSudoAction::ReadUnitFile { user_mode, .. } | PendingSudoAction::ReadJournal { user_mode, .. }
                | PendingSudoAction::QueryEnabled { user_mode, .. } | PendingSudoAction::InspectUnit { user_mode, .. } => *user_mode,
            };
            let current_editor = current_host && self.systemd_editor_context.as_ref() == Some(&(host_key.to_string(), action_scope));
            match result.result {
                Err(error) => {
                    if matches!(action, PendingSudoAction::InspectUnit { .. } | PendingSudoAction::QueryEnabled { .. }) { self.service_query_errors.insert(result.key.clone(), Instant::now()); }
                    if let PendingSudoAction::ReadUnitFile { path, .. } = &action {
                        self.systemd_file_content_cache.lock().unwrap().insert(format!("{}:{}", host_key, path), Err(error.clone()));
                    }
                    if let PendingSudoAction::ReadJournal { unit, user_mode } = &action {
                        let scope = if *user_mode { "user" } else { "system" };
                        if result.host_id.is_some() { self.remote_journal_logs.lock().unwrap().insert(format!("{}:{}:{}", host_key, unit, scope), format!("Error: {}", error)); }
                        else { self.local_journal_logs.lock().unwrap().insert(format!("{}:{}", unit, scope), format!("Error: {}", error)); }
                    }
                    if current_host && matches!(action, PendingSudoAction::SetChargeLimit { .. }) { self.power_status_msg = Some((error.clone(), true, Instant::now())); }
                    if current_host { self.systemd_file_status_msg = Some((error.clone(), true)); }
                    if result.host_id.is_some() { *self.remote_service_action_msg.lock().unwrap() = Some(format!("Error: {}", error)); }
                    else { self.service_action_msg = Some(format!("Error: {}", error)); }
                    if !matches!(action, PendingSudoAction::UnlockSensorAccess) && crate::systemd::needs_sudo_password(&error) {
                        if let Some(id) = &result.host_id { self.remote_sudo_passwords.remove(id); } else { self.local_sudo_password.clear(); }
                        if (!result.from_popup || matching_popup) && (!self.sudo_popup.open || matching_popup) {
                            self.sudo_popup.open = true;
                            self.sudo_popup.pending = Some(action.authenticated_retry(&error));
                            self.sudo_popup.host_id = result.host_id;
                            self.sudo_popup.password_input.clear();
                            self.sudo_popup.error = result.from_popup.then_some(error);
                            self.sudo_popup.just_opened = true;
                        }
                    } else if matching_popup && matches!(action, PendingSudoAction::UnlockSensorAccess) {
                        self.sudo_popup.error = Some(error);
                        self.sudo_popup.password_input.clear();
                    } else if matching_popup {
                        self.sudo_popup.open = false;
                        self.sudo_popup.pending = None;
                        self.sudo_popup.password_input.clear();
                    }
                }
                Ok(value) => {
                    self.service_query_errors.remove(&result.key);
                    if matching_popup {
                        self.sudo_popup.open = false;
                        self.sudo_popup.pending = None;
                        self.sudo_popup.password_input.clear();
                        self.sudo_popup.error = None;
                    }
                    match value {
                        SystemdActionValue::FileContent(content) => {
                            if let PendingSudoAction::ReadUnitFile { path, .. } = &action {
                                self.systemd_file_content_cache.lock().unwrap().insert(format!("{}:{}", host_key, path), Ok(content.clone()));
                                if current_editor && self.systemd_selected_file.as_deref() == Some(path) && self.systemd_file_content == self.systemd_file_original {
                                    self.systemd_file_content = content.clone(); self.systemd_file_original = content;
                                    self.systemd_file_status_msg = None;
                                }
                            }
                        }
                        SystemdActionValue::Journal(logs) => {
                            if let PendingSudoAction::ReadJournal { unit, user_mode } = &action {
                                let scope = if *user_mode { "user" } else { "system" };
                                if result.host_id.is_some() { self.remote_journal_logs.lock().unwrap().insert(format!("{}:{}:{}", host_key, unit, scope), logs); }
                                else { self.local_journal_logs.lock().unwrap().insert(format!("{}:{}", unit, scope), logs); }
                            }
                        }
                        SystemdActionValue::Enabled(enabled) => {
                            if let PendingSudoAction::QueryEnabled { unit, user_mode } = &action {
                                self.service_enabled_cache.insert(format!("{}:{}:{}", host_key, unit, if *user_mode { "user" } else { "system" }), (enabled, Instant::now()));
                            }
                        }
                        SystemdActionValue::Status(service) => {
                            self.service_details_cache.insert(format!("{}:{}:{}", host_key, service.name, service.is_user), (service, Instant::now()));
                        }
                        SystemdActionValue::Message(message) => {
                            if result.host_id.is_some() { *self.remote_service_action_msg.lock().unwrap() = Some(message.clone()); }
                            else { self.service_action_msg = Some(message.clone()); }
                            let mut refresh = None;
                            match &action {
                                PendingSudoAction::UnlockSensorAccess => self.trigger_local_sensors_query(false, None),
                                PendingSudoAction::SetChargeLimit { .. } => { if current_host { self.power_status_msg = Some((message.clone(), false, Instant::now())); if let Some(host) = host.clone() { self.trigger_remote_sensors_query(host); } else { self.trigger_local_sensors_query(false, None); } } },
                                PendingSudoAction::ServiceAction { unit, user_mode, .. } => { refresh = Some((unit.clone(), *user_mode)); }
                                PendingSudoAction::CreateService { def } => {
                                    if let Ok(unit) = crate::systemd::service_unit_name(def) { refresh = Some((unit, def.is_user)); }
                                }
                                PendingSudoAction::SaveUnitFile { path, content, .. } | PendingSudoAction::CreateDropIn { path, content, .. } => {
                                    self.systemd_file_content_cache.lock().unwrap().insert(format!("{}:{}", host_key, path), Ok(content.clone()));
                                    if current_editor {
                                        if matches!(action, PendingSudoAction::CreateDropIn { .. }) && self.systemd_file_content == self.systemd_file_original
                                            && self.selected_service.as_ref().is_some_and(|unit| path.contains(&format!("/{}.d/", unit))) {
                                            self.systemd_selected_file = Some(path.clone()); self.systemd_file_content = content.clone(); self.systemd_new_dropin_open = false;
                                        }
                                        if self.systemd_selected_file.as_deref() == Some(path) { self.systemd_file_original = content.clone(); }
                                        self.systemd_file_status_msg = Some((message.clone(), false));
                                    }
                                    self.systemd_files_cache.lock().unwrap().clear();
                                }
                                PendingSudoAction::DeleteDropIn { path, .. } => {
                                    self.systemd_file_content_cache.lock().unwrap().remove(&format!("{}:{}", host_key, path));
                                    self.systemd_files_cache.lock().unwrap().clear();
                                    if current_editor && self.systemd_selected_file.as_deref() == Some(path) {
                                        self.systemd_selected_file = None; self.systemd_file_content.clear(); self.systemd_file_original.clear();
                                        self.systemd_file_status_msg = Some((message.clone(), false));
                                    }
                                }
                                _ => {}
                            }
                            if let Some((unit, user_mode)) = refresh.filter(|_| result.host_id.is_none() || host.is_some()) {
                                self.service_enabled_cache.remove(&format!("{}:{}:{}", host_key, unit, if user_mode { "user" } else { "system" }));
                                self.service_details_cache.remove(&format!("{}:{}:{}", host_key, unit, user_mode));
                                if let Some(host) = &host { self.trigger_remote_services_list(host, user_mode); }
                                else if result.host_id.is_none() { self.trigger_local_services_list(user_mode); }
                                self.queue_systemd_action(PendingSudoAction::InspectUnit { unit: unit.clone(), user_mode }, host.as_ref(), None);
                                self.queue_systemd_action(PendingSudoAction::ReadJournal { unit, user_mode }, host.as_ref(), None);
                            }
                        }
                    }
                }
            }
        }
    }

    fn execute_pending_sudo_action(&mut self, action: PendingSudoAction) {
        let host = self.sudo_popup.host_id.as_ref().and_then(|id| self.ssh_hosts.iter().find(|host| &host.id == id)).cloned();
        if self.sudo_popup.host_id.is_some() && host.is_none() {
            self.sudo_popup.error = Some("The original remote host is no longer configured.".into());
            return;
        }
        let password = self.sudo_popup.password_input.clone();
        if password.is_empty() { self.sudo_popup.error = Some("Enter the sudo password for this host.".into()); return; }
        if self.sudo_popup.remember_password {
            if let Some(host) = &host { self.remote_sudo_passwords.insert(host.id.clone(), password.clone()); }
            else { self.local_sudo_password = password.clone(); }
        }
        self.queue_systemd_action(action, host.as_ref(), Some(password));
    }








    #[inline]
    pub fn palette(&self) -> crate::theme::ThemePalette {
        let mut p = self.theme.palette();
        if let Some(ref acc_str) = self.custom_accent_color {
            if acc_str == "system" || acc_str == "gtk4" {
                p.apply_accent(crate::theme::current_gtk4_accent_color());
            } else if let Some(c) = crate::theme::parse_hex_color(acc_str) {
                p.apply_accent(c);
            }
        }
        p
    }

    #[inline]
    pub fn sensors_accent_color(&self, p: &crate::theme::ThemePalette) -> Color32 {
        p.accent
    }

    pub fn host_display_name(&self, host_id: Option<&str>) -> String {
        if let Some(hid) = host_id {
            self.ssh_hosts
                .iter()
                .find(|h| h.id == hid)
                .map(|h| h.name.clone())
                .unwrap_or_else(|| hid.to_string())
        } else {
            "Local Host".to_string()
        }
    }























}

include!("adb_view.rs");
