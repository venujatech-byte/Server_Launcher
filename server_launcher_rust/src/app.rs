use crate::config::{ConfigFile, CustomScript, SavedCommand, SshRemoteHost};
use crate::modals::{
    render_add_edit_modal, render_command_modal, render_command_palette,
    render_close_confirmation_modal, render_new_service_modal, render_script_modal,
    render_ssh_host_modal, AddEditModalState, CloseConfirmChoice, CommandPaletteState,
    ModalAction, NewServiceModalState, PaletteAction, SshHostModalState,
};
use crate::scripts::{
    execute_local_script, execute_remote_script, ScriptExecution, ScriptModalState, ScriptRunStatus,
};
use crate::commands::{CommandModalState, TerminalCommandStatus, TerminalSession};
use crate::systemd::{
    control_local_service, control_remote_service, create_local_service, create_remote_service,
    fetch_local_journal, fetch_remote_journal, list_local_services, list_remote_services,
    SystemdService,
};
use crate::remote::{
    build_ssh_command, fetch_remote_listeners, import_from_ssh_config,
    launch_external_ssh_terminal, test_ssh_connection, RemoteListener,
};
use crate::scanner::{get_lan_ip, SystemPortScanner};
use crate::service::{LogKind, Service, ServiceState};
use crate::theme::ThemeMode;
use eframe::egui;
use egui::{
    Color32, Context, Frame, Key, Modifiers, RichText, Rounding, ScrollArea, Stroke, Ui,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;
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
            SidebarTab::Servers => "🖥 Servers",
            SidebarTab::Scripts => "📜 Scripts",
            SidebarTab::Commands => "⚡ Cmds",
            SidebarTab::SystemMD => "⚙ SystemD",
            SidebarTab::Sensors => "🌡 Sensors",
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
    pub theme: ThemeMode,
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
    pub remote_services: HashMap<String, Vec<SystemdService>>,
    pub remote_services_user: HashMap<String, bool>,
    pub selected_service: Option<String>,
    pub service_journal_logs: String,
    pub service_search_query: String,
    pub service_action_msg: Option<String>,
    pub journal_refresh_interval: u32,
    pub last_journal_refresh: Instant,

    // Sensors State
    pub sensors_collector: crate::sensors::SystemSensorsCollector,
    pub sensors_snapshot: crate::sensors::SystemSensorsSnapshot,
    pub sensors_refresh_interval: u32,
    pub last_sensors_refresh: Instant,
    pub remote_sensors_snapshots: HashMap<String, crate::sensors::SystemSensorsSnapshot>,
    pub sensors_search_query: String,
    pub sensors_category_filter: String,
}

pub struct ExternalTerminalSession {
    pub logs: Arc<Mutex<VecDeque<crate::service::LogEntry>>>,
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
                execute_local_script(sc, &mut exec);
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
            theme,
            app_autostart,
            auto_scroll: HashMap::new(),
            proc_info_cache: HashMap::new(),
            remote_search_query: HashMap::new(),
            close_confirm_open: false,
            close_confirmed: false,

            script_executions,
            script_modal: ScriptModalState::default(),
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
            remote_services: HashMap::new(),
            remote_services_user: HashMap::new(),
            selected_service: None,
            service_journal_logs: String::new(),
            service_search_query: String::new(),
            service_action_msg: None,
            journal_refresh_interval: config_file.journal_refresh_interval.unwrap_or(0),
            last_journal_refresh: Instant::now(),

            sensors_collector: crate::sensors::SystemSensorsCollector::new(),
            sensors_snapshot: crate::sensors::SystemSensorsSnapshot::default(),
            sensors_refresh_interval: 2,
            last_sensors_refresh: Instant::now() - Duration::from_secs(10),
            remote_sensors_snapshots: HashMap::new(),
            sensors_search_query: String::new(),
            sensors_category_filter: "All".to_string(),
        }
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
            .stroke(Stroke::new(1.0_f32, p.terminal_border))
            .rounding(Rounding::same(6.0))
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("● Live Terminal").size(11.5).strong().color(p.success));
                    let pid_str = pid.map(|p| p.to_string()).unwrap_or_else(|| "N/A".to_string());
                    ui.label(RichText::new(format!("Port :{} | PID {}", port, pid_str)).size(10.5).color(p.text_muted));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕ Close").clicked() {
                            self.open_inline_terminals.remove(&port);
                        }
                        if ui.button("⟳ Refresh Logs").on_hover_text("Fetch latest systemd/journald process output").clicked() {
                            self.refresh_inline_logs(port, pid, cwd);
                        }
                        if ui.button("🗑 Clear").clicked() {
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
                    format!("⏹  Stop {}{}", s.config.name, grp_tag),
                    PaletteAction::StopServer(s.config.key.clone()),
                ));
                items.push((
                    format!("🔄  Restart {}{}", s.config.name, grp_tag),
                    PaletteAction::RestartServer(s.config.key.clone()),
                ));
            } else {
                items.push((
                    format!("▶  Start {}{}", s.config.name, grp_tag),
                    PaletteAction::StartServer(s.config.key.clone()),
                ));
            }
            items.push((
                format!("📜  View logs: {}", s.config.name),
                PaletteAction::SelectTab(s.config.key.clone()),
            ));

            // Custom actions
            for act in &s.config.actions {
                items.push((
                    format!("⚡  {} [{}]", act.label, s.config.name),
                    PaletteAction::RunAction {
                        key: s.config.key.clone(),
                        command: act.command.clone(),
                    },
                ));
            }

            // Links
            for lnk in &s.config.links {
                items.push((
                    format!("🔗  Open {} ({})", lnk.label, lnk.url),
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
                format!("▶  Start Group: {}", g),
                PaletteAction::StartGroup(g.clone()),
            ));
            items.push((
                format!("⏹  Stop Group: {}", g),
                PaletteAction::StopGroup(g.clone()),
            ));
        }

        // 3. Global actions
        items.push((
            "▶  Start All Servers".to_string(),
            PaletteAction::StartAll,
        ));
        items.push((
            "⏹  Stop All Servers".to_string(),
            PaletteAction::StopAll,
        ));

        // 4. Color Themes
        for thm in ThemeMode::all() {
            items.push((
                format!("🎨  Switch Theme: {} {}", thm.emoji(), thm.name()),
                PaletteAction::SetTheme(thm.id_str().to_string()),
            ));
        }

        // 5. Autostart Actions
        let app_auto = crate::autostart::is_app_autostart_enabled();
        items.push((
            format!("🚀  App Autostart on System Login: {}", if app_auto { "Disable" } else { "Enable" }),
            PaletteAction::ToggleAppAutostart,
        ));

        for s in &self.services {
            items.push((
                format!(
                    "🚀  Server Autostart on Launch: {} ({})",
                    s.config.name,
                    if s.config.autostart { "Disable" } else { "Enable" }
                ),
                PaletteAction::ToggleServerAutostart(s.config.key.clone()),
            ));
        }

        items
    }

    fn handle_global_shortcuts(&mut self, ctx: &Context) {
        // Ctrl+P: Quick Launch
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::P))
            || ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::P))
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
            if let Ok(listeners) = res {
                if let Ok(mut map) = listeners_arc.lock() {
                    map.insert(host_clone.id.clone(), listeners);
                }
            }
            if let Ok(mut scanning) = scanning_arc.lock() {
                scanning.remove(&host_clone.id);
            }
        });
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

    fn run_remote_ssh_command(&self, host: &SshRemoteHost, cmd_str: &str) {
        let logs = self.get_remote_logs(&host.id);
        let prompt = format!("{}> {}", host.user, cmd_str);
        crate::service::push_log_line(&prompt, &logs, 1000, crate::service::LogKind::Stdin);

        let host_clone = host.clone();
        let cmd_owned = cmd_str.to_string();
        let logs_clone = logs.clone();

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
                }
                Err(e) => {
                    crate::service::push_log_line(
                        &format!("[error] Failed to start SSH process: {}", e),
                        &logs_clone,
                        1000,
                        crate::service::LogKind::Error,
                    );
                }
            }
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

            // Top: App Icon / Logo
            if let Some(tex) = &self.icon_texture {
                ui.image((tex.id(), egui::vec2(26.0, 26.0)));
            } else {
                ui.label(RichText::new("🚀").size(20.0));
            }
            ui.add_space(10.0);

            // 1. This PC (Local) Tile
            let is_local_selected = self.selected_machine == "local";
            let running_count = self
                .services
                .iter()
                .filter(|s| s.state == ServiceState::Running)
                .count();

            let local_tooltip = if running_count > 0 {
                format!("🖥 This PC (Local)\n● {} active server{} running", running_count, if running_count > 1 { "s" } else { "" })
            } else {
                "🖥 This PC (Local)\nNo servers running".to_string()
            };

            let (local_rect, local_resp) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::click());
            let local_bg = if is_local_selected {
                p.accent
            } else if local_resp.hovered() {
                p.bg_card_active
            } else {
                p.bg_card
            };
            let local_stroke = if is_local_selected {
                Stroke::NONE
            } else {
                Stroke::new(1.0_f32, p.border)
            };

            ui.painter().rect(local_rect, Rounding::same(8.0), local_bg, local_stroke);
            let local_icon_col = if is_local_selected { Color32::WHITE } else { p.text_primary };
            ui.painter().text(
                local_rect.center(),
                egui::Align2::CENTER_CENTER,
                "🖥",
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
            // Subtle horizontal separator line
            let (sep_rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 1.0), egui::Sense::hover());
            ui.painter().rect_filled(sep_rect, Rounding::ZERO, p.border.gamma_multiply(0.6));
            ui.add_space(8.0);

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
                                "🌐 {}\n{}@{}:{}\nStatus: {}\nRight-click for options",
                                host.name, host.user, host.host, host.port, status_text
                            );

                            let (host_rect, host_resp) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::click());
                            let host_bg = if is_selected {
                                p.accent
                            } else if host_resp.hovered() {
                                p.bg_card_active
                            } else {
                                p.bg_card
                            };
                            let host_stroke = if is_selected {
                                Stroke::NONE
                            } else {
                                Stroke::new(1.0_f32, p.border)
                            };

                            ui.painter().rect(host_rect, Rounding::same(8.0), host_bg, host_stroke);
                            let host_icon_col = if is_selected { Color32::WHITE } else { p.text_secondary };
                            ui.painter().text(
                                host_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "🌐",
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
                                if ui.button("⚙ Edit Host").clicked() {
                                    host_to_edit = Some(host.clone());
                                    ui.close_menu();
                                }
                                if ui.button("🔄 Test Connection").clicked() {
                                    host_to_test = Some(host.clone());
                                    ui.close_menu();
                                }
                                if ui.button("↗ Launch External Terminal").clicked() {
                                    host_to_term = Some(host.clone());
                                    ui.close_menu();
                                }
                                ui.separator();
                                if ui.button(RichText::new("🗑 Delete Host").color(Color32::from_rgb(239, 68, 68))).clicked() {
                                    host_to_delete = Some(host.id.clone());
                                    ui.close_menu();
                                }
                            });

                            ui.add_space(6.0);
                        }

                        // Add SSH Host button tile
                        let (add_rect, add_resp) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::click());
                        let add_bg = if add_resp.hovered() { p.bg_card_active } else { Color32::TRANSPARENT };
                        let add_stroke = Stroke::new(1.0_f32, if add_resp.hovered() { p.accent } else { p.border.gamma_multiply(0.6) });
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
                        let imp_stroke = Stroke::new(1.0_f32, if imp_resp.hovered() { p.accent } else { p.border.gamma_multiply(0.6) });
                        ui.painter().rect(imp_rect, Rounding::same(8.0), imp_bg, imp_stroke);
                        ui.painter().text(
                            imp_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "📥",
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
        let is_connecting = {
            let map = self.remote_connecting.lock().unwrap();
            map.contains(&host.id)
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
            // Dashboard Header Bar
            Frame::none()
                .fill(Color32::from_rgb(20, 23, 31))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 43, 58)))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(16.0, 12.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("🌐").size(24.0));
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&host.name)
                                        .size(18.0)
                                        .strong()
                                        .color(Color32::from_rgb(235, 238, 245)),
                                );
                                ui.add_space(8.0);
                                if is_connecting {
                                    ui.label(
                                        RichText::new("○ Connecting...")
                                            .size(11.0)
                                            .color(Color32::from_rgb(250, 204, 21)),
                                    );
                                } else if let Some(Ok(info)) = &status {
                                    ui.label(
                                        RichText::new(format!("● Online ({})", info))
                                            .size(11.0)
                                            .strong()
                                            .color(Color32::from_rgb(0, 210, 160)),
                                    );
                                } else if let Some(Err(err)) = &status {
                                    ui.label(
                                        RichText::new(format!("● Connection Error: {}", err))
                                            .size(11.0)
                                            .color(Color32::from_rgb(239, 68, 68)),
                                    );
                                } else {
                                    ui.label(
                                        RichText::new("○ Idle")
                                            .size(11.0)
                                            .color(Color32::from_rgb(130, 138, 155)),
                                    );
                                }
                            });

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
                                    .color(Color32::from_rgb(120, 128, 145)),
                            );
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn_edit = egui::Button::new(
                                RichText::new("⚙ Edit")
                                    .size(11.5)
                                    .color(Color32::from_rgb(200, 205, 215)),
                            )
                            .fill(Color32::from_rgb(32, 36, 48))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(50, 56, 75)))
                            .rounding(Rounding::same(4.0));
                            if ui.add(btn_edit).clicked() {
                                self.ssh_host_modal.open_edit(&host);
                            }

                            let btn_reconnect = egui::Button::new(
                                RichText::new("🔄 Reconnect")
                                    .size(11.5)
                                    .color(Color32::from_rgb(200, 205, 215)),
                            )
                            .fill(Color32::from_rgb(32, 36, 48))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(50, 56, 75)))
                            .rounding(Rounding::same(4.0));
                            if ui.add(btn_reconnect).clicked() {
                                self.test_remote_host(&host);
                                self.trigger_remote_scan(&host);
                            }

                            let btn_ext = egui::Button::new(
                                RichText::new("↗ External Terminal")
                                    .size(11.5)
                                    .strong()
                                    .color(Color32::from_rgb(96, 165, 250)),
                            )
                            .fill(Color32::from_rgb(20, 30, 48))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(59, 130, 246)))
                            .rounding(Rounding::same(4.0));
                            if ui.add(btn_ext).on_hover_text("Open desktop terminal connected via SSH").clicked() {
                                let _ = launch_external_ssh_terminal(&host);
                            }

                            let scan_label = if is_scanning { "Scanning..." } else { "Scan Ports" };
                            let btn_scan = egui::Button::new(
                                RichText::new(scan_label)
                                    .size(11.5)
                                    .color(Color32::from_rgb(162, 155, 254)),
                            )
                            .fill(Color32::from_rgb(34, 30, 52))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(108, 92, 231)))
                            .rounding(Rounding::same(4.0));
                            if ui.add(btn_scan).clicked() {
                                self.trigger_remote_scan(&host);
                            }
                        });
                    });
                });

            ui.add_space(8.0);

            // Trigger autostart scripts for this SSH host if connected
            if let Some(Ok(_)) = &status {
                for sc in &self.scripts {
                    if sc.autostart && sc.host_id.as_deref() == Some(&host.id) {
                        if !self.script_executions.contains_key(&sc.id) {
                            let mut exec = ScriptExecution::new(sc.id.clone());
                            execute_remote_script(&host, sc, &mut exec);
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
                    ui.separator();
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
        let is_scanning = {
            let map = self.remote_scanning.lock().unwrap();
            map.contains(&host.id)
        };
        let listeners = {
            let map = self.remote_listeners.lock().unwrap();
            map.get(&host.id).cloned().unwrap_or_default()
        };

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("REMOTE SERVERS")
                        .size(11.5)
                        .strong()
                        .color(Color32::from_rgb(148, 163, 184)),
                );
                ui.label(
                    RichText::new(format!("({})", listeners.len()))
                        .size(11.0)
                        .color(Color32::from_rgb(100, 110, 130)),
                );
            });
            ui.add_space(4.0);

            let search_q = self.remote_search_query.entry(host.id.clone()).or_default();
            ui.add(
                egui::TextEdit::singleline(search_q)
                    .hint_text("🔍 Filter remote servers...")
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

            if listeners.is_empty() {
                Frame::none()
                    .fill(Color32::from_rgb(15, 17, 23))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(35, 40, 54)))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if is_scanning {
                            ui.label(RichText::new("Scanning remote listening ports (ss -tulpn)...").size(11.0).color(Color32::from_rgb(250, 204, 21)));
                        } else {
                            ui.label(RichText::new("No active listening ports detected on remote PC. Click 'Scan Ports' above.").size(11.0).color(Color32::from_rgb(120, 128, 145)));
                        }
                    });
            } else if visible_listeners.is_empty() {
                ui.label(RichText::new(format!("No servers matching \"{}\"", q)).size(11.0).color(Color32::from_rgb(120, 128, 145)));
            } else {
                ScrollArea::vertical()
                    .id_salt(format!("remote_listeners_scroll_{}", host.id))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_max_width(ui.available_width());
                        for l in &visible_listeners {
                            Frame::none()
                                .fill(Color32::from_rgb(15, 17, 23))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(35, 40, 54)))
                                .rounding(Rounding::same(6.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.set_max_width(ui.available_width());
                                    // Row 1: Port badge + Proto + Name
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
                                        let name_w = (ui.available_width() - 4.0).max(40.0);
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
                                    });

                                    // Row 2: PID and command
                                    if let Some(pid) = l.pid {
                                        let mut desc = format!("PID {}", pid);
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

                                    // Row 3: Action Buttons
                                    ui.horizontal(|ui| {
                                        let kill_btn = egui::Button::new(
                                            RichText::new("🛑 Kill").size(10.0).color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(239, 68, 68))
                                        .rounding(Rounding::same(3.0));
                                        if ui.add(kill_btn).on_hover_text("Kill this remote process via SSH").clicked() {
                                            let kill_cmd = if let Some(pid) = l.pid {
                                                format!("kill -9 {}", pid)
                                            } else {
                                                format!("fuser -k -n tcp {} 2>/dev/null || true", l.port)
                                            };
                                            self.run_remote_ssh_command(host, &kill_cmd);
                                        }

                                        let test_btn = egui::Button::new(
                                            RichText::new("🌐 Curl").size(10.0).color(Color32::from_rgb(56, 189, 248)),
                                        )
                                        .fill(Color32::from_rgb(24, 32, 48))
                                        .rounding(Rounding::same(3.0));
                                        if ui.add(test_btn).on_hover_text("Test HTTP response on remote host (curl -I)").clicked() {
                                            let curl_cmd = format!("curl -I -s --connect-timeout 2 http://localhost:{} | head -n 5", l.port);
                                            self.run_remote_ssh_command(host, &curl_cmd);
                                        }

                                        let ps_btn = egui::Button::new(
                                            RichText::new("🔍 Info").size(10.0).color(Color32::from_rgb(250, 204, 21)),
                                        )
                                        .fill(Color32::from_rgb(34, 30, 20))
                                        .rounding(Rounding::same(3.0));
                                        if ui.add(ps_btn).on_hover_text("Inspect process details on remote host").clicked() {
                                            let ps_cmd = if let Some(pid) = l.pid {
                                                format!("ps -u -p {} 2>/dev/null", pid)
                                            } else {
                                                format!("lsof -i :{} 2>/dev/null", l.port)
                                            };
                                            self.run_remote_ssh_command(host, &ps_cmd);
                                        }
                                    });
                                });
                            ui.add_space(4.0);
                        }
                    });
            }
        });
    }

    fn render_remote_servers_right_panel(&mut self, ui: &mut Ui, host: &SshRemoteHost) {
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
                            self.run_remote_ssh_command(host, qcmd);
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
                .fill(Color32::from_rgb(10, 12, 17))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(35, 40, 54)))
                .rounding(Rounding::same(6.0))
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
                            self.run_remote_ssh_command(host, &cmd);
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
    ui.painter().circle_filled(rect.center(), 5.5, color.gamma_multiply(0.25));
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

