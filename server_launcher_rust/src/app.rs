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
}

impl LauncherApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config_path = ConfigFile::default_path();
        let config_file = ConfigFile::load_from_file(&config_path);

        let services = config_file
            .servers
            .into_iter()
            .map(Service::new)
            .collect();

        let scanner = SystemPortScanner::new();
        scanner.trigger_scan();

        Self {
            config_path,
            services,
            active_tab: "overview".to_string(),
            active_group_filter: "All".to_string(),
            lan_ip: get_lan_ip(),
            sys: System::new_all(),
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

    fn collect_palette_actions(&self) -> Vec<(String, PaletteAction)> {
        let mut items = Vec::new();

        for s in &self.services {
            if s.state == ServiceState::Running {
                items.push((
                    format!("Stop  {}", s.config.name),
                    PaletteAction::StopServer(s.config.key.clone()),
                ));
            } else {
                items.push((
                    format!("Start  {}", s.config.name),
                    PaletteAction::StartServer(s.config.key.clone()),
                ));
            }
            items.push((
                format!("View Logs:  {}", s.config.name),
                PaletteAction::SelectTab(s.config.key.clone()),
            ));
        }

        let mut groups = BTreeSet::new();
        for s in &self.services {
            groups.insert(s.config.group.clone());
        }
        for g in groups {
            items.push((
                format!("Start Group: {}", g),
                PaletteAction::StartGroup(g.clone()),
            ));
            items.push((
                format!("Stop Group: {}", g),
                PaletteAction::StopGroup(g.clone()),
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

        // Ctrl+F: Find in logs
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F))
            || ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::F))
        {
            if self.active_tab == "overview" {
                if let Some(first) = self.services.first() {
                    self.active_tab = first.config.key.clone();
                }
            }
            if self.active_tab != "overview" {
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

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Periodic poll (every 800ms)
        if self.last_poll.elapsed() >= std::time::Duration::from_millis(800) {
            self.sys.refresh_all();
            for s in &mut self.services {
                s.poll_status(&mut self.sys);
            }
            self.last_poll = Instant::now();
        }

        self.handle_global_shortcuts(ctx);

        // ═════════════════════════════════════════════════════════════════════
        // TOP HEADER BAR (Exactly as in screenshot)
        // ═════════════════════════════════════════════════════════════════════
        egui::TopBottomPanel::top("top_header")
            .frame(
                Frame::none()
                    .fill(Color32::from_rgb(15, 17, 23))
                    .inner_margin(egui::Margin {
                        left: 16.0,
                        right: 16.0,
                        top: 12.0,
                        bottom: 12.0,
                    }),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Left: Server Launcher title & details
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Server Launcher").size(20.0).strong().color(Color32::from_rgb(228, 231, 238)));
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(format!(
                                "this pc: {}    servers: {}",
                                self.lan_ip,
                                self.services.len()
                            ))
                            .size(11.0)
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
                        .rounding(Rounding::same(4.0));
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
                        .rounding(Rounding::same(4.0));
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
                        .rounding(Rounding::same(4.0));
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
                        .rounding(Rounding::same(4.0));
                        if ui.add(btn_start_all).clicked() {
                            self.start_all();
                        }
                    });
                });
            });

        // ═════════════════════════════════════════════════════════════════════
        // MAIN TWO-COLUMN BODY
        // ═════════════════════════════════════════════════════════════════════
        egui::CentralPanel::default()
            .frame(Frame::none().fill(Color32::from_rgb(15, 17, 23)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // LEFT COLUMN: Groups, Filter Pills, Server Cards (~340px)
                    self.render_left_panel(ui);

                    // Vertical Separator
                    ui.add_space(2.0);

                    // RIGHT COLUMN: Tabs (Overview & Server Logs)
                    self.render_right_panel(ui);
                });
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
            ModalAction::ExecutePaletteAction(PaletteAction::SelectTab(k)) => {
                self.active_tab = k;
            }
            ModalAction::ExecutePaletteAction(PaletteAction::StartGroup(g)) => {
                self.start_group(&g);
            }
            ModalAction::ExecutePaletteAction(PaletteAction::StopGroup(g)) => {
                self.stop_group(&g);
            }
            _ => {}
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(150));
    }
}

