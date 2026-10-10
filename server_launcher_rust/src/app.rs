use crate::config::ConfigFile;
use crate::modals::{
    render_add_edit_modal, render_command_palette, AddEditModalState, CommandPaletteState,
    ModalAction, PaletteAction,
};
use crate::service::{LogKind, Service, ServiceState};
use eframe::egui;
use egui::{Color32, Context, Key, Modifiers, RichText, Rounding, ScrollArea, Stroke, Ui};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Instant;
use sysinfo::System;

pub struct LauncherApp {
    config_path: PathBuf,
    services: Vec<Service>,
    active_tab: String, // "overview" or service.key

    // System monitor
    sys: System,
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

        Self {
            config_path,
            services,
            active_tab: "overview".to_string(),
            sys: System::new_all(),
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

        // Server Start / Stop / View
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

        // Groups
        let mut groups = std::collections::BTreeSet::new();
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
        // Poll services and sysinfo periodically (every 800ms)
        if self.last_poll.elapsed() >= std::time::Duration::from_millis(800) {
            self.sys.refresh_all();
            for s in &mut self.services {
                s.poll_status(&mut self.sys);
            }
            self.last_poll = Instant::now();
        }

        // Keyboard shortcuts
        self.handle_global_shortcuts(ctx);

        // Top Navigation Bar
        egui::TopBottomPanel::top("top_header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("⚡ Server Launcher").strong().size(15.0));

                let running_count = self
                    .services
                    .iter()
                    .filter(|s| s.state == ServiceState::Running)
                    .count();
                ui.label(
                    RichText::new(format!(
                        "│  {}/{} running",
                        running_count,
                        self.services.len()
                    ))
                    .weak(),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("+ Add Server").clicked() {
                        self.add_edit_modal.open_new();
                    }
                    if ui.button("⏹ Stop All (Ctrl+X)").clicked() {
                        self.stop_all();
                    }
                    if ui
                        .button(RichText::new("▶ Start All (Ctrl+A)").color(Color32::from_rgb(0, 210, 160)))
                        .clicked()
                    {
                        self.start_all();
                    }
                    if ui.button("⚡ Quick Launch (Ctrl+P)").clicked() {
                        self.palette_modal.open = true;
                        self.palette_modal.query.clear();
                    }
                });
            });
            ui.add_space(6.0);
        });

        // Tab Strip Panel
        egui::TopBottomPanel::top("tab_strip").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let ov_selected = self.active_tab == "overview";
                if ui
                    .selectable_label(
                        ov_selected,
                        RichText::new("  Overview  ").strong(),
                    )
                    .clicked()
                {
                    self.active_tab = "overview".to_string();
                }

                for s in &self.services {
                    let is_active = self.active_tab == s.config.key;
                    let dot_color = match s.state {
                        ServiceState::Running => Color32::from_rgb(0, 210, 160),
                        ServiceState::Starting => Color32::from_rgb(243, 156, 18),
                        ServiceState::Stopping => Color32::from_rgb(230, 126, 34),
                        ServiceState::Stopped => Color32::from_rgb(120, 130, 150),
                        ServiceState::Errored => Color32::from_rgb(235, 87, 87),
                    };

                    let label = RichText::new(format!("● {}", s.config.name)).color(dot_color);
                    if ui.selectable_label(is_active, label).clicked() {
                        self.active_tab = s.config.key.clone();
                    }
                }
            });
        });

        // Main Central Panel
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.active_tab == "overview" {
                self.render_overview_tab(ui);
            } else {
                let key = self.active_tab.clone();
                self.render_service_tab(ui, &key);
            }
        });

        // Render Modals
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

        // Request repaint so live logs, CPU stats, and terminal outputs update smoothly
        ctx.request_repaint_after(std::time::Duration::from_millis(150));
    }
}