fn render_group_badge(ui: &mut Ui, text: &str) {
    let visuals = ui.visuals();
    let bg = visuals.faint_bg_color;
    let stroke_col = visuals.widgets.inactive.bg_fill;
    let text_col = visuals.selection.bg_fill;
    Frame::none()
        .fill(bg)
        .stroke(Stroke::new(1.0_f32, stroke_col))
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .size(11.0)
                    .color(text_col)
                    .strong(),
            );
        });
}

fn render_metric_pill(ui: &mut Ui, icon: &str, label: &str, text_color: Color32) {
    let bg_color = ui.visuals().widgets.inactive.bg_fill;
    render_metric_pill_custom(ui, icon, label, text_color, bg_color);
}

fn render_metric_pill_custom(ui: &mut Ui, icon: &str, label: &str, text_color: Color32, bg_color: Color32) {
    Frame::none()
        .fill(bg_color)
        .stroke(Stroke::new(1.0_f32, text_color.gamma_multiply(0.35)))
        .rounding(Rounding::same(5.0))
        .inner_margin(egui::Margin::symmetric(7.0, 3.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).size(11.0));
                ui.label(RichText::new(label).size(11.0).color(text_color).strong());
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

        self.handle_global_shortcuts(ctx);
        let p = self.theme.palette();

        // ═════════════════════════════════════════════════════════════════════
        // TOP HEADER BAR (Pinned to Top, exact match to screenshot)
        // ═════════════════════════════════════════════════════════════════════
        egui::TopBottomPanel::top("top_header")
            .frame(
                Frame::none()
                    .fill(p.bg_header)
                    .inner_margin(egui::Margin {
                        left: 18.0,
                        right: 18.0,
                        top: 14.0,
                        bottom: 12.0,
                    }),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Left: App Icon + Server Launcher title & status line
                    if let Some(texture) = &self.icon_texture {
                        ui.add(
                            egui::Image::from_texture(texture)
                                .fit_to_exact_size(egui::vec2(36.0, 36.0)),
                        );
                        ui.add_space(6.0);
                    }
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Server Launcher")
                                .size(20.0)
                                .strong()
                                .color(p.text_primary),
                        );
                        ui.add_space(2.0);
                        let subtitle = if self.selected_machine == "local" {
                            format!(
                                "this pc: {}    servers: {}",
                                self.lan_ip,
                                self.services.len()
                            )
                        } else if let Some(h) = self.ssh_hosts.iter().find(|h| h.id == self.selected_machine) {
                            format!(
                                "ssh remote: {} ({}:{})",
                                h.name,
                                h.host,
                                h.port
                            )
                        } else {
                            format!("this pc: {}", self.lan_ip)
                        };
                        ui.label(
                            RichText::new(subtitle)
                                .size(11.5)
                                .color(p.text_muted),
                        );
                    });

                    // Right: Actions buttons
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // + Add Server
                        let btn_add = egui::Button::new(
                            RichText::new("+ Add Server")
                                .size(12.0)
                                .strong()
                                .color(p.accent_text),
                        )
                        .fill(p.accent)
                        .rounding(Rounding::same(4.0))
                        .min_size(egui::vec2(95.0, 26.0));
                        if ui.add(btn_add).clicked() {
                            self.add_edit_modal.open_new();
                        }

                        // App Boot/Startup toggle button
                        let is_boot = crate::autostart::is_app_autostart_enabled();
                        let (boot_label, boot_color) = if is_boot {
                            ("🚀 Boot: ON", p.success)
                        } else {
                            ("🚀 Boot: OFF", p.text_muted)
                        };
                        let btn_boot = egui::Button::new(
                            RichText::new(boot_label)
                                .size(11.5)
                                .strong()
                                .color(boot_color),
                        )
                        .fill(p.bg_button)
                        .stroke(Stroke::new(1.0_f32, if is_boot { p.success } else { p.border }))
                        .rounding(Rounding::same(4.0))
                        .min_size(egui::vec2(86.0, 26.0));
                        if ui.add(btn_boot)
                            .on_hover_text(if is_boot {
                                "App autostart is ON: Server Launcher runs on system login/boot. Click to turn OFF."
                            } else {
                                "App autostart is OFF. Click to make Server Launcher run on system login/boot."
                            })
                            .clicked()
                        {
                            let new_state = !is_boot;
                            let _ = crate::autostart::set_app_autostart(new_state);
                            self.app_autostart = new_state;
                            self.persist_config();
                        }

                        // Theme Selector
                        let prev_theme = self.theme;
                        egui::ComboBox::from_id_salt("top_theme_picker")
                            .selected_text(
                                RichText::new(format!("{} {}", self.theme.emoji(), self.theme.name()))
                                    .size(11.5)
                                    .color(p.text_primary),
                            )
                            .width(148.0)
                            .show_ui(ui, |ui| {
                                for &thm in ThemeMode::all() {
                                    let label = format!("{} {}", thm.emoji(), thm.name());
                                    ui.selectable_value(&mut self.theme, thm, RichText::new(label).size(11.5));
                                }
                            });
                        if self.theme != prev_theme {
                            ctx.set_visuals(self.theme.palette().egui_visuals());
                            self.persist_config();
                        }

                        // Quick Launch (Ctrl+P)
                        let btn_pal = egui::Button::new(
                            RichText::new("⚡ Quick Launch (Ctrl+P)")
                                .size(12.0)
                                .color(p.accent_light),
                        )
                        .fill(p.bg_button)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .rounding(Rounding::same(4.0))
                        .min_size(egui::vec2(165.0, 26.0));
                        if ui.add(btn_pal).clicked() {
                            self.palette_modal.open = true;
                            self.palette_modal.query.clear();
                        }

                        // Stop all
                        let btn_stop_all = egui::Button::new(
                            RichText::new("Stop all")
                                .size(12.0)
                                .color(p.text_secondary),
                        )
                        .fill(p.bg_button)
                        .rounding(Rounding::same(4.0))
                        .min_size(egui::vec2(75.0, 26.0));
                        if ui.add(btn_stop_all).clicked() {
                            self.stop_all();
                        }

                        // Start all
                        let btn_start_all = egui::Button::new(
                            RichText::new("Start all")
                                .size(12.0)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(p.success)
                        .rounding(Rounding::same(4.0))
                        .min_size(egui::vec2(75.0, 26.0));
                        if ui.add(btn_start_all).clicked() {
                            self.start_all();
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
            .show_separator_line(true)
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
            // LEFT SIDEBAR: Full Window Height (~380px width)
            // ═════════════════════════════════════════════════════════════════
            egui::SidePanel::left("left_sidebar")
                .resizable(false)
                .exact_width(380.0)
                .show_separator_line(true)
                .frame(
                    Frame::none()
                        .fill(p.bg_sidebar)
                        .inner_margin(egui::Margin {
                            left: 14.0,
                            right: 12.0,
                            top: 8.0,
                            bottom: 12.0,
                        }),
                )
                .show(ctx, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_width(ui.available_width());
                    ui.set_max_width(ui.available_width());
                    self.render_left_panel(ui);
                });

            // ═════════════════════════════════════════════════════════════════
            // RIGHT MAIN AREA: Full Window Height & Remaining Width
            // ═════════════════════════════════════════════════════════════════
            egui::CentralPanel::default()
                .frame(
                    Frame::none()
                        .fill(p.bg_main)
                        .inner_margin(egui::Margin {
                            left: 6.0,
                            right: 18.0,
                            top: 8.0,
                            bottom: 14.0,
                        }),
                )
                .show(ctx, |ui| {
                    self.render_right_panel(ui);
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
                            top: 8.0,
                            bottom: 14.0,
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
                ctx.set_visuals(thm.palette().egui_visuals());
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
            if let Some(existing) = self.scripts.iter_mut().find(|s| s.id == script.id) {
                *existing = script.clone();
            } else {
                self.scripts.push(script.clone());
            }
            self.persist_config();
        }

        if let Some(cmd) = render_command_modal(ctx, &mut self.command_modal, &p) {
            if let Some(existing) = self.saved_commands.iter_mut().find(|c| c.id == cmd.id) {
                *existing = cmd.clone();
            } else {
                self.saved_commands.push(cmd.clone());
            }
            self.persist_config();
        }

        if let Some(service_def) = render_new_service_modal(ctx, &mut self.systemd_modal, &p) {
            let is_remote = self.selected_machine != "local";
            if is_remote {
                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == self.selected_machine).cloned() {
                    match create_remote_service(&host, &service_def) {
                        Ok(msg) => {
                            self.service_action_msg = Some(msg);
                            if let Ok(services) = list_remote_services(&host, service_def.is_user) {
                                self.remote_services.insert(host.id.clone(), services);
                            }
                        }
                        Err(err) => {
                            self.service_action_msg = Some(format!("Error: {}", err));
                        }
                    }
                }
            } else {
                match create_local_service(&service_def) {
                    Ok(msg) => {
                        self.service_action_msg = Some(msg);
                        if let Ok(services) = list_local_services(service_def.is_user) {
                            self.local_services = services;
                        }
                    }
                    Err(err) => {
                        self.service_action_msg = Some(format!("Error: {}", err));
                    }
                }
            }
        }

        let is_scanning = self.scanner.is_scanning.lock().map(|s| *s).unwrap_or(false);
        if self.close_confirm_open {
            // Keep repainting while confirmation dialog is open
            ctx.request_repaint();
        } else if is_scanning {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
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
        let avail_w = ui.available_width().max(60.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.spacing_mut().button_padding = egui::vec2(4.0, 4.0);
            let tabs = [
                (SidebarTab::Servers, SidebarTab::Servers.label()),
                (SidebarTab::Scripts, SidebarTab::Scripts.label()),
                (SidebarTab::Commands, SidebarTab::Commands.label()),
                (SidebarTab::SystemMD, SidebarTab::SystemMD.label()),
                (SidebarTab::Sensors, SidebarTab::Sensors.label()),
            ];

            let btn_w = ((avail_w - 14.0) / 5.0).max(10.0);

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
                let stroke = if is_active {
                    Stroke::new(1.0_f32, p.accent)
                } else {
                    Stroke::new(1.0_f32, p.border)
                };

                let btn = egui::Button::new(
                    RichText::new(label)
                        .size(10.0)
                        .strong()
                        .color(fg),
                )
                .fill(bg)
                .stroke(stroke)
                .rounding(Rounding::same(5.0))
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
            ui.separator();
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
                    .hint_text("🔍 Filter scripts...")
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

                        let card_border = if is_selected {
                            Stroke::new(1.5_f32, p.accent)
                        } else {
                            Stroke::new(1.0_f32, p.border)
                        };

                        Frame::none()
                            .fill(if is_selected { p.bg_card } else { p.bg_sidebar })
                            .stroke(card_border)
                            .rounding(Rounding::same(6.0))
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
                                                RichText::new("⚡ Startup")
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
                                            RichText::new("⏹ Stop").size(10.5).color(Color32::WHITE),
                                        )
                                        .fill(p.danger)
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_stop).clicked() {
                                            to_stop = Some(s.id.clone());
                                        }
                                    } else {
                                        let btn_run = egui::Button::new(
                                            RichText::new("▶ Run").size(10.5).color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(16, 185, 129))
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_run).clicked() {
                                            to_run = Some(s.clone());
                                        }
                                    }

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let btn_del = egui::Button::new(
                                            RichText::new("🗑").size(10.5).color(p.text_muted),
                                        )
                                        .fill(Color32::TRANSPARENT);
                                        if ui.add(btn_del).on_hover_text("Delete Script").clicked() {
                                            to_delete = Some(s.id.clone());
                                        }

                                        let btn_edit = egui::Button::new(
                                            RichText::new("✏").size(10.5).color(p.text_muted),
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
                        self.selected_script_id = Some(s.id.clone());
                        let mut exec = self
                            .script_executions
                            .remove(&s.id)
                            .unwrap_or_else(|| ScriptExecution::new(s.id.clone()));

                        if let Some(hid) = host_id {
                            if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                execute_remote_script(&host, &s, &mut exec);
                            }
                        } else {
                            execute_local_script(&s, &mut exec);
                        }
                        self.script_executions.insert(s.id.clone(), exec);
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
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("📜").size(20.0));
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
                                                RichText::new("⚡ Runs at Startup")
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
                                    let btn_edit = egui::Button::new(RichText::new("✏ Edit").size(11.0).color(p.text_secondary))
                                        .fill(p.bg_button);
                                    if ui.add(btn_edit).clicked() {
                                        self.script_modal.open_edit(&s);
                                    }

                                    let btn_clear = egui::Button::new(RichText::new("🧹 Clear Logs").size(11.0).color(p.text_secondary))
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
                                            RichText::new("⏹ Stop Script").size(11.5).strong().color(Color32::WHITE),
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
                                            RichText::new("▶ Run Script").size(11.5).strong().color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(16, 185, 129))
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_run).clicked() {
                                            let mut exec = self
                                                .script_executions
                                                .remove(&s.id)
                                                .unwrap_or_else(|| ScriptExecution::new(s.id.clone()));

                                            if let Some(hid) = host_id {
                                                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                                    execute_remote_script(&host, &s, &mut exec);
                                                }
                                            } else {
                                                execute_local_script(&s, &mut exec);
                                            }
                                            self.script_executions.insert(s.id.clone(), exec);
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
                        .fill(Color32::from_rgb(12, 14, 20))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 36, 48)))
                        .rounding(Rounding::same(6.0))
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
                                                    RichText::new("Script ready. Click '▶ Run Script' to execute.")
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
                                            RichText::new("Script ready. Click '▶ Run Script' to execute.")
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
                    ui.label(RichText::new("📜 Custom Scripts").size(16.0).strong().color(p.text_primary));
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
                    .hint_text("🔍 Filter commands...")
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
                    let mut to_run_cmd = None;
                    let mut to_edit = None;
                    let mut to_delete = None;

                    for c in &commands {
                        Frame::none()
                            .fill(p.bg_sidebar)
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .rounding(Rounding::same(6.0))
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
                                    let btn_run = egui::Button::new(
                                        RichText::new("▶ Run").size(10.5).color(Color32::WHITE),
                                    )
                                    .fill(Color32::from_rgb(16, 185, 129))
                                    .rounding(Rounding::same(4.0));
                                    if ui.add(btn_run).on_hover_text("Execute this command in terminal").clicked() {
                                        to_run_cmd = Some(c.command.clone());
                                    }

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let btn_del = egui::Button::new(RichText::new("🗑").size(10.5).color(p.text_muted))
                                            .fill(Color32::TRANSPARENT);
                                        if ui.add(btn_del).on_hover_text("Delete Command").clicked() {
                                            to_delete = Some(c.id.clone());
                                        }

                                        let btn_edit = egui::Button::new(RichText::new("✏").size(10.5).color(p.text_muted))
                                            .fill(Color32::TRANSPARENT);
                                        if ui.add(btn_edit).on_hover_text("Edit Command").clicked() {
                                            to_edit = Some(c.clone());
                                        }

                                        let btn_copy = egui::Button::new(RichText::new("📋").size(10.5).color(p.text_muted))
                                            .fill(Color32::TRANSPARENT);
                                        if ui.add(btn_copy).on_hover_text("Copy to Clipboard").clicked() {
                                            ui.output_mut(|o| o.copied_text = c.command.clone());
                                        }
                                    });
                                });
                            });

                        ui.add_space(6.0);
                    }

                    if let Some(cmd_str) = to_run_cmd {
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
            TerminalCommandStatus::Idle => "● Ready".to_string(),
            TerminalCommandStatus::Running(c) => format!("▶ Running: {}", c),
            TerminalCommandStatus::Success(code) => format!("✓ Exit code {}", code),
            TerminalCommandStatus::Failed(err) => format!("✘ Failed: {}", err),
        }).unwrap_or_default();

        let title = if let Some(h) = &host_opt {
            format!("🌐 Remote SSH Terminal ({}@{}:{})", h.user, h.host, h.port)
        } else {
            "💻 Local Interactive Terminal".to_string()
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
                .stroke(Stroke::new(1.0_f32, p.border))
                .rounding(Rounding::same(6.0))
                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("⚡").size(20.0));
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&title).size(13.5).strong().color(p.text_primary));
                                ui.label(
                                    RichText::new(format!("📂 {}", cwd_display))
                                        .size(11.0)
                                        .color(Color32::from_rgb(147, 197, 253))
                                        .monospace(),
                                );
                            });
                            ui.label(
                                RichText::new(&stat_msg)
                                    .size(11.0)
                                    .color(if is_running { Color32::from_rgb(16, 185, 129) } else { p.text_muted }),
                            );
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn_clear = egui::Button::new(RichText::new("🧹 Clear (Ctrl+L)").size(11.0).color(p.text_secondary))
                                .fill(p.bg_button);
                            if ui.add(btn_clear).clicked() {
                                term.clear();
                                term.request_focus = true;
                            }

                            if is_running {
                                let btn_stop = egui::Button::new(
                                    RichText::new("⏹ Interrupt (Ctrl+C)").size(11.0).strong().color(Color32::WHITE),
                                )
                                .fill(p.danger)
                                .rounding(Rounding::same(4.0));
                                if ui.add(btn_stop).clicked() {
                                    term.interrupt();
                                    term.request_focus = true;
                                }
                            } else if host_opt.is_none() {
                                let btn_ext = egui::Button::new(RichText::new("💻 Native Terminal").size(11.0).color(p.text_secondary))
                                    .fill(p.bg_button);
                                if ui.add(btn_ext).on_hover_text("Open native system terminal window in current directory").clicked() {
                                    let _ = crate::commands::open_system_terminal(&term.cwd);
                                }
                            }
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
                .fill(Color32::from_rgb(12, 14, 20))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 36, 48)))
                .rounding(Rounding::same(6.0))
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
                            "▶ stdin > ".to_string()
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
                                RichText::new("⏹ Stop").size(11.0).strong().color(Color32::WHITE),
                            )
                            .fill(p.danger)
                            .rounding(Rounding::same(4.0));
                            if ui.add(btn_stop).on_hover_text("Interrupt running process (Ctrl+C)").clicked() {
                                term.interrupt();
                                resp.request_focus();
                            }
                        } else {
                            let btn_run = egui::Button::new(
                                RichText::new("▶ Run").size(11.0).strong().color(Color32::WHITE),
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

        ui.set_width(ui.available_width());
        ui.set_max_width(ui.available_width());
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.set_max_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("SYSTEMD SERVICES").size(12.0).strong().color(p.text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_new = egui::Button::new(
                        RichText::new("+ New Service").size(11.0).strong().color(Color32::WHITE),
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
                        if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                            if let Ok(s) = list_remote_services(&host, true) {
                                self.remote_services.insert(hid.to_string(), s);
                            }
                        }
                    } else {
                        self.local_services_user = true;
                        if let Ok(s) = list_local_services(true) {
                            self.local_services = s;
                        }
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
                        if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                            if let Ok(s) = list_remote_services(&host, false) {
                                self.remote_services.insert(hid.to_string(), s);
                            }
                        }
                    } else {
                        self.local_services_user = false;
                        if let Ok(s) = list_local_services(false) {
                            self.local_services = s;
                        }
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_ref = egui::Button::new(RichText::new("🔄").size(11.0).color(p.text_secondary))
                        .fill(p.bg_button);
                    if ui.add(btn_ref).on_hover_text("Refresh systemd services list").clicked() {
                        if let Some(hid) = host_id {
                            if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                if let Ok(s) = list_remote_services(&host, user_mode) {
                                    self.remote_services.insert(hid.to_string(), s);
                                }
                            }
                        } else {
                            if let Ok(s) = list_local_services(user_mode) {
                                self.local_services = s;
                            }
                        }
                    }
                });
            });

            ui.add_space(6.0);

            // Auto-load services on first open
            if host_id.is_none() && !self.local_services_loaded {
                self.local_services_loaded = true;
                if let Ok(s) = list_local_services(self.local_services_user) {
                    self.local_services = s;
                }
            } else if let Some(hid) = host_id {
                if !self.remote_services.contains_key(hid) {
                    if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                        if let Ok(s) = list_remote_services(&host, user_mode) {
                            self.remote_services.insert(hid.to_string(), s);
                        }
                    }
                }
            }

            // Search filter
            ui.add(
                egui::TextEdit::singleline(&mut self.service_search_query)
                    .hint_text("🔍 Filter services...")
                    .desired_width(ui.available_width()),
            );

            ui.add_space(8.0);

            let q = self.service_search_query.trim().to_lowercase();
            let services_list: Vec<SystemdService> = if let Some(hid) = host_id {
                self.remote_services.get(hid).cloned().unwrap_or_default()
            } else {
                self.local_services.clone()
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
                    ui.label(RichText::new("No systemd services found").size(12.0).color(p.text_muted));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Click '🔄' to scan or switch scope.").size(11.0).color(p.text_muted));
                });
                return;
            }

            ScrollArea::vertical()
                .id_salt("systemd_services_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_max_width(ui.available_width());
                    let mut selected = None;

                    for s in &filtered_services {
                        let is_active = s.is_active();
                        let is_failed = s.is_failed();
                        let is_selected = self.selected_service.as_deref() == Some(&s.name);

                        let card_border = if is_selected {
                            Stroke::new(1.5_f32, p.accent)
                        } else {
                            Stroke::new(1.0_f32, p.border)
                        };

                        Frame::none()
                            .fill(if is_selected { p.bg_card } else { p.bg_sidebar })
                            .stroke(card_border)
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.set_max_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    let status_dot = if is_active {
                                        RichText::new("●").size(12.0).color(Color32::from_rgb(16, 185, 129))
                                    } else if is_failed {
                                        RichText::new("✘").size(12.0).color(p.danger)
                                    } else {
                                        RichText::new("○").size(12.0).color(p.text_muted)
                                    };
                                    ui.label(status_dot);

                                    let status_w = 68.0;
                                    let name_w = (ui.available_width() - status_w - 6.0).max(40.0);

                                    let title = RichText::new(&s.name).size(12.0).strong().color(if is_selected { p.accent_light } else { p.text_primary });
                                    let name_resp = ui.allocate_ui_with_layout(
                                        egui::vec2(name_w, 16.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.set_max_width(name_w);
                                            ui.add(
                                                egui::Label::new(title)
                                                    .truncate()
                                                    .sense(egui::Sense::click()),
                                            )
                                            .on_hover_text(&s.name)
                                        },
                                    ).inner;

                                    if name_resp.clicked() {
                                        selected = Some(s.name.clone());
                                    }

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

                        ui.add_space(5.0);
                    }

                    if let Some(unit) = selected {
                        self.selected_service = Some(unit.clone());
                        self.service_action_msg = None;
                        // Fetch journalctl
                        if let Some(hid) = host_id {
                            if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                self.service_journal_logs = fetch_remote_journal(&host, &unit, user_mode, 100).unwrap_or_default();
                            }
                        } else {
                            self.service_journal_logs = fetch_local_journal(&unit, user_mode, 100).unwrap_or_default();
                        }
                        self.last_journal_refresh = Instant::now();
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

        let services_list: Vec<SystemdService> = if let Some(hid) = host_id {
            self.remote_services.get(hid).cloned().unwrap_or_default()
        } else {
            self.local_services.clone()
        };

        let selected_unit = self.selected_service.clone();
        let service_opt = selected_unit.as_ref().and_then(|u| services_list.iter().find(|s| &s.name == u).cloned());

        match service_opt {
            Some(s) => {
                let is_active = s.is_active();
                let is_failed = s.is_failed();

                ui.vertical(|ui| {
                    // Header Bar
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("⚙").size(22.0));
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&s.name).size(15.0).strong().color(p.text_primary));
                                        let status_tag = if is_active {
                                            RichText::new("● Active (Running)").size(11.0).strong().color(Color32::from_rgb(16, 185, 129))
                                        } else if is_failed {
                                            RichText::new("✘ Failed").size(11.0).strong().color(p.danger)
                                        } else {
                                            RichText::new("○ Inactive").size(11.0).color(p.text_muted)
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

                            // Control buttons row
                            let mut action_to_run = None;
                            ui.horizontal_wrapped(|ui| {
                                let btn_start = egui::Button::new(RichText::new("▶ Start").size(11.0).strong().color(Color32::WHITE))
                                    .fill(Color32::from_rgb(16, 185, 129))
                                    .rounding(Rounding::same(4.0));
                                if ui.add(btn_start).clicked() {
                                    action_to_run = Some("start");
                                }

                                let btn_stop = egui::Button::new(RichText::new("⏹ Stop").size(11.0).strong().color(Color32::WHITE))
                                    .fill(p.danger)
                                    .rounding(Rounding::same(4.0));
                                if ui.add(btn_stop).clicked() {
                                    action_to_run = Some("stop");
                                }

                                let btn_restart = egui::Button::new(RichText::new("🔄 Restart").size(11.0).color(p.text_primary))
                                    .fill(p.bg_button)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .rounding(Rounding::same(4.0));
                                if ui.add(btn_restart).clicked() {
                                    action_to_run = Some("restart");
                                }

                                let btn_enable = egui::Button::new(RichText::new("✅ Enable").size(11.0).color(p.text_primary))
                                    .fill(p.bg_button)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .rounding(Rounding::same(4.0));
                                if ui.add(btn_enable).clicked() {
                                    action_to_run = Some("enable");
                                }

                                let btn_disable = egui::Button::new(RichText::new("❌ Disable").size(11.0).color(p.text_primary))
                                    .fill(p.bg_button)
                                    .stroke(Stroke::new(1.0_f32, p.border))
                                    .rounding(Rounding::same(4.0));
                                if ui.add(btn_disable).clicked() {
                                    action_to_run = Some("disable");
                                }

                                let btn_reload = egui::Button::new(RichText::new("🔄 Reload Daemon").size(11.0).color(p.text_secondary))
                                    .fill(p.bg_button)
                                    .rounding(Rounding::same(4.0));
                                if ui.add(btn_reload).clicked() {
                                    action_to_run = Some("daemon-reload");
                                }
                            });

                            if let Some(act) = action_to_run {
                                if let Some(hid) = host_id {
                                    if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                        let res = control_remote_service(&host, &s.name, act, user_mode);
                                        match res {
                                            Ok(msg) => self.service_action_msg = Some(msg),
                                            Err(e) => self.service_action_msg = Some(format!("Error: {}", e)),
                                        }
                                        if let Ok(services) = list_remote_services(&host, user_mode) {
                                            self.remote_services.insert(hid.to_string(), services);
                                        }
                                        self.service_journal_logs = fetch_remote_journal(&host, &s.name, user_mode, 100).unwrap_or_default();
                                    }
                                } else {
                                    let res = control_local_service(&s.name, act, user_mode);
                                    match res {
                                        Ok(msg) => self.service_action_msg = Some(msg),
                                        Err(e) => self.service_action_msg = Some(format!("Error: {}", e)),
                                    }
                                    if let Ok(services) = list_local_services(user_mode) {
                                        self.local_services = services;
                                    }
                                    self.service_journal_logs = fetch_local_journal(&s.name, user_mode, 100).unwrap_or_default();
                                }
                            }
                        });

                    if let Some(msg) = &self.service_action_msg {
                        ui.add_space(6.0);
                        Frame::none()
                            .fill(p.bg_subtle)
                            .stroke(Stroke::new(1.0_f32, p.accent))
                            .rounding(Rounding::same(4.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new(msg).size(11.0).color(p.accent));
                            });
                    }

                    ui.add_space(8.0);

                    // Journalctl Logs Viewer
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Service Logs (journalctl -n 100)").size(12.0).strong().color(p.text_muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn_ref_log = egui::Button::new(RichText::new("🔄 Refresh Logs").size(11.0).color(p.text_secondary))
                                .fill(p.bg_button);
                            if ui.add(btn_ref_log).clicked() {
                                if let Some(hid) = host_id {
                                    if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                        self.service_journal_logs = fetch_remote_journal(&host, &s.name, user_mode, 100).unwrap_or_default();
                                    }
                                } else {
                                    self.service_journal_logs = fetch_local_journal(&s.name, user_mode, 100).unwrap_or_default();
                                }
                                self.last_journal_refresh = Instant::now();
                            }

                            // Refresh Interval Dropdown
                            let prev_interval = self.journal_refresh_interval;
                            let interval_label = match self.journal_refresh_interval {
                                0 => "⏱ Auto: Off".to_string(),
                                secs => format!("⏱ Auto: {}s", secs),
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
                            if let Some(hid) = host_id {
                                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                    self.service_journal_logs = fetch_remote_journal(&host, &s.name, user_mode, 100).unwrap_or_default();
                                }
                            } else {
                                self.service_journal_logs = fetch_local_journal(&s.name, user_mode, 100).unwrap_or_default();
                            }
                        }
                    }

                    ui.add_space(4.0);

                    let log_h = (ui.available_height() - 20.0).max(180.0);
                    Frame::none()
                        .fill(Color32::from_rgb(12, 14, 20))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 36, 48)))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.set_height(log_h);
                            ScrollArea::vertical()
                                .id_salt("journalctl_scroll")
                                .stick_to_bottom(true)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if self.service_journal_logs.trim().is_empty() {
                                        ui.label(RichText::new("No journal logs recorded for this service.").size(11.5).color(Color32::from_rgb(100, 110, 130)));
                                    } else {
                                        ui.label(
                                            RichText::new(&self.service_journal_logs)
                                                .size(11.0)
                                                .monospace()
                                                .color(Color32::from_rgb(220, 225, 235)),
                                        );
                                    }
                                });
                        });
                });
            }
            None => {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(RichText::new("⚙ SystemMD Services").size(16.0).strong().color(p.text_primary));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Select a systemd unit from the left panel to control (start/stop/restart) and view journalctl logs,\nor click '+ New Service' to create and enable a new systemd unit.")
                            .size(12.0)
                            .color(p.text_muted),
                    );
                });
            }
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // SENSORS TAB: Left Sidebar (Quick Overview Cards & Category Filters)
    // ═════════════════════════════════════════════════════════════════════════
    fn render_sensors_left_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.theme.palette();
        ui.set_width(ui.available_width());
        ui.set_max_width(ui.available_width());

        // Fetch or refresh sensors snapshot
        let snap = if let Some(hid) = host_id {
            self.remote_sensors_snapshots.get(hid).cloned().unwrap_or_default()
        } else {
            if self.last_sensors_refresh.elapsed() >= Duration::from_secs(self.sensors_refresh_interval.max(1) as u64) {
                self.sensors_snapshot = self.sensors_collector.collect();
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
                    let btn_ref = egui::Button::new(RichText::new("🔄").size(11.0).color(p.text_secondary))
                        .fill(p.bg_button);
                    if ui.add(btn_ref).on_hover_text("Refresh all sensors now").clicked() {
                        if let Some(hid) = host_id {
                            if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                if let Ok(s) = crate::sensors::query_remote_sensors(&host) {
                                    self.remote_sensors_snapshots.insert(hid.to_string(), s);
                                }
                            }
                        } else {
                            self.sensors_snapshot = self.sensors_collector.collect();
                        }
                        self.last_sensors_refresh = Instant::now();
                    }

                    // Auto-refresh interval dropdown
                    let interval_label = match self.sensors_refresh_interval {
                        0 => "⏱ Off".to_string(),
                        s => format!("⏱ {}s", s),
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
                    .hint_text("🔍 Filter sensors & temps...")
                    .desired_width(ui.available_width()),
            );

            ui.add_space(8.0);

            // Category Filter Pills
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                let categories = ["All", "CPU", "RAM", "GPU", "Storage", "Network", "Temps"];
                for cat in categories {
                    let is_active = self.sensors_category_filter == cat;
                    let bg = if is_active { p.accent } else { p.bg_button };
                    let fg = if is_active { p.accent_text } else { p.text_secondary };
                    let btn = egui::Button::new(RichText::new(cat).size(10.0).strong().color(fg))
                        .fill(bg)
                        .stroke(Stroke::new(1.0_f32, if is_active { p.accent } else { p.border }))
                        .rounding(Rounding::same(4.0));
                    if ui.add(btn).clicked() {
                        self.sensors_category_filter = cat.to_string();
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
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("🖥 CPU").size(12.0).strong().color(p.text_primary));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if let Some(t) = snap.cpu_temp {
                                        ui.label(RichText::new(format!("{:.1}°C", t)).size(11.0).strong().color(crate::sensors::temp_color(t)));
                                    }
                                });
                            });
                            ui.add_space(3.0);
                            let cpu_val = (snap.cpu_global_usage / 100.0).clamp(0.0, 1.0);
                            ui.add(egui::ProgressBar::new(cpu_val).text(format!("{:.1}% ({} cores)", snap.cpu_global_usage, snap.cpu_cores_logical)).animate(false));
                        });

                    ui.add_space(6.0);

                    // Card 2: RAM Quick Card
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("🧠 Memory").size(12.0).strong().color(p.text_primary));
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
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("🎮 GPU").size(12.0).strong().color(p.text_primary));
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
                            });

                        ui.add_space(6.0);
                    }

                    // Card 4: Network Live Bandwidth Card
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("🌐 Network").size(12.0).strong().color(p.text_primary));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(format!("{} ifaces", snap.networks.len())).size(10.5).color(p.text_muted));
                                });
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("↓").size(11.5).strong().color(Color32::from_rgb(56, 189, 248)));
                                ui.label(RichText::new(crate::sensors::format_rate(snap.total_rx_rate_bps)).size(11.0).monospace().color(p.text_primary));
                                ui.add_space(8.0);
                                ui.label(RichText::new("↑").size(11.5).strong().color(Color32::from_rgb(168, 85, 247)));
                                ui.label(RichText::new(crate::sensors::format_rate(snap.total_tx_rate_bps)).size(11.0).monospace().color(p.text_primary));
                            });
                        });

                    ui.add_space(6.0);

                    // Card 5: Sensors Count Card
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("🌡 Temperature Probes").size(12.0).strong().color(p.text_primary));
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

        // Auto-refresh timer logic
        if let Some(hid) = host_id {
            if self.sensors_refresh_interval > 0 && self.last_sensors_refresh.elapsed() >= Duration::from_secs(self.sensors_refresh_interval as u64) {
                self.last_sensors_refresh = Instant::now();
                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                    if let Ok(s) = crate::sensors::query_remote_sensors(&host) {
                        self.remote_sensors_snapshots.insert(hid.to_string(), s);
                    }
                }
            }
        } else if self.sensors_refresh_interval > 0 && self.last_sensors_refresh.elapsed() >= Duration::from_secs(self.sensors_refresh_interval as u64) {
            self.sensors_snapshot = self.sensors_collector.collect();
            self.last_sensors_refresh = Instant::now();
        }

        let snap = if let Some(hid) = host_id {
            self.remote_sensors_snapshots.get(hid).cloned().unwrap_or_default()
        } else {
            self.sensors_snapshot.clone()
        };

        let filter_cat = self.sensors_category_filter.clone();
        let search_q = self.sensors_search_query.trim().to_lowercase();

        let show_cpu = filter_cat == "All" || filter_cat == "CPU";
        let show_ram = filter_cat == "All" || filter_cat == "RAM";
        let show_gpu = filter_cat == "All" || filter_cat == "GPU";
        let show_storage = filter_cat == "All" || filter_cat == "Storage";
        let show_net = filter_cat == "All" || filter_cat == "Network";
        let show_temps = filter_cat == "All" || filter_cat == "Temps";

        ui.vertical(|ui| {
            // ═════════════════════════════════════════════════════════════════
            // Header Card: System Overview Bar
            // ═════════════════════════════════════════════════════════════════
            Frame::none()
                .fill(p.bg_card)
                .stroke(Stroke::new(1.0_f32, p.border))
                .rounding(Rounding::same(6.0))
                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("🌡").size(22.0));
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                let title = if let Some(hid) = host_id {
                                    format!("Hardware Sensors Monitor — Remote Host ({})", hid)
                                } else {
                                    "Hardware Sensors & System Resource Monitor".to_string()
                                };
                                ui.label(RichText::new(title).size(15.0).strong().color(p.text_primary));
                                if self.sensors_refresh_interval > 0 {
                                    ui.label(RichText::new(format!("● Live ({}s refresh)", self.sensors_refresh_interval)).size(11.0).color(Color32::from_rgb(16, 185, 129)));
                                } else {
                                    ui.label(RichText::new("○ Manual refresh").size(11.0).color(p.text_muted));
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&snap.cpu_brand).size(11.0).strong().color(p.text_secondary));
                                ui.label(RichText::new("•").size(11.0).color(p.text_muted));
                                ui.label(RichText::new(format!("{} Physical / {} Logical Cores", snap.cpu_cores_physical, snap.cpu_cores_logical)).size(11.0).color(p.text_muted));
                                if snap.cpu_frequency_mhz > 0 {
                                    ui.label(RichText::new("•").size(11.0).color(p.text_muted));
                                    ui.label(RichText::new(format!("{} MHz", snap.cpu_frequency_mhz)).size(11.0).monospace().color(p.info));
                                }
                                ui.label(RichText::new("•").size(11.0).color(p.text_muted));
                                ui.label(RichText::new(format!("{} Total RAM", crate::sensors::format_bytes(snap.mem_total_bytes))).size(11.0).color(p.success));
                            });
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn_ref = egui::Button::new(RichText::new("🔄 Refresh Now").size(11.5).strong().color(Color32::WHITE))
                                .fill(p.accent)
                                .rounding(Rounding::same(4.0));
                            if ui.add(btn_ref).clicked() {
                                if let Some(hid) = host_id {
                                    if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                        if let Ok(s) = crate::sensors::query_remote_sensors(&host) {
                                            self.remote_sensors_snapshots.insert(hid.to_string(), s);
                                        }
                                    }
                                } else {
                                    self.sensors_snapshot = self.sensors_collector.collect();
                                }
                                self.last_sensors_refresh = Instant::now();
                            }
                        });
                    });
                });

            ui.add_space(8.0);

            // ═════════════════════════════════════════════════════════════════
            // Main Scrollable Area for Sensors Sections
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
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("🖥 Central Processing Unit (CPU)").size(13.5).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if let Some(t) = snap.cpu_temp {
                                            ui.label(RichText::new(format!("Package: {:.1}°C", t)).size(12.0).strong().color(crate::sensors::temp_color(t)));
                                        }
                                    });
                                });

                                ui.add_space(6.0);
                                let global_pct = (snap.cpu_global_usage / 100.0).clamp(0.0, 1.0);
                                ui.add(egui::ProgressBar::new(global_pct).text(format!("Global Usage: {:.1}%", snap.cpu_global_usage)).animate(false));

                                // Per-core usages grid
                                if !snap.cpu_core_usages.is_empty() {
                                    ui.add_space(8.0);
                                    ui.label(RichText::new("Per-Core Utilization:").size(11.0).color(p.text_muted));
                                    ui.add_space(4.0);

                                    let cols = 4.min(snap.cpu_core_usages.len().max(1));
                                    egui::Grid::new("cpu_cores_grid")
                                        .num_columns(cols)
                                        .spacing(egui::vec2(12.0, 6.0))
                                        .show(ui, |ui| {
                                            for (i, &usage) in snap.cpu_core_usages.iter().enumerate() {
                                                let core_val = (usage / 100.0).clamp(0.0, 1.0);
                                                ui.horizontal(|ui| {
                                                    ui.label(RichText::new(format!("C{}:", i)).size(10.5).color(p.text_muted));
                                                    ui.add(egui::ProgressBar::new(core_val).desired_width(120.0).text(format!("{:.0}%", usage)).animate(false));
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
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("🧠 System Memory (RAM) & Swap").size(13.5).strong().color(p.text_primary));
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
                                .stroke(Stroke::new(1.0_f32, p.border))
                                .rounding(Rounding::same(6.0))
                                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(format!("🎮 GPU #{}: {}", idx, gpu.name)).size(13.5).strong().color(p.text_primary));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if let Some(t) = gpu.temp {
                                                ui.label(RichText::new(format!("Temp: {:.1}°C", t)).size(12.0).strong().color(crate::sensors::temp_color(t)));
                                            }
                                        });
                                    });

                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(format!("Driver: {}", gpu.driver)).size(10.5).color(p.text_muted));
                                        if let (Some(used), Some(total)) = (gpu.mem_used_mb, gpu.mem_total_mb) {
                                            ui.label(RichText::new("•").size(10.5).color(p.text_muted));
                                            ui.label(RichText::new(format!("VRAM: {} MB / {} MB", used, total)).size(10.5).color(p.info));
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

                    // SECTION 4: STORAGE / DISKS
                    if show_storage && !snap.disks.is_empty() {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("💾 Storage & Mounted Disks").size(13.5).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if let Some(t) = snap.storage_temp {
                                            ui.label(RichText::new(format!("NVMe/Drive: {:.1}°C", t)).size(12.0).strong().color(crate::sensors::temp_color(t)));
                                        }
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
                                        });
                                    });

                                    let disk_pct = (d.usage_pct / 100.0).clamp(0.0, 1.0);
                                    ui.add(egui::ProgressBar::new(disk_pct).text(format!("{:.1}% used", d.usage_pct)).animate(false));
                                    ui.add_space(4.0);
                                }
                            });

                        ui.add_space(8.0);
                    }

                    // SECTION 5: NETWORK TRAFFIC & INTERFACES
                    if show_net && !snap.networks.is_empty() {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("🌐 Network Interfaces & Real-Time Bandwidth").size(13.5).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("Total ↓").size(11.0).color(Color32::from_rgb(56, 189, 248)));
                                            ui.label(RichText::new(crate::sensors::format_rate(snap.total_rx_rate_bps)).size(11.0).monospace().color(p.text_primary));
                                            ui.add_space(6.0);
                                            ui.label(RichText::new("↑").size(11.0).color(Color32::from_rgb(168, 85, 247)));
                                            ui.label(RichText::new(crate::sensors::format_rate(snap.total_tx_rate_bps)).size(11.0).monospace().color(p.text_primary));
                                        });
                                    });
                                });

                                ui.add_space(6.0);
                                egui::Grid::new("network_interfaces_grid")
                                    .num_columns(5)
                                    .striped(true)
                                    .spacing(egui::vec2(16.0, 6.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new("Interface").size(11.0).strong().color(p.text_muted));
                                        ui.label(RichText::new("Download Rate").size(11.0).strong().color(Color32::from_rgb(56, 189, 248)));
                                        ui.label(RichText::new("Upload Rate").size(11.0).strong().color(Color32::from_rgb(168, 85, 247)));
                                        ui.label(RichText::new("Total Received").size(11.0).strong().color(p.text_muted));
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
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("🌡 All Hardware Temperature Probes ({})", snap.temp_sensors.len())).size(13.5).strong().color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new("Status badge: ● <55°C Normal | ● 55-75°C Warm | ● >75°C High").size(10.5).color(p.text_muted));
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
                                        .spacing(egui::vec2(20.0, 7.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new("Component / Label").size(11.0).strong().color(p.text_muted));
                                            ui.label(RichText::new("Category").size(11.0).strong().color(p.text_muted));
                                            ui.label(RichText::new("Chip Driver").size(11.0).strong().color(p.text_muted));
                                            ui.label(RichText::new("Current Temp").size(11.0).strong().color(p.text_muted));
                                            ui.label(RichText::new("Thresholds (Max / Crit)").size(11.0).strong().color(p.text_muted));
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
        });
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
                        .rounding(Rounding::same(4.0))
                        .stroke(Stroke::new(1.0_f32, p.border));

                    if ui.add(btn).clicked() {
                        self.active_group_filter = grp.clone();
                    }
                }
            });

            ui.add_space(6.0);
            ui.separator();
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
                        // Group Section Header
                        Frame::none()
                            .fill(p.bg_subtle)
                            .stroke(Stroke::new(1.0_f32, p.border_subtle))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let header_text = format!("🏷  {} ({})", group_name.to_uppercase(), indices.len());
                                    let title_w = (ui.available_width() - 145.0).max(40.0);
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
                                        let btn_stop_grp = egui::Button::new(
                                            RichText::new("⏹ Stop").size(10.5).color(Color32::WHITE),
                                        )
                                        .fill(p.danger)
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_stop_grp).on_hover_text("Stop all servers in this group").clicked() {
                                            stop_grp = Some(group_name.clone());
                                        }

                                        let btn_start_grp = egui::Button::new(
                                            RichText::new("▶ Start").size(10.5).color(Color32::WHITE),
                                        )
                                        .fill(p.success)
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_start_grp).on_hover_text("Start all servers in this group").clicked() {
                                            start_grp = Some(group_name.clone());
                                        }
                                    });
                                });
                            });

                        ui.add_space(6.0);

                        // Server Cards for this group
                        for idx in indices {
                            let s = &mut self.services[idx];
                            let key = s.config.key.clone();
                            let is_running = s.state == ServiceState::Running || s.state == ServiceState::Starting;
                            let is_selected_tab = self.active_tab == key;
                            let stroke_col = if is_selected_tab { p.accent } else { p.border };

                            Frame::none()
                                .fill(p.bg_card)
                                .stroke(Stroke::new(1.0_f32, stroke_col))
                                .rounding(Rounding::same(8.0))
                                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    // Row 1: Status Dot + Name (clickable to switch tab) on left, Status Text on right
                                    let (st_text, st_col) = if s.restart_at.is_some() {
                                        ("⟳ Restarting...", p.warning)
                                    } else if is_running {
                                        ("Running", p.success)
                                    } else if s.state == ServiceState::Errored {
                                        ("Errored", p.danger)
                                    } else {
                                        ("Stopped", p.text_muted)
                                    };

                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        let dot_color = if is_running {
                                            p.success
                                        } else {
                                            p.text_muted
                                        };
                                        draw_status_dot(ui, dot_color);

                                        let status_w = 75.0;
                                        let name_w = (ui.available_width() - status_w - 6.0).max(40.0);

                                        let name_resp = ui.allocate_ui_with_layout(
                                            egui::vec2(name_w, 18.0),
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.set_max_width(name_w);
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(&s.config.name)
                                                            .strong()
                                                            .size(13.0)
                                                            .color(if is_selected_tab { p.accent } else { p.text_primary }),
                                                    )
                                                    .truncate()
                                                    .sense(egui::Sense::click()),
                                                )
                                                .on_hover_text(&s.config.name)
                                            },
                                        ).inner;

                                        if name_resp.clicked() {
                                            to_switch_tab = Some(key.clone());
                                        }

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(RichText::new(st_text).size(11.0).strong().color(st_col));
                                        });
                                    });

                                    ui.add_space(3.0);

                                    // Row 2: Subtitle / command
                                    Frame::none()
                                        .fill(p.bg_input)
                                        .stroke(Stroke::new(1.0_f32, p.border_subtle))
                                        .rounding(Rounding::same(4.0))
                                        .inner_margin(egui::Margin::symmetric(7.0, 3.0))
                                        .show(ui, |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&s.config.command)
                                                        .size(11.0)
                                                        .monospace()
                                                        .color(p.text_muted),
                                                )
                                                .truncate(),
                                            );
                                        });

                                    // Row 2.5: If running, display PID, CPU%, RAM MB, Uptime!
                                    if is_running {
                                        ui.add_space(4.0);
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                                            if s.config.port > 0 {
                                                let (port_text_col, port_bg_col) = if s.port_open {
                                                    (Color32::from_rgb(96, 165, 250), Color32::from_rgb(18, 38, 56))
                                                } else {
                                                    (Color32::from_rgb(160, 170, 190), Color32::from_rgb(26, 30, 42))
                                                };
                                                render_metric_pill_custom(
                                                    ui,
                                                    "🔌",
                                                    &format!(":{}", s.config.port),
                                                    port_text_col,
                                                    port_bg_col,
                                                );
                                            }
                                            if let Some(pid) = s.pid {
                                                render_metric_pill_custom(
                                                    ui,
                                                    "🆔",
                                                    &format!("{}", pid),
                                                    Color32::from_rgb(160, 170, 190),
                                                    Color32::from_rgb(26, 30, 42),
                                                );
                                            }
                                            render_metric_pill_custom(
                                                ui,
                                                "⚡",
                                                &format!("{:.1}%", s.cpu_usage),
                                                Color32::from_rgb(56, 189, 248),
                                                Color32::from_rgb(18, 38, 56),
                                            );
                                            render_metric_pill_custom(
                                                ui,
                                                "💾",
                                                &format!("{:.1} MB", s.memory_mb),
                                                Color32::from_rgb(52, 211, 153),
                                                Color32::from_rgb(18, 48, 40),
                                            );
                                            let uptime = s.uptime_formatted();
                                            if !uptime.is_empty() {
                                                render_metric_pill_custom(
                                                    ui,
                                                    "⏱",
                                                    &uptime,
                                                    Color32::from_rgb(251, 191, 36),
                                                    Color32::from_rgb(45, 38, 20),
                                                );
                                            }
                                        });
                                    }

                                    ui.add_space(6.0);

                                    // Row 3: Primary Server Controls (Start / Stop & Restart)
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                                        if is_running {
                                            let btn_stop = egui::Button::new(
                                                RichText::new("⏹ Stop").size(11.0).strong().color(Color32::WHITE),
                                            )
                                            .fill(Color32::from_rgb(239, 68, 68))
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(58.0, 24.0));
                                            if ui.add(btn_stop).on_hover_text("Stop this server").clicked() {
                                                to_stop = Some(idx);
                                            }

                                            let btn_restart = egui::Button::new(
                                                RichText::new("⟳ Restart").size(11.0).strong().color(Color32::WHITE),
                                            )
                                            .fill(Color32::from_rgb(245, 158, 11))
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(68.0, 24.0));
                                            if ui.add(btn_restart).on_hover_text("Restart this server").clicked() {
                                                to_restart = Some(idx);
                                            }
                                        } else {
                                            let btn_start = egui::Button::new(
                                                RichText::new("▶ Start").size(11.0).strong().color(Color32::WHITE),
                                            )
                                            .fill(Color32::from_rgb(16, 185, 129))
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(64.0, 24.0));
                                            if ui.add(btn_start).on_hover_text("Start this server").clicked() {
                                                to_start = Some(idx);
                                            }
                                        }
                                    });

                                    ui.add_space(4.0);

                                    // Row 3.5: Additional Action Buttons (placed DOWN, not right)
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);

                                        let btn_vscode = egui::Button::new(
                                            RichText::new("💻").size(11.5).color(Color32::from_rgb(56, 189, 248)),
                                        )
                                        .fill(Color32::from_rgb(24, 38, 54))
                                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 60, 80)))
                                        .rounding(Rounding::same(4.0))
                                        .min_size(egui::vec2(25.0, 22.0));
                                        if ui.add(btn_vscode).on_hover_text("Open in VS Code (code .)").clicked() {
                                            crate::service::open_in_vscode(&s.config.cwd);
                                        }

                                        let btn_folder = egui::Button::new(
                                            RichText::new("📁").size(11.5).color(Color32::from_rgb(203, 213, 225)),
                                        )
                                        .fill(Color32::from_rgb(33, 38, 52))
                                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(50, 56, 75)))
                                        .rounding(Rounding::same(4.0))
                                        .min_size(egui::vec2(25.0, 22.0));
                                        if ui.add(btn_folder).on_hover_text("Open working directory in file manager").clicked() {
                                            crate::service::open_folder(&s.config.cwd);
                                        }

                                        let btn_console = egui::Button::new(
                                            RichText::new("🖥️").size(11.5).color(Color32::from_rgb(168, 85, 247)),
                                        )
                                        .fill(Color32::from_rgb(34, 30, 48))
                                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(55, 45, 75)))
                                        .rounding(Rounding::same(4.0))
                                        .min_size(egui::vec2(25.0, 22.0));
                                        if ui.add(btn_console).on_hover_text("Open in console / terminal window").clicked() {
                                            s.open_console();
                                        }

                                        let btn_edit = egui::Button::new(
                                            RichText::new("Edit").size(10.5).color(Color32::from_rgb(203, 213, 225)),
                                        )
                                        .fill(Color32::from_rgb(33, 38, 52))
                                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(50, 56, 75)))
                                        .rounding(Rounding::same(4.0))
                                        .min_size(egui::vec2(34.0, 22.0));
                                        if ui.add(btn_edit).on_hover_text("Edit server configuration").clicked() {
                                            to_edit = Some(s.config.clone());
                                        }

                                        let btn_del = egui::Button::new(
                                            RichText::new("Delete").size(10.5).color(Color32::from_rgb(248, 113, 113)),
                                        )
                                        .fill(Color32::from_rgb(45, 30, 35))
                                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(70, 40, 48)))
                                        .rounding(Rounding::same(4.0))
                                        .min_size(egui::vec2(42.0, 22.0));
                                        if ui.add(btn_del).on_hover_text("Delete server").clicked() {
                                            to_delete = Some(key.clone());
                                        }
                                    });

                                    ui.add_space(4.0);

                                    // Row 4: Dedicated Autostart & Auto-restart Checkboxes Row
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = egui::vec2(12.0, 4.0);

                                        let autostart_color = if s.config.autostart { p.accent } else { p.text_muted };
                                        if ui.checkbox(
                                            &mut s.config.autostart,
                                            RichText::new("🚀 autostart").size(11.0).color(autostart_color),
                                        ).on_hover_text("Automatically start this server when Server Launcher opens").changed() {
                                            to_save_config = true;
                                        }

                                        if ui.checkbox(
                                            &mut s.config.auto_restart,
                                            RichText::new("⟳ auto-restart").size(11.0).color(p.text_muted),
                                        ).on_hover_text("Auto-restart this server if it exits unexpectedly or crashes").changed() {
                                            to_save_config = true;
                                        }
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
            // Notebook Tabs Strip (matching screenshot)
            ui.horizontal(|ui| {
                // Tab 0: Server Overview
                let is_ov_active = self.active_tab == "overview";
                let ov_border = if is_ov_active {
                    Stroke::new(1.0_f32, p.accent)
                } else {
                    Stroke::new(1.0_f32, p.border)
                };
                let ov_bg = if is_ov_active {
                    p.bg_card
                } else {
                    p.bg_sidebar
                };

                let ov_btn = egui::Button::new(
                    RichText::new("  Server Overview  ")
                        .size(12.0)
                        .strong()
                        .color(if is_ov_active { p.text_primary } else { p.text_secondary }),
                )
                .fill(ov_bg)
                .stroke(ov_border)
                .rounding(Rounding { nw: 4.0, ne: 4.0, sw: 0.0, se: 0.0 });

                if ui.add(ov_btn).clicked() {
                    self.active_tab = "overview".to_string();
                }

                // Server tabs
                for s in &self.services {
                    let is_active = self.active_tab == s.config.key;
                    let border = if is_active {
                        Stroke::new(1.0_f32, p.accent)
                    } else {
                        Stroke::new(1.0_f32, p.border)
                    };
                    let bg = if is_active {
                        p.bg_card
                    } else {
                        p.bg_sidebar
                    };

                    let is_running = s.state == ServiceState::Running || s.state == ServiceState::Starting;
                    let tab_title = if is_running && s.config.port > 0 {
                        format!("  {} (: {})  ", s.config.name, s.config.port)
                    } else {
                        format!("  {}  ", s.config.name)
                    };

                    let tab_btn = egui::Button::new(
                        RichText::new(tab_title)
                            .size(12.0)
                            .color(if is_active { p.text_primary } else { p.text_secondary }),
                    )
                    .fill(bg)
                    .stroke(border)
                    .rounding(Rounding { nw: 4.0, ne: 4.0, sw: 0.0, se: 0.0 });

                    if ui.add(tab_btn).clicked() {
                        self.active_tab = s.config.key.clone();
                    }
                }
            });

            // Outer Frame for the tab content (FILLS 100% OF REMAINING SPACE!)
            Frame::none()
                .fill(p.bg_main)
                .stroke(Stroke::new(1.0_f32, p.border))
                .rounding(Rounding { nw: 0.0, ne: 4.0, sw: 4.0, se: 4.0 })
                .inner_margin(14.0)
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
                    RichText::new("🔄 Refresh")
                        .size(11.5)
                        .strong()
                        .color(p.accent),
                )
                .fill(p.bg_button)
                .stroke(Stroke::new(1.0_f32, p.border))
                .rounding(Rounding::same(4.0));
                if ui.add(btn_refresh).clicked() {
                    self.refresh_server_list();
                }

                // Search Box with 🔍 icon, hint, clear button, and Ctrl+F focus support
                Frame::none()
                    .fill(p.bg_card)
                    .stroke(Stroke::new(
                        1.0_f32,
                        if !self.overview_search_query.is_empty() {
                            p.accent
                        } else {
                            p.border
                        },
                    ))
                    .rounding(Rounding::same(4.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 3.5))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("🔍")
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
                                            RichText::new("✕")
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

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(10.0);

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
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .rounding(Rounding::same(6.0))
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

                for &idx in &managed_indices {
                    let s = &self.services[idx];
                    let is_running = s.state == ServiceState::Running || s.state == ServiceState::Starting;

                    let card_height = if is_running { 90.0 } else { 62.0 };
                    let current_pos = ui.cursor().min;
                    let approx_rect = egui::Rect::from_min_size(current_pos, egui::vec2(ui.available_width(), card_height));
                    if !ui.clip_rect().expand(120.0).intersects(approx_rect) {
                        ui.allocate_space(egui::vec2(ui.available_width(), card_height));
                        ui.add_space(6.0);
                        continue;
                    }

                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                // Row 1: Status dot, Server Name, Group badge, and Action Buttons (right-aligned)
                                ui.horizontal(|ui| {
                                    let dot_color = if is_running {
                                        p.success
                                    } else {
                                        p.text_muted
                                    };
                                    draw_status_dot(ui, dot_color);

                                    ui.label(
                                        RichText::new(&s.config.name)
                                            .size(13.5)
                                            .strong()
                                            .color(p.text_primary),
                                    );
                                    render_group_badge(ui, &s.config.group);

                                    if s.config.port > 0 {
                                        let (port_label, port_color) = if s.port_open {
                                            (format!(":{}", s.config.port), p.info)
                                        } else if is_running {
                                            (format!(":{}", s.config.port), p.accent_light)
                                        } else {
                                            (format!(":{}", s.config.port), p.text_muted)
                                        };
                                        render_metric_pill(ui, "🔌", &port_label, port_color);
                                    }

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if is_running {
                                            let btn_stop = egui::Button::new(
                                                RichText::new("Stop").size(11.5).strong().color(Color32::WHITE),
                                            )
                                            .fill(p.danger)
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(50.0, 24.0));
                                            if ui.add(btn_stop).clicked() {
                                                to_toggle = Some((idx, 0));
                                            }

                                            let btn_restart = egui::Button::new(
                                                RichText::new("Restart").size(11.5).strong().color(Color32::WHITE),
                                            )
                                            .fill(p.warning)
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(58.0, 24.0));
                                            if ui.add(btn_restart).clicked() {
                                                to_toggle = Some((idx, 2));
                                            }
                                        } else {
                                            let btn_start = egui::Button::new(
                                                RichText::new("Start").size(11.5).strong().color(Color32::WHITE),
                                            )
                                            .fill(p.success)
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(54.0, 24.0));
                                            if ui.add(btn_start).clicked() {
                                                to_toggle = Some((idx, 1));
                                            }
                                        }

                                        let btn_folder = egui::Button::new(
                                            RichText::new("📁").size(11.0).color(p.text_secondary),
                                        )
                                        .fill(p.bg_button)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .rounding(Rounding::same(5.0));
                                        if ui.add(btn_folder).on_hover_text("Open working directory in file manager (xdg-open)").clicked() {
                                            crate::service::open_folder(&s.config.cwd);
                                        }

                                        let btn_vscode = egui::Button::new(
                                            RichText::new("💻").size(11.0).color(p.info),
                                        )
                                        .fill(p.bg_button)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .rounding(Rounding::same(5.0));
                                        if ui.add(btn_vscode).on_hover_text("Open in VS Code (code .)").clicked() {
                                            crate::service::open_in_vscode(&s.config.cwd);
                                        }

                                        let btn_console = egui::Button::new(
                                            RichText::new("🖥️").size(11.0).color(Color32::from_rgb(168, 85, 247)),
                                        )
                                        .fill(p.bg_button)
                                        .stroke(Stroke::new(1.0_f32, p.border))
                                        .rounding(Rounding::same(5.0));
                                        if ui.add(btn_console).on_hover_text("Open in console / terminal window").clicked() {
                                            s.open_console();
                                        }

                                        let st_text = if is_running { "Running" } else { "Stopped" };
                                        let st_color = if is_running {
                                            p.success
                                        } else {
                                            p.text_muted
                                        };
                                        ui.label(RichText::new(st_text).size(12.0).strong().color(st_color));
                                    });
                                });

                                // Row 2: Command
                                ui.add_space(3.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(20.0);
                                    ui.label(
                                        RichText::new(&s.config.command)
                                            .size(10.5)
                                            .monospace()
                                            .color(p.text_muted),
                                    );
                                });

                                // Row 3: Live Resource & Runtime Metrics (when running)
                                if is_running {
                                    ui.add_space(4.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.add_space(20.0);
                                        if let Some(pid) = s.pid {
                                            render_metric_pill(
                                                ui,
                                                "🆔",
                                                &format!("PID {}", pid),
                                                p.text_muted,
                                            );
                                        }
                                        if s.config.port > 0 {
                                            let port_color = if s.port_open { p.info } else { p.accent_light };
                                            render_metric_pill(
                                                ui,
                                                "🔌",
                                                &format!(":{}", s.config.port),
                                                port_color,
                                            );
                                        }
                                        render_metric_pill(
                                            ui,
                                            "⚡",
                                            &format!("{:.1}%", s.cpu_usage),
                                            p.info,
                                        );
                                        render_metric_pill(
                                            ui,
                                            "💾",
                                            &format!("{:.1} MB", s.memory_mb),
                                            p.success,
                                        );
                                        let uptime = s.uptime_formatted();
                                        if !uptime.is_empty() {
                                            render_metric_pill(
                                                ui,
                                                "⏱️",
                                                &uptime,
                                                p.warning,
                                            );
                                        }
                                        if s.config.autostart {
                                            render_metric_pill(
                                                ui,
                                                "🚀",
                                                "Autostart",
                                                p.accent_light,
                                            );
                                        }
                                    });
                                }
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
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .rounding(Rounding::same(6.0))
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
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .rounding(Rounding::same(6.0))
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
                                                "🆔",
                                                &format!("PID {}", pid),
                                                p.text_muted,
                                            );
                                        }
                                        render_metric_pill(
                                            ui,
                                            "🔌",
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
                                                    "⚡",
                                                    &format!("{:.1}%", cpu),
                                                    p.info,
                                                );
                                                render_metric_pill(
                                                    ui,
                                                    "💾",
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
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .rounding(Rounding::same(5.0))
                                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                            .show(ui, |ui| {
                                                ui.vertical(|ui| {
                                                    if !cwd_str.is_empty() {
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new("📁 Location:").size(11.0).strong().color(p.accent));
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
                                                            ui.label(RichText::new("⚙ Command:").size(11.0).strong().color(p.accent));
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
                                                            "📟 Hide Inline Terminal"
                                                        } else {
                                                            "📟 View Output (Inline Terminal)"
                                                        };
                                                        let btn_inline = egui::Button::new(
                                                            RichText::new(inline_btn_text)
                                                                .size(11.0)
                                                                .strong()
                                                                .color(if is_inline_open { p.warning } else { p.accent }),
                                                        )
                                                        .fill(if is_inline_open { p.bg_card } else { p.bg_button })
                                                        .stroke(Stroke::new(1.0_f32, if is_inline_open { p.warning } else { p.border }))
                                                        .rounding(Rounding::same(4.0));

                                                        if ui.add(btn_inline).on_hover_text("Open inline terminal to view live process output and run shell commands in working directory").clicked() {
                                                            if is_inline_open {
                                                                self.open_inline_terminals.remove(&listener.port);
                                                            } else {
                                                                self.open_inline_terminals.insert(listener.port);
                                                                self.ensure_inline_session(listener.port, &listener.name, listener.pid, &cmd_str, &cwd_str);
                                                            }
                                                        }

                                                        let btn_external = egui::Button::new(
                                                            RichText::new("↗ External Terminal")
                                                                .size(11.0)
                                                                .strong()
                                                                .color(p.info),
                                                        )
                                                        .fill(p.bg_button)
                                                        .stroke(Stroke::new(1.0_f32, p.border))
                                                        .rounding(Rounding::same(4.0));

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
                                                                RichText::new("📁 Open Folder")
                                                                    .size(11.0)
                                                                    .strong()
                                                                    .color(p.warning),
                                                            )
                                                            .fill(p.bg_button)
                                                            .stroke(Stroke::new(1.0_f32, p.border))
                                                            .rounding(Rounding::same(4.0));

                                                            if ui.add(btn_folder).on_hover_text("Open working directory in desktop file manager (xdg-open)").clicked() {
                                                                crate::service::open_folder(&cwd_str);
                                                            }

                                                            let btn_vscode = egui::Button::new(
                                                                RichText::new("💻 VS Code")
                                                                    .size(11.0)
                                                                    .strong()
                                                                    .color(p.info),
                                                            )
                                                            .fill(p.bg_button)
                                                            .stroke(Stroke::new(1.0_f32, p.border))
                                                            .rounding(Rounding::same(4.0));

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
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .rounding(Rounding::same(6.0))
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
                                            "🔌",
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
                                                RichText::new("✕").size(11.0).color(p.text_muted),
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
                                            .stroke(Stroke::new(1.0_f32, p.border))
                                            .rounding(Rounding::same(5.0))
                                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                            .show(ui, |ui| {
                                                ui.vertical(|ui| {
                                                    if !cwd.is_empty() {
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new("📁 Location:").size(11.0).strong().color(p.accent));
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
                                                            ui.label(RichText::new("⚙ Command:").size(11.0).strong().color(p.accent));
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
                                                            "📟 Hide Inline Terminal"
                                                        } else {
                                                            "📟 View Output (Inline Terminal)"
                                                        };
                                                        let btn_inline = egui::Button::new(
                                                            RichText::new(inline_btn_text)
                                                                .size(11.0)
                                                                .strong()
                                                                .color(if is_inline_open { p.warning } else { p.accent }),
                                                        )
                                                        .fill(if is_inline_open { p.bg_card } else { p.bg_button })
                                                        .stroke(Stroke::new(1.0_f32, if is_inline_open { p.warning } else { p.border }))
                                                        .rounding(Rounding::same(4.0));

                                                        if ui.add(btn_inline).on_hover_text("Open inline terminal to run shell commands in working directory").clicked() {
                                                            if is_inline_open {
                                                                self.open_inline_terminals.remove(port);
                                                            } else {
                                                                self.open_inline_terminals.insert(*port);
                                                                self.ensure_inline_session(*port, name, None, cmd, cwd);
                                                            }
                                                        }

                                                        let btn_external = egui::Button::new(
                                                            RichText::new("↗ External Terminal")
                                                                .size(11.0)
                                                                .strong()
                                                                .color(p.info),
                                                        )
                                                        .fill(p.bg_button)
                                                        .stroke(Stroke::new(1.0_f32, p.border))
                                                        .rounding(Rounding::same(4.0));

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
                                                                RichText::new("📁 Open Folder")
                                                                    .size(11.0)
                                                                    .strong()
                                                                    .color(p.warning),
                                                            )
                                                            .fill(p.bg_button)
                                                            .stroke(Stroke::new(1.0_f32, p.border))
                                                            .rounding(Rounding::same(4.0));

                                                            if ui.add(btn_folder).on_hover_text("Open working directory in desktop file manager (xdg-open)").clicked() {
                                                                crate::service::open_folder(cwd);
                                                            }

                                                            let btn_vscode = egui::Button::new(
                                                                RichText::new("💻 VS Code")
                                                                    .size(11.0)
                                                                    .strong()
                                                                    .color(p.info),
                                                            )
                                                            .fill(p.bg_button)
                                                            .stroke(Stroke::new(1.0_f32, p.border))
                                                            .rounding(Rounding::same(4.0));

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
        let (name, is_running, pid, port, port_open, cpu, mem, uptime, actions, links) = {
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

        // Sub-header above logs:
        // Row 1: Service name + Live Metrics ... Find toggle + Start/Stop/Restart
        let p = self.theme.palette();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&name)
                    .size(15.0)
                    .strong()
                    .color(p.text_primary),
            );

            if is_running {
                ui.add_space(8.0);
                if let Some(p) = pid {
                    render_metric_pill(
                        ui,
                        "🆔",
                        &format!("PID {}", p),
                        Color32::from_rgb(148, 163, 184),
                    );
                }
                if port > 0 {
                    let port_color = if port_open {
                        Color32::from_rgb(96, 165, 250)
                    } else {
                        Color32::from_rgb(148, 163, 184)
                    };
                    render_metric_pill(
                        ui,
                        "🔌",
                        &format!(":{}", port),
                        port_color,
                    );
                }
                render_metric_pill(
                    ui,
                    "⚡",
                    &format!("{:.1}%", cpu),
                    Color32::from_rgb(96, 165, 250),
                );
                render_metric_pill(
                    ui,
                    "💾",
                    &format!("{:.1} MB", mem),
                    Color32::from_rgb(52, 211, 153),
                );
                if !uptime.is_empty() {
                    render_metric_pill(
                        ui,
                        "⏱️",
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
                    RichText::new("🔍 Find (Ctrl+F)")
                        .size(11.0)
                        .color(if search_is_open { p.accent } else { p.text_secondary }),
                )
                .fill(if search_is_open { p.accent.gamma_multiply(0.25) } else { p.bg_button })
                .stroke(Stroke::new(1.0_f32, if search_is_open { p.accent } else { p.border }))
                .rounding(Rounding::same(4.0));
                if ui.add(find_btn).clicked() {
                    self.search_open.insert(key.to_string(), !search_is_open);
                }
            });
        });

        ui.add_space(5.0);

        // Row 2: Secondary Tool Buttons & Service / Log Options
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);

            let vscode_btn = egui::Button::new(
                RichText::new("💻 VS Code")
                    .size(11.0)
                    .color(p.info),
            )
            .fill(p.bg_button)
            .stroke(Stroke::new(1.0_f32, p.border))
            .rounding(Rounding::same(4.0));
            if ui.add(vscode_btn).on_hover_text("Open working directory in VS Code (code .)").clicked() {
                let cwd = self.find_service(key).map(|s| s.config.cwd.clone()).unwrap_or_default();
                crate::service::open_in_vscode(&cwd);
            }

            let folder_btn = egui::Button::new(
                RichText::new("📁 Folder")
                    .size(11.0)
                    .color(p.text_secondary),
            )
            .fill(p.bg_button)
            .stroke(Stroke::new(1.0_f32, p.border))
            .rounding(Rounding::same(4.0));
            if ui.add(folder_btn).on_hover_text("Open working directory in file manager (xdg-open)").clicked() {
                let cwd = self.find_service(key).map(|s| s.config.cwd.clone()).unwrap_or_default();
                crate::service::open_folder(&cwd);
            }

            let console_btn = egui::Button::new(
                RichText::new("🖥️ Console")
                    .size(11.0)
                    .color(p.accent_light),
            )
            .fill(p.bg_button)
            .stroke(Stroke::new(1.0_f32, p.border))
            .rounding(Rounding::same(4.0));
            if ui.add(console_btn).on_hover_text("Open interactive console / terminal window in working directory").clicked() {
                if let Some(s) = self.find_service(key) {
                    s.open_console();
                }
            }

            ui.add_space(4.0);

            let mut auto_scroll = self.auto_scroll.get(key).copied().unwrap_or(true);
            if ui.checkbox(
                &mut auto_scroll,
                RichText::new("📜 Auto-scroll").size(11.0).color(p.accent_light),
            ).on_hover_text("Keep pinned to bottom when new logs arrive (uncheck to scroll up smoothly without jitter)").changed() {
                self.auto_scroll.insert(key.to_string(), auto_scroll);
            }

            let mut autostart = self.find_service(key).map(|s| s.config.autostart).unwrap_or(false);
            if ui.checkbox(
                &mut autostart,
                RichText::new("🚀 Autostart").size(11.0).color(p.accent_light),
            ).on_hover_text("Automatically start this server when Server Launcher opens").changed() {
                if let Some(s) = self.find_service_mut(key) {
                    s.config.autostart = autostart;
                    self.persist_config();
                }
            }

            let mut auto_restart = self.find_service(key).map(|s| s.config.auto_restart).unwrap_or(false);
            if ui.checkbox(
                &mut auto_restart,
                RichText::new("⏱️ Auto-restart").size(11.0).color(p.accent_light),
            ).on_hover_text("Auto-restart this server if it exits unexpectedly or crashes").changed() {
                if let Some(s) = self.find_service_mut(key) {
                    s.config.auto_restart = auto_restart;
                    self.persist_config();
                }
            }
        });

        // Quick Sub-command & Link Buttons Bar (if defined)
        if !actions.is_empty() || !links.is_empty() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for act in &actions {
                    let act_btn = egui::Button::new(
                        RichText::new(format!("⚡ {}", act.label))
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
                        RichText::new(format!("🔗 {}", lnk.label))
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

        ui.add_space(4.0);
        ui.separator();

        // Collapsible Search Bar (Ctrl+F)
        let search_is_open = self.search_open.get(key).copied().unwrap_or(false);
        if search_is_open {
            Frame::none()
                .fill(p.bg_subtle)
                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                .rounding(Rounding::same(4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("🔍").color(p.accent));
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
                            if ui.button("✕").clicked() {
                                self.search_open.insert(key.to_string(), false);
                            }
                        });
                    });
                });
            ui.add_space(4.0);
        }

        // Log Terminal Output Area (fills all space down to the bottom command bar)
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

        let available_height = ui.available_height() - 44.0;
        let auto_scroll_enabled = self.auto_scroll.get(key).copied().unwrap_or(true);
        let row_height = 18.0_f32;
        let total_rows = filtered_logs.len();

        ScrollArea::both()
            .id_salt(format!("log_scroll_{}", key))
            .stick_to_bottom(auto_scroll_enabled)
            .auto_shrink([false, false])
            .max_height(available_height)
            .show_rows(ui, row_height, total_rows, |ui, row_range| {
                ui.set_width(ui.available_width());
                for idx in row_range {
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
                }
            });

        ui.separator();

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

        // Quick Dev Server Shortcuts Toolbar (when running)
        if is_running {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 3.0);
                ui.label(RichText::new("⌨ Dev Server Shortcuts:").size(11.0).color(p.text_muted));
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
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .rounding(Rounding::same(3.0));
                    if ui.add(btn).on_hover_text(tip).clicked() {
                        if let Some(s) = self.find_service(key) {
                            s.send_input(key_cmd);
                        }
                    }
                }
            });
            ui.add_space(2.0);
        }

        let resp = ui.horizontal(|ui| {
            ui.label(RichText::new("❯").color(p.accent).strong().size(13.0));

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
                .rounding(Rounding::same(4.0));
            if ui.add(btn_send).clicked() {
                do_send = true;
            }

            let btn_ctrlc = egui::Button::new(RichText::new("Ctrl+C").size(11.0).color(p.warning))
                .fill(p.bg_button)
                .rounding(Rounding::same(4.0));
            if ui.add(btn_ctrlc).clicked() {
                if let Some(s) = self.find_service(key) {
                    s.send_interrupt();
                }
            }

            let btn_clear = egui::Button::new(RichText::new("Clear").size(11.0).color(p.text_muted))
                .fill(p.bg_button)
                .rounding(Rounding::same(4.0));
            if ui.add(btn_clear).clicked() {
                if let Some(s) = self.find_service(key) {
                    s.clear_logs();
                }
            }

            let btn_console = egui::Button::new(RichText::new("🖥️ Console").size(11.0).color(p.accent_light))
                .fill(p.bg_button)
                .stroke(Stroke::new(1.0_f32, p.border))
                .rounding(Rounding::same(4.0));
            if ui.add(btn_console).on_hover_text("Open in external console / terminal window").clicked() {
                if let Some(s) = self.find_service(key) {
                    s.open_console();
                }
            }

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
                        .stroke(Stroke::new(1.0_f32, p.border))
                        .rounding(Rounding::same(6.0))
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
                                ui.label(RichText::new("⚡ Commands").size(11.0).strong().color(p.accent));
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
                                                        .stroke(Stroke::new(1.0_f32, p.border))
                                                        .rounding(Rounding::same(3.0))
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