impl LauncherApp {
    // ═════════════════════════════════════════════════════════════════════════
    // LEFT SIDEBAR: Group Filter + Server Cards
    // ═════════════════════════════════════════════════════════════════════════
    fn render_left_panel(&mut self, ui: &mut Ui) {
        ui.allocate_ui_with_layout(
            egui::vec2(330.0, ui.available_height()),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.add_space(4.0);

                // 1. Group Filter Pills Bar
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Group:").size(11.0).color(Color32::from_rgb(123, 131, 148)));

                    // Collect unique groups
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

                ui.add_space(8.0);

                // 2. Scrollable list of Groups & Server Cards
                ScrollArea::vertical().id_salt("left_cards_scroll").show(ui, |ui| {
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
                    let mut to_edit = None;
                    let mut to_delete = None;
                    let mut to_switch_tab = None;
                    let mut start_grp = None;
                    let mut stop_grp = None;

                    for (group_name, indices) in groups {
                        ui.add_space(8.0);
                        // Group Section Header
                        Frame::none()
                            .fill(Color32::from_rgb(19, 22, 30))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(37, 42, 56)))
                            .rounding(Rounding::same(4.0))
                            .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let header_text = format!("🏷  {} ({})", group_name.to_uppercase(), indices.len());
                                    ui.label(RichText::new(header_text).size(11.0).strong().color(Color32::from_rgb(116, 185, 255)));

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let btn_stop_grp = egui::Button::new(RichText::new("⏹ Stop Group").size(10.0).color(Color32::WHITE))
                                            .fill(Color32::from_rgb(208, 64, 48))
                                            .rounding(Rounding::same(3.0));
                                        if ui.add(btn_stop_grp).clicked() {
                                            stop_grp = Some(group_name.clone());
                                        }

                                        let btn_start_grp = egui::Button::new(RichText::new("▶ Start Group").size(10.0).color(Color32::WHITE))
                                            .fill(Color32::from_rgb(0, 210, 160))
                                            .rounding(Rounding::same(3.0));
                                        if ui.add(btn_start_grp).clicked() {
                                            start_grp = Some(group_name.clone());
                                        }
                                    });
                                });
                            });

                        ui.add_space(4.0);

                        // Server Cards for this group
                        for idx in indices {
                            let s = &mut self.services[idx];
                            let key = s.config.key.clone();
                            let is_running = s.state == ServiceState::Running || s.state == ServiceState::Starting;

                            Frame::none()
                                .fill(Color32::from_rgb(22, 25, 34))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(37, 42, 56)))
                                .rounding(Rounding::same(6.0))
                                .inner_margin(10.0)
                                .show(ui, |ui| {
                                    // Row 1: Status Dot + Name + Group Badge + Status Text
                                    ui.horizontal(|ui| {
                                        let dot_color = if is_running {
                                            Color32::from_rgb(0, 210, 160)
                                        } else {
                                            Color32::from_rgb(123, 131, 148)
                                        };
                                        ui.label(RichText::new("●").color(dot_color).size(13.0));

                                        // Clickable name to view logs
                                        let name_resp = ui.selectable_label(
                                            false,
                                            RichText::new(&s.config.name).strong().size(13.0).color(Color32::from_rgb(228, 231, 238)),
                                        );
                                        if name_resp.clicked() {
                                            to_switch_tab = Some(key.clone());
                                        }

                                        // Group Badge Pill
                                        Frame::none()
                                            .fill(Color32::from_rgb(45, 35, 75))
                                            .rounding(Rounding::same(10.0))
                                            .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new(&s.config.group).size(10.0).color(Color32::from_rgb(162, 155, 254)));
                                            });

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let st_text = if is_running { "Running" } else { "Stopped" };
                                            let st_col = if is_running { Color32::from_rgb(0, 210, 160) } else { Color32::from_rgb(123, 131, 148) };
                                            ui.label(RichText::new(st_text).size(11.0).color(st_col));
                                        });
                                    });

                                    ui.add_space(2.0);

                                    // Row 2: Subtitle / command
                                    ui.label(RichText::new(&s.config.command).size(11.0).monospace().color(Color32::from_rgb(123, 131, 148)));

                                    ui.add_space(6.0);

                                    // Row 3: Action Buttons (Start, [ ] own console, Edit, Delete)
                                    ui.horizontal(|ui| {
                                        if is_running {
                                            let btn_stop = egui::Button::new(RichText::new("Stop").size(11.0).strong().color(Color32::WHITE))
                                                .fill(Color32::from_rgb(208, 64, 48))
                                                .rounding(Rounding::same(4.0));
                                            if ui.add(btn_stop).clicked() {
                                                to_stop = Some(idx);
                                            }
                                        } else {
                                            let btn_start = egui::Button::new(RichText::new("Start").size(11.0).strong().color(Color32::WHITE))
                                                .fill(Color32::from_rgb(0, 210, 160))
                                                .rounding(Rounding::same(4.0));
                                            if ui.add(btn_start).clicked() {
                                                to_start = Some(idx);
                                            }
                                        }

                                        ui.checkbox(&mut s.config.own_console, RichText::new("own console").size(10.0).color(Color32::from_rgb(123, 131, 148)));

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let btn_del = egui::Button::new(RichText::new("Delete").size(11.0).color(Color32::from_rgb(200, 205, 216)))
                                                .fill(Color32::from_rgb(37, 40, 51))
                                                .rounding(Rounding::same(4.0));
                                            if ui.add(btn_del).clicked() {
                                                to_delete = Some(key.clone());
                                            }

                                            let btn_edit = egui::Button::new(RichText::new("Edit").size(11.0).color(Color32::from_rgb(200, 205, 216)))
                                                .fill(Color32::from_rgb(37, 40, 51))
                                                .rounding(Rounding::same(4.0));
                                            if ui.add(btn_edit).clicked() {
                                                to_edit = Some(s.config.clone());
                                            }
                                        });
                                    });
                                });

                            ui.add_space(6.0);
                        }
                    }

                    if let Some(g) = start_grp { self.start_group(&g); }
                    if let Some(g) = stop_grp { self.stop_group(&g); }
                    if let Some(idx) = to_start { self.services[idx].start(); }
                    if let Some(idx) = to_stop { self.services[idx].stop(); }
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
            },
        );
    }

    // ═════════════════════════════════════════════════════════════════════════
    // RIGHT PANEL: Notebook Tabs Bar + Overview / Server Log View
    // ═════════════════════════════════════════════════════════════════════════
    fn render_right_panel(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.add_space(4.0);

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
                    Color32::from_rgb(22, 25, 34)
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
                        Color32::from_rgb(22, 25, 34)
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

            // Outer Frame for the tab content (matches screenshot's framed border)
            Frame::none()
                .fill(Color32::from_rgb(15, 17, 23))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 52, 70)))
                .rounding(Rounding { nw: 0.0, ne: 4.0, sw: 4.0, se: 4.0 })
                .inner_margin(12.0)
                .show(ui, |ui| {
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
    // Right Panel Tab 0: "Server Overview"
    // ─────────────────────────────────────────────────────────────────────────
    fn render_overview_content(&mut self, ui: &mut Ui) {
        // Heading Row: Servers running on this PC | last scan: HH:MM:SS | Refresh button
        ui.horizontal(|ui| {
            ui.label(RichText::new("Servers running on this PC").size(16.0).strong().color(Color32::from_rgb(228, 231, 238)));
            ui.add_space(8.0);
            let scan_time = self.scanner.get_last_scan_time();
            ui.label(RichText::new(format!("last scan: {}", scan_time)).size(11.0).color(Color32::from_rgb(123, 131, 148)));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn_refresh = egui::Button::new(RichText::new("Refresh").size(11.0).strong().color(Color32::from_rgb(108, 92, 231)))
                    .fill(Color32::from_rgb(37, 40, 51))
                    .rounding(Rounding::same(4.0));
                if ui.add(btn_refresh).clicked() {
                    self.scanner.trigger_scan();
                }
            });
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        ScrollArea::vertical().id_salt("overview_scroll_body").show(ui, |ui| {
            // Section 1: Launcher-managed servers
            ui.label(RichText::new("Launcher-managed servers").size(13.0).strong().color(Color32::from_rgb(228, 231, 238)));
            ui.add_space(6.0);

            let mut to_toggle = None;

            for (idx, s) in self.services.iter().enumerate() {
                let is_running = s.state == ServiceState::Running || s.state == ServiceState::Starting;
                Frame::none()
                    .fill(Color32::from_rgb(22, 25, 34))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(37, 42, 56)))
                    .rounding(Rounding::same(4.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let dot_color = if is_running { Color32::from_rgb(0, 210, 160) } else { Color32::from_rgb(123, 131, 148) };
                            ui.label(RichText::new("●").color(dot_color).size(13.0));

                            ui.vertical(|ui| {
                                ui.label(RichText::new(&s.config.name).size(13.0).strong().color(Color32::from_rgb(228, 231, 238)));
                                ui.label(RichText::new(&s.config.command).size(11.0).monospace().color(Color32::from_rgb(123, 131, 148)));
                            });

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if is_running {
                                    let btn = egui::Button::new(RichText::new("Stop").size(11.0).strong().color(Color32::WHITE))
                                        .fill(Color32::from_rgb(208, 64, 48))
                                        .rounding(Rounding::same(4.0));
                                    if ui.add(btn).clicked() { to_toggle = Some((idx, false)); }
                                } else {
                                    let btn = egui::Button::new(RichText::new("Start").size(11.0).strong().color(Color32::WHITE))
                                        .fill(Color32::from_rgb(0, 210, 160))
                                        .rounding(Rounding::same(4.0));
                                    if ui.add(btn).clicked() { to_toggle = Some((idx, true)); }
                                }

                                let st_text = if is_running { "Running" } else { "Stopped" };
                                let st_color = if is_running { Color32::from_rgb(0, 210, 160) } else { Color32::from_rgb(123, 131, 148) };
                                ui.label(RichText::new(st_text).size(11.0).color(st_color));
                            });
                        });
                    });

                ui.add_space(4.0);
            }

            if let Some((idx, start)) = to_toggle {
                if start { self.services[idx].start(); } else { self.services[idx].stop(); }
            }

            ui.add_space(14.0);

            // Section 2: Other processes listening on this PC
            ui.label(RichText::new("Other processes listening on this PC").size(13.0).strong().color(Color32::from_rgb(228, 231, 238)));
            ui.add_space(6.0);

            let listeners = self.scanner.get_listeners();
            let managed_ports: BTreeSet<u16> = self.services.iter().map(|s| s.config.port).filter(|&p| p > 0).collect();

            if listeners.is_empty() {
                ui.label(RichText::new("No other listening processes detected.").size(11.0).weak());
            } else {
                for listener in listeners {
                    if managed_ports.contains(&listener.port) {
                        continue;
                    }

                    Frame::none()
                        .fill(Color32::from_rgb(22, 25, 34))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(37, 42, 56)))
                        .rounding(Rounding::same(4.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("●").color(Color32::from_rgb(0, 210, 160)).size(13.0));

                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&listener.name).size(13.0).strong().color(Color32::from_rgb(228, 231, 238)));
                                    ui.label(
                                        RichText::new(format!("{} :{} ({})", listener.name, listener.port, listener.proto))
                                            .size(11.0)
                                            .color(Color32::from_rgb(123, 131, 148)),
                                    );
                                });

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new("Running").size(11.0).color(Color32::from_rgb(0, 210, 160)));
                                    ui.label(RichText::new(format!(":{}", listener.port)).size(12.0).color(Color32::from_rgb(123, 131, 148)));
                                });
                            });
                        });

                    ui.add_space(4.0);
                }
            }
        });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Right Panel Tab 1+: Server Log View + Ctrl+F Search + Command Bar
    // ─────────────────────────────────────────────────────────────────────────
    fn render_server_log_content(&mut self, ui: &mut Ui, key: &str) {
        let (name, is_running) = {
            if let Some(s) = self.find_service(key) {
                (s.config.name.clone(), s.state == ServiceState::Running)
            } else {
                ui.label("Service not found.");
                return;
            }
        };

        // Sub-header above logs: Service name + Find toggle + Start/Stop
        ui.horizontal(|ui| {
            ui.label(RichText::new(&name).size(15.0).strong().color(Color32::from_rgb(228, 231, 238)));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if is_running {
                    let btn_stop = egui::Button::new(RichText::new("Stop").size(11.0).strong().color(Color32::WHITE))
                        .fill(Color32::from_rgb(208, 64, 48))
                        .rounding(Rounding::same(4.0));
                    if ui.add(btn_stop).clicked() {
                        if let Some(s) = self.find_service_mut(key) { s.stop(); }
                    }
                } else {
                    let btn_start = egui::Button::new(RichText::new("Start").size(11.0).strong().color(Color32::WHITE))
                        .fill(Color32::from_rgb(0, 210, 160))
                        .rounding(Rounding::same(4.0));
                    if ui.add(btn_start).clicked() {
                        if let Some(s) = self.find_service_mut(key) { s.start(); }
                    }
                }

                let search_is_open = self.search_open.get(key).copied().unwrap_or(false);
                let find_btn = egui::Button::new(RichText::new("🔍 Find (Ctrl+F)").size(11.0).color(Color32::from_rgb(200, 205, 216)))
                    .fill(Color32::from_rgb(37, 40, 51))
                    .rounding(Rounding::same(4.0));
                if ui.add(find_btn).clicked() {
                    self.search_open.insert(key.to_string(), !search_is_open);
                }
            });
        });

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

        // Log Terminal Output Area
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

        // Bottom Interactive Command Row (matching screenshot's interactive prompt)
        ui.horizontal(|ui| {
            ui.label(RichText::new("❯").color(Color32::from_rgb(108, 92, 231)).strong().size(13.0));
            let mut input_val = self.input_texts.get(key).cloned().unwrap_or_default();
            let mut do_send = false;

            let resp = ui.add(
                egui::TextEdit::singleline(&mut input_val)
                    .hint_text("Run command in directory (e.g. npm i, pip install) or send input...")
                    .desired_width(ui.available_width() - 250.0),
            );

            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
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

            if do_send {
                let to_send = input_val.trim().to_string();
                if !to_send.is_empty() {
                    if let Some(s) = self.find_service(key) {
                        s.send_input(&to_send);
                    }
                    input_val.clear();
                }
            }

            self.input_texts.insert(key.to_string(), input_val);
        });
    }
}
