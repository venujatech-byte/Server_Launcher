use super::*;

impl LauncherApp {
    // ═════════════════════════════════════════════════════════════════════════
    // LEFT SIDEBAR: Group Filter + Server Cards (Expands full height)
    // ═════════════════════════════════════════════════════════════════════════
    pub(in crate::app) fn render_servers_left_panel(&mut self, ui: &mut Ui) {
        let p = self.palette();
        ui.set_width(ui.available_width());
        ui.set_max_width(ui.available_width());
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.set_max_width(ui.available_width());
            ui.add_space(8.0);
            // Reserve the row height before laying out children so their centres stay aligned.
            // Keep filters scrollable while the Add action stays visible.
            ui.horizontal(|ui| {
                ui.set_height(28.0);
                ui.set_min_height(28.0);
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(RichText::new("Group").size(10.5).color(p.text_muted));
                let filters_width = (ui.available_width() - 54.0 - ui.spacing().item_spacing.x).max(0.0);
                ui.allocate_ui_with_layout(egui::vec2(filters_width, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ScrollArea::horizontal()
                        .id_salt("server_groups_hscroll")
                        .auto_shrink([false, true])
                        .max_height(24.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.set_height(24.0);
                                ui.set_min_height(24.0);
                                ui.spacing_mut().item_spacing.x = 4.0;
                                ui.spacing_mut().button_padding = egui::vec2(9.0, 3.0);
                                let mut all_groups = vec!["All".to_string()];
                                let unique_groups: BTreeSet<_> = self.services.iter().map(|service| service.config.group.clone()).collect();
                                all_groups.extend(unique_groups);
                                for group in all_groups {
                                    let selected = self.active_group_filter == group;
                                    let button = egui::Button::new(RichText::new(&group).size(10.5)
                                        .color(if selected { p.accent } else { p.text_secondary }))
                                        .fill(if selected { p.accent_subtle } else { p.bg_subtle })
                                        .rounding(Rounding::same(8.0)).stroke(Stroke::NONE)
                                        .min_size(egui::vec2(32.0, 24.0));
                                    if ui.add(button).hover_tip(format!("Show {} servers", group)).clicked() {
                                        self.active_group_filter = group;
                                    }
                                }
                            });
                        });
                });
                let add = egui::Button::new(RichText::new(format!("{} Add", crate::lucide::PLUS)).size(10.5).color(p.accent))
                    .fill(p.bg_button).rounding(Rounding::same(8.0)).stroke(Stroke::NONE);
                if ui.add_sized([54.0, 24.0], add).hover_tip("Configure and add a new server").clicked() {
                    self.add_edit_modal.open_new();
                }
            });

            ui.add_space(10.0);

            // 2. Scrollable list of Groups & Server Cards (NEVER shrinks, fills full height!)
            ScrollArea::vertical()
                .id_salt("left_cards_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let list_width = ui.available_width().min(SERVERS_SIDEBAR_WIDTH);
                    ui.set_width(list_width);
                    ui.set_max_width(list_width);
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
                    for (idx, s) in self.services.iter().enumerate() {
                        if self.active_group_filter == "All" || self.active_group_filter == s.config.group {
                            groups
                                .entry(s.config.group.clone())
                                .or_default()
                                .push(idx);
                        }
                    }

                    if groups.is_empty() {
                        ui.add_space(20.0);
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::new(1.0_f32, p.border))
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(24.0, 24.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.vertical_centered(|ui| {
                                    // Icon circle with theme accent glow
                                    Frame::none()
                                        .fill(accent_with_alpha(p.accent, 22))
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::same(14.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(crate::lucide::SERVER).size(28.0).color(p.accent));
                                        });

                                    ui.add_space(12.0);

                                    if self.services.is_empty() {
                                        ui.label(RichText::new("Server Orchestration").size(16.0).strong().color(p.text_primary));
                                        ui.add_space(6.0);
                                        ui.label(
                                            RichText::new("Servers let you launch, monitor, and manage backend services, web applications, and database processes directly from Server Launcher. Track real-time resource telemetry, stream terminal logs, configure automatic restarts, and control port bindings in one place.")
                                                .size(11.5)
                                                .color(p.text_muted)
                                        );
                                        ui.add_space(16.0);
                                        let add_first_btn = egui::Button::new(RichText::new(format!("{} Add Your First Server", crate::lucide::PLUS)).size(12.0).strong().color(p.accent_text))
                                            .fill(p.accent)
                                            .rounding(Rounding::same(999.0))
                                            .min_size(egui::vec2(170.0, 32.0));
                                        if ui.add(add_first_btn).clicked() {
                                            self.add_edit_modal.open_new();
                                        }
                                    } else {
                                        ui.label(RichText::new(format!("No Servers in Group '{}'", self.active_group_filter)).size(15.0).strong().color(p.text_primary));
                                        ui.add_space(6.0);
                                        ui.label(RichText::new("No configured server matches the selected group filter.").size(11.5).color(p.text_muted));
                                        ui.add_space(14.0);
                                        let reset_btn = egui::Button::new(RichText::new("Show All Servers").size(12.0).color(p.text_primary))
                                            .fill(p.bg_button)
                                            .rounding(Rounding::same(999.0))
                                            .min_size(egui::vec2(130.0, 28.0));
                                        if ui.add(reset_btn).clicked() {
                                            self.active_group_filter = "All".to_string();
                                        }
                                    }
                                });
                            });
                    }

                    let mut to_start = None;
                    let mut to_stop = None;
                    let mut to_restart = None;
                    let mut to_edit = None;
                    let mut to_delete = None;
                    let mut to_switch_tab = None;
                    let mut to_toggle_collapse = None;
                    let mut start_grp = None;
                    let mut stop_grp = None;
                    let mut to_save_config = false;

                    for (group_name, indices) in groups {
                        let group_running = indices.iter().any(|&i| matches!(self.services[i].state, ServiceState::Running | ServiceState::Starting)
                            || self.services[i].restart_at.is_some());
                        let group_startable = indices.iter().any(|&i| !matches!(self.services[i].state, ServiceState::Running | ServiceState::Starting)
                            && self.services[i].restart_at.is_none());
                        // Quiet group heading; filled emphasis belongs to the group action.
                        Frame::none()
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 2.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let header_text = format!("{} · {}", group_name, indices.len());
                                    let title_w = (ui.available_width() - 168.0).max(40.0);
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(title_w, 20.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.set_max_width(title_w);
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(header_text)
                                                        .size(10.5)
                                                        .strong()
                                                        .color(p.text_secondary),
                                                )
                                                .truncate(),
                                            );
                                        },
                                    );

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        let btn_stop_grp = egui::Button::new(
                                            RichText::new(format!("{} Stop all", crate::lucide::STOP_FILLED)).size(10.5).strong().color(Color32::WHITE),
                                        )
                                        .fill(p.danger)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .min_size(egui::vec2(76.0, 22.0));
                                        if ui.add_enabled(group_running, btn_stop_grp).hover_tip("Stop all servers in this group").disabled_hover_tip("No running servers in this group").clicked() {
                                            stop_grp = Some(group_name.clone());
                                        }

                                        let btn_start_grp = egui::Button::new(
                                            RichText::new(format!("{} Start all", crate::lucide::PLAY_FILLED)).size(10.5).strong().color(p.accent_text),
                                        )
                                        .fill(p.accent)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .min_size(egui::vec2(76.0, 22.0));
                                        if ui.add_enabled(group_startable, btn_start_grp).hover_tip("Start all servers in this group").disabled_hover_tip("All servers in this group are already running").clicked() {
                                            start_grp = Some(group_name.clone());
                                        }
                                    });
                                });
                            });

                        ui.add_space(2.0);

                        // Theme-aware server cards with minimize/expand support.
                        for idx in indices {
                            let s = &mut self.services[idx];
                            let key = s.config.key.clone();
                            let is_running = s.state == ServiceState::Running || s.state == ServiceState::Starting;
                            let is_selected_tab = self.active_tab == key;
                            let is_collapsed = !self.expanded_server_cards.contains(&key);

                            let card_bg = if is_selected_tab {
                                p.bg_card_active
                            } else {
                                p.bg_card
                            };

                            let (chip_bg, chip_dot, chip_text, status_label) = if s.restart_at.is_some() {
                                (p.warning_subtle, p.warning, p.warning, "Restarting")
                            } else if s.state == ServiceState::Starting {
                                (p.warning_subtle, p.warning, p.warning, "Starting")
                            } else if is_running {
                                (p.success_subtle, p.success, p.success, "Running")
                            } else if s.state == ServiceState::Errored {
                                (p.danger_subtle, p.danger, p.danger, "Errored")
                            } else {
                                (Color32::TRANSPARENT, p.text_muted, p.text_secondary, "Stopped")
                            };

                            if is_collapsed {
                                // ─── Collapsed / Minimized Card ───────────────────────
                                Frame::none()
                                    .fill(card_bg)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(12.0))
                                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.spacing_mut().item_spacing.y = 3.0;
                                        ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.text_primary);
                                        ui.visuals_mut().widgets.active.bg_stroke = Stroke::new(1.0_f32, p.accent);
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 6.0;

                                            let title_width = (ui.available_width() - 144.0).max(0.0);
                                            let title_resp = render_server_card_title(ui, &s.config.name, title_width, 24.0, is_selected_tab, &p);

                                            if title_resp.clicked() {
                                                to_switch_tab = Some(key.clone());
                                            }

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.spacing_mut().item_spacing.x = 6.0;

                                                let btn_chevron = egui::Button::new(
                                                    RichText::new(crate::lucide::CHEVRON_DOWN)
                                                        .size(11.0)
                                                        .color(p.text_secondary),
                                                )
                                                .fill(Color32::TRANSPARENT)
                                                .min_size(egui::vec2(22.0, 22.0));

                                                if ui.add(btn_chevron).hover_tip("Expand card").clicked() {
                                                    to_toggle_collapse = Some(key.clone());
                                                }

                                                let (action_icon, action_fg, action_bg) = if is_running {
                                                    (crate::lucide::STOP_FILLED, Color32::WHITE, p.danger)
                                                } else {
                                                    (crate::lucide::PLAY_FILLED, p.accent, p.accent_subtle)
                                                };
                                                let action = egui::Button::new(RichText::new(action_icon).size(11.0).color(action_fg))
                                                    .fill(action_bg)
                                                    .rounding(Rounding::same(8.0))
                                                    .min_size(egui::vec2(24.0, 24.0));
                                                let hint = format!("{} {}", if is_running { "Stop" } else { "Start" }, s.config.name);
                                                let response = ui.add(action).hover_tip(&hint);
                                                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &hint));
                                                if response.clicked() {
                                                    if is_running { to_stop = Some(idx); } else { to_start = Some(idx); }
                                                }

                                                // Status Chip Pill (strictly dot on left, text on right)
                                                Frame::none()
                                                    .fill(chip_bg)
                                                    .rounding(Rounding::same(999.0))
                                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                    .show(ui, |ui| {
                                                        ui.allocate_ui_with_layout(
                                                            egui::vec2(64.0, 14.0),
                                                            egui::Layout::left_to_right(egui::Align::Center),
                                                            |ui| {
                                                                ui.spacing_mut().item_spacing.x = 6.0;
                                                                let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                                                ui.painter().circle_filled(dot_rect.center(), 2.5, chip_dot);
                                                                ui.label(
                                                                    RichText::new(status_label)
                                                                        .size(10.5)
                                                                        .strong()
                                                                        .color(chip_text),
                                                                );
                                                            },
                                                        );
                                                    });
                                            });
                                        });
                                    });
                            } else {
                                // ─── Expanded Card ───────────────────────────────────
                                Frame::none()
                                    .fill(card_bg)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(12.0))
                                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.spacing_mut().item_spacing.y = 3.0;
                                        ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.text_primary);
                                        ui.visuals_mut().widgets.active.bg_stroke = Stroke::new(1.0_f32, p.accent);

                                        // Row 1: Header (Title on left, Chevron Up + Status Chip on right)
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 8.0;

                                            let title_width = (ui.available_width() - 120.0).max(0.0);
                                            let title_resp = render_server_card_title(ui, &s.config.name, title_width, 20.0, is_selected_tab, &p);

                                            if title_resp.clicked() {
                                                to_switch_tab = Some(key.clone());
                                            }

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.spacing_mut().item_spacing.x = 8.0;

                                                let btn_chevron = egui::Button::new(
                                                    RichText::new(crate::lucide::CHEVRON_UP)
                                                        .size(11.0)
                                                        .color(p.text_secondary),
                                                )
                                                .fill(Color32::TRANSPARENT)
                                                .min_size(egui::vec2(22.0, 22.0));

                                                if ui.add(btn_chevron).hover_tip("Minimize card").clicked() {
                                                    to_toggle_collapse = Some(key.clone());
                                                }

                                                // Status Chip Pill (strictly dot on left, text on right)
                                                Frame::none()
                                                    .fill(chip_bg)
                                                    .rounding(Rounding::same(999.0))
                                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                    .show(ui, |ui| {
                                                        ui.allocate_ui_with_layout(
                                                            egui::vec2(64.0, 14.0),
                                                            egui::Layout::left_to_right(egui::Align::Center),
                                                            |ui| {
                                                                ui.spacing_mut().item_spacing.x = 6.0;
                                                                let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                                                ui.painter().circle_filled(dot_rect.center(), 2.5, chip_dot);
                                                                ui.label(
                                                                    RichText::new(status_label)
                                                                        .size(10.5)
                                                                        .strong()
                                                                        .color(chip_text),
                                                                );
                                                            },
                                                        );
                                                    });
                                            });
                                        });

                                        ui.add_space(6.0);

                                        // Row 2: Recessed Command Box
                                        Frame::none()
                                            .fill(p.bg_input)
                                            .rounding(Rounding::same(10.0))
                                            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.spacing_mut().item_spacing.y = 3.0;
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 6.0;
                                                    ui.label(
                                                        RichText::new(">_")
                                                            .strong()
                                                            .size(10.5)
                                                            .monospace()
                                                            .color(p.accent),
                                                    );
                                                    ui.add_space(4.0);
                                                    let command_width = (ui.available_width() - if s.config.port > 0 { 92.0 } else { 30.0 }).max(0.0);
                                                    ui.add_sized([command_width, 18.0], egui::Label::new(
                                                        RichText::new(&s.config.command)
                                                            .size(10.5)
                                                            .monospace()
                                                            .color(p.text_primary),
                                                    ).truncate()).hover_tip(&s.config.command);

                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        let btn_copy = egui::Button::new(
                                                            RichText::new(crate::lucide::COPY)
                                                                .size(10.5)
                                                                .color(p.text_muted),
                                                        )
                                                        .fill(Color32::TRANSPARENT)
                                                        .stroke(Stroke::NONE);

                                                        if ui.add(btn_copy).hover_tip("Copy command").clicked() {
                                                            ui.ctx().copy_text(s.config.command.clone());
                                                        }

                                                        if s.config.port > 0 {
                                                            ui.add_space(6.0);
                                                            ui.label(
                                                                RichText::new(format!(":{}", s.config.port))
                                                                    .size(11.0)
                                                                    .monospace()
                                                                    .color(p.info),
                                                            );
                                                        }
                                                    });
                                                });
                                            });

                                        ui.add_space(4.0);

                                        // Row 3: Telemetry Grid (memory is shown only while running)
                                        let grid_bg = p.bg_subtle;
                                        let grid_rounding = Rounding::same(8.0);
                                        let grid_margin = egui::Margin::symmetric(4.0, 3.0);
                                        let total_w = ui.available_width();
                                        let columns = if is_running { 4.0 } else { 3.0 };
                                        let card_w = (((total_w - (columns - 1.0) * 5.0) / columns) - grid_margin.sum().x).floor().max(0.0);

                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 5.0;

                                            // Card 1: PID
                                            Frame::none()
                                                .fill(grid_bg)
                                                .rounding(grid_rounding)
                                                .inner_margin(grid_margin)
                                                .show(ui, |ui| {
                                                    ui.set_width(card_w);
                                                    ui.vertical(|ui| {
                                                        ui.spacing_mut().item_spacing.y = 2.0;
                                                        ui.horizontal(|ui| {
                                                            ui.spacing_mut().item_spacing.x = 3.0;
                                                            ui.label(RichText::new(crate::lucide::HASH).size(9.5).color(p.text_secondary));
                                                            ui.label(RichText::new("PID").size(9.5).color(p.text_secondary));
                                                        });
                                                        let pid_str = if is_running {
                                                            s.pid.map(|p| format!("{}", p)).unwrap_or_else(|| "-".to_string())
                                                        } else if s.config.port > 0 {
                                                            format!(":{}", s.config.port)
                                                        } else {
                                                            "-".to_string()
                                                        };
                                                        ui.add(egui::Label::new(RichText::new(pid_str).size(10.5).monospace().strong().color(p.text_primary)).truncate());
                                                    });
                                                });

                                            // Card 2: CPU
                                            Frame::none()
                                                .fill(grid_bg)
                                                .rounding(grid_rounding)
                                                .inner_margin(grid_margin)
                                                .show(ui, |ui| {
                                                    ui.set_width(card_w);
                                                    ui.vertical(|ui| {
                                                        ui.spacing_mut().item_spacing.y = 2.0;
                                                        ui.horizontal(|ui| {
                                                            ui.spacing_mut().item_spacing.x = 3.0;
                                                            ui.label(RichText::new(crate::lucide::CPU).size(9.5).color(p.text_secondary));
                                                            ui.label(RichText::new("CPU").size(9.5).color(p.text_secondary));
                                                        });
                                                        let cpu_str = if is_running { format!("{:.1}%", s.cpu_usage) } else { "0.0%".to_string() };
                                                        ui.add(egui::Label::new(RichText::new(cpu_str).size(10.5).monospace().strong().color(p.text_primary)).truncate());
                                                    });
                                                });

                                            // Card 3: Memory (running servers)
                                            if is_running {
                                                Frame::none()
                                                    .fill(grid_bg)
                                                    .rounding(grid_rounding)
                                                    .inner_margin(grid_margin)
                                                    .show(ui, |ui| {
                                                        ui.set_width(card_w);
                                                        ui.vertical(|ui| {
                                                            ui.spacing_mut().item_spacing.y = 2.0;
                                                            ui.horizontal(|ui| {
                                                                ui.spacing_mut().item_spacing.x = 3.0;
                                                                ui.label(RichText::new(crate::lucide::RAM).size(9.5).color(p.text_secondary));
                                                                ui.label(RichText::new("Memory").size(9.5).color(p.text_secondary));
                                                            });
                                                            let mem_str = format!("{:.1} MB", s.memory_mb);
                                                            ui.add(egui::Label::new(RichText::new(mem_str).size(11.0).monospace().strong().color(p.text_primary)).truncate());
                                                        });
                                                    });
                                            }

                                            // Card 4: Uptime
                                            Frame::none()
                                                .fill(grid_bg)
                                                .rounding(grid_rounding)
                                                .inner_margin(grid_margin)
                                                .show(ui, |ui| {
                                                    ui.set_width(card_w);
                                                    ui.vertical(|ui| {
                                                        ui.spacing_mut().item_spacing.y = 2.0;
                                                        ui.horizontal(|ui| {
                                                            ui.spacing_mut().item_spacing.x = 3.0;
                                                            ui.label(RichText::new(crate::lucide::CLOCK).size(9.5).color(p.text_secondary));
                                                            ui.label(RichText::new("Uptime").size(9.5).color(p.text_secondary));
                                                        });
                                                        let uptime_str = if is_running {
                                                            let up = s.uptime_formatted();
                                                            if up.is_empty() { "0s".to_string() } else { up }
                                                        } else {
                                                            "0s".to_string()
                                                        };
                                                        ui.add(egui::Label::new(RichText::new(uptime_str).size(10.5).monospace().strong().color(p.text_primary)).truncate());
                                                    });
                                                });
                                        });

                                        ui.add_space(6.0);

                                        // Row 4: Primary Action Buttons
                                        if is_running {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 8.0;

                                                let btn_stop = egui::Button::new(
                                                    RichText::new(format!("{} Stop", crate::lucide::STOP_FILLED))
                                                        .size(10.5)
                                                        .strong()
                                                        .color(Color32::WHITE),
                                                )
                                                .fill(p.danger)
                                                .rounding(Rounding::same(999.0))
                                                .min_size(egui::vec2(80.0, 24.0));

                                                if ui.add(btn_stop).hover_tip("Stop this server").clicked() {
                                                    to_stop = Some(idx);
                                                }

                                                let btn_restart = egui::Button::new(
                                                    RichText::new(format!("{} Restart", crate::lucide::REFRESH_CW))
                                                        .size(10.5)
                                                        .strong()
                                                        .color(p.accent_text),
                                                )
                                                .fill(p.accent)
                                                .rounding(Rounding::same(999.0))
                                                .min_size(egui::vec2(80.0, 24.0));

                                                if ui.add(btn_restart).hover_tip("Restart this server").clicked() {
                                                    to_restart = Some(idx);
                                                }
                                            });
                                        } else {
                                            let btn_start = egui::Button::new(
                                                RichText::new(format!("{} Start", crate::lucide::PLAY_FILLED))
                                                    .size(10.5)
                                                    .strong()
                                                    .color(p.accent_text),
                                            )
                                            .fill(p.accent)
                                            .rounding(Rounding::same(999.0))
                                            .min_size(egui::vec2(80.0, 24.0));

                                            if ui.add(btn_start).hover_tip("Start this server").clicked() {
                                                to_start = Some(idx);
                                            }
                                        }

                                        ui.add_space(4.0);

                                        // Row 5: Secondary tools with a separate destructive action on the right.
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(ui.available_width(), 20.0),
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.spacing_mut().item_spacing.x = 6.0;

                                                let circ_bg = p.bg_button;
                                                let circ_size = egui::vec2(24.0, 24.0);
                                                let circ_rnd = Rounding::same(999.0);
                                                let tool_icon_col = p.text_secondary;

                                                let btn_console = egui::Button::new(
                                                    RichText::new(crate::lucide::TERMINAL).size(11.0).color(tool_icon_col),
                                                )
                                                .fill(circ_bg)
                                                .rounding(circ_rnd)
                                                .min_size(circ_size);
                                                if ui.add(btn_console).hover_tip("Open in console / terminal window").clicked() {
                                                    s.open_console();
                                                }

                                                let btn_edit = egui::Button::new(
                                                    RichText::new(crate::lucide::PENCIL).size(11.0).color(tool_icon_col),
                                                )
                                                .fill(circ_bg)
                                                .rounding(circ_rnd)
                                                .min_size(circ_size);
                                                if ui.add(btn_edit).hover_tip("Edit server configuration").clicked() {
                                                    to_edit = Some(s.config.clone());
                                                }

                                                let btn_vscode = egui::Button::new(
                                                    RichText::new(crate::lucide::CODE).size(11.0).color(tool_icon_col),
                                                )
                                                .fill(circ_bg)
                                                .rounding(circ_rnd)
                                                .min_size(circ_size);
                                                if ui.add(btn_vscode).hover_tip("Open working directory in VS Code (code .)").clicked() {
                                                    crate::service::open_in_vscode(&s.config.cwd);
                                                }

                                                let btn_folder = egui::Button::new(
                                                    RichText::new(crate::lucide::FOLDER).size(11.0).color(tool_icon_col),
                                                )
                                                .fill(circ_bg)
                                                .rounding(circ_rnd)
                                                .min_size(circ_size);
                                                if ui.add(btn_folder).hover_tip("Open working directory in file manager").clicked() {
                                                    crate::service::open_folder(&s.config.cwd);
                                                }

                                                if (is_running && s.config.port > 0) || s.config.url.is_some() {
                                                    let target_url = if let Some(u) = &s.config.url {
                                                        u.clone()
                                                    } else {
                                                        format!("http://localhost:{}", s.config.port)
                                                    };
                                                    let btn_browser = egui::Button::new(
                                                        RichText::new(crate::lucide::EXTERNAL_LINK).size(11.0).strong().color(p.accent),
                                                    )
                                                    .fill(circ_bg)
                                                    .rounding(circ_rnd)
                                                    .min_size(circ_size);
                                                    if ui.add(btn_browser).hover_tip(format!("Open {} in browser", target_url)).clicked() {
                                                        ui.ctx().open_url(egui::OpenUrl::new_tab(target_url));
                                                    }
                                                }

                                                // Dedicated Delete action pinned to the right.
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    let btn_del = egui::Button::new(
                                                        RichText::new(crate::lucide::TRASH).size(10.5).color(p.danger),
                                                    )
                                                    .fill(p.danger_subtle)
                                                    .stroke(Stroke::NONE)
                                                    .rounding(circ_rnd)
                                                    .min_size(circ_size);

                                                    if ui.add(btn_del).hover_tip("Delete server").clicked() {
                                                        to_delete = Some(key.clone());
                                                    }
                                                });
                                            },
                                        );

                                        ui.add_space(4.0);

                                        // Subtle horizontal divider line
                                        let line_color = accent_with_alpha(p.text_muted, 40);
                                        let (div_rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
                                        ui.painter().line_segment(
                                            [div_rect.left_top(), div_rect.right_top()],
                                            Stroke::new(1.0_f32, line_color),
                                        );

                                        ui.add_space(6.0);

                                        // Row 6: Modern Toggle Switches (Guaranteed full width rows)
                                        let toggle_label_color = p.text_secondary;

                                        // Row 1: autostart
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(ui.available_width(), 20.0),
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.label(RichText::new("autostart").size(11.0).color(toggle_label_color));
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    if render_modern_toggle_switch(ui, &mut s.config.autostart, &p, "Start automatically") {
                                                        to_save_config = true;
                                                    }
                                                });
                                            },
                                        );

                                        ui.add_space(4.0);

                                        // Row 2: run in external terminal
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(ui.available_width(), 20.0),
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.label(RichText::new("run in external terminal").size(11.0).color(toggle_label_color));
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    if render_modern_toggle_switch(ui, &mut s.config.own_console, &p, "Run in external terminal") {
                                                        to_save_config = true;
                                                    }
                                                });
                                            },
                                        );

                                        ui.add_space(4.0);

                                        // Row 3: auto-restart
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(ui.available_width(), 20.0),
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.label(RichText::new("auto-restart").size(11.0).color(toggle_label_color));
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    if render_modern_toggle_switch(ui, &mut s.config.auto_restart, &p, "Restart automatically") {
                                                        to_save_config = true;
                                                    }
                                                });
                                            },
                                        );
                                    });
                            }
                            ui.add_space(8.0);
                        }
                        ui.add_space(20.0);
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
                    if let Some(k) = to_toggle_collapse {
                        if self.expanded_server_cards.contains(&k) {
                            self.expanded_server_cards.remove(&k);
                        } else {
                            self.expanded_server_cards.insert(k);
                        }
                    }
                });
        });
    }
}
