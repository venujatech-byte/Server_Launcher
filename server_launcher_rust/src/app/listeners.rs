use super::*;

impl LauncherApp {
    pub(in crate::app) fn render_inline_terminal(
        &mut self,
        ui: &mut egui::Ui,
        port: u16,
        _name: &str,
        pid: Option<u32>,
        cwd: &str,
    ) {
        let p = self.palette();
        Frame::none()
            .fill(p.bg_card)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(12.0))
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.spacing_mut().button_padding = egui::vec2(6.0, 3.0);
                    let pid_str = pid.map(|p| p.to_string()).unwrap_or_else(|| "unavailable".to_string());
                    let metadata = format!("Port :{} · PID {}", port, pid_str);
                    let mut heading = egui::text::LayoutJob::default();
                    heading.append(&format!("{} ", crate::lucide::TERMINAL), 0.0, egui::TextFormat {
                        font_id: egui::FontId::proportional(11.5), color: p.accent,
                        ..Default::default()
                    });
                    heading.append("Command Console", 0.0, egui::TextFormat {
                        font_id: egui::FontId::proportional(11.5), color: p.text_primary,
                        ..Default::default()
                    });
                    heading.append(&format!("  {}", metadata), 0.0, egui::TextFormat {
                        font_id: egui::FontId::proportional(10.5), color: p.text_muted,
                        ..Default::default()
                    });
                    let heading_width = (ui.available_width() - 226.0).max(0.0);
                    ui.allocate_ui_with_layout(egui::vec2(heading_width, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_width(heading_width);
                        ui.add(egui::Label::new(heading).truncate().halign(egui::Align::Min))
                            .on_hover_text(format!("Command Console · {}", metadata));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Buttons use weak fills; set each state locally to follow the active palette.
                        let visuals = ui.visuals_mut();
                        visuals.override_text_color = None;
                        visuals.widgets.inactive.weak_bg_fill = p.bg_subtle;
                        visuals.widgets.hovered.weak_bg_fill = p.bg_button_hover;
                        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.accent);
                        visuals.widgets.active.weak_bg_fill = p.accent_subtle;
                        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.accent);
                        let button = |icon: &str, label: &str, width: f32| {
                            egui::Button::new(RichText::new(format!("{} {}", icon, label)).size(10.5))
                                .rounding(Rounding::same(6.0)).min_size(egui::vec2(width, 24.0))
                        };
                        if ui.add(button(crate::lucide::X, "Close", 58.0)).on_hover_text("Close command console").clicked() {
                            self.open_inline_terminals.remove(&port);
                        }
                        if ui.add(button(crate::lucide::REFRESH_CW, "Refresh Logs", 92.0)).on_hover_text("Load available process logs; commands run separately from the server process").clicked() {
                            self.refresh_inline_logs(port, pid, cwd);
                        }
                        if ui.add(button(crate::lucide::TRASH, "Clear", 58.0)).on_hover_text("Clear terminal output").clicked() {
                            self.clear_inline_logs(port);
                        }
                    });
                });
                let directory = if cwd.is_empty() { "launcher working directory" } else { cwd };
                ui.add(egui::Label::new(RichText::new(format!("Commands in: {}", directory))
                    .size(10.5).color(p.text_muted)).truncate())
                    .on_hover_text(format!("Working directory: {}\nEach command starts a separate shell; it does not send input to the server process.\nRefresh Logs loads available process logs on demand.", directory));
                ui.add_space(4.0);

                let logs = self.inline_terminal_sessions.get(&port).and_then(|session| session.logs.lock().ok());
                render_inline_console_output(ui, port, logs.as_deref(), &p);
                drop(logs);

                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(RichText::new(">").size(11.5).monospace().strong().color(p.accent));
                    let input_width = (ui.available_width() - 32.0 - 58.0 - 2.0 * ui.spacing().item_spacing.x).max(0.0);
                    let input_ref = self.inline_terminal_inputs.entry(port).or_default();
                    let resp = ui.add_sized([input_width, 24.0],
                        egui::TextEdit::singleline(input_ref)
                            .hint_text("Enter command…")
                            .desired_width(input_width)
                            .font(egui::FontId::monospace(11.5))
                            .margin(egui::vec2(8.0, 4.0)),
                    );
                    ui.add_sized([32.0, 24.0], egui::Label::new(RichText::new("Enter").size(10.0).color(p.text_muted)))
                        .on_hover_text("Press Enter to run the command");
                    let run = egui::Button::new(RichText::new(format!("{} Run", crate::lucide::PLAY_FILLED))
                        .size(10.5).strong().color(p.accent_text))
                        .fill(p.accent).rounding(Rounding::same(6.0)).min_size(egui::vec2(58.0, 24.0));
                    let run_clicked = ui.add_enabled(!input_ref.trim().is_empty(), run)
                        .on_hover_text("Run command · Enter").clicked();
                    let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (enter_pressed || run_clicked) && !input_ref.trim().is_empty() {
                        let cmd = input_ref.trim().to_string();
                        input_ref.clear();
                        self.run_inline_command(port, &cmd, cwd);
                        resp.request_focus();
                    }
                });
            });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Right Panel Tab 0: "Server Overview" (Fills available height)
    // ─────────────────────────────────────────────────────────────────────────
    pub(in crate::app) fn render_overview_content(&mut self, ui: &mut Ui) {
        let p = self.palette();
        // Separate status from controls so the search field never competes with the heading.
        let is_scanning = self.scanner.is_scanning.lock().map(|value| *value).unwrap_or(false);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Servers on this PC").size(16.0).strong().color(p.text_primary));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if is_scanning {
                    ui.label(RichText::new("Scanning…").size(11.5).color(p.text_muted));
                    ui.spinner();
                } else {
                    ui.label(RichText::new(format!("Last scan {}", self.scanner.get_last_scan_time()))
                        .size(11.5).color(p.text_muted));
                }
            });
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let search_width = (ui.available_width() - 96.0 - ui.spacing().item_spacing.x).max(60.0);
            Frame::none().fill(p.bg_button).stroke(Stroke::new(1.0_f32, p.border))
                .rounding(Rounding::same(6.0)).inner_margin(egui::Margin::symmetric(10.0, 4.0))
                .show(ui, |ui| {
                    ui.set_width((search_width - 20.0).max(40.0));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::lucide::SEARCH).size(12.0).color(p.text_muted));
                        let clear_width = if self.overview_search_query.is_empty() { 0.0 } else { 24.0 + ui.spacing().item_spacing.x };
                        let input_width = (ui.available_width() - clear_width).max(24.0);
                        let response = ui.add_sized([input_width, 20.0],
                            egui::TextEdit::singleline(&mut self.overview_search_query)
                                .font(egui::FontId::proportional(13.0))
                                .hint_text(RichText::new("Search servers, ports or commands").color(p.text_muted))
                                .desired_width(input_width).frame(false))
                            .on_hover_text("Search by server name, port or command · Ctrl+F");
                        if self.focus_overview_search {
                            response.request_focus();
                            self.focus_overview_search = false;
                            self.refresh_server_list();
                        }
                        if response.changed() || (response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter))) {
                            self.refresh_server_list();
                        }
                        if !self.overview_search_query.is_empty() && ui.add_sized([24.0, 20.0],
                            egui::Button::new(RichText::new(crate::lucide::X).size(12.0)).frame(false))
                            .on_hover_text("Clear search").clicked() {
                            self.overview_search_query.clear();
                            self.refresh_server_list();
                        }
                    });
                });
            let refresh_label = if is_scanning { "Scanning…" } else { "Refresh" };
            if ui.add_enabled(!is_scanning, egui::Button::new(
                RichText::new(format!("{} {refresh_label}", crate::lucide::REFRESH_CW)).size(12.0).color(p.accent))
                .fill(p.bg_button).stroke(Stroke::NONE).rounding(Rounding::same(6.0))
                .min_size(egui::vec2(96.0, 28.0))).on_hover_text("Scan for running servers").clicked() {
                self.refresh_server_list();
            }
        });

        ui.add_space(6.0);

        let q = self.overview_search_query.trim().to_lowercase();

        ScrollArea::vertical()
            .id_salt("overview_scroll_body")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Other processes listening on this PC
                let listeners = self.scanner.get_listeners();
                let managed_pids: BTreeSet<u32> = self.services.iter()
                    .filter(|service| service.is_active()).filter_map(|service| service.pid).collect();
                let managed_ports: BTreeSet<u16> = self
                    .services
                    .iter()
                    .filter(|service| service.is_active())
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
                    .filter(|listener| !listener_is_managed(listener, &managed_pids, &self.sys))
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

                ui.horizontal_wrapped(|ui| {
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
                        let metrics = if self.show_other_process_metrics {
                            listener.pid.map(|pid| {
                                let (cpu, mem) = self.other_metrics.get(&pid).copied().unwrap_or((0.0, 0.0));
                                (cpu.to_bits(), mem.to_bits())
                            })
                        } else { None };
                        let row_layout = ProcessRowLayout {
                            width: ui.available_width().to_bits(), scale: ui.ctx().pixels_per_point().to_bits(),
                            expanded: self.expanded_listeners.contains(&listener.port),
                            console: self.open_inline_terminals.contains(&listener.port), metrics,
                        };
                        let height_id = ui.make_persistent_id(("listener_measured_height", &listener.proto, listener.port, listener.pid));
                        if skip_measured_process_row(ui, height_id, row_layout) {
                            ui.add_space(6.0);
                            continue;
                        }
                        let (cmd_opt, cwd_opt) = listener.pid
                            .map(|p| self.get_proc_cmd_and_cwd(p))
                            .unwrap_or((None, None));
                        let cmd_str = cmd_opt.unwrap_or_default();
                        let cwd_str = cwd_opt.unwrap_or_default();

                        let row = ui.push_id(("external_process_card", &listener.proto, listener.port, listener.pid), |ui| Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    let is_expanded = self.expanded_listeners.contains(&listener.port);
                                    ui.horizontal_top(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        let actions_width = 76.0 + 60.0 + 2.0 * ui.spacing().item_spacing.x;
                                        let identity_width = (ui.available_width() - actions_width).max(0.0);
                                        ui.allocate_ui_with_layout(egui::vec2(identity_width, 24.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                            ui.set_width(identity_width);
                                            ui.spacing_mut().item_spacing.y = 4.0;
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 6.0;
                                                draw_status_dot(ui, p.success);
                                                let icon = if is_expanded { crate::lucide::CHEVRON_DOWN } else { crate::lucide::CHEVRON_RIGHT };
                                                let toggle = egui::Button::new(RichText::new(icon).size(11.0).color(p.text_secondary))
                                                    .fill(p.bg_button).rounding(Rounding::same(6.0)).min_size(egui::vec2(22.0, 24.0));
                                                if ui.add(toggle).on_hover_text(if is_expanded { "Collapse process details" } else { "Expand process details" }).clicked() {
                                                    if is_expanded { self.expanded_listeners.remove(&listener.port); }
                                                    else { self.expanded_listeners.insert(listener.port); }
                                                }
                                                ui.add(egui::Label::new(RichText::new(&listener.name).size(12.5).strong().color(p.text_primary)).truncate())
                                                    .on_hover_text(&listener.name);
                                            });
                                            ui.horizontal_wrapped(|ui| {
                                                ui.spacing_mut().item_spacing.x = 8.0;
                                                ui.label(RichText::new(&listener.proto).size(10.5).color(p.text_muted));
                                                if let Some(pid) = listener.pid {
                                                    ui.label(RichText::new(format!("PID {}", pid)).size(10.5).monospace().color(p.text_muted));
                                                }
                                                ui.label(RichText::new(format!(":{}", listener.port)).size(10.5).monospace().color(p.info));
                                                ui.label(RichText::new("Running").size(10.5).color(p.success));
                                                if self.show_other_process_metrics {
                                                    if let Some(pid) = listener.pid {
                                                        let (cpu, mem) = self.other_metrics.get(&pid).copied().unwrap_or((0.0, 0.0));
                                                        ui.label(RichText::new(format!("{} {:.1}%", crate::lucide::CPU, cpu)).size(10.5).monospace().color(p.info));
                                                        ui.label(RichText::new(format!("{} {:.1} MB", crate::lucide::RAM, mem)).size(10.5).monospace().color(p.text_secondary));
                                                    }
                                                }
                                            });
                                        });
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                            let stop = egui::Button::new(RichText::new(format!("{} Stop", crate::lucide::STOP_FILLED)).size(10.5).color(Color32::WHITE))
                                                .fill(p.danger).rounding(Rounding::same(6.0)).min_size(egui::vec2(60.0, 24.0));
                                            if ui.add(stop).on_hover_text("Stop this process").clicked() {
                                                to_kill_pid = Some((listener.pid, listener.port, listener.name.clone(), cmd_str.clone(), cwd_str.clone()));
                                            }
                                            let restart = egui::Button::new(RichText::new(format!("{} Restart", crate::lucide::REFRESH_CW)).size(10.5).color(p.accent))
                                                .fill(p.accent_subtle).rounding(Rounding::same(6.0)).min_size(egui::vec2(76.0, 24.0));
                                            if ui.add(restart).on_hover_text("Restart this process").clicked() {
                                                to_restart_pid = Some((listener.pid, listener.port, cmd_str.clone(), cwd_str.clone()));
                                            }
                                        });
                                    });

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
                                                            process_detail_value(ui, &cwd_str, p.text_primary);
                                                        });
                                                    }
                                                    if !cmd_str.is_empty() {
                                                        if !cwd_str.is_empty() {
                                                            ui.add_space(4.0);
                                                        }
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new(format!("{} Command:", crate::lucide::SETTINGS)).size(11.0).strong().color(p.accent));
                                                            process_detail_value(ui, &cmd_str, p.text_secondary);
                                                        });
                                                    }
                                                    if cwd_str.is_empty() && cmd_str.is_empty() {
                                                        ui.label(
                                                            RichText::new("Directory and command details unavailable.")
                                                                .size(10.5)
                                                                .color(p.text_secondary),
                                                        );
                                                    }

                                                    ui.add_space(8.0);
                                                    ui.horizontal_wrapped(|ui| {
                                                        if !self.open_inline_terminals.contains(&listener.port) {
                                                            let btn_inline = egui::Button::new(
                                                                RichText::new(format!("{} Open Console", crate::lucide::TERMINAL))
                                                                    .size(11.0).strong().color(p.accent),
                                                            )
                                                            .fill(p.bg_button)
                                                            .stroke(Stroke::NONE)
                                                            .rounding(Rounding::same(999.0));
                                                            if ui.add(btn_inline).on_hover_text("Open a shell-command console and load available process logs").clicked() {
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
                            }));

                        ui.ctx().data_mut(|data| data.insert_temp(height_id, MeasuredProcessRow {
                            layout: row_layout, height: row.response.rect.height(),
                        }));
                        ui.add_space(6.0);
                    }

                    // Render any stopped external processes
                    for (port, name, cmd, cwd) in &visible_stopped {
                        let row_layout = ProcessRowLayout {
                            width: ui.available_width().to_bits(), scale: ui.ctx().pixels_per_point().to_bits(),
                            expanded: self.expanded_listeners.contains(port),
                            console: self.open_inline_terminals.contains(port), metrics: None,
                        };
                        let height_id = ui.make_persistent_id(("stopped_measured_height", *port));
                        if skip_measured_process_row(ui, height_id, row_layout) {
                            ui.add_space(6.0);
                            continue;
                        }
                        let row = ui.push_id(("stopped_process_card", *port), |ui| Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    let is_expanded = self.expanded_listeners.contains(port);
                                    ui.horizontal_top(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        let actions_width = 76.0 + 24.0 + 2.0 * ui.spacing().item_spacing.x;
                                        let identity_width = (ui.available_width() - actions_width).max(0.0);
                                        ui.allocate_ui_with_layout(egui::vec2(identity_width, 24.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                            ui.set_width(identity_width);
                                            ui.spacing_mut().item_spacing.y = 4.0;
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 6.0;
                                                draw_status_dot(ui, p.text_muted);
                                                let icon = if is_expanded { crate::lucide::CHEVRON_DOWN } else { crate::lucide::CHEVRON_RIGHT };
                                                let toggle = egui::Button::new(RichText::new(icon).size(11.0).color(p.text_secondary))
                                                    .fill(p.bg_button).rounding(Rounding::same(6.0)).min_size(egui::vec2(22.0, 24.0));
                                                if ui.add(toggle).on_hover_text(if is_expanded { "Collapse process details" } else { "Expand process details" }).clicked() {
                                                    if is_expanded { self.expanded_listeners.remove(port); }
                                                    else { self.expanded_listeners.insert(*port); }
                                                }
                                                ui.add(egui::Label::new(RichText::new(name).size(12.5).strong().color(p.text_primary)).truncate()).on_hover_text(name);
                                            });
                                            ui.horizontal_wrapped(|ui| {
                                                ui.spacing_mut().item_spacing.x = 8.0;
                                                ui.label(RichText::new(format!(":{}", port)).size(10.5).monospace().color(p.info));
                                                ui.label(RichText::new("Stopped").size(10.5).color(p.text_muted));
                                            });
                                        });
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                            let remove = egui::Button::new(RichText::new(crate::lucide::X).size(11.0).color(p.text_secondary))
                                                .fill(p.bg_button).rounding(Rounding::same(6.0)).min_size(egui::vec2(24.0, 24.0));
                                            if ui.add(remove).on_hover_text("Remove stopped process from this list").clicked() { to_remove_stopped = Some(*port); }
                                            let start = egui::Button::new(RichText::new(format!("{} Start", crate::lucide::PLAY_FILLED)).size(10.5).color(p.accent_text))
                                                .fill(p.accent).rounding(Rounding::same(6.0)).min_size(egui::vec2(76.0, 24.0));
                                            if ui.add(start).on_hover_text("Start this process").clicked() { to_start_stopped = Some((*port, cmd.clone(), cwd.clone())); }
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
                                                            process_detail_value(ui, cwd, p.text_primary);
                                                        });
                                                    }
                                                    if !cmd.is_empty() {
                                                        if !cwd.is_empty() {
                                                            ui.add_space(4.0);
                                                        }
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new(format!("{} Command:", crate::lucide::SETTINGS)).size(11.0).strong().color(p.accent));
                                                            process_detail_value(ui, cmd, p.text_secondary);
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
                                                    ui.horizontal_wrapped(|ui| {
                                                        if !self.open_inline_terminals.contains(port) {
                                                            let btn_inline = egui::Button::new(
                                                                RichText::new(format!("{} Open Console", crate::lucide::TERMINAL))
                                                                    .size(11.0).strong().color(p.accent),
                                                            )
                                                            .fill(p.bg_button)
                                                            .stroke(Stroke::NONE)
                                                            .rounding(Rounding::same(999.0));
                                                            if ui.add(btn_inline).on_hover_text("Open a shell-command console in this working directory").clicked() {
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
                            }));

                        ui.ctx().data_mut(|data| data.insert_temp(height_id, MeasuredProcessRow {
                            layout: row_layout, height: row.response.rect.height(),
                        }));
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
}
