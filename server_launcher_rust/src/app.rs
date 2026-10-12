use crate::config::{ConfigFile, CustomScript, SavedCommand, ServerConfig, SshRemoteHost};
use crate::modals::{
    render_add_edit_modal, render_command_modal, render_command_palette,
    render_close_confirmation_modal, render_new_service_modal, render_script_modal,
    render_ssh_host_modal, render_remote_start_server_modal, render_sudo_modal, AddEditModalState, CloseConfirmChoice,
    CommandPaletteState, ModalAction, NewServiceModalState, PaletteAction,
    RemoteStartServerModalState, SshHostModalState, SudoModalState,
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
    build_remote_stop_cmd, build_ssh_command,
    clean_service_name, fetch_remote_listeners, guess_remote_start_cmd,
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
}

impl SidebarTab {
    pub fn label(&self) -> &'static str {
        match self {
            SidebarTab::Servers => "\u{e153} Servers",
            SidebarTab::Scripts => "\u{e2ed} Scripts",
            SidebarTab::Commands => "\u{e181} Cmds",
            SidebarTab::SystemMD => "\u{e245} SystemD",
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
    System,
    TopApps,
}

/// Represents a local systemd action that requires sudo.
#[derive(Clone, Debug)]
pub enum PendingSudoAction {
    /// control_local_service(unit, action, user_mode)
    ServiceAction { unit: String, action: &'static str, user_mode: bool },
    /// create_local_service(def)
    CreateService { def: crate::systemd::NewServiceDefinition },
    /// write_local_unit_file(path, content, user_mode)
    SaveUnitFile { path: String, content: String, user_mode: bool },
    /// delete_local_drop_in(path, user_mode)
    DeleteDropIn { path: String, user_mode: bool },
}

/// State for the sudo-password popup dialog.
#[derive(Clone, Debug)]
pub struct SudoPopupState {
    pub open: bool,
    pub pending: Option<PendingSudoAction>,
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
            password_input: String::new(),
            remember_password: true,
            error: None,
            just_opened: false,
        }
    }
}

pub struct LauncherApp {
    pub active_sidebar_tab: SidebarTab,
    pub scripts: Vec<CustomScript>,
    pub saved_commands: Vec<SavedCommand>,
    config_path: PathBuf,
    services: Vec<Service>,
    active_tab: String, // "overview" or service.key
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

    // App header icon texture
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
    ssh_host_modal: SshHostModalState,
    remote_listeners: Arc<Mutex<HashMap<String, Vec<RemoteListener>>>>,
    remote_scanning: Arc<Mutex<BTreeSet<String>>>,
    remote_terminal_logs: Arc<Mutex<HashMap<String, Arc<Mutex<VecDeque<crate::service::LogEntry>>>>>>,
    remote_terminal_inputs: HashMap<String, String>,
    remote_connection_status: Arc<Mutex<HashMap<String, Result<String, String>>>>,
    remote_connecting: Arc<Mutex<BTreeSet<String>>>,
    pub remote_stopped_servers: Arc<Mutex<HashMap<String, Vec<RemoteStoppedServer>>>>,
    pub remote_start_server_modal: RemoteStartServerModalState,
    pub theme: ThemeMode,
    pub last_applied_palette: Option<crate::theme::ThemePalette>,
    pub app_autostart: bool,
    pub auto_scroll: HashMap<String, bool>,
    proc_info_cache: HashMap<u32, (Option<String>, Option<String>, Instant)>,
    remote_search_query: HashMap<String, String>,
    // Close confirmation dialog state
    close_confirm_open: bool,
    close_confirmed: bool,

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
    pub service_journal_logs: String,
    pub local_journal_logs: Arc<Mutex<HashMap<String, String>>>,
    pub local_journal_in_flight: Arc<Mutex<HashSet<String>>>,
    pub remote_journal_logs: Arc<Mutex<HashMap<String, String>>>,
    pub remote_journal_in_flight: Arc<Mutex<HashSet<String>>>,
    pub remote_service_action_in_flight: Arc<Mutex<HashSet<String>>>,
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
    pub systemd_file_content_cache: Arc<Mutex<HashMap<String, String>>>,
    pub systemd_file_content_in_flight: Arc<Mutex<HashSet<String>>>,
    pub systemd_selected_file: Option<String>,
    pub systemd_file_content: String,
    pub systemd_file_original: String,
    pub systemd_file_status_msg: Option<(String, bool)>,
    pub systemd_new_dropin_open: bool,
    pub systemd_new_dropin_name: String,
    pub systemd_new_dropin_template: usize,
    pub systemd_deps_cache: Arc<Mutex<HashMap<String, (crate::systemd::UnitDependenciesInfo, Instant)>>>,
    pub systemd_deps_in_flight: Arc<Mutex<HashSet<String>>>,
    pub systemd_deps_history: Vec<String>,
    pub systemd_deps_filter: String,
    pub systemd_deps_view_mode: usize,

    // Sensors State
    pub sensors_collector: crate::sensors::SystemSensorsCollector,
    pub sensors_snapshot: crate::sensors::SystemSensorsSnapshot,
    pub sensors_refresh_interval: u32,
    pub last_sensors_refresh: Instant,
    pub remote_sensors_snapshots: Arc<Mutex<HashMap<String, crate::sensors::SystemSensorsSnapshot>>>,
    pub remote_sensors_in_flight: Arc<Mutex<HashSet<String>>>,
    pub remote_sensors_last_fetch: Arc<Mutex<HashMap<String, Instant>>>,
    pub remote_sensors_errors: Arc<Mutex<HashMap<String, String>>>,
    pub sensors_search_query: String,
    pub sensors_category_filter: String,
    pub sensors_top_apps_sort: String,
    pub sensors_top_apps_sort_desc: bool,
    pub sensors_top_apps_group_by_app: bool,
    pub sensors_top_apps_expanded_groups: HashSet<String>,
    pub sensors_top_apps_selected_pid: Option<u32>,
    pub sensors_top_apps_normalize_cpu: bool,
    pub sensors_top_apps_status_msg: Option<(String, bool, Instant)>,
    pub sensors_subtab: SensorsSubTab,
    pub sensors_top_apps_visited: bool,

    // Sudo popup for local system-level systemd operations
    pub sudo_popup: SudoPopupState,
    pub local_sudo_password: String, // remembered for the session

    // Import status notification: (message, is_error, timestamp)
    pub import_status_msg: Option<(String, bool, Instant)>,

    // Settings window open state
    pub settings_modal_open: bool,
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
    let size = egui::vec2(32.0, 20.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let hovered = resp.hovered();

    let (bg_color, stroke_color) = match kind {
        WindowButtonKind::Close => {
            if hovered {
                (palette.danger, Color32::WHITE)
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
        ui.painter().rect_filled(rect, Rounding::same(4.0), bg_color);
    }

    let painter = ui.painter();
    let center = rect.center();
    let stroke = Stroke::new(1.2_f32, stroke_color);

    match kind {
        WindowButtonKind::Minimize => {
            let y = center.y + 1.0;
            painter.line_segment(
                [egui::pos2(center.x - 5.0, y), egui::pos2(center.x + 5.0, y)],
                stroke,
            );
        }
        WindowButtonKind::Maximize(maximized) => {
            if maximized {
                // Restore icon: two overlapping squares
                let back_top_left = egui::pos2(center.x - 2.5, center.y - 4.5);
                let back_top_right = egui::pos2(center.x + 4.5, center.y - 4.5);
                let back_bottom_right = egui::pos2(center.x + 4.5, center.y + 2.5);
                painter.line_segment([back_top_left, back_top_right], stroke);
                painter.line_segment([back_top_right, back_bottom_right], stroke);

                let front_rect = egui::Rect::from_min_size(
                    egui::pos2(center.x - 4.5, center.y - 2.5),
                    egui::vec2(7.0, 7.0),
                );
                painter.rect_stroke(front_rect, Rounding::ZERO, stroke);
            } else {
                // Maximize icon: crisp square outline
                let square = egui::Rect::from_center_size(center, egui::vec2(9.5, 9.5));
                painter.rect_stroke(square, Rounding::ZERO, stroke);
            }
        }
        WindowButtonKind::Close => {
            let d = 4.5;
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

    resp.on_hover_text(match kind {
        WindowButtonKind::Minimize => "Minimize",
        WindowButtonKind::Maximize(true) => "Restore",
        WindowButtonKind::Maximize(false) => "Maximize",
        WindowButtonKind::Close => "Close",
    })
    .clicked()
}

impl LauncherApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config_path = ConfigFile::default_path();
        let config_file = ConfigFile::load_from_file(&config_path);

        let mut services: Vec<Service> = config_file
            .servers
            .into_iter()
            .map(Service::new)
            .collect();

        // 🚀 Detect previously running servers and reconnect
        let active_listeners = crate::scanner::scan_system_listening_ports();
        let mut port_to_listener: HashMap<u16, &crate::scanner::ListenerInfo> = HashMap::new();
        for l in &active_listeners {
            port_to_listener.insert(l.port, l);
        }

        for s in &mut services {
            let pid_file = std::env::temp_dir().join(format!("launcher_{}.pid", s.config.key));
            let mut detected_pid: Option<u32> = None;
            let mut matched_by_port = false;

            if let Ok(content) = std::fs::read_to_string(&pid_file) {
                if let Ok(p) = content.trim().parse::<u32>() {
                    #[cfg(unix)]
                    let alive = unsafe { libc::kill(p as i32, 0) == 0 };
                    #[cfg(not(unix))]
                    let alive = true;

                    if alive {
                        detected_pid = Some(p);
                    } else {
                        let _ = std::fs::remove_file(&pid_file);
                    }
                }
            }

            if detected_pid.is_none() && s.config.port > 0 {
                if let Some(l) = port_to_listener.get(&s.config.port) {
                    if let Some(p) = l.pid {
                        #[cfg(unix)]
                        let alive = unsafe { libc::kill(p as i32, 0) == 0 };
                        #[cfg(not(unix))]
                        let alive = true;

                        if alive {
                            detected_pid = Some(p);
                            matched_by_port = true;
                        }
                    }
                }
            }

            if let Some(pid) = detected_pid {
                s.attach_existing(pid, matched_by_port);
            } else if s.config.autostart {
                s.start();
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
                    egui::TextureOptions::LINEAR,
                ))
            } else {
                None
            }
        };

        let selected_machine = "local".to_string();
        let ssh_hosts = config_file.ssh_hosts;
        let scripts = config_file.scripts;
        let saved_commands = config_file.saved_commands;
        let ssh_host_modal = SshHostModalState::default();
        let remote_listeners = Arc::new(Mutex::new(HashMap::new()));
        let remote_scanning = Arc::new(Mutex::new(BTreeSet::new()));
        let remote_terminal_logs = Arc::new(Mutex::new(HashMap::new()));
        let remote_terminal_inputs = HashMap::new();
        let remote_connection_status = Arc::new(Mutex::new(HashMap::new()));
        let remote_connecting = Arc::new(Mutex::new(BTreeSet::new()));
        let theme = config_file
            .theme
            .as_deref()
            .map(ThemeMode::from_id_str)
            .unwrap_or_default();

        let mut script_executions = HashMap::new();
        for sc in &scripts {
            if sc.autostart && sc.host_id.is_none() {
                let mut exec = ScriptExecution::new(sc.id.clone());
                execute_local_script(sc, &mut exec, None);
                script_executions.insert(sc.id.clone(), exec);
            }
        }

        Self {
            active_sidebar_tab: SidebarTab::Servers,
            scripts,
            saved_commands,
            config_path,
            services,
            active_tab: "overview".to_string(),
            active_group_filter: "All".to_string(),
            lan_ip: get_lan_ip(),
            sys: System::new(),
            scanner,
            last_poll: Instant::now(),
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
            icon_texture,
            show_other_process_metrics: false,
            other_metrics: HashMap::new(),
            last_other_poll: Instant::now(),
            open_inline_terminals: BTreeSet::new(),
            inline_terminal_sessions: HashMap::new(),
            inline_terminal_inputs: HashMap::new(),
            selected_machine,
            ssh_hosts,
            ssh_host_modal,
            remote_listeners,
            remote_scanning,
            remote_terminal_logs,
            remote_terminal_inputs,
            remote_connection_status,
            remote_connecting,
            remote_stopped_servers: Arc::new(Mutex::new(HashMap::new())),
            remote_start_server_modal: RemoteStartServerModalState::default(),
            theme,
            last_applied_palette: None,
            app_autostart,
            auto_scroll: HashMap::new(),
            proc_info_cache: HashMap::new(),
            remote_search_query: HashMap::new(),
            close_confirm_open: false,
            close_confirmed: false,

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
            service_journal_logs: String::new(),
            local_journal_logs: Arc::new(Mutex::new(HashMap::new())),
            local_journal_in_flight: Arc::new(Mutex::new(HashSet::new())),
            remote_journal_logs: Arc::new(Mutex::new(HashMap::new())),
            remote_journal_in_flight: Arc::new(Mutex::new(HashSet::new())),
            remote_service_action_in_flight: Arc::new(Mutex::new(HashSet::new())),
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
            systemd_file_content_in_flight: Arc::new(Mutex::new(HashSet::new())),
            systemd_selected_file: None,
            systemd_file_content: String::new(),
            systemd_file_original: String::new(),
            systemd_file_status_msg: None,
            systemd_new_dropin_open: false,
            systemd_new_dropin_name: "override.conf".to_string(),
            systemd_new_dropin_template: 0,
            systemd_deps_cache: Arc::new(Mutex::new(HashMap::new())),
            systemd_deps_in_flight: Arc::new(Mutex::new(HashSet::new())),
            systemd_deps_history: Vec::new(),
            systemd_deps_filter: String::new(),
            systemd_deps_view_mode: 0,

            sensors_collector: crate::sensors::SystemSensorsCollector::new(),
            sensors_snapshot: crate::sensors::SystemSensorsSnapshot::default(),
            sensors_refresh_interval: 2,
            last_sensors_refresh: Instant::now() - Duration::from_secs(10),
            remote_sensors_snapshots: Arc::new(Mutex::new(HashMap::new())),
            remote_sensors_in_flight: Arc::new(Mutex::new(HashSet::new())),
            remote_sensors_last_fetch: Arc::new(Mutex::new(HashMap::new())),
            remote_sensors_errors: Arc::new(Mutex::new(HashMap::new())),
            sensors_search_query: String::new(),
            sensors_category_filter: "All".to_string(),
            sensors_top_apps_sort: "cpu".to_string(),
            sensors_top_apps_sort_desc: true,
            sensors_top_apps_group_by_app: true,
            sensors_top_apps_expanded_groups: HashSet::new(),
            sensors_top_apps_selected_pid: None,
            sensors_top_apps_normalize_cpu: true,
            sensors_top_apps_status_msg: None,
            sensors_subtab: SensorsSubTab::System,
            sensors_top_apps_visited: false,
            sudo_popup: SudoPopupState::default(),
            local_sudo_password: String::new(),
            import_status_msg: None,
            settings_modal_open: false,
        }
    }

    pub fn trigger_remote_sensors_query(&self, host: SshRemoteHost) {
        let hid = host.id.clone();
        {
            let mut in_flight = self.remote_sensors_in_flight.lock().unwrap();
            if in_flight.contains(&hid) {
                return;
            }
            in_flight.insert(hid.clone());
        }

        let snapshots_arc = self.remote_sensors_snapshots.clone();
        let in_flight_arc = self.remote_sensors_in_flight.clone();
        let last_fetch_arc = self.remote_sensors_last_fetch.clone();
        let errors_arc = self.remote_sensors_errors.clone();

        std::thread::spawn(move || {
            match crate::sensors::query_remote_sensors(&host) {
                Ok(snap) => {
                    snapshots_arc.lock().unwrap().insert(hid.clone(), snap);
                    last_fetch_arc.lock().unwrap().insert(hid.clone(), Instant::now());
                    errors_arc.lock().unwrap().remove(&hid);
                }
                Err(e) => {
                    errors_arc.lock().unwrap().insert(hid.clone(), e);
                }
            }
            in_flight_arc.lock().unwrap().remove(&hid);
        });
    }

    fn persist_config(&self) {
        let cfg = ConfigFile {
            servers: self.services.iter().map(|s| s.config.clone()).collect(),
            ssh_hosts: self.ssh_hosts.clone(),
            scripts: self.scripts.clone(),
            saved_commands: self.saved_commands.clone(),
            theme: Some(self.theme.id_str().to_string()),
            app_autostart: self.app_autostart,
            journal_refresh_interval: Some(self.journal_refresh_interval),
        };
        let _ = cfg.save_to_file(&self.config_path);
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
            .set_title("Export servers.json Configuration")
            .set_file_name("servers.json")
            .add_filter("JSON Configuration (*.json)", &["json"])
            .save_file();

        if let Some(path) = file_opt {
            let servers: Vec<_> = self.services.iter().map(|s| s.config.clone()).collect();
            match serde_json::to_string_pretty(&servers) {
                Ok(json_str) => {
                    match std::fs::write(&path, json_str) {
                        Ok(_) => {
                            let fname = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "servers.json".to_string());
                            self.import_status_msg = Some((
                                format!("{} Exported {} server(s) to {}", crate::lucide::CHECK, servers.len(), fname),
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

        // 1. Try parsing full ConfigFile
        if let Ok(cfg) = serde_json::from_str::<ConfigFile>(trimmed) {
            if !cfg.servers.is_empty() || !cfg.ssh_hosts.is_empty() || !cfg.scripts.is_empty() || !cfg.saved_commands.is_empty() {
                for s in cfg.servers {
                    add_server(s, &mut self.services, &mut imported, &mut updated);
                }
                for h in cfg.ssh_hosts {
                    if !self.ssh_hosts.iter().any(|existing| existing.id == h.id) {
                        self.ssh_hosts.push(h);
                    }
                }
                for sc in cfg.scripts {
                    if !self.scripts.iter().any(|existing| existing.id == sc.id) {
                        self.scripts.push(sc);
                    }
                }
                for cmd in cfg.saved_commands {
                    if !self.saved_commands.iter().any(|existing| existing.id == cmd.id) {
                        self.saved_commands.push(cmd);
                    }
                }
                return Ok((imported, updated));
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
            let pid_str = pid.map(|p| p.to_string()).unwrap_or_else(|| "N/A".to_string());
            let entry_welcome = crate::service::LogEntry::new(
                format!("[terminal] Attached to {} (Port :{}, PID {})", name, port, pid_str),
                crate::service::LogKind::Launcher,
            );
            if let Ok(mut l) = logs.lock() {
                l.push_back(entry_welcome);
                if !cwd_str.is_empty() {
                    l.push_back(crate::service::LogEntry::new(
                        format!("[terminal] CWD: {}", cwd_str),
                        crate::service::LogKind::Launcher,
                    ));
                }
                if !cmd_str.is_empty() {
                    l.push_back(crate::service::LogEntry::new(
                        format!("[terminal] Command: {}", cmd_str),
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

    fn render_inline_terminal(
        &mut self,
        ui: &mut egui::Ui,
        port: u16,
        _name: &str,
        pid: Option<u32>,
        cwd: &str,
    ) {
        let p = self.theme.palette();
        Frame::none()
            .fill(p.terminal_bg)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(12.0))
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("● Live Terminal").size(11.5).strong().color(p.success));
                    let pid_str = pid.map(|p| p.to_string()).unwrap_or_else(|| "N/A".to_string());
                    ui.label(RichText::new(format!("Port :{} | PID {}", port, pid_str)).size(10.5).color(p.text_muted));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(format!("{} Close", crate::lucide::X)).clicked() {
                            self.open_inline_terminals.remove(&port);
                        }
                        if ui.button(format!("{} Refresh Logs", crate::lucide::REFRESH_CW)).on_hover_text("Fetch latest systemd/journald process output").clicked() {
                            self.refresh_inline_logs(port, pid, cwd);
                        }
                        if ui.button(format!("{} Clear", crate::lucide::TRASH)).clicked() {
                            self.clear_inline_logs(port);
                        }
                    });
                });
                ui.separator();

                ScrollArea::both()
                    .stick_to_bottom(true)
                    .max_height(200.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if let Some(session) = self.inline_terminal_sessions.get(&port) {
                            if let Ok(logs) = session.logs.lock() {
                                if logs.is_empty() {
                                    ui.label(
                                        RichText::new("No captured output yet. Type a shell command below or click 'Refresh Logs'.")
                                            .italics()
                                            .size(11.0)
                                            .color(p.text_muted),
                                    );
                                } else {
                                    for entry in logs.iter() {
                                        let color = match entry.kind {
                                            crate::service::LogKind::Stderr | crate::service::LogKind::Error => p.log_stderr,
                                            crate::service::LogKind::Warning => p.log_warning,
                                            crate::service::LogKind::Launcher => p.log_launcher,
                                            crate::service::LogKind::Stdin => p.log_stdin,
                                            crate::service::LogKind::Stdout => p.log_stdout,
                                        };
                                        let rt = RichText::new(format!("[{}] {}", entry.timestamp, entry.text))
                                            .color(color)
                                            .monospace()
                                            .size(10.5);
                                        ui.add(egui::Label::new(rt).wrap());
                                    }
                                }
                            }
                        }
                    });

                ui.add_space(4.0);
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label(RichText::new(">").monospace().strong().color(p.accent));
                    let input_ref = self.inline_terminal_inputs.entry(port).or_default();
                    let resp = ui.add(
                        egui::TextEdit::singleline(input_ref)
                            .hint_text("Run shell command in directory (e.g. ps, ls -la, tail log, curl)...")
                            .desired_width(ui.available_width() - 65.0)
                            .font(egui::TextStyle::Monospace),
                    );

                    let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (enter_pressed || ui.button("Run").clicked()) && !input_ref.trim().is_empty() {
                        let cmd = input_ref.trim().to_string();
                        input_ref.clear();
                        self.run_inline_command(port, &cmd, cwd);
                        resp.request_focus();
                    }
                });
            });
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

    fn ensure_remote_connected(&self, host: &SshRemoteHost) {
        let is_testing = {
            let connecting = self.remote_connecting.lock().unwrap();
            connecting.contains(&host.id)
        };
        let has_status = {
            let status = self.remote_connection_status.lock().unwrap();
            status.contains_key(&host.id)
        };

        if !is_testing && !has_status {
            self.test_remote_host(host);
        }
    }

    fn test_remote_host(&self, host: &SshRemoteHost) {
        {
            let mut connecting = self.remote_connecting.lock().unwrap();
            connecting.insert(host.id.clone());
        }
        let host_clone = host.clone();
        let status_arc = self.remote_connection_status.clone();
        let connecting_arc = self.remote_connecting.clone();

        std::thread::spawn(move || {
            let res = test_ssh_connection(&host_clone);
            if let Ok(mut status) = status_arc.lock() {
                status.insert(host_clone.id.clone(), res);
            }
            if let Ok(mut connecting) = connecting_arc.lock() {
                connecting.remove(&host_clone.id);
            }
        });
    }

    fn trigger_remote_scan(&self, host: &SshRemoteHost) {
        let is_scanning = {
            let scanning = self.remote_scanning.lock().unwrap();
            scanning.contains(&host.id)
        };
        if is_scanning {
            return;
        }

        {
            let mut scanning = self.remote_scanning.lock().unwrap();
            scanning.insert(host.id.clone());
        }

        let host_clone = host.clone();
        let listeners_arc = self.remote_listeners.clone();
        let scanning_arc = self.remote_scanning.clone();

        std::thread::spawn(move || {
            let res = fetch_remote_listeners(&host_clone);
            if let Ok(mut map) = listeners_arc.lock() {
                // Ensure entry is inserted even on error to prevent runaway reconnect loop
                map.insert(host_clone.id.clone(), res.unwrap_or_default());
            }
            if let Ok(mut scanning) = scanning_arc.lock() {
                scanning.remove(&host_clone.id);
            }
        });
    }

    fn trigger_remote_services_list(&self, host: &SshRemoteHost, user_mode: bool) {
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

        std::thread::spawn(move || {
            match list_remote_services(&host_clone, user_mode) {
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

    fn trigger_remote_journal_fetch(&self, host: &SshRemoteHost, unit: &str, user_mode: bool) {
        let key = format!("{}:{}:{}", host.id, unit, if user_mode { "user" } else { "system" });
        {
            let mut in_flight = self.remote_journal_in_flight.lock().unwrap();
            if in_flight.contains(&key) {
                return;
            }
            in_flight.insert(key.clone());
        }

        let host_clone = host.clone();
        let unit_clone = unit.to_string();
        let key_clone = key.clone();
        let journal_arc = self.remote_journal_logs.clone();
        let in_flight_arc = self.remote_journal_in_flight.clone();

        std::thread::spawn(move || {
            let logs = fetch_remote_journal(&host_clone, &unit_clone, user_mode, 100).unwrap_or_default();
            if let Ok(mut map) = journal_arc.lock() {
                map.insert(key_clone.clone(), logs);
            }
            if let Ok(mut in_flight) = in_flight_arc.lock() {
                in_flight.remove(&key_clone);
            }
        });
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

        std::thread::spawn(move || {
            if let Ok(s) = list_local_services(user_mode) {
                if let Ok(mut map) = cache_arc.lock() {
                    map.insert(user_mode, s);
                }
            }
            if let Ok(mut in_flight) = in_flight_arc.lock() {
                in_flight.remove(&user_mode);
            }
        });
    }

    pub fn trigger_local_journal_fetch(&self, unit: &str, user_mode: bool, sudo_password: Option<&str>) {
        let key = format!("{}:{}", unit, if user_mode { "user" } else { "system" });
        {
            let mut in_flight = self.local_journal_in_flight.lock().unwrap();
            if in_flight.contains(&key) {
                return;
            }
            in_flight.insert(key.clone());
        }

        let unit_clone = unit.to_string();
        let key_clone = key.clone();
        let pass_clone = sudo_password.map(|p| p.to_string());
        let logs_arc = self.local_journal_logs.clone();
        let in_flight_arc = self.local_journal_in_flight.clone();

        std::thread::spawn(move || {
            let logs = fetch_local_journal(&unit_clone, user_mode, 100, pass_clone.as_deref()).unwrap_or_default();
            if let Ok(mut map) = logs_arc.lock() {
                map.insert(key_clone.clone(), logs);
            }
            if let Ok(mut in_flight) = in_flight_arc.lock() {
                in_flight.remove(&key_clone);
            }
        });
    }

    pub fn trigger_systemd_unit_files_fetch(&self, unit: &str, user_mode: bool, host_opt: Option<&SshRemoteHost>) {
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

        std::thread::spawn(move || {
            let info = if let Some(host) = &host_clone {
                crate::systemd::get_remote_unit_files(host, &unit_clone, user_mode).unwrap_or_default()
            } else {
                crate::systemd::get_local_unit_files(&unit_clone, user_mode)
            };
            if let Ok(mut map) = files_arc.lock() {
                map.insert(cache_key_clone.clone(), (info, Instant::now()));
            }
            if let Ok(mut in_flight) = in_flight_arc.lock() {
                in_flight.remove(&cache_key_clone);
            }
        });
    }

    pub fn trigger_systemd_file_read(&self, path: &str, _user_mode: bool, host_opt: Option<&SshRemoteHost>, sudo_pass: Option<&str>) {
        let cache_key = format!("{}:{}", host_opt.map(|h| h.id.as_str()).unwrap_or("local"), path);
        {
            let mut in_flight = self.systemd_file_content_in_flight.lock().unwrap();
            if in_flight.contains(&cache_key) {
                return;
            }
            in_flight.insert(cache_key.clone());
        }

        let path_clone = path.to_string();
        let host_clone = host_opt.cloned();
        let pass_clone = sudo_pass.map(|p| p.to_string());
        let cache_key_clone = cache_key.clone();
        let content_arc = self.systemd_file_content_cache.clone();
        let in_flight_arc = self.systemd_file_content_in_flight.clone();

        std::thread::spawn(move || {
            let content = if let Some(host) = &host_clone {
                crate::systemd::read_remote_unit_file(host, &path_clone).unwrap_or_default()
            } else {
                crate::systemd::read_local_unit_file(&path_clone, pass_clone.as_deref()).unwrap_or_default()
            };
            if let Ok(mut map) = content_arc.lock() {
                map.insert(cache_key_clone.clone(), content);
            }
            if let Ok(mut in_flight) = in_flight_arc.lock() {
                in_flight.remove(&cache_key_clone);
            }
        });
    }

    pub fn trigger_systemd_unit_dependencies_fetch(&self, unit: &str, user_mode: bool, host_opt: Option<&SshRemoteHost>) {
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

        std::thread::spawn(move || {
            let deps = if let Some(host) = &host_clone {
                crate::systemd::get_remote_unit_dependencies(host, &unit_clone, user_mode).unwrap_or_default()
            } else {
                crate::systemd::get_local_unit_dependencies(&unit_clone, user_mode)
            };
            if let Ok(mut map) = deps_arc.lock() {
                map.insert(cache_key_clone.clone(), (deps, Instant::now()));
            }
            if let Ok(mut in_flight) = in_flight_arc.lock() {
                in_flight.remove(&cache_key_clone);
            }
        });
    }

    pub fn select_systemd_unit(&mut self, unit: &str, user_mode: bool, host_opt: Option<&SshRemoteHost>) {
        self.selected_service = Some(unit.to_string());
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
            let pass_opt = if !user_mode && !self.local_sudo_password.is_empty() {
                Some(self.local_sudo_password.as_str())
            } else {
                None
            };
            self.trigger_local_journal_fetch(unit, user_mode, pass_opt);
        }

        // 2. Pre-fetch unit files & dependencies in the background for zero-lag tab switches!
        self.trigger_systemd_unit_files_fetch(unit, user_mode, host_opt);
        self.trigger_systemd_unit_dependencies_fetch(unit, user_mode, host_opt);
    }

    fn trigger_remote_service_action(&self, host: &SshRemoteHost, unit: &str, action: &'static str, user_mode: bool) {
        let action_key = format!("{}:{}", host.id, unit);
        {
            let mut in_flight = self.remote_service_action_in_flight.lock().unwrap();
            if in_flight.contains(&action_key) {
                return;
            }
            in_flight.insert(action_key.clone());
        }

        let host_clone = host.clone();
        let unit_clone = unit.to_string();
        let action_key_clone = action_key.clone();
        let in_flight_arc = self.remote_service_action_in_flight.clone();
        let services_arc = self.remote_services.clone();
        let journal_arc = self.remote_journal_logs.clone();
        let action_msg_arc = self.remote_service_action_msg.clone();

        std::thread::spawn(move || {
            let res = control_remote_service(&host_clone, &unit_clone, action, user_mode);
            let action_msg = match res {
                Ok(msg) => msg,
                Err(e) => format!("Error: {}", e),
            };
            if let Ok(mut m) = action_msg_arc.lock() {
                *m = Some(action_msg);
            }

            // Refresh services list for this host & scope
            let s_key = format!("{}:{}", host_clone.id, if user_mode { "user" } else { "system" });
            if let Ok(services) = list_remote_services(&host_clone, user_mode) {
                if let Ok(mut map) = services_arc.lock() {
                    map.insert(s_key, services);
                }
            }

            // Refresh journal logs
            let j_key = format!("{}:{}:{}", host_clone.id, unit_clone, if user_mode { "user" } else { "system" });
            let logs = fetch_remote_journal(&host_clone, &unit_clone, user_mode, 100).unwrap_or_default();
            if let Ok(mut map) = journal_arc.lock() {
                map.insert(j_key, logs);
            }

            if let Ok(mut in_flight) = in_flight_arc.lock() {
                in_flight.remove(&action_key_clone);
            }
        });
    }

    fn get_or_query_service_enabled(&mut self, unit: &str, user_mode: bool, host_opt: Option<&SshRemoteHost>) -> bool {
        let host_id_str = host_opt.map(|h| h.id.as_str()).unwrap_or("local");
        let cache_key = format!("{}:{}:{}", host_id_str, unit, if user_mode { "user" } else { "system" });
        if let Some((is_enabled, time)) = self.service_enabled_cache.get(&cache_key) {
            if time.elapsed() < Duration::from_secs(6) {
                return *is_enabled;
            }
        }

        let is_enabled = if let Some(host) = host_opt {
            let cmd_str = if user_mode {
                format!("uid=$(id -u); export XDG_RUNTIME_DIR=/run/user/$uid; export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$uid/bus; systemctl --user is-enabled {} 2>/dev/null", unit)
            } else {
                format!("systemctl is-enabled {} 2>/dev/null", unit)
            };
            let mut cmd = crate::remote::build_ssh_command(host, &cmd_str);
            if let Ok(out) = cmd.output() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
                s.contains("enabled")
            } else {
                false
            }
        } else {
            let mut cmd = std::process::Command::new("systemctl");
            if user_mode {
                cmd.arg("--user");
            }
            cmd.arg("is-enabled").arg(unit);
            if let Ok(out) = cmd.output() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
                s.contains("enabled")
            } else {
                false
            }
        };

        self.service_enabled_cache.insert(cache_key, (is_enabled, Instant::now()));
        is_enabled
    }

    fn get_remote_logs(&self, host_id: &str) -> Arc<Mutex<VecDeque<crate::service::LogEntry>>> {
        let mut map = self.remote_terminal_logs.lock().unwrap();
        if let Some(logs) = map.get(host_id) {
            logs.clone()
        } else {
            let logs = Arc::new(Mutex::new(VecDeque::with_capacity(1000)));
            logs.lock().unwrap().push_back(crate::service::LogEntry::new(
                "[ssh] Terminal initialized. Ready to execute remote commands.".to_string(),
                crate::service::LogKind::Launcher,
            ));
            map.insert(host_id.to_string(), logs.clone());
            logs
        }
    }

    fn run_remote_ssh_command(&self, host: &SshRemoteHost, cmd_str: &str, ctx: Option<egui::Context>) {
        let logs = self.get_remote_logs(&host.id);
        let prompt = format!("{}> {}", host.user, cmd_str);
        crate::service::push_log_line(&prompt, &logs, 1000, crate::service::LogKind::Stdin);
        if let Some(ref c) = ctx {
            c.request_repaint();
        }

        let host_clone = host.clone();
        let cmd_owned = cmd_str.to_string();
        let logs_clone = logs.clone();
        let ctx_clone = ctx.clone();

        std::thread::spawn(move || {
            let mut c = build_ssh_command(&host_clone, &cmd_owned);
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
                    if let Some(c) = ctx_clone {
                        c.request_repaint();
                    }
                }
                Err(e) => {
                    crate::service::push_log_line(
                        &format!("[error] Failed to start SSH process: {}", e),
                        &logs_clone,
                        1000,
                        crate::service::LogKind::Error,
                    );
                    if let Some(c) = ctx_clone {
                        c.request_repaint();
                    }
                }
            }
        });
    }

    fn schedule_remote_scan(&self, host: &SshRemoteHost, delay_ms: u64, ctx: egui::Context) {
        let host_clone = host.clone();
        let listeners_arc = self.remote_listeners.clone();
        let scanning_arc = self.remote_scanning.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            if let Ok(listeners) = crate::remote::fetch_remote_listeners(&host_clone) {
                if let Ok(mut map) = listeners_arc.lock() {
                    map.insert(host_clone.id.clone(), listeners);
                }
            }
            if let Ok(mut scanning) = scanning_arc.lock() {
                scanning.remove(&host_clone.id);
            }
            ctx.request_repaint();
        });
    }

    fn clear_remote_terminal_logs(&self, host_id: &str) {
        if let Ok(map) = self.remote_terminal_logs.lock() {
            if let Some(logs) = map.get(host_id) {
                if let Ok(mut l) = logs.lock() {
                    l.clear();
                }
            }
        }
    }

    fn render_machines_nav_bar(&mut self, ui: &mut Ui) {
        let p = self.theme.palette();

        let mut host_to_edit: Option<SshRemoteHost> = None;
        let mut host_to_delete: Option<String> = None;
        let mut host_to_test: Option<SshRemoteHost> = None;
        let mut host_to_term: Option<SshRemoteHost> = None;

        ui.vertical_centered(|ui| {
            ui.add_space(4.0);

            // 1. This PC (Local) Tile
            let is_local_selected = self.selected_machine == "local";
            let running_count = self
                .services
                .iter()
                .filter(|s| s.state == ServiceState::Running)
                .count();

            let local_tooltip = if running_count > 0 {
                format!("{} This PC (Local)\n● {} active server{} running", crate::lucide::MONITOR, running_count, if running_count > 1 { "s" } else { "" })
            } else {
                format!("{} This PC (Local)\nNo servers running", crate::lucide::MONITOR)
            };

            let (local_rect, local_resp) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::click());
            let local_bg = if is_local_selected {
                p.accent
            } else if local_resp.hovered() {
                p.bg_card_active
            } else {
                p.bg_card
            };
            let local_stroke = Stroke::NONE;

            ui.painter().rect(local_rect, Rounding::same(8.0), local_bg, local_stroke);
            let local_icon_col = if is_local_selected { Color32::WHITE } else { p.text_primary };
            ui.painter().text(
                local_rect.center(),
                egui::Align2::CENTER_CENTER,
                crate::lucide::MONITOR,
                egui::FontId::proportional(16.0),
                local_icon_col,
            );

            // Badge dot if running servers exist
            if running_count > 0 {
                let dot_center = egui::pos2(local_rect.right() - 6.0, local_rect.bottom() - 6.0);
                ui.painter().circle_filled(dot_center, 4.0, p.bg_machines_nav);
                ui.painter().circle_filled(dot_center, 2.5, p.success);
            }

            if local_resp.on_hover_text(local_tooltip).clicked() {
                self.selected_machine = "local".to_string();
            }

            ui.add_space(10.0);

            // 2. SSH Remote Hosts List (Scrollable)
            ScrollArea::vertical()
                .id_salt("ssh_hosts_dock_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        for host in &self.ssh_hosts {
                            let is_selected = self.selected_machine == host.id;
                            let status = {
                                let map = self.remote_connection_status.lock().unwrap();
                                map.get(&host.id).cloned()
                            };
                            let is_connecting = {
                                let map = self.remote_connecting.lock().unwrap();
                                map.contains(&host.id)
                            };

                            let (status_color, status_text) = if is_connecting {
                                (p.warning, "connecting…")
                            } else if let Some(Ok(_)) = &status {
                                (p.success, "online")
                            } else if let Some(Err(_)) = &status {
                                (p.danger, "error")
                            } else {
                                (p.text_muted, "idle")
                            };

                            let host_tooltip = format!(
                                "{} {}\n{}@{}:{}\nStatus: {}\nRight-click for options",
                                crate::lucide::GLOBE, host.name, host.user, host.host, host.port, status_text
                            );

                            let (host_rect, host_resp) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::click());
                            let host_bg = if is_selected {
                                p.accent
                            } else if host_resp.hovered() {
                                p.bg_card_active
                            } else {
                                p.bg_card
                            };
                            let host_stroke = Stroke::NONE;

                            ui.painter().rect(host_rect, Rounding::same(8.0), host_bg, host_stroke);
                            let host_icon_col = if is_selected { Color32::WHITE } else { p.text_secondary };
                            ui.painter().text(
                                host_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                crate::lucide::GLOBE,
                                egui::FontId::proportional(15.0),
                                host_icon_col,
                            );

                            // Status indicator dot at bottom-right
                            let dot_center = egui::pos2(host_rect.right() - 6.0, host_rect.bottom() - 6.0);
                            ui.painter().circle_filled(dot_center, 4.0, p.bg_machines_nav);
                            ui.painter().circle_filled(dot_center, 2.5, status_color);

                            let host_resp = host_resp.on_hover_text(host_tooltip);
                            if host_resp.clicked() {
                                self.selected_machine = host.id.clone();
                            }

                            host_resp.context_menu(|ui| {
                                if ui.button(format!("{} Edit Host", crate::lucide::SETTINGS)).clicked() {
                                    host_to_edit = Some(host.clone());
                                    ui.close_menu();
                                }
                                if ui.button(format!("{} Test Connection", crate::lucide::REFRESH_CW)).clicked() {
                                    host_to_test = Some(host.clone());
                                    ui.close_menu();
                                }
                                if ui.button(format!("{} Launch External Terminal", crate::lucide::EXTERNAL_LINK)).clicked() {
                                    host_to_term = Some(host.clone());
                                    ui.close_menu();
                                }
                                ui.separator();
                                if ui.button(RichText::new(format!("{} Delete Host", crate::lucide::TRASH)).color(Color32::from_rgb(239, 68, 68))).clicked() {
                                    host_to_delete = Some(host.id.clone());
                                    ui.close_menu();
                                }
                            });

                            ui.add_space(6.0);
                        }

                        // Add SSH Host button tile
                        let (add_rect, add_resp) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::click());
                        let add_bg = if add_resp.hovered() { p.bg_card_active } else { Color32::TRANSPARENT };
                        let add_stroke = Stroke::NONE;
                        ui.painter().rect(add_rect, Rounding::same(8.0), add_bg, add_stroke);
                        ui.painter().text(
                            add_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "+",
                            egui::FontId::proportional(17.0),
                            if add_resp.hovered() { p.accent } else { p.text_muted },
                        );
                        if add_resp.on_hover_text("Add new SSH Remote PC").clicked() {
                            self.ssh_host_modal.open_add();
                        }

                        ui.add_space(6.0);

                        // Import SSH button tile
                        let (imp_rect, imp_resp) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::click());
                        let imp_bg = if imp_resp.hovered() { p.bg_card_active } else { Color32::TRANSPARENT };
                        let imp_stroke = Stroke::NONE;
                        ui.painter().rect(imp_rect, Rounding::same(8.0), imp_bg, imp_stroke);
                        ui.painter().text(
                            imp_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            crate::lucide::DOWNLOAD,
                            egui::FontId::proportional(14.0),
                            if imp_resp.hovered() { p.accent } else { p.text_muted },
                        );
                        if imp_resp.on_hover_text("Import hosts from ~/.ssh/config").clicked() {
                            let imported = import_from_ssh_config();
                            let mut added = 0;
                            for inh in imported {
                                if !self.ssh_hosts.iter().any(|h| h.host == inh.host && h.port == inh.port && h.user == inh.user) {
                                    self.ssh_hosts.push(inh);
                                    added += 1;
                                }
                            }
                            if added > 0 {
                                self.persist_config();
                            }
                        }
                    });
                });
        });

        if let Some(h) = host_to_test {
            self.test_remote_host(&h);
        }
        if let Some(h) = host_to_term {
            let _ = launch_external_ssh_terminal(&h);
        }
        if let Some(h) = host_to_edit {
            self.ssh_host_modal.open_edit(&h);
        }
        if let Some(id) = host_to_delete {
            if let Some(pos) = self.ssh_hosts.iter().position(|h| h.id == id) {
                self.ssh_hosts.remove(pos);
                if self.selected_machine == id {
                    self.selected_machine = "local".to_string();
                }
                self.persist_config();
            }
        }
    }

    fn render_remote_pc_dashboard(&mut self, ui: &mut Ui, host_id: &str) {
        let host_opt = self.ssh_hosts.iter().find(|h| h.id == host_id).cloned();
        let host = match host_opt {
            Some(h) => h,
            None => {
                ui.label(RichText::new("Selected SSH host not found.").color(Color32::from_rgb(239, 68, 68)));
                return;
            }
        };

        // Ensure background connection check has started
        self.ensure_remote_connected(&host);

        let status = {
            let map = self.remote_connection_status.lock().unwrap();
            map.get(&host.id).cloned()
        };
        let is_scanning = {
            let map = self.remote_scanning.lock().unwrap();
            map.contains(&host.id)
        };

        // If connected and no scan triggered yet, trigger scan
        if let Some(Ok(_)) = &status {
            let has_scanned = {
                let map = self.remote_listeners.lock().unwrap();
                map.contains_key(&host.id)
            };
            if !has_scanned && !is_scanning {
                self.trigger_remote_scan(&host);
            }
        }

        ui.vertical(|ui| {
            // Trigger autostart scripts for this SSH host if connected
            if let Some(Ok(_)) = &status {
                for sc in &self.scripts {
                    if sc.autostart && sc.host_id.as_deref() == Some(&host.id) {
                        if !self.script_executions.contains_key(&sc.id) {
                            let mut exec = ScriptExecution::new(sc.id.clone());
                            execute_remote_script(&host, sc, &mut exec, None);
                            self.script_executions.insert(sc.id.clone(), exec);
                        }
                    }
                }
            }

            // Split layout: Left panel (width 380px with Tabs on top) + Right panel (Remaining width)
            let total_h = ui.available_height();
            ui.horizontal(|ui| {
                ui.set_height(total_h);
                ui.set_min_height(total_h);

                let left_w = 380.0;
                let right_w = (ui.available_width() - left_w - 12.0).max(300.0);

                // LEFT PANEL (380px)
                ui.allocate_ui_with_layout(egui::vec2(left_w, total_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(left_w);
                    ui.set_max_width(left_w);
                    ui.set_height(total_h);
                    ui.set_min_height(total_h);
                    // 1. Navigation Tabs on top of left panel
                    self.render_sidebar_tabs_bar(ui);
                    ui.add_space(6.0);

                    // 2. Left panel content for active tab
                    match self.active_sidebar_tab {
                        SidebarTab::Servers => self.render_remote_servers_left_panel(ui, &host),
                        SidebarTab::Scripts => self.render_scripts_left_panel(ui, Some(&host.id)),
                        SidebarTab::Commands => self.render_commands_left_panel(ui, Some(&host.id)),
                        SidebarTab::SystemMD => self.render_systemd_left_panel(ui, Some(&host.id)),
                        SidebarTab::Sensors => self.render_sensors_left_panel(ui, Some(&host.id)),
                    }
                });

                ui.separator();

                // RIGHT PANEL (Remaining width)
                ui.allocate_ui_with_layout(egui::vec2(right_w, total_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(right_w);
                    ui.set_max_width(right_w);
                    ui.set_height(total_h);
                    ui.set_min_height(total_h);
                    match self.active_sidebar_tab {
                        SidebarTab::Servers => self.render_remote_servers_right_panel(ui, &host),
                        SidebarTab::Scripts => self.render_scripts_right_panel(ui, Some(&host.id)),
                        SidebarTab::Commands => self.render_commands_right_panel(ui, Some(&host.id)),
                        SidebarTab::SystemMD => self.render_systemd_right_panel(ui, Some(&host.id)),
                        SidebarTab::Sensors => self.render_sensors_right_panel(ui, Some(&host.id)),
                    }
                });
            });
        });
    }

    fn render_remote_servers_left_panel(&mut self, ui: &mut Ui, host: &SshRemoteHost) {
        let p = self.theme.palette();
        let is_scanning = {
            let map = self.remote_scanning.lock().unwrap();
            map.contains(&host.id)
        };
        let listeners = {
            let map = self.remote_listeners.lock().unwrap();
            map.get(&host.id).cloned().unwrap_or_default()
        };

        // Sync stopped servers: prune any that are now active in listeners
        let stopped = {
            let mut map = self.remote_stopped_servers.lock().unwrap();
            let list = map.entry(host.id.clone()).or_default();
            list.retain(|s| !listeners.iter().any(|l| l.port == s.port));
            list.clone()
        };

        let mut dismiss_port: Option<u16> = None;

        ui.vertical(|ui| {
            // Header with title, counts, and action toolbar
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("REMOTE SERVERS")
                        .size(11.5)
                        .strong()
                        .color(Color32::from_rgb(148, 163, 184)),
                );
                let count_desc = if stopped.is_empty() {
                    format!("({})", listeners.len())
                } else {
                    format!("({} active, {} stopped)", listeners.len(), stopped.len())
                };
                ui.label(
                    RichText::new(count_desc)
                        .size(11.0)
                        .color(Color32::from_rgb(100, 110, 130)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;

                    // + Start Server button (accent pill)
                    let btn_add = egui::Button::new(
                        RichText::new("+ Start Server").size(10.5).strong().color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(0x05, 0x96, 0x69))
                    .rounding(Rounding::same(999.0));
                    if ui.add(btn_add).on_hover_text("Start a new server or process on remote host").clicked() {
                        self.remote_start_server_modal.open_for_host(host);
                    }
                });
            });
            ui.add_space(4.0);

            let search_q = self.remote_search_query.entry(host.id.clone()).or_default();
            ui.add(
                egui::TextEdit::singleline(search_q)
                    .hint_text(format!("{} Filter remote servers...", crate::lucide::SEARCH))
                    .desired_width(ui.available_width()),
            );
            ui.add_space(6.0);

            let q = self.remote_search_query.get(&host.id).cloned().unwrap_or_default().trim().to_lowercase();
            let visible_listeners: Vec<_> = listeners
                .iter()
                .filter(|l| {
                    if q.is_empty() {
                        true
                    } else {
                        l.name.to_lowercase().contains(&q)
                            || l.port.to_string().contains(&q)
                            || l.proto.to_lowercase().contains(&q)
                            || l.cmd.to_lowercase().contains(&q)
                            || l.pid.map(|p| p.to_string().contains(&q)).unwrap_or(false)
                    }
                })
                .cloned()
                .collect();

            let visible_stopped: Vec<_> = stopped
                .iter()
                .filter(|s| {
                    if q.is_empty() {
                        true
                    } else {
                        s.name.to_lowercase().contains(&q)
                            || s.port.to_string().contains(&q)
                            || s.proto.to_lowercase().contains(&q)
                            || s.cmd.to_lowercase().contains(&q)
                            || s.start_cmd.to_lowercase().contains(&q)
                            || s.last_pid.map(|p| p.to_string().contains(&q)).unwrap_or(false)
                    }
                })
                .cloned()
                .collect();

            if listeners.is_empty() && stopped.is_empty() {
                Frame::none()
                    .fill(p.bg_card)
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if is_scanning {
                            ui.label(RichText::new("Scanning remote listening ports (ss -tulpn)...").size(11.0).color(Color32::from_rgb(250, 204, 21)));
                        } else {
                            ui.label(RichText::new("No active listening ports detected on remote PC. Click 'Scan Ports' or '+ Start Server'.").size(11.0).color(Color32::from_rgb(120, 128, 145)));
                        }
                    });
            } else if visible_listeners.is_empty() && visible_stopped.is_empty() {
                ui.label(RichText::new(format!("No servers matching \"{}\"", q)).size(11.0).color(Color32::from_rgb(120, 128, 145)));
            } else {
                ScrollArea::vertical()
                    .id_salt(format!("remote_listeners_scroll_{}", host.id))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_max_width(ui.available_width());

                        // 1. ACTIVE RUNNING SERVERS
                        for l in &visible_listeners {
                            Frame::none()
                                .fill(p.bg_card)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(12.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.set_max_width(ui.available_width());
                                    // Row 1: Port badge + Proto + Name + Running Chip
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(format!(":{}", l.port))
                                                .size(12.5)
                                                .strong()
                                                .color(Color32::from_rgb(0, 210, 160)),
                                        );
                                        ui.label(
                                            RichText::new(format!("[{}]", l.proto))
                                                .size(10.5)
                                                .color(Color32::from_rgb(130, 138, 155)),
                                        );
                                        let name_w = (ui.available_width() - 75.0).max(40.0);
                                        ui.add_sized(
                                            [name_w, 16.0],
                                            egui::Label::new(
                                                RichText::new(&l.name)
                                                    .size(12.0)
                                                    .strong()
                                                    .color(Color32::from_rgb(230, 233, 240)),
                                            )
                                            .truncate(),
                                        );
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            Frame::none()
                                                .fill(Color32::from_rgba_unmultiplied(16, 185, 129, 30))
                                                .rounding(Rounding::same(999.0))
                                                .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                .show(ui, |ui| {
                                                    ui.label(RichText::new("● Running").size(9.5).color(Color32::from_rgb(52, 211, 153)));
                                                });
                                        });
                                    });

                                    // Row 2: PID, CPU, RAM, and command
                                    if let Some(pid) = l.pid {
                                        let mut desc = format!("PID {}", pid);
                                        if let Some(cpu) = l.cpu_pct {
                                            desc.push_str(&format!(" • {:.1}% CPU", cpu));
                                        }
                                        if let Some(mem) = l.mem_mb {
                                            desc.push_str(&format!(" • {:.1} MB", mem));
                                        }
                                        if !l.cmd.is_empty() {
                                            desc.push_str(&format!(" • {}", l.cmd));
                                        }
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(desc)
                                                    .size(10.0)
                                                    .color(Color32::from_rgb(120, 128, 145)),
                                            )
                                            .truncate(),
                                        );
                                    }

                                    ui.add_space(4.0);

                                    // Row 3: Action Buttons (Stop, Restart, Kill, Curl, Info)
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;

                                        // Stop Button
                                        let stop_btn = egui::Button::new(
                                            RichText::new(format!("{} Stop", crate::lucide::STOP)).size(10.0).strong().color(Color32::from_rgb(0xfe, 0xf2, 0xf2)),
                                        )
                                        .fill(Color32::from_rgb(0x99, 0x1b, 0x1b))
                                        .rounding(Rounding::same(999.0));
                                        if ui.add(stop_btn).on_hover_text("Stop this remote server gracefully").clicked() {
                                            let stop_cmd = build_remote_stop_cmd(&l.name, l.port, l.pid);
                                            let start_cmd = guess_remote_start_cmd(&l.name, l.port);
                                            if let Ok(mut map) = self.remote_stopped_servers.lock() {
                                                let list = map.entry(host.id.clone()).or_default();
                                                if !list.iter().any(|s| s.port == l.port) {
                                                    list.push(RemoteStoppedServer {
                                                        name: l.name.clone(),
                                                        port: l.port,
                                                        proto: l.proto.clone(),
                                                        last_pid: l.pid,
                                                        cmd: l.cmd.clone(),
                                                        start_cmd,
                                                    });
                                                }
                                            }
                                            self.run_remote_ssh_command(host, &stop_cmd, Some(ui.ctx().clone()));
                                            self.schedule_remote_scan(host, 1500, ui.ctx().clone());
                                        }

                                        // Restart Button
                                        let restart_btn = egui::Button::new(
                                            RichText::new(format!("{} Restart", crate::lucide::REFRESH_CW)).size(10.0).strong().color(Color32::from_rgb(0xec, 0xfe, 0xff)),
                                        )
                                        .fill(Color32::from_rgb(0x0e, 0x74, 0x90))
                                        .rounding(Rounding::same(999.0));
                                        if ui.add(restart_btn).on_hover_text("Restart this remote server").clicked() {
                                            let restart_cmd = build_remote_restart_cmd(&l.name, l.port, l.pid);
                                            self.run_remote_ssh_command(host, &restart_cmd, Some(ui.ctx().clone()));
                                            self.schedule_remote_scan(host, 2000, ui.ctx().clone());
                                        }

                                        // Force Kill Button
                                        let kill_btn = egui::Button::new(
                                            RichText::new(format!("{} Kill", crate::lucide::X)).size(10.0).color(Color32::from_rgb(254, 202, 202)),
                                        )
                                        .fill(Color32::from_rgb(45, 20, 25))
                                        .rounding(Rounding::same(999.0));
                                        if ui.add(kill_btn).on_hover_text("Force kill this process (-9) via SSH").clicked() {
                                            let kill_cmd = if let Some(pid) = l.pid {
                                                format!("kill -9 {}", pid)
                                            } else {
                                                format!("fuser -k -n tcp {} 2>/dev/null || true", l.port)
                                            };
                                            self.run_remote_ssh_command(host, &kill_cmd, Some(ui.ctx().clone()));
                                            self.schedule_remote_scan(host, 1500, ui.ctx().clone());
                                        }

                                        // Curl Button
                                        let test_btn = egui::Button::new(
                                            RichText::new(format!("{} Curl", crate::lucide::GLOBE)).size(10.0).color(Color32::from_rgb(56, 189, 248)),
                                        )
                                        .fill(Color32::from_rgb(24, 32, 48))
                                        .rounding(Rounding::same(999.0));
                                        if ui.add(test_btn).on_hover_text("Test HTTP response on remote host (curl -I)").clicked() {
                                            let curl_cmd = format!("curl -I -s --connect-timeout 2 http://localhost:{} | head -n 5", l.port);
                                            self.run_remote_ssh_command(host, &curl_cmd, Some(ui.ctx().clone()));
                                        }

                                        // Info Button
                                        let ps_btn = egui::Button::new(
                                            RichText::new(format!("{} Info", crate::lucide::INFO)).size(10.0).color(Color32::from_rgb(250, 204, 21)),
                                        )
                                        .fill(Color32::from_rgb(34, 30, 20))
                                        .rounding(Rounding::same(999.0));
                                        if ui.add(ps_btn).on_hover_text("Inspect process details on remote host").clicked() {
                                            let ps_cmd = if let Some(pid) = l.pid {
                                                format!("ps -u -p {} 2>/dev/null", pid)
                                            } else {
                                                format!("lsof -i :{} 2>/dev/null", l.port)
                                            };
                                            self.run_remote_ssh_command(host, &ps_cmd, Some(ui.ctx().clone()));
                                        }
                                    });
                                });
                            ui.add_space(4.0);
                        }

                        // 2. STOPPED SERVERS
                        for s in &visible_stopped {
                            Frame::none()
                                .fill(p.bg_card)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(12.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.set_max_width(ui.available_width());

                                    // Row 1: Port badge (muted) + Proto + Name + Stopped Chip
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(format!(":{}", s.port))
                                                .size(12.5)
                                                .strong()
                                                .color(Color32::from_rgb(148, 163, 184)),
                                        );
                                        ui.label(
                                            RichText::new(format!("[{}]", s.proto))
                                                .size(10.5)
                                                .color(Color32::from_rgb(100, 110, 130)),
                                        );
                                        let name_w = (ui.available_width() - 75.0).max(40.0);
                                        ui.add_sized(
                                            [name_w, 16.0],
                                            egui::Label::new(
                                                RichText::new(&s.name)
                                                    .size(12.0)
                                                    .strong()
                                                    .color(Color32::from_rgb(180, 185, 195)),
                                            )
                                            .truncate(),
                                        );
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            Frame::none()
                                                .fill(Color32::from_rgba_unmultiplied(239, 68, 68, 30))
                                                .rounding(Rounding::same(999.0))
                                                .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                .show(ui, |ui| {
                                                    ui.label(RichText::new("○ Stopped").size(9.5).color(Color32::from_rgb(248, 113, 113)));
                                                });
                                        });
                                    });

                                    // Row 2: Stopped details
                                    let desc = if let Some(last_pid) = s.last_pid {
                                        format!("Last PID {} • Offline", last_pid)
                                    } else {
                                        "Offline".to_string()
                                    };
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(desc)
                                                .size(10.0)
                                                .color(Color32::from_rgb(100, 110, 130)),
                                        )
                                        .truncate(),
                                    );

                                    ui.add_space(4.0);

                                    // Row 3: Action Buttons (Start, Restart, Dismiss, Info)
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;

                                        // Start Button
                                        let start_btn = egui::Button::new(
                                            RichText::new(format!("{} Start", crate::lucide::PLAY)).size(10.0).strong().color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(0x05, 0x96, 0x69))
                                        .rounding(Rounding::same(999.0));
                                        if ui.add(start_btn).on_hover_text("Start this remote server").clicked() {
                                            let start_cmd = build_remote_start_cmd(&s.name, s.port, if s.start_cmd.is_empty() { None } else { Some(&s.start_cmd) });
                                            self.run_remote_ssh_command(host, &start_cmd, Some(ui.ctx().clone()));
                                            self.schedule_remote_scan(host, 1500, ui.ctx().clone());
                                        }

                                        // Restart Button
                                        let restart_btn = egui::Button::new(
                                            RichText::new(format!("{} Restart", crate::lucide::REFRESH_CW)).size(10.0).strong().color(Color32::from_rgb(0xec, 0xfe, 0xff)),
                                        )
                                        .fill(Color32::from_rgb(0x0e, 0x74, 0x90))
                                        .rounding(Rounding::same(999.0));
                                        if ui.add(restart_btn).on_hover_text("Attempt to restart this remote server").clicked() {
                                            let start_cmd = build_remote_start_cmd(&s.name, s.port, if s.start_cmd.is_empty() { None } else { Some(&s.start_cmd) });
                                            self.run_remote_ssh_command(host, &start_cmd, Some(ui.ctx().clone()));
                                            self.schedule_remote_scan(host, 1500, ui.ctx().clone());
                                        }

                                        // Dismiss Button
                                        let dismiss_btn = egui::Button::new(
                                            RichText::new(format!("{} Dismiss", crate::lucide::X)).size(10.0).color(Color32::from_rgb(148, 163, 184)),
                                        )
                                        .fill(Color32::from_rgb(28, 30, 40))
                                        .rounding(Rounding::same(999.0));
                                        if ui.add(dismiss_btn).on_hover_text("Dismiss from stopped servers list").clicked() {
                                            dismiss_port = Some(s.port);
                                        }

                                        // Info Button
                                        let info_btn = egui::Button::new(
                                            RichText::new(format!("{} Info", crate::lucide::INFO)).size(10.0).color(Color32::from_rgb(250, 204, 21)),
                                        )
                                        .fill(Color32::from_rgb(34, 30, 20))
                                        .rounding(Rounding::same(999.0));
                                        if ui.add(info_btn).on_hover_text("Inspect status on remote host").clicked() {
                                            let info_cmd = format!("lsof -i :{} 2>/dev/null || systemctl status {} 2>/dev/null", s.port, clean_service_name(&s.name));
                                            self.run_remote_ssh_command(host, &info_cmd, Some(ui.ctx().clone()));
                                        }
                                    });
                                });
                            ui.add_space(4.0);
                        }
                    });
            }
        });

        // Apply any dismissals from stopped list
        if let Some(port) = dismiss_port {
            if let Ok(mut map) = self.remote_stopped_servers.lock() {
                if let Some(list) = map.get_mut(&host.id) {
                    list.retain(|s| s.port != port);
                }
            }
        }
    }

    fn render_remote_servers_right_panel(&mut self, ui: &mut Ui, host: &SshRemoteHost) {
        let p = self.theme.palette();
        ui.vertical(|ui| {
            // Interactive SSH Shell Terminal
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("REMOTE SSH TERMINAL")
                        .size(11.5)
                        .strong()
                        .color(Color32::from_rgb(148, 163, 184)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_clear = egui::Button::new(
                        RichText::new("Clear")
                            .size(11.0)
                            .color(Color32::from_rgb(160, 168, 185)),
                    )
                    .fill(Color32::from_rgb(25, 28, 38))
                    .rounding(Rounding::same(3.0));
                    if ui.add(btn_clear).clicked() {
                        self.clear_remote_terminal_logs(&host.id);
                    }

                    // Quick Command Buttons
                    let quick_cmds = [
                        ("df -h", "Disk Usage"),
                        ("free -m", "Memory Usage"),
                        ("uptime", "Uptime & Load"),
                        ("docker ps", "Docker Containers"),
                        ("ss -tulpn", "Listening Ports"),
                    ];
                    for (qcmd, qtip) in quick_cmds {
                        let btn = egui::Button::new(
                            RichText::new(qcmd)
                                .size(10.5)
                                .color(Color32::from_rgb(162, 155, 254)),
                        )
                        .fill(Color32::from_rgb(28, 30, 44))
                        .rounding(Rounding::same(3.0));
                        if ui.add(btn).on_hover_text(qtip).clicked() {
                            self.run_remote_ssh_command(host, qcmd, Some(ui.ctx().clone()));
                        }
                    }
                });
            });
            ui.add_space(4.0);

            // Integrated Dark Terminal Window (Output + Prompt at bottom, fills full available height)
            let total_term_h = (ui.available_height() - 4.0).max(200.0);
            let prompt_bar_h = 36.0;
            let scroll_h = (total_term_h - prompt_bar_h - 22.0).max(80.0);

            let term_logs = self.get_remote_logs(&host.id);

            Frame::none()
                .fill(p.terminal_bg)
                .stroke(Stroke::NONE)
                .rounding(Rounding::same(14.0))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(total_term_h);

                    // 1. Scrollable Output Log Area
                    ScrollArea::both()
                        .id_salt(format!("remote_term_scroll_{}", host.id))
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .max_height(scroll_h)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_min_height(scroll_h);
                            let logs_guard = term_logs.lock().unwrap();
                            for entry in logs_guard.iter() {
                                let (col, bg) = match entry.kind {
                                    crate::service::LogKind::Stdout => (Color32::from_rgb(220, 225, 235), Color32::TRANSPARENT),
                                    crate::service::LogKind::Stderr | crate::service::LogKind::Error => (Color32::from_rgb(255, 107, 107), Color32::TRANSPARENT),
                                    crate::service::LogKind::Stdin => (Color32::from_rgb(162, 155, 254), Color32::TRANSPARENT),
                                    crate::service::LogKind::Launcher => (Color32::from_rgb(0, 210, 160), Color32::TRANSPARENT),
                                    crate::service::LogKind::Warning => (Color32::from_rgb(250, 204, 21), Color32::TRANSPARENT),
                                };

                                let rt = RichText::new(&entry.text)
                                    .font(egui::FontId::monospace(11.5))
                                    .color(col)
                                    .background_color(bg);
                                ui.add(egui::Label::new(rt).wrap());
                            }
                        });

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // 2. Command Input Box anchored at bottom of terminal
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{}@{}:~$", host.user, host.host))
                                .font(egui::FontId::monospace(12.0))
                                .strong()
                                .color(Color32::from_rgb(0, 210, 160)),
                        );

                        let (cmd_to_run, execute) = {
                            let input_text = self.remote_terminal_inputs.entry(host.id.clone()).or_insert_with(String::new);
                            let text_resp = ui.add(
                                egui::TextEdit::singleline(input_text)
                                    .font(egui::FontId::monospace(12.0))
                                    .desired_width(ui.available_width() - 85.0)
                                    .hint_text("Type remote command and press Enter (e.g. ls -la, uname -a)..."),
                            );

                            let btn_run = egui::Button::new(
                                RichText::new("Execute")
                                    .size(11.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(108, 92, 231))
                            .rounding(Rounding::same(4.0));

                            let run_clicked = ui.add(btn_run).clicked();
                            let hit_enter = text_resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                            if (run_clicked || hit_enter) && !input_text.trim().is_empty() {
                                let cmd = input_text.trim().to_string();
                                input_text.clear();
                                text_resp.request_focus();
                                (Some(cmd), true)
                            } else {
                                (None, false)
                            }
                        };

                        if let (Some(cmd), true) = (cmd_to_run, execute) {
                            self.run_remote_ssh_command(host, &cmd, Some(ui.ctx().clone()));
                        }
                    });
                });
        });
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
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // ── Close Confirmation ──────────────────────────────────────────────
        let any_running = self.services.iter().any(|s| {
            s.state == ServiceState::Running || s.state == ServiceState::Starting || s.restart_at.is_some()
        });

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.close_confirmed {
                // User already approved exit - let egui close
            } else if any_running && !self.close_confirm_open {
                // Block close and show dialog
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_confirm_open = true;
            }
        }

        if self.close_confirm_open {
            let running_info: Vec<(String, Option<u16>, Option<u32>)> = self.services.iter()
                .filter(|s| s.state == ServiceState::Running || s.state == ServiceState::Starting)
                .map(|s| (s.config.name.clone(), if s.config.port > 0 { Some(s.config.port) } else { None }, s.pid))
                .collect();
            let p = self.theme.palette();
            let choice = render_close_confirmation_modal(ctx, &mut self.close_confirm_open, &running_info, &p);
            match choice {
                CloseConfirmChoice::StopAndExit => {
                    self.close_confirmed = true;
                    self.close_confirm_open = false;
                    self.stop_all();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    std::process::exit(0);
                }
                CloseConfirmChoice::KeepRunningAndExit => {
                    self.close_confirmed = true;
                    self.close_confirm_open = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    std::process::exit(0);
                }
                CloseConfirmChoice::Cancel => {
                    self.close_confirm_open = false;
                }
                CloseConfirmChoice::None => {}
            }
        }

        // Periodic poll only if any services are active (every 2s)
        if any_running && self.last_poll.elapsed() >= std::time::Duration::from_millis(2000) {
            for s in &mut self.services {
                s.poll_status(&mut self.sys);
            }
            self.last_poll = Instant::now();
        }

        // Periodic poll for other listening processes if user ticked the checkbox (every 3s)
        if self.show_other_process_metrics && self.active_tab == "overview" {
            ctx.request_repaint_after(std::time::Duration::from_millis(3000));
            if self.other_metrics.is_empty() || self.last_other_poll.elapsed() >= std::time::Duration::from_millis(3000) {
                let listeners = self.scanner.get_listeners();
                let pids: Vec<u32> = listeners.iter().filter_map(|l| l.pid).collect();
                if !pids.is_empty() {
                    let s_pids: Vec<sysinfo::Pid> = pids.iter().map(|&p| sysinfo::Pid::from_u32(p)).collect();
                    self.sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&s_pids));
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
            let p = self.theme.palette();
            if self.last_applied_palette.as_ref() != Some(&p) {
                ctx.set_visuals(p.egui_visuals());
                self.last_applied_palette = Some(p);
            }
        }

        self.handle_global_shortcuts(ctx);
        let p = self.theme.palette();

        // ═════════════════════════════════════════════════════════════════════
        // 0. DEDICATED SLIM CUSTOM TITLEBAR (Height 22px)
        // ═════════════════════════════════════════════════════════════════════
        egui::TopBottomPanel::top("custom_titlebar")
            .exact_height(22.0)
            .frame(
                Frame::none()
                    .fill(p.bg_header)
                    .inner_margin(egui::Margin {
                        left: 10.0,
                        right: 4.0,
                        top: 1.0,
                        bottom: 1.0,
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

                    // Left title: Green dot + SERVER LAUNCHER
                    ui.label(RichText::new("●").size(8.0).color(p.success));
                    ui.label(
                        RichText::new("SERVER LAUNCHER")
                            .size(10.5)
                            .monospace()
                            .strong()
                            .color(p.text_muted),
                    );

                    // Right window controls: only Minimize, Maximize, and Close
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;

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
        egui::TopBottomPanel::top("top_header")
            .frame(
                Frame::none()
                    .fill(p.bg_header)
                    .inner_margin(egui::Margin {
                        left: 14.0,
                        right: 14.0,
                        top: 4.0,
                        bottom: 5.0,
                    }),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let remote_host_opt = if self.selected_machine != "local" {
                        self.ssh_hosts.iter().find(|h| h.id == self.selected_machine).cloned()
                    } else {
                        None
                    };

                    // Left: App Icon + Server Launcher / Host title & status line
                    if let Some(texture) = &self.icon_texture {
                        ui.add(
                            egui::Image::from_texture(texture)
                                .fit_to_exact_size(egui::vec2(28.0, 28.0)),
                        );
                        ui.add_space(6.0);
                    }

                    if let Some(ref host) = remote_host_opt {
                        let (status, is_connecting) = {
                            let s = self.remote_connection_status.lock().unwrap().get(&host.id).cloned();
                            let c = self.remote_connecting.lock().unwrap().contains(&host.id);
                            (s, c)
                        };

                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&host.name)
                                        .size(19.0)
                                        .strong()
                                        .color(p.text_primary),
                                );
                                ui.add_space(6.0);
                                if is_connecting {
                                    Frame::none()
                                        .fill(p.warning_subtle)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 2.5))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new("○ Connecting...").size(10.5).color(p.warning));
                                        });
                                } else if let Some(Ok(ref info)) = status {
                                    Frame::none()
                                        .fill(p.success_subtle)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 2.5))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(format!("● Online ({})", info)).size(10.5).strong().color(p.success));
                                        });
                                } else if let Some(Err(ref err)) = status {
                                    Frame::none()
                                        .fill(p.danger_subtle)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 2.5))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(format!("● Error: {}", err)).size(10.5).color(p.danger));
                                        });
                                } else {
                                    Frame::none()
                                        .fill(p.bg_subtle)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 2.5))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new("○ Idle").size(10.5).color(p.text_muted));
                                        });
                                }
                            });
                            ui.add_space(2.0);
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
                            ui.label(
                                RichText::new(target_desc)
                                    .size(11.0)
                                    .color(p.text_muted),
                            );
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
                                    "this pc: {}    servers: {}",
                                    self.lan_ip,
                                    self.services.len()
                                ))
                                .size(10.5)
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

                    // Right: Actions buttons in modern pill layout
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;


                        // Settings Button (Pill icon button)
                        let btn_settings = egui::Button::new(
                            RichText::new(crate::lucide::SETTINGS).size(13.0).color(p.text_secondary),
                        )
                        .fill(p.bg_button)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(999.0))
                        .min_size(egui::vec2(30.0, 24.0));
                        if ui.add(btn_settings).on_hover_text("Settings & Preferences (Ctrl+,)").clicked() {
                            self.settings_modal_open = true;
                        }

                        if let Some(ref host) = remote_host_opt {
                            // REMOTE SSH PC CONTROLS (Moved up here)
                            let is_scanning = {
                                let map = self.remote_scanning.lock().unwrap();
                                map.contains(&host.id)
                            };

                            let btn_edit = egui::Button::new(
                                RichText::new(format!("{} Edit", crate::lucide::SETTINGS))
                                    .size(11.0)
                                    .color(p.text_secondary),
                            )
                            .fill(p.bg_button)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(64.0, 24.0));
                            if ui.add(btn_edit).clicked() {
                                self.ssh_host_modal.open_edit(host);
                            }

                            let btn_reconnect = egui::Button::new(
                                RichText::new(format!("{} Reconnect", crate::lucide::REFRESH_CW))
                                    .size(11.0)
                                    .color(p.text_secondary),
                            )
                            .fill(p.bg_button)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(90.0, 24.0));
                            if ui.add(btn_reconnect).clicked() {
                                self.test_remote_host(host);
                                self.trigger_remote_scan(host);
                            }

                            let btn_ext = egui::Button::new(
                                RichText::new(format!("{} External Terminal", crate::lucide::EXTERNAL_LINK))
                                    .size(11.0)
                                    .strong()
                                    .color(p.info),
                            )
                            .fill(p.info_subtle)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(130.0, 24.0));
                            if ui.add(btn_ext).on_hover_text("Open desktop terminal connected via SSH").clicked() {
                                let _ = launch_external_ssh_terminal(host);
                            }

                            let scan_label = if is_scanning { "Scanning..." } else { "Scan Ports" };
                            let btn_scan = egui::Button::new(
                                RichText::new(scan_label)
                                    .size(11.0)
                                    .strong()
                                    .color(p.accent),
                            )
                            .fill(p.bg_button)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(84.0, 24.0));
                            if ui.add(btn_scan).clicked() {
                                self.trigger_remote_scan(host);
                            }

                            // Remote Server Controls: + Start Server, Stop All, Restart All
                            let btn_add_server = egui::Button::new(
                                RichText::new("+ Start Server")
                                    .size(11.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(0x05, 0x96, 0x69))
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(98.0, 24.0));
                            if ui.add(btn_add_server).on_hover_text("Start a new server or process on remote host").clicked() {
                                self.remote_start_server_modal.open_for_host(host);
                            }
                        } else {
                            // LOCAL PC CONTROLS (+ Add Server, Quick Launch, Stop All, Start All, Search)
                            // + Add Server (Primary Accent Pill)
                            let btn_add = egui::Button::new(
                                RichText::new("+ Add Server")
                                    .size(11.5)
                                    .strong()
                                    .color(p.accent_text),
                            )
                            .fill(p.accent)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(92.0, 24.0));
                            if ui.add(btn_add).clicked() {
                                self.add_edit_modal.open_new();
                            }

                            // Quick Launch (Ctrl+P / Ctrl+K)
                            let btn_pal = egui::Button::new(
                                RichText::new(format!("{} Quick Launch Ctrl+P", crate::lucide::ZAP))
                                    .size(11.0)
                                    .color(p.accent),
                            )
                            .fill(p.bg_button)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(135.0, 24.0));
                            if ui.add(btn_pal).clicked() {
                                self.palette_modal.open = true;
                                self.palette_modal.query.clear();
                            }

                            // Search Pill Bar (with Ctrl+K hint chip)
                            Frame::none()
                                .fill(p.bg_input)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(999.0))
                                .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        ui.label(RichText::new(crate::lucide::SEARCH).size(11.0).color(p.text_muted));
                                        let s_resp = ui.add(
                                            egui::TextEdit::singleline(&mut self.overview_search_query)
                                                .hint_text(
                                                    RichText::new("Search instances, ports...")
                                                        .size(11.0)
                                                        .color(p.text_muted),
                                                )
                                                .desired_width(150.0)
                                                .frame(false),
                                        );
                                        Frame::none()
                                            .fill(p.bg_card)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::symmetric(4.0, 1.0))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new("Ctrl+K")
                                                        .size(9.0)
                                                        .color(p.text_muted)
                                                        .monospace(),
                                                );
                                            });
                                        if s_resp.changed() {
                                            self.refresh_server_list();
                                        }
                                        if !self.overview_search_query.is_empty() {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new(crate::lucide::X).size(10.0).color(p.text_muted),
                                                    )
                                                    .frame(false),
                                                )
                                                .clicked()
                                            {
                                                self.overview_search_query.clear();
                                                self.refresh_server_list();
                                            }
                                        }
                                    });
                                });
                        }


                    });
                });
            });

        // ═════════════════════════════════════════════════════════════════════
        // LEFT MACHINES NAV BAR (Switch between This PC and SSH Remotes)
        // ═════════════════════════════════════════════════════════════════════
        egui::SidePanel::left("machines_nav_bar")
            .resizable(false)
            .exact_width(52.0)
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
                ui.set_width(ui.available_width());
                ui.set_min_width(ui.available_width());
                self.render_machines_nav_bar(ui);
            });

        if self.selected_machine == "local" {
            // ═════════════════════════════════════════════════════════════════
            // LOCAL PC VIEW: Left Sidebar (380px) + Right Main Area (Remaining)
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
                    let total_h = ui.available_height();
                    let left_w = 380.0;
                    ui.horizontal(|ui| {
                        ui.set_height(total_h);
                        ui.set_min_height(total_h);

                        // LEFT SIDEBAR (380px width)
                        Frame::none()
                            .fill(p.bg_sidebar)
                            .stroke(Stroke::NONE)
                            .inner_margin(egui::Margin {
                                left: 14.0,
                                right: 12.0,
                                top: 0.0,
                                bottom: 0.0,
                            })
                            .show(ui, |ui| {
                                ui.set_width(left_w);
                                ui.set_min_width(left_w);
                                ui.set_max_width(left_w);
                                ui.set_height(total_h);
                                ui.set_min_height(total_h);
                                self.render_left_panel(ui);
                            });

                        ui.add_space(8.0);

                        // RIGHT MAIN AREA: Fills 100% of remaining width
                        let right_w = ui.available_width().max(100.0);
                        ui.allocate_ui_with_layout(egui::vec2(right_w, total_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(right_w);
                            ui.set_max_width(right_w);
                            ui.set_height(total_h);
                            ui.set_min_height(total_h);
                            self.render_right_panel(ui);
                        });
                    });
                });
        } else {
            // ═════════════════════════════════════════════════════════════════
            // SSH REMOTE PC DASHBOARD
            // ═════════════════════════════════════════════════════════════════
            egui::CentralPanel::default()
                .frame(
                    Frame::none()
                        .fill(p.bg_main)
                        .inner_margin(egui::Margin {
                            left: 12.0,
                            right: 18.0,
                            top: 4.0,
                            bottom: 6.0,
                        }),
                )
                .show(ctx, |ui| {
                    let machine_id = self.selected_machine.clone();
                    self.render_remote_pc_dashboard(ui, &machine_id);
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

        let ssh_modal_action = render_ssh_host_modal(ctx, &mut self.ssh_host_modal, &p);
        match ssh_modal_action {
            ModalAction::SaveSshHost(new_host, old_id) => {
                self.ssh_host_modal.open = false;
                if let Some(old) = old_id {
                    if let Some(pos) = self.ssh_hosts.iter().position(|h| h.id == old) {
                        self.ssh_hosts[pos] = new_host.clone();
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
                if let Some(pos) = self.ssh_hosts.iter().position(|h| h.id == id) {
                    self.ssh_hosts.remove(pos);
                    if self.selected_machine == id {
                        self.selected_machine = "local".to_string();
                    }
                    self.persist_config();
                }
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
            self.persist_config();
        }

        if let Some(service_def) = render_new_service_modal(ctx, &mut self.systemd_modal, &p) {
            self.systemd_modal.open = false;
            let is_remote = self.selected_machine != "local";
            if is_remote {
                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == self.selected_machine).cloned() {
                    let action_msg_arc = self.remote_service_action_msg.clone();
                    let services_arc = self.remote_services.clone();
                    std::thread::spawn(move || {
                        match create_remote_service(&host, &service_def) {
                            Ok(msg) => {
                                if let Ok(mut m) = action_msg_arc.lock() {
                                    *m = Some(msg);
                                }
                                if let Ok(services) = list_remote_services(&host, service_def.is_user) {
                                    let key = format!("{}:{}", host.id, if service_def.is_user { "user" } else { "system" });
                                    if let Ok(mut map) = services_arc.lock() {
                                        map.insert(key, services);
                                    }
                                }
                            }
                            Err(err) => {
                                if let Ok(mut m) = action_msg_arc.lock() {
                                    *m = Some(format!("Error: {}", err));
                                }
                            }
                        }
                    });
                }
            } else {
                if !service_def.is_user && self.local_sudo_password.is_empty() {
                    self.sudo_popup.open = true;
                    self.sudo_popup.pending = Some(PendingSudoAction::CreateService { def: service_def });
                    self.sudo_popup.password_input.clear();
                    self.sudo_popup.error = None;
                    self.sudo_popup.remember_password = true;
                    self.sudo_popup.just_opened = true;
                } else {
                    let pass_opt = if !service_def.is_user && !self.local_sudo_password.is_empty() {
                        Some(self.local_sudo_password.as_str())
                    } else {
                        None
                    };
                    match create_local_service(&service_def, pass_opt) {
                        Ok(msg) => {
                            self.service_action_msg = Some(msg);
                            self.trigger_local_services_list(service_def.is_user);
                        }
                        Err(err) => {
                            if !service_def.is_user && (err.contains("Sudo password needed") || err.contains("authentication") || err.contains("Incorrect sudo password")) {
                                self.local_sudo_password.clear();
                                self.sudo_popup.open = true;
                                self.sudo_popup.pending = Some(PendingSudoAction::CreateService { def: service_def });
                                self.sudo_popup.password_input.clear();
                                self.sudo_popup.error = Some(err);
                                self.sudo_popup.remember_password = true;
                                self.sudo_popup.just_opened = true;
                            } else {
                                self.service_action_msg = Some(format!("Error: {}", err));
                            }
                        }
                    }
                }
            }
        }

        if let Some(req) = render_remote_start_server_modal(ctx, &mut self.remote_start_server_modal, &p) {
            self.remote_start_server_modal.open = false;
            if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == req.host_id).cloned() {
                let full_cmd = if !req.cwd.is_empty() {
                    format!("cd \"{}\" 2>/dev/null || true; nohup {} >/dev/null 2>&1 & echo '[launcher] Started {}: {}'", req.cwd, req.command, req.name, req.command)
                } else {
                    format!("nohup {} >/dev/null 2>&1 & echo '[launcher] Started {}: {}'", req.command, req.name, req.command)
                };
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
                            });
                        }
                    }
                }
                self.run_remote_ssh_command(&host, &full_cmd, Some(ctx.clone()));
                self.schedule_remote_scan(&host, 1500, ctx.clone());
            }
        }

        self.render_sudo_popup(ctx, &p);
        self.render_settings_modal(ctx, &p);

        let is_scanning = self.scanner.is_scanning.lock().map(|s| *s).unwrap_or(false);
        let has_remote_bg_work = {
            self.remote_connecting.lock().map(|s| !s.is_empty()).unwrap_or(false)
                || self.remote_scanning.lock().map(|s| !s.is_empty()).unwrap_or(false)
                || self.remote_services_in_flight.lock().map(|s| !s.is_empty()).unwrap_or(false)
                || self.remote_service_action_in_flight.lock().map(|s| !s.is_empty()).unwrap_or(false)
                || self.remote_journal_in_flight.lock().map(|s| !s.is_empty()).unwrap_or(false)
                || self.remote_sensors_in_flight.lock().map(|s| !s.is_empty()).unwrap_or(false)
        };
        if self.close_confirm_open {
            // Keep repainting while confirmation dialog is open
            ctx.request_repaint();
        } else if is_scanning || has_remote_bg_work {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        } else if self.active_sidebar_tab == SidebarTab::Sensors && self.sensors_refresh_interval > 0 {
            // Repaint dynamically matching sensors interval (minimum 500ms for smooth UI)
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        } else if any_running {
            // When servers are running, repaint every 1.5s — stats update rate doesn't need to be faster
            ctx.request_repaint_after(std::time::Duration::from_millis(1500));
        } else {
            // When fully idle, repaint reactively on user interaction only (4s heartbeat)
            ctx.request_repaint_after(std::time::Duration::from_secs(4));
        }
    }
}

impl LauncherApp {
    pub fn refresh_server_list(&mut self) {
        self.scanner.trigger_scan();
        let listeners = self.scanner.get_listeners();
        let mut port_to_listener: HashMap<u16, &crate::scanner::ListenerInfo> = HashMap::new();
        for l in &listeners {
            port_to_listener.insert(l.port, l);
        }

        for s in &mut self.services {
            if s.state == ServiceState::Stopped {
                let pid_file = std::env::temp_dir().join(format!("launcher_{}.pid", s.config.key));
                let mut detected_pid: Option<u32> = None;
                let mut matched_by_port = false;

                if let Ok(content) = std::fs::read_to_string(&pid_file) {
                    if let Ok(p) = content.trim().parse::<u32>() {
                        #[cfg(unix)]
                        let alive = unsafe { libc::kill(p as i32, 0) == 0 };
                        #[cfg(not(unix))]
                        let alive = true;

                        if alive {
                            detected_pid = Some(p);
                        } else {
                            let _ = std::fs::remove_file(&pid_file);
                        }
                    }
                }

                if detected_pid.is_none() && s.config.port > 0 {
                    if let Some(l) = port_to_listener.get(&s.config.port) {
                        if let Some(p) = l.pid {
                            #[cfg(unix)]
                            let alive = unsafe { libc::kill(p as i32, 0) == 0 };
                            #[cfg(not(unix))]
                            let alive = true;

                            if alive {
                                detected_pid = Some(p);
                                matched_by_port = true;
                            }
                        }
                    }
                }

                if let Some(pid) = detected_pid {
                    s.attach_existing(pid, matched_by_port);
                }
            }

            s.poll_status(&mut self.sys);
        }
        self.last_poll = Instant::now();
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
    // TOP NAVIGATION TABS BAR (Servers, Scripts, Commands, SystemMD, Sensors)
    // ═════════════════════════════════════════════════════════════════════════
    fn render_sidebar_tabs_bar(&mut self, ui: &mut Ui) {
        let p = self.theme.palette();
        Frame::none()
            .fill(p.bg_card)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(999.0))
            .inner_margin(egui::Margin::symmetric(4.0, 3.5))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let avail_w = ui.available_width().max(60.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.spacing_mut().button_padding = egui::vec2(3.0, 4.0);
                    let tabs = [
                        (SidebarTab::Servers, SidebarTab::Servers.label()),
                        (SidebarTab::Scripts, SidebarTab::Scripts.label()),
                        (SidebarTab::Commands, SidebarTab::Commands.label()),
                        (SidebarTab::SystemMD, SidebarTab::SystemMD.label()),
                        (SidebarTab::Sensors, SidebarTab::Sensors.label()),
                    ];

                    let btn_w = ((avail_w - 20.0) / 5.0).max(10.0);

                    for (tab, label) in tabs {
                        let is_active = self.active_sidebar_tab == tab;
                        let bg = if is_active {
                            p.accent
                        } else {
                            p.bg_button
                        };
                        let fg = if is_active {
                            p.accent_text
                        } else {
                            p.text_secondary
                        };
                        let btn = egui::Button::new(
                            RichText::new(label)
                                .size(10.0)
                                .strong()
                                .color(fg),
                        )
                        .fill(bg)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(999.0))
                        .min_size(egui::vec2(btn_w, 24.0));

                        if ui.add(btn).clicked() {
                            self.active_sidebar_tab = tab;
                            if tab == SidebarTab::Commands {
                                if self.selected_machine == "local" {
                                    self.local_terminal.request_focus = true;
                                } else {
                                    self.remote_terminals.entry(self.selected_machine.clone()).or_default().request_focus = true;
                                }
                            }
                        }
                    }
                });
            });
    }

    fn render_left_panel(&mut self, ui: &mut Ui) {
        ui.set_width(ui.available_width());
        ui.set_max_width(ui.available_width());
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.set_max_width(ui.available_width());
            // 0. Top 5 Navigation Tabs Bar
            self.render_sidebar_tabs_bar(ui);
            ui.add_space(6.0);

            match self.active_sidebar_tab {
                SidebarTab::Servers => self.render_servers_left_panel(ui),
                SidebarTab::Scripts => self.render_scripts_left_panel(ui, None),
                SidebarTab::Commands => self.render_commands_left_panel(ui, None),
                SidebarTab::SystemMD => self.render_systemd_left_panel(ui, None),
                SidebarTab::Sensors => self.render_sensors_left_panel(ui, None),
            }
        });
    }

    fn render_right_panel(&mut self, ui: &mut Ui) {
        match self.active_sidebar_tab {
            SidebarTab::Servers => self.render_servers_right_panel(ui),
            SidebarTab::Scripts => self.render_scripts_right_panel(ui, None),
            SidebarTab::Commands => self.render_commands_right_panel(ui, None),
            SidebarTab::SystemMD => self.render_systemd_right_panel(ui, None),
            SidebarTab::Sensors => self.render_sensors_right_panel(ui, None),
        }
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

    fn render_scripts_left_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.theme.palette();
        ui.set_width(ui.available_width());
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("CUSTOM SCRIPTS").size(12.0).strong().color(p.text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_new = egui::Button::new(
                        RichText::new("+ New Script").size(11.0).strong().color(Color32::WHITE),
                    )
                    .fill(p.accent)
                    .rounding(Rounding::same(4.0));
                    if ui.add(btn_new).clicked() {
                        self.script_modal.open_create(host_id.map(|s| s.to_string()));
                    }
                });
            });

            ui.add_space(6.0);

            // Search filter
            ui.add(
                egui::TextEdit::singleline(&mut self.script_search_query)
                    .hint_text(format!("{} Filter scripts...", crate::lucide::SEARCH))
                    .desired_width(ui.available_width()),
            );

            ui.add_space(8.0);

            let q = self.script_search_query.trim().to_lowercase();
            let machine_scripts: Vec<CustomScript> = self
                .scripts
                .iter()
                .filter(|s| {
                    s.host_id.as_deref() == host_id
                        && (q.is_empty() || s.name.to_lowercase().contains(&q) || s.script.to_lowercase().contains(&q))
                })
                .cloned()
                .collect();

            if machine_scripts.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No scripts found").size(12.0).color(p.text_muted));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Click '+ New Script' to create one.").size(11.0).color(p.text_muted));
                });
                return;
            }

            ScrollArea::vertical()
                .id_salt("scripts_left_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_max_width(ui.available_width());
                    let mut to_run = None;
                    let mut to_stop = None;
                    let mut to_edit = None;
                    let mut to_delete = None;

                    for s in &machine_scripts {
                        let is_selected = self.selected_script_id.as_deref() == Some(&s.id);
                        let is_running = self
                            .script_executions
                            .get(&s.id)
                            .and_then(|e| e.status.lock().ok())
                            .map(|st| matches!(*st, ScriptRunStatus::Running))
                            .unwrap_or(false);

                        Frame::none()
                            .fill(if is_selected { p.bg_card } else { p.bg_sidebar })
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.set_max_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let status_text = if is_running {
                                        RichText::new("●").size(12.0).color(Color32::from_rgb(16, 185, 129))
                                    } else {
                                        RichText::new("○").size(12.0).color(p.text_muted)
                                    };
                                    ui.label(status_text);

                                    let tag_w = 55.0;
                                    let name_w = (ui.available_width() - tag_w).max(40.0);

                                    let title = RichText::new(&s.name).size(12.5).strong().color(if is_selected { p.accent_light } else { p.text_primary });
                                    let title_resp = ui.add_sized(
                                        [name_w, 16.0],
                                        egui::Label::new(title).truncate().sense(egui::Sense::click()),
                                    );
                                    if title_resp.clicked() {
                                        self.selected_script_id = Some(s.id.clone());
                                    }

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(
                                            RichText::new(format!("[{}]", s.shell))
                                                .size(10.5)
                                                .color(p.text_muted),
                                        );
                                    });
                                });

                                if s.autostart || !s.cwd.is_empty() {
                                    ui.add_space(2.0);
                                    ui.horizontal(|ui| {
                                        if s.autostart {
                                            ui.label(
                                                RichText::new(format!("{} Startup", crate::lucide::ZAP))
                                                    .size(10.0)
                                                    .color(Color32::from_rgb(250, 204, 21)),
                                            );
                                        }
                                        if !s.cwd.is_empty() {
                                            let cwd_w = (ui.available_width() - 4.0).max(40.0);
                                            ui.add_sized(
                                                [cwd_w, 14.0],
                                                egui::Label::new(
                                                    RichText::new(format!("cwd: {}", s.cwd))
                                                        .size(10.0)
                                                        .color(p.text_muted),
                                                )
                                                .truncate(),
                                            );
                                        }
                                    });
                                }

                                ui.add_space(4.0);

                                ui.horizontal(|ui| {
                                    if is_running {
                                        let btn_stop = egui::Button::new(
                                            RichText::new(format!("{} Stop", crate::lucide::STOP)).size(10.5).color(Color32::WHITE),
                                        )
                                        .fill(p.danger)
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_stop).clicked() {
                                            to_stop = Some(s.id.clone());
                                        }
                                    } else {
                                        let btn_run = egui::Button::new(
                                            RichText::new(format!("{} Run", crate::lucide::PLAY)).size(10.5).color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(16, 185, 129))
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_run).clicked() {
                                            to_run = Some(s.clone());
                                        }
                                    }

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let btn_del = egui::Button::new(
                                            RichText::new(crate::lucide::TRASH).size(10.5).color(p.text_muted),
                                        )
                                        .fill(Color32::TRANSPARENT);
                                        if ui.add(btn_del).on_hover_text("Delete Script").clicked() {
                                            to_delete = Some(s.id.clone());
                                        }

                                        let btn_edit = egui::Button::new(
                                            RichText::new(crate::lucide::PENCIL).size(10.5).color(p.text_muted),
                                        )
                                        .fill(Color32::TRANSPARENT);
                                        if ui.add(btn_edit).on_hover_text("Edit Script").clicked() {
                                            to_edit = Some(s.clone());
                                        }
                                    });
                                });
                            });

                        ui.add_space(6.0);
                    }

                    if let Some(s) = to_run {
                        self.request_run_script(&s, host_id.map(|h| h.to_string()));
                    }

                    if let Some(id) = to_stop {
                        if let Some(exec) = self.script_executions.get_mut(&id) {
                            exec.stop();
                        }
                    }

                    if let Some(s) = to_edit {
                        self.script_modal.open_edit(&s);
                    }

                    if let Some(id) = to_delete {
                        self.scripts.retain(|s| s.id != id);
                        self.script_executions.remove(&id);
                        if self.selected_script_id.as_deref() == Some(&id) {
                            self.selected_script_id = None;
                        }
                        self.persist_config();
                    }
                });
        });
    }

    fn render_scripts_right_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.theme.palette();
        let selected_id = self.selected_script_id.clone().or_else(|| {
            self.scripts
                .iter()
                .find(|s| s.host_id.as_deref() == host_id)
                .map(|s| s.id.clone())
        });

        let script = selected_id.as_ref().and_then(|id| {
            self.scripts.iter().find(|s| &s.id == id).cloned()
        });

        match script {
            Some(s) => {
                let is_running = self
                    .script_executions
                    .get(&s.id)
                    .and_then(|e| e.status.lock().ok())
                    .map(|st| matches!(*st, ScriptRunStatus::Running))
                    .unwrap_or(false);

                ui.vertical(|ui| {
                    // Header Bar
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(14.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::lucide::SCRIPT).size(20.0).color(p.accent));
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&s.name).size(15.0).strong().color(p.text_primary));
                                        ui.label(
                                            RichText::new(format!("({})", s.shell))
                                                .size(11.0)
                                                .color(p.text_muted),
                                        );
                                        if s.autostart {
                                            ui.label(
                                                RichText::new(format!("{} Runs at Startup", crate::lucide::ZAP))
                                                    .size(10.5)
                                                    .color(Color32::from_rgb(250, 204, 21)),
                                            );
                                        }
                                    });
                                    if !s.cwd.is_empty() {
                                        ui.label(
                                            RichText::new(format!("CWD: {}", s.cwd))
                                                .size(10.5)
                                                .color(p.text_muted),
                                        );
                                    }
                                });

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let btn_edit = egui::Button::new(RichText::new(format!("{} Edit", crate::lucide::PENCIL)).size(11.0).color(p.text_secondary))
                                        .fill(p.bg_button);
                                    if ui.add(btn_edit).clicked() {
                                        self.script_modal.open_edit(&s);
                                    }

                                    let btn_clear = egui::Button::new(RichText::new(format!("{} Clear Logs", crate::lucide::TRASH)).size(11.0).color(p.text_secondary))
                                        .fill(p.bg_button);
                                    if ui.add(btn_clear).clicked() {
                                        if let Some(exec) = self.script_executions.get_mut(&s.id) {
                                            if let Ok(mut l) = exec.logs.lock() {
                                                l.clear();
                                            }
                                        }
                                    }

                                    if is_running {
                                        let btn_stop = egui::Button::new(
                                            RichText::new(format!("{} Stop Script", crate::lucide::STOP)).size(11.5).strong().color(Color32::WHITE),
                                        )
                                        .fill(p.danger)
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_stop).clicked() {
                                            if let Some(exec) = self.script_executions.get_mut(&s.id) {
                                                exec.stop();
                                            }
                                        }
                                    } else {
                                        let btn_run = egui::Button::new(
                                            RichText::new(format!("{} Run Script", crate::lucide::PLAY)).size(11.5).strong().color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(16, 185, 129))
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_run).clicked() {
                                            self.request_run_script(&s, host_id.map(|h| h.to_string()));
                                        }
                                    }
                                });
                            });
                        });

                    ui.add_space(8.0);

                    // Console Output Area
                    let logs = self
                        .script_executions
                        .get(&s.id)
                        .map(|e| e.logs.clone());

                    Frame::none()
                        .fill(p.terminal_bg)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(14.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ScrollArea::vertical()
                                .id_salt("script_console_scroll")
                                .stick_to_bottom(true)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if let Some(logs_arc) = logs {
                                        if let Ok(l) = logs_arc.lock() {
                                            if l.is_empty() {
                                                ui.label(
                                                    RichText::new(format!("Script ready. Click '{} Run Script' to execute.", crate::lucide::PLAY))
                                                        .size(11.5)
                                                        .color(Color32::from_rgb(100, 110, 130)),
                                                );
                                            } else {
                                                for entry in l.iter() {
                                                    let color = match entry.kind {
                                                        LogKind::Launcher => Color32::from_rgb(96, 165, 250),
                                                        LogKind::Stdout => Color32::from_rgb(220, 225, 235),
                                                        LogKind::Stderr | LogKind::Error => Color32::from_rgb(248, 113, 113),
                                                        LogKind::Warning => Color32::from_rgb(250, 204, 21),
                                                        LogKind::Stdin => Color32::from_rgb(52, 211, 153),
                                                    };
                                                    ui.horizontal(|ui| {
                                                        ui.label(
                                                            RichText::new(&entry.timestamp)
                                                                .size(10.5)
                                                                .color(Color32::from_rgb(90, 100, 120))
                                                                .monospace(),
                                                        );
                                                        ui.label(
                                                            RichText::new(&entry.text)
                                                                .size(11.5)
                                                                .color(color)
                                                                .monospace(),
                                                        );
                                                    });
                                                }
                                            }
                                        }
                                    } else {
                                        ui.label(
                                            RichText::new(format!("Script ready. Click '{} Run Script' to execute.", crate::lucide::PLAY))
                                                .size(11.5)
                                                .color(Color32::from_rgb(100, 110, 130)),
                                        );
                                    }
                                });
                        });
                });
            }
            None => {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(RichText::new(format!("{} Custom Scripts", crate::lucide::SCRIPT)).size(16.0).strong().color(p.text_primary));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Select a script from the left or click '+ New Script' to create one.\nScripts can execute any command or interpreter and can run automatically on launch.")
                            .size(12.0)
                            .color(p.text_muted),
                    );
                });
            }
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // COMMANDS TAB: Saved Snippets Library & Interactive Terminal
    // ═════════════════════════════════════════════════════════════════════════
    fn render_commands_left_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.theme.palette();
        ui.set_width(ui.available_width());
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("SAVED COMMANDS").size(12.0).strong().color(p.text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_new = egui::Button::new(
                        RichText::new("+ Save Command").size(11.0).strong().color(Color32::WHITE),
                    )
                    .fill(p.accent)
                    .rounding(Rounding::same(4.0));
                    if ui.add(btn_new).clicked() {
                        self.command_modal.open_create(host_id.map(|s| s.to_string()));
                    }
                });
            });

            ui.add_space(6.0);

            // Category Filter Pills
            let mut categories = vec!["All".to_string()];
            let mut unique_cats = BTreeSet::new();
            for c in &self.saved_commands {
                if c.host_id.as_deref() == host_id {
                    unique_cats.insert(c.category.clone());
                }
            }
            categories.extend(unique_cats);

            ui.horizontal_wrapped(|ui| {
                for cat in categories {
                    let is_active = self.saved_command_category == cat;
                    let bg = if is_active { p.accent } else { p.bg_button };
                    let fg = if is_active { p.accent_text } else { p.text_secondary };
                    let btn = egui::Button::new(RichText::new(&cat).size(10.5).color(fg))
                        .fill(bg)
                        .rounding(Rounding::same(4.0));
                    if ui.add(btn).clicked() {
                        self.saved_command_category = cat;
                    }
                }
            });

            ui.add_space(6.0);

            // Search filter
            ui.add(
                egui::TextEdit::singleline(&mut self.saved_command_search)
                    .hint_text(format!("{} Filter commands...", crate::lucide::SEARCH))
                    .desired_width(ui.available_width()),
            );

            ui.add_space(8.0);

            let q = self.saved_command_search.trim().to_lowercase();
            let cat_filter = &self.saved_command_category;
            let commands: Vec<SavedCommand> = self
                .saved_commands
                .iter()
                .filter(|c| {
                    c.host_id.as_deref() == host_id
                        && (cat_filter == "All" || &c.category == cat_filter)
                        && (q.is_empty()
                            || c.name.to_lowercase().contains(&q)
                            || c.command.to_lowercase().contains(&q)
                            || c.description.to_lowercase().contains(&q))
                })
                .cloned()
                .collect();

            if commands.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No saved commands found").size(12.0).color(p.text_muted));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Click '+ Save Command' to add snippets.").size(11.0).color(p.text_muted));
                });
                return;
            }

            ScrollArea::vertical()
                .id_salt("saved_commands_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_max_width(ui.available_width());
                    let mut to_run_cmd: Option<(String, bool)> = None;
                    let mut to_edit = None;
                    let mut to_delete = None;

                    for c in &commands {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.set_max_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let cat_w = 60.0;
                                    let name_w = (ui.available_width() - cat_w).max(40.0);

                                    ui.add_sized(
                                        [name_w, 16.0],
                                        egui::Label::new(RichText::new(&c.name).size(12.5).strong().color(p.text_primary)).truncate(),
                                    );
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(
                                            RichText::new(format!("[{}]", c.category))
                                                .size(10.5)
                                                .color(p.text_muted),
                                        );
                                        if c.run_in_terminal {
                                            ui.add_space(4.0);
                                            ui.label(
                                                RichText::new(format!("{} External", crate::lucide::MONITOR))
                                                    .size(10.0)
                                                    .strong()
                                                    .color(p.info),
                                            );
                                        }
                                    });
                                });

                                ui.add_space(2.0);
                                let cmd_w = (ui.available_width() - 4.0).max(40.0);
                                ui.add_sized(
                                    [cmd_w, 15.0],
                                    egui::Label::new(
                                        RichText::new(&c.command)
                                            .size(11.0)
                                            .monospace()
                                            .color(Color32::from_rgb(165, 180, 252)),
                                    )
                                    .truncate(),
                                );

                                if !c.description.is_empty() {
                                    ui.add_space(2.0);
                                    let desc_w = (ui.available_width() - 4.0).max(40.0);
                                    ui.add_sized(
                                        [desc_w, 14.0],
                                        egui::Label::new(
                                            RichText::new(&c.description)
                                                .size(10.5)
                                                .color(p.text_muted),
                                        )
                                        .truncate(),
                                    );
                                }

                                ui.add_space(4.0);

                                ui.horizontal(|ui| {
                                    let run_label = if c.run_in_terminal {
                                        format!("{} Run (Terminal)", crate::lucide::PLAY)
                                    } else {
                                        format!("{} Run", crate::lucide::PLAY)
                                    };
                                    let run_bg = if c.run_in_terminal { Color32::from_rgb(0x02, 0x84, 0xc7) } else { Color32::from_rgb(16, 185, 129) };
                                    let btn_run = egui::Button::new(
                                        RichText::new(run_label).size(10.5).color(Color32::WHITE),
                                    )
                                    .fill(run_bg)
                                    .rounding(Rounding::same(4.0));
                                    if ui.add(btn_run).on_hover_text(if c.run_in_terminal { "Execute in external desktop terminal window" } else { "Execute in terminal" }).clicked() {
                                        to_run_cmd = Some((c.command.clone(), c.run_in_terminal));
                                    }

                                    if !c.run_in_terminal {
                                        let btn_ext = egui::Button::new(
                                            RichText::new(format!("{} External", crate::lucide::EXTERNAL_LINK)).size(10.0).color(p.accent_light),
                                        )
                                        .fill(p.bg_button)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_ext).on_hover_text("Execute in external desktop terminal window").clicked() {
                                            to_run_cmd = Some((c.command.clone(), true));
                                        }
                                    }

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let btn_del = egui::Button::new(RichText::new(crate::lucide::TRASH).size(10.5).color(p.text_muted))
                                            .fill(Color32::TRANSPARENT);
                                        if ui.add(btn_del).on_hover_text("Delete Command").clicked() {
                                            to_delete = Some(c.id.clone());
                                        }

                                        let btn_edit = egui::Button::new(RichText::new(crate::lucide::PENCIL).size(10.5).color(p.text_muted))
                                            .fill(Color32::TRANSPARENT);
                                        if ui.add(btn_edit).on_hover_text("Edit Command").clicked() {
                                            to_edit = Some(c.clone());
                                        }

                                        let btn_copy = egui::Button::new(RichText::new(crate::lucide::COPY).size(10.5).color(p.text_muted))
                                            .fill(Color32::TRANSPARENT);
                                        if ui.add(btn_copy).on_hover_text("Copy to Clipboard").clicked() {
                                            ui.output_mut(|o| o.copied_text = c.command.clone());
                                        }
                                    });
                                });
                            });

                        ui.add_space(6.0);
                    }

                    if let Some((cmd_str, in_external)) = to_run_cmd {
                        if in_external {
                            if let Some(hid) = host_id {
                                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                    let _ = crate::remote::launch_external_ssh_command(&host, &cmd_str);
                                }
                            } else {
                                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                                let _ = crate::commands::launch_external_command(&cmd_str, &cwd);
                            }
                        } else {
                            if let Some(hid) = host_id {
                                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                    let term = self.remote_terminals.entry(hid.to_string()).or_default();
                                    term.execute_remote(&host, &cmd_str);
                                    term.request_focus = true;
                                }
                            } else {
                                self.local_terminal.execute_local(&cmd_str, "");
                                self.local_terminal.request_focus = true;
                            }
                        }
                    }

                    if let Some(c) = to_edit {
                        self.command_modal.open_edit(&c);
                    }

                    if let Some(id) = to_delete {
                        self.saved_commands.retain(|c| c.id != id);
                        self.persist_config();
                    }
                });
        });
    }

    fn render_commands_right_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.theme.palette();
        let host_opt = host_id.and_then(|hid| self.ssh_hosts.iter().find(|h| h.id == hid).cloned());

        let term = if let Some(h) = &host_opt {
            self.remote_terminals.entry(h.id.clone()).or_default()
        } else {
            &mut self.local_terminal
        };

        let is_running = term.is_running();
        let stat_msg = term.status.lock().ok().map(|st| match &*st {
            TerminalCommandStatus::Idle => format!("{} Ready", crate::lucide::CIRCLE_DOT),
            TerminalCommandStatus::Running(c) => format!("{} Running: {}", crate::lucide::PLAY, c),
            TerminalCommandStatus::Success(code) => format!("{} Exit code {}", crate::lucide::CHECK, code),
            TerminalCommandStatus::Failed(err) => format!("{} Failed: {}", crate::lucide::X, err),
        }).unwrap_or_default();

        let title = if let Some(h) = &host_opt {
            format!("{} Remote SSH Terminal ({}@{}:{})", crate::lucide::GLOBE, h.user, h.host, h.port)
        } else {
            format!("{} Local Interactive Terminal", crate::lucide::MONITOR)
        };

        let cwd_display = if host_opt.is_none() {
            let home_dir = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok().map(std::path::PathBuf::from);
            if let Some(home) = home_dir {
                if let Ok(rel) = term.cwd.strip_prefix(&home) {
                    let rel_str = rel.display().to_string();
                    if rel_str.is_empty() {
                        "~".to_string()
                    } else {
                        format!("~/{}", rel_str)
                    }
                } else {
                    term.cwd.display().to_string()
                }
            } else {
                term.cwd.display().to_string()
            }
        } else {
            "~".to_string()
        };

        let prompt_str = term.get_prompt_str(host_opt.as_ref());

        ui.vertical(|ui| {
            // ═════════════════════════════════════════════════════════════════
            // Header Bar
            // ═════════════════════════════════════════════════════════════════
            Frame::none()
                .fill(p.bg_card)
                .stroke(Stroke::NONE)
                .rounding(Rounding::same(14.0))
                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        // Row 1: Title and Action Buttons (No overlap possible)
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::lucide::TERMINAL).size(18.0).color(p.accent));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&title).size(13.5).strong().color(p.text_primary),
                                )
                                .truncate(),
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let btn_clear = egui::Button::new(
                                    RichText::new(format!("{} Clear (Ctrl+L)", crate::lucide::TRASH))
                                        .size(11.0)
                                        .color(p.text_secondary),
                                )
                                .fill(p.bg_button);
                                if ui.add(btn_clear).clicked() {
                                    term.clear();
                                    term.request_focus = true;
                                }

                                if is_running {
                                    let btn_stop = egui::Button::new(
                                        RichText::new(format!("{} Interrupt (Ctrl+C)", crate::lucide::STOP))
                                            .size(11.0)
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(p.danger)
                                    .rounding(Rounding::same(4.0));
                                    if ui.add(btn_stop).clicked() {
                                        term.interrupt();
                                        term.request_focus = true;
                                    }
                                } else if host_opt.is_none() {
                                    let btn_ext = egui::Button::new(
                                        RichText::new(format!("{} Native Terminal", crate::lucide::EXTERNAL_LINK))
                                            .size(11.0)
                                            .color(p.text_secondary),
                                    )
                                    .fill(p.bg_button);
                                    if ui
                                        .add(btn_ext)
                                        .on_hover_text("Open native system terminal window in current directory")
                                        .clicked()
                                    {
                                        let _ = crate::commands::open_system_terminal(&term.cwd);
                                    }
                                }
                            });
                        });

                        ui.add_space(3.0);

                        // Row 2: Status & Current Directory Sub-bar (Full width available)
                        ui.horizontal(|ui| {
                            ui.add_space(26.0); // Align with title text past terminal icon
                            ui.label(
                                RichText::new(&stat_msg)
                                    .size(11.0)
                                    .color(if is_running {
                                        Color32::from_rgb(16, 185, 129)
                                    } else {
                                        p.text_muted
                                    }),
                            );
                            ui.label(RichText::new("•").size(11.0).color(p.text_muted));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("{} {}", crate::lucide::FOLDER, cwd_display))
                                        .size(11.0)
                                        .color(Color32::from_rgb(147, 197, 253))
                                        .monospace(),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&cwd_display);
                        });
                    });
                });

            ui.add_space(8.0);

            // ═════════════════════════════════════════════════════════════════
            // Integrated Dark Terminal Window (Output + Prompt at bottom)
            // ═════════════════════════════════════════════════════════════════
            let total_h = ui.available_height().max(200.0);
            let logs_clone = term.logs.clone();
            let mut should_focus = false;

            Frame::none()
                .fill(p.terminal_bg)
                .stroke(Stroke::NONE)
                .rounding(Rounding::same(14.0))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.set_height(total_h);
                    let prompt_bar_h = 36.0;
                    let scroll_h = (total_h - prompt_bar_h - 16.0).max(80.0);

                    // 1. Scrollable Output Log Area
                    let scroll_resp = ScrollArea::vertical()
                        .id_salt("cmd_terminal_scroll")
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .max_height(scroll_h)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            if let Ok(l) = logs_clone.lock() {
                                if l.is_empty() {
                                    ui.label(
                                        RichText::new("Terminal ready. Type a command below or press Tab for auto-completion.")
                                            .size(11.5)
                                            .color(Color32::from_rgb(100, 110, 130)),
                                    );
                                } else {
                                    for entry in l.iter() {
                                        let color = match entry.kind {
                                            LogKind::Stdin => Color32::from_rgb(52, 211, 153),
                                            LogKind::Launcher => Color32::from_rgb(96, 165, 250),
                                            LogKind::Stdout => Color32::from_rgb(220, 225, 235),
                                            LogKind::Stderr | LogKind::Error => Color32::from_rgb(248, 113, 113),
                                            LogKind::Warning => Color32::from_rgb(250, 204, 21),
                                        };
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(&entry.timestamp)
                                                    .size(10.5)
                                                    .color(Color32::from_rgb(80, 90, 110))
                                                    .monospace(),
                                            );
                                            ui.label(
                                                RichText::new(&entry.text)
                                                    .size(11.5)
                                                    .color(color)
                                                    .monospace(),
                                            );
                                        });
                                    }
                                }
                            }
                        });

                    // Clicking anywhere in the terminal scroll area focuses the input prompt
                    if scroll_resp.inner_rect.contains(ui.input(|i| i.pointer.interact_pos()).unwrap_or_default())
                        && ui.input(|i| i.pointer.primary_clicked())
                    {
                        should_focus = true;
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(2.0);

                    // 2. Integrated Interactive Terminal Prompt Row
                    ui.horizontal(|ui| {
                        let prompt_label = if is_running {
                            format!("{} stdin > ", crate::lucide::CHEVRON_RIGHT)
                        } else {
                            prompt_str.clone()
                        };

                        let prompt_color = if is_running {
                            Color32::from_rgb(16, 185, 129)
                        } else {
                            p.accent
                        };

                        ui.label(
                            RichText::new(&prompt_label)
                                .size(12.5)
                                .strong()
                                .monospace()
                                .color(prompt_color),
                        );

                        let hint = if is_running {
                            "Send input to running process (or Ctrl+C to interrupt)..."
                        } else {
                            "Type command (Tab to complete, Enter to run)..."
                        };

                        let text_edit = egui::TextEdit::singleline(&mut term.input_text)
                            .frame(false)
                            .font(egui::TextStyle::Monospace)
                            .text_color(Color32::from_rgb(230, 235, 245))
                            .hint_text(hint)
                            .desired_width(ui.available_width() - 80.0);

                        let resp = ui.add(text_edit);

                        if term.request_focus || should_focus {
                            resp.request_focus();
                            term.request_focus = false;
                        }

                        // Enter submission (keeps focus!)
                        let enter_hit = (resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)))
                            || (resp.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)));

                        let mut to_execute = None;
                        if enter_hit {
                            resp.request_focus();
                            let cmd = term.input_text.clone();
                            term.input_text.clear();
                            to_execute = Some(cmd);
                        }

                        // Ctrl + C (SIGINT or cancel line)
                        let ctrl_c = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::C))
                            || ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::C));
                        if ctrl_c {
                            if is_running {
                                term.interrupt();
                            } else {
                                term.cancel_line();
                            }
                            resp.request_focus();
                        }

                        // Ctrl + L (Clear screen)
                        let ctrl_l = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::L))
                            || ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::L));
                        if ctrl_l {
                            term.clear();
                            resp.request_focus();
                        }

                        // Tab (Auto-completion for files and paths)
                        let tab_hit = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab));
                        if tab_hit && !is_running {
                            term.autocomplete_path();
                            resp.request_focus();
                        }

                        // Up / Down arrow history
                        if resp.has_focus() {
                            if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                                if !term.history.is_empty() {
                                    let next_idx = match term.history_idx {
                                        None => term.history.len().saturating_sub(1),
                                        Some(i) => i.saturating_sub(1),
                                    };
                                    if let Some(cmd) = term.history.get(next_idx).cloned() {
                                        term.input_text = cmd;
                                        term.history_idx = Some(next_idx);
                                    }
                                }
                            } else if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                                if let Some(i) = term.history_idx {
                                    if i + 1 < term.history.len() {
                                        let next_idx = i + 1;
                                        if let Some(cmd) = term.history.get(next_idx).cloned() {
                                            term.input_text = cmd;
                                            term.history_idx = Some(next_idx);
                                        }
                                    } else {
                                        term.input_text.clear();
                                        term.history_idx = None;
                                    }
                                }
                            }
                        }

                        // Inline Action Button
                        if is_running {
                            let btn_stop = egui::Button::new(
                                RichText::new(format!("{} Stop", crate::lucide::STOP)).size(11.0).strong().color(Color32::WHITE),
                            )
                            .fill(p.danger)
                            .rounding(Rounding::same(4.0));
                            if ui.add(btn_stop).on_hover_text("Interrupt running process (Ctrl+C)").clicked() {
                                term.interrupt();
                                resp.request_focus();
                            }
                        } else {
                            let btn_run = egui::Button::new(
                                RichText::new(format!("{} Run", crate::lucide::PLAY)).size(11.0).strong().color(Color32::WHITE),
                            )
                            .fill(p.accent)
                            .rounding(Rounding::same(4.0));
                            if ui.add(btn_run).on_hover_text("Run command (Enter)").clicked() {
                                to_execute = Some(term.input_text.clone());
                                term.input_text.clear();
                                resp.request_focus();
                            }
                        }

                        // Execution dispatch
                        if let Some(cmd) = to_execute {
                            if is_running {
                                term.send_stdin(&cmd);
                            } else if let Some(h) = &host_opt {
                                term.execute_remote(h, &cmd);
                            } else {
                                term.execute_local(&cmd, "");
                            }
                        }
                    });
                });
        });
    }

    // ═════════════════════════════════════════════════════════════════════════
    // SYSTEMD TAB: Services List, Controller, and Journalctl Viewer
    // ═════════════════════════════════════════════════════════════════════════
    fn render_systemd_left_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.theme.palette();
        let user_mode = if let Some(hid) = host_id {
            *self.remote_services_user.entry(hid.to_string()).or_insert(true)
        } else {
            self.local_services_user
        };
        let host_opt = host_id.and_then(|hid| self.ssh_hosts.iter().find(|h| h.id == hid).cloned());
        let s_key = host_id.map(|hid| format!("{}:{}", hid, if user_mode { "user" } else { "system" }));

        ui.set_width(ui.available_width());
        ui.set_max_width(ui.available_width());
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.set_max_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("SYSTEMD SERVICES").size(12.0).strong().color(p.text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_new = egui::Button::new(
                        RichText::new(format!("{} New Service", crate::lucide::PLUS)).size(11.0).strong().color(Color32::WHITE),
                    )
                    .fill(p.accent)
                    .rounding(Rounding::same(4.0));
                    if ui.add(btn_new).clicked() {
                        self.systemd_modal.open();
                    }
                });
            });

            ui.add_space(6.0);

            // Scope & Refresh Row
            ui.horizontal(|ui| {
                let user_btn = egui::Button::new(
                    RichText::new("User (--user)")
                        .size(10.5)
                        .color(if user_mode { p.accent_text } else { p.text_secondary }),
                )
                .fill(if user_mode { p.accent } else { p.bg_button })
                .rounding(Rounding::same(4.0));
                if ui.add(user_btn).clicked() {
                    if let Some(hid) = host_id {
                        self.remote_services_user.insert(hid.to_string(), true);
                        if let Some(host) = &host_opt {
                            self.trigger_remote_services_list(host, true);
                        }
                    } else {
                        self.local_services_user = true;
                        self.trigger_local_services_list(true);
                    }
                }

                let sys_btn = egui::Button::new(
                    RichText::new("System")
                        .size(10.5)
                        .color(if !user_mode { p.accent_text } else { p.text_secondary }),
                )
                .fill(if !user_mode { p.accent } else { p.bg_button })
                .rounding(Rounding::same(4.0));
                if ui.add(sys_btn).clicked() {
                    if let Some(hid) = host_id {
                        self.remote_services_user.insert(hid.to_string(), false);
                        if let Some(host) = &host_opt {
                            self.trigger_remote_services_list(host, false);
                        }
                    } else {
                        self.local_services_user = false;
                        self.trigger_local_services_list(false);
                    }
                }

                if host_id.is_none() && !self.local_sudo_password.is_empty() {
                    ui.add_space(4.0);
                    if ui.button(RichText::new(format!("{} Sudo [{}]", crate::lucide::KEY, crate::lucide::X)).size(10.0).color(Color32::from_rgb(16, 185, 129)))
                        .on_hover_text("Sudo password cached for this session. Click to clear/forget.")
                        .clicked()
                    {
                        self.local_sudo_password.clear();
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_ref = egui::Button::new(RichText::new(crate::lucide::REFRESH_CW).size(11.0).color(p.text_secondary))
                        .fill(p.bg_button);
                    if ui.add(btn_ref).on_hover_text("Refresh systemd services list").clicked() {
                        if host_id.is_some() {
                            if let Some(host) = &host_opt {
                                self.trigger_remote_services_list(host, user_mode);
                            }
                        } else {
                            self.trigger_local_services_list(user_mode);
                        }
                    }
                });
            });

            ui.add_space(6.0);

            // Auto-load services on first open
            if host_id.is_none() && !self.local_services_loaded {
                self.local_services_loaded = true;
                self.trigger_local_services_list(self.local_services_user);
            } else if let (Some(host), Some(ref key)) = (&host_opt, &s_key) {
                let has_cached = self.remote_services.lock().map(|m| m.contains_key(key)).unwrap_or(false);
                let is_in_flight = self.remote_services_in_flight.lock().map(|s| s.contains(key)).unwrap_or(false);
                if !has_cached && !is_in_flight {
                    self.trigger_remote_services_list(host, user_mode);
                }
            }

            // Sync local services cache if available
            if host_id.is_none() {
                if let Ok(cache) = self.local_services_cache.lock() {
                    if let Some(list) = cache.get(&user_mode) {
                        self.local_services = list.clone();
                    }
                }
            }

            // Search filter
            ui.add(
                egui::TextEdit::singleline(&mut self.service_search_query)
                    .hint_text(format!("{} Filter services...", crate::lucide::SEARCH))
                    .desired_width(ui.available_width()),
            );

            ui.add_space(8.0);

            let q = self.service_search_query.trim().to_lowercase();
            let services_list: Vec<SystemdService> = if let Some(ref key) = s_key {
                self.remote_services.lock().map(|m| m.get(key).cloned().unwrap_or_default()).unwrap_or_default()
            } else {
                self.local_services_cache.lock().map(|m| m.get(&user_mode).cloned().unwrap_or_default()).unwrap_or_else(|_| self.local_services.clone())
            };

            let is_services_loading = if let Some(ref k) = s_key {
                self.remote_services_in_flight.lock().map(|s| s.contains(k)).unwrap_or(false)
            } else {
                self.local_services_in_flight.lock().map(|s| s.contains(&user_mode)).unwrap_or(false)
            };

            let filtered_services: Vec<SystemdService> = services_list
                .into_iter()
                .filter(|s| {
                    q.is_empty()
                        || s.name.to_lowercase().contains(&q)
                        || s.description.to_lowercase().contains(&q)
                        || s.sub_state.to_lowercase().contains(&q)
                })
                .collect();

            if filtered_services.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    if is_services_loading {
                        ui.spinner();
                        ui.add_space(6.0);
                        ui.label(RichText::new("Loading systemd services...").size(12.0).color(p.text_muted));
                    } else if let Some(ref key) = s_key {
                        let err_opt = self.remote_services_errors.lock().ok()
                            .and_then(|e| e.get(key).cloned())
                            .filter(|e| !e.is_empty()); // skip empty error strings
                        if let Some(err) = err_opt {
                            ui.label(RichText::new(format!("{} Failed to load services", crate::lucide::ALERT_TRIANGLE)).size(12.0).strong().color(p.danger));
                            ui.add_space(4.0);
                            ui.label(RichText::new(&err).size(11.0).color(p.danger));
                            ui.add_space(8.0);
                            ui.label(RichText::new(format!("Click '{}' to retry.", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_muted));
                        } else {
                            ui.label(RichText::new("No systemd services found").size(12.0).color(p.text_muted));
                            ui.add_space(4.0);
                            ui.label(RichText::new(format!("Click '{}' to refresh or switch scope (User / System).", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_muted));
                        }
                    } else {
                        ui.label(RichText::new("No systemd services found").size(12.0).color(p.text_muted));
                        ui.add_space(4.0);
                        ui.label(RichText::new(format!("Click '{}' to scan or switch scope.", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_muted));
                    }
                });
                return;
            }

            ScrollArea::vertical()
                .id_salt("systemd_services_scroll")
                .auto_shrink([false, false])
                .show_rows(ui, 54.0, filtered_services.len(), |ui, row_range| {
                    ui.set_width(ui.available_width());
                    ui.set_max_width(ui.available_width());
                    let mut selected = None;

                    for idx in row_range {
                        let s = &filtered_services[idx];
                        let is_active = s.is_active();
                        let is_failed = s.is_failed();
                        let is_selected = self.selected_service.as_deref() == Some(&s.name);

                        let card_resp = Frame::none()
                            .fill(if is_selected { p.bg_card } else { p.bg_sidebar })
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.set_max_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    let status_dot = if is_active {
                                        RichText::new(crate::lucide::CIRCLE_DOT).size(11.0).color(Color32::from_rgb(16, 185, 129))
                                    } else if is_failed {
                                        RichText::new(crate::lucide::X).size(11.0).color(p.danger)
                                    } else {
                                        RichText::new(crate::lucide::CIRCLE).size(11.0).color(p.text_muted)
                                    };
                                    ui.label(status_dot);

                                    let status_w = 68.0;
                                    let name_w = (ui.available_width() - status_w - 6.0).max(40.0);

                                    let title = RichText::new(&s.name).size(12.0).strong().color(if is_selected { p.accent_light } else { p.text_primary });
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(name_w, 16.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.set_max_width(name_w);
                                            ui.add(
                                                egui::Label::new(title)
                                                    .truncate(),
                                            )
                                            .on_hover_text(&s.name);
                                        },
                                    );

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let sub_color = if is_active {
                                            Color32::from_rgb(16, 185, 129)
                                        } else if is_failed {
                                            p.danger
                                        } else {
                                            p.text_muted
                                        };
                                        ui.label(RichText::new(&s.sub_state).size(10.5).color(sub_color));
                                    });
                                });

                                if !s.description.is_empty() {
                                    ui.add_space(2.0);
                                    let desc_w = ui.available_width().max(40.0);
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(desc_w, 14.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.set_max_width(desc_w);
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&s.description)
                                                        .size(10.5)
                                                        .color(p.text_muted),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_text(&s.description);
                                        },
                                    );
                                }
                            });

                        let whole_card_click = ui.interact(
                            card_resp.response.rect,
                            card_resp.response.id.with("whole_card_click"),
                            egui::Sense::click(),
                        );
                        if whole_card_click.clicked() {
                            selected = Some(s.name.clone());
                        }
                        if whole_card_click.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }

                        ui.add_space(5.0);
                    }

                    if let Some(unit) = selected {
                        self.select_systemd_unit(&unit, user_mode, host_opt.as_ref());
                    }
                });
        });
    }

    fn render_systemd_right_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.theme.palette();
        let user_mode = if let Some(hid) = host_id {
            *self.remote_services_user.entry(hid.to_string()).or_insert(true)
        } else {
            self.local_services_user
        };
        let host_opt = host_id.and_then(|hid| self.ssh_hosts.iter().find(|h| h.id == hid).cloned());
        let s_key = host_id.map(|hid| format!("{}:{}", hid, if user_mode { "user" } else { "system" }));

        let services_list: Vec<SystemdService> = if let Some(ref key) = s_key {
            self.remote_services.lock().map(|m| m.get(key).cloned().unwrap_or_default()).unwrap_or_default()
        } else {
            self.local_services.clone()
        };

        let selected_unit = self.selected_service.clone();
        let service_opt = selected_unit.as_ref().and_then(|u| {
            services_list.iter().find(|s| &s.name == u).cloned().or_else(|| {
                Some(SystemdService {
                    name: u.clone(),
                    load_state: "loaded".to_string(),
                    active_state: "active".to_string(),
                    sub_state: "".to_string(),
                    description: String::new(),
                    is_user: user_mode,
                })
            })
        });

        match service_opt {
            Some(s) => {
                let is_active = s.is_active();
                let is_failed = s.is_failed();

                let action_key = host_id.map(|hid| format!("{}:{}", hid, s.name));
                let is_action_in_flight = action_key.as_ref().map(|k| {
                    self.remote_service_action_in_flight.lock().map(|set| set.contains(k)).unwrap_or(false)
                }).unwrap_or(false);

                let local_j_key = format!("{}:{}", s.name, if user_mode { "user" } else { "system" });
                let is_local_journal_in_flight = self.local_journal_in_flight.lock().map(|set| set.contains(&local_j_key)).unwrap_or(false);

                let j_key = host_id.map(|hid| format!("{}:{}:{}", hid, s.name, if user_mode { "user" } else { "system" }));
                let is_journal_in_flight = if host_id.is_some() {
                    j_key.as_ref().map(|k| {
                        self.remote_journal_in_flight.lock().map(|set| set.contains(k)).unwrap_or(false)
                    }).unwrap_or(false)
                } else {
                    is_local_journal_in_flight
                };

                // Auto-fetch journal logs on first selection if not yet loaded
                if let (Some(host), Some(ref jk)) = (&host_opt, &j_key) {
                    let has_logs = self.remote_journal_logs.lock().map(|m| m.contains_key(jk)).unwrap_or(false);
                    if !has_logs && !is_journal_in_flight {
                        self.trigger_remote_journal_fetch(host, &s.name, user_mode);
                    }
                } else if host_id.is_none() {
                    let has_logs = self.local_journal_logs.lock().map(|m| m.contains_key(&local_j_key)).unwrap_or(false);
                    if !has_logs && !is_journal_in_flight {
                        let pass_opt = if !user_mode && !self.local_sudo_password.is_empty() {
                            Some(self.local_sudo_password.as_str())
                        } else {
                            None
                        };
                        self.trigger_local_journal_fetch(&s.name, user_mode, pass_opt);
                    }
                }

                ui.vertical(|ui| {
                    // Header Bar
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(14.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::lucide::SYSTEMD).size(20.0).color(p.accent));
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&s.name).size(15.0).strong().color(p.text_primary));
                                        let status_tag = if is_active {
                                            RichText::new(format!("{} Active (Running)", crate::lucide::CHECK)).size(11.0).strong().color(Color32::from_rgb(16, 185, 129))
                                        } else if is_failed {
                                            RichText::new(format!("{} Failed", crate::lucide::X)).size(11.0).strong().color(p.danger)
                                        } else {
                                            RichText::new(format!("{} Inactive", crate::lucide::CIRCLE)).size(11.0).color(p.text_muted)
                                        };
                                        ui.label(status_tag);
                                    });

                                    ui.label(
                                        RichText::new(format!(
                                            "Load: {} | Sub: {} | Scope: {}",
                                            s.load_state,
                                            s.sub_state,
                                            if user_mode { "User (--user)" } else { "System" }
                                        ))
                                        .size(11.0)
                                        .color(p.text_muted),
                                    );

                                    if !s.description.is_empty() {
                                        ui.label(RichText::new(&s.description).size(11.0).color(p.text_secondary));
                                    }
                                });
                            });

                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(6.0);

                            // Control buttons row:
                            // - if running: only show stop; if stopped: only show start
                            // - if disabled: only show enable; if enabled: only show disable
                            let is_enabled = self.get_or_query_service_enabled(&s.name, user_mode, host_opt.as_ref());
                            let mut action_to_run = None;
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                if is_action_in_flight {
                                    ui.spinner();
                                    ui.add_space(4.0);
                                    ui.label(RichText::new("Executing remote action...").size(11.0).color(p.accent));
                                } else {
                                    if is_active {
                                        let btn_stop = egui::Button::new(RichText::new(format!("{} Stop", crate::lucide::STOP)).size(11.0).strong().color(Color32::WHITE))
                                            .fill(p.danger)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(999.0))
                                            .min_size(egui::vec2(76.0, 26.0));
                                        if ui.add(btn_stop).clicked() {
                                            action_to_run = Some("stop");
                                        }
                                    } else {
                                        let btn_start = egui::Button::new(RichText::new(format!("{} Start", crate::lucide::PLAY)).size(11.0).strong().color(Color32::WHITE))
                                            .fill(Color32::from_rgb(16, 185, 129))
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(999.0))
                                            .min_size(egui::vec2(76.0, 26.0));
                                        if ui.add(btn_start).clicked() {
                                            action_to_run = Some("start");
                                        }
                                    }

                                    let btn_restart = egui::Button::new(RichText::new(format!("{} Restart", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_primary))
                                        .fill(p.bg_button)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .min_size(egui::vec2(86.0, 26.0));
                                    if ui.add(btn_restart).clicked() {
                                        action_to_run = Some("restart");
                                    }

                                    if is_enabled {
                                        let btn_disable = egui::Button::new(RichText::new(format!("{} Disable", crate::lucide::X)).size(11.0).color(p.text_primary))
                                            .fill(p.bg_button)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(999.0))
                                            .min_size(egui::vec2(86.0, 26.0));
                                        if ui.add(btn_disable).clicked() {
                                            action_to_run = Some("disable");
                                        }
                                    } else {
                                        let btn_enable = egui::Button::new(RichText::new(format!("{} Enable", crate::lucide::CHECK)).size(11.0).color(p.text_primary))
                                            .fill(p.bg_button)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(999.0))
                                            .min_size(egui::vec2(86.0, 26.0));
                                        if ui.add(btn_enable).clicked() {
                                            action_to_run = Some("enable");
                                        }
                                    }

                                    let btn_reload = egui::Button::new(RichText::new(format!("{} Reload Daemon", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_secondary))
                                        .fill(p.bg_button)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .min_size(egui::vec2(120.0, 26.0));
                                    if ui.add(btn_reload).clicked() {
                                        action_to_run = Some("daemon-reload");
                                    }
                                }
                            });

                            if let Some(act) = action_to_run {
                                let host_id_str = host_opt.as_ref().map(|h| h.id.as_str()).unwrap_or("local");
                                let cache_key = format!("{}:{}:{}", host_id_str, s.name, if user_mode { "user" } else { "system" });
                                if act == "enable" {
                                    self.service_enabled_cache.insert(cache_key, (true, Instant::now()));
                                } else if act == "disable" {
                                    self.service_enabled_cache.insert(cache_key, (false, Instant::now()));
                                }

                                if let Some(host) = &host_opt {
                                    self.trigger_remote_service_action(host, &s.name, act, user_mode);
                                } else {
                                    if !user_mode && self.local_sudo_password.is_empty() {
                                        self.sudo_popup.open = true;
                                        self.sudo_popup.pending = Some(PendingSudoAction::ServiceAction {
                                            unit: s.name.clone(),
                                            action: act,
                                            user_mode: false,
                                        });
                                        self.sudo_popup.password_input.clear();
                                        self.sudo_popup.error = None;
                                        self.sudo_popup.remember_password = true;
                                        self.sudo_popup.just_opened = true;
                                    } else {
                                        let pass_opt = if !user_mode && !self.local_sudo_password.is_empty() {
                                            Some(self.local_sudo_password.as_str())
                                        } else {
                                            None
                                        };
                                        let res = control_local_service(&s.name, act, user_mode, pass_opt);
                                        match res {
                                            Ok(msg) => self.service_action_msg = Some(msg),
                                            Err(e) => {
                                                if !user_mode && (e.contains("Sudo password needed") || e.contains("authentication") || e.contains("Incorrect sudo password")) {
                                                    self.local_sudo_password.clear();
                                                    self.sudo_popup.open = true;
                                                    self.sudo_popup.pending = Some(PendingSudoAction::ServiceAction {
                                                        unit: s.name.clone(),
                                                        action: act,
                                                        user_mode: false,
                                                    });
                                                    self.sudo_popup.password_input.clear();
                                                    self.sudo_popup.error = Some(e);
                                                    self.sudo_popup.remember_password = true;
                                                    self.sudo_popup.just_opened = true;
                                                } else {
                                                    self.service_action_msg = Some(format!("Error: {}", e));
                                                }
                                            }
                                        }
                                        self.trigger_local_services_list(user_mode);
                                        let pass_for_journal = if !user_mode && !self.local_sudo_password.is_empty() {
                                            Some(self.local_sudo_password.as_str())
                                        } else {
                                            None
                                        };
                                        self.trigger_local_journal_fetch(&s.name, user_mode, pass_for_journal);
                                    }
                                }
                            }
                        });

                    let action_msg = if host_id.is_some() {
                        self.remote_service_action_msg.lock().ok().and_then(|m| m.clone())
                    } else {
                        self.service_action_msg.clone()
                    };

                    if let Some(msg) = &action_msg {
                        ui.add_space(6.0);
                        Frame::none()
                            .fill(p.bg_subtle)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new(msg).size(11.0).color(p.accent));
                            });
                    }

                    ui.add_space(8.0);

                    // ═════════════════════════════════════════════════════════
                    // SystemMD Tabs: [ Status ] [ Files & Drop-ins ] [ Dependencies ]
                    // ═════════════════════════════════════════════════════════
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let tabs = [
                            (SystemdTab::Status, format!("{} Status", crate::lucide::ACTIVITY)),
                            (SystemdTab::Files, format!("{} Files & Drop-ins", crate::lucide::FILE_CODE)),
                            (SystemdTab::Dependencies, format!("{} Dependencies", crate::lucide::LAYERS)),
                        ];
                        for (tab, label) in tabs {
                            let is_sel = self.systemd_tab == tab;
                            let btn = egui::Button::new(
                                RichText::new(label)
                                    .size(11.5)
                                    .strong()
                                    .color(if is_sel { p.accent_text } else { p.text_secondary }),
                            )
                            .fill(if is_sel { p.accent } else { p.bg_button })
                            .stroke(if is_sel { Stroke::NONE } else { Stroke::new(1.0_f32, p.border.gamma_multiply(0.5)) })
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(86.0, 26.0));
                            if ui.add(btn).clicked() {
                                self.systemd_tab = tab;
                            }
                        }
                    });

                    ui.add_space(8.0);

                    match self.systemd_tab {
                        SystemdTab::Status => {
                            // Journalctl Logs Viewer
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Service Logs (journalctl -n 100)").size(12.0).strong().color(p.text_muted));
                                if is_journal_in_flight {
                                    ui.add_space(4.0);
                                    ui.spinner();
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let btn_ref_log = egui::Button::new(RichText::new(format!("{} Refresh Logs", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_secondary))
                                        .fill(p.bg_button);
                                    if ui.add(btn_ref_log).clicked() {
                                        if let Some(host) = &host_opt {
                                            self.trigger_remote_journal_fetch(host, &s.name, user_mode);
                                        } else {
                                            let pass_for_journal = if !user_mode && !self.local_sudo_password.is_empty() {
                                                Some(self.local_sudo_password.as_str())
                                            } else {
                                                None
                                            };
                                            self.trigger_local_journal_fetch(&s.name, user_mode, pass_for_journal);
                                        }
                                        self.last_journal_refresh = Instant::now();
                                    }

                                    // Refresh Interval Dropdown
                                    let prev_interval = self.journal_refresh_interval;
                                    let interval_label = match self.journal_refresh_interval {
                                        0 => format!("{} Auto: Off", crate::lucide::CLOCK),
                                        secs => format!("{} Auto: {}s", crate::lucide::CLOCK, secs),
                                    };
                                    egui::ComboBox::from_id_salt("journal_refresh_interval_combo")
                                        .selected_text(RichText::new(interval_label).size(11.0).color(p.text_primary))
                                        .width(105.0)
                                        .show_ui(ui, |ui| {
                                             ui.selectable_value(&mut self.journal_refresh_interval, 0, "Off (Manual)");
                                            ui.selectable_value(&mut self.journal_refresh_interval, 1, "Every 1s");
                                            ui.selectable_value(&mut self.journal_refresh_interval, 2, "Every 2s");
                                            ui.selectable_value(&mut self.journal_refresh_interval, 5, "Every 5s");
                                            ui.selectable_value(&mut self.journal_refresh_interval, 10, "Every 10s");
                                            ui.selectable_value(&mut self.journal_refresh_interval, 30, "Every 30s");
                                            ui.selectable_value(&mut self.journal_refresh_interval, 60, "Every 60s");
                                        });
                                    if self.journal_refresh_interval != prev_interval {
                                        self.last_journal_refresh = Instant::now();
                                        self.persist_config();
                                    }
                                });
                            });

                            // Auto-refresh timer check
                            if self.journal_refresh_interval > 0 {
                                ui.ctx().request_repaint_after(Duration::from_millis(500));
                                if self.last_journal_refresh.elapsed() >= Duration::from_secs(self.journal_refresh_interval as u64) {
                                    self.last_journal_refresh = Instant::now();
                                    if let Some(host) = &host_opt {
                                        self.trigger_remote_journal_fetch(host, &s.name, user_mode);
                                    } else {
                                        let pass_for_journal = if !user_mode && !self.local_sudo_password.is_empty() {
                                            Some(self.local_sudo_password.as_str())
                                        } else {
                                            None
                                        };
                                        self.trigger_local_journal_fetch(&s.name, user_mode, pass_for_journal);
                                    }
                                }
                            }

                            ui.add_space(4.0);

                            let display_logs = if let Some(ref jk) = j_key {
                                self.remote_journal_logs.lock().map(|m| m.get(jk).cloned()).unwrap_or(None).unwrap_or_else(|| {
                                    if is_journal_in_flight {
                                        "Fetching logs from remote host...".to_string()
                                    } else {
                                        format!("No journal logs received yet. Click '{} Refresh Logs'.", crate::lucide::REFRESH_CW)
                                    }
                                })
                            } else {
                                let local_key = format!("{}:{}", s.name, if user_mode { "user" } else { "system" });
                                self.local_journal_logs.lock().map(|m| m.get(&local_key).cloned()).unwrap_or(None).unwrap_or_else(|| {
                                    if is_journal_in_flight {
                                        "Fetching journal logs in background...".to_string()
                                    } else if !self.service_journal_logs.is_empty() {
                                        self.service_journal_logs.clone()
                                    } else {
                                        format!("No journal logs recorded yet. Click '{} Refresh Logs'.", crate::lucide::REFRESH_CW)
                                    }
                                })
                            };

                            let log_h = (ui.available_height() - 20.0).max(180.0);
                            Frame::none()
                                .fill(Color32::from_rgb(12, 14, 20))
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(8.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                .show(ui, |ui| {
                                    ui.set_height(log_h);
                                    ScrollArea::vertical()
                                        .id_salt("journalctl_scroll")
                                        .stick_to_bottom(true)
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            if display_logs.trim().is_empty() {
                                                ui.label(RichText::new("No journal logs recorded for this service.").size(11.5).color(Color32::from_rgb(100, 110, 130)));
                                            } else {
                                                ui.label(
                                                    RichText::new(&display_logs)
                                                        .size(11.0)
                                                        .monospace()
                                                        .color(Color32::from_rgb(220, 225, 235)),
                                                );
                                            }
                                        });
                                });
                        }
                        SystemdTab::Files => {
                            self.render_systemd_files_tab(ui, &s, user_mode, host_opt.as_ref());
                        }
                        SystemdTab::Dependencies => {
                            self.render_systemd_dependencies_tab(ui, &s, user_mode, host_opt.as_ref(), &services_list);
                        }
                    }
                });
            }
            None => {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(RichText::new(format!("{} SystemMD Services", crate::lucide::SYSTEMD)).size(16.0).strong().color(p.text_primary));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Select a systemd unit from the left panel to control, edit unit files & drop-ins, and inspect dependencies,\nor click '+ New Service' to create and enable a new systemd unit.")
                            .size(12.0)
                            .color(p.text_muted),
                    );
                });
            }
        }
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

    fn render_systemd_files_tab(
        &mut self,
        ui: &mut Ui,
        s: &SystemdService,
        user_mode: bool,
        host_opt: Option<&SshRemoteHost>,
    ) {
        let p = self.theme.palette();
        let (files_info, is_files_loading) = self.load_systemd_unit_files(&s.name, user_mode, host_opt);

        // Determine available files
        let mut available_paths = Vec::new();
        if let Some(ref fp) = files_info.fragment_path {
            available_paths.push((fp.clone(), false, s.name.clone()));
        }
        for dp in &files_info.drop_in_paths {
            let name = Path::new(dp).file_name().and_then(|n| n.to_str()).unwrap_or(dp).to_string();
            available_paths.push((dp.clone(), true, name));
        }

        if self.systemd_selected_file.is_none() && !available_paths.is_empty() {
            let (first_path, _, _) = &available_paths[0];
            self.systemd_selected_file = Some(first_path.clone());
        }

        let cur_path = self.systemd_selected_file.clone().unwrap_or_default();
        let file_cache_key = format!("{}:{}", host_opt.map(|h| h.id.as_str()).unwrap_or("local"), cur_path);
        let is_content_in_flight = self.systemd_file_content_in_flight.lock().map(|s| s.contains(&file_cache_key)).unwrap_or(false);

        // Auto-read selected file into editor if cached or trigger async read
        if !cur_path.is_empty() {
            if let Ok(cache) = self.systemd_file_content_cache.lock() {
                if let Some(content) = cache.get(&file_cache_key) {
                    if self.systemd_file_original.is_empty() && self.systemd_file_content.is_empty() {
                        self.systemd_file_content = content.clone();
                        self.systemd_file_original = content.clone();
                    }
                } else if !is_content_in_flight {
                    let pass = if !user_mode && !self.local_sudo_password.is_empty() {
                        Some(self.local_sudo_password.as_str())
                    } else {
                        None
                    };
                    self.trigger_systemd_file_read(&cur_path, user_mode, host_opt, pass);
                }
            }
        }

        // Sub-toolbar: File Selector Pills & Actions
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
            ui.label(RichText::new("Unit Files:").size(11.5).strong().color(p.text_muted));
            if is_files_loading {
                ui.spinner();
            }

            let mut switch_to_path = None;
            for (path, is_dropin, label) in &available_paths {
                let is_current = self.systemd_selected_file.as_deref() == Some(path.as_str());
                let icon = if *is_dropin { crate::lucide::PACKAGE } else { crate::lucide::FILE_TEXT };
                let btn = egui::Button::new(
                    RichText::new(format!("{} {}", icon, label))
                        .size(11.0)
                        .color(if is_current { p.accent_text } else { p.text_secondary }),
                )
                .fill(if is_current { p.accent } else { p.bg_button })
                .stroke(if is_current { Stroke::NONE } else { Stroke::new(1.0_f32, p.border.gamma_multiply(0.5)) })
                .rounding(Rounding::same(999.0));

                if ui.add(btn).on_hover_text(format!("Path: {}\nType: {}", path, if *is_dropin { "Drop-In Override" } else { "Primary Unit File" })).clicked() {
                    switch_to_path = Some(path.clone());
                }
            }

            if let Some(new_p) = switch_to_path {
                self.systemd_selected_file = Some(new_p.clone());
                self.systemd_file_status_msg = None;
                let target_file_cache_key = format!("{}:{}", host_opt.map(|h| h.id.as_str()).unwrap_or("local"), new_p);
                let cached_opt = self.systemd_file_content_cache.lock().ok().and_then(|c| c.get(&target_file_cache_key).cloned());
                if let Some(content) = cached_opt {
                    self.systemd_file_content = content.clone();
                    self.systemd_file_original = content;
                } else {
                    self.systemd_file_content.clear();
                    self.systemd_file_original.clear();
                    let pass = if !user_mode && !self.local_sudo_password.is_empty() {
                        Some(self.local_sudo_password.as_str())
                    } else {
                        None
                    };
                    self.trigger_systemd_file_read(&new_p, user_mode, host_opt, pass);
                }
            }

            // + New Drop-In Button
            let btn_new_dropin = egui::Button::new(
                RichText::new(format!("{} New Drop-In", crate::lucide::PLUS)).size(11.0).strong().color(Color32::WHITE),
            )
            .fill(p.accent)
            .rounding(Rounding::same(999.0));
            if ui.add(btn_new_dropin).on_hover_text("Create a new configuration drop-in override (e.g. override.conf)").clicked() {
                self.systemd_new_dropin_open = !self.systemd_new_dropin_open;
            }

            // Refresh Files button
            let btn_ref = egui::Button::new(RichText::new(format!("{} Refresh", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_secondary))
                .fill(p.bg_button)
                .rounding(Rounding::same(999.0));
            if ui.add(btn_ref).on_hover_text("Reload files and re-read from disk").clicked() {
                let cache_key = format!("{}:{}:{}", host_opt.map(|h| h.id.as_str()).unwrap_or("local"), s.name, user_mode);
                if let Ok(mut c) = self.systemd_files_cache.lock() {
                    c.remove(&cache_key);
                }
                if let Ok(mut c) = self.systemd_file_content_cache.lock() {
                    c.clear();
                }
                self.systemd_selected_file = None;
                self.systemd_file_content.clear();
                self.systemd_file_original.clear();
                self.trigger_systemd_unit_files_fetch(&s.name, user_mode, host_opt);
            }
        });

        // New Drop-In Dialog Card (if open)
        if self.systemd_new_dropin_open {
            ui.add_space(4.0);
            Frame::none()
                .fill(p.bg_card)
                .stroke(Stroke::new(1.0_f32, p.accent.gamma_multiply(0.4)))
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} Create New Drop-In Override for {}", crate::lucide::PACKAGE, s.name)).size(13.0).strong().color(p.accent));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(RichText::new(crate::lucide::CLOSE).size(12.0).color(p.text_muted)).clicked() {
                                self.systemd_new_dropin_open = false;
                            }
                        });
                    });
                    ui.label(RichText::new("Drop-ins safely extend or override unit settings without modifying package-managed files.").size(10.5).color(p.text_muted));
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Filename:").size(11.0).color(p.text_secondary));
                        ui.add(egui::TextEdit::singleline(&mut self.systemd_new_dropin_name).desired_width(160.0).font(egui::TextStyle::Monospace));
                        let dropin_dir_preview = if user_mode {
                            format!("~/.config/systemd/user/{}.d/{}", s.name, self.systemd_new_dropin_name.trim())
                        } else {
                            format!("/etc/systemd/system/{}.d/{}", s.name, self.systemd_new_dropin_name.trim())
                        };
                        ui.label(RichText::new(format!("→ {}", dropin_dir_preview)).size(10.5).color(p.text_muted));
                    });

                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 3.0);
                        ui.label(RichText::new("Templates:").size(10.5).color(p.text_muted));
                        let templates = [
                            ("ExecStart Override", "[Service]\nExecStart=\nExecStart=/usr/bin/my-app --flags\n"),
                            ("Environment Vars", "[Service]\nEnvironment=\"PORT=8080\"\nEnvironment=\"NODE_ENV=production\"\n"),
                            ("Restart Policy", "[Service]\nRestart=always\nRestartSec=5s\n"),
                            ("Resource Limits", "[Service]\nMemoryMax=2G\nCPUQuota=100%\n"),
                            ("Empty", "[Service]\n"),
                        ];
                        for (idx, (t_name, t_body)) in templates.iter().enumerate() {
                            let btn = egui::Button::new(RichText::new(*t_name).size(10.5).color(if self.systemd_new_dropin_template == idx { p.accent_text } else { p.accent }))
                                .fill(if self.systemd_new_dropin_template == idx { p.accent } else { p.bg_button })
                                .rounding(Rounding::same(999.0));
                            if ui.add(btn).clicked() {
                                self.systemd_new_dropin_template = idx;
                                self.systemd_file_content = t_body.to_string();
                            }
                        }
                    });

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let btn_create = egui::Button::new(RichText::new(format!("{} Create Drop-In", crate::lucide::CHECK)).size(11.0).strong().color(Color32::WHITE))
                            .fill(Color32::from_rgb(16, 185, 129))
                            .rounding(Rounding::same(999.0));
                        if ui.add(btn_create).clicked() {
                            let clean_name = if self.systemd_new_dropin_name.trim().ends_with(".conf") {
                                self.systemd_new_dropin_name.trim().to_string()
                            } else {
                                format!("{}.conf", self.systemd_new_dropin_name.trim())
                            };
                            let target_path = if user_mode {
                                let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
                                format!("{}/.config/systemd/user/{}.d/{}", home, s.name, clean_name)
                            } else {
                                format!("/etc/systemd/system/{}.d/{}", s.name, clean_name)
                            };

                            let initial_content = if self.systemd_file_content.trim().is_empty() {
                                "[Service]\n".to_string()
                            } else {
                                self.systemd_file_content.clone()
                            };

                            if let Some(host) = host_opt {
                                match crate::systemd::write_remote_unit_file(host, &target_path, &initial_content, user_mode) {
                                    Ok(msg) => {
                                        self.systemd_file_status_msg = Some((msg, false));
                                        self.systemd_selected_file = Some(target_path.clone());
                                        self.systemd_file_original = initial_content;
                                        self.systemd_new_dropin_open = false;
                                        let cache_key = format!("{}:{}:{}", host.id, s.name, user_mode);
                                        if let Ok(mut c) = self.systemd_files_cache.lock() {
                                            c.remove(&cache_key);
                                        }
                                    }
                                    Err(e) => self.systemd_file_status_msg = Some((e, true)),
                                }
                            } else {
                                if !user_mode && self.local_sudo_password.is_empty() {
                                    self.sudo_popup.open = true;
                                    self.sudo_popup.pending = Some(PendingSudoAction::SaveUnitFile {
                                        path: target_path.clone(),
                                        content: initial_content.clone(),
                                        user_mode,
                                    });
                                    self.sudo_popup.password_input.clear();
                                    self.sudo_popup.error = None;
                                    self.sudo_popup.remember_password = true;
                                    self.sudo_popup.just_opened = true;
                                    self.systemd_selected_file = Some(target_path);
                                    self.systemd_new_dropin_open = false;
                                } else {
                                    let pass = if !user_mode && !self.local_sudo_password.is_empty() {
                                        Some(self.local_sudo_password.as_str())
                                    } else {
                                        None
                                    };
                                    match crate::systemd::write_local_unit_file(&target_path, &initial_content, user_mode, pass) {
                                        Ok(msg) => {
                                            self.systemd_file_status_msg = Some((msg, false));
                                            self.systemd_selected_file = Some(target_path.clone());
                                            self.systemd_file_original = initial_content;
                                            self.systemd_new_dropin_open = false;
                                            let cache_key = format!("local:{}:{}", s.name, user_mode);
                                            if let Ok(mut c) = self.systemd_files_cache.lock() {
                                                c.remove(&cache_key);
                                            }
                                        }
                                        Err(e) => self.systemd_file_status_msg = Some((e, true)),
                                    }
                                }
                            }
                        }
                    });
                });
        }

        ui.add_space(6.0);

        // Editor Control Card Header
        let cur_path = self.systemd_selected_file.clone().unwrap_or_default();
        let is_dropin = cur_path.contains(".d/");
        let is_modified = self.systemd_file_content != self.systemd_file_original;

        Frame::none()
            .fill(p.bg_card)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} {}", if is_dropin { crate::lucide::PACKAGE } else { crate::lucide::FILE_TEXT }, &cur_path)).size(11.5).monospace().color(p.accent_light));
                    if is_dropin {
                        ui.label(RichText::new("Drop-In Override").size(10.0).color(Color32::from_rgb(56, 189, 248)));
                    } else {
                        ui.label(RichText::new("Primary Unit").size(10.0).color(p.text_muted));
                    }
                    if is_modified {
                        ui.label(RichText::new("● Unsaved Changes").size(10.5).strong().color(Color32::from_rgb(250, 204, 21)));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;

                        // Quick restart service button
                        let btn_restart = egui::Button::new(RichText::new(format!("{} Restart Service", crate::lucide::REFRESH_CW)).size(10.5).color(p.text_secondary))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(999.0));
                        if ui.add(btn_restart).on_hover_text("Restart service to activate new configuration").clicked() {
                            if let Some(host) = host_opt {
                                self.trigger_remote_service_action(host, &s.name, "restart", user_mode);
                            } else {
                                let pass = if !user_mode && !self.local_sudo_password.is_empty() {
                                    Some(self.local_sudo_password.as_str())
                                } else {
                                    None
                                };
                                let _ = crate::systemd::control_local_service(&s.name, "restart", user_mode, pass);
                            }
                        }

                        // Delete Drop-In (if dropin)
                        if is_dropin {
                            let btn_del = egui::Button::new(RichText::new(format!("{} Delete Drop-In", crate::lucide::TRASH)).size(10.5).strong().color(Color32::WHITE))
                                .fill(p.danger)
                                .rounding(Rounding::same(999.0));
                            if ui.add(btn_del).on_hover_text("Remove this drop-in override and reload daemon").clicked() {
                                if let Some(host) = host_opt {
                                    match crate::systemd::delete_remote_drop_in(host, &cur_path, user_mode) {
                                        Ok(msg) => {
                                            self.systemd_file_status_msg = Some((msg, false));
                                            self.systemd_selected_file = None;
                                            if let Ok(mut c) = self.systemd_files_cache.lock() {
                                                c.clear();
                                            }
                                        }
                                        Err(e) => self.systemd_file_status_msg = Some((e, true)),
                                    }
                                } else {
                                    if !user_mode && self.local_sudo_password.is_empty() {
                                        self.sudo_popup.open = true;
                                        self.sudo_popup.pending = Some(PendingSudoAction::DeleteDropIn {
                                            path: cur_path.clone(),
                                            user_mode,
                                        });
                                        self.sudo_popup.password_input.clear();
                                        self.sudo_popup.error = None;
                                        self.sudo_popup.remember_password = true;
                                        self.sudo_popup.just_opened = true;
                                    } else {
                                        let pass = if !user_mode && !self.local_sudo_password.is_empty() {
                                            Some(self.local_sudo_password.as_str())
                                        } else {
                                            None
                                        };
                                        match crate::systemd::delete_local_drop_in(&cur_path, user_mode, pass) {
                                            Ok(msg) => {
                                                self.systemd_file_status_msg = Some((msg, false));
                                                self.systemd_selected_file = None;
                                                if let Ok(mut c) = self.systemd_files_cache.lock() {
                                                    c.clear();
                                                }
                                            }
                                            Err(e) => self.systemd_file_status_msg = Some((e, true)),
                                        }
                                    }
                                }
                            }
                        }

                        // Revert Button
                        if is_modified {
                            let btn_revert = egui::Button::new(RichText::new("Revert").size(10.5).color(p.text_secondary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(999.0));
                            if ui.add(btn_revert).on_hover_text("Discard changes and restore original file").clicked() {
                                self.systemd_file_content = self.systemd_file_original.clone();
                            }
                        }

                        // Save & Reload Button
                        let btn_save = egui::Button::new(
                            RichText::new(format!("{} Save & Reload", crate::lucide::SAVE)).size(10.5).strong().color(Color32::WHITE),
                        )
                        .fill(if is_modified { p.accent } else { p.bg_button })
                        .rounding(Rounding::same(999.0));
                        if ui.add(btn_save).on_hover_text("Save file to disk and execute systemctl daemon-reload").clicked() {
                            if let Some(host) = host_opt {
                                match crate::systemd::write_remote_unit_file(host, &cur_path, &self.systemd_file_content, user_mode) {
                                    Ok(msg) => {
                                        self.systemd_file_original = self.systemd_file_content.clone();
                                        self.systemd_file_status_msg = Some((msg, false));
                                    }
                                    Err(e) => self.systemd_file_status_msg = Some((e, true)),
                                }
                            } else {
                                if !user_mode && self.local_sudo_password.is_empty() {
                                    self.sudo_popup.open = true;
                                    self.sudo_popup.pending = Some(PendingSudoAction::SaveUnitFile {
                                        path: cur_path.clone(),
                                        content: self.systemd_file_content.clone(),
                                        user_mode,
                                    });
                                    self.sudo_popup.password_input.clear();
                                    self.sudo_popup.error = None;
                                    self.sudo_popup.remember_password = true;
                                    self.sudo_popup.just_opened = true;
                                } else {
                                    let pass = if !user_mode && !self.local_sudo_password.is_empty() {
                                        Some(self.local_sudo_password.as_str())
                                    } else {
                                        None
                                    };
                                    match crate::systemd::write_local_unit_file(&cur_path, &self.systemd_file_content, user_mode, pass) {
                                        Ok(msg) => {
                                            self.systemd_file_original = self.systemd_file_content.clone();
                                            self.systemd_file_status_msg = Some((msg, false));
                                        }
                                        Err(e) => self.systemd_file_status_msg = Some((e, true)),
                                    }
                                }
                            }
                        }
                    });
                });
            });

        // Status banner
        if let Some((ref msg, is_err)) = self.systemd_file_status_msg {
            ui.add_space(4.0);
            Frame::none()
                .fill(if is_err { p.danger.gamma_multiply(0.2) } else { p.bg_card })
                .stroke(Stroke::new(1.0_f32, if is_err { p.danger } else { p.accent }))
                .rounding(Rounding::same(6.0))
                .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                .show(ui, |ui| {
                    ui.label(RichText::new(msg).size(11.0).color(if is_err { p.danger } else { p.accent }));
                });
        }

        ui.add_space(4.0);

        // Code Editor Viewport
        let edit_h = (ui.available_height() - 16.0).max(180.0);
        Frame::none()
            .fill(Color32::from_rgb(12, 14, 20))
            .stroke(Stroke::new(1.0_f32, p.border.gamma_multiply(0.6)))
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.set_height(edit_h);
                ScrollArea::both()
                    .id_salt("systemd_file_editor_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.systemd_file_content)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .desired_rows(25)
                                .code_editor()
                                .lock_focus(true),
                        );
                    });
            });
    }

    fn render_systemd_dependencies_tab(
        &mut self,
        ui: &mut Ui,
        s: &SystemdService,
        user_mode: bool,
        host_opt: Option<&SshRemoteHost>,
        services_list: &[SystemdService],
    ) {
        let p = self.theme.palette();
        let (deps, is_deps_loading) = self.load_systemd_unit_dependencies(&s.name, user_mode, host_opt);

        let mut navigate_to_unit: Option<String> = None;

        // Top Toolbar
        ui.horizontal(|ui| {
            // History Back button
            if let Some(prev) = self.systemd_deps_history.last() {
                let btn_back = egui::Button::new(
                    RichText::new(format!("{} Back to {}", crate::lucide::ARROW_LEFT, prev)).size(11.0).strong().color(p.accent_text),
                )
                .fill(p.accent)
                .rounding(Rounding::same(999.0));
                if ui.add(btn_back).on_hover_text("Navigate back to previously viewed unit").clicked() {
                    let prev_unit = self.systemd_deps_history.pop().unwrap();
                    self.select_systemd_unit(&prev_unit, user_mode, host_opt);
                    return;
                }
            }

            if is_deps_loading {
                ui.spinner();
            }

            // Filter box
            ui.label(RichText::new(crate::lucide::SEARCH).size(12.0).color(p.text_muted));
            ui.add(
                egui::TextEdit::singleline(&mut self.systemd_deps_filter)
                    .hint_text("Filter dependencies...")
                    .desired_width(180.0),
            );

            // View Mode segmented buttons
            let btn_cat = egui::Button::new(
                RichText::new("Categorized").size(11.0).color(if self.systemd_deps_view_mode == 0 { p.accent_text } else { p.text_secondary }),
            )
            .fill(if self.systemd_deps_view_mode == 0 { p.accent } else { p.bg_button })
            .rounding(Rounding::same(999.0));
            if ui.add(btn_cat).clicked() {
                self.systemd_deps_view_mode = 0;
            }

            let btn_tree = egui::Button::new(
                RichText::new("Dependency Tree").size(11.0).color(if self.systemd_deps_view_mode == 1 { p.accent_text } else { p.text_secondary }),
            )
            .fill(if self.systemd_deps_view_mode == 1 { p.accent } else { p.bg_button })
            .rounding(Rounding::same(999.0));
            if ui.add(btn_tree).clicked() {
                self.systemd_deps_view_mode = 1;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn_ref = egui::Button::new(RichText::new(format!("{} Refresh", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_secondary))
                    .fill(p.bg_button)
                    .rounding(Rounding::same(999.0));
                if ui.add(btn_ref).on_hover_text("Refresh dependencies").clicked() {
                    let cache_key = format!("{}:{}:{}", host_opt.map(|h| h.id.as_str()).unwrap_or("local"), s.name, user_mode);
                    if let Ok(mut c) = self.systemd_deps_cache.lock() {
                        c.remove(&cache_key);
                    }
                    self.trigger_systemd_unit_dependencies_fetch(&s.name, user_mode, host_opt);
                }
            });
        });

        ui.add_space(6.0);

        if is_deps_loading && deps.requires.is_empty() && deps.wants.is_empty() && deps.after.is_empty() && deps.tree_output.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.spinner();
                ui.add_space(6.0);
                ui.label(RichText::new("Resolving systemd dependency graph...").size(12.0).color(p.text_muted));
            });
            return;
        }

        if self.systemd_deps_view_mode == 0 {
            // Categorized View
            ScrollArea::vertical()
                .id_salt("systemd_deps_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let filter = self.systemd_deps_filter.trim().to_lowercase();

                    let categories = [
                        ("Requires (Hard Dependencies)", "Units that must be active for this unit to start. If any of them fails or stops, this unit is deactivated.", &deps.requires, Color32::from_rgb(239, 68, 68)),
                        ("Wants (Soft Dependencies)", "Suggested units that start alongside this unit. A failure does not stop this unit.", &deps.wants, Color32::from_rgb(56, 189, 248)),
                        ("After (Order After)", "This unit starts after these units have completed initialization.", &deps.after, Color32::from_rgb(168, 85, 247)),
                        ("Before (Order Before)", "These units only start after this unit has completed initialization.", &deps.before, Color32::from_rgb(147, 197, 253)),
                        ("Conflicts (Incompatible Units)", "Units that cannot run simultaneously with this unit.", &deps.conflicts, Color32::from_rgb(244, 63, 94)),
                        ("Required By (Reverse Hard Dependencies)", "Units on the system that strictly require this unit.", &deps.required_by, Color32::from_rgb(245, 158, 11)),
                        ("Wanted By (Reverse Soft Dependencies)", "Units on the system that declare Wants= on this unit.", &deps.wanted_by, Color32::from_rgb(34, 197, 94)),
                        ("Part Of & Binds To", "Units sharing life cycle binding with this unit.", &deps.part_of, Color32::from_rgb(192, 132, 252)),
                    ];

                    let mut any_shown = false;
                    for (cat_title, cat_desc, cat_units, accent_color) in categories {
                        let filtered_units: Vec<&String> = cat_units.iter()
                            .filter(|u| filter.is_empty() || u.to_lowercase().contains(&filter))
                            .collect();

                        if filtered_units.is_empty() && !filter.is_empty() {
                            continue;
                        }
                        any_shown = true;

                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::new(1.0_f32, p.border.gamma_multiply(0.4)))
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("● {}", cat_title)).size(12.0).strong().color(accent_color));
                                    ui.label(RichText::new(format!("({})", filtered_units.len())).size(11.0).color(p.text_muted));
                                });
                                ui.label(RichText::new(cat_desc).size(10.5).color(p.text_muted));
                                ui.add_space(4.0);

                                if filtered_units.is_empty() {
                                    ui.label(RichText::new("None declared").size(10.5).italics().color(p.text_muted));
                                } else {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                                        for unit_name in filtered_units {
                                            // Find unit status in services_list
                                            let s_status = services_list.iter().find(|s| s.name == **unit_name);
                                            let (dot, _dot_color) = if let Some(known) = s_status {
                                                if known.is_active() {
                                                    ("●", Color32::from_rgb(16, 185, 129))
                                                } else if known.is_failed() {
                                                    ("✕", p.danger)
                                                } else {
                                                    ("○", p.text_muted)
                                                }
                                            } else if unit_name.ends_with(".target") {
                                                ("🎯", Color32::from_rgb(148, 163, 184))
                                            } else if unit_name.ends_with(".socket") {
                                                ("🔌", Color32::from_rgb(56, 189, 248))
                                            } else if unit_name.ends_with(".slice") {
                                                ("🍰", Color32::from_rgb(168, 85, 247))
                                            } else {
                                                ("⚙", p.text_muted)
                                            };

                                            let btn = egui::Button::new(
                                                RichText::new(format!("{} {}", dot, unit_name))
                                                    .size(11.0)
                                                    .monospace()
                                                    .color(p.text_primary),
                                            )
                                            .fill(p.bg_button)
                                            .stroke(Stroke::new(1.0_f32, p.border.gamma_multiply(0.4)))
                                            .rounding(Rounding::same(999.0));

                                            if ui.add(btn).on_hover_text(format!("Click to view and inspect dependencies for {}", unit_name)).clicked() {
                                                navigate_to_unit = Some((*unit_name).clone());
                                            }
                                        }
                                    });
                                }
                            });
                        ui.add_space(6.0);
                    }

                    if !any_shown {
                        ui.add_space(20.0);
                        ui.label(RichText::new("No matching dependencies found").size(12.0).color(p.text_muted));
                    }
                });
        } else {
            // Dependency Tree View
            let tree_h = (ui.available_height() - 16.0).max(180.0);
            Frame::none()
                .fill(Color32::from_rgb(12, 14, 20))
                .stroke(Stroke::new(1.0_f32, p.border.gamma_multiply(0.6)))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.set_height(tree_h);
                    ScrollArea::both()
                        .id_salt("systemd_deps_tree_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if deps.tree_output.trim().is_empty() {
                                ui.label(RichText::new("No dependency tree output returned from systemctl list-dependencies.").size(11.0).color(p.text_muted));
                            } else {
                                ui.label(
                                    RichText::new(&deps.tree_output)
                                        .size(11.0)
                                        .monospace()
                                        .color(Color32::from_rgb(220, 225, 235)),
                                );
                            }
                        });
                });
        }

        if let Some(target_unit) = navigate_to_unit {
            self.systemd_deps_history.push(s.name.clone());
            self.select_systemd_unit(&target_unit, user_mode, host_opt);
        }
    }

    fn execute_pending_sudo_action(&mut self, action: PendingSudoAction, from_popup: bool) {
        let pass = if from_popup {
            self.sudo_popup.password_input.clone()
        } else {
            self.local_sudo_password.clone()
        };

        match action {
            PendingSudoAction::ServiceAction { unit, action: act, user_mode } => {
                let res = control_local_service(&unit, act, user_mode, Some(&pass));
                match res {
                    Ok(msg) => {
                        if from_popup && self.sudo_popup.remember_password {
                            self.local_sudo_password = pass;
                        }
                        self.sudo_popup.open = false;
                        self.sudo_popup.pending = None;
                        self.sudo_popup.password_input.clear();
                        self.sudo_popup.error = None;
                        self.service_action_msg = Some(msg);
                        self.trigger_local_services_list(user_mode);
                        let pass_opt = if !self.local_sudo_password.is_empty() {
                            Some(self.local_sudo_password.as_str())
                        } else {
                            None
                        };
                        self.trigger_local_journal_fetch(&unit, user_mode, pass_opt);
                    }
                    Err(e) => {
                        self.local_sudo_password.clear();
                        self.sudo_popup.open = true;
                        self.sudo_popup.pending = Some(PendingSudoAction::ServiceAction { unit, action: act, user_mode });
                        self.sudo_popup.error = Some(e);
                        self.sudo_popup.just_opened = true;
                    }
                }
            }
            PendingSudoAction::CreateService { def } => {
                let res = create_local_service(&def, Some(&pass));
                match res {
                    Ok(msg) => {
                        if from_popup && self.sudo_popup.remember_password {
                            self.local_sudo_password = pass;
                        }
                        self.sudo_popup.open = false;
                        self.sudo_popup.pending = None;
                        self.sudo_popup.password_input.clear();
                        self.sudo_popup.error = None;
                        self.service_action_msg = Some(msg);
                        self.trigger_local_services_list(def.is_user);
                    }
                    Err(e) => {
                        self.local_sudo_password.clear();
                        self.sudo_popup.open = true;
                        self.sudo_popup.pending = Some(PendingSudoAction::CreateService { def });
                        self.sudo_popup.error = Some(e);
                        self.sudo_popup.just_opened = true;
                    }
                }
            }
            PendingSudoAction::SaveUnitFile { path, content, user_mode } => {
                let res = write_local_unit_file(&path, &content, user_mode, Some(&pass));
                match res {
                    Ok(msg) => {
                        if from_popup && self.sudo_popup.remember_password {
                            self.local_sudo_password = pass;
                        }
                        self.sudo_popup.open = false;
                        self.sudo_popup.pending = None;
                        self.sudo_popup.password_input.clear();
                        self.sudo_popup.error = None;
                        self.systemd_file_original = content.clone();
                        self.systemd_file_status_msg = Some((msg, false));
                        if let Ok(mut c) = self.systemd_files_cache.lock() {
                            c.clear();
                        }
                    }
                    Err(e) => {
                        self.local_sudo_password.clear();
                        self.sudo_popup.open = true;
                        self.sudo_popup.pending = Some(PendingSudoAction::SaveUnitFile { path, content, user_mode });
                        self.sudo_popup.error = Some(e);
                        self.sudo_popup.just_opened = true;
                    }
                }
            }
            PendingSudoAction::DeleteDropIn { path, user_mode } => {
                let res = delete_local_drop_in(&path, user_mode, Some(&pass));
                match res {
                    Ok(msg) => {
                        if from_popup && self.sudo_popup.remember_password {
                            self.local_sudo_password = pass;
                        }
                        self.sudo_popup.open = false;
                        self.sudo_popup.pending = None;
                        self.sudo_popup.password_input.clear();
                        self.sudo_popup.error = None;
                        self.systemd_selected_file = None;
                        self.systemd_file_content.clear();
                        self.systemd_file_original.clear();
                        self.systemd_file_status_msg = Some((msg, false));
                        if let Ok(mut c) = self.systemd_files_cache.lock() {
                            c.clear();
                        }
                    }
                    Err(e) => {
                        self.local_sudo_password.clear();
                        self.sudo_popup.open = true;
                        self.sudo_popup.pending = Some(PendingSudoAction::DeleteDropIn { path, user_mode });
                        self.sudo_popup.error = Some(e);
                        self.sudo_popup.just_opened = true;
                    }
                }
            }
        }
    }

    fn render_sudo_popup(&mut self, ctx: &Context, palette: &crate::theme::ThemePalette) {
        if !self.sudo_popup.open {
            return;
        }

        let mut is_open = self.sudo_popup.open;
        let mut submit = false;
        let mut cancel = false;
        let screen = ctx.screen_rect();
        let window_id = egui::Id::new("sudo_auth_modal_dialog");
        let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();
        egui::Window::new("Administrator Authentication")
            .id(window_id)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .current_pos(screen.center() + drag_offset)
            .pivot(egui::Align2::CENTER_CENTER)
            .fixed_size([420.0, 240.0])
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(17, 19, 27))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                    .rounding(Rounding::same(16.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 8.0),
                        blur: 24.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(160),
                    })
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
            )
            .show(ctx, |ui| {
                crate::modals::render_modal_titlebar(ui, "Administrator Authentication", &mut is_open);
                ui.add_space(8.0);
                ui.add_space(4.0);

                if let Some(pending) = &self.sudo_popup.pending {
                    match pending {
                        PendingSudoAction::ServiceAction { unit, action, .. } => {
                            let act_color = match *action {
                                "start" => Color32::from_rgb(16, 185, 129),
                                "stop" => palette.danger,
                                _ => palette.accent,
                            };
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Action:").size(12.0).color(palette.text_secondary));
                                ui.label(RichText::new(action.to_uppercase()).size(12.0).strong().color(act_color));
                                ui.label(RichText::new("on system service").size(12.0).color(palette.text_secondary));
                                ui.label(RichText::new(unit).size(12.0).strong().color(palette.text_primary));
                            });
                        }
                        PendingSudoAction::CreateService { def } => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Create system service:").size(12.0).color(palette.text_secondary));
                                ui.label(RichText::new(&def.name).size(12.0).strong().color(palette.text_primary));
                            });
                        }
                        PendingSudoAction::SaveUnitFile { path, .. } => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Save system unit file:").size(12.0).color(palette.text_secondary));
                                ui.label(RichText::new(path).size(12.0).strong().color(palette.text_primary));
                            });
                        }
                        PendingSudoAction::DeleteDropIn { path, .. } => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Delete system drop-in:").size(12.0).color(palette.text_secondary));
                                ui.label(RichText::new(path).size(12.0).strong().color(palette.danger));
                            });
                        }
                    }
                }

                ui.add_space(4.0);
                ui.label(
                    RichText::new("System-level services require root privileges (sudo). Enter password to proceed:")
                        .size(11.0)
                        .color(palette.text_muted),
                );

                ui.add_space(6.0);

                if let Some(err) = &self.sudo_popup.error {
                    Frame::none()
                        .fill(palette.bg_subtle)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(err).size(11.0).color(palette.danger));
                        });
                    ui.add_space(6.0);
                }

                ui.horizontal(|ui| {
                    ui.label(RichText::new("Password:").size(12.0).strong().color(palette.text_primary));
                    let edit = egui::TextEdit::singleline(&mut self.sudo_popup.password_input)
                        .password(true)
                        .desired_width(260.0);
                    let resp = ui.add(edit);
                    if self.sudo_popup.just_opened {
                        resp.request_focus();
                        self.sudo_popup.just_opened = false;
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit = true;
                    }
                });

                ui.add_space(6.0);
                ui.checkbox(&mut self.sudo_popup.remember_password, "Remember for this session");

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add(
                        egui::Button::new(RichText::new("Cancel").size(11.5).color(palette.text_secondary))
                            .fill(palette.bg_button)
                            .rounding(Rounding::same(4.0)),
                    ).clicked() {
                        cancel = true;
                    }

                    let btn_auth = egui::Button::new(RichText::new(format!("{} Authenticate", crate::lucide::CHECK)).size(11.5).strong().color(Color32::WHITE))
                        .fill(palette.accent)
                        .rounding(Rounding::same(4.0));
                    if ui.add(btn_auth).clicked() {
                        submit = true;
                    }
                });
            });

        if cancel || !is_open {
            self.sudo_popup.open = false;
            self.sudo_popup.pending = None;
            self.sudo_popup.error = None;
            self.sudo_popup.password_input.clear();
        } else if submit {
            if let Some(pending) = self.sudo_popup.pending.clone() {
                self.execute_pending_sudo_action(pending, true);
            }
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // SETTINGS MODAL: Themes, Boot Autostart, Server Import/Export, Shortcuts
    // ═════════════════════════════════════════════════════════════════════════
    fn render_settings_modal(&mut self, ctx: &Context, palette: &crate::theme::ThemePalette) {
        if !self.settings_modal_open {
            return;
        }

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.settings_modal_open = false;
            return;
        }

        let mut is_open = self.settings_modal_open;
        let screen = ctx.screen_rect();
        let modal_w = 540.0_f32.min(screen.width() - 40.0).max(360.0);
        let max_h = (screen.height() - 80.0).max(400.0);

        let mut change_theme: Option<ThemeMode> = None;
        let mut toggle_boot = false;
        let mut do_import = false;
        let mut do_export = false;
        let mut close_requested = false;

        let window_id = egui::Id::new("settings_modal_dialog");
        let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();
        egui::Window::new("Settings & Preferences")
            .id(window_id)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .current_pos(screen.center() + drag_offset)
            .pivot(egui::Align2::CENTER_CENTER)
            .min_width(modal_w)
            .max_width(modal_w)
            .default_height(480.0)
            .max_height(max_h)
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(17, 19, 27))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                    .rounding(Rounding::same(16.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 8.0),
                        blur: 24.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(160),
                    })
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
            )
            .show(ctx, |ui| {
                crate::modals::render_modal_titlebar(ui, "Settings & Preferences", &mut is_open);
                ui.add_space(8.0);
            ui.add_space(4.0);

            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;

                    // ─────────────────────────────────────────────────────────────
                    // SECTION 1: 🎨 Themes & Visual Styling
                    // ─────────────────────────────────────────────────────────────
                    Frame::none()
                        .fill(palette.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(10.0))
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{} Theme & Visual Styling", crate::lucide::PALETTE))
                                        .size(13.0)
                                        .strong()
                                        .color(palette.text_primary),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let mut selected_theme = self.theme;
                                    egui::ComboBox::from_id_salt("settings_theme_dropdown")
                                        .selected_text(
                                            RichText::new(format!(
                                                "{} {}",
                                                self.theme.emoji(),
                                                self.theme.name()
                                            ))
                                            .size(11.5)
                                            .color(palette.text_primary),
                                        )
                                        .width(175.0)
                                        .show_ui(ui, |ui| {
                                            for &thm in ThemeMode::all() {
                                                let label = format!("{} {}", thm.emoji(), thm.name());
                                                if ui.selectable_value(
                                                    &mut selected_theme,
                                                    thm,
                                                    RichText::new(label).size(11.5),
                                                ).clicked() {
                                                    change_theme = Some(thm);
                                                }
                                            }
                                        });
                                });
                            });

                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(
                                    "Choose an aesthetic palette tailored for high-density developer operations.",
                                )
                                .size(10.5)
                                .color(palette.text_muted),
                            );

                            ui.add_space(8.0);

                            // Quick theme pills
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                for &thm in ThemeMode::all() {
                                    let is_active = self.theme == thm;
                                    let (bg, fg) = if is_active {
                                        (palette.accent, palette.accent_text)
                                    } else {
                                        (palette.bg_subtle, palette.text_secondary)
                                    };
                                    let btn = egui::Button::new(
                                        RichText::new(format!("{} {}", thm.emoji(), thm.name()))
                                            .size(10.5)
                                            .strong()
                                            .color(fg),
                                    )
                                    .fill(bg)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(999.0))
                                    .min_size(egui::vec2(0.0, 24.0));

                                    if ui.add(btn).clicked() {
                                        change_theme = Some(thm);
                                    }
                                }
                            });
                        });

                    // ─────────────────────────────────────────────────────────────
                    // SECTION 2: 🚀 System Startup & Boot
                    // ─────────────────────────────────────────────────────────────
                    Frame::none()
                        .fill(palette.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(10.0))
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            let is_boot = crate::autostart::is_app_autostart_enabled();

                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{} System Startup (Autostart on Boot)", crate::lucide::ROCKET))
                                        .size(13.0)
                                        .strong()
                                        .color(palette.text_primary),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let (btn_txt, btn_bg, btn_fg) = if is_boot {
                                        ("Turn OFF Autostart", palette.danger_subtle, palette.danger)
                                    } else {
                                        ("Turn ON Autostart", palette.success_subtle, palette.success)
                                    };
                                    let btn = egui::Button::new(
                                        RichText::new(btn_txt)
                                            .size(11.0)
                                            .strong()
                                            .color(btn_fg),
                                    )
                                    .fill(btn_bg)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(999.0))
                                    .min_size(egui::vec2(130.0, 26.0));

                                    if ui.add(btn).clicked() {
                                        toggle_boot = true;
                                    }

                                    let (badge_txt, badge_bg, badge_fg) = if is_boot {
                                        (format!("{} ENABLED", crate::lucide::CHECK), palette.success_subtle, palette.success)
                                    } else {
                                        (format!("{} DISABLED", crate::lucide::CIRCLE), palette.bg_subtle, palette.text_muted)
                                    };
                                    Frame::none()
                                        .fill(badge_bg)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(badge_txt).size(10.0).strong().color(badge_fg));
                                        });
                                });
                            });

                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(
                                    "When enabled, Server Launcher initializes silently on login or desktop startup.",
                                )
                                .size(10.5)
                                .color(palette.text_muted),
                            );
                        });

                    // ─────────────────────────────────────────────────────────────
                    // SECTION 3: 📦 Servers Configuration (Import / Export)
                    // ─────────────────────────────────────────────────────────────
                    Frame::none()
                        .fill(palette.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(10.0))
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("{} Servers Configuration (Import & Export)", crate::lucide::PACKAGE))
                                    .size(13.0)
                                    .strong()
                                    .color(palette.text_primary),
                            );
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(
                                    "Load configuration profiles from a servers.json file, or export current servers.",
                                )
                                .size(10.5)
                                .color(palette.text_muted),
                            );
                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;

                                let btn_import = egui::Button::new(
                                    RichText::new(format!("{} Import servers.json", crate::lucide::DOWNLOAD))
                                        .size(11.5)
                                        .strong()
                                        .color(palette.accent_text),
                                )
                                .fill(palette.accent)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(999.0))
                                .min_size(egui::vec2(150.0, 28.0));
                                if ui.add(btn_import).on_hover_text("Select and import servers.json file").clicked() {
                                    do_import = true;
                                }

                                let btn_export = egui::Button::new(
                                    RichText::new(format!("{} Export servers.json", crate::lucide::UPLOAD))
                                        .size(11.5)
                                        .strong()
                                        .color(palette.text_primary),
                                )
                                .fill(palette.bg_button)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(999.0))
                                .min_size(egui::vec2(150.0, 28.0));
                                if ui.add(btn_export).on_hover_text("Export current servers to servers.json file").clicked() {
                                    do_export = true;
                                }
                            });

                            if let Some((ref msg, is_err, _)) = self.import_status_msg {
                                ui.add_space(6.0);
                                let color = if is_err { palette.danger } else { Color32::from_rgb(16, 185, 129) };
                                Frame::none()
                                    .fill(palette.bg_subtle)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(msg).size(10.5).color(color));
                                    });
                            }
                        });

                    // ─────────────────────────────────────────────────────────────
                    // SECTION 4: ⌨ Keyboard Shortcuts Reference
                    // ─────────────────────────────────────────────────────────────
                    Frame::none()
                        .fill(palette.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(10.0))
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("{} Keyboard Shortcuts", crate::lucide::KEYBOARD))
                                    .size(13.0)
                                    .strong()
                                    .color(palette.text_primary),
                            );
                            ui.add_space(6.0);

                            let shortcuts = [
                                ("Ctrl+P / Ctrl+K", "Quick Launch & Command Palette"),
                                ("Ctrl+,", "Open Settings & Preferences"),
                                ("Ctrl+A", "Start All Servers"),
                                ("Ctrl+X", "Stop All Servers"),
                                ("Ctrl+F", "Focus Instance Search Bar"),
                                ("Esc", "Close Modals / Dismiss Dialogs"),
                            ];

                            for (keys, desc) in shortcuts {
                                ui.horizontal(|ui| {
                                    Frame::none()
                                        .fill(palette.bg_subtle)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(4.0))
                                        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(keys)
                                                    .size(10.5)
                                                    .monospace()
                                                    .strong()
                                                    .color(palette.accent),
                                            );
                                        });
                                    ui.label(RichText::new(desc).size(11.0).color(palette.text_secondary));
                                });
                                ui.add_space(1.0);
                            }
                        });

                    // ─────────────────────────────────────────────────────────────
                    // SECTION 5: ℹ️ System Diagnostics & Information
                    // ─────────────────────────────────────────────────────────────
                    Frame::none()
                        .fill(palette.bg_subtle)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(10.0))
                        .inner_margin(egui::Margin::same(10.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Host IP:").size(10.5).color(palette.text_muted));
                                ui.label(RichText::new(&self.lan_ip).size(10.5).monospace().color(palette.text_primary));
                                ui.add_space(8.0);
                                ui.label(RichText::new("Managed Servers:").size(10.5).color(palette.text_muted));
                                ui.label(RichText::new(format!("{}", self.services.len())).size(10.5).strong().color(palette.text_primary));
                            });
                            ui.add_space(2.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Config File:").size(10.5).color(palette.text_muted));
                                ui.label(RichText::new(self.config_path.display().to_string()).size(10.5).monospace().color(palette.text_secondary));
                            });
                        });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn_done = egui::Button::new(
                                RichText::new("Done")
                                    .size(11.5)
                                    .strong()
                                    .color(palette.accent_text),
                            )
                            .fill(palette.accent)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(80.0, 26.0));
                            if ui.add(btn_done).clicked() {
                                close_requested = true;
                            }
                        });
                    });
                });
        });

        if close_requested {
            is_open = false;
        }

        // Apply any actions after UI pass (avoids borrow conflict)
        if let Some(thm) = change_theme {
            if thm != self.theme {
                self.theme = thm;
                let pal = self.theme.palette();
                ctx.set_visuals(pal.egui_visuals());
                self.last_applied_palette = Some(pal);
                self.persist_config();
            }
        }
        if toggle_boot {
            let is_boot = crate::autostart::is_app_autostart_enabled();
            let new_state = !is_boot;
            let _ = crate::autostart::set_app_autostart(new_state);
            self.app_autostart = new_state;
            self.persist_config();
        }
        if do_import {
            self.import_servers_from_file_dialog();
        }
        if do_export {
            self.export_servers_to_file_dialog();
        }

        self.settings_modal_open = is_open;
    }

    // ═════════════════════════════════════════════════════════════════════════
    // SENSORS TAB: Left Sidebar (Quick Overview Cards & Category Filters)
    // ═════════════════════════════════════════════════════════════════════════
    fn render_sensors_left_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.theme.palette();
        ui.set_width(ui.available_width());
        ui.set_max_width(ui.available_width());

        let is_fetching = if let Some(hid) = host_id {
            self.remote_sensors_in_flight.lock().unwrap().contains(hid)
        } else {
            false
        };

        // Fetch or refresh sensors snapshot asynchronously without blocking UI thread
        let snap = if let Some(hid) = host_id {
            let has_snap = self.remote_sensors_snapshots.lock().unwrap().contains_key(hid);
            let last_fetch = self.remote_sensors_last_fetch.lock().unwrap().get(hid).copied();
            let interval_passed = self.sensors_refresh_interval > 0
                && last_fetch
                    .map(|t| t.elapsed() >= Duration::from_secs(self.sensors_refresh_interval as u64))
                    .unwrap_or(true);

            if (!has_snap || interval_passed) && !is_fetching {
                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                    self.trigger_remote_sensors_query(host);
                }
            }
            self.remote_sensors_snapshots.lock().unwrap().get(hid).cloned().unwrap_or_default()
        } else {
            if self.last_sensors_refresh.elapsed() >= Duration::from_secs(self.sensors_refresh_interval.max(1) as u64) {
                let include_top = self.sensors_subtab == SensorsSubTab::TopApps && self.sensors_top_apps_visited;
                self.sensors_snapshot = self.sensors_collector.collect_with_top_apps(include_top);
                self.last_sensors_refresh = Instant::now();
            }
            self.sensors_snapshot.clone()
        };

        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.set_max_width(ui.available_width());

            // Header Row: Title & Manual Refresh
            ui.horizontal(|ui| {
                ui.label(RichText::new("HARDWARE SENSORS").size(12.0).strong().color(p.text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_ref = egui::Button::new(RichText::new(crate::lucide::REFRESH_CW).size(11.0).color(p.text_secondary))
                        .fill(p.bg_button);
                    if ui.add(btn_ref).on_hover_text("Refresh all sensors now").clicked() {
                        if let Some(hid) = host_id {
                            if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                self.trigger_remote_sensors_query(host);
                            }
                        } else {
                            let include_top = self.sensors_subtab == SensorsSubTab::TopApps && self.sensors_top_apps_visited;
                            self.sensors_snapshot = self.sensors_collector.collect_with_top_apps(include_top);
                            self.last_sensors_refresh = Instant::now();
                        }
                    }

                    // Auto-refresh interval dropdown
                    let interval_label = match self.sensors_refresh_interval {
                        0 => format!("{} Off", crate::lucide::CLOCK),
                        s => format!("{} {}s", crate::lucide::CLOCK, s),
                    };
                    egui::ComboBox::from_id_salt("sensors_refresh_interval_combo")
                        .selected_text(RichText::new(interval_label).size(10.0).color(p.text_primary))
                        .width(72.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.sensors_refresh_interval, 0, "Off (Manual)");
                            ui.selectable_value(&mut self.sensors_refresh_interval, 1, "1 second");
                            ui.selectable_value(&mut self.sensors_refresh_interval, 2, "2 seconds");
                            ui.selectable_value(&mut self.sensors_refresh_interval, 3, "3 seconds");
                            ui.selectable_value(&mut self.sensors_refresh_interval, 5, "5 seconds");
                        });
                });
            });

            ui.add_space(6.0);

            // Search filter
            ui.add(
                egui::TextEdit::singleline(&mut self.sensors_search_query)
                    .hint_text(format!("{} Filter sensors & temps...", crate::lucide::SEARCH))
                    .desired_width(ui.available_width()),
            );

            ui.add_space(8.0);

            // Category Filter Pills
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                let categories = ["All", "Top Apps", "CPU", "RAM", "GPU", "Power", "Storage", "Network", "Temps"];
                for cat in categories {
                    let is_active = if cat == "Top Apps" {
                        self.sensors_subtab == SensorsSubTab::TopApps
                    } else {
                        self.sensors_subtab == SensorsSubTab::System && self.sensors_category_filter == cat
                    };
                    let bg = if is_active { p.accent } else { p.bg_button };
                    let fg = if is_active { p.accent_text } else { p.text_secondary };
                    let btn = egui::Button::new(RichText::new(cat).size(10.0).strong().color(fg))
                        .fill(bg)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(999.0));
                    if ui.add(btn).clicked() {
                        if cat == "Top Apps" {
                            self.sensors_subtab = SensorsSubTab::TopApps;
                            self.sensors_top_apps_visited = true;
                            if self.selected_machine == "local" && self.sensors_snapshot.top_apps.is_empty() {
                                self.sensors_snapshot = self.sensors_collector.collect_with_top_apps(true);
                                self.last_sensors_refresh = Instant::now();
                            }
                        } else {
                            self.sensors_subtab = SensorsSubTab::System;
                            self.sensors_category_filter = cat.to_string();
                        }
                    }
                }
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            // Quick Stats Summary Cards in Sidebar
            ScrollArea::vertical()
                .id_salt("sensors_left_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_max_width(ui.available_width());

                    // Card 1: CPU Quick Card
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{} CPU", crate::lucide::CPU)).size(12.0).strong().color(p.text_primary));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if let Some(t) = snap.cpu_temp {
                                        ui.label(RichText::new(format!("{:.1}°C", t)).size(11.0).strong().color(crate::sensors::temp_color(t)));
                                    }
                                });
                            });
                            ui.add_space(3.0);
                            let cpu_val = (snap.cpu_global_usage / 100.0).clamp(0.0, 1.0);
                            ui.add(egui::ProgressBar::new(cpu_val).text(format!("{:.1}% ({} cores)", snap.cpu_global_usage, snap.cpu_cores_logical)).animate(false));

                            ui.add_space(3.0);
                            ui.horizontal(|ui| {
                                if snap.cpu_frequency_mhz > 0 {
                                    let freq_s = if snap.cpu_frequency_mhz >= 1000 {
                                        format!("{:.2} GHz", snap.cpu_frequency_mhz as f64 / 1000.0)
                                    } else {
                                        format!("{} MHz", snap.cpu_frequency_mhz)
                                    };
                                    ui.label(RichText::new(format!("{} {}", crate::lucide::CLOCK, freq_s)).size(10.5).monospace().color(p.info));
                                }
                                if let Some(w) = snap.cpu_power_watts {
                                    if snap.cpu_frequency_mhz > 0 {
                                        ui.label(RichText::new("•").size(10.0).color(p.text_muted));
                                    }
                                    ui.label(RichText::new(format!("{} {:.1} W", crate::lucide::ZAP, w)).size(10.5).strong().color(Color32::from_rgb(250, 204, 21)));
                                } else if let Some(tdp) = snap.cpu_tdp_watts {
                                    if snap.cpu_frequency_mhz > 0 {
                                        ui.label(RichText::new("•").size(10.0).color(p.text_muted));
                                    }
                                    ui.label(RichText::new(format!("{} TDP: {:.0} W", crate::lucide::ZAP, tdp)).size(10.0).color(p.text_muted));
                                }
                            });
                        });

                    ui.add_space(6.0);

                    // Card 1b: Top Apps Quick Card
                    if !snap.top_apps.is_empty() {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("{} Top Apps", crate::lucide::ACTIVITY)).size(12.0).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let btn_all = egui::Button::new(RichText::new(format!("View all {}", crate::lucide::EXTERNAL_LINK)).size(10.0).color(p.accent))
                                            .frame(false);
                                        if ui.add(btn_all).clicked() {
                                            self.sensors_subtab = SensorsSubTab::TopApps;
                                            self.sensors_top_apps_visited = true;
                                            if self.selected_machine == "local" && self.sensors_snapshot.top_apps.is_empty() {
                                                self.sensors_snapshot = self.sensors_collector.collect_with_top_apps(true);
                                                self.last_sensors_refresh = Instant::now();
                                            }
                                        }
                                    });
                                });
                                ui.add_space(3.0);
                                for app in snap.top_apps.iter().take(3) {
                                    ui.horizontal(|ui| {
                                        let short_name = if app.name.len() > 14 {
                                            format!("{}…", &app.name[..12])
                                        } else {
                                            app.name.clone()
                                        };
                                        ui.label(RichText::new(short_name).size(10.5).color(p.text_secondary));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let col = if app.cpu_usage >= 50.0 { p.danger }
                                                else if app.cpu_usage >= 15.0 { Color32::from_rgb(250, 204, 21) }
                                                else { p.text_primary };
                                            ui.label(RichText::new(format!("{:.1}%", app.cpu_usage)).size(10.5).strong().monospace().color(col));
                                            if app.disk_rate_bps > 1024.0 {
                                                ui.label(RichText::new(crate::sensors::format_rate(app.disk_rate_bps)).size(9.5).monospace().color(Color32::from_rgb(56, 189, 248)));
                                            }
                                        });
                                    });
                                    ui.add_space(2.0);
                                }
                            });

                        ui.add_space(6.0);
                    }

                    // Card 2: RAM Quick Card
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{} Memory", crate::lucide::RAM)).size(12.0).strong().color(p.text_primary));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if let Some(t) = snap.ram_temp {
                                        ui.label(RichText::new(format!("{:.1}°C", t)).size(11.0).strong().color(crate::sensors::temp_color(t)));
                                    }
                                });
                            });
                            ui.add_space(3.0);
                            let mem_val = (snap.mem_usage_pct / 100.0).clamp(0.0, 1.0);
                            let used_str = crate::sensors::format_bytes(snap.mem_used_bytes);
                            let total_str = crate::sensors::format_bytes(snap.mem_total_bytes);
                            ui.add(egui::ProgressBar::new(mem_val).text(format!("{:.1}% ({} / {})", snap.mem_usage_pct, used_str, total_str)).animate(false));
                        });

                    ui.add_space(6.0);

                    // Card 3: GPU Quick Card (if detected)
                    if let Some(gpu) = snap.gpus.first() {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("{} GPU", crate::lucide::GPU)).size(12.0).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if let Some(t) = gpu.temp {
                                            ui.label(RichText::new(format!("{:.1}°C", t)).size(11.0).strong().color(crate::sensors::temp_color(t)));
                                        }
                                    });
                                });
                                ui.add_space(3.0);
                                if let Some(load) = gpu.load_pct {
                                    let load_val = (load / 100.0).clamp(0.0, 1.0);
                                    ui.add(egui::ProgressBar::new(load_val).text(format!("{:.1}% Load", load)).animate(false));
                                } else {
                                    ui.label(RichText::new(&gpu.name).size(10.5).color(p.text_secondary));
                                }
                                ui.add_space(3.0);
                                ui.horizontal_wrapped(|ui| {
                                    if let Some(clk) = gpu.clock_mhz {
                                        ui.label(RichText::new(format!("{} {} MHz", crate::lucide::CLOCK, clk)).size(10.5).monospace().color(p.info));
                                    }
                                    if let Some(mclk) = gpu.mem_clock_mhz {
                                        if gpu.clock_mhz.is_some() {
                                            ui.label(RichText::new("•").size(10.0).color(p.text_muted));
                                        }
                                        ui.label(RichText::new(format!("VRAM: {} MHz", mclk)).size(10.0).monospace().color(p.text_muted));
                                    }
                                    if let Some(w) = gpu.power_watts {
                                        if gpu.clock_mhz.is_some() || gpu.mem_clock_mhz.is_some() {
                                            ui.label(RichText::new("•").size(10.0).color(p.text_muted));
                                        }
                                        ui.label(RichText::new(format!("{} {:.1} W", crate::lucide::ZAP, w)).size(10.5).strong().color(Color32::from_rgb(250, 204, 21)));
                                    }
                                });
                            });

                        ui.add_space(6.0);
                    }

                    // Card 4: Storage Live Speed & Capacity Card
                    if !snap.disks.is_empty() {
                        let total_disk_bytes: u64 = snap.disks.iter().map(|d| d.total_bytes).sum();
                        let used_disk_bytes: u64 = snap.disks.iter().map(|d| d.used_bytes).sum();
                        let disk_usage_pct = if total_disk_bytes > 0 {
                            (used_disk_bytes as f32 / total_disk_bytes as f32) * 100.0
                        } else {
                            0.0
                        };

                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("{} Storage", crate::lucide::STORAGE)).size(12.0).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if let Some(t) = snap.storage_temp {
                                            ui.label(RichText::new(format!("{:.1}°C", t)).size(11.0).strong().color(crate::sensors::temp_color(t)));
                                        }
                                    });
                                });
                                ui.add_space(3.0);
                                let disk_bar_val = (disk_usage_pct / 100.0).clamp(0.0, 1.0);
                                let u_str = crate::sensors::format_bytes(used_disk_bytes);
                                let t_str = crate::sensors::format_bytes(total_disk_bytes);
                                ui.add(egui::ProgressBar::new(disk_bar_val).text(format!("{:.1}% ({} / {})", disk_usage_pct, u_str, t_str)).animate(false));

                                ui.add_space(3.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(crate::lucide::ARROW_DOWN).size(11.0).strong().color(Color32::from_rgb(56, 189, 248)));
                                    ui.label(RichText::new(crate::sensors::format_rate(snap.total_disk_read_rate_bps)).size(10.5).monospace().color(p.text_primary));
                                    ui.add_space(6.0);
                                    ui.label(RichText::new(crate::lucide::ARROW_UP).size(11.0).strong().color(Color32::from_rgb(168, 85, 247)));
                                    ui.label(RichText::new(crate::sensors::format_rate(snap.total_disk_write_rate_bps)).size(10.5).monospace().color(p.text_primary));
                                });
                            });

                        ui.add_space(6.0);
                    }

                    // Card 4: Power Supply / Battery Quick Card
                    if snap.power.ac_online.is_some() || !snap.power.batteries.is_empty() {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("{} Power", crate::lucide::BATTERY)).size(12.0).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let ac_text = if snap.power.ac_online == Some(true) { format!("{} AC Online", crate::lucide::PLUG) } else { format!("{} Battery", crate::lucide::BATTERY) };
                                        ui.label(RichText::new(ac_text).size(10.5).strong().color(p.text_secondary));
                                    });
                                });
                                if let Some(pct) = snap.power.total_battery_pct {
                                    ui.add_space(3.0);
                                    let val = (pct / 100.0).clamp(0.0, 1.0);
                                    let status_text = snap.power.batteries.first().map(|b| b.status.as_str()).unwrap_or("Battery");
                                    ui.add(egui::ProgressBar::new(val).text(format!("{:.0}% ({})", pct, status_text)).animate(false));
                                }
                                if let Some(w) = snap.power.total_power_watts {
                                    ui.add_space(2.0);
                                    ui.label(RichText::new(format!("{} Rate: {:.1} W", crate::lucide::ZAP, w)).size(10.0).color(p.info));
                                }
                            });

                        ui.add_space(6.0);
                    }

                    // Card 5: Network Live Bandwidth Card
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{} Network", crate::lucide::NETWORK)).size(12.0).strong().color(p.text_primary));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(format!("{} ifaces", snap.networks.len())).size(10.5).color(p.text_muted));
                                });
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::lucide::ARROW_DOWN).size(11.5).strong().color(Color32::from_rgb(56, 189, 248)));
                                ui.label(RichText::new(crate::sensors::format_rate(snap.total_rx_rate_bps)).size(11.0).monospace().color(p.text_primary));
                                ui.add_space(8.0);
                                ui.label(RichText::new(crate::lucide::ARROW_UP).size(11.5).strong().color(Color32::from_rgb(168, 85, 247)));
                                ui.label(RichText::new(crate::sensors::format_rate(snap.total_tx_rate_bps)).size(11.0).monospace().color(p.text_primary));
                            });
                        });

                    ui.add_space(6.0);

                    // Card 6: Sensors Count Card
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{} Temperature Probes", crate::lucide::THERMOMETER)).size(12.0).strong().color(p.text_primary));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(format!("{}", snap.temp_sensors.len())).size(12.0).strong().color(p.accent));
                                });
                            });
                            ui.add_space(3.0);
                            let hot_count = snap.temp_sensors.iter().filter(|s| s.temp >= 75.0).count();
                            let warm_count = snap.temp_sensors.iter().filter(|s| s.temp >= 55.0 && s.temp < 75.0).count();
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("Normal: {}", snap.temp_sensors.len().saturating_sub(hot_count + warm_count))).size(10.5).color(Color32::from_rgb(16, 185, 129)));
                                if warm_count > 0 {
                                    ui.label(RichText::new(format!("Warm: {}", warm_count)).size(10.5).color(Color32::from_rgb(245, 158, 11)));
                                }
                                if hot_count > 0 {
                                    ui.label(RichText::new(format!("Hot: {}", hot_count)).size(10.5).strong().color(Color32::from_rgb(239, 68, 68)));
                                }
                            });
                        });
                });
        });
    }

    // ═════════════════════════════════════════════════════════════════════════
    // SENSORS TAB: Right Main Area (Full Sensors & Hardware Monitor Viewer)
    // ═════════════════════════════════════════════════════════════════════════
    fn render_sensors_right_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.theme.palette();

        let is_fetching = if let Some(hid) = host_id {
            self.remote_sensors_in_flight.lock().unwrap().contains(hid)
        } else {
            false
        };

        // Auto-refresh timer logic (asynchronous for SSH hosts, never blocking UI thread)
        let snap = if let Some(hid) = host_id {
            let has_snap = self.remote_sensors_snapshots.lock().unwrap().contains_key(hid);
            let last_fetch = self.remote_sensors_last_fetch.lock().unwrap().get(hid).copied();
            let interval_passed = self.sensors_refresh_interval > 0
                && last_fetch
                    .map(|t| t.elapsed() >= Duration::from_secs(self.sensors_refresh_interval as u64))
                    .unwrap_or(true);

            if (!has_snap || interval_passed) && !is_fetching {
                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                    self.trigger_remote_sensors_query(host);
                }
            }
            self.remote_sensors_snapshots.lock().unwrap().get(hid).cloned().unwrap_or_default()
        } else {
            if self.sensors_refresh_interval > 0 && self.last_sensors_refresh.elapsed() >= Duration::from_secs(self.sensors_refresh_interval as u64) {
                let include_top = self.sensors_subtab == SensorsSubTab::TopApps && self.sensors_top_apps_visited;
                self.sensors_snapshot = self.sensors_collector.collect_with_top_apps(include_top);
                self.last_sensors_refresh = Instant::now();
            }
            self.sensors_snapshot.clone()
        };

        let filter_cat = self.sensors_category_filter.clone();
        let search_q = self.sensors_search_query.trim().to_lowercase();

        let show_cpu = filter_cat == "All" || filter_cat == "CPU";
        let show_ram = filter_cat == "All" || filter_cat == "RAM";
        let show_gpu = filter_cat == "All" || filter_cat == "GPU";
        let show_power = filter_cat == "All" || filter_cat == "Power";
        let show_storage = filter_cat == "All" || filter_cat == "Storage";
        let show_net = filter_cat == "All" || filter_cat == "Network";
        let show_temps = filter_cat == "All" || filter_cat == "Temps";

        ui.vertical(|ui| {
            // ═════════════════════════════════════════════════════════════════
            // Header Card: System Overview Bar
            // ═════════════════════════════════════════════════════════════════
            Frame::none()
                .fill(p.bg_card)
                .stroke(Stroke::NONE)
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    let refresh_w = 125.0;
                    let left_w = (ui.available_width() - refresh_w - 14.0).max(100.0);
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(egui::vec2(left_w, 0.0), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(crate::lucide::ACTIVITY).size(18.0).color(p.accent));
                                let title = if let Some(hid) = host_id {
                                    format!("Hardware Sensors Monitor — Remote Host ({})", hid)
                                } else {
                                    "Hardware Sensors & System Resource Monitor".to_string()
                                };
                                ui.label(RichText::new(title).size(14.5).strong().color(p.text_primary));
                                if self.sensors_refresh_interval > 0 {
                                    ui.label(RichText::new(format!("● Live ({}s refresh)", self.sensors_refresh_interval)).size(11.0).color(Color32::from_rgb(16, 185, 129)));
                                } else {
                                    ui.label(RichText::new("○ Manual refresh").size(11.0).color(p.text_muted));
                                }
                            });
                            ui.add_space(2.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(&snap.cpu_brand).size(11.0).strong().color(p.text_secondary));
                                ui.label(RichText::new("•").size(11.0).color(p.text_muted));
                                ui.label(RichText::new(format!("{} Cores", snap.cpu_cores_logical)).size(11.0).color(p.text_muted));
                                if snap.cpu_frequency_mhz > 0 {
                                    ui.label(RichText::new("•").size(11.0).color(p.text_muted));
                                    let freq_s = if snap.cpu_frequency_mhz >= 1000 {
                                        format!("{:.2} GHz", snap.cpu_frequency_mhz as f64 / 1000.0)
                                    } else {
                                        format!("{} MHz", snap.cpu_frequency_mhz)
                                    };
                                    ui.label(RichText::new(format!("{} {}", crate::lucide::CLOCK, freq_s)).size(11.0).monospace().color(p.info));
                                }
                                if let Some(w) = snap.cpu_power_watts {
                                    ui.label(RichText::new("•").size(11.0).color(p.text_muted));
                                    ui.label(RichText::new(format!("{} {:.1} W", crate::lucide::ZAP, w)).size(11.0).strong().color(Color32::from_rgb(250, 204, 21)));
                                }
                                if let Some(gpu) = snap.gpus.first() {
                                    ui.label(RichText::new("•").size(11.0).color(p.text_muted));
                                    let mut g_parts = Vec::new();
                                    if let Some(clk) = gpu.clock_mhz {
                                        g_parts.push(format!("{} {} MHz", crate::lucide::CLOCK, clk));
                                    }
                                    if let Some(w) = gpu.power_watts {
                                        g_parts.push(format!("{} {:.1} W", crate::lucide::ZAP, w));
                                    }
                                    let g_desc = if !g_parts.is_empty() {
                                        format!("{} GPU: {}", crate::lucide::GPU, g_parts.join(" • "))
                                    } else {
                                        format!("{} GPU: {}", crate::lucide::GPU, gpu.name)
                                    };
                                    ui.label(RichText::new(g_desc).size(11.0).color(p.info));
                                }
                                ui.label(RichText::new("•").size(11.0).color(p.text_muted));
                                ui.label(RichText::new(format!("{} Total RAM", crate::sensors::format_bytes(snap.mem_total_bytes))).size(11.0).color(p.success));
                                if snap.total_disk_read_rate_bps > 0.0 || snap.total_disk_write_rate_bps > 0.0 {
                                    ui.label(RichText::new("•").size(11.0).color(p.text_muted));
                                    ui.label(RichText::new(format!("{} Live Storage Speed: {} {}  {} {}", crate::lucide::STORAGE, crate::lucide::ARROW_DOWN, crate::sensors::format_rate(snap.total_disk_read_rate_bps), crate::lucide::ARROW_UP, crate::sensors::format_rate(snap.total_disk_write_rate_bps))).size(11.0).monospace().color(Color32::from_rgb(56, 189, 248)));
                                }
                            });
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn_ref = egui::Button::new(RichText::new(format!("{} Refresh Now", crate::lucide::REFRESH_CW)).size(11.5).strong().color(Color32::WHITE))
                                .fill(p.accent)
                                .rounding(Rounding::same(999.0));
                            if ui.add(btn_ref).clicked() {
                                if let Some(hid) = host_id {
                                    if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                        self.trigger_remote_sensors_query(host);
                                    }
                                } else {
                                    let include_top = self.sensors_subtab == SensorsSubTab::TopApps && self.sensors_top_apps_visited;
                                    self.sensors_snapshot = self.sensors_collector.collect_with_top_apps(include_top);
                                    self.last_sensors_refresh = Instant::now();
                                }
                            }
                        });
                    });
                });

            // Remote host error notification (if query failed)
            if let Some(hid) = host_id {
                if let Some(err_msg) = self.remote_sensors_errors.lock().unwrap().get(hid) {
                    ui.add_space(4.0);
                    Frame::none()
                        .fill(Color32::from_rgba_premultiplied(239, 68, 68, 25))
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{} Remote sensor query notice:", crate::lucide::ALERT_TRIANGLE)).size(11.5).strong().color(Color32::from_rgb(239, 68, 68)));
                                ui.label(RichText::new(err_msg).size(11.0).color(p.text_secondary));
                            });
                        });
                }
            }

            ui.add_space(6.0);

            // ═════════════════════════════════════════════════════════════════
            // Sensors Subtabs: [ System ]  [ Top Apps ]
            // ═════════════════════════════════════════════════════════════════
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let tabs = [
                    (SensorsSubTab::System, format!("{} System", crate::lucide::CPU)),
                    (SensorsSubTab::TopApps, format!("{} Top Apps", crate::lucide::ACTIVITY)),
                ];
                for (tab, label) in tabs {
                    let is_sel = self.sensors_subtab == tab;
                    let btn = egui::Button::new(
                        RichText::new(label)
                            .size(11.5)
                            .strong()
                            .color(if is_sel { p.accent_text } else { p.text_secondary }),
                    )
                    .fill(if is_sel { p.accent } else { p.bg_button })
                    .stroke(if is_sel { Stroke::NONE } else { Stroke::new(1.0_f32, p.border.gamma_multiply(0.5)) })
                    .rounding(Rounding::same(999.0))
                    .min_size(egui::vec2(90.0, 26.0));
                    if ui.add(btn).clicked() {
                        self.sensors_subtab = tab;
                        if tab == SensorsSubTab::TopApps {
                            self.sensors_top_apps_visited = true;
                            if self.selected_machine == "local" && self.sensors_snapshot.top_apps.is_empty() {
                                self.sensors_snapshot = self.sensors_collector.collect_with_top_apps(true);
                                self.last_sensors_refresh = Instant::now();
                            }
                        }
                    }
                }
            });

            ui.add_space(6.0);

            match self.sensors_subtab {
                SensorsSubTab::System => {
                    // ═════════════════════════════════════════════════════════════════
                    // Main Scrollable Area for System Hardware Sensors
                    // ═════════════════════════════════════════════════════════════════
                    ScrollArea::vertical()
                        .id_salt("sensors_main_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());

                    // SECTION 1: CPU MONITOR & PER-CORE BREAKDOWN
                    if show_cpu {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("{} Central Processing Unit (CPU)", crate::lucide::CPU)).size(13.5).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if let Some(t) = snap.cpu_temp {
                                            ui.label(RichText::new(format!("Package: {:.1}°C", t)).size(12.0).strong().color(crate::sensors::temp_color(t)));
                                        }
                                        if let Some(w) = snap.cpu_power_watts {
                                            ui.label(RichText::new(format!("{} {:.1} W", crate::lucide::ZAP, w)).size(12.0).strong().color(p.info));
                                        }
                                    });
                                });

                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    let global_pct = (snap.cpu_global_usage / 100.0).clamp(0.0, 1.0);
                                    let bar_w = (ui.available_width() - 200.0).max(120.0);
                                    ui.add(egui::ProgressBar::new(global_pct).desired_width(bar_w).text(format!("Global Usage: {:.1}%", snap.cpu_global_usage)).animate(false));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if snap.cpu_frequency_mhz > 0 {
                                            let freq_text = if snap.cpu_frequency_mhz >= 1000 {
                                                format!("{:.2} GHz avg", snap.cpu_frequency_mhz as f64 / 1000.0)
                                            } else {
                                                format!("{} MHz avg", snap.cpu_frequency_mhz)
                                            };
                                            ui.label(RichText::new(freq_text).size(11.0).monospace().color(p.text_secondary));
                                        }
                                        if let Some(tdp) = snap.cpu_tdp_watts {
                                            ui.label(RichText::new(format!("TDP Limit: {:.0} W", tdp)).size(10.5).color(p.text_muted));
                                        }
                                    });
                                });

                                // Per-core usages and live frequencies grid
                                if !snap.cpu_core_usages.is_empty() {
                                    ui.add_space(8.0);
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Per-Core Utilization & Speed:").size(11.0).color(p.text_muted));
                                        if !snap.cpu_frequencies_mhz.is_empty() {
                                            ui.label(RichText::new("• Dynamic Core Clocks active").size(10.5).color(Color32::from_rgb(16, 185, 129)));
                                        }
                                    });
                                    ui.add_space(4.0);

                                    let avail = ui.available_width();
                                    let cols = ((avail / 140.0).floor() as usize).clamp(1, 4).min(snap.cpu_core_usages.len().max(1));
                                    let item_w = (avail / cols as f32) - 12.0;
                                    let bar_w = (item_w - 75.0).clamp(45.0, 85.0);
                                    egui::Grid::new("cpu_cores_grid")
                                        .num_columns(cols)
                                        .spacing(egui::vec2(10.0, 6.0))
                                        .show(ui, |ui| {
                                            for (i, &usage) in snap.cpu_core_usages.iter().enumerate() {
                                                let core_val = (usage / 100.0).clamp(0.0, 1.0);
                                                let core_freq = snap.cpu_frequencies_mhz.get(i).copied().unwrap_or(0);
                                                ui.horizontal(|ui| {
                                                    ui.label(RichText::new(format!("C{}:", i)).size(10.5).color(p.text_muted));
                                                    ui.add(egui::ProgressBar::new(core_val).desired_width(bar_w).text(format!("{:.0}%", usage)).animate(false));
                                                    if core_freq > 0 {
                                                        let f_str = if core_freq >= 1000 {
                                                            format!("{:.2}G", core_freq as f64 / 1000.0)
                                                        } else {
                                                            format!("{}M", core_freq)
                                                        };
                                                        ui.label(RichText::new(f_str).size(10.0).monospace().color(p.text_secondary));
                                                    }
                                                });
                                                if (i + 1) % cols == 0 {
                                                    ui.end_row();
                                                }
                                            }
                                        });
                                }
                            });

                        ui.add_space(8.0);
                    }

                    // SECTION 2: RAM & SWAP MEMORY
                    if show_ram {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("{} System Memory (RAM) & Swap", crate::lucide::RAM)).size(13.5).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if let Some(t) = snap.ram_temp {
                                            ui.label(RichText::new(format!("SODIMM: {:.1}°C", t)).size(12.0).strong().color(crate::sensors::temp_color(t)));
                                        }
                                    });
                                });

                                ui.add_space(6.0);
                                let mem_pct = (snap.mem_usage_pct / 100.0).clamp(0.0, 1.0);
                                let used_str = crate::sensors::format_bytes(snap.mem_used_bytes);
                                let free_str = crate::sensors::format_bytes(snap.mem_free_bytes);
                                let total_str = crate::sensors::format_bytes(snap.mem_total_bytes);
                                ui.add(egui::ProgressBar::new(mem_pct).text(format!("RAM: {:.1}% (Used: {} / Total: {}, Free: {})", snap.mem_usage_pct, used_str, total_str, free_str)).animate(false));

                                if snap.swap_total_bytes > 0 {
                                    ui.add_space(6.0);
                                    let swap_pct = (snap.swap_usage_pct / 100.0).clamp(0.0, 1.0);
                                    let swap_used = crate::sensors::format_bytes(snap.swap_used_bytes);
                                    let swap_total = crate::sensors::format_bytes(snap.swap_total_bytes);
                                    ui.add(egui::ProgressBar::new(swap_pct).text(format!("Swap: {:.1}% ({} / {})", snap.swap_usage_pct, swap_used, swap_total)).animate(false));
                                }
                            });

                        ui.add_space(8.0);
                    }


                    // SECTION 3: GPU MONITOR
                    if show_gpu && !snap.gpus.is_empty() {
                        for (idx, gpu) in snap.gpus.iter().enumerate() {
                            Frame::none()
                                .fill(p.bg_card)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(8.0))
                                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(format!("{} GPU #{}: {}", crate::lucide::GPU, idx, gpu.name)).size(13.5).strong().color(p.text_primary));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if let Some(t) = gpu.temp {
                                                ui.label(RichText::new(format!("Temp: {:.1}°C", t)).size(12.0).strong().color(crate::sensors::temp_color(t)));
                                            }
                                            if let Some(w) = gpu.power_watts {
                                                let p_str = if let Some(lim) = gpu.power_limit_watts {
                                                    format!("{} Power: {:.1} W / {:.0} W limit", crate::lucide::ZAP, w, lim)
                                                } else {
                                                    format!("{} Power: {:.1} W", crate::lucide::ZAP, w)
                                                };
                                                ui.label(RichText::new(p_str).size(11.5).strong().color(p.info));
                                            }
                                        });
                                    });

                                    ui.add_space(4.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(RichText::new(format!("Driver: {}", gpu.driver)).size(10.5).color(p.text_muted));
                                        if let Some(clk) = gpu.clock_mhz {
                                            ui.label(RichText::new("•").size(10.5).color(p.text_muted));
                                            ui.label(RichText::new(format!("{} Core Clock: {} MHz", crate::lucide::CLOCK, clk)).size(11.0).strong().monospace().color(p.info));
                                        }
                                        if let Some(mclk) = gpu.mem_clock_mhz {
                                            ui.label(RichText::new("•").size(10.5).color(p.text_muted));
                                            ui.label(RichText::new(format!("{} VRAM Clock: {} MHz", crate::lucide::CLOCK, mclk)).size(11.0).monospace().color(p.accent));
                                        }
                                        if let Some(w) = gpu.power_watts {
                                            ui.label(RichText::new("•").size(10.5).color(p.text_muted));
                                            ui.label(RichText::new(format!("{} Draw: {:.1} W", crate::lucide::ZAP, w)).size(11.0).strong().color(Color32::from_rgb(250, 204, 21)));
                                        }
                                        if let (Some(used), Some(total)) = (gpu.mem_used_mb, gpu.mem_total_mb) {
                                            ui.label(RichText::new("•").size(10.5).color(p.text_muted));
                                            ui.label(RichText::new(format!("VRAM: {} MB / {} MB", used, total)).size(10.5).color(p.text_secondary));
                                        }
                                    });

                                    if let Some(load) = gpu.load_pct {
                                        ui.add_space(6.0);
                                        let load_pct = (load / 100.0).clamp(0.0, 1.0);
                                        ui.add(egui::ProgressBar::new(load_pct).text(format!("GPU Core Load: {:.1}%", load)).animate(false));
                                    }
                                });

                            ui.add_space(8.0);
                        }
                    }

                    // SECTION 4: POWER SUPPLY & BATTERY HEALTH
                    if show_power && (snap.power.ac_online.is_some() || !snap.power.batteries.is_empty()) {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("{} Power Supply & Battery Health", crate::lucide::BATTERY)).size(13.5).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if let Some(ac) = snap.power.ac_online {
                                            let (txt, col) = if ac {
                                                (format!("{} AC Mains: Online", crate::lucide::PLUG), Color32::from_rgb(16, 185, 129))
                                            } else {
                                                (format!("{} Battery Mode", crate::lucide::BATTERY), Color32::from_rgb(245, 158, 11))
                                            };
                                            ui.label(RichText::new(txt).size(11.5).strong().color(col));
                                        }
                                        if let Some(w) = snap.power.total_power_watts {
                                            ui.label(RichText::new(format!("{} {:.1} W", crate::lucide::ZAP, w)).size(11.5).monospace().color(p.info));
                                        }
                                    });
                                });

                                if snap.power.batteries.is_empty() {
                                    ui.add_space(6.0);
                                    ui.label(RichText::new("Host is connected to AC mains power (Desktop / Rack Server — No battery attached).").size(11.0).color(p.text_muted));
                                } else {
                                    for (b_idx, bat) in snap.power.batteries.iter().enumerate() {
                                        ui.add_space(8.0);
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(format!("Battery #{}: {}", b_idx + 1, bat.name)).size(12.0).strong().color(p.text_primary));
                                            ui.label(RichText::new(format!("[{}]", bat.status)).size(11.0).strong().color(
                                                match bat.status.to_lowercase().as_str() {
                                                    "charging" => Color32::from_rgb(16, 185, 129),
                                                    "full" => Color32::from_rgb(56, 189, 248),
                                                    "discharging" => Color32::from_rgb(245, 158, 11),
                                                    _ => p.text_secondary,
                                                }
                                            ));

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if let Some(h) = bat.health_pct {
                                                    ui.label(RichText::new(format!("Health: {:.1}%", h)).size(11.0).strong().color(
                                                        if h >= 80.0 { Color32::from_rgb(16, 185, 129) }
                                                        else if h >= 60.0 { Color32::from_rgb(245, 158, 11) }
                                                        else { Color32::from_rgb(239, 68, 68) }
                                                    ));
                                                }
                                                if let Some(v) = bat.voltage_volts {
                                                    ui.label(RichText::new(format!("Voltage: {:.2} V", v)).size(11.0).color(p.text_muted));
                                                }
                                            });
                                        });

                                        ui.add_space(4.0);
                                        let pct_val = (bat.capacity_pct / 100.0).clamp(0.0, 1.0);
                                        let bar_text = if let (Some(cur_wh), Some(full_wh)) = (bat.energy_wh, bat.energy_full_wh) {
                                            format!("{:.0}% ({:.1} Wh / {:.1} Wh)", bat.capacity_pct, cur_wh, full_wh)
                                        } else {
                                            format!("{:.1}% Charge Level", bat.capacity_pct)
                                        };
                                        ui.add(egui::ProgressBar::new(pct_val).text(bar_text).animate(bat.status.to_lowercase() == "charging"));

                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            if !bat.technology.is_empty() {
                                                ui.label(RichText::new(format!("Tech: {}", bat.technology)).size(10.5).color(p.text_muted));
                                            }
                                            if !bat.model_name.is_empty() {
                                                ui.label(RichText::new("•").size(10.5).color(p.text_muted));
                                                ui.label(RichText::new(format!("Model: {}", bat.model_name)).size(10.5).color(p.text_muted));
                                            }
                                            if !bat.manufacturer.is_empty() {
                                                ui.label(RichText::new("•").size(10.5).color(p.text_muted));
                                                ui.label(RichText::new(format!("Vendor: {}", bat.manufacturer)).size(10.5).color(p.text_muted));
                                            }
                                            if let Some(c) = bat.cycle_count {
                                                ui.label(RichText::new("•").size(10.5).color(p.text_muted));
                                                ui.label(RichText::new(format!("Cycles: {}", c)).size(10.5).color(p.text_muted));
                                            }
                                            if let Some(d_wh) = bat.energy_design_wh {
                                                ui.label(RichText::new("•").size(10.5).color(p.text_muted));
                                                ui.label(RichText::new(format!("Design: {:.1} Wh", d_wh)).size(10.5).color(p.text_muted));
                                            }
                                        });
                                    }
                                }
                            });

                        ui.add_space(8.0);
                    }

                    // SECTION 4: STORAGE / DISKS
                    if show_storage && !snap.disks.is_empty() {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("{} Storage & Mounted Disks", crate::lucide::STORAGE)).size(13.5).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if let Some(t) = snap.storage_temp {
                                            ui.label(RichText::new(format!("NVMe/Drive: {:.1}°C", t)).size(12.0).strong().color(crate::sensors::temp_color(t)));
                                        }
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("Total I/O:").size(11.0).color(p.text_muted));
                                            ui.label(RichText::new(crate::lucide::ARROW_DOWN).size(11.0).color(Color32::from_rgb(56, 189, 248)));
                                            ui.label(RichText::new(crate::sensors::format_rate(snap.total_disk_read_rate_bps)).size(11.0).monospace().color(p.text_primary));
                                            ui.add_space(4.0);
                                            ui.label(RichText::new(crate::lucide::ARROW_UP).size(11.0).color(Color32::from_rgb(168, 85, 247)));
                                            ui.label(RichText::new(crate::sensors::format_rate(snap.total_disk_write_rate_bps)).size(11.0).monospace().color(p.text_primary));
                                        });
                                    });
                                });

                                ui.add_space(6.0);
                                for d in &snap.disks {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&d.mount_point).size(12.0).strong().color(p.text_primary));
                                        ui.label(RichText::new(format!("({})", d.name)).size(10.5).color(p.text_muted));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let used_s = crate::sensors::format_bytes(d.used_bytes);
                                            let total_s = crate::sensors::format_bytes(d.total_bytes);
                                            ui.label(RichText::new(format!("{} / {} ({:.1}%)", used_s, total_s, d.usage_pct)).size(11.0).monospace().color(p.text_secondary));
                                            if d.read_rate_bps > 0.0 || d.write_rate_bps > 0.0 {
                                                ui.add_space(6.0);
                                                ui.label(RichText::new(format!("{} {}  {} {}", crate::lucide::ARROW_DOWN, crate::sensors::format_rate(d.read_rate_bps), crate::lucide::ARROW_UP, crate::sensors::format_rate(d.write_rate_bps))).size(10.5).monospace().color(Color32::from_rgb(56, 189, 248)));
                                            }
                                        });
                                    });

                                    let disk_pct = (d.usage_pct / 100.0).clamp(0.0, 1.0);
                                    ui.add(egui::ProgressBar::new(disk_pct).text(format!("{:.1}% used", d.usage_pct)).animate(false));

                                    ui.add_space(2.0);
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Live Speed:").size(10.5).color(p.text_muted));
                                        ui.label(RichText::new(crate::lucide::ARROW_DOWN).size(10.5).color(Color32::from_rgb(56, 189, 248)));
                                        ui.label(RichText::new(format!("Read: {}", crate::sensors::format_rate(d.read_rate_bps))).size(10.5).monospace().color(Color32::from_rgb(56, 189, 248)));
                                        ui.add_space(6.0);
                                        ui.label(RichText::new(crate::lucide::ARROW_UP).size(10.5).color(Color32::from_rgb(168, 85, 247)));
                                        ui.label(RichText::new(format!("Write: {}", crate::sensors::format_rate(d.write_rate_bps))).size(10.5).monospace().color(Color32::from_rgb(168, 85, 247)));
                                        if d.total_read_bytes > 0 || d.total_write_bytes > 0 {
                                            ui.label(RichText::new("•").size(10.5).color(p.text_muted));
                                            ui.label(RichText::new(format!("Lifetime: {} R / {} W", crate::sensors::format_bytes(d.total_read_bytes), crate::sensors::format_bytes(d.total_write_bytes))).size(10.0).color(p.text_muted));
                                        }
                                    });
                                    ui.add_space(6.0);
                                }
                            });

                        ui.add_space(8.0);
                    }

                    // SECTION 5: NETWORK TRAFFIC & INTERFACES
                    if show_net && !snap.networks.is_empty() {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("{} Network Interfaces & Real-Time Bandwidth", crate::lucide::NETWORK)).size(13.5).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(format!("Total {}", crate::lucide::ARROW_DOWN)).size(11.0).color(Color32::from_rgb(56, 189, 248)));
                                            ui.label(RichText::new(crate::sensors::format_rate(snap.total_rx_rate_bps)).size(11.0).monospace().color(p.text_primary));
                                            ui.add_space(6.0);
                                            ui.label(RichText::new(crate::lucide::ARROW_UP).size(11.0).color(Color32::from_rgb(168, 85, 247)));
                                            ui.label(RichText::new(crate::sensors::format_rate(snap.total_tx_rate_bps)).size(11.0).monospace().color(p.text_primary));
                                        });
                                    });
                                });

                                ui.add_space(6.0);
                                egui::Grid::new("network_interfaces_grid")
                                    .num_columns(5)
                                    .striped(true)
                                    .spacing(egui::vec2(10.0, 5.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new("Interface").size(11.0).strong().color(p.text_muted));
                                        ui.label(RichText::new("Download Rate").size(11.0).strong().color(Color32::from_rgb(56, 189, 248)));
                                        ui.label(RichText::new("Upload Rate").size(11.0).strong().color(Color32::from_rgb(168, 85, 247)));
                                        ui.label(RichText::new("Total Recv").size(11.0).strong().color(p.text_muted));
                                        ui.label(RichText::new("Total Sent").size(11.0).strong().color(p.text_muted));
                                        ui.end_row();

                                        for net in &snap.networks {
                                            ui.label(RichText::new(&net.name).size(11.5).strong().color(p.text_primary));
                                            ui.label(RichText::new(crate::sensors::format_rate(net.rx_rate_bps)).size(11.0).monospace().color(Color32::from_rgb(56, 189, 248)));
                                            ui.label(RichText::new(crate::sensors::format_rate(net.tx_rate_bps)).size(11.0).monospace().color(Color32::from_rgb(168, 85, 247)));
                                            ui.label(RichText::new(crate::sensors::format_bytes(net.rx_bytes)).size(10.5).monospace().color(p.text_secondary));
                                            ui.label(RichText::new(crate::sensors::format_bytes(net.tx_bytes)).size(10.5).monospace().color(p.text_secondary));
                                            ui.end_row();
                                        }
                                    });
                            });

                        ui.add_space(8.0);
                    }

                    // SECTION 6: COMPLETE HARDWARE TEMPERATURE SENSORS TABLE
                    if show_temps {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("{} All Hardware Temperature Probes ({})", crate::lucide::THERMOMETER, snap.temp_sensors.len())).size(13.5).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new("● <55°C | ● 55-75°C | ● >75°C").size(10.5).color(p.text_muted));
                                    });
                                });

                                ui.add_space(6.0);

                                let filtered_sensors: Vec<&crate::sensors::TempSensor> = snap.temp_sensors.iter().filter(|s| {
                                    if search_q.is_empty() {
                                        true
                                    } else {
                                        s.name.to_lowercase().contains(&search_q)
                                            || s.chip.to_lowercase().contains(&search_q)
                                            || s.category.label().to_lowercase().contains(&search_q)
                                    }
                                }).collect();

                                if filtered_sensors.is_empty() {
                                    ui.add_space(10.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(RichText::new("No temperature sensors matching the filter.").size(11.5).color(p.text_muted));
                                    });
                                } else {
                                    egui::Grid::new("all_temp_sensors_grid")
                                        .num_columns(5)
                                        .striped(true)
                                        .spacing(egui::vec2(10.0, 5.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new("Component / Label").size(11.0).strong().color(p.text_muted));
                                            ui.label(RichText::new("Category").size(11.0).strong().color(p.text_muted));
                                            ui.label(RichText::new("Driver").size(11.0).strong().color(p.text_muted));
                                            ui.label(RichText::new("Temp").size(11.0).strong().color(p.text_muted));
                                            ui.label(RichText::new("Max / Crit").size(11.0).strong().color(p.text_muted));
                                            ui.end_row();

                                            for s in filtered_sensors {
                                                // Component & emoji
                                                ui.horizontal(|ui| {
                                                    ui.label(RichText::new(s.category.emoji()).size(13.0));
                                                    ui.label(RichText::new(&s.name).size(12.0).strong().color(p.text_primary));
                                                });

                                                // Category Pill
                                                ui.label(RichText::new(s.category.label()).size(10.5).color(p.text_secondary));

                                                // Chip Driver
                                                ui.label(RichText::new(&s.chip).size(10.5).monospace().color(p.text_muted));

                                                // Current Temperature with color badge
                                                let t_color = crate::sensors::temp_color(s.temp);
                                                ui.horizontal(|ui| {
                                                    ui.label(RichText::new("●").size(12.0).color(t_color));
                                                    ui.label(RichText::new(format!("{:.1}°C", s.temp)).size(12.0).strong().color(t_color));
                                                });

                                                // Thresholds
                                                let thresh_str = match (s.max, s.crit) {
                                                    (Some(m), Some(c)) => format!("Max: {:.0}°C / Crit: {:.0}°C", m, c),
                                                    (Some(m), None) => format!("Max: {:.0}°C", m),
                                                    (None, Some(c)) => format!("Crit: {:.0}°C", c),
                                                    (None, None) => "—".to_string(),
                                                };
                                                ui.label(RichText::new(thresh_str).size(10.5).color(p.text_muted));

                                                ui.end_row();
                                            }
                                        });
                                }
                            });

                        ui.add_space(8.0);
                    }
                });
                }
                SensorsSubTab::TopApps => {
                    self.render_sensors_top_apps_tab(ui, &snap);
                }
            }
        });
    }

    fn render_sensors_top_apps_tab(&mut self, ui: &mut Ui, snap: &crate::sensors::SystemSensorsSnapshot) {
        let p = self.theme.palette();

        const COL_APP_W: f32 = 250.0;
        const COL_PID_W: f32 = 75.0;
        const COL_STATUS_W: f32 = 85.0;
        const COL_CPU_W: f32 = 135.0;
        const COL_MEM_W: f32 = 145.0;
        const COL_SPEED_W: f32 = 165.0;
        const COL_IO_W: f32 = 145.0;

        let num_logical_cores = snap.cpu_cores_logical.max(1) as f32;
        let norm_factor = if self.sensors_top_apps_normalize_cpu { num_logical_cores } else { 1.0 };

        let mem_total_pct = if snap.mem_total_bytes > 0 {
            (snap.mem_used_bytes as f32 / snap.mem_total_bytes as f32) * 100.0
        } else {
            0.0
        };

        // Heatmap color helper
        let get_cpu_heatmap = |pct: f32| -> Color32 {
            if pct < 1.0 {
                Color32::TRANSPARENT
            } else if pct < 10.0 {
                Color32::from_rgba_premultiplied(59, 130, 246, 25)
            } else if pct < 25.0 {
                Color32::from_rgba_premultiplied(234, 179, 8, 40)
            } else if pct < 50.0 {
                Color32::from_rgba_premultiplied(249, 115, 22, 55)
            } else {
                Color32::from_rgba_premultiplied(239, 68, 68, 75)
            }
        };

        let get_mem_heatmap = |pct: f32| -> Color32 {
            if pct < 1.0 {
                Color32::TRANSPARENT
            } else if pct < 5.0 {
                Color32::from_rgba_premultiplied(59, 130, 246, 25)
            } else if pct < 15.0 {
                Color32::from_rgba_premultiplied(234, 179, 8, 40)
            } else if pct < 30.0 {
                Color32::from_rgba_premultiplied(249, 115, 22, 55)
            } else {
                Color32::from_rgba_premultiplied(239, 68, 68, 75)
            }
        };

        let get_disk_heatmap = |rate_bps: f64| -> Color32 {
            if rate_bps < 100_000.0 {
                Color32::TRANSPARENT
            } else if rate_bps < 5_000_000.0 {
                Color32::from_rgba_premultiplied(59, 130, 246, 25)
            } else if rate_bps < 25_000_000.0 {
                Color32::from_rgba_premultiplied(234, 179, 8, 40)
            } else {
                Color32::from_rgba_premultiplied(249, 115, 22, 55)
            }
        };

        let mut kill_target_pid = None;
        let mut toggle_group_expand = None;
        let mut select_target_pid = None;

        Frame::none()
            .fill(p.bg_card)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
            .show(ui, |ui| {
                // ═════════════════════════════════════════════════════════════
                // WINDOWS TASK MANAGER TOOLBAR
                // ═════════════════════════════════════════════════════════════
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} Processes", crate::lucide::ACTIVITY)).size(14.0).strong().color(p.text_primary));

                    let count_text = format!("{} processes", snap.top_apps.len());
                    Frame::none()
                        .fill(p.bg_button)
                        .rounding(Rounding::same(999.0))
                        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(count_text).size(10.5).strong().color(p.text_secondary));
                        });

                    // Group by App view toggle
                    let btn_grp = egui::Button::new(
                        RichText::new(format!("{} Group by App", if self.sensors_top_apps_group_by_app { crate::lucide::CHECK } else { crate::lucide::LAYERS }))
                            .size(10.5)
                            .strong()
                            .color(if self.sensors_top_apps_group_by_app { p.accent_text } else { p.text_secondary }),
                    )
                    .fill(if self.sensors_top_apps_group_by_app { p.accent } else { p.bg_button })
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(999.0));
                    if ui.add(btn_grp).on_hover_text("Group multiple instances and subprocesses under parent app row (Task Manager style)").clicked() {
                        self.sensors_top_apps_group_by_app = !self.sensors_top_apps_group_by_app;
                    }

                    // Normalized CPU mode toggle (Task Manager Total % vs Raw Per-Core %)
                    let cpu_mode_label = if self.sensors_top_apps_normalize_cpu {
                        format!("{} % Total CPU ({} cores)", crate::lucide::CPU, num_logical_cores as u32)
                    } else {
                        format!("{} % Per-Core CPU", crate::lucide::CPU)
                    };
                    let btn_norm = egui::Button::new(RichText::new(cpu_mode_label).size(10.5).color(p.text_secondary))
                        .fill(p.bg_button)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(999.0));
                    if ui.add(btn_norm).on_hover_text("Switch between Windows Task Manager Total System CPU % (0-100%) and raw per-core CPU %").clicked() {
                        self.sensors_top_apps_normalize_cpu = !self.sensors_top_apps_normalize_cpu;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // End Task Button
                        let has_selection = self.sensors_top_apps_selected_pid.is_some();
                        let btn_end = if has_selection {
                            let sel_pid = self.sensors_top_apps_selected_pid.unwrap();
                            egui::Button::new(RichText::new(format!("{} End task (PID {})", crate::lucide::X, sel_pid)).size(11.0).strong().color(Color32::WHITE))
                                .fill(p.danger)
                                .rounding(Rounding::same(999.0))
                        } else {
                            egui::Button::new(RichText::new(format!("{} End task", crate::lucide::X)).size(11.0).color(p.text_muted))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(999.0))
                        };
                        let btn_end_resp = ui.add(btn_end).on_hover_text("Select a process below and click to terminate/kill it (Windows Task Manager style)");
                        if has_selection && btn_end_resp.clicked() {
                            kill_target_pid = self.sensors_top_apps_selected_pid;
                        }

                        ui.add_space(8.0);

                        // Clear search button if active
                        if !self.sensors_search_query.is_empty() {
                            if ui.button(RichText::new(crate::lucide::X).size(10.5).color(p.text_muted)).on_hover_text("Clear filter").clicked() {
                                self.sensors_search_query.clear();
                            }
                        }

                        // Search box
                        ui.add(
                            egui::TextEdit::singleline(&mut self.sensors_search_query)
                                .hint_text(format!("{} Search by name or PID...", crate::lucide::SEARCH))
                                .desired_width(190.0),
                        );
                    });
                });

                // Status message (e.g. process killed)
                if let Some((ref msg, is_err, timestamp)) = self.sensors_top_apps_status_msg {
                    if timestamp.elapsed() < Duration::from_secs(4) {
                        ui.add_space(4.0);
                        Frame::none()
                            .fill(if is_err { p.danger.gamma_multiply(0.2) } else { Color32::from_rgba_premultiplied(16, 185, 129, 30) })
                            .stroke(Stroke::new(1.0_f32, if is_err { p.danger } else { Color32::from_rgb(16, 185, 129) }))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new(msg).size(10.5).color(if is_err { p.danger } else { Color32::from_rgb(16, 185, 129) }));
                            });
                    }
                }

                ui.add_space(8.0);

                // Filter apps
                let search_q = self.sensors_search_query.trim().to_lowercase();
                let mut filtered_apps = snap.top_apps.clone();
                if !search_q.is_empty() {
                    filtered_apps.retain(|a| {
                        a.name.to_lowercase().contains(&search_q)
                            || a.pid.to_string().contains(&search_q)
                            || a.status.to_lowercase().contains(&search_q)
                    });
                }

                if filtered_apps.is_empty() {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        if !self.sensors_top_apps_visited {
                            ui.spinner();
                            ui.add_space(6.0);
                            ui.label(RichText::new("Loading system processes...").size(12.0).color(p.text_muted));
                        } else {
                            ui.label(RichText::new("No processes match the search query.").size(12.0).color(p.text_muted));
                        }
                    });
                    ui.add_space(24.0);
                    return;
                }

                // ═════════════════════════════════════════════════════════════
                // WINDOWS TASK MANAGER PROCESS TABLE
                // ═════════════════════════════════════════════════════════════
                ScrollArea::vertical()
                    .id_salt("top_apps_v_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ScrollArea::horizontal()
                            .id_salt("top_apps_h_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                egui::Grid::new("top_apps_task_manager_grid")
                                    .num_columns(7)
                                    .striped(true)
                                    .spacing(egui::vec2(10.0, 5.0))
                                    .show(ui, |ui| {
                                        // ═════════════════════════════════════
                                        // CLICKABLE COLUMN HEADERS WITH METRICS & SORT ARROWS
                                        // ═════════════════════════════════════
                                        let mut new_sort = None;

                                        // 1. Name
                                        ui.allocate_ui_with_layout(egui::vec2(COL_APP_W, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            let is_sel = self.sensors_top_apps_sort == "name";
                                            let arrow = if is_sel { if self.sensors_top_apps_sort_desc { " ▼" } else { " ▲" } } else { "" };
                                            let btn = egui::Button::new(RichText::new(format!("Name{}", arrow)).size(11.0).strong().color(if is_sel { p.accent_light } else { p.text_secondary }))
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::NONE);
                                            if ui.add(btn).on_hover_text("Sort by Application Name").clicked() {
                                                new_sort = Some("name");
                                            }
                                        });

                                        // 2. PID
                                        ui.allocate_ui_with_layout(egui::vec2(COL_PID_W, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            let is_sel = self.sensors_top_apps_sort == "pid";
                                            let arrow = if is_sel { if self.sensors_top_apps_sort_desc { " ▼" } else { " ▲" } } else { "" };
                                            let btn = egui::Button::new(RichText::new(format!("PID{}", arrow)).size(11.0).strong().color(if is_sel { p.accent_light } else { p.text_secondary }))
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::NONE);
                                            if ui.add(btn).on_hover_text("Sort by Process ID").clicked() {
                                                new_sort = Some("pid");
                                            }
                                        });

                                        // 3. Status
                                        ui.allocate_ui_with_layout(egui::vec2(COL_STATUS_W, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            let is_sel = self.sensors_top_apps_sort == "status";
                                            let arrow = if is_sel { if self.sensors_top_apps_sort_desc { " ▼" } else { " ▲" } } else { "" };
                                            let btn = egui::Button::new(RichText::new(format!("Status{}", arrow)).size(11.0).strong().color(if is_sel { p.accent_light } else { p.text_secondary }))
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::NONE);
                                            if ui.add(btn).on_hover_text("Sort by Process Status").clicked() {
                                                new_sort = Some("status");
                                            }
                                        });

                                        // 4. CPU (Shows total system CPU % like Task Manager!)
                                        ui.allocate_ui_with_layout(egui::vec2(COL_CPU_W, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            let is_sel = self.sensors_top_apps_sort == "cpu";
                                            let arrow = if is_sel { if self.sensors_top_apps_sort_desc { " ▼" } else { " ▲" } } else { "" };
                                            let cpu_title = format!("CPU  {:.0}%{}", snap.cpu_global_usage, arrow);
                                            let btn = egui::Button::new(RichText::new(cpu_title).size(11.0).strong().color(if is_sel { p.accent_light } else { p.text_primary }))
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::NONE);
                                            if ui.add(btn).on_hover_text("Sort by CPU Usage (Click to toggle ascending/descending)").clicked() {
                                                new_sort = Some("cpu");
                                            }
                                        });

                                        // 5. Memory (Shows total RAM % like Task Manager!)
                                        ui.allocate_ui_with_layout(egui::vec2(COL_MEM_W, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            let is_sel = self.sensors_top_apps_sort == "mem";
                                            let arrow = if is_sel { if self.sensors_top_apps_sort_desc { " ▼" } else { " ▲" } } else { "" };
                                            let mem_title = format!("Memory  {:.0}%{}", mem_total_pct, arrow);
                                            let btn = egui::Button::new(RichText::new(mem_title).size(11.0).strong().color(if is_sel { p.accent_light } else { p.text_primary }))
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::NONE);
                                            if ui.add(btn).on_hover_text("Sort by Memory Usage (Click to toggle ascending/descending)").clicked() {
                                                new_sort = Some("mem");
                                            }
                                        });

                                        // 6. Disk Speed
                                        ui.allocate_ui_with_layout(egui::vec2(COL_SPEED_W, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            let is_sel = self.sensors_top_apps_sort == "disk";
                                            let arrow = if is_sel { if self.sensors_top_apps_sort_desc { " ▼" } else { " ▲" } } else { "" };
                                            let btn = egui::Button::new(RichText::new(format!("Disk Speed{}", arrow)).size(11.0).strong().color(if is_sel { Color32::from_rgb(56, 189, 248) } else { p.text_secondary }))
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::NONE);
                                            if ui.add(btn).on_hover_text("Sort by Active Disk Read/Write Speed").clicked() {
                                                new_sort = Some("disk");
                                            }
                                        });

                                        // 7. Lifetime I/O
                                        ui.allocate_ui_with_layout(egui::vec2(COL_IO_W, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            ui.label(RichText::new("Lifetime Disk I/O").size(11.0).strong().color(p.text_muted));
                                        });
                                        ui.end_row();

                                        if let Some(col) = new_sort {
                                            if self.sensors_top_apps_sort == col {
                                                self.sensors_top_apps_sort_desc = !self.sensors_top_apps_sort_desc;
                                            } else {
                                                self.sensors_top_apps_sort = col.to_string();
                                                self.sensors_top_apps_sort_desc = col != "name";
                                            }
                                        }

                                        // ═════════════════════════════════════
                                        // PROCESS ROWS RENDERING (GROUPED OR FLAT)
                                        // ═════════════════════════════════════
                                        if self.sensors_top_apps_group_by_app {
                                            // Group processes by executable name
                                            let mut groups_map: HashMap<String, (String, Vec<crate::sensors::ProcessSensorInfo>)> = HashMap::new();
                                            for app in filtered_apps {
                                                let key = app.name.trim().to_lowercase();
                                                let entry = groups_map.entry(key).or_insert_with(|| (app.name.clone(), Vec::new()));
                                                entry.1.push(app);
                                            }

                                            struct GroupSummary {
                                                key: String,
                                                display_name: String,
                                                sum_cpu: f32,
                                                sum_mem: u64,
                                                sum_disk_rate: f64,
                                                sum_read_rate: f64,
                                                sum_write_rate: f64,
                                                sum_read_bytes: u64,
                                                sum_write_bytes: u64,
                                                primary_status: String,
                                                pids: Vec<u32>,
                                                children: Vec<crate::sensors::ProcessSensorInfo>,
                                            }

                                            let mut groups: Vec<GroupSummary> = groups_map.into_iter().map(|(key, (disp_name, mut list))| {
                                                // sort children by CPU
                                                list.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap_or(std::cmp::Ordering::Equal));
                                                let sum_cpu: f32 = list.iter().map(|p| p.cpu_usage).sum();
                                                let sum_mem: u64 = list.iter().map(|p| p.memory_bytes).sum();
                                                let sum_disk_rate: f64 = list.iter().map(|p| p.disk_rate_bps).sum();
                                                let sum_read_rate: f64 = list.iter().map(|p| p.disk_read_rate_bps).sum();
                                                let sum_write_rate: f64 = list.iter().map(|p| p.disk_write_rate_bps).sum();
                                                let sum_read_bytes: u64 = list.iter().map(|p| p.disk_read_bytes).sum();
                                                let sum_write_bytes: u64 = list.iter().map(|p| p.disk_written_bytes).sum();
                                                let pids: Vec<u32> = list.iter().map(|p| p.pid).collect();
                                                let is_any_running = list.iter().any(|p| p.status.to_lowercase().contains("run"));
                                                let primary_status = if is_any_running { "Running".to_string() } else { "Sleep".to_string() };

                                                GroupSummary {
                                                    key,
                                                    display_name: disp_name,
                                                    sum_cpu,
                                                    sum_mem,
                                                    sum_disk_rate,
                                                    sum_read_rate,
                                                    sum_write_rate,
                                                    sum_read_bytes,
                                                    sum_write_bytes,
                                                    primary_status,
                                                    pids,
                                                    children: list,
                                                }
                                            }).collect();

                                            // Sort groups
                                            let desc = self.sensors_top_apps_sort_desc;
                                            match self.sensors_top_apps_sort.as_str() {
                                                "mem" => groups.sort_by(|a, b| if desc { b.sum_mem.cmp(&a.sum_mem) } else { a.sum_mem.cmp(&b.sum_mem) }),
                                                "disk" => groups.sort_by(|a, b| if desc { b.sum_disk_rate.partial_cmp(&a.sum_disk_rate).unwrap_or(std::cmp::Ordering::Equal) } else { a.sum_disk_rate.partial_cmp(&b.sum_disk_rate).unwrap_or(std::cmp::Ordering::Equal) }),
                                                "name" => groups.sort_by(|a, b| if desc { b.display_name.to_lowercase().cmp(&a.display_name.to_lowercase()) } else { a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()) }),
                                                "pid" => groups.sort_by(|a, b| if desc { b.pids.get(0).cmp(&a.pids.get(0)) } else { a.pids.get(0).cmp(&b.pids.get(0)) }),
                                                "status" => groups.sort_by(|a, b| if desc { b.primary_status.cmp(&a.primary_status) } else { a.primary_status.cmp(&b.primary_status) }),
                                                _ => groups.sort_by(|a, b| if desc { b.sum_cpu.partial_cmp(&a.sum_cpu).unwrap_or(std::cmp::Ordering::Equal) } else { a.sum_cpu.partial_cmp(&b.sum_cpu).unwrap_or(std::cmp::Ordering::Equal) }),
                                            }

                                            for grp in groups.iter().take(60) {
                                                let is_multi = grp.children.len() > 1;
                                                let is_expanded = self.sensors_top_apps_expanded_groups.contains(&grp.key);
                                                let is_group_selected = grp.pids.iter().any(|pid| self.sensors_top_apps_selected_pid == Some(*pid));

                                                let norm_cpu = grp.sum_cpu / norm_factor;
                                                let mem_str = crate::sensors::format_bytes(grp.sum_mem);
                                                let mem_pct = if snap.mem_total_bytes > 0 {
                                                    (grp.sum_mem as f32 / snap.mem_total_bytes as f32) * 100.0
                                                } else {
                                                    0.0
                                                };

                                                let cpu_fill = get_cpu_heatmap(norm_cpu);
                                                let mem_fill = get_mem_heatmap(mem_pct);
                                                let disk_fill = get_disk_heatmap(grp.sum_disk_rate);

                                                // 1. Name & Expander
                                                ui.allocate_ui_with_layout(egui::vec2(COL_APP_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    if is_multi {
                                                        let chev = if is_expanded { crate::lucide::CHEVRON_DOWN } else { crate::lucide::CHEVRON_RIGHT };
                                                        if ui.button(RichText::new(chev).size(11.0).color(p.text_muted)).clicked() {
                                                            toggle_group_expand = Some(grp.key.clone());
                                                        }
                                                    } else {
                                                        ui.add_space(14.0);
                                                    }

                                                    ui.label(RichText::new(crate::lucide::ACTIVITY).size(11.0).color(p.accent));
                                                    let short_name = if grp.display_name.len() > 20 {
                                                        format!("{}…", &grp.display_name[..18])
                                                    } else {
                                                        grp.display_name.clone()
                                                    };
                                                    let resp = ui.label(RichText::new(short_name).size(11.5).strong().color(if is_group_selected { p.accent_light } else { p.text_primary }));
                                                    if grp.display_name.len() > 20 {
                                                        resp.on_hover_text(&grp.display_name);
                                                    }

                                                    if is_multi {
                                                        Frame::none()
                                                            .fill(p.bg_button)
                                                            .rounding(Rounding::same(999.0))
                                                            .inner_margin(egui::Margin::symmetric(5.0, 1.0))
                                                            .show(ui, |ui| {
                                                                ui.label(RichText::new(format!("{}", grp.children.len())).size(9.5).color(p.text_muted));
                                                            });
                                                    }
                                                });

                                                // 2. PID
                                                ui.allocate_ui_with_layout(egui::vec2(COL_PID_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    let pid_txt = if is_multi {
                                                        format!("{} PIDs", grp.children.len())
                                                    } else {
                                                        format!("{}", grp.pids[0])
                                                    };
                                                    let resp = ui.label(RichText::new(pid_txt).size(11.0).monospace().color(p.text_muted));
                                                    if ui.interact(resp.rect, resp.id.with("click"), egui::Sense::click()).clicked() {
                                                        select_target_pid = Some(grp.pids[0]);
                                                    }
                                                });

                                                // 3. Status
                                                ui.allocate_ui_with_layout(egui::vec2(COL_STATUS_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    let (st_color, st_bg) = if grp.primary_status == "Running" {
                                                        (Color32::from_rgb(16, 185, 129), Color32::from_rgba_premultiplied(16, 185, 129, 25))
                                                    } else {
                                                        (p.text_secondary, p.bg_button)
                                                    };
                                                    Frame::none()
                                                        .fill(st_bg)
                                                        .stroke(Stroke::NONE)
                                                        .rounding(Rounding::same(999.0))
                                                        .inner_margin(egui::Margin::symmetric(6.0, 1.5))
                                                        .show(ui, |ui| {
                                                            ui.label(RichText::new(&grp.primary_status).size(9.5).color(st_color));
                                                        });
                                                });

                                                // 4. CPU (Heatmap tinted cell)
                                                ui.allocate_ui_with_layout(egui::vec2(COL_CPU_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    let cell_rect = ui.available_rect_before_wrap();
                                                    if cpu_fill != Color32::TRANSPARENT {
                                                        ui.painter().rect_filled(cell_rect.shrink2(egui::vec2(2.0, 1.0)), 4.0, cpu_fill);
                                                    }
                                                    let cpu_col = if norm_cpu >= 50.0 { p.danger }
                                                        else if norm_cpu >= 15.0 { Color32::from_rgb(250, 204, 21) }
                                                        else { p.text_primary };
                                                    let cpu_lbl = ui.label(RichText::new(format!("{:.1}%", norm_cpu)).size(11.0).strong().monospace().color(cpu_col));
                                                    cpu_lbl.on_hover_text(format!("{:.1}% Total System CPU\n({:.1}% single-core raw across {} cores)", norm_cpu, grp.sum_cpu, num_logical_cores as u32));
                                                });

                                                // 5. Memory (Heatmap tinted cell)
                                                ui.allocate_ui_with_layout(egui::vec2(COL_MEM_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    let cell_rect = ui.available_rect_before_wrap();
                                                    if mem_fill != Color32::TRANSPARENT {
                                                        ui.painter().rect_filled(cell_rect.shrink2(egui::vec2(2.0, 1.0)), 4.0, mem_fill);
                                                    }
                                                    ui.label(RichText::new(format!("{} ({:.1}%)", mem_str, mem_pct)).size(11.0).monospace().color(p.text_secondary));
                                                });

                                                // 6. Disk Speed (Heatmap tinted cell)
                                                ui.allocate_ui_with_layout(egui::vec2(COL_SPEED_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    let cell_rect = ui.available_rect_before_wrap();
                                                    if disk_fill != Color32::TRANSPARENT {
                                                        ui.painter().rect_filled(cell_rect.shrink2(egui::vec2(2.0, 1.0)), 4.0, disk_fill);
                                                    }
                                                    ui.horizontal(|ui| {
                                                        ui.label(RichText::new(crate::lucide::ARROW_DOWN).size(10.0).color(Color32::from_rgb(56, 189, 248)));
                                                        ui.label(RichText::new(crate::sensors::format_rate(grp.sum_read_rate)).size(10.5).monospace().color(p.text_primary));
                                                        ui.add_space(2.0);
                                                        ui.label(RichText::new(crate::lucide::ARROW_UP).size(10.0).color(Color32::from_rgb(168, 85, 247)));
                                                        ui.label(RichText::new(crate::sensors::format_rate(grp.sum_write_rate)).size(10.5).monospace().color(p.text_primary));
                                                    });
                                                });

                                                // 7. Lifetime I/O
                                                ui.allocate_ui_with_layout(egui::vec2(COL_IO_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    let lt_str = format!("R: {} / W: {}", crate::sensors::format_bytes(grp.sum_read_bytes), crate::sensors::format_bytes(grp.sum_write_bytes));
                                                    ui.label(RichText::new(lt_str).size(10.0).color(p.text_muted));
                                                });
                                                ui.end_row();

                                                // Render expanded children if expanded
                                                if is_multi && is_expanded {
                                                    for child in &grp.children {
                                                        let is_child_sel = self.sensors_top_apps_selected_pid == Some(child.pid);
                                                        let child_norm_cpu = child.cpu_usage / norm_factor;
                                                        let child_mem_pct = if snap.mem_total_bytes > 0 {
                                                            (child.memory_bytes as f32 / snap.mem_total_bytes as f32) * 100.0
                                                        } else {
                                                            0.0
                                                        };

                                                        // Child Name (indented)
                                                        ui.allocate_ui_with_layout(egui::vec2(COL_APP_W, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                            ui.add_space(22.0);
                                                            let child_lbl = format!("↳ PID {}", child.pid);
                                                            let resp = ui.label(RichText::new(child_lbl).size(11.0).monospace().color(if is_child_sel { p.accent_light } else { p.text_secondary }));
                                                            if ui.interact(resp.rect, resp.id.with("child_sel"), egui::Sense::click()).clicked() {
                                                                select_target_pid = Some(child.pid);
                                                            }
                                                        });

                                                        // Child PID
                                                        ui.allocate_ui_with_layout(egui::vec2(COL_PID_W, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                            let resp = ui.label(RichText::new(format!("{}", child.pid)).size(10.5).monospace().color(p.text_muted));
                                                            if ui.interact(resp.rect, resp.id.with("pid_sel"), egui::Sense::click()).clicked() {
                                                                select_target_pid = Some(child.pid);
                                                            }
                                                        });

                                                        // Child Status
                                                        ui.allocate_ui_with_layout(egui::vec2(COL_STATUS_W, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                            ui.label(RichText::new(&child.status).size(9.0).color(p.text_muted));
                                                        });

                                                        // Child CPU
                                                        ui.allocate_ui_with_layout(egui::vec2(COL_CPU_W, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                            ui.label(RichText::new(format!("{:.1}%", child_norm_cpu)).size(10.5).monospace().color(p.text_secondary));
                                                        });

                                                        // Child Memory
                                                        ui.allocate_ui_with_layout(egui::vec2(COL_MEM_W, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                            ui.label(RichText::new(format!("{} ({:.1}%)", crate::sensors::format_bytes(child.memory_bytes), child_mem_pct)).size(10.0).monospace().color(p.text_muted));
                                                        });

                                                        // Child Disk
                                                        ui.allocate_ui_with_layout(egui::vec2(COL_SPEED_W, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                            ui.label(RichText::new(crate::sensors::format_rate(child.disk_rate_bps)).size(10.0).monospace().color(p.text_muted));
                                                        });

                                                        // Child Lifetime
                                                        ui.allocate_ui_with_layout(egui::vec2(COL_IO_W, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                            ui.label(RichText::new(crate::sensors::format_bytes(child.disk_read_bytes + child.disk_written_bytes)).size(9.5).color(p.text_muted));
                                                        });
                                                        ui.end_row();
                                                    }
                                                }
                                            }
                                        } else {
                                            // ═════════════════════════════════
                                            // FLAT LIST VIEW
                                            // ═════════════════════════════════
                                            let desc = self.sensors_top_apps_sort_desc;
                                            match self.sensors_top_apps_sort.as_str() {
                                                "mem" => filtered_apps.sort_by(|a, b| if desc { b.memory_bytes.cmp(&a.memory_bytes) } else { a.memory_bytes.cmp(&b.memory_bytes) }),
                                                "disk" => filtered_apps.sort_by(|a, b| if desc { b.disk_rate_bps.partial_cmp(&a.disk_rate_bps).unwrap_or(std::cmp::Ordering::Equal) } else { a.disk_rate_bps.partial_cmp(&b.disk_rate_bps).unwrap_or(std::cmp::Ordering::Equal) }),
                                                "name" => filtered_apps.sort_by(|a, b| if desc { b.name.to_lowercase().cmp(&a.name.to_lowercase()) } else { a.name.to_lowercase().cmp(&b.name.to_lowercase()) }),
                                                "pid" => filtered_apps.sort_by(|a, b| if desc { b.pid.cmp(&a.pid) } else { a.pid.cmp(&b.pid) }),
                                                "status" => filtered_apps.sort_by(|a, b| if desc { b.status.cmp(&a.status) } else { a.status.cmp(&b.status) }),
                                                _ => filtered_apps.sort_by(|a, b| if desc { b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap_or(std::cmp::Ordering::Equal) } else { a.cpu_usage.partial_cmp(&b.cpu_usage).unwrap_or(std::cmp::Ordering::Equal) }),
                                            }

                                            for app in filtered_apps.iter().take(80) {
                                                let is_sel = self.sensors_top_apps_selected_pid == Some(app.pid);
                                                let norm_cpu = app.cpu_usage / norm_factor;
                                                let mem_str = crate::sensors::format_bytes(app.memory_bytes);
                                                let mem_pct = if snap.mem_total_bytes > 0 {
                                                    (app.memory_bytes as f32 / snap.mem_total_bytes as f32) * 100.0
                                                } else {
                                                    0.0
                                                };

                                                let cpu_fill = get_cpu_heatmap(norm_cpu);
                                                let mem_fill = get_mem_heatmap(mem_pct);
                                                let disk_fill = get_disk_heatmap(app.disk_rate_bps);

                                                // 1. Name
                                                ui.allocate_ui_with_layout(egui::vec2(COL_APP_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.label(RichText::new(crate::lucide::ACTIVITY).size(11.0).color(p.accent));
                                                        let app_name = if app.name.is_empty() { format!("PID {}", app.pid) } else { app.name.clone() };
                                                        let short_name = if app_name.len() > 24 {
                                                            format!("{}…", &app_name[..22])
                                                        } else {
                                                            app_name.clone()
                                                        };
                                                        let mut lbl = ui.label(RichText::new(short_name).size(11.5).strong().color(if is_sel { p.accent_light } else { p.text_primary }));
                                                        if app_name.len() > 24 {
                                                            lbl = lbl.on_hover_text(&app_name);
                                                        }
                                                        if ui.interact(lbl.rect, lbl.id.with("flat_sel"), egui::Sense::click()).clicked() {
                                                            select_target_pid = Some(app.pid);
                                                        }
                                                    });
                                                });

                                                // 2. PID
                                                ui.allocate_ui_with_layout(egui::vec2(COL_PID_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    let resp = ui.label(RichText::new(format!("{}", app.pid)).size(11.0).monospace().color(p.text_muted));
                                                    if ui.interact(resp.rect, resp.id.with("pid_click"), egui::Sense::click()).clicked() {
                                                        select_target_pid = Some(app.pid);
                                                    }
                                                });

                                                // 3. Status
                                                ui.allocate_ui_with_layout(egui::vec2(COL_STATUS_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    let (st_color, st_bg) = match app.status.to_lowercase().as_str() {
                                                        "run" | "running" => (Color32::from_rgb(16, 185, 129), Color32::from_rgba_premultiplied(16, 185, 129, 25)),
                                                        "idle" | "sleep" | "sleeping" => (p.text_secondary, p.bg_button),
                                                        _ => (Color32::from_rgb(56, 189, 248), Color32::from_rgba_premultiplied(56, 189, 248, 25)),
                                                    };
                                                    Frame::none()
                                                        .fill(st_bg)
                                                        .stroke(Stroke::NONE)
                                                        .rounding(Rounding::same(999.0))
                                                        .inner_margin(egui::Margin::symmetric(6.0, 1.5))
                                                        .show(ui, |ui| {
                                                            ui.label(RichText::new(&app.status).size(9.5).color(st_color));
                                                        });
                                                });

                                                // 4. CPU (Heatmap tinted cell)
                                                ui.allocate_ui_with_layout(egui::vec2(COL_CPU_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    let cell_rect = ui.available_rect_before_wrap();
                                                    if cpu_fill != Color32::TRANSPARENT {
                                                        ui.painter().rect_filled(cell_rect.shrink2(egui::vec2(2.0, 1.0)), 4.0, cpu_fill);
                                                    }
                                                    let cpu_col = if norm_cpu >= 50.0 { p.danger }
                                                        else if norm_cpu >= 15.0 { Color32::from_rgb(250, 204, 21) }
                                                        else { p.text_primary };
                                                    let cpu_lbl = ui.label(RichText::new(format!("{:.1}%", norm_cpu)).size(11.0).strong().monospace().color(cpu_col));
                                                    cpu_lbl.on_hover_text(format!("{:.1}% Total System CPU\n({:.1}% single-core raw across {} cores)", norm_cpu, app.cpu_usage, num_logical_cores as u32));
                                                });

                                                // 5. Memory (Heatmap tinted cell)
                                                ui.allocate_ui_with_layout(egui::vec2(COL_MEM_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    let cell_rect = ui.available_rect_before_wrap();
                                                    if mem_fill != Color32::TRANSPARENT {
                                                        ui.painter().rect_filled(cell_rect.shrink2(egui::vec2(2.0, 1.0)), 4.0, mem_fill);
                                                    }
                                                    ui.label(RichText::new(format!("{} ({:.1}%)", mem_str, mem_pct)).size(11.0).monospace().color(p.text_secondary));
                                                });

                                                // 6. Disk Speed (Heatmap tinted cell)
                                                ui.allocate_ui_with_layout(egui::vec2(COL_SPEED_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    let cell_rect = ui.available_rect_before_wrap();
                                                    if disk_fill != Color32::TRANSPARENT {
                                                        ui.painter().rect_filled(cell_rect.shrink2(egui::vec2(2.0, 1.0)), 4.0, disk_fill);
                                                    }
                                                    ui.horizontal(|ui| {
                                                        ui.label(RichText::new(crate::lucide::ARROW_DOWN).size(10.0).color(Color32::from_rgb(56, 189, 248)));
                                                        ui.label(RichText::new(crate::sensors::format_rate(app.disk_read_rate_bps)).size(10.5).monospace().color(p.text_primary));
                                                        ui.add_space(2.0);
                                                        ui.label(RichText::new(crate::lucide::ARROW_UP).size(10.0).color(Color32::from_rgb(168, 85, 247)));
                                                        ui.label(RichText::new(crate::sensors::format_rate(app.disk_write_rate_bps)).size(10.5).monospace().color(p.text_primary));
                                                    });
                                                });

                                                // 7. Lifetime I/O
                                                ui.allocate_ui_with_layout(egui::vec2(COL_IO_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    let lt_str = format!("R: {} / W: {}", crate::sensors::format_bytes(app.disk_read_bytes), crate::sensors::format_bytes(app.disk_written_bytes));
                                                    ui.label(RichText::new(lt_str).size(10.0).color(p.text_muted));
                                                });
                                                ui.end_row();
                                            }
                                        }
                                    });
                            });
                    });
            });

        // Apply state updates outside borrow closures
        if let Some(key) = toggle_group_expand {
            if self.sensors_top_apps_expanded_groups.contains(&key) {
                self.sensors_top_apps_expanded_groups.remove(&key);
            } else {
                self.sensors_top_apps_expanded_groups.insert(key);
            }
        }

        if let Some(pid) = select_target_pid {
            if self.sensors_top_apps_selected_pid == Some(pid) {
                self.sensors_top_apps_selected_pid = None; // toggle deselect
            } else {
                self.sensors_top_apps_selected_pid = Some(pid);
            }
        }

        if let Some(pid) = kill_target_pid {
            crate::scanner::kill_process_pid(pid);
            self.sensors_top_apps_selected_pid = None;
            self.sensors_top_apps_status_msg = Some((format!("Terminated process PID {}", pid), false, Instant::now()));
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // LEFT SIDEBAR: Group Filter + Server Cards (Expands full height)
    // ═════════════════════════════════════════════════════════════════════════
    fn render_servers_left_panel(&mut self, ui: &mut Ui) {
        let p = self.theme.palette();
        ui.set_width(ui.available_width());
        ui.set_max_width(ui.available_width());
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.set_max_width(ui.available_width());
            // 1. Group Filter Pills Bar
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Group:").size(11.5).color(p.text_muted));

                let mut all_groups = vec!["All".to_string()];
                let mut unique_groups = BTreeSet::new();
                for s in &self.services {
                    unique_groups.insert(s.config.group.clone());
                }
                all_groups.extend(unique_groups);

                for grp in all_groups {
                    let is_active = self.active_group_filter == grp;
                    let bg_color = if is_active {
                        p.accent
                    } else {
                        p.bg_button
                    };
                    let text_color = if is_active {
                        p.accent_text
                    } else {
                        p.text_secondary
                    };

                    let btn = egui::Button::new(RichText::new(&grp).size(11.0).color(text_color))
                        .fill(bg_color)
                        .rounding(Rounding::same(999.0))
                        .stroke(Stroke::NONE);

                    if ui.add(btn).clicked() {
                        self.active_group_filter = grp.clone();
                    }
                }
            });

            ui.add_space(6.0);

            // 2. Scrollable list of Groups & Server Cards (NEVER shrinks, fills full height!)
            ScrollArea::vertical()
                .id_salt("left_cards_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_max_width(ui.available_width());
                    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
                    for (idx, s) in self.services.iter().enumerate() {
                        if self.active_group_filter == "All" || self.active_group_filter == s.config.group {
                            groups
                                .entry(s.config.group.clone())
                                .or_default()
                                .push(idx);
                        }
                    }

                    let mut to_start = None;
                    let mut to_stop = None;
                    let mut to_restart = None;
                    let mut to_edit = None;
                    let mut to_delete = None;
                    let mut to_switch_tab = None;
                    let mut start_grp = None;
                    let mut stop_grp = None;
                    let mut to_save_config = false;

                    for (group_name, indices) in groups {
                        ui.add_space(4.0);
                        // Group Section Header (Pill container)
                        Frame::none()
                            .fill(p.bg_subtle)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .inner_margin(egui::Margin::symmetric(12.0, 6.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let header_text = format!("{}  {} ({})", crate::lucide::TAG, group_name.to_uppercase(), indices.len());
                                    let title_w = (ui.available_width() - 150.0).max(40.0);
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(title_w, 20.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.set_max_width(title_w);
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(header_text)
                                                        .size(11.5)
                                                        .strong()
                                                        .color(p.accent_light),
                                                )
                                                .truncate(),
                                            );
                                        },
                                    );

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        let btn_stop_grp = egui::Button::new(
                                            RichText::new(format!("{} Stop", crate::lucide::STOP)).size(10.5).strong().color(Color32::from_rgb(0xff, 0xda, 0xd6)),
                                        )
                                        .fill(Color32::from_rgb(0x99, 0x1b, 0x1b))
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .min_size(egui::vec2(60.0, 22.0));
                                        if ui.add(btn_stop_grp).on_hover_text("Stop all servers in this group").clicked() {
                                            stop_grp = Some(group_name.clone());
                                        }

                                        let btn_start_grp = egui::Button::new(
                                            RichText::new(format!("{} Start", crate::lucide::PLAY)).size(10.5).strong().color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(0x05, 0x96, 0x69))
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .min_size(egui::vec2(60.0, 22.0));
                                        if ui.add(btn_start_grp).on_hover_text("Start all servers in this group").clicked() {
                                            start_grp = Some(group_name.clone());
                                        }
                                    });
                                });
                            });

                        ui.add_space(6.0);

                        // Server Cards for this group (Nocturnal Command exact container view)
                        for idx in indices {
                            let s = &mut self.services[idx];
                            let key = s.config.key.clone();
                            let is_running = s.state == ServiceState::Running || s.state == ServiceState::Starting;
                            let is_selected_tab = self.active_tab == key;
                            let card_bg = if is_selected_tab {
                                Color32::from_rgb(0x22, 0x24, 0x30)
                            } else {
                                Color32::from_rgb(0x18, 0x19, 0x22)
                            };

                            Frame::none()
                                .fill(card_bg)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(18.0))
                                .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());

                                    // Row 1: Header (Glowing dot + Server Title on Left, Status chip badge on Right)
                                    let (dot_col, halo_col, chip_bg, chip_text, status_label) = if s.restart_at.is_some() {
                                        (
                                            Color32::from_rgb(0xfc, 0xd3, 0x4d),
                                            Color32::from_rgb(0xfc, 0xd3, 0x4d).gamma_multiply(0.25),
                                            Color32::from_rgb(0x35, 0x22, 0x05),
                                            Color32::from_rgb(0xfc, 0xd3, 0x4d),
                                            "Restarting",
                                        )
                                    } else if is_running {
                                        (
                                            Color32::from_rgb(0x5e, 0xea, 0xd4),
                                            Color32::from_rgb(0x5e, 0xea, 0xd4).gamma_multiply(0.28),
                                            Color32::from_rgb(0x13, 0x27, 0x22),
                                            Color32::from_rgb(0x5e, 0xea, 0xd4),
                                            "Running",
                                        )
                                    } else if s.state == ServiceState::Errored {
                                        (
                                            Color32::from_rgb(0xf8, 0x71, 0x71),
                                            Color32::from_rgb(0xf8, 0x71, 0x71).gamma_multiply(0.25),
                                            Color32::from_rgb(0x38, 0x14, 0x18),
                                            Color32::from_rgb(0xf8, 0x71, 0x71),
                                            "Errored",
                                        )
                                    } else {
                                        (
                                            Color32::from_rgb(0x64, 0x74, 0x8b),
                                            Color32::from_rgb(0x64, 0x74, 0x8b).gamma_multiply(0.20),
                                            Color32::from_rgb(0x20, 0x23, 0x2d),
                                            Color32::from_rgb(0x94, 0xa3, 0xb8),
                                            "Stopped",
                                        )
                                    };

                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 8.0;

                                        // Glowing dot
                                        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                                        ui.painter().circle_filled(dot_rect.center(), 7.0, halo_col);
                                        ui.painter().circle_filled(dot_rect.center(), 4.0, dot_col);

                                        // Title
                                        let name_resp = ui.add(
                                            egui::Label::new(
                                                RichText::new(&s.config.name)
                                                    .strong()
                                                    .size(14.0)
                                                    .color(Color32::from_rgb(0xf1, 0xf5, 0xf9)),
                                            )
                                            .sense(egui::Sense::click()),
                                        )
                                        .on_hover_text(&s.config.name);

                                        if name_resp.clicked() {
                                            to_switch_tab = Some(key.clone());
                                        }

                                        // Status Chip (pill badge on the right, zero border)
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            Frame::none()
                                                .fill(chip_bg)
                                                .stroke(Stroke::NONE)
                                                .rounding(Rounding::same(999.0))
                                                .inner_margin(egui::Margin::symmetric(10.0, 3.5))
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        RichText::new(status_label)
                                                            .size(11.0)
                                                            .strong()
                                                            .color(chip_text),
                                                    );
                                                });
                                        });
                                    });

                                    ui.add_space(8.0);

                                    // Row 2: Recessed Command Box (full width, zero border)
                                    Frame::none()
                                        .fill(Color32::from_rgb(0x10, 0x11, 0x17))
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(8.0))
                                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(&s.config.command)
                                                        .size(11.0)
                                                        .monospace()
                                                        .color(Color32::from_rgb(0x38, 0xbd, 0xf8)),
                                                );
                                                if s.config.port > 0 {
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(
                                                            RichText::new(format!(":{}", s.config.port))
                                                                .size(10.5)
                                                                .monospace()
                                                                .color(Color32::from_rgb(0x64, 0x74, 0x8b)),
                                                        );
                                                    });
                                                }
                                            });
                                        });

                                    ui.add_space(7.0);

                                    // Row 3: Micro Telemetry Pills (PID, CPU%, RAM MB, Uptime)
                                    let pill_bg = Color32::from_rgb(0x23, 0x25, 0x30);
                                    let pill_rounding = Rounding::same(6.0);
                                    let pill_margin = egui::Margin::symmetric(8.0, 4.0);

                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;

                                        if is_running {
                                            if let Some(pid) = s.pid {
                                                Frame::none()
                                                    .fill(pill_bg)
                                                    .stroke(Stroke::NONE)
                                                    .rounding(pill_rounding)
                                                    .inner_margin(pill_margin)
                                                    .show(ui, |ui| {
                                                        ui.horizontal(|ui| {
                                                            ui.spacing_mut().item_spacing.x = 4.0;
                                                            ui.label(RichText::new(crate::lucide::HASH).size(10.0));
                                                            ui.label(
                                                                RichText::new(format!("{}", pid))
                                                                    .size(11.0)
                                                                    .monospace()
                                                                    .color(Color32::from_rgb(0x94, 0xa3, 0xb8)),
                                                            );
                                                        });
                                                    });
                                            }

                                            Frame::none()
                                                .fill(pill_bg)
                                                .stroke(Stroke::NONE)
                                                .rounding(pill_rounding)
                                                .inner_margin(pill_margin)
                                                .show(ui, |ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 4.0;
                                                        ui.label(RichText::new(crate::lucide::CPU).size(10.5).color(Color32::from_rgb(0x38, 0xbd, 0xf8)));
                                                        ui.label(
                                                            RichText::new(format!("{:.1}%", s.cpu_usage))
                                                                .size(11.0)
                                                                .monospace()
                                                                .strong()
                                                                .color(Color32::from_rgb(0x38, 0xbd, 0xf8)),
                                                        );
                                                    });
                                                });

                                            Frame::none()
                                                .fill(pill_bg)
                                                .stroke(Stroke::NONE)
                                                .rounding(pill_rounding)
                                                .inner_margin(pill_margin)
                                                .show(ui, |ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 4.0;
                                                        ui.label(RichText::new(crate::lucide::RAM).size(10.0).color(Color32::from_rgb(0x4a, 0xde, 0x80)));
                                                        ui.label(
                                                            RichText::new(format!("{:.1} MB", s.memory_mb))
                                                                .size(11.0)
                                                                .monospace()
                                                                .strong()
                                                                .color(Color32::from_rgb(0x4a, 0xde, 0x80)),
                                                        );
                                                    });
                                                });

                                            let uptime = s.uptime_formatted();
                                            let uptime_str = if uptime.is_empty() { "0s".to_string() } else { uptime };
                                            Frame::none()
                                                .fill(pill_bg)
                                                .stroke(Stroke::NONE)
                                                .rounding(pill_rounding)
                                                .inner_margin(pill_margin)
                                                .show(ui, |ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 4.0;
                                                        ui.label(RichText::new(crate::lucide::CLOCK).size(10.5).color(Color32::from_rgb(0x94, 0xa3, 0xb8)));
                                                        ui.label(
                                                            RichText::new(uptime_str)
                                                                .size(11.0)
                                                                .monospace()
                                                                .color(Color32::from_rgb(0x94, 0xa3, 0xb8)),
                                                        );
                                                    });
                                                });
                                        } else {
                                            Frame::none()
                                                .fill(pill_bg)
                                                .stroke(Stroke::NONE)
                                                .rounding(pill_rounding)
                                                .inner_margin(pill_margin)
                                                .show(ui, |ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 4.0;
                                                        ui.label(RichText::new(crate::lucide::STOP).size(9.0).color(Color32::from_rgb(0x94, 0xa3, 0xb8)));
                                                        ui.label(RichText::new("Idle").size(11.0).monospace().color(Color32::from_rgb(0x94, 0xa3, 0xb8)));
                                                    });
                                                });

                                            if s.config.port > 0 {
                                                Frame::none()
                                                    .fill(pill_bg)
                                                    .stroke(Stroke::NONE)
                                                    .rounding(pill_rounding)
                                                    .inner_margin(pill_margin)
                                                    .show(ui, |ui| {
                                                        ui.horizontal(|ui| {
                                                            ui.spacing_mut().item_spacing.x = 4.0;
                                                            ui.label(RichText::new(crate::lucide::NETWORK).size(10.0).color(Color32::from_rgb(0x70, 0xd5, 0xe4)));
                                                            ui.label(RichText::new(format!(":{}", s.config.port)).size(11.0).monospace().color(Color32::from_rgb(0x70, 0xd5, 0xe4)));
                                                        });
                                                    });
                                            }

                                            let folder_name = std::path::Path::new(&s.config.cwd)
                                                .file_name()
                                                .map(|n| n.to_string_lossy().to_string())
                                                .unwrap_or_else(|| "dir".to_string());
                                            Frame::none()
                                                .fill(pill_bg)
                                                .stroke(Stroke::NONE)
                                                .rounding(pill_rounding)
                                                .inner_margin(pill_margin)
                                                .show(ui, |ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 4.0;
                                                        ui.label(RichText::new(crate::lucide::FOLDER).size(10.0));
                                                        ui.label(RichText::new(folder_name).size(10.5).monospace().color(Color32::from_rgb(0x94, 0xa3, 0xb8)));
                                                    });
                                                });
                                        }
                                    });

                                    ui.add_space(8.0);

                                    // Row 4: Action Buttons (Start/Stop & Restart on left, circular tools on right)
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;

                                        if is_running {
                                            let btn_stop = egui::Button::new(
                                                RichText::new(format!("{} Stop", crate::lucide::STOP))
                                                    .size(10.5)
                                                    .strong()
                                                    .color(Color32::from_rgb(0xfe, 0xf2, 0xf2)),
                                            )
                                            .fill(Color32::from_rgb(0x99, 0x1b, 0x1b))
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(999.0))
                                            .min_size(egui::vec2(56.0, 24.0));
                                            if ui.add(btn_stop).on_hover_text("Stop this server").clicked() {
                                                to_stop = Some(idx);
                                            }

                                            let btn_restart = egui::Button::new(
                                                RichText::new(format!("{} Restart", crate::lucide::REFRESH_CW))
                                                    .size(10.5)
                                                    .strong()
                                                    .color(Color32::from_rgb(0xec, 0xfe, 0xff)),
                                            )
                                            .fill(Color32::from_rgb(0x0e, 0x74, 0x90))
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(999.0))
                                            .min_size(egui::vec2(66.0, 24.0));
                                            if ui.add(btn_restart).on_hover_text("Restart this server").clicked() {
                                                to_restart = Some(idx);
                                            }
                                        } else {
                                            let btn_start = egui::Button::new(
                                                RichText::new(format!("{} Start", crate::lucide::PLAY))
                                                    .size(10.5)
                                                    .strong()
                                                    .color(Color32::WHITE),
                                            )
                                            .fill(Color32::from_rgb(0x05, 0x96, 0x69))
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(999.0))
                                            .min_size(egui::vec2(58.0, 24.0));
                                            if ui.add(btn_start).on_hover_text("Start this server").clicked() {
                                                to_start = Some(idx);
                                            }
                                        }

                                        // Right side circular action buttons
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.spacing_mut().item_spacing.x = 3.0;

                                            let circ_bg = Color32::from_rgb(0x27, 0x29, 0x36);
                                            let circ_size = egui::vec2(24.0, 24.0);
                                            let circ_rnd = Rounding::same(999.0);

                                            let btn_del = egui::Button::new(
                                                RichText::new(crate::lucide::TRASH).size(10.0).color(Color32::from_rgb(0xf8, 0x71, 0x71)),
                                            )
                                            .fill(circ_bg)
                                            .stroke(Stroke::NONE)
                                            .rounding(circ_rnd)
                                            .min_size(circ_size);
                                            if ui.add(btn_del).on_hover_text("Delete server").clicked() {
                                                to_delete = Some(key.clone());
                                            }

                                            let btn_folder = egui::Button::new(
                                                RichText::new(crate::lucide::FOLDER).size(10.0).color(Color32::from_rgb(0x94, 0xa3, 0xb8)),
                                            )
                                            .fill(circ_bg)
                                            .stroke(Stroke::NONE)
                                            .rounding(circ_rnd)
                                            .min_size(circ_size);
                                            if ui.add(btn_folder).on_hover_text("Open working directory in file manager").clicked() {
                                                crate::service::open_folder(&s.config.cwd);
                                            }

                                            let btn_vscode = egui::Button::new(
                                                RichText::new(crate::lucide::CODE).size(10.0).color(Color32::from_rgb(0x60, 0xa5, 0xfa)),
                                            )
                                            .fill(circ_bg)
                                            .stroke(Stroke::NONE)
                                            .rounding(circ_rnd)
                                            .min_size(circ_size);
                                            if ui.add(btn_vscode).on_hover_text("Open working directory in VS Code (code .)").clicked() {
                                                crate::service::open_in_vscode(&s.config.cwd);
                                            }

                                            let btn_edit = egui::Button::new(
                                                RichText::new(crate::lucide::PENCIL).size(10.0).color(Color32::from_rgb(0x94, 0xa3, 0xb8)),
                                            )
                                            .fill(circ_bg)
                                            .stroke(Stroke::NONE)
                                            .rounding(circ_rnd)
                                            .min_size(circ_size);
                                            if ui.add(btn_edit).on_hover_text("Edit server configuration").clicked() {
                                                to_edit = Some(s.config.clone());
                                            }

                                            let btn_console = egui::Button::new(
                                                RichText::new(crate::lucide::TERMINAL).size(10.0).color(Color32::from_rgb(0x94, 0xa3, 0xb8)),
                                            )
                                            .fill(circ_bg)
                                            .stroke(Stroke::NONE)
                                            .rounding(circ_rnd)
                                            .min_size(circ_size);
                                            if ui.add(btn_console).on_hover_text("Open in console / terminal window").clicked() {
                                                s.open_console();
                                            }

                                            if (is_running && s.config.port > 0) || s.config.url.is_some() {
                                                let target_url = if let Some(u) = &s.config.url {
                                                    u.clone()
                                                } else {
                                                    format!("http://localhost:{}", s.config.port)
                                                };
                                                let btn_browser = egui::Button::new(
                                                    RichText::new(crate::lucide::EXTERNAL_LINK).size(10.5).strong().color(Color32::from_rgb(0x38, 0xbd, 0xf8)),
                                                )
                                                .fill(circ_bg)
                                                .stroke(Stroke::NONE)
                                                .rounding(circ_rnd)
                                                .min_size(circ_size);
                                                if ui.add(btn_browser).on_hover_text(format!("Open {} in browser", target_url)).clicked() {
                                                    ui.ctx().open_url(egui::OpenUrl::new_tab(target_url));
                                                }
                                            }
                                        });
                                    });

                                    ui.add_space(8.0);

                                    // Row 5: Checkboxes (autostart on left, auto-restart on right)
                                    ui.horizontal(|ui| {
                                        let cb_color = Color32::from_rgb(0x94, 0xa3, 0xb8);
                                        if ui.checkbox(
                                            &mut s.config.autostart,
                                            RichText::new("autostart").size(11.0).color(cb_color),
                                        ).on_hover_text("Automatically start this server when Server Launcher opens").changed() {
                                            to_save_config = true;
                                        }

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.checkbox(
                                                &mut s.config.auto_restart,
                                                RichText::new("auto-restart").size(11.0).color(cb_color),
                                            ).on_hover_text("Auto-restart this server if it exits unexpectedly or crashes").changed() {
                                                to_save_config = true;
                                            }
                                        });
                                    });
                                });

                            ui.add_space(6.0);
                        }
                    }

                    if to_save_config {
                        self.persist_config();
                    }

                    if let Some(g) = start_grp { self.start_group(&g); }
                    if let Some(g) = stop_grp { self.stop_group(&g); }
                    if let Some(idx) = to_start { self.services[idx].start(); }
                    if let Some(idx) = to_stop { self.services[idx].stop(); }
                    if let Some(idx) = to_restart { self.services[idx].restart(); }
                    if let Some(k) = to_switch_tab { self.active_tab = k; }
                    if let Some(cfg) = to_edit { self.add_edit_modal.open_edit(&cfg); }
                    if let Some(k) = to_delete {
                        if let Some(idx) = self.services.iter().position(|s| s.config.key == k) {
                            let mut s = self.services.remove(idx);
                            s.stop();
                            self.persist_config();
                            if self.active_tab == k { self.active_tab = "overview".to_string(); }
                        }
                    }
                });
        });
    }

    // ═════════════════════════════════════════════════════════════════════════
    // RIGHT PANEL: Notebook Tabs Bar + Overview / Server Log View
    // ═════════════════════════════════════════════════════════════════════════
    fn render_servers_right_panel(&mut self, ui: &mut Ui) {
        let p = self.theme.palette();
        ui.vertical(|ui| {
            // Notebook Tabs Strip (Material 3 Dark Cockpit)
            let online_count = self.services.iter().filter(|s| s.state == ServiceState::Running).count();
            let total_count = self.services.len();

            let mut to_switch_tab: Option<String> = None;

            ScrollArea::horizontal()
                .id_salt("notebook_tabs_scroll")
                .auto_shrink([false, true])
                .max_height(38.0)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
                    ui.horizontal(|ui| {
                        // Tab 0: Server Overview
                        let is_ov_active = self.active_tab == "overview";
                        let ov_bg = if is_ov_active {
                            p.bg_card
                        } else {
                            p.bg_sidebar.gamma_multiply(0.65)
                        };
                        let ov_stroke = if is_ov_active {
                            Stroke::new(1.5_f32, p.accent)
                        } else {
                            Stroke::new(1.0_f32, p.border.gamma_multiply(0.4))
                        };

                        let ov_frame = Frame::none()
                            .fill(ov_bg)
                            .stroke(ov_stroke)
                            .rounding(Rounding { nw: 6.0, ne: 6.0, sw: 0.0, se: 0.0 })
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0));

                        let ov_resp = ov_frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
                                ui.label(
                                    RichText::new(format!("{} Server Overview", crate::lucide::SERVER))
                                        .size(12.0)
                                        .strong()
                                        .color(if is_ov_active { p.text_primary } else { p.text_secondary }),
                                );

                                // Chip: online count
                                let chip_bg = if is_ov_active {
                                    p.accent.gamma_multiply(0.2)
                                } else {
                                    p.bg_card.gamma_multiply(0.5)
                                };
                                let chip_color = if online_count > 0 {
                                    Color32::from_rgb(52, 211, 153)
                                } else {
                                    p.text_muted
                                };
                                Frame::none()
                                    .fill(chip_bg)
                                    .rounding(Rounding::same(999.0))
                                    .inner_margin(egui::Margin::symmetric(6.0, 1.5))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(format!("{}/{}", online_count, total_count))
                                                .size(10.0)
                                                .monospace()
                                                .color(chip_color),
                                        );
                                    });
                            });
                        }).response;

                        let ov_interact = ui.interact(ov_resp.rect, ui.id().with("tab_overview"), egui::Sense::click());
                        if ov_interact.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if ov_interact.clicked() {
                            to_switch_tab = Some("overview".to_string());
                        }

                        // Server tabs
                        for s in &self.services {
                            let is_active = self.active_tab == s.config.key;
                            let (dot_char, dot_color) = match s.state {
                                ServiceState::Running => ("●", Color32::from_rgb(52, 211, 153)),
                                ServiceState::Starting => ("●", Color32::from_rgb(251, 191, 36)),
                                ServiceState::Stopping => ("●", Color32::from_rgb(249, 115, 22)),
                                ServiceState::Errored => ("●", Color32::from_rgb(239, 68, 68)),
                                ServiceState::Stopped => ("○", Color32::from_rgb(148, 163, 184)),
                            };

                            let bg = if is_active {
                                p.bg_card
                            } else {
                                p.bg_sidebar.gamma_multiply(0.65)
                            };
                            let stroke = if is_active {
                                Stroke::new(1.5_f32, p.accent)
                            } else {
                                Stroke::new(1.0_f32, p.border.gamma_multiply(0.4))
                            };

                            let tab_frame = Frame::none()
                                .fill(bg)
                                .stroke(stroke)
                                .rounding(Rounding { nw: 6.0, ne: 6.0, sw: 0.0, se: 0.0 })
                                .inner_margin(egui::Margin::symmetric(10.0, 6.0));

                            let tab_resp = tab_frame.show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(5.0, 0.0);
                                    ui.label(RichText::new(dot_char).size(11.0).color(dot_color));
                                    ui.label(
                                        RichText::new(&s.config.name)
                                            .size(12.0)
                                            .strong()
                                            .color(if is_active { p.text_primary } else { p.text_secondary }),
                                    );

                                    if s.config.port > 0 {
                                        let port_color = if s.state == ServiceState::Running {
                                            Color32::from_rgb(96, 165, 250)
                                        } else {
                                            p.text_muted
                                        };
                                        ui.label(
                                            RichText::new(format!(":{}", s.config.port))
                                                .size(10.5)
                                                .monospace()
                                                .color(port_color),
                                        );
                                    }
                                });
                            }).response;

                            let tab_interact = ui.interact(tab_resp.rect, ui.id().with(&s.config.key), egui::Sense::click());
                            if tab_interact.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if tab_interact.clicked() {
                                to_switch_tab = Some(s.config.key.clone());
                            }
                        }
                    });
                });

            if let Some(target) = to_switch_tab {
                self.active_tab = target;
            }

            // Outer Frame for the tab content (FILLS 100% OF REMAINING SPACE!)
            Frame::none()
                .fill(p.bg_main)
                .stroke(Stroke::NONE)
                .rounding(Rounding { nw: 0.0, ne: 6.0, sw: 6.0, se: 6.0 })
                .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                .show(ui, |ui| {
                    ui.set_height(ui.available_height());
                    if self.active_tab == "overview" {
                        self.render_overview_content(ui);
                    } else {
                        let key = self.active_tab.clone();
                        self.render_server_log_content(ui, &key);
                    }
                });
        });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Right Panel Tab 0: "Server Overview" (Fills available height)
    // ─────────────────────────────────────────────────────────────────────────
    fn render_overview_content(&mut self, ui: &mut Ui) {
        let p = self.theme.palette();
        // Heading Row: Servers running on this PC | last scan: HH:MM:SS | Search Box | Refresh button
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Servers running on this PC")
                    .size(16.0)
                    .strong()
                    .color(p.text_primary),
            );
            ui.add_space(8.0);
            let is_scanning = self.scanner.is_scanning.lock().map(|s| *s).unwrap_or(false);
            if is_scanning {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("refreshing…")
                            .size(11.5)
                            .color(p.accent),
                    );
                });
            } else {
                let scan_time = self.scanner.get_last_scan_time();
                ui.label(
                    RichText::new(format!("last scan: {}", scan_time))
                        .size(11.5)
                        .color(p.text_muted),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn_refresh = egui::Button::new(
                    RichText::new(format!("{} Refresh", crate::lucide::REFRESH_CW))
                        .size(11.5)
                        .strong()
                        .color(p.accent),
                )
                .fill(p.bg_button)
                .stroke(Stroke::NONE)
                .rounding(Rounding::same(999.0))
                .min_size(egui::vec2(86.0, 26.0));
                if ui.add(btn_refresh).clicked() {
                    self.refresh_server_list();
                }

                // Search Box with search icon, hint, clear button, and Ctrl+F focus support
                Frame::none()
                    .fill(p.bg_input)
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(999.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(crate::lucide::SEARCH)
                                    .size(11.5)
                                    .color(p.text_muted),
                            );
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.overview_search_query)
                                    .hint_text(
                                        RichText::new("Search servers, ports, commands... (Ctrl+F)")
                                            .size(11.5)
                                            .color(p.text_muted),
                                    )
                                    .desired_width(240.0)
                                    .frame(false),
                            );
                            if self.focus_overview_search {
                                resp.request_focus();
                                self.focus_overview_search = false;
                                self.refresh_server_list();
                            }
                            if resp.changed() {
                                self.refresh_server_list();
                            }
                            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                                self.refresh_server_list();
                            }
                            if !self.overview_search_query.is_empty() {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(crate::lucide::X)
                                                .size(10.5)
                                                .color(p.text_muted),
                                        )
                                        .frame(false),
                                    )
                                    .clicked()
                                {
                                    self.overview_search_query.clear();
                                    self.refresh_server_list();
                                }
                            }
                        });
                    });
            });
        });

        ui.add_space(6.0);

        let q = self.overview_search_query.trim().to_lowercase();

        ScrollArea::vertical()
            .id_salt("overview_scroll_body")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Section 1: Launcher-managed servers
                let managed_indices: Vec<usize> = self
                    .services
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| {
                        if q.is_empty() {
                            return true;
                        }
                        s.config.name.to_lowercase().contains(&q)
                            || s.config.command.to_lowercase().contains(&q)
                            || s.config.group.to_lowercase().contains(&q)
                            || (s.config.port > 0 && s.config.port.to_string().contains(&q))
                            || s.pid.map(|p| p.to_string().contains(&q)).unwrap_or(false)
                    })
                    .map(|(i, _)| i)
                    .collect();

                let managed_title = if q.is_empty() {
                    "Launcher-managed servers".to_string()
                } else {
                    format!("Launcher-managed servers ({})", managed_indices.len())
                };

                ui.label(
                    RichText::new(managed_title)
                        .size(13.5)
                        .strong()
                        .color(p.text_primary),
                );
                ui.add_space(8.0);

                if managed_indices.is_empty() && !q.is_empty() {
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("No launcher-managed servers matching \"{}\"", q))
                                    .size(12.0)
                                    .color(p.text_muted),
                            );
                        });
                    ui.add_space(6.0);
                }

                let mut to_toggle: Option<(usize, u8)> = None; // 0=stop, 1=start, 2=restart
                let mut to_edit: Option<crate::config::ServerConfig> = None;
                let mut to_delete: Option<String> = None;
                let mut to_toggle_autostart: Option<(usize, bool)> = None;
                let mut to_toggle_autorestart: Option<(usize, bool)> = None;

                for &idx in &managed_indices {
                    let s = &self.services[idx];
                    let key = s.config.key.clone();
                    let is_running = s.state == ServiceState::Running || s.state == ServiceState::Starting;
                    let is_selected_tab = self.active_tab == key;
                    let card_bg = if is_selected_tab {
                        Color32::from_rgb(0x22, 0x24, 0x30)
                    } else {
                        Color32::from_rgb(0x18, 0x19, 0x22)
                    };

                    let card_height = if is_running { 150.0 } else { 120.0 };
                    let current_pos = ui.cursor().min;
                    let approx_rect = egui::Rect::from_min_size(current_pos, egui::vec2(ui.available_width(), card_height));
                    if !ui.clip_rect().expand(120.0).intersects(approx_rect) {
                        ui.allocate_space(egui::vec2(ui.available_width(), card_height));
                        ui.add_space(6.0);
                        continue;
                    }

                    Frame::none()
                        .fill(card_bg)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(18.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());

                            // Row 1: Header (Glowing dot + Server Title on Left, Status chip badge on Right)
                            let (dot_col, halo_col, chip_bg, chip_text, status_label) = if s.restart_at.is_some() {
                                (
                                    Color32::from_rgb(0xfc, 0xd3, 0x4d),
                                    Color32::from_rgb(0xfc, 0xd3, 0x4d).gamma_multiply(0.25),
                                    Color32::from_rgb(0x35, 0x22, 0x05),
                                    Color32::from_rgb(0xfc, 0xd3, 0x4d),
                                    "Restarting",
                                )
                            } else if is_running {
                                (
                                    Color32::from_rgb(0x5e, 0xea, 0xd4),
                                    Color32::from_rgb(0x5e, 0xea, 0xd4).gamma_multiply(0.28),
                                    Color32::from_rgb(0x13, 0x27, 0x22),
                                    Color32::from_rgb(0x5e, 0xea, 0xd4),
                                    "Running",
                                )
                            } else if s.state == ServiceState::Errored {
                                (
                                    Color32::from_rgb(0xf8, 0x71, 0x71),
                                    Color32::from_rgb(0xf8, 0x71, 0x71).gamma_multiply(0.25),
                                    Color32::from_rgb(0x38, 0x14, 0x18),
                                    Color32::from_rgb(0xf8, 0x71, 0x71),
                                    "Errored",
                                )
                            } else {
                                (
                                    Color32::from_rgb(0x64, 0x74, 0x8b),
                                    Color32::from_rgb(0x64, 0x74, 0x8b).gamma_multiply(0.20),
                                    Color32::from_rgb(0x20, 0x23, 0x2d),
                                    Color32::from_rgb(0x94, 0xa3, 0xb8),
                                    "Stopped",
                                )
                            };

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;

                                // Glowing dot
                                let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                                ui.painter().circle_filled(dot_rect.center(), 7.0, halo_col);
                                ui.painter().circle_filled(dot_rect.center(), 4.0, dot_col);

                                // Title
                                ui.label(
                                    RichText::new(&s.config.name)
                                        .strong()
                                        .size(14.0)
                                        .color(Color32::from_rgb(0xf1, 0xf5, 0xf9)),
                                );

                                // Status Chip (pill badge on the right, zero border)
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    Frame::none()
                                        .fill(chip_bg)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(10.0, 3.5))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(status_label)
                                                    .size(11.0)
                                                    .strong()
                                                    .color(chip_text),
                                            );
                                        });
                                });
                            });

                            ui.add_space(8.0);

                            // Row 2: Recessed Command Box (full width, zero border)
                            Frame::none()
                                .fill(Color32::from_rgb(0x10, 0x11, 0x17))
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(8.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(&s.config.command)
                                                .size(11.0)
                                                .monospace()
                                                .color(Color32::from_rgb(0x38, 0xbd, 0xf8)),
                                        );
                                        if s.config.port > 0 {
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.label(
                                                    RichText::new(format!(":{}", s.config.port))
                                                        .size(10.5)
                                                        .monospace()
                                                        .color(Color32::from_rgb(0x64, 0x74, 0x8b)),
                                                );
                                            });
                                        }
                                    });
                                });

                            ui.add_space(7.0);

                            // Row 3: Micro Telemetry Pills (PID, CPU%, RAM MB, Uptime)
                            let pill_bg = Color32::from_rgb(0x23, 0x25, 0x30);
                            let pill_rounding = Rounding::same(6.0);
                            let pill_margin = egui::Margin::symmetric(8.0, 4.0);

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;

                                if is_running {
                                    if let Some(pid) = s.pid {
                                        Frame::none()
                                            .fill(pill_bg)
                                            .stroke(Stroke::NONE)
                                            .rounding(pill_rounding)
                                            .inner_margin(pill_margin)
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 4.0;
                                                    ui.label(RichText::new(crate::lucide::HASH).size(10.0));
                                                    ui.label(
                                                        RichText::new(format!("{}", pid))
                                                            .size(11.0)
                                                            .monospace()
                                                            .color(Color32::from_rgb(0x94, 0xa3, 0xb8)),
                                                    );
                                                });
                                            });
                                    }

                                    Frame::none()
                                        .fill(pill_bg)
                                        .stroke(Stroke::NONE)
                                        .rounding(pill_rounding)
                                        .inner_margin(pill_margin)
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 4.0;
                                                ui.label(RichText::new(crate::lucide::CPU).size(10.5).color(Color32::from_rgb(0x38, 0xbd, 0xf8)));
                                                ui.label(
                                                    RichText::new(format!("{:.1}%", s.cpu_usage))
                                                        .size(11.0)
                                                        .monospace()
                                                        .strong()
                                                        .color(Color32::from_rgb(0x38, 0xbd, 0xf8)),
                                                );
                                            });
                                        });

                                    Frame::none()
                                        .fill(pill_bg)
                                        .stroke(Stroke::NONE)
                                        .rounding(pill_rounding)
                                        .inner_margin(pill_margin)
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 4.0;
                                                ui.label(RichText::new(crate::lucide::RAM).size(10.0).color(Color32::from_rgb(0x4a, 0xde, 0x80)));
                                                ui.label(
                                                    RichText::new(format!("{:.1} MB", s.memory_mb))
                                                        .size(11.0)
                                                        .monospace()
                                                        .strong()
                                                        .color(Color32::from_rgb(0x4a, 0xde, 0x80)),
                                                );
                                            });
                                        });

                                    let uptime = s.uptime_formatted();
                                    let uptime_str = if uptime.is_empty() { "0s".to_string() } else { uptime };
                                    Frame::none()
                                        .fill(pill_bg)
                                        .stroke(Stroke::NONE)
                                        .rounding(pill_rounding)
                                        .inner_margin(pill_margin)
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 4.0;
                                                ui.label(RichText::new(crate::lucide::CLOCK).size(10.5).color(Color32::from_rgb(0x94, 0xa3, 0xb8)));
                                                ui.label(
                                                    RichText::new(uptime_str)
                                                        .size(11.0)
                                                        .monospace()
                                                        .color(Color32::from_rgb(0x94, 0xa3, 0xb8)),
                                                );
                                            });
                                        });
                                } else {
                                    Frame::none()
                                        .fill(pill_bg)
                                        .stroke(Stroke::NONE)
                                        .rounding(pill_rounding)
                                        .inner_margin(pill_margin)
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 4.0;
                                                ui.label(RichText::new(crate::lucide::STOP).size(9.0).color(Color32::from_rgb(0x94, 0xa3, 0xb8)));
                                                ui.label(RichText::new("Idle").size(11.0).monospace().color(Color32::from_rgb(0x94, 0xa3, 0xb8)));
                                            });
                                        });

                                    if s.config.port > 0 {
                                        Frame::none()
                                            .fill(pill_bg)
                                            .stroke(Stroke::NONE)
                                            .rounding(pill_rounding)
                                            .inner_margin(pill_margin)
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 4.0;
                                                    ui.label(RichText::new(crate::lucide::NETWORK).size(10.0).color(Color32::from_rgb(0x70, 0xd5, 0xe4)));
                                                    ui.label(RichText::new(format!(":{}", s.config.port)).size(11.0).monospace().color(Color32::from_rgb(0x70, 0xd5, 0xe4)));
                                                });
                                            });
                                    }

                                    let folder_name = std::path::Path::new(&s.config.cwd)
                                        .file_name()
                                        .map(|n| n.to_string_lossy().to_string())
                                        .unwrap_or_else(|| "dir".to_string());
                                    Frame::none()
                                        .fill(pill_bg)
                                        .stroke(Stroke::NONE)
                                        .rounding(pill_rounding)
                                        .inner_margin(pill_margin)
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 4.0;
                                                ui.label(RichText::new(crate::lucide::FOLDER).size(10.0));
                                                ui.label(RichText::new(folder_name).size(10.5).monospace().color(Color32::from_rgb(0x94, 0xa3, 0xb8)));
                                            });
                                        });
                                }
                            });

                            ui.add_space(8.0);

                            // Row 4: Action Buttons (Start/Stop & Restart on left, circular tools on right)
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;

                                if is_running {
                                    let btn_stop = egui::Button::new(
                                        RichText::new(format!("{} Stop", crate::lucide::STOP))
                                            .size(10.5)
                                            .strong()
                                            .color(Color32::from_rgb(0xfe, 0xf2, 0xf2)),
                                    )
                                    .fill(Color32::from_rgb(0x99, 0x1b, 0x1b))
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(999.0))
                                    .min_size(egui::vec2(56.0, 24.0));
                                    if ui.add(btn_stop).on_hover_text("Stop this server").clicked() {
                                        to_toggle = Some((idx, 0));
                                    }

                                    let btn_restart = egui::Button::new(
                                        RichText::new(format!("{} Restart", crate::lucide::REFRESH_CW))
                                            .size(10.5)
                                            .strong()
                                            .color(Color32::from_rgb(0xec, 0xfe, 0xff)),
                                    )
                                    .fill(Color32::from_rgb(0x0e, 0x74, 0x90))
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(999.0))
                                    .min_size(egui::vec2(66.0, 24.0));
                                    if ui.add(btn_restart).on_hover_text("Restart this server").clicked() {
                                        to_toggle = Some((idx, 2));
                                    }
                                } else {
                                    let btn_start = egui::Button::new(
                                        RichText::new(format!("{} Start", crate::lucide::PLAY))
                                            .size(10.5)
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(Color32::from_rgb(0x05, 0x96, 0x69))
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(999.0))
                                    .min_size(egui::vec2(58.0, 24.0));
                                    if ui.add(btn_start).on_hover_text("Start this server").clicked() {
                                        to_toggle = Some((idx, 1));
                                    }
                                }

                                // Right side circular action buttons
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.spacing_mut().item_spacing.x = 3.0;

                                    let circ_bg = Color32::from_rgb(0x27, 0x29, 0x36);
                                    let circ_size = egui::vec2(24.0, 24.0);
                                    let circ_rnd = Rounding::same(999.0);

                                    let btn_del = egui::Button::new(
                                        RichText::new(crate::lucide::TRASH).size(10.0).color(Color32::from_rgb(0xf8, 0x71, 0x71)),
                                    )
                                    .fill(circ_bg)
                                    .stroke(Stroke::NONE)
                                    .rounding(circ_rnd)
                                    .min_size(circ_size);
                                    if ui.add(btn_del).on_hover_text("Delete server").clicked() {
                                        to_delete = Some(key.clone());
                                    }

                                    let btn_folder = egui::Button::new(
                                        RichText::new(crate::lucide::FOLDER).size(10.0).color(Color32::from_rgb(0x94, 0xa3, 0xb8)),
                                    )
                                    .fill(circ_bg)
                                    .stroke(Stroke::NONE)
                                    .rounding(circ_rnd)
                                    .min_size(circ_size);
                                    if ui.add(btn_folder).on_hover_text("Open working directory in file manager").clicked() {
                                        crate::service::open_folder(&s.config.cwd);
                                    }

                                    let btn_vscode = egui::Button::new(
                                        RichText::new(crate::lucide::CODE).size(10.0).color(Color32::from_rgb(0x60, 0xa5, 0xfa)),
                                    )
                                    .fill(circ_bg)
                                    .stroke(Stroke::NONE)
                                    .rounding(circ_rnd)
                                    .min_size(circ_size);
                                    if ui.add(btn_vscode).on_hover_text("Open working directory in VS Code (code .)").clicked() {
                                        crate::service::open_in_vscode(&s.config.cwd);
                                    }

                                    let btn_edit = egui::Button::new(
                                        RichText::new(crate::lucide::PENCIL).size(10.0).color(Color32::from_rgb(0x94, 0xa3, 0xb8)),
                                    )
                                    .fill(circ_bg)
                                    .stroke(Stroke::NONE)
                                    .rounding(circ_rnd)
                                    .min_size(circ_size);
                                    if ui.add(btn_edit).on_hover_text("Edit server configuration").clicked() {
                                        to_edit = Some(s.config.clone());
                                    }

                                    let btn_console = egui::Button::new(
                                        RichText::new(crate::lucide::TERMINAL).size(10.0).color(Color32::from_rgb(0x94, 0xa3, 0xb8)),
                                    )
                                    .fill(circ_bg)
                                    .stroke(Stroke::NONE)
                                    .rounding(circ_rnd)
                                    .min_size(circ_size);
                                    if ui.add(btn_console).on_hover_text("Open in console / terminal window").clicked() {
                                        s.open_console();
                                    }

                                    if (is_running && s.config.port > 0) || s.config.url.is_some() {
                                        let target_url = if let Some(u) = &s.config.url {
                                             u.clone()
                                        } else {
                                            format!("http://localhost:{}", s.config.port)
                                        };
                                        let btn_browser = egui::Button::new(
                                            RichText::new(crate::lucide::EXTERNAL_LINK).size(10.5).strong().color(Color32::from_rgb(0x38, 0xbd, 0xf8)),
                                        )
                                        .fill(circ_bg)
                                        .stroke(Stroke::NONE)
                                        .rounding(circ_rnd)
                                        .min_size(circ_size);
                                        if ui.add(btn_browser).on_hover_text(format!("Open {} in browser", target_url)).clicked() {
                                            ui.ctx().open_url(egui::OpenUrl::new_tab(target_url));
                                        }
                                    }
                                });
                            });

                            ui.add_space(8.0);

                            // Row 5: Checkboxes (autostart on left, auto-restart on right)
                            ui.horizontal(|ui| {
                                let cb_color = Color32::from_rgb(0x94, 0xa3, 0xb8);
                                let mut autostart_val = s.config.autostart;
                                if ui.checkbox(
                                    &mut autostart_val,
                                    RichText::new("autostart").size(11.0).color(cb_color),
                                ).on_hover_text("Automatically start this server when Server Launcher opens").changed() {
                                    to_toggle_autostart = Some((idx, autostart_val));
                                }

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let mut autorestart_val = s.config.auto_restart;
                                    if ui.checkbox(
                                        &mut autorestart_val,
                                        RichText::new("auto-restart").size(11.0).color(cb_color),
                                    ).on_hover_text("Auto-restart this server if it exits unexpectedly or crashes").changed() {
                                        to_toggle_autorestart = Some((idx, autorestart_val));
                                    }
                                });
                            });
                        });

                    ui.add_space(6.0);
                }

                if let Some((idx, op)) = to_toggle {
                    match op {
                        0 => self.services[idx].stop(),
                        1 => self.services[idx].start(),
                        2 => self.services[idx].restart(),
                        _ => {}
                    }
                }
                if let Some(cfg) = to_edit {
                    self.add_edit_modal.open_edit(&cfg);
                }
                if let Some(k) = to_delete {
                    if let Some(idx) = self.services.iter().position(|s| s.config.key == k) {
                        let mut s = self.services.remove(idx);
                        s.stop();
                        self.persist_config();
                        if self.active_tab == k { self.active_tab = "overview".to_string(); }
                    }
                }
                if let Some((idx, val)) = to_toggle_autostart {
                    self.services[idx].config.autostart = val;
                    self.persist_config();
                }
                if let Some((idx, val)) = to_toggle_autorestart {
                    self.services[idx].config.auto_restart = val;
                    self.persist_config();
                }

                ui.add_space(16.0);

                // Section 2: Other processes listening on this PC
                let listeners = self.scanner.get_listeners();
                let managed_ports: BTreeSet<u16> = self
                    .services
                    .iter()
                    .map(|s| s.config.port)
                    .filter(|&p| p > 0)
                    .collect();

                let mut to_kill_pid: Option<(Option<u32>, u16, String, String, String)> = None;
                let mut to_restart_pid: Option<(Option<u32>, u16, String, String)> = None;
                let mut to_start_stopped: Option<(u16, String, String)> = None;
                let mut to_remove_stopped: Option<u16> = None;
                let mut to_launch_external_terminal: Option<(String, Option<u32>, u16, String, String)> = None;

                let unmanaged_raw: Vec<_> = listeners
                    .iter()
                    .filter(|l| !managed_ports.contains(&l.port))
                    .cloned()
                    .collect();

                let unmanaged_listeners: Vec<_> = if q.is_empty() {
                    unmanaged_raw
                } else {
                    unmanaged_raw
                        .into_iter()
                        .filter(|l| {
                            if l.name.to_lowercase().contains(&q)
                                || l.port.to_string().contains(&q)
                                || l.proto.to_lowercase().contains(&q)
                                || l.pid.map(|p| p.to_string().contains(&q)).unwrap_or(false)
                            {
                                return true;
                            }
                            if let Some(pid) = l.pid {
                                let (cmd, _) = self.get_proc_cmd_and_cwd(pid);
                                if let Some(c) = cmd {
                                    if c.to_lowercase().contains(&q) {
                                        return true;
                                    }
                                }
                            }
                            false
                        })
                        .collect()
                };

                let running_ports: BTreeSet<u16> = listeners.iter().map(|l| l.port).collect();

                let visible_stopped: Vec<_> = self
                    .stopped_external
                    .iter()
                    .filter(|(&port, (name, cmd, _))| {
                        if running_ports.contains(&port) || managed_ports.contains(&port) {
                            return false;
                        }
                        if q.is_empty() {
                            return true;
                        }
                        name.to_lowercase().contains(&q)
                            || port.to_string().contains(&q)
                            || cmd.to_lowercase().contains(&q)
                    })
                    .map(|(&port, (name, cmd, cwd))| (port, name.clone(), cmd.clone(), cwd.clone()))
                    .collect();

                let other_title = if q.is_empty() {
                    "Other processes listening on this PC".to_string()
                } else {
                    format!(
                        "Other processes listening on this PC ({})",
                        unmanaged_listeners.len() + visible_stopped.len()
                    )
                };

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(other_title)
                            .size(13.5)
                            .strong()
                            .color(p.text_primary),
                    );
                    ui.add_space(8.0);
                    let is_metrics_on = self.show_other_process_metrics;
                    ui.checkbox(
                        &mut self.show_other_process_metrics,
                        RichText::new("Show CPU & RAM usage")
                            .size(11.5)
                            .color(if is_metrics_on {
                                p.accent
                            } else {
                                p.text_muted
                            }),
                    );
                });
                ui.add_space(8.0);

                if unmanaged_listeners.is_empty() && visible_stopped.is_empty() {
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                        .show(ui, |ui| {
                            let msg = if q.is_empty() {
                                "No unmanaged listening processes detected.".to_string()
                            } else {
                                format!("No other listening processes matching \"{}\"", q)
                            };
                            ui.label(
                                RichText::new(msg)
                                    .size(12.5)
                                    .color(p.text_muted),
                            );
                        });
                } else {
                    for listener in &unmanaged_listeners {
                        let is_expanded = self.expanded_listeners.contains(&listener.port);
                        let card_height = if is_expanded {
                            170.0
                        } else if self.show_other_process_metrics {
                            76.0
                        } else {
                            50.0
                        };
                        let current_pos = ui.cursor().min;
                        let approx_rect = egui::Rect::from_min_size(current_pos, egui::vec2(ui.available_width(), card_height));
                        if !ui.clip_rect().expand(120.0).intersects(approx_rect) {
                            ui.allocate_space(egui::vec2(ui.available_width(), card_height));
                            ui.add_space(6.0);
                            continue;
                        }

                        let (cmd_opt, cwd_opt) = listener.pid
                            .map(|p| self.get_proc_cmd_and_cwd(p))
                            .unwrap_or((None, None));
                        let cmd_str = cmd_opt.unwrap_or_default();
                        let cwd_str = cwd_opt.unwrap_or_default();

                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    let is_expanded = self.expanded_listeners.contains(&listener.port);
                                    ui.horizontal(|ui| {
                                        draw_status_dot(ui, p.success);

                                        let toggle_label = if is_expanded { "v" } else { ">" };
                                        let btn_toggle = egui::Button::new(
                                            RichText::new(toggle_label)
                                                .size(11.0)
                                                .strong()
                                                .color(if is_expanded { p.accent } else { p.text_muted }),
                                        )
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(4.0))
                                        .min_size(egui::vec2(22.0, 20.0));
                                        if ui.add(btn_toggle).on_hover_text(if is_expanded { "Collapse server location & command" } else { "View server location in expanded view" }).clicked() {
                                            if is_expanded {
                                                self.expanded_listeners.remove(&listener.port);
                                            } else {
                                                self.expanded_listeners.insert(listener.port);
                                            }
                                        }

                                        ui.label(
                                            RichText::new(&listener.name)
                                                .size(13.5)
                                                .strong()
                                                .color(p.text_primary),
                                        );
                                        ui.label(
                                            RichText::new(format!("({})", listener.proto))
                                                .size(11.0)
                                                .color(p.text_muted),
                                        );

                                        ui.add_space(4.0);
                                        if let Some(pid) = listener.pid {
                                            render_metric_pill(
                                                ui,
                                                crate::lucide::HASH,
                                                &format!("PID {}", pid),
                                                p.text_muted,
                                            );
                                        }
                                        render_metric_pill(
                                            ui,
                                            crate::lucide::PLUG,
                                            &format!(":{}", listener.port),
                                            p.info,
                                        );
                                        render_metric_pill(
                                            ui,
                                            "●",
                                            "Running",
                                            p.success,
                                        );

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            // Stop button
                                            let btn_stop = egui::Button::new(
                                                RichText::new("Stop").size(11.5).strong().color(Color32::WHITE),
                                            )
                                            .fill(p.danger)
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(50.0, 24.0));
                                            if ui.add(btn_stop).clicked() {
                                                to_kill_pid = Some((
                                                    listener.pid,
                                                    listener.port,
                                                    listener.name.clone(),
                                                    cmd_str.clone(),
                                                    cwd_str.clone(),
                                                ));
                                            }

                                            // Restart button
                                            let btn_restart = egui::Button::new(
                                                RichText::new("Restart").size(11.5).strong().color(Color32::WHITE),
                                            )
                                            .fill(p.warning)
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(58.0, 24.0));
                                            if ui.add(btn_restart).clicked() {
                                                to_restart_pid = Some((
                                                    listener.pid,
                                                    listener.port,
                                                    cmd_str.clone(),
                                                    cwd_str.clone(),
                                                ));
                                            }
                                        });
                                    });

                                    // Row 2: CPU & RAM metrics when enabled, rendered on the line below
                                    if self.show_other_process_metrics {
                                        if let Some(pid) = listener.pid {
                                            let (cpu, mem) = self.other_metrics.get(&pid).copied().unwrap_or((0.0, 0.0));
                                            ui.add_space(4.0);
                                            ui.horizontal(|ui| {
                                                ui.add_space(36.0);
                                                render_metric_pill(
                                                    ui,
                                                    crate::lucide::CPU,
                                                    &format!("{:.1}%", cpu),
                                                    p.info,
                                                );
                                                render_metric_pill(
                                                    ui,
                                                    crate::lucide::RAM,
                                                    &format!("{:.1} MB", mem),
                                                    p.success,
                                                );
                                            });
                                        }
                                    }

                                    // Expandable location & command details
                                    if is_expanded {
                                        ui.add_space(8.0);
                                        Frame::none()
                                            .fill(p.bg_main)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(8.0))
                                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                            .show(ui, |ui| {
                                                ui.vertical(|ui| {
                                                    if !cwd_str.is_empty() {
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new(format!("{} Location:", crate::lucide::FOLDER)).size(11.0).strong().color(p.accent));
                                                            ui.label(
                                                                RichText::new(&cwd_str)
                                                                    .size(10.5)
                                                                    .monospace()
                                                                    .color(p.text_primary),
                                                            );
                                                        });
                                                    }
                                                    if !cmd_str.is_empty() {
                                                        if !cwd_str.is_empty() {
                                                            ui.add_space(4.0);
                                                        }
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new(format!("{} Command:", crate::lucide::SETTINGS)).size(11.0).strong().color(p.accent));
                                                            ui.label(
                                                                RichText::new(&cmd_str)
                                                                    .size(10.5)
                                                                    .monospace()
                                                                    .color(p.text_secondary),
                                                            );
                                                        });
                                                    }
                                                    if cwd_str.is_empty() && cmd_str.is_empty() {
                                                        ui.label(
                                                            RichText::new("Location or command line details not accessible for this system process.")
                                                                .size(10.5)
                                                                .italics()
                                                                .color(p.text_muted),
                                                        );
                                                    }

                                                    ui.add_space(8.0);
                                                    ui.horizontal(|ui| {
                                                        let is_inline_open = self.open_inline_terminals.contains(&listener.port);
                                                        let inline_btn_text = if is_inline_open {
                                                             format!("{} Hide Inline Terminal", crate::lucide::TERMINAL)
                                                        } else {
                                                             format!("{} View Output (Inline Terminal)", crate::lucide::TERMINAL)
                                                        };
                                                        let btn_inline = egui::Button::new(
                                                            RichText::new(inline_btn_text)
                                                                .size(11.0)
                                                                .strong()
                                                                .color(if is_inline_open { p.warning } else { p.accent }),
                                                        )
                                                        .fill(if is_inline_open { p.bg_card } else { p.bg_button })
                                                        .stroke(Stroke::NONE)
                                                        .rounding(Rounding::same(999.0));

                                                        if ui.add(btn_inline).on_hover_text("Open inline terminal to view live process output and run shell commands in working directory").clicked() {
                                                            if is_inline_open {
                                                                self.open_inline_terminals.remove(&listener.port);
                                                            } else {
                                                                self.open_inline_terminals.insert(listener.port);
                                                                self.ensure_inline_session(listener.port, &listener.name, listener.pid, &cmd_str, &cwd_str);
                                                            }
                                                        }

                                                        let btn_external = egui::Button::new(
                                                            RichText::new(format!("{} External Terminal", crate::lucide::EXTERNAL_LINK))
                                                                .size(11.0)
                                                                .strong()
                                                                .color(p.info),
                                                        )
                                                        .fill(p.bg_button)
                                                        .stroke(Stroke::NONE)
                                                        .rounding(Rounding::same(999.0));

                                                        if ui.add(btn_external).on_hover_text("Open desktop terminal window (gnome-terminal, alacritty, etc.) in working directory").clicked() {
                                                            to_launch_external_terminal = Some((
                                                                listener.name.clone(),
                                                                listener.pid,
                                                                listener.port,
                                                                cmd_str.clone(),
                                                                cwd_str.clone(),
                                                            ));
                                                        }

                                                        if !cwd_str.is_empty() {
                                                            let btn_folder = egui::Button::new(
                                                                RichText::new(format!("{} Open Folder", crate::lucide::FOLDER))
                                                                    .size(11.0)
                                                                    .strong()
                                                                    .color(p.warning),
                                                            )
                                                            .fill(p.bg_button)
                                                            .stroke(Stroke::NONE)
                                                            .rounding(Rounding::same(999.0));

                                                            if ui.add(btn_folder).on_hover_text("Open working directory in desktop file manager (xdg-open)").clicked() {
                                                                crate::service::open_folder(&cwd_str);
                                                            }

                                                            let btn_vscode = egui::Button::new(
                                                                RichText::new(format!("{} VS Code", crate::lucide::CODE))
                                                                    .size(11.0)
                                                                    .strong()
                                                                    .color(p.info),
                                                            )
                                                            .fill(p.bg_button)
                                                            .stroke(Stroke::NONE)
                                                            .rounding(Rounding::same(999.0));

                                                            if ui.add(btn_vscode).on_hover_text("Open working directory in VS Code (code .)").clicked() {
                                                                crate::service::open_in_vscode(&cwd_str);
                                                            }
                                                        }
                                                    });

                                                    if self.open_inline_terminals.contains(&listener.port) {
                                                        ui.add_space(8.0);
                                                        self.render_inline_terminal(ui, listener.port, &listener.name, listener.pid, &cwd_str);
                                                    }
                                                });
                                            });
                                    }
                                });
                            });

                        ui.add_space(6.0);
                    }

                    // Render any stopped external processes
                    for (port, name, cmd, cwd) in &visible_stopped {
                        let is_expanded = self.expanded_listeners.contains(port);
                        let card_height = if is_expanded { 120.0 } else { 50.0 };
                        let current_pos = ui.cursor().min;
                        let approx_rect = egui::Rect::from_min_size(current_pos, egui::vec2(ui.available_width(), card_height));
                        if !ui.clip_rect().expand(120.0).intersects(approx_rect) {
                            ui.allocate_space(egui::vec2(ui.available_width(), card_height));
                            ui.add_space(6.0);
                            continue;
                        }

                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    let is_expanded = self.expanded_listeners.contains(port);
                                    ui.horizontal(|ui| {
                                        draw_status_dot(ui, p.text_muted);

                                        let toggle_label = if is_expanded { "v" } else { ">" };
                                        let btn_toggle = egui::Button::new(
                                            RichText::new(toggle_label)
                                                .size(11.0)
                                                .strong()
                                                .color(if is_expanded { p.accent } else { p.text_muted }),
                                        )
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(4.0))
                                        .min_size(egui::vec2(22.0, 20.0));
                                        if ui.add(btn_toggle).on_hover_text(if is_expanded { "Collapse server location & command" } else { "View server location in expanded view" }).clicked() {
                                            if is_expanded {
                                                self.expanded_listeners.remove(port);
                                            } else {
                                                self.expanded_listeners.insert(*port);
                                            }
                                        }

                                        ui.label(
                                            RichText::new(name)
                                                .size(13.5)
                                                .strong()
                                                .color(p.text_primary),
                                        );
                                        ui.label(
                                            RichText::new(format!(":{}", port))
                                                .size(11.5)
                                                .color(p.text_muted),
                                        );

                                        ui.add_space(4.0);
                                        render_metric_pill(
                                            ui,
                                            crate::lucide::PLUG,
                                            &format!(":{}", port),
                                            p.text_muted,
                                        );
                                        render_metric_pill(
                                            ui,
                                            "○",
                                            "Stopped",
                                            p.text_muted,
                                        );

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let btn_del = egui::Button::new(
                                                RichText::new(crate::lucide::X).size(11.0).color(p.text_muted),
                                            )
                                            .fill(p.bg_button)
                                            .rounding(Rounding::same(4.0));
                                            if ui.add(btn_del).clicked() {
                                                to_remove_stopped = Some(*port);
                                            }

                                            let btn_start = egui::Button::new(
                                                RichText::new("Start").size(11.5).strong().color(Color32::WHITE),
                                            )
                                            .fill(p.success)
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(54.0, 24.0));
                                            if ui.add(btn_start).clicked() {
                                                to_start_stopped = Some((*port, cmd.clone(), cwd.clone()));
                                            }
                                        });
                                    });

                                    // Expandable location & command details for stopped
                                    if is_expanded {
                                        ui.add_space(8.0);
                                        Frame::none()
                                            .fill(p.bg_main)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(8.0))
                                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                            .show(ui, |ui| {
                                                ui.vertical(|ui| {
                                                    if !cwd.is_empty() {
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new(format!("{} Location:", crate::lucide::FOLDER)).size(11.0).strong().color(p.accent));
                                                            ui.label(
                                                                RichText::new(cwd)
                                                                    .size(10.5)
                                                                    .monospace()
                                                                    .color(p.text_primary),
                                                            );
                                                        });
                                                    }
                                                    if !cmd.is_empty() {
                                                        if !cwd.is_empty() {
                                                            ui.add_space(4.0);
                                                        }
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new(format!("{} Command:", crate::lucide::SETTINGS)).size(11.0).strong().color(p.accent));
                                                            ui.label(
                                                                RichText::new(cmd)
                                                                    .size(10.5)
                                                                    .monospace()
                                                                    .color(p.text_secondary),
                                                            );
                                                        });
                                                    }
                                                    if cwd.is_empty() && cmd.is_empty() {
                                                        ui.label(
                                                            RichText::new("No command line or location saved.")
                                                                .size(10.5)
                                                                .italics()
                                                                .color(p.text_muted),
                                                        );
                                                    }

                                                    ui.add_space(8.0);
                                                    ui.horizontal(|ui| {
                                                        let is_inline_open = self.open_inline_terminals.contains(port);
                                                        let inline_btn_text = if is_inline_open {
                                                             format!("{} Hide Inline Terminal", crate::lucide::TERMINAL)
                                                        } else {
                                                             format!("{} View Output (Inline Terminal)", crate::lucide::TERMINAL)
                                                        };
                                                        let btn_inline = egui::Button::new(
                                                            RichText::new(inline_btn_text)
                                                                .size(11.0)
                                                                .strong()
                                                                .color(if is_inline_open { p.warning } else { p.accent }),
                                                        )
                                                        .fill(if is_inline_open { p.bg_card } else { p.bg_button })
                                                        .stroke(Stroke::NONE)
                                                        .rounding(Rounding::same(999.0));

                                                        if ui.add(btn_inline).on_hover_text("Open inline terminal to run shell commands in working directory").clicked() {
                                                            if is_inline_open {
                                                                self.open_inline_terminals.remove(port);
                                                            } else {
                                                                self.open_inline_terminals.insert(*port);
                                                                self.ensure_inline_session(*port, name, None, cmd, cwd);
                                                            }
                                                        }

                                                        let btn_external = egui::Button::new(
                                                            RichText::new(format!("{} External Terminal", crate::lucide::EXTERNAL_LINK))
                                                                .size(11.0)
                                                                .strong()
                                                                .color(p.info),
                                                        )
                                                        .fill(p.bg_button)
                                                        .stroke(Stroke::NONE)
                                                        .rounding(Rounding::same(999.0));

                                                        if ui.add(btn_external).on_hover_text("Open desktop terminal window (gnome-terminal, alacritty, etc.) in working directory").clicked() {
                                                            to_launch_external_terminal = Some((
                                                                name.clone(),
                                                                None,
                                                                *port,
                                                                cmd.clone(),
                                                                cwd.clone(),
                                                            ));
                                                        }

                                                        if !cwd.is_empty() {
                                                            let btn_folder = egui::Button::new(
                                                                RichText::new(format!("{} Open Folder", crate::lucide::FOLDER))
                                                                    .size(11.0)
                                                                    .strong()
                                                                    .color(p.warning),
                                                            )
                                                            .fill(p.bg_button)
                                                            .stroke(Stroke::NONE)
                                                            .rounding(Rounding::same(999.0));

                                                            if ui.add(btn_folder).on_hover_text("Open working directory in desktop file manager (xdg-open)").clicked() {
                                                                crate::service::open_folder(cwd);
                                                            }

                                                            let btn_vscode = egui::Button::new(
                                                                RichText::new(format!("{} VS Code", crate::lucide::CODE))
                                                                    .size(11.0)
                                                                    .strong()
                                                                    .color(p.info),
                                                            )
                                                            .fill(p.bg_button)
                                                            .stroke(Stroke::NONE)
                                                            .rounding(Rounding::same(999.0));

                                                            if ui.add(btn_vscode).on_hover_text("Open working directory in VS Code (code .)").clicked() {
                                                                crate::service::open_in_vscode(cwd);
                                                            }
                                                        }
                                                    });

                                                    if self.open_inline_terminals.contains(port) {
                                                        ui.add_space(8.0);
                                                        self.render_inline_terminal(ui, *port, name, None, cwd);
                                                    }
                                                });
                                            });
                                    }
                                });
                            });

                        ui.add_space(6.0);
                    }
                }

                if let Some((name, pid, port, cmd, cwd)) = to_launch_external_terminal {
                    crate::service::open_in_console(&name, pid, port, &cwd, &cmd);
                }

                if let Some((pid_opt, port, name, cmd, cwd)) = to_kill_pid {
                    if let Some(pid) = pid_opt {
                        crate::scanner::kill_process_pid(pid);
                    }
                    crate::scanner::kill_port_listener(port);
                    if !cmd.is_empty() {
                        self.stopped_external.insert(port, (name, cmd, cwd));
                    }
                    self.scanner.trigger_scan();
                }

                if let Some((pid_opt, port, cmd, cwd)) = to_restart_pid {
                    if let Some(pid) = pid_opt {
                        crate::scanner::kill_process_pid(pid);
                    }
                    crate::scanner::kill_port_listener(port);
                    if !cmd.is_empty() {
                        let cmd_to_run = cmd.clone();
                        let cwd_to_run = cwd.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(250));
                            #[cfg(target_os = "windows")]
                            let _ = std::process::Command::new("cmd.exe")
                                .args(&["/C", &cmd_to_run])
                                .current_dir(&cwd_to_run)
                                .spawn();
                            #[cfg(not(target_os = "windows"))]
                            let _ = std::process::Command::new("sh")
                                .args(&["-c", &cmd_to_run])
                                .current_dir(&cwd_to_run)
                                .spawn();
                        });
                    }
                    self.scanner.trigger_scan();
                }

                if let Some((port, cmd, cwd)) = to_start_stopped {
                    self.stopped_external.remove(&port);
                    if !cmd.is_empty() {
                        let cmd_to_run = cmd.clone();
                        let cwd_to_run = cwd.clone();
                        std::thread::spawn(move || {
                            #[cfg(target_os = "windows")]
                            let _ = std::process::Command::new("cmd.exe")
                                .args(&["/C", &cmd_to_run])
                                .current_dir(&cwd_to_run)
                                .spawn();
                            #[cfg(not(target_os = "windows"))]
                            let _ = std::process::Command::new("sh")
                                .args(&["-c", &cmd_to_run])
                                .current_dir(&cwd_to_run)
                                .spawn();
                        });
                    }
                    self.scanner.trigger_scan();
                }

                if let Some(port) = to_remove_stopped {
                    self.stopped_external.remove(&port);
                }
            });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Right Panel Tab 1+: Server Log View + Ctrl+F Search + Command Bar
    // ─────────────────────────────────────────────────────────────────────────
    fn render_server_log_content(&mut self, ui: &mut Ui, key: &str) {
        let (name, is_running, _pid, port, port_open, cpu, mem, uptime, actions, links) = {
            if let Some(s) = self.find_service(key) {
                (
                    s.config.name.clone(),
                    s.state == ServiceState::Running || s.state == ServiceState::Starting,
                    s.pid,
                    s.config.port,
                    s.port_open,
                    s.cpu_usage,
                    s.memory_mb,
                    s.uptime_formatted(),
                    s.config.actions.clone(),
                    s.config.links.clone(),
                )
            } else {
                ui.label("Service not found.");
                return;
            }
        };

        // ─────────────────────────────────────────────────────────────────────
        // Section A: Upper Controls Cockpit (Card)
        // ─────────────────────────────────────────────────────────────────────
        let p = self.theme.palette();
        Frame::none()
            .fill(p.bg_card)
            .stroke(Stroke::new(1.0_f32, p.border))
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(10.0, 5.0))
            .show(ui, |ui| {
                // Row 1: Service name + Live Metrics ... Find toggle + Start/Stop/Restart
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&name)
                            .size(15.0)
                            .strong()
                            .color(p.text_primary),
                    );

                    if is_running {
                        ui.add_space(8.0);
                        if port > 0 {
                            let port_color = if port_open {
                                Color32::from_rgb(96, 165, 250)
                            } else {
                                Color32::from_rgb(148, 163, 184)
                            };
                            render_metric_pill(
                                ui,
                                crate::lucide::NETWORK,
                                &format!(":{}", port),
                                port_color,
                            );
                        }
                        render_metric_pill(
                            ui,
                            crate::lucide::CPU,
                            &format!("{:.1}%", cpu),
                            Color32::from_rgb(96, 165, 250),
                        );
                        render_metric_pill(
                            ui,
                            crate::lucide::RAM,
                            &format!("{:.1} MB", mem),
                            Color32::from_rgb(52, 211, 153),
                        );
                        if !uptime.is_empty() {
                            render_metric_pill(
                                ui,
                                crate::lucide::CLOCK,
                                &uptime,
                                Color32::from_rgb(251, 191, 36),
                            );
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if is_running {
                            let btn_stop = egui::Button::new(
                                RichText::new("Stop").size(11.0).strong().color(Color32::WHITE),
                            )
                            .fill(p.danger)
                            .rounding(Rounding::same(4.0));
                            if ui.add(btn_stop).clicked() {
                                if let Some(s) = self.find_service_mut(key) {
                                    s.stop();
                                }
                            }

                            let btn_restart = egui::Button::new(
                                RichText::new("Restart").size(11.0).strong().color(Color32::WHITE),
                            )
                            .fill(p.warning)
                            .rounding(Rounding::same(4.0));
                            if ui.add(btn_restart).clicked() {
                                if let Some(s) = self.find_service_mut(key) {
                                    s.restart();
                                }
                            }
                        } else {
                            let btn_start = egui::Button::new(
                                RichText::new("Start").size(11.0).strong().color(Color32::WHITE),
                            )
                            .fill(p.success)
                            .rounding(Rounding::same(4.0));
                            if ui.add(btn_start).clicked() {
                                if let Some(s) = self.find_service_mut(key) {
                                    s.start();
                                }
                            }
                        }

                        let search_is_open = self.search_open.get(key).copied().unwrap_or(false);
                        let find_btn = egui::Button::new(
                            RichText::new(format!("{} Find (Ctrl+F)", crate::lucide::SEARCH))
                                .size(11.0)
                                .color(if search_is_open { p.accent } else { p.text_secondary }),
                        )
                        .fill(if search_is_open { p.accent.gamma_multiply(0.25) } else { p.bg_button })
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(999.0));
                        if ui.add(find_btn).clicked() {
                            self.search_open.insert(key.to_string(), !search_is_open);
                        }
                    });
                });

                ui.add_space(3.0);

                // Row 2: Secondary Tool Buttons & Service / Log Options
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 3.0);

                    let vscode_btn = egui::Button::new(
                        RichText::new(format!("{} VS Code", crate::lucide::CODE))
                            .size(11.0)
                            .color(p.info),
                    )
                    .fill(p.bg_button)
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(999.0));
                    if ui.add(vscode_btn).on_hover_text("Open working directory in VS Code (code .)").clicked() {
                        let cwd = self.find_service(key).map(|s| s.config.cwd.clone()).unwrap_or_default();
                        crate::service::open_in_vscode(&cwd);
                    }

                    let folder_btn = egui::Button::new(
                        RichText::new(format!("{} Folder", crate::lucide::FOLDER))
                            .size(11.0)
                            .color(p.text_secondary),
                    )
                    .fill(p.bg_button)
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(999.0));
                    if ui.add(folder_btn).on_hover_text("Open working directory in file manager (xdg-open)").clicked() {
                        let cwd = self.find_service(key).map(|s| s.config.cwd.clone()).unwrap_or_default();
                        crate::service::open_folder(&cwd);
                    }

                    let console_btn = egui::Button::new(
                        RichText::new(format!("{} Console", crate::lucide::TERMINAL))
                            .size(11.0)
                            .color(p.accent_light),
                    )
                    .fill(p.bg_button)
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(999.0));
                    if ui.add(console_btn).on_hover_text("Open interactive console / terminal window in working directory").clicked() {
                        if let Some(s) = self.find_service(key) {
                            s.open_console();
                        }
                    }

                    ui.add_space(3.0);

                    let mut auto_scroll = self.auto_scroll.get(key).copied().unwrap_or(true);
                    if ui.checkbox(
                        &mut auto_scroll,
                        RichText::new(format!("{} Auto-scroll", crate::lucide::ARROW_DOWN)).size(11.0).color(p.accent_light),
                    ).on_hover_text("Keep pinned to bottom when new logs arrive (uncheck to scroll up smoothly without jitter)").changed() {
                        self.auto_scroll.insert(key.to_string(), auto_scroll);
                    }

                    let mut autostart = self.find_service(key).map(|s| s.config.autostart).unwrap_or(false);
                    if ui.checkbox(
                        &mut autostart,
                        RichText::new(format!("{} Autostart", crate::lucide::PLAY)).size(11.0).color(p.accent_light),
                    ).on_hover_text("Automatically start this server when Server Launcher opens").changed() {
                        if let Some(s) = self.find_service_mut(key) {
                            s.config.autostart = autostart;
                            self.persist_config();
                        }
                    }

                    let mut auto_restart = self.find_service(key).map(|s| s.config.auto_restart).unwrap_or(false);
                    if ui.checkbox(
                        &mut auto_restart,
                        RichText::new(format!("{} Auto-restart", crate::lucide::REFRESH_CW)).size(11.0).color(p.accent_light),
                    ).on_hover_text("Auto-restart this server if it exits unexpectedly or crashes").changed() {
                        if let Some(s) = self.find_service_mut(key) {
                            s.config.auto_restart = auto_restart;
                            self.persist_config();
                        }
                    }
                });

                // Quick Sub-command & Link Buttons Bar (if defined)
                if !actions.is_empty() || !links.is_empty() {
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        for act in &actions {
                            let act_btn = egui::Button::new(
                                RichText::new(format!("{} {}", crate::lucide::ZAP, act.label))
                                    .size(10.5)
                                    .color(p.warning),
                            )
                            .fill(p.bg_button)
                            .rounding(Rounding::same(4.0));
                            if ui.add(act_btn).clicked() {
                                self.run_service_action(key, &act.command);
                            }
                        }

                        for lnk in &links {
                            let lnk_btn = egui::Button::new(
                                RichText::new(format!("{} {}", crate::lucide::EXTERNAL_LINK, lnk.label))
                                    .size(10.5)
                                    .color(p.info),
                            )
                            .fill(p.bg_button)
                            .rounding(Rounding::same(4.0));
                            if ui.add(lnk_btn).clicked() {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(&lnk.url));
                            }
                        }
                    });
                }

                // Collapsible Search Bar (Ctrl+F)
                let search_is_open = self.search_open.get(key).copied().unwrap_or(false);
                if search_is_open {
                    ui.add_space(2.0);
                    Frame::none()
                        .fill(p.bg_subtle)
                        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                        .rounding(Rounding::same(4.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::lucide::SEARCH).color(p.accent));
                                let q_entry = self.search_queries.entry(key.to_string()).or_default();
                                let search_input = ui.add(
                                    egui::TextEdit::singleline(q_entry)
                                        .hint_text("Search logs…")
                                        .desired_width(220.0),
                                );
                                if search_is_open {
                                    search_input.request_focus();
                                }

                                let filter_entry = self.filter_lines.entry(key.to_string()).or_default();
                                ui.checkbox(filter_entry, RichText::new("Filter lines").size(11.0).color(p.text_secondary));

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button(crate::lucide::X).clicked() {
                                        self.search_open.insert(key.to_string(), false);
                                    }
                                });
                            });
                        });
                }
            });

        ui.add_space(3.0);

        // ─────────────────────────────────────────────────────────────────────
        // Section B: Terminal Output Area (Recessed Monospace Code Well)
        // ─────────────────────────────────────────────────────────────────────
        let logs_vec: Vec<_> = {
            if let Some(s) = self.find_service(key) {
                if let Ok(l) = s.logs.lock() {
                    l.iter().cloned().collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        };

        let q = self.search_queries.get(key).cloned().unwrap_or_default().to_lowercase();
        let filter_active = self.filter_lines.get(key).copied().unwrap_or(false) && !q.is_empty();

        let filtered_logs: Vec<&crate::service::LogEntry> = logs_vec
            .iter()
            .filter(|entry| !filter_active || entry.text.to_lowercase().contains(&q))
            .collect();

        let bottom_dock_reserve = if is_running { 92.0 } else { 58.0 };
        let terminal_available_height = (ui.available_height() - bottom_dock_reserve).max(100.0);
        let frame_v_margin = 8.0_f32;
        let inner_terminal_height = (terminal_available_height - frame_v_margin).max(80.0);
        let auto_scroll_enabled = self.auto_scroll.get(key).copied().unwrap_or(true);
        let row_height = 17.0_f32;
        let total_rows = filtered_logs.len();

        Frame::none()
            .fill(p.terminal_bg)
            .stroke(Stroke::new(1.0_f32, p.border.gamma_multiply(0.7)))
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                ui.set_height(inner_terminal_height);
                ui.set_max_height(inner_terminal_height);
                ui.spacing_mut().item_spacing.y = 1.0;
                ScrollArea::both()
                    .id_salt(format!("log_scroll_{}", key))
                    .stick_to_bottom(auto_scroll_enabled)
                    .auto_shrink([false, false])
                    .min_scrolled_height(inner_terminal_height)
                    .max_height(inner_terminal_height)
                    .show_rows(ui, row_height, total_rows + 1, |ui, row_range| {
                        ui.set_width(ui.available_width());
                        for idx in row_range {
                            if idx < total_rows {
                                if let Some(entry) = filtered_logs.get(idx) {
                                    let base_color = match entry.kind {
                                        LogKind::Launcher => p.log_launcher,
                                        LogKind::Stdin => p.log_stdin,
                                        LogKind::Stderr | LogKind::Error => p.log_stderr,
                                        LogKind::Warning => p.log_warning,
                                        LogKind::Stdout => p.log_stdout,
                                    };

                                    let contains_query = !q.is_empty() && entry.text.to_lowercase().contains(&q);
                                    let mut rt = RichText::new(format!("[{}] {}", entry.timestamp, entry.text))
                                        .color(base_color)
                                        .monospace()
                                        .size(11.0);

                                    if contains_query {
                                        rt = rt.background_color(Color32::from_rgb(180, 80, 0)).color(Color32::WHITE);
                                    }

                                    ui.add(egui::Label::new(rt));
                                }
                            } else {
                                // Bottom clearance spacer so the last terminal line is NEVER hidden by the horizontal scrollbar or bottom edge
                                ui.add_space(20.0);
                            }
                        }
                    });
            });

        // Build available slash commands
        let mut all_commands: Vec<SlashCommand> = vec![
            SlashCommand {
                name: "/start".to_string(),
                description: "Start this server".to_string(),
                category: "Control",
                action: SlashAction::Start,
            },
            SlashCommand {
                name: "/stop".to_string(),
                description: "Stop this server".to_string(),
                category: "Control",
                action: SlashAction::Stop,
            },
            SlashCommand {
                name: "/restart".to_string(),
                description: "Restart this server".to_string(),
                category: "Control",
                action: SlashAction::Restart,
            },
            SlashCommand {
                name: "/clear".to_string(),
                description: "Clear terminal logs".to_string(),
                category: "Utility",
                action: SlashAction::Clear,
            },
            SlashCommand {
                name: "/status".to_string(),
                description: "Show PID, port, uptime and resource usage".to_string(),
                category: "Utility",
                action: SlashAction::Status,
            },
            SlashCommand {
                name: "/edit".to_string(),
                description: "Open server configuration modal".to_string(),
                category: "Utility",
                action: SlashAction::Edit,
            },
            SlashCommand {
                name: "/help".to_string(),
                description: "Show command reference and tips".to_string(),
                category: "Utility",
                action: SlashAction::Help,
            },
            SlashCommand {
                name: "/console".to_string(),
                description: "Open in external console / terminal window".to_string(),
                category: "Utility",
                action: SlashAction::Console,
            },
            SlashCommand {
                name: "/terminal".to_string(),
                description: "Open in external console / terminal window".to_string(),
                category: "Utility",
                action: SlashAction::Console,
            },
            SlashCommand {
                name: "/h".to_string(),
                description: "Send 'h + enter' to server (Vite / dev server help)".to_string(),
                category: "Input",
                action: SlashAction::SendStdin("h".to_string()),
            },
            SlashCommand {
                name: "/r".to_string(),
                description: "Send 'r + enter' to server (restart dev server)".to_string(),
                category: "Input",
                action: SlashAction::SendStdin("r".to_string()),
            },
            SlashCommand {
                name: "/o".to_string(),
                description: "Send 'o + enter' to server (open in browser)".to_string(),
                category: "Input",
                action: SlashAction::SendStdin("o".to_string()),
            },
            SlashCommand {
                name: "/u".to_string(),
                description: "Send 'u + enter' to server (show URLs)".to_string(),
                category: "Input",
                action: SlashAction::SendStdin("u".to_string()),
            },
            SlashCommand {
                name: "/c".to_string(),
                description: "Send 'c + enter' to server (clear console)".to_string(),
                category: "Input",
                action: SlashAction::SendStdin("c".to_string()),
            },
            SlashCommand {
                name: "/q".to_string(),
                description: "Send 'q + enter' to server (quit dev server)".to_string(),
                category: "Input",
                action: SlashAction::SendStdin("q".to_string()),
            },
        ];

        for act in &actions {
            let slug = format!("/{}", act.label.to_lowercase().replace(' ', "-"));
            all_commands.push(SlashCommand {
                name: slug,
                description: format!("Execute: {}", act.command),
                category: "Action",
                action: SlashAction::RunAction(act.command.clone()),
            });
        }

        for lnk in &links {
            let slug = format!("/{}", lnk.label.to_lowercase().replace(' ', "-"));
            all_commands.push(SlashCommand {
                name: slug,
                description: format!("Open: {}", lnk.url),
                category: "Link",
                action: SlashAction::OpenLink(lnk.url.clone()),
            });
        }

        let mut input_val = self.input_texts.get(key).cloned().unwrap_or_default();
        let is_slash = input_val.starts_with('/');
        let filter_query = input_val.trim().to_lowercase();
        let filtered: Vec<SlashCommand> = if is_slash {
            let q_no_slash = filter_query.trim_start_matches('/');
            all_commands
                .iter()
                .filter(|c| {
                    if filter_query == "/" {
                        true
                    } else {
                        c.name.to_lowercase().starts_with(&filter_query)
                            || c.name.to_lowercase().contains(q_no_slash)
                            || c.description.to_lowercase().contains(q_no_slash)
                    }
                })
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let show_autocomplete = is_slash && !filtered.is_empty();

        if show_autocomplete && self.autocomplete_selected >= filtered.len() {
            self.autocomplete_selected = 0;
        }

        if show_autocomplete {
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
                self.autocomplete_selected = (self.autocomplete_selected + 1) % filtered.len();
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
                self.autocomplete_selected = if self.autocomplete_selected == 0 {
                    filtered.len().saturating_sub(1)
                } else {
                    self.autocomplete_selected - 1
                };
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab)) {
                if let Some(cmd) = filtered.get(self.autocomplete_selected) {
                    input_val = format!("{} ", cmd.name);
                }
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                input_val.clear();
            }
        }

        let mut do_send = false;
        let mut clicked_cmd: Option<SlashCommand> = None;
        let mut execute_action: Option<SlashAction> = None;

        // ─────────────────────────────────────────────────────────────────────
        // Section C: Bottom Command & Shortcuts Dock (Card)
        // ─────────────────────────────────────────────────────────────────────
        ui.add_space(3.0);

        let resp = Frame::none()
            .fill(p.bg_card)
            .stroke(Stroke::new(1.0_f32, p.border))
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                // Quick Dev Server Shortcuts Toolbar (when running)
                if is_running {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(5.0, 2.0);
                        ui.label(RichText::new(format!("{} Shortcuts:", crate::lucide::ZAP)).size(11.0).color(p.text_muted));
                        let shortcuts = [
                            ("h (help)", "h", "Send 'h + enter' (show dev server help & shortcuts)"),
                            ("r (restart)", "r", "Send 'r + enter' (restart dev server)"),
                            ("o (open)", "o", "Send 'o + enter' (open in browser)"),
                            ("u (url)", "u", "Send 'u + enter' (show server URLs)"),
                            ("c (clear)", "c", "Send 'c + enter' (clear dev console)"),
                            ("q (quit)", "q", "Send 'q + enter' (quit dev server)"),
                        ];
                        for (label, key_cmd, tip) in shortcuts {
                            let btn = egui::Button::new(RichText::new(label).size(10.5).color(p.accent_light))
                                .fill(p.bg_button)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(999.0));
                            if ui.add(btn).on_hover_text(tip).clicked() {
                                if let Some(s) = self.find_service(key) {
                                    s.send_input(key_cmd);
                                }
                            }
                        }
                    });
                    ui.add_space(2.0);
                }

                let edit_resp = ui.horizontal(|ui| {
                    ui.label(RichText::new(">").monospace().color(p.accent).strong().size(13.0));

                    let hint = if is_running {
                        "Send input to server (e.g. h + enter for help, r to restart)..."
                    } else {
                        "Type '/' for commands, or run command in directory (e.g. npm i, cargo test)..."
                    };

                    let text_edit = egui::TextEdit::singleline(&mut input_val)
                        .hint_text(hint)
                        .desired_width(ui.available_width() - 325.0);

                    let edit_resp = ui.add(text_edit);

                    if edit_resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        do_send = true;
                    }

                    let btn_send = egui::Button::new(RichText::new("Send").size(11.0).strong().color(p.accent_text))
                        .fill(p.accent)
                        .rounding(Rounding::same(999.0));
                    if ui.add(btn_send).clicked() {
                        do_send = true;
                    }

                    let btn_ctrlc = egui::Button::new(RichText::new("Ctrl+C").size(11.0).color(p.warning))
                        .fill(p.bg_button)
                        .rounding(Rounding::same(999.0));
                    if ui.add(btn_ctrlc).clicked() {
                        if let Some(s) = self.find_service(key) {
                            s.send_interrupt();
                        }
                    }

                    let btn_clear = egui::Button::new(RichText::new("Clear").size(11.0).color(p.text_muted))
                        .fill(p.bg_button)
                        .rounding(Rounding::same(999.0));
                    if ui.add(btn_clear).clicked() {
                        if let Some(s) = self.find_service(key) {
                            s.clear_logs();
                        }
                    }

                    let btn_console = egui::Button::new(RichText::new(format!("{} Console", crate::lucide::TERMINAL)).size(11.0).color(p.accent_light))
                        .fill(p.bg_button)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(999.0));
                    if ui.add(btn_console).on_hover_text("Open in external console / terminal window").clicked() {
                        if let Some(s) = self.find_service(key) {
                            s.open_console();
                        }
                    }

                    edit_resp
                }).inner;

                edit_resp
            }).inner;

        if show_autocomplete {
            let popup_id = egui::Id::new(format!("slash_autocomplete_area_{}", key));
            let item_h = 28.0;
            let visible_count = filtered.len().min(6);
            let total_h = (visible_count as f32 * item_h) + 42.0;
            let popup_w = (resp.rect.width() + 60.0).max(480.0);
            let popup_pos = egui::pos2(resp.rect.min.x, (resp.rect.min.y - total_h - 6.0).max(40.0));

            egui::Area::new(popup_id)
                .fixed_pos(popup_pos)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(12.0))
                        .shadow(egui::epaint::Shadow {
                            offset: egui::vec2(0.0, 4.0),
                            blur: 12.0,
                            spread: 0.0,
                            color: Color32::from_black_alpha(160),
                        })
                        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                        .show(ui, |ui| {
                            ui.set_width(popup_w);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{} Commands", crate::lucide::ZAP)).size(11.0).strong().color(p.accent));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new("↑↓ Navigate  •  Enter Select  •  Tab Complete  •  Esc Close").size(9.5).color(p.text_muted));
                                });
                            });
                            ui.add_space(2.0);
                            ui.separator();
                            ui.add_space(2.0);

                            ScrollArea::vertical()
                                .max_height(visible_count as f32 * item_h)
                                .show(ui, |ui| {
                                    for (idx, cmd) in filtered.iter().enumerate() {
                                        let is_selected = idx == self.autocomplete_selected;
                                        let bg = if is_selected {
                                            p.bg_card_hover
                                        } else {
                                            Color32::TRANSPARENT
                                        };

                                        let row_frame = Frame::none()
                                            .fill(bg)
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::symmetric(6.0, 4.0));

                                        let row_inner = row_frame.show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                let name_color = if is_selected {
                                                    p.accent
                                                } else {
                                                    p.text_primary
                                                };
                                                ui.label(RichText::new(&cmd.name).strong().size(12.0).color(name_color));
                                                ui.add_space(6.0);
                                                ui.label(RichText::new(&cmd.description).size(11.0).color(p.text_muted));

                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    let (badge_bg, badge_fg) = match cmd.category {
                                                        "Control" => (p.bg_sidebar, p.accent),
                                                        "Action" => (p.bg_sidebar, p.warning),
                                                        "Link" => (p.bg_sidebar, p.info),
                                                        _ => (p.bg_button, p.text_muted),
                                                    };
                                                    Frame::none()
                                                        .fill(badge_bg)
                                                        .stroke(Stroke::NONE)
                                                        .rounding(Rounding::same(999.0))
                                                        .inner_margin(egui::Margin::symmetric(5.0, 2.0))
                                                        .show(ui, |ui| {
                                                            ui.label(RichText::new(cmd.category).size(9.5).color(badge_fg));
                                                        });
                                                });
                                            });
                                        });

                                        let row_interact = ui.interact(row_inner.response.rect, row_inner.response.id, egui::Sense::click());
                                        if row_interact.hovered() && !is_selected {
                                            self.autocomplete_selected = idx;
                                        }
                                        if row_interact.clicked() {
                                            clicked_cmd = Some(cmd.clone());
                                        }
                                    }
                                });
                        });
                });
        }

        if let Some(cmd) = clicked_cmd {
            execute_action = Some(cmd.action);
            input_val.clear();
            self.autocomplete_selected = 0;
            resp.request_focus();
        } else if do_send {
            let trimmed = input_val.trim().to_string();
            if !trimmed.is_empty() {
                if show_autocomplete {
                    if let Some(cmd) = filtered.get(self.autocomplete_selected) {
                        execute_action = Some(cmd.action.clone());
                    }
                } else if trimmed.starts_with('/') {
                    if trimmed.starts_with("/run ") || trimmed.starts_with("/sh ") {
                        let sub = trimmed.trim_start_matches("/run ").trim_start_matches("/sh ").trim();
                        execute_action = Some(SlashAction::RunAction(sub.to_string()));
                    } else {
                        let first_token = trimmed.split_whitespace().next().unwrap_or(&trimmed);
                        if let Some(cmd) = all_commands.iter().find(|c| c.name.eq_ignore_ascii_case(first_token)) {
                            execute_action = Some(cmd.action.clone());
                        } else {
                            if let Some(s) = self.find_service(key) {
                                s.append_log(
                                    format!("[command error] Unknown slash command: '{}'. Type /help for available commands.", first_token),
                                    LogKind::Warning,
                                );
                            }
                        }
                    }
                } else {
                    // Regular command or stdin
                    let is_active = self.find_service(key).map(|s| s.is_active()).unwrap_or(false);

                    if is_active {
                        if let Some(s) = self.find_service(key) {
                            s.send_input(&trimmed);
                        }
                    } else {
                        let lower = trimmed.to_lowercase();
                        let s_cmd = self.find_service(key).map(|s| s.config.command.clone());
                        if lower == "h" || lower == "help" || lower == "?" {
                            if let Some(s) = self.find_service(key) {
                                s.append_log(
                                    "[hint] Server is currently stopped. Click '▶ Start' (or type /start) to launch it. Once running, you can send 'h + enter', 'r + enter', 'o + enter' and other interactive shortcuts directly to the server.".to_string(),
                                    LogKind::Launcher,
                                );
                            }
                        } else if lower == "start" || lower == "s" || s_cmd.as_deref().map(|c| c.to_lowercase() == lower).unwrap_or(false) {
                            if let Some(s) = self.find_service_mut(key) {
                                s.start();
                            }
                        } else if lower == "restart" || lower == "r" {
                            if let Some(s) = self.find_service_mut(key) {
                                s.restart();
                            }
                        } else if lower == "q" || lower == "quit" || lower == "exit" {
                            if let Some(s) = self.find_service(key) {
                                s.append_log("[hint] Server is already stopped.".to_string(), LogKind::Launcher);
                            }
                        } else if lower == "o" || lower == "u" || lower == "c" {
                            if let Some(s) = self.find_service(key) {
                                s.append_log(
                                    format!("[hint] '{}' is an interactive dev server shortcut. Click '▶ Start' to launch the server first, then send it.", trimmed),
                                    LogKind::Launcher,
                                );
                            }
                        } else {
                            // Server is NOT running: execute directly in cwd!
                            self.run_service_action(key, &trimmed);
                        }
                    }
                }
                input_val.clear();
                self.autocomplete_selected = 0;
                resp.request_focus();
            }
        }

        if let Some(act) = execute_action {
            match act {
                SlashAction::Start => {
                    if let Some(s) = self.find_service_mut(key) {
                        s.start();
                    }
                }
                SlashAction::Stop => {
                    if let Some(s) = self.find_service_mut(key) {
                        s.stop();
                    }
                }
                SlashAction::Restart => {
                    if let Some(s) = self.find_service_mut(key) {
                        s.restart();
                    }
                }
                SlashAction::Clear => {
                    if let Some(s) = self.find_service(key) {
                        s.clear_logs();
                    }
                }
                SlashAction::Status => {
                    if let Some(s) = self.find_service(key) {
                        let status_msg = format!(
                            "━━━ Status for {} ━━━\n• State: {:?}\n• PID: {}\n• Port: {}\n• Uptime: {}\n• Memory: {:.1} MB\n• CPU: {:.1}%",
                            s.config.name,
                            s.state,
                            s.pid.map(|p| p.to_string()).unwrap_or_else(|| "N/A".to_string()),
                            s.config.port,
                            if s.uptime_formatted().is_empty() { "0s".to_string() } else { s.uptime_formatted() },
                            s.memory_mb,
                            s.cpu_usage,
                        );
                        s.append_log(status_msg, LogKind::Launcher);
                    }
                }
                SlashAction::Edit => {
                    if let Some(s) = self.find_service(key) {
                        let cfg = s.config.clone();
                        self.add_edit_modal.open_edit(&cfg);
                    }
                }
                SlashAction::Help => {
                    if let Some(s) = self.find_service(key) {
                        let mut help = String::from("━━━ Terminal & Slash Commands ━━━\n");
                        help.push_str("• /start    - Start this server\n");
                        help.push_str("• /stop     - Stop this server\n");
                        help.push_str("• /restart  - Restart this server\n");
                        help.push_str("• /clear    - Clear terminal logs\n");
                        help.push_str("• /status   - Show status, PID, port, uptime and resource usage\n");
                        help.push_str("• /edit     - Open configuration modal for this server\n");
                        help.push_str("• /console  - Open server in external console / terminal window\n");
                        help.push_str("• /help     - Show this command reference\n");
                        if !s.config.actions.is_empty() {
                            help.push_str("\nCustom Actions:\n");
                            for a in &s.config.actions {
                                let slug = format!("/{}", a.label.to_lowercase().replace(' ', "-"));
                                help.push_str(&format!("• {:<12} - Exec: {}\n", slug, a.command));
                            }
                        }
                        if !s.config.links.is_empty() {
                            help.push_str("\nQuick Links:\n");
                            for l in &s.config.links {
                                let slug = format!("/{}", l.label.to_lowercase().replace(' ', "-"));
                                help.push_str(&format!("• {:<12} - Open: {}\n", slug, l.url));
                            }
                        }
                        help.push_str("\nWhen server is stopped: Type any command (e.g. npm i, cargo test) to run it directly in this server directory.\n");
                        help.push_str("When server is running: Type any text to send directly to process standard input.\n");
                        s.append_log(help, LogKind::Launcher);
                    }
                }
                SlashAction::Console => {
                    if let Some(s) = self.find_service(key) {
                        s.open_console();
                    }
                }
                SlashAction::SendStdin(txt) => {
                    if let Some(s) = self.find_service(key) {
                        s.send_input(&txt);
                    }
                }
                SlashAction::RunAction(cmd) => {
                    self.run_service_action(key, &cmd);
                }
                SlashAction::OpenLink(url) => {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(&url));
                }
            }
        }

        self.input_texts.insert(key.to_string(), input_val);
    }
}