impl LauncherApp {
    fn render_overview_tab(&mut self, ui: &mut Ui) {
        ScrollArea::vertical().show(ui, |ui| {
            // Group services
            let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
            for (idx, s) in self.services.iter().enumerate() {
                groups
                    .entry(s.config.group.clone())
                    .or_default()
                    .push(idx);
            }

            let mut to_start = None;
            let mut to_stop = None;
            let mut to_edit = None;
            let mut to_delete = None;
            let mut to_view = None;
            let mut start_grp = None;
            let mut stop_grp = None;

            for (group_name, indices) in groups {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.heading(RichText::new(&group_name).size(15.0).strong());
                    ui.label(RichText::new(format!("({} servers)", indices.len())).weak());

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("⏹ Stop Group").clicked() {
                            stop_grp = Some(group_name.clone());
                        }
                        if ui.button("▶ Start Group").clicked() {
                            start_grp = Some(group_name.clone());
                        }
                    });
                });
                ui.separator();

                // Render grid of cards
                egui::Grid::new(format!("grid_{}", group_name))
                    .num_columns(3)
                    .spacing([12.0, 12.0])
                    .show(ui, |ui| {
                        let mut col = 0;
                        for idx in indices {
                            let s = &self.services[idx];
                            egui::Frame::none()
                                .fill(Color32::from_rgb(26, 30, 43))
                                .rounding(Rounding::same(6.0))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 52, 70)))
                                .inner_margin(10.0)
                                .show(ui, |ui| {
                                    ui.set_width(280.0);
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            let (dot_color, state_txt) = match s.state {
                                                ServiceState::Running => (Color32::from_rgb(0, 210, 160), "RUNNING"),
                                                ServiceState::Starting => (Color32::from_rgb(243, 156, 18), "STARTING"),
                                                ServiceState::Stopping => (Color32::from_rgb(230, 126, 34), "STOPPING"),
                                                ServiceState::Stopped => (Color32::from_rgb(120, 130, 150), "STOPPED"),
                                                ServiceState::Errored => (Color32::from_rgb(235, 87, 87), "ERROR"),
                                            };
                                            ui.label(RichText::new("●").color(dot_color).size(14.0));
                                            ui.label(RichText::new(&s.config.name).strong().size(14.0));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.label(RichText::new(state_txt).size(10.0).color(dot_color));
                                            });
                                        });

                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            if s.config.port > 0 {
                                                let p_col = if s.port_open {
                                                    Color32::from_rgb(0, 210, 160)
                                                } else {
                                                    Color32::from_rgb(120, 130, 150)
                                                };
                                                ui.label(RichText::new(format!("Port: {}", s.config.port)).color(p_col).size(11.0));
                                            }
                                            if s.state == ServiceState::Running {
                                                ui.label(RichText::new(format!("CPU: {:.1}%  RAM: {:.1} MB", s.cpu_usage, s.memory_mb)).weak().size(11.0));
                                            }
                                        });

                                        ui.label(RichText::new(format!("Dir: {}", s.config.cwd)).weak().size(10.0));

                                        ui.add_space(6.0);
                                        ui.horizontal(|ui| {
                                            if s.state == ServiceState::Running {
                                                if ui.button(RichText::new("Stop").color(Color32::from_rgb(235, 87, 87))).clicked() {
                                                    to_stop = Some(idx);
                                                }
                                            } else {
                                                if ui.button(RichText::new("Start").color(Color32::from_rgb(0, 210, 160))).clicked() {
                                                    to_start = Some(idx);
                                                }
                                            }

                                            if ui.button("Logs").clicked() {
                                                to_view = Some(s.config.key.clone());
                                            }
                                            if ui.button("Edit").clicked() {
                                                to_edit = Some(s.config.clone());
                                            }
                                            if ui.button("🗑").clicked() {
                                                to_delete = Some(s.config.key.clone());
                                            }
                                        });
                                    });
                                });

                            col += 1;
                            if col >= 3 {
                                ui.end_row();
                                col = 0;
                            }
                        }
                    });
            }

            if let Some(g) = start_grp {
                self.start_group(&g);
            }
            if let Some(g) = stop_grp {
                self.stop_group(&g);
            }
            if let Some(idx) = to_start {
                self.services[idx].start();
            }
            if let Some(idx) = to_stop {
                self.services[idx].stop();
            }
            if let Some(k) = to_view {
                self.active_tab = k;
            }
            if let Some(cfg) = to_edit {
                self.add_edit_modal.open_edit(&cfg);
            }
            if let Some(k) = to_delete {
                if let Some(idx) = self.services.iter().position(|s| s.config.key == k) {
                    let mut s = self.services.remove(idx);
                    s.stop();
                    self.persist_config();
                }
            }
        });
    }

    fn render_service_tab(&mut self, ui: &mut Ui, key: &str) {
        let (name, is_running, port, port_open, cpu, ram) = {
            if let Some(s) = self.find_service(key) {
                (
                    s.config.name.clone(),
                    s.state == ServiceState::Running,
                    s.config.port,
                    s.port_open,
                    s.cpu_usage,
                    s.memory_mb,
                )
            } else {
                ui.label("Service not found.");
                return;
            }
        };

        // Header controls for this service
        ui.horizontal(|ui| {
            ui.heading(RichText::new(&name).size(16.0));
            if port > 0 {
                let col = if port_open {
                    Color32::from_rgb(0, 210, 160)
                } else {
                    Color32::from_rgb(120, 130, 150)
                };
                ui.label(RichText::new(format!("Port: {}", port)).color(col));
            }
            if is_running {
                ui.label(RichText::new(format!("CPU: {:.1}%  RAM: {:.1} MB", cpu, ram)).weak());
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let search_is_open = self.search_open.get(key).copied().unwrap_or(false);
                let find_btn_text = if search_is_open {
                    "🔍 Find [Active]"
                } else {
                    "🔍 Find (Ctrl+F)"
                };
                if ui.button(find_btn_text).clicked() {
                    self.search_open.insert(key.to_string(), !search_is_open);
                }

                if is_running {
                    if ui.button(RichText::new("⏹ Stop").color(Color32::from_rgb(235, 87, 87))).clicked() {
                        if let Some(s) = self.find_service_mut(key) {
                            s.stop();
                        }
                    }
                } else {
                    if ui.button(RichText::new("▶ Start").color(Color32::from_rgb(0, 210, 160))).clicked() {
                        if let Some(s) = self.find_service_mut(key) {
                            s.start();
                        }
                    }
                }
            });
        });
        ui.separator();

        // Collapsible Search Bar (Ctrl+F)
        let search_is_open = self.search_open.get(key).copied().unwrap_or(false);
        if search_is_open {
            ui.horizontal(|ui| {
                ui.label(RichText::new("🔍").color(Color32::from_rgb(108, 92, 231)));
                let q_entry = self.search_queries.entry(key.to_string()).or_default();
                let search_input = ui.add(
                    egui::TextEdit::singleline(q_entry)
                        .hint_text("Search logs…")
                        .desired_width(220.0),
                );
                search_input.request_focus();

                let filter_entry = self.filter_lines.entry(key.to_string()).or_default();
                ui.checkbox(filter_entry, "Filter lines");

                if ui.button("✕ Close").clicked() {
                    self.search_open.insert(key.to_string(), false);
                }
            });
            ui.separator();
        }

        // Log content view
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

        let available_height = ui.available_height() - 40.0;
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
                        .monospace();

                    if contains_query {
                        rt = rt.background_color(Color32::from_rgb(180, 80, 0)).color(Color32::WHITE);
                    }

                    ui.label(rt);
                }
            });

        ui.separator();

        // Bottom Interactive Command Row
        ui.horizontal(|ui| {
            ui.label(RichText::new("❯").color(Color32::from_rgb(108, 92, 231)).strong());
            let mut input_val = self.input_texts.get(key).cloned().unwrap_or_default();
            let mut do_send = false;

            let resp = ui.add(
                egui::TextEdit::singleline(&mut input_val)
                    .hint_text("Send input to running server or terminal…")
                    .desired_width(ui.available_width() - 200.0),
            );

            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                do_send = true;
            }

            if ui.button(RichText::new("Send").strong()).clicked() {
                do_send = true;
            }

            let mut send_interrupt_req = false;
            if ui.button("Ctrl+C").clicked() {
                send_interrupt_req = true;
            }

            let mut clear_logs_req = false;
            if ui.button("Clear Logs").clicked() {
                clear_logs_req = true;
            }

            if send_interrupt_req {
                if let Some(s) = self.find_service(key) {
                    s.send_interrupt();
                }
            }

            if clear_logs_req {
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
