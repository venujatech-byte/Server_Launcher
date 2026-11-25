use super::*;

#[derive(Clone, Default)]
struct ServerLogViewCache {
    logs: Arc<Vec<crate::service::LogEntry>>,
    filter: String,
    matches: Option<Arc<Vec<usize>>>,
}

impl LauncherApp {
    // ═════════════════════════════════════════════════════════════════════════
    // RIGHT PANEL: Notebook Tabs Bar + Overview / Server Log View
    // ═════════════════════════════════════════════════════════════════════════
    pub(in crate::app) fn render_servers_right_panel(&mut self, ui: &mut Ui) {
        let p = self.palette();
        ui.vertical(|ui| {
            let online_count = self.services.iter().filter(|s| s.state == ServiceState::Running).count();
            let total_count = self.services.len();
            let keys: Vec<_> = self.services.iter().map(|s| s.config.key.clone()).collect();
            sync_server_tab_order(&mut self.server_tab_order, &keys);
            let ordered: Vec<_> = self.server_tab_order.iter()
                .filter_map(|key| self.services.iter().find(|s| s.config.key == *key)).collect();
            let mut to_switch_tab: Option<String> = None;
            let mut reorder: Option<(String, String, bool)> = None;
            let (tab_width, tab_gap) = notebook_tab_metrics(ui.available_width(), ordered.len() + 1);

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = tab_gap;
                let overview_active = self.active_tab == "overview";
                let mut label = egui::text::LayoutJob::default();
                label.append(&format!("{} Overview", crate::lucide::SERVER), 0.0, egui::TextFormat {
                    font_id: egui::FontId::proportional(12.0),
                    color: if overview_active { p.text_primary } else { p.text_secondary },
                    ..Default::default()
                });
                label.append(&format!("  {online_count}/{total_count}"), 0.0, egui::TextFormat {
                    font_id: egui::FontId::proportional(10.5),
                    color: if online_count > 0 { p.success } else { p.text_muted },
                    ..Default::default()
                });
                let response = ui.push_id("tab_overview", |ui| render_notebook_tab(ui, label, tab_width, overview_active, &p))
                    .inner.hover_tip(format!("Overview · {online_count} running · {total_count} configured"));
                if response.clicked() { to_switch_tab = Some("overview".to_string()); }

                for server in &ordered {
                    let active = self.active_tab == server.config.key;
                    let (dot, color, state) = if server.restart_at.is_some() {
                        ("●", p.warning, "Restarting")
                    } else {
                        match server.state {
                            ServiceState::Running => ("●", p.success, "Running"),
                            ServiceState::Starting => ("●", p.warning, "Starting"),
                            ServiceState::Stopping => ("●", p.warning, "Stopping"),
                            ServiceState::Errored => ("●", p.danger, "Errored"),
                            ServiceState::Stopped => ("○", p.text_muted, "Stopped"),
                        }
                    };
                    let mut label = egui::text::LayoutJob::default();
                    label.append(&format!("{dot}  "), 0.0, egui::TextFormat {
                        font_id: egui::FontId::proportional(10.5), color, ..Default::default()
                    });
                    label.append(&server.config.name, 0.0, egui::TextFormat {
                        font_id: egui::FontId::proportional(12.0),
                        color: if active { p.text_primary } else { p.text_secondary },
                        ..Default::default()
                    });
                    let response = ui.push_id(("server_tab", &server.config.key), |ui| render_notebook_tab(ui, label, tab_width, active, &p))
                        .inner.hover_tip(format!("{} · {}\nDrag to reorder", server.config.name, state));
                    response.dnd_set_drag_payload(ServerTabDrag { key: server.config.key.clone() });
                    if let Some(payload) = response.dnd_hover_payload::<ServerTabDrag>() {
                        if payload.key != server.config.key {
                            let after = ui.input(|i| i.pointer.interact_pos()).is_some_and(|pos| pos.x >= response.rect.center().x);
                            let x = if after { response.rect.right() } else { response.rect.left() };
                            ui.painter().line_segment([
                                egui::pos2(x, response.rect.top() + 3.0),
                                egui::pos2(x, response.rect.bottom() - 3.0),
                            ], Stroke::new(2.0_f32, p.accent));
                            if let Some(payload) = response.dnd_release_payload::<ServerTabDrag>() {
                                reorder = Some((payload.key.clone(), server.config.key.clone(), after));
                            }
                        }
                    }
                    if response.clicked() { to_switch_tab = Some(server.config.key.clone()); }
                }
            });
            if let Some(payload) = egui::DragAndDrop::payload::<ServerTabDrag>(ui.ctx()) {
                if let (Some(pos), Some(server)) = (ui.input(|i| i.pointer.interact_pos()), self.services.iter().find(|s| s.config.key == payload.key)) {
                    egui::Area::new(egui::Id::new("dragged_server_tab")).order(egui::Order::Tooltip)
                        .fixed_pos(pos + egui::vec2(10.0, 12.0)).interactable(false).show(ui.ctx(), |ui| {
                            Frame::none().fill(p.bg_card_active).rounding(Rounding::same(6.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 6.0)).show(ui, |ui| {
                                    ui.set_max_width(200.0);
                                    ui.add(egui::Label::new(RichText::new(&server.config.name).size(12.0).color(p.text_primary)).truncate());
                                });
                        });
                }
            }
            if let Some((source, target, after)) = reorder {
                if move_server_tab(&mut self.server_tab_order, &source, &target, after) { self.persist_config(); }
            }

            if let Some(target) = to_switch_tab { self.active_tab = target; }

            // Outer Frame for the tab content (FILLS 100% OF REMAINING SPACE!)
            Frame::none()
                .fill(p.bg_main)
                .stroke(Stroke::NONE)
                .rounding(Rounding { nw: 0.0, ne: 6.0, sw: 6.0, se: 6.0 })
                .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                .show(ui, |ui| {
                    ui.set_height(ui.available_height().max(100.0));
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
    // Right Panel Tab 1+: Server Log View + Ctrl+F Search + Command Bar
    // ─────────────────────────────────────────────────────────────────────────
    pub(in crate::app) fn render_server_log_content(&mut self, ui: &mut Ui, key: &str) {
        let (name, is_running, pid, port, port_open, cpu, mem, uptime, actions, links, cwd, state) = {
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
                    s.config.cwd.clone(),
                    s.state,
                )
            } else {
                ui.label("Service not found.");
                return;
            }
        };

        // ─────────────────────────────────────────────────────────────────────
        // Section A: Upper Controls Cockpit (Card)
        // ─────────────────────────────────────────────────────────────────────
        let p = self.palette();
        let (status_label, status_color) = match state {
            ServiceState::Running => ("Running", p.success),
            ServiceState::Starting => ("Starting", p.warning),
            ServiceState::Stopping => ("Stopping", p.warning),
            ServiceState::Errored => ("Failed", p.danger),
            ServiceState::Stopped => ("Stopped", p.text_muted),
        };
        Frame::none()
            .fill(p.bg_card)
            .stroke(Stroke::new(1.0_f32, p.border))
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(10.0, 5.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().button_padding = egui::vec2(6.0, 2.0);
                ui.spacing_mut().interact_size.y = 22.0;
                ui.spacing_mut().icon_width = 12.0;
                ui.spacing_mut().icon_spacing = 4.0;
                // Title and state share a row with the primary server actions.
                ui.horizontal(|ui| {
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
                            RichText::new(format!("{} Find", crate::lucide::SEARCH))
                                .size(11.0)
                                .color(if search_is_open { p.accent } else { p.text_secondary }),
                        )
                        .fill(if search_is_open { p.accent.gamma_multiply(0.25) } else { p.bg_button })
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(4.0));
                        if ui.add(find_btn).hover_tip("Find in output · Ctrl+F").clicked() {
                            self.search_open.insert(key.to_string(), !search_is_open);
                        }
                        if is_running {
                            let pending = state == ServiceState::Starting && pid.is_none();
                            let mut readings = vec![
                                (crate::lucide::ACTIVITY, if pending { "—".into() } else { format!("{cpu:.1}%") }, "CPU usage reported for the server process tree".to_string(), None),
                                (crate::lucide::RAM, if pending { "—".into() } else { format!("{mem:.1} MiB") }, "Resident memory reported for the server process tree".to_string(), None),
                            ];
                            if port > 0 {
                                let address = format!("127.0.0.1:{port}");
                                let listener = if port_open { "Listening" } else { "Waiting for listener" };
                                readings.insert(0, (crate::lucide::PLUG, format!(":{port}"), format!("{listener} · Click to copy {address}"), Some(address)));
                            }
                            if !uptime.is_empty() {
                                readings.push((crate::lucide::TIMER, uptime.clone(), "Time since this server started".to_string(), None));
                            }
                            let metrics: Vec<_> = readings.into_iter().map(|(icon, value, tooltip, copy)| {
                                let mut label = egui::text::LayoutJob::default();
                                label.append(icon, 0.0, egui::TextFormat {
                                    font_id: egui::FontId::proportional(11.0),
                                    color: if copy.is_some() && port_open { p.info } else { p.text_muted },
                                    ..Default::default()
                                });
                                label.append(&format!(" {value}"), 0.0, egui::TextFormat {
                                    font_id: egui::FontId::monospace(10.5), color: p.text_secondary, ..Default::default()
                                });
                                (label, tooltip, copy)
                            }).collect();
                            let metrics_width = ui.fonts(|fonts| metrics.iter().map(|(label, _, copy)| {
                                fonts.layout_job(label.clone()).size().x + if copy.is_some() { 12.0 } else { 0.0 }
                            }).sum::<f32>()) + 10.0 * metrics.len().saturating_sub(1) as f32;
                            let render_metrics = |ui: &mut Ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                for (label, tooltip, copy) in &metrics {
                                    if let Some(address) = copy {
                                        if ui.add(egui::Button::new(label.clone()).small().frame(false))
                                            .hover_tip(tooltip).clicked() { ui.ctx().copy_text(address.clone()); }
                                    } else {
                                        ui.add(egui::Label::new(label.clone())).hover_tip(tooltip);
                                    }
                                }
                            };
                            // Keep the title readable without adding another row above the output.
                            if ui.available_width() >= metrics_width + 150.0 {
                                ui.allocate_ui_with_layout(egui::vec2(metrics_width, 22.0), egui::Layout::left_to_right(egui::Align::Center), render_metrics);
                            } else {
                                ui.menu_button(RichText::new(format!("{} Stats", crate::lucide::ACTIVITY)).size(10.5), render_metrics)
                                    .response.hover_tip("Show port, CPU, memory and uptime");
                            }
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let mut title = egui::text::LayoutJob::default();
                            title.append(&name, 0.0, egui::TextFormat {
                                font_id: egui::FontId::proportional(14.0), color: p.text_primary, ..Default::default()
                            });
                            title.append(&format!("  {status_label}"), 0.0, egui::TextFormat {
                                font_id: egui::FontId::proportional(10.5), color: status_color, ..Default::default()
                            });
                            ui.add(egui::Label::new(title).truncate()).hover_tip(format!("{name} · {status_label}\nWorking directory: {cwd}"));
                        });
                    });
                });
                egui::CollapsingHeader::new(RichText::new("Tools & actions").size(11.0).color(p.text_secondary))
                    .id_salt(("server_console_tools", key))
                    .default_open(false)
                    .show(ui, |ui| {
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
                            .rounding(Rounding::same(4.0));
                            if ui.add(vscode_btn).hover_tip("Open working directory in VS Code (code .)").clicked() {
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
                            .rounding(Rounding::same(4.0));
                            if ui.add(folder_btn).hover_tip("Open working directory in file manager (xdg-open)").clicked() {
                                let cwd = self.find_service(key).map(|s| s.config.cwd.clone()).unwrap_or_default();
                                crate::service::open_folder(&cwd);
                            }

                            let console_btn = egui::Button::new(
                                RichText::new(format!("{} Console", crate::lucide::TERMINAL))
                                    .size(11.0)
                                    .color(p.text_secondary),
                            )
                            .fill(p.bg_button)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(4.0));
                            if ui.add(console_btn).hover_tip("Open interactive console / terminal window in working directory").clicked() {
                                if let Some(s) = self.find_service(key) {
                                    s.open_console();
                                }
                            }

                            ui.separator();

                            let mut auto_scroll = self.auto_scroll.get(key).copied().unwrap_or(true);
                            if ui.checkbox(
                                &mut auto_scroll,
                                RichText::new("Auto-scroll").size(11.0).color(p.text_secondary),
                            ).hover_tip("Keep pinned to bottom when new logs arrive (uncheck to scroll up smoothly without jitter)").changed() {
                                self.auto_scroll.insert(key.to_string(), auto_scroll);
                            }

                            let mut autostart = self.find_service(key).map(|s| s.config.autostart).unwrap_or(false);
                            if ui.checkbox(
                                &mut autostart,
                                RichText::new("Autostart").size(11.0).color(p.text_secondary),
                            ).hover_tip("Automatically start this server when Server Launcher opens").changed() {
                                if let Some(s) = self.find_service_mut(key) {
                                    s.config.autostart = autostart;
                                    self.persist_config();
                                }
                            }

                            let mut auto_restart = self.find_service(key).map(|s| s.config.auto_restart).unwrap_or(false);
                            if ui.checkbox(
                                &mut auto_restart,
                                RichText::new("Auto-restart").size(11.0).color(p.text_secondary),
                            ).hover_tip("Auto-restart this server if it exits unexpectedly or crashes").changed() {
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

                    });

                // Collapsible Search Bar (Ctrl+F)
                let search_is_open = self.search_open.get(key).copied().unwrap_or(false);
                let find_just_opened = ui.ctx().data_mut(|data| {
                    let id = egui::Id::new(("server_find_open", key));
                    let previous = data.get_temp::<bool>(id).unwrap_or(false);
                    data.insert_temp(id, search_is_open);
                    search_is_open && !previous
                });
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
                                        .desired_width((ui.available_width() - 130.0).max(60.0)),
                                );
                                if find_just_opened {
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
        let cache_id = egui::Id::new("active_server_log_snapshot");
        let mut cache = ui.ctx().data(|data| data.get_temp::<ServerLogViewCache>(cache_id)).unwrap_or_default();
        let q = self.search_queries.get(key).cloned().unwrap_or_default().to_lowercase();
        let filter_active = self.filter_lines.get(key).copied().unwrap_or(false) && !q.is_empty();
        let filter = if filter_active { q.as_str() } else { "" };
        let snapshot = self.find_service(key)
            .and_then(|service| service.logs.lock().ok())
            .map(|logs| {
                // Entries are immutable and writers only append, evict from the front, or clear.
                let same_entry = |left: Option<&crate::service::LogEntry>, right: Option<&crate::service::LogEntry>| {
                    match (left, right) {
                        (Some(left), Some(right)) => Arc::ptr_eq(&left.text, &right.text),
                        (None, None) => true,
                        _ => false,
                    }
                };
                let unchanged = logs.len() == cache.logs.len()
                    && same_entry(logs.front(), cache.logs.first())
                    && same_entry(logs.back(), cache.logs.last());
                (!unchanged).then(|| logs.iter().cloned().collect())
            })
            .unwrap_or_else(|| (!cache.logs.is_empty()).then(Vec::new));
        let logs_changed = snapshot.is_some();
        if let Some(snapshot) = snapshot {
            cache.logs = Arc::new(snapshot);
        }
        if logs_changed || cache.filter != filter {
            cache.filter = filter.into();
            cache.matches = filter_active.then(|| Arc::new(cache.logs.iter().enumerate()
                .filter_map(|(index, entry)| entry.text.to_lowercase().contains(filter).then_some(index))
                .collect()));
            ui.ctx().data_mut(|data| data.insert_temp(cache_id, cache.clone()));
        }
        let logs_vec = &cache.logs;

        let bottom_dock_reserve = 48.0;
        let terminal_available_height = (ui.available_height() - bottom_dock_reserve).max(100.0);
        let frame_v_margin = 8.0_f32;
        let inner_terminal_height = (terminal_available_height - frame_v_margin).max(80.0);
        let auto_scroll_enabled = self.auto_scroll.get(key).copied().unwrap_or(true);
        let row_height = 17.0_f32;
        let total_rows = cache.matches.as_ref().map_or(logs_vec.len(), |matches| matches.len());

        Frame::none()
            .fill(p.terminal_bg)
            .stroke(Stroke::new(1.0_f32, p.border.gamma_multiply(0.7)))
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                ui.set_height(inner_terminal_height);
                ui.set_max_height(inner_terminal_height);
                ui.spacing_mut().item_spacing.y = 1.0;
                if total_rows == 0 {
                    ui.vertical_centered(|ui| {
                        ui.add_space(((inner_terminal_height - 76.0) * 0.4).max(8.0));
                        ui.label(RichText::new(crate::lucide::TERMINAL).size(16.0).color(p.text_muted));
                        let title = if filter_active { "No matching log lines" } else if is_running { "Waiting for output" } else { "No output yet" };
                        let detail = if filter_active { "Try another search or turn off Filter lines." } else if is_running { "Server output will appear here." } else { "Start this server above, or run a command below." };
                        ui.label(RichText::new(title).size(13.0).color(p.text_secondary));
                        ui.label(RichText::new(detail).size(11.0).color(p.text_muted));
                    });
                } else {
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
                                    let index = cache.matches.as_ref().map_or(idx, |matches| matches[idx]);
                                    if let Some(entry) = logs_vec.get(index) {
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
                }
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
                ui.set_width(ui.available_width());
                ui.spacing_mut().button_padding = egui::vec2(6.0, 2.0);
                ui.spacing_mut().interact_size.y = 24.0;
                let edit_resp = ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(RichText::new(">").monospace().color(p.accent).size(12.0))
                        .hover_tip(format!("Working directory: {cwd}"));
                    if is_running {
                        ui.menu_button(RichText::new(crate::lucide::ZAP).size(11.0).color(p.text_secondary), |ui| {
                            for (label, command, tip) in [
                                ("Help (h)", "h", "Show dev server help and shortcuts"),
                                ("Restart (r)", "r", "Restart the dev server"),
                                ("Open browser (o)", "o", "Open the server in a browser"),
                                ("Show URLs (u)", "u", "Show the server URLs"),
                                ("Clear console (c)", "c", "Clear the dev server console"),
                                ("Quit (q)", "q", "Quit the dev server"),
                            ] {
                                if ui.button(label).hover_tip(tip).clicked() {
                                    if let Some(server) = self.find_service(key) { server.send_input(command); }
                                    ui.close_menu();
                                }
                            }
                        }).response.hover_tip("Dev server shortcuts");
                    }
                    let hint = if is_running { "Send input… · / commands" } else { "Run a command… · / commands" };
                    let input_width = (ui.available_width() - 48.0 - 3.0 * 24.0 - 4.0 * ui.spacing().item_spacing.x).max(48.0);
                    let edit_resp = ui.add_sized([input_width, 24.0], egui::TextEdit::singleline(&mut input_val)
                        .font(egui::FontId::monospace(11.5)).hint_text(hint).desired_width(input_width))
                        .hover_tip("Enter to submit · Type / for available commands");
                    if edit_resp.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) { do_send = true; }
                    if ui.add_enabled(!input_val.trim().is_empty(), egui::Button::new(RichText::new("Send").size(11.0).color(p.accent_text))
                        .fill(p.accent).rounding(Rounding::same(4.0)).min_size(egui::vec2(48.0, 24.0))).clicked() { do_send = true; }
                    if ui.add_enabled(pid.is_some() || is_running, egui::Button::new(RichText::new(crate::lucide::STOP).size(11.0).color(Color32::WHITE))
                        .fill(p.danger).rounding(Rounding::same(4.0)).min_size(egui::vec2(24.0, 24.0)))
                        .hover_tip("Send Ctrl+C to the server").clicked() {
                        if let Some(server) = self.find_service(key) { server.send_interrupt(); }
                    }
                    if ui.add_enabled(!logs_vec.is_empty(), egui::Button::new(RichText::new(crate::lucide::TRASH).size(11.0).color(p.text_secondary))
                        .fill(p.bg_button).rounding(Rounding::same(4.0)).min_size(egui::vec2(24.0, 24.0)))
                        .hover_tip("Clear output").clicked() {
                        if let Some(server) = self.find_service(key) { server.clear_logs(); }
                    }
                    if ui.add_sized([24.0, 24.0], egui::Button::new(RichText::new(crate::lucide::EXTERNAL_LINK).size(11.0).color(p.text_secondary))
                        .fill(p.bg_button).rounding(Rounding::same(4.0)))
                        .hover_tip("Open external console").clicked() {
                        if let Some(server) = self.find_service(key) { server.open_console(); }
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
                                    ui.label(RichText::new("Up/Down Navigate  •  Enter Select  •  Tab Complete  •  Esc Close").size(9.5).color(p.text_muted));
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
