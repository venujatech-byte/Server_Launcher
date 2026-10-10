use crate::config::ConfigFile;
use crate::modals::{
    render_add_edit_modal, render_command_palette, AddEditModalState, CommandPaletteState,
    ModalAction, PaletteAction,
};
use crate::scanner::{get_lan_ip, SystemPortScanner};
use crate::service::{LogKind, Service, ServiceState};
use eframe::egui;
use egui::{
    Color32, Context, Frame, Key, Modifiers, RichText, Rounding, ScrollArea, Stroke, Ui,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Instant;
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

pub struct LauncherApp {
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
}

impl LauncherApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config_path = ConfigFile::default_path();
        let config_file = ConfigFile::load_from_file(&config_path);

        let services = config_file
            .servers
            .into_iter()
            .map(Service::new)
            .collect();

        let scanner = SystemPortScanner::new();
        scanner.trigger_scan();

        let icon_texture = {
            let png_bytes = include_bytes!("../assets/icon.png");
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

        Self {
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
        }
    }

    fn persist_config(&self) {
        let cfg = ConfigFile {
            servers: self.services.iter().map(|s| s.config.clone()).collect(),
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
            let cmd_str = command.to_string();
            let logs_clone = s.logs.clone();
            std::thread::spawn(move || {
                let current_dir = if cwd.is_empty() { ".".to_string() } else { cwd };
                #[cfg(target_os = "windows")]
                let output = std::process::Command::new("cmd.exe")
                    .args(&["/C", &cmd_str])
                    .current_dir(&current_dir)
                    .output();

                #[cfg(not(target_os = "windows"))]
                let output = std::process::Command::new("sh")
                    .args(&["-c", &cmd_str])
                    .current_dir(&current_dir)
                    .output();

                match output {
                    Ok(out) => {
                        let mut has_output = false;
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        for line in stdout.lines() {
                            has_output = true;
                            let entry = crate::service::LogEntry::new(
                                line.to_string(),
                                crate::service::LogKind::Stdout,
                            );
                            if let Ok(mut l) = logs_clone.lock() {
                                if l.len() >= 3000 {
                                    l.pop_front();
                                }
                                l.push_back(entry);
                            }
                        }
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        for line in stderr.lines() {
                            has_output = true;
                            let entry = crate::service::LogEntry::new(
                                line.to_string(),
                                crate::service::LogKind::Stderr,
                            );
                            if let Ok(mut l) = logs_clone.lock() {
                                if l.len() >= 3000 {
                                    l.pop_front();
                                }
                                l.push_back(entry);
                            }
                        }
                        if out.status.success() {
                            if !has_output {
                                let entry = crate::service::LogEntry::new(
                                    "[action completed successfully]".to_string(),
                                    crate::service::LogKind::Launcher,
                                );
                                if let Ok(mut l) = logs_clone.lock() {
                                    l.push_back(entry);
                                }
                            }
                        } else {
                            let code = out
                                .status
                                .code()
                                .map(|c| c.to_string())
                                .unwrap_or_else(|| "signal".to_string());
                            let entry = crate::service::LogEntry::new(
                                format!("[action exited with error code {}]", code),
                                crate::service::LogKind::Warning,
                            );
                            if let Ok(mut l) = logs_clone.lock() {
                                l.push_back(entry);
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
    Frame::none()
        .fill(Color32::from_rgb(40, 32, 68))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(65, 52, 105)))
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .size(11.0)
                    .color(Color32::from_rgb(175, 165, 255))
                    .strong(),
            );
        });
}

fn render_metric_pill(ui: &mut Ui, icon: &str, label: &str, text_color: Color32) {
    let bg_color = Color32::from_rgb(26, 30, 42);
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
        let any_running = self.services.iter().any(|s| {
            s.state == ServiceState::Running || s.state == ServiceState::Starting
        });

        // Periodic poll only if any services are active (every 1000ms)
        if any_running && self.last_poll.elapsed() >= std::time::Duration::from_millis(1000) {
            for s in &mut self.services {
                s.poll_status(&mut self.sys);
            }
            self.last_poll = Instant::now();
        }

        // Periodic poll for other listening processes if user ticked the checkbox
        if self.show_other_process_metrics && self.active_tab == "overview" {
            ctx.request_repaint_after(std::time::Duration::from_millis(1000));
            if self.other_metrics.is_empty() || self.last_other_poll.elapsed() >= std::time::Duration::from_millis(1000) {
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

        // ═════════════════════════════════════════════════════════════════════
        // TOP HEADER BAR (Pinned to Top, exact match to screenshot)
        // ═════════════════════════════════════════════════════════════════════
        egui::TopBottomPanel::top("top_header")
            .frame(
                Frame::none()
                    .fill(Color32::from_rgb(15, 17, 23))
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
                                .fit_to_exact_size(egui::vec2(36.0, 36.0))
                                .rounding(Rounding::same(6.0)),
                        );
                        ui.add_space(4.0);
                    }
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Server Launcher")
                                .size(20.0)
                                .strong()
                                .color(Color32::from_rgb(235, 238, 245)),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(format!(
                                "this pc: {}    servers: {}",
                                self.lan_ip,
                                self.services.len()
                            ))
                            .size(11.5)
                            .color(Color32::from_rgb(123, 131, 148)),
                        );
                    });

                    // Right: Actions buttons
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // + Add Server
                        let btn_add = egui::Button::new(
                            RichText::new("+ Add Server")
                                .size(12.0)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(108, 92, 231))
                        .rounding(Rounding::same(4.0))
                        .min_size(egui::vec2(95.0, 26.0));
                        if ui.add(btn_add).clicked() {
                            self.add_edit_modal.open_new();
                        }

                        // Quick Launch (Ctrl+P)
                        let btn_pal = egui::Button::new(
                            RichText::new("⚡ Quick Launch (Ctrl+P)")
                                .size(12.0)
                                .color(Color32::from_rgb(162, 155, 254)),
                        )
                        .fill(Color32::from_rgb(37, 40, 51))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(55, 60, 75)))
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
                                .color(Color32::from_rgb(228, 231, 238)),
                        )
                        .fill(Color32::from_rgb(37, 40, 51))
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
                        .fill(Color32::from_rgb(0, 210, 160))
                        .rounding(Rounding::same(4.0))
                        .min_size(egui::vec2(75.0, 26.0));
                        if ui.add(btn_start_all).clicked() {
                            self.start_all();
                        }
                    });
                });
            });

        // ═════════════════════════════════════════════════════════════════════
        // LEFT SIDEBAR: Full Window Height (~350px width)
        // ═════════════════════════════════════════════════════════════════════
        egui::SidePanel::left("left_sidebar")
            .resizable(true)
            .default_width(345.0)
            .min_width(300.0)
            .max_width(450.0)
            .frame(
                Frame::none()
                    .fill(Color32::from_rgb(15, 17, 23))
                    .inner_margin(egui::Margin {
                        left: 14.0,
                        right: 12.0,
                        top: 8.0,
                        bottom: 12.0,
                    }),
            )
            .show(ctx, |ui| {
                self.render_left_panel(ui);
            });

        // ═════════════════════════════════════════════════════════════════════
        // RIGHT MAIN AREA: Full Window Height & Remaining Width
        // ═════════════════════════════════════════════════════════════════════
        egui::CentralPanel::default()
            .frame(
                Frame::none()
                    .fill(Color32::from_rgb(15, 17, 23))
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

        // ═════════════════════════════════════════════════════════════════════
        // MODALS
        // ═════════════════════════════════════════════════════════════════════
        let modal_action = render_add_edit_modal(ctx, &mut self.add_edit_modal);
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
        let palette_action = render_command_palette(ctx, &mut self.palette_modal, &palette_items);
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
            _ => {}
        }

        if any_running {
            // When servers are running, repaint every 500ms for smooth stats & uptime display
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        } else {
            // When idle, sleep and repaint reactively on user interaction or 2s heartbeat
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
        }
    }
}

impl LauncherApp {
    // ═════════════════════════════════════════════════════════════════════════
    // LEFT SIDEBAR: Group Filter + Server Cards (Expands full height)
    // ═════════════════════════════════════════════════════════════════════════
    fn render_left_panel(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            // 1. Group Filter Pills Bar
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Group:").size(11.5).color(Color32::from_rgb(123, 131, 148)));

                let mut all_groups = vec!["All".to_string()];
                let mut unique_groups = BTreeSet::new();
                for s in &self.services {
                    unique_groups.insert(s.config.group.clone());
                }
                all_groups.extend(unique_groups);

                for grp in all_groups {
                    let is_active = self.active_group_filter == grp;
                    let bg_color = if is_active {
                        Color32::from_rgb(108, 92, 231)
                    } else {
                        Color32::from_rgb(37, 40, 51)
                    };
                    let text_color = if is_active {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(200, 205, 216)
                    };

                    let btn = egui::Button::new(RichText::new(&grp).size(11.0).color(text_color))
                        .fill(bg_color)
                        .rounding(Rounding::same(4.0))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 50, 65)));

                    if ui.add(btn).clicked() {
                        self.active_group_filter = grp.clone();
                    }
                }
            });

            ui.add_space(10.0);

            // 2. Scrollable list of Groups & Server Cards (NEVER shrinks, fills full height!)
            ScrollArea::vertical()
                .id_salt("left_cards_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
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
                            .fill(Color32::from_rgb(20, 24, 34))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(37, 44, 62)))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let header_text = format!("🏷  {} ({})", group_name.to_uppercase(), indices.len());
                                    ui.label(
                                        RichText::new(header_text)
                                            .size(12.0)
                                            .strong()
                                            .color(Color32::from_rgb(129, 140, 248)),
                                    );

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let btn_stop_grp = egui::Button::new(
                                            RichText::new("⏹ Stop Group").size(10.5).color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(239, 68, 68))
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_stop_grp).clicked() {
                                            stop_grp = Some(group_name.clone());
                                        }

                                        let btn_start_grp = egui::Button::new(
                                            RichText::new("▶ Start Group").size(10.5).color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(16, 185, 129))
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_start_grp).clicked() {
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

                            Frame::none()
                                .fill(Color32::from_rgb(21, 25, 36))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 46, 66)))
                                .rounding(Rounding::same(8.0))
                                .inner_margin(egui::Margin::symmetric(14.0, 11.0))
                                .show(ui, |ui| {
                                    // Row 1: Status Dot + Name + Group Badge + Status Text
                                    ui.horizontal(|ui| {
                                        let dot_color = if is_running {
                                            Color32::from_rgb(16, 185, 129)
                                        } else {
                                            Color32::from_rgb(100, 116, 139)
                                        };
                                        draw_status_dot(ui, dot_color);

                                        // Clickable name to view logs
                                        let name_resp = ui.selectable_label(
                                            false,
                                            RichText::new(&s.config.name)
                                                .strong()
                                                .size(14.5)
                                                .color(Color32::from_rgb(241, 245, 249)),
                                        );
                                        if name_resp.clicked() {
                                            to_switch_tab = Some(key.clone());
                                        }

                                        render_group_badge(ui, &s.config.group);

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let st_text = if is_running { "Running" } else { "Stopped" };
                                            let st_col = if is_running {
                                                Color32::from_rgb(16, 185, 129)
                                            } else {
                                                Color32::from_rgb(100, 116, 139)
                                            };
                                            ui.label(RichText::new(st_text).size(11.5).strong().color(st_col));
                                        });
                                    });

                                    ui.add_space(4.0);

                                    // Row 2: Subtitle / command
                                    Frame::none()
                                        .fill(Color32::from_rgb(14, 16, 24))
                                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 36, 52)))
                                        .rounding(Rounding::same(4.0))
                                        .inner_margin(egui::Margin::symmetric(7.0, 3.0))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(&s.config.command)
                                                    .size(11.0)
                                                    .monospace()
                                                    .color(Color32::from_rgb(160, 174, 200)),
                                            );
                                        });

                                    // Row 2.5: If running, display PID, CPU%, RAM MB, Uptime!
                                    if is_running {
                                        ui.add_space(5.0);
                                        ui.horizontal_wrapped(|ui| {
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

                                    ui.add_space(8.0);

                                    // Row 3: Action Buttons (Start, Stop, Restart, [ ] own console, Edit, Delete)
                                    ui.horizontal(|ui| {
                                        if is_running {
                                            let btn_stop = egui::Button::new(
                                                RichText::new("Stop").size(11.5).strong().color(Color32::WHITE),
                                            )
                                            .fill(Color32::from_rgb(239, 68, 68))
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(50.0, 24.0));
                                            if ui.add(btn_stop).clicked() {
                                                to_stop = Some(idx);
                                            }

                                            let btn_restart = egui::Button::new(
                                                RichText::new("Restart").size(11.5).strong().color(Color32::WHITE),
                                            )
                                            .fill(Color32::from_rgb(245, 158, 11))
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(56.0, 24.0));
                                            if ui.add(btn_restart).clicked() {
                                                to_restart = Some(idx);
                                            }
                                        } else {
                                            let btn_start = egui::Button::new(
                                                RichText::new("Start").size(11.5).strong().color(Color32::WHITE),
                                            )
                                            .fill(Color32::from_rgb(16, 185, 129))
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(54.0, 24.0));
                                            if ui.add(btn_start).clicked() {
                                                to_start = Some(idx);
                                            }
                                        }

                                        if ui.checkbox(
                                            &mut s.config.own_console,
                                            RichText::new("console").size(11.0).color(Color32::from_rgb(148, 163, 184)),
                                        ).changed() {
                                            to_save_config = true;
                                        }

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let btn_del = egui::Button::new(
                                                RichText::new("Delete").size(11.0).color(Color32::from_rgb(203, 213, 225)),
                                            )
                                            .fill(Color32::from_rgb(33, 38, 52))
                                            .rounding(Rounding::same(5.0));
                                            if ui.add(btn_del).clicked() {
                                                to_delete = Some(key.clone());
                                            }

                                            let btn_edit = egui::Button::new(
                                                RichText::new("Edit").size(11.0).color(Color32::from_rgb(203, 213, 225)),
                                            )
                                            .fill(Color32::from_rgb(33, 38, 52))
                                            .rounding(Rounding::same(5.0));
                                            if ui.add(btn_edit).clicked() {
                                                to_edit = Some(s.config.clone());
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
    fn render_right_panel(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            // Notebook Tabs Strip (matching screenshot)
            ui.horizontal(|ui| {
                // Tab 0: Server Overview
                let is_ov_active = self.active_tab == "overview";
                let ov_border = if is_ov_active {
                    Stroke::new(1.0_f32, Color32::from_rgb(228, 231, 238))
                } else {
                    Stroke::new(1.0_f32, Color32::from_rgb(45, 52, 70))
                };
                let ov_bg = if is_ov_active {
                    Color32::from_rgb(20, 23, 32)
                } else {
                    Color32::from_rgb(15, 17, 23)
                };

                let ov_btn = egui::Button::new(
                    RichText::new("  Server Overview  ")
                        .size(12.0)
                        .strong()
                        .color(if is_ov_active { Color32::WHITE } else { Color32::from_rgb(180, 185, 200) }),
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
                        Stroke::new(1.0_f32, Color32::from_rgb(228, 231, 238))
                    } else {
                        Stroke::new(1.0_f32, Color32::from_rgb(45, 52, 70))
                    };
                    let bg = if is_active {
                        Color32::from_rgb(20, 23, 32)
                    } else {
                        Color32::from_rgb(15, 17, 23)
                    };

                    let tab_btn = egui::Button::new(
                        RichText::new(format!("  {}  ", s.config.name))
                            .size(12.0)
                            .color(if is_active { Color32::WHITE } else { Color32::from_rgb(180, 185, 200) }),
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
                .fill(Color32::from_rgb(16, 18, 25))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 52, 70)))
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
        // Heading Row: Servers running on this PC | last scan: HH:MM:SS | Search Box | Refresh button
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Servers running on this PC")
                    .size(16.0)
                    .strong()
                    .color(Color32::from_rgb(235, 238, 245)),
            );
            ui.add_space(8.0);
            let scan_time = self.scanner.get_last_scan_time();
            ui.label(
                RichText::new(format!("last scan: {}", scan_time))
                    .size(11.5)
                    .color(Color32::from_rgb(123, 131, 148)),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn_refresh = egui::Button::new(
                    RichText::new("🔄 Refresh")
                        .size(11.5)
                        .strong()
                        .color(Color32::from_rgb(108, 92, 231)),
                )
                .fill(Color32::from_rgb(37, 40, 51))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(50, 55, 70)))
                .rounding(Rounding::same(4.0));
                if ui.add(btn_refresh).clicked() {
                    self.scanner.trigger_scan();
                }

                // Search Box with 🔍 icon, hint, clear button, and Ctrl+F focus support
                Frame::none()
                    .fill(Color32::from_rgb(22, 25, 34))
                    .stroke(Stroke::new(
                        1.0_f32,
                        if !self.overview_search_query.is_empty() {
                            Color32::from_rgb(108, 92, 231)
                        } else {
                            Color32::from_rgb(50, 56, 72)
                        },
                    ))
                    .rounding(Rounding::same(4.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 3.5))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("🔍")
                                    .size(11.5)
                                    .color(Color32::from_rgb(123, 131, 148)),
                            );
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.overview_search_query)
                                    .hint_text(
                                        RichText::new("Search servers, ports, commands... (Ctrl+F)")
                                            .size(11.5)
                                            .color(Color32::from_rgb(100, 116, 139)),
                                    )
                                    .desired_width(240.0)
                                    .frame(false),
                            );
                            if self.focus_overview_search {
                                resp.request_focus();
                                self.focus_overview_search = false;
                            }
                            if !self.overview_search_query.is_empty() {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("✕")
                                                .size(10.5)
                                                .color(Color32::from_rgb(148, 163, 184)),
                                        )
                                        .frame(false),
                                    )
                                    .clicked()
                                {
                                    self.overview_search_query.clear();
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
                        .color(Color32::from_rgb(228, 231, 238)),
                );
                ui.add_space(8.0);

                if managed_indices.is_empty() && !q.is_empty() {
                    Frame::none()
                        .fill(Color32::from_rgb(20, 23, 32))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(37, 42, 56)))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("No launcher-managed servers matching \"{}\"", q))
                                    .size(12.0)
                                    .color(Color32::from_rgb(148, 163, 184)),
                            );
                        });
                    ui.add_space(6.0);
                }

                let mut to_toggle: Option<(usize, u8)> = None; // 0=stop, 1=start, 2=restart

                for &idx in &managed_indices {
                    let s = &self.services[idx];
                    let is_running = s.state == ServiceState::Running || s.state == ServiceState::Starting;
                    Frame::none()
                        .fill(Color32::from_rgb(22, 25, 34))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(37, 42, 56)))
                        .rounding(Rounding::same(4.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 11.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let dot_color = if is_running {
                                    Color32::from_rgb(0, 210, 160)
                                } else {
                                    Color32::from_rgb(123, 131, 148)
                                };
                                draw_status_dot(ui, dot_color);

                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(&s.config.name)
                                                .size(13.5)
                                                .strong()
                                                .color(Color32::from_rgb(230, 234, 242)),
                                        );
                                        render_group_badge(ui, &s.config.group);
                                    });

                                    ui.label(
                                        RichText::new(&s.config.command)
                                            .size(10.5)
                                            .monospace()
                                            .color(Color32::from_rgb(123, 131, 148)),
                                    );

                                    if is_running {
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            if let Some(pid) = s.pid {
                                                render_metric_pill(
                                                    ui,
                                                    "🆔",
                                                    &format!("PID {}", pid),
                                                    Color32::from_rgb(148, 163, 184),
                                                );
                                            }
                                            render_metric_pill(
                                                ui,
                                                "⚡",
                                                &format!("{:.1}%", s.cpu_usage),
                                                Color32::from_rgb(96, 165, 250),
                                            );
                                            render_metric_pill(
                                                ui,
                                                "💾",
                                                &format!("{:.1} MB", s.memory_mb),
                                                Color32::from_rgb(52, 211, 153),
                                            );
                                            let uptime = s.uptime_formatted();
                                            if !uptime.is_empty() {
                                                render_metric_pill(
                                                    ui,
                                                    "⏱️",
                                                    &uptime,
                                                    Color32::from_rgb(251, 191, 36),
                                                );
                                            }
                                        });
                                    }
                                });

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if is_running {
                                        let btn_stop = egui::Button::new(
                                            RichText::new("Stop").size(11.5).strong().color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(239, 68, 68))
                                        .rounding(Rounding::same(5.0))
                                        .min_size(egui::vec2(50.0, 24.0));
                                        if ui.add(btn_stop).clicked() {
                                            to_toggle = Some((idx, 0));
                                        }

                                        let btn_restart = egui::Button::new(
                                            RichText::new("Restart").size(11.5).strong().color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(245, 158, 11))
                                        .rounding(Rounding::same(5.0))
                                        .min_size(egui::vec2(58.0, 24.0));
                                        if ui.add(btn_restart).clicked() {
                                            to_toggle = Some((idx, 2));
                                        }
                                    } else {
                                        let btn_start = egui::Button::new(
                                            RichText::new("Start").size(11.5).strong().color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(16, 185, 129))
                                        .rounding(Rounding::same(5.0))
                                        .min_size(egui::vec2(54.0, 24.0));
                                        if ui.add(btn_start).clicked() {
                                            to_toggle = Some((idx, 1));
                                        }
                                    }

                                    let st_text = if is_running { "Running" } else { "Stopped" };
                                    let st_color = if is_running {
                                        Color32::from_rgb(16, 185, 129)
                                    } else {
                                        Color32::from_rgb(148, 163, 184)
                                    };
                                    ui.label(RichText::new(st_text).size(12.0).strong().color(st_color));
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
                let mut to_import: Option<(String, u16, String, String)> = None;
                let mut to_start_stopped: Option<(u16, String, String)> = None;
                let mut to_remove_stopped: Option<u16> = None;

                let unmanaged_listeners: Vec<_> = listeners
                    .into_iter()
                    .filter(|l| !managed_ports.contains(&l.port))
                    .filter(|l| {
                        if q.is_empty() {
                            return true;
                        }
                        let (cmd_opt, _) = l.pid.map(read_proc_cmd_and_cwd).unwrap_or((None, None));
                        let cmd_str = cmd_opt.unwrap_or_default();
                        l.name.to_lowercase().contains(&q)
                            || l.port.to_string().contains(&q)
                            || l.proto.to_lowercase().contains(&q)
                            || cmd_str.to_lowercase().contains(&q)
                            || l.pid.map(|p| p.to_string().contains(&q)).unwrap_or(false)
                    })
                    .collect();

                let running_ports: BTreeSet<u16> = self
                    .scanner
                    .get_listeners()
                    .iter()
                    .map(|l| l.port)
                    .collect();

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
                            .color(Color32::from_rgb(228, 231, 238)),
                    );
                    ui.add_space(8.0);
                    let is_metrics_on = self.show_other_process_metrics;
                    ui.checkbox(
                        &mut self.show_other_process_metrics,
                        RichText::new("Show CPU & RAM usage")
                            .size(11.5)
                            .color(if is_metrics_on {
                                Color32::from_rgb(162, 155, 254)
                            } else {
                                Color32::from_rgb(148, 163, 184)
                            }),
                    );
                });
                ui.add_space(8.0);

                if unmanaged_listeners.is_empty() && visible_stopped.is_empty() {
                    Frame::none()
                        .fill(Color32::from_rgb(20, 23, 32))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(37, 42, 56)))
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
                                    .color(Color32::from_rgb(148, 163, 184)),
                            );
                        });
                } else {
                    for listener in &unmanaged_listeners {
                        let (cmd_opt, cwd_opt) = listener.pid.map(read_proc_cmd_and_cwd).unwrap_or((None, None));
                        let cmd_str = cmd_opt.unwrap_or_default();
                        let cwd_str = cwd_opt.unwrap_or_default();

                        Frame::none()
                            .fill(Color32::from_rgb(22, 25, 34))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(37, 42, 56)))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    let is_expanded = self.expanded_listeners.contains(&listener.port);
                                    ui.horizontal(|ui| {
                                        draw_status_dot(ui, Color32::from_rgb(16, 185, 129));

                                        let toggle_label = if is_expanded { "v" } else { ">" };
                                        let btn_toggle = egui::Button::new(
                                            RichText::new(toggle_label)
                                                .size(11.0)
                                                .strong()
                                                .color(if is_expanded { Color32::from_rgb(162, 155, 254) } else { Color32::from_rgb(148, 163, 184) }),
                                        )
                                        .fill(Color32::from_rgb(32, 36, 48))
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
                                                .color(Color32::from_rgb(230, 234, 242)),
                                        );
                                        ui.label(
                                            RichText::new(format!("({})", listener.proto))
                                                .size(11.0)
                                                .color(Color32::from_rgb(123, 131, 148)),
                                        );

                                        ui.add_space(4.0);
                                        if let Some(pid) = listener.pid {
                                            render_metric_pill(
                                                ui,
                                                "🆔",
                                                &format!("PID {}", pid),
                                                Color32::from_rgb(148, 163, 184),
                                            );
                                        }
                                        render_metric_pill(
                                            ui,
                                            "🔌",
                                            &format!(":{}", listener.port),
                                            Color32::from_rgb(96, 165, 250),
                                        );
                                        render_metric_pill(
                                            ui,
                                            "●",
                                            "Running",
                                            Color32::from_rgb(52, 211, 153),
                                        );

                                        if self.show_other_process_metrics {
                                            if let Some(pid) = listener.pid {
                                                let (cpu, mem) = self.other_metrics.get(&pid).copied().unwrap_or((0.0, 0.0));
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
                                            }
                                        }

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            // + Add Server button
                                            let btn_add = egui::Button::new(
                                                RichText::new("+ Add Server")
                                                    .size(11.5)
                                                    .strong()
                                                    .color(Color32::WHITE),
                                            )
                                            .fill(Color32::from_rgb(108, 92, 231))
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(86.0, 24.0));
                                            if ui.add(btn_add).clicked() {
                                                to_import = Some((
                                                    listener.name.clone(),
                                                    listener.port,
                                                    cmd_str.clone(),
                                                    cwd_str.clone(),
                                                ));
                                            }

                                            // Stop button
                                            let btn_stop = egui::Button::new(
                                                RichText::new("Stop").size(11.5).strong().color(Color32::WHITE),
                                            )
                                            .fill(Color32::from_rgb(239, 68, 68))
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
                                            .fill(Color32::from_rgb(245, 158, 11))
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

                                    // Expandable location & command details
                                    if is_expanded {
                                        ui.add_space(8.0);
                                        Frame::none()
                                            .fill(Color32::from_rgb(15, 17, 24))
                                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(37, 42, 56)))
                                            .rounding(Rounding::same(5.0))
                                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                            .show(ui, |ui| {
                                                ui.vertical(|ui| {
                                                    if !cwd_str.is_empty() {
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new("📁 Location:").size(11.0).strong().color(Color32::from_rgb(162, 155, 254)));
                                                            ui.label(
                                                                RichText::new(&cwd_str)
                                                                    .size(10.5)
                                                                    .monospace()
                                                                    .color(Color32::from_rgb(228, 231, 238)),
                                                            );
                                                        });
                                                    }
                                                    if !cmd_str.is_empty() {
                                                        if !cwd_str.is_empty() {
                                                            ui.add_space(4.0);
                                                        }
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new("⚙ Command:").size(11.0).strong().color(Color32::from_rgb(162, 155, 254)));
                                                            ui.label(
                                                                RichText::new(&cmd_str)
                                                                    .size(10.5)
                                                                    .monospace()
                                                                    .color(Color32::from_rgb(180, 186, 202)),
                                                            );
                                                        });
                                                    }
                                                    if cwd_str.is_empty() && cmd_str.is_empty() {
                                                        ui.label(
                                                            RichText::new("Location or command line details not accessible for this system process.")
                                                                .size(10.5)
                                                                .italics()
                                                                .color(Color32::from_rgb(123, 131, 148)),
                                                        );
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
                        Frame::none()
                            .fill(Color32::from_rgb(18, 20, 27))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 36, 48)))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    let is_expanded = self.expanded_listeners.contains(port);
                                    ui.horizontal(|ui| {
                                        draw_status_dot(ui, Color32::from_rgb(100, 116, 139));

                                        let toggle_label = if is_expanded { "v" } else { ">" };
                                        let btn_toggle = egui::Button::new(
                                            RichText::new(toggle_label)
                                                .size(11.0)
                                                .strong()
                                                .color(if is_expanded { Color32::from_rgb(162, 155, 254) } else { Color32::from_rgb(148, 163, 184) }),
                                        )
                                        .fill(Color32::from_rgb(28, 32, 44))
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
                                                .color(Color32::from_rgb(203, 213, 225)),
                                        );
                                        ui.label(
                                            RichText::new(format!(":{}", port))
                                                .size(11.5)
                                                .color(Color32::from_rgb(148, 163, 184)),
                                        );

                                        ui.add_space(4.0);
                                        render_metric_pill(
                                            ui,
                                            "🔌",
                                            &format!(":{}", port),
                                            Color32::from_rgb(148, 163, 184),
                                        );
                                        render_metric_pill(
                                            ui,
                                            "○",
                                            "Stopped",
                                            Color32::from_rgb(148, 163, 184),
                                        );

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let btn_del = egui::Button::new(
                                                RichText::new("✕").size(11.0).color(Color32::from_rgb(148, 163, 184)),
                                            )
                                            .fill(Color32::from_rgb(28, 32, 44))
                                            .rounding(Rounding::same(4.0));
                                            if ui.add(btn_del).clicked() {
                                                to_remove_stopped = Some(*port);
                                            }

                                            let btn_add = egui::Button::new(
                                                RichText::new("+ Add Server")
                                                    .size(11.5)
                                                    .strong()
                                                    .color(Color32::WHITE),
                                            )
                                            .fill(Color32::from_rgb(108, 92, 231))
                                            .rounding(Rounding::same(5.0))
                                            .min_size(egui::vec2(86.0, 24.0));
                                            if ui.add(btn_add).clicked() {
                                                to_import = Some((
                                                    name.clone(),
                                                    *port,
                                                    cmd.clone(),
                                                    cwd.clone(),
                                                ));
                                            }

                                            let btn_start = egui::Button::new(
                                                RichText::new("Start").size(11.5).strong().color(Color32::WHITE),
                                            )
                                            .fill(Color32::from_rgb(16, 185, 129))
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
                                            .fill(Color32::from_rgb(13, 15, 21))
                                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 36, 48)))
                                            .rounding(Rounding::same(5.0))
                                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                            .show(ui, |ui| {
                                                ui.vertical(|ui| {
                                                    if !cwd.is_empty() {
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new("📁 Location:").size(11.0).strong().color(Color32::from_rgb(162, 155, 254)));
                                                            ui.label(
                                                                RichText::new(cwd)
                                                                    .size(10.5)
                                                                    .monospace()
                                                                    .color(Color32::from_rgb(203, 213, 225)),
                                                            );
                                                        });
                                                    }
                                                    if !cmd.is_empty() {
                                                        if !cwd.is_empty() {
                                                            ui.add_space(4.0);
                                                        }
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new("⚙ Command:").size(11.0).strong().color(Color32::from_rgb(162, 155, 254)));
                                                            ui.label(
                                                                RichText::new(cmd)
                                                                    .size(10.5)
                                                                    .monospace()
                                                                    .color(Color32::from_rgb(180, 186, 202)),
                                                            );
                                                        });
                                                    }
                                                    if cwd.is_empty() && cmd.is_empty() {
                                                        ui.label(
                                                            RichText::new("No command line or location saved.")
                                                                .size(10.5)
                                                                .italics()
                                                                .color(Color32::from_rgb(123, 131, 148)),
                                                        );
                                                    }
                                                });
                                            });
                                    }
                                });
                            });

                        ui.add_space(6.0);
                    }
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

                if let Some((name, port, cmd, cwd)) = to_import {
                    self.add_edit_modal.open_import(&name, port, &cmd, &cwd);
                }
            });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Right Panel Tab 1+: Server Log View + Ctrl+F Search + Command Bar
    // ─────────────────────────────────────────────────────────────────────────
    fn render_server_log_content(&mut self, ui: &mut Ui, key: &str) {
        let (name, is_running, pid, cpu, mem, uptime, actions, links) = {
            if let Some(s) = self.find_service(key) {
                (
                    s.config.name.clone(),
                    s.state == ServiceState::Running || s.state == ServiceState::Starting,
                    s.pid,
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

        // Sub-header above logs: Service name + Metrics + Find toggle + Start/Stop/Restart
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&name)
                    .size(15.0)
                    .strong()
                    .color(Color32::from_rgb(228, 231, 238)),
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
                    .fill(Color32::from_rgb(208, 64, 48))
                    .rounding(Rounding::same(4.0));
                    if ui.add(btn_stop).clicked() {
                        if let Some(s) = self.find_service_mut(key) {
                            s.stop();
                        }
                    }

                    let btn_restart = egui::Button::new(
                        RichText::new("Restart").size(11.0).strong().color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(230, 126, 34))
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
                    .fill(Color32::from_rgb(0, 210, 160))
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
                        .color(Color32::from_rgb(200, 205, 216)),
                )
                .fill(Color32::from_rgb(37, 40, 51))
                .rounding(Rounding::same(4.0));
                if ui.add(find_btn).clicked() {
                    self.search_open.insert(key.to_string(), !search_is_open);
                }
            });
        });

        // Quick Sub-command & Link Buttons Bar (if defined)
        if !actions.is_empty() || !links.is_empty() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for act in &actions {
                    let act_btn = egui::Button::new(
                        RichText::new(format!("⚡ {}", act.label))
                            .size(10.5)
                            .color(Color32::from_rgb(253, 203, 110)),
                    )
                    .fill(Color32::from_rgb(37, 40, 51))
                    .rounding(Rounding::same(4.0));
                    if ui.add(act_btn).clicked() {
                        self.run_service_action(key, &act.command);
                    }
                }

                for lnk in &links {
                    let lnk_btn = egui::Button::new(
                        RichText::new(format!("🔗 {}", lnk.label))
                            .size(10.5)
                            .color(Color32::from_rgb(116, 185, 255)),
                    )
                    .fill(Color32::from_rgb(37, 40, 51))
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
                .fill(Color32::from_rgb(26, 30, 43))
                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                .rounding(Rounding::same(4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("🔍").color(Color32::from_rgb(108, 92, 231)));
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
                        ui.checkbox(filter_entry, RichText::new("Filter lines").size(11.0).color(Color32::from_rgb(200, 205, 216)));

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

        let available_height = ui.available_height() - 44.0;
        ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .max_height(available_height)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                for entry in &logs_vec {
                    if filter_active && !entry.text.to_lowercase().contains(&q) {
                        continue;
                    }

                    let base_color = match entry.kind {
                        LogKind::Launcher => Color32::from_rgb(124, 138, 255),
                        LogKind::Stdin => Color32::from_rgb(0, 210, 160),
                        LogKind::Stderr | LogKind::Error => Color32::from_rgb(235, 87, 87),
                        LogKind::Warning => Color32::from_rgb(243, 156, 18),
                        LogKind::Stdout => Color32::from_rgb(200, 205, 216),
                    };

                    let contains_query = !q.is_empty() && entry.text.to_lowercase().contains(&q);
                    let mut rt = RichText::new(format!("[{}] {}", entry.timestamp, entry.text))
                        .color(base_color)
                        .monospace()
                        .size(11.0);

                    if contains_query {
                        rt = rt.background_color(Color32::from_rgb(180, 80, 0)).color(Color32::WHITE);
                    }

                    ui.label(rt);
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

        let resp = ui.horizontal(|ui| {
            ui.label(RichText::new("❯").color(Color32::from_rgb(108, 92, 231)).strong().size(13.0));

            let text_edit = egui::TextEdit::singleline(&mut input_val)
                .hint_text("Type '/' for commands, or run command in directory (e.g. npm i, cargo test)...")
                .desired_width(ui.available_width() - 250.0);

            let edit_resp = ui.add(text_edit);

            if edit_resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                do_send = true;
            }

            let btn_send = egui::Button::new(RichText::new("Send").size(11.0).strong().color(Color32::WHITE))
                .fill(Color32::from_rgb(108, 92, 231))
                .rounding(Rounding::same(4.0));
            if ui.add(btn_send).clicked() {
                do_send = true;
            }

            let btn_ctrlc = egui::Button::new(RichText::new("Ctrl+C").size(11.0).color(Color32::from_rgb(253, 203, 110)))
                .fill(Color32::from_rgb(37, 40, 51))
                .rounding(Rounding::same(4.0));
            if ui.add(btn_ctrlc).clicked() {
                if let Some(s) = self.find_service(key) {
                    s.send_interrupt();
                }
            }

            let btn_clear = egui::Button::new(RichText::new("Clear").size(11.0).color(Color32::from_rgb(123, 131, 148)))
                .fill(Color32::from_rgb(37, 40, 51))
                .rounding(Rounding::same(4.0));
            if ui.add(btn_clear).clicked() {
                if let Some(s) = self.find_service(key) {
                    s.clear_logs();
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
                        .fill(Color32::from_rgb(22, 26, 38))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(60, 68, 95)))
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
                                ui.label(RichText::new("⚡ Commands").size(11.0).strong().color(Color32::from_rgb(162, 155, 254)));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new("↑↓ Navigate  •  Enter Select  •  Tab Complete  •  Esc Close").size(9.5).color(Color32::from_rgb(120, 126, 142)));
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
                                            Color32::from_rgb(45, 52, 75)
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
                                                    Color32::from_rgb(162, 155, 254)
                                                } else {
                                                    Color32::from_rgb(235, 238, 245)
                                                };
                                                ui.label(RichText::new(&cmd.name).strong().size(12.0).color(name_color));
                                                ui.add_space(6.0);
                                                ui.label(RichText::new(&cmd.description).size(11.0).color(Color32::from_rgb(150, 155, 172)));

                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    let (badge_bg, badge_fg) = match cmd.category {
                                                        "Control" => (Color32::from_rgb(48, 40, 80), Color32::from_rgb(180, 160, 255)),
                                                        "Action" => (Color32::from_rgb(70, 50, 20), Color32::from_rgb(255, 185, 90)),
                                                        "Link" => (Color32::from_rgb(20, 50, 60), Color32::from_rgb(100, 210, 240)),
                                                        _ => (Color32::from_rgb(37, 40, 51), Color32::from_rgb(160, 165, 180)),
                                                    };
                                                    Frame::none()
                                                        .fill(badge_bg)
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
                    if is_running {
                        if let Some(s) = self.find_service(key) {
                            s.send_input(&trimmed);
                        }
                    } else {
                        // Server is NOT running: execute directly in cwd!
                        self.run_service_action(key, &trimmed);
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
