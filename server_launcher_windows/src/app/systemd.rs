use super::*;

impl LauncherApp {
    // ═════════════════════════════════════════════════════════════════════════
    // SYSTEMD TAB: Services List, Controller, and Journalctl Viewer
    // ═════════════════════════════════════════════════════════════════════════
    pub(in crate::app) fn render_systemd_left_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.palette();
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
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.set_min_height(26.0);
                ui.label(RichText::new(if cfg!(windows) && host_id.is_none() { "Windows services" } else { "Systemd services" }).size(12.0).strong().color(p.text_primary));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_new = egui::Button::new(
                        RichText::new(format!("{} New service", crate::lucide::PLUS)).size(11.0).color(p.accent_text),
                    )
                    .fill(p.accent)
                    .stroke(Stroke::NONE).rounding(Rounding::same(8.0)).min_size(egui::vec2(0.0, 24.0));
                    if ui.add_enabled(!cfg!(windows) || host_id.is_some(), btn_new).clicked() {
                        self.systemd_modal.open();
                        self.systemd_modal.def.is_user = user_mode;
                    }
                });
            });

            ui.add_space(6.0);

            // Scope & Refresh Row
            ui.horizontal(|ui| {
                ui.set_min_height(26.0);
                let user_btn = egui::Button::new(
                    RichText::new("User (--user)")
                        .size(10.5)
                        .color(if user_mode { p.accent } else { p.text_secondary }),
                )
                .fill(if user_mode { p.accent_subtle } else { p.bg_button })
                .stroke(Stroke::NONE).rounding(Rounding::same(8.0)).min_size(egui::vec2(0.0, 24.0));
                if ui.add_enabled(!cfg!(windows) || host_id.is_some(), user_btn).clicked() {
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
                        .color(if !user_mode { p.accent } else { p.text_secondary }),
                )
                .fill(if !user_mode { p.accent_subtle } else { p.bg_button })
                .stroke(Stroke::NONE).rounding(Rounding::same(8.0)).min_size(egui::vec2(0.0, 24.0));
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
                    if ui.button(RichText::new(format!("{} Sudo [{}]", crate::lucide::KEY, crate::lucide::X)).size(10.0).color(p.success))
                        .hover_tip("Sudo password cached for this session. Click to clear/forget.")
                        .clicked()
                    {
                        self.local_sudo_password.clear();
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_ref = egui::Button::new(RichText::new(crate::lucide::REFRESH_CW).size(11.0).color(p.text_secondary))
                        .fill(p.bg_button).stroke(Stroke::NONE).rounding(Rounding::same(6.0));
                    if ui.add_sized([24.0, 24.0], btn_ref).hover_tip("Refresh services list").clicked() {
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

            // A visible search surface with muted hint text.
            Frame::none().fill(p.bg_button).stroke(Stroke::new(1.0_f32, p.border))
                .rounding(Rounding::same(8.0)).inner_margin(egui::Margin::symmetric(8.0, 4.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::lucide::SEARCH).size(11.0).color(p.text_muted));
                        ui.add(egui::TextEdit::singleline(&mut self.service_search_query)
                            .font(egui::FontId::proportional(11.5)).text_color(p.text_secondary)
                            .hint_text(RichText::new("Filter services…").color(p.text_muted))
                            .frame(false).margin(egui::Vec2::ZERO).clip_text(true).desired_width(ui.available_width()));
                    });
                });

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
                        ui.label(RichText::new("Loading services...").size(12.0).color(p.text_muted));
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
                            ui.label(RichText::new("No services found").size(12.0).color(p.text_muted));
                            ui.add_space(4.0);
                            ui.label(RichText::new(format!("Click '{}' to refresh or switch scope (User / System).", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_muted));
                        }
                    } else {
                        ui.label(RichText::new("No services found").size(12.0).color(p.text_muted));
                        ui.add_space(4.0);
                        ui.label(RichText::new(format!("Click '{}' to scan or switch scope.", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_muted));
                    }
                });
                return;
            }

            ui.spacing_mut().item_spacing.y = 8.0;
            ScrollArea::vertical()
                .id_salt(("systemd_services_scroll", host_id, user_mode))
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

                        let card_resp = ui.push_id(("service_card", host_id, user_mode, &s.name), |ui| Frame::none()
                            .fill(if is_selected { p.bg_card_active } else { p.bg_card })
                            .stroke(Stroke::NONE).rounding(Rounding::same(10.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.set_max_width(ui.available_width());
                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                                ui.spacing_mut().interact_size.y = 18.0;
                                ui.set_min_height(38.0);
                                ui.horizontal(|ui| {
                                    ui.set_min_height(20.0);
                                    let colour = if is_active { p.success } else if is_failed { p.danger } else { p.text_muted };
                                    let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                                    ui.painter().circle_filled(dot.center(), 3.0, colour);
                                    let name_width = (ui.available_width() - 74.0).max(0.0);
                                    ui.add_sized([name_width, 20.0], egui::Label::new(
                                        RichText::new(&s.name).size(11.5).strong().color(if is_selected { p.accent } else { p.text_primary }))
                                        .truncate().halign(egui::Align::Min)).hover_tip(&s.name);
                                    ui.add_sized([68.0, 20.0], egui::Label::new(
                                        RichText::new(&s.sub_state).size(10.0).color(colour))
                                        .truncate().halign(egui::Align::Max)).hover_tip(&s.sub_state);
                                });
                                let description = if s.description.is_empty() || s.description == s.name {
                                    format!("{} service · {}", if user_mode { "User" } else { "System" }, s.load_state)
                                } else { s.description.clone() };
                                ui.add_sized([ui.available_width(), 14.0], egui::Label::new(
                                    RichText::new(&description).size(10.5).color(p.text_muted))
                                    .truncate().halign(egui::Align::Min)).hover_tip(&description);
                            })).inner;

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

                    }

                    if let Some(unit) = selected {
                        self.select_systemd_unit(&unit, user_mode, host_opt.as_ref());
                    }
                });
        });
    }

    pub(in crate::app) fn render_systemd_right_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.palette();
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
            self.local_services_cache.lock().unwrap().get(&user_mode).cloned().unwrap_or_default()
        };

        let selected_unit = self.selected_service.clone();
        let detail_key = selected_unit.as_ref().map(|unit| format!("{}:{}:{}", host_id.unwrap_or("local"), unit, user_mode));
        if let (Some(unit), Some(key)) = (&selected_unit, &detail_key) {
            if self.service_details_cache.get(key).is_none_or(|(_, time)| time.elapsed() >= Duration::from_secs(5)) {
                self.queue_systemd_action(PendingSudoAction::InspectUnit { unit: unit.clone(), user_mode }, host_opt.as_ref(), None);
            }
        }
        let service_opt = detail_key.as_ref().and_then(|key| self.service_details_cache.get(key).map(|(service, _)| service.clone()))
            .or_else(|| selected_unit.as_ref().and_then(|unit| services_list.iter().find(|service| &service.name == unit).cloned()));

        match service_opt {
            Some(s) => {
                let is_active = s.is_active();
                let is_failed = s.is_failed();

                let control_key = PendingSudoAction::ServiceAction { unit: s.name.clone(), action: "start", user_mode }.key(host_id);
                let is_action_in_flight = self.systemd_actions_in_flight.lock().unwrap().contains(&control_key);
                let journal_key = PendingSudoAction::ReadJournal { unit: s.name.clone(), user_mode }.key(host_id);
                let is_journal_in_flight = self.systemd_actions_in_flight.lock().unwrap().contains(&journal_key);
                let local_j_key = format!("{}:{}", s.name, if user_mode { "user" } else { "system" });
                let j_key = host_id.map(|host| format!("{}:{}:{}", host, s.name, if user_mode { "user" } else { "system" }));

                // Auto-fetch journal logs on first selection if not yet loaded
                if let (Some(host), Some(ref jk)) = (&host_opt, &j_key) {
                    let has_logs = self.remote_journal_logs.lock().map(|m| m.contains_key(jk)).unwrap_or(false);
                    if !has_logs && !is_journal_in_flight {
                        self.trigger_remote_journal_fetch(host, &s.name, user_mode);
                    }
                } else if host_id.is_none() {
                    let has_logs = self.local_journal_logs.lock().map(|m| m.contains_key(&local_j_key)).unwrap_or(false);
                    if !has_logs && !is_journal_in_flight {
                        self.trigger_local_journal_fetch(&s.name, user_mode);
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
                                ui.label(RichText::new(crate::lucide::SYSTEMD).size(14.0).color(p.accent));
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        let name_width = (ui.available_width() - 128.0).max(0.0);
                                        ui.add_sized([name_width, 20.0], egui::Label::new(RichText::new(&s.name).size(13.0).strong().color(p.text_primary))
                                            .truncate().halign(egui::Align::Min)).hover_tip(&s.name);
                                        let status_tag = if is_active {
                                            RichText::new(format!("{} Active (Running)", crate::lucide::CHECK)).size(11.0).strong().color(p.success)
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
                                    ui.label(RichText::new("Executing action…").size(11.0).color(p.accent));
                                } else {
                                    if is_active {
                                        let btn_stop = egui::Button::new(RichText::new(format!("{} Stop", crate::lucide::STOP_FILLED)).size(11.0).color(Color32::WHITE))
                                            .fill(p.danger)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(999.0))
                                            .min_size(egui::vec2(76.0, 26.0));
                                        if ui.add(btn_stop).clicked() {
                                            action_to_run = Some("stop");
                                        }
                                    } else {
                                        let btn_start = egui::Button::new(RichText::new(format!("{} Start", crate::lucide::PLAY_FILLED)).size(11.0).color(p.accent_text))
                                            .fill(p.accent)
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

                                    if is_enabled == Some(true) {
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
                                        if ui.add_enabled(is_enabled.is_some(), btn_enable).hover_tip("Enable at startup; available after querying the unit state").clicked() {
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

                            if let Some(action) = action_to_run {
                                self.queue_systemd_action(PendingSudoAction::ServiceAction { unit: s.name.clone(), action, user_mode }, host_opt.as_ref(), None);
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
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let tabs = [
                            (SystemdTab::Status, format!("{} Status", crate::lucide::ACTIVITY)),
                            (SystemdTab::Files, format!("{} Files & Drop-ins", crate::lucide::FILE_CODE)),
                            (SystemdTab::Dependencies, format!("{} Dependencies", crate::lucide::LAYERS)),
                        ];
                        for (tab, label) in tabs {
                            if cfg!(windows) && host_id.is_none() && tab != SystemdTab::Status { continue; }
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
                                ui.set_min_height(24.0);
                                ui.spacing_mut().interact_size.y = 24.0;
                                ui.spacing_mut().button_padding = egui::vec2(6.0, 3.0);
                                ui.visuals_mut().widgets.inactive.weak_bg_fill = p.bg_button;
                                ui.visuals_mut().widgets.inactive.rounding = Rounding::same(6.0);
                                ui.visuals_mut().widgets.hovered.rounding = Rounding::same(6.0);
                                ui.visuals_mut().widgets.active.rounding = Rounding::same(6.0);
                                let title_width = (ui.available_width() - 232.0 - if is_journal_in_flight { 24.0 } else { 0.0 }).max(0.0);
                                ui.add_sized([title_width, 24.0], egui::Label::new(RichText::new(if cfg!(windows) && host_id.is_none() { "Service events" } else { "Service logs" }).size(11.5).strong().color(p.text_muted))
                                    .truncate().halign(egui::Align::Min));
                                if is_journal_in_flight {
                                    ui.add_space(4.0);
                                    ui.spinner();
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let btn_ref_log = egui::Button::new(RichText::new(format!("{} Refresh Logs", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_secondary))
                                        .fill(p.bg_button).stroke(Stroke::NONE).rounding(Rounding::same(6.0));
                                    if ui.add_sized([104.0, 24.0], btn_ref_log).clicked() {
                                        if let Some(host) = &host_opt {
                                            self.trigger_remote_journal_fetch(host, &s.name, user_mode);
                                        } else {
                                            self.trigger_local_journal_fetch(&s.name, user_mode);
                                        }
                                        self.last_journal_refresh = Instant::now();
                                    }

                                    // Refresh Interval Dropdown
                                    let prev_interval = self.journal_refresh_interval;
                                    let interval_label = match self.journal_refresh_interval {
                                        0 => format!("{} Auto: Off", crate::lucide::CLOCK),
                                        secs => format!("{} Auto: {}s", crate::lucide::CLOCK, secs),
                                    };
                                    ui.allocate_ui_with_layout(egui::vec2(108.0, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        egui::ComboBox::from_id_salt("journal_refresh_interval_combo")
                                            .selected_text(RichText::new(interval_label).size(11.0).color(p.text_secondary))
                                            .width(108.0).height(216.0).wrap_mode(egui::TextWrapMode::Truncate)
                                            .show_ui(ui, |ui| {
                                                ui.spacing_mut().interact_size.y = 24.0;
                                                ui.spacing_mut().item_spacing.y = 2.0;
                                                ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                                                ui.visuals_mut().selection.bg_fill = p.accent_subtle;
                                                ui.visuals_mut().selection.stroke = Stroke::new(1.0_f32, p.accent);
                                                ui.visuals_mut().widgets.hovered.weak_bg_fill = p.bg_button_hover;
                                                for (seconds, label) in [(0, "Off (Manual)"), (1, "Every 1s"), (2, "Every 2s"), (5, "Every 5s"), (10, "Every 10s"), (30, "Every 30s"), (60, "Every 60s")] {
                                                    let selected = self.journal_refresh_interval == seconds;
                                                    ui.selectable_value(&mut self.journal_refresh_interval, seconds,
                                                        RichText::new(label).size(11.5).color(if selected { p.accent } else { p.text_secondary }));
                                                }
                                            });
                                    });
                                    if self.journal_refresh_interval != prev_interval {
                                        self.last_journal_refresh = Instant::now();
                                        self.persist_config();
                                    }
                                });
                            });

                            // Auto-refresh timer check
                            let journal_auth_error = if let Some(key) = &j_key {
                                self.remote_journal_logs.lock().unwrap().get(key).is_some_and(|logs| crate::systemd::needs_sudo_password(logs))
                            } else {
                                self.local_journal_logs.lock().unwrap().get(&local_j_key).is_some_and(|logs| crate::systemd::needs_sudo_password(logs))
                            };
                            if self.journal_refresh_interval > 0 && !journal_auth_error {
                                let next_refresh = Duration::from_secs(self.journal_refresh_interval as u64)
                                    .saturating_sub(self.last_journal_refresh.elapsed());
                                ui.ctx().request_repaint_after(next_refresh.max(Duration::from_millis(1000)));
                                if self.last_journal_refresh.elapsed() >= Duration::from_secs(self.journal_refresh_interval as u64) {
                                    self.last_journal_refresh = Instant::now();
                                    if let Some(host) = &host_opt {
                                        self.trigger_remote_journal_fetch(host, &s.name, user_mode);
                                    } else {
                                        self.trigger_local_journal_fetch(&s.name, user_mode);
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

                                    } else {
                                        format!("No journal logs recorded yet. Click '{} Refresh Logs'.", crate::lucide::REFRESH_CW)
                                    }
                                })
                            };

                            let log_h = (ui.available_height() - 16.0).max(0.0);
                            Frame::none()
                                .fill(p.terminal_bg)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(8.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                .show(ui, |ui| {
                                    ui.set_height(log_h);
                                    ScrollArea::vertical()
                                        .id_salt("journalctl_scroll")
                                        .max_height(log_h)
                                        .stick_to_bottom(true)
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            if display_logs.trim().is_empty() {
                                                ui.label(RichText::new("No journal logs recorded for this service.").size(11.5).color(p.text_muted));
                                            } else {
                                                ui.label(
                                                    RichText::new(&display_logs)
                                                        .size(11.0)
                                                        .monospace()
                                                        .color(p.log_stdout),
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
                    ui.add_space((ui.available_height() * 0.22).min(100.0));
                    Frame::none().fill(p.bg_card).stroke(Stroke::NONE).rounding(Rounding::same(14.0))
                        .inner_margin(egui::Margin::same(20.0)).show(ui, |ui| {
                            ui.set_width(ui.available_width().min(380.0));
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new(crate::lucide::SYSTEMD).size(24.0).color(p.accent));
                                ui.label(RichText::new(if selected_unit.is_some() { "Loading service details…" } else { "Select a service" }).size(14.0).strong().color(p.text_primary));
                                if let Some(message) = if host_id.is_some() { self.remote_service_action_msg.lock().ok().and_then(|message| message.clone()) } else { self.service_action_msg.clone() } {
                                    ui.label(RichText::new(message).size(11.5).color(p.warning));
                                }
                                ui.label(RichText::new("Choose a service from the list to view its status, logs and controls.")
                                    .size(11.5).color(p.text_muted));
                            });
                        });
                });
            }
        }
    }

    pub(in crate::app) fn render_systemd_files_tab(
        &mut self,
        ui: &mut Ui,
        s: &SystemdService,
        user_mode: bool,
        host_opt: Option<&SshRemoteHost>,
    ) {
        let p = self.palette();
        let context = (host_opt.map(|host| host.id.as_str()).unwrap_or("local").to_string(), user_mode);
        if self.systemd_editor_context.as_ref() != Some(&context) {
            if self.systemd_file_content != self.systemd_file_original {
                ui.label(RichText::new("Unsaved edits belong to another host or scope. Return there to save or revert them before opening this editor.").color(p.warning));
                return;
            }
            self.systemd_editor_context = Some(context);
            self.systemd_selected_file = None;
            self.systemd_file_content.clear();
            self.systemd_file_original.clear();
            self.systemd_file_status_msg = None;
        }
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
        let read_key = PendingSudoAction::ReadUnitFile { path: cur_path.clone(), user_mode }.key(host_opt.map(|host| host.id.as_str()));
        let is_content_in_flight = self.systemd_actions_in_flight.lock().unwrap().contains(&read_key);

        // Auto-read selected file into editor if cached or trigger async read
        if !cur_path.is_empty() {
            if let Ok(cache) = self.systemd_file_content_cache.lock() {
                if let Some(Ok(content)) = cache.get(&file_cache_key) {
                    if self.systemd_file_original.is_empty() && self.systemd_file_content.is_empty() {
                        self.systemd_file_content = content.clone();
                        self.systemd_file_original = content.clone();
                    }
                } else if !cache.contains_key(&file_cache_key) && !is_content_in_flight {
                    self.trigger_systemd_file_read(&cur_path, user_mode, host_opt);
                }
            }
        }

        // File selector strip: long unit names cannot push the action controls offscreen.
        ScrollArea::horizontal().id_salt(("unit_file_tabs", host_opt.map(|host| host.id.as_str()), &s.name))
            .auto_shrink([false, true]).max_height(30.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.set_min_height(24.0);
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
                .rounding(Rounding::same(8.0)).wrap_mode(egui::TextWrapMode::Truncate);
                let file_width = ui.fonts(|fonts| fonts.layout_no_wrap(label.clone(), egui::FontId::proportional(11.0), p.text_primary).size().x + 28.0).min(180.0);
                if ui.add_sized([file_width, 24.0], btn).hover_tip(format!("Path: {}\nType: {}", path, if *is_dropin { "Drop-In Override" } else { "Primary Unit File" })).clicked() {
                    switch_to_path = Some(path.clone());
                }
            }

            if let Some(new_p) = switch_to_path {
                if self.systemd_file_content != self.systemd_file_original {
                    self.systemd_file_status_msg = Some(("Save or revert changes before switching files.".into(), true));
                    return;
                }
                self.systemd_selected_file = Some(new_p.clone());
                self.systemd_file_status_msg = None;
                let target_file_cache_key = format!("{}:{}", host_opt.map(|h| h.id.as_str()).unwrap_or("local"), new_p);
                let cached_opt = self.systemd_file_content_cache.lock().ok().and_then(|c| c.get(&target_file_cache_key).cloned());
                if let Some(Ok(content)) = cached_opt {
                    self.systemd_file_content = content.clone();
                    self.systemd_file_original = content;
                } else {
                    self.systemd_file_content.clear();
                    self.systemd_file_original.clear();
                    self.trigger_systemd_file_read(&new_p, user_mode, host_opt);
                }
            }

        });
        });
        ui.horizontal(|ui| {
            ui.set_min_height(24.0);
            // + New Drop-In Button
            let btn_new_dropin = egui::Button::new(
                RichText::new(format!("{} New Drop-In", crate::lucide::PLUS)).size(11.0).strong().color(p.accent_text),
            )
            .fill(p.accent)
            .rounding(Rounding::same(999.0));
            if ui.add(btn_new_dropin).hover_tip("Create a new configuration drop-in override (e.g. override.conf)").clicked() {
                self.systemd_new_dropin_open = !self.systemd_new_dropin_open;
            }

            // Refresh Files button
            let btn_ref = egui::Button::new(RichText::new(format!("{} Refresh", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_secondary))
                .fill(p.bg_button)
                .rounding(Rounding::same(999.0));
            if ui.add_enabled(self.systemd_file_content == self.systemd_file_original, btn_ref).hover_tip("Save or revert edits before reloading files from disk").clicked() {
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
                        ui.label(RichText::new(format!("{} {}", crate::lucide::CHEVRON_RIGHT, dropin_dir_preview)).size(10.5).color(p.text_muted));
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
                                self.systemd_new_dropin_content = t_body.to_string();
                            }
                        }
                    });

                    ui.add_space(6.0);
                    ui.add(egui::TextEdit::multiline(&mut self.systemd_new_dropin_content).code_editor()
                        .font(egui::FontId::monospace(11.5)).text_color(p.text_secondary).desired_rows(4).desired_width(ui.available_width()));
                    ui.horizontal(|ui| {
                        let btn_create = egui::Button::new(RichText::new(format!("{} Create Drop-In", crate::lucide::CHECK)).size(11.0).strong().color(Color32::WHITE))
                            .fill(Color32::from_rgb(16, 185, 129))
                            .rounding(Rounding::same(999.0));
                        if ui.add(btn_create).clicked() {
                            if self.systemd_file_content != self.systemd_file_original {
                                self.systemd_file_status_msg = Some(("Save or revert the current file before creating a drop-in.".into(), true));
                                return;
                            }
                            let name = self.systemd_new_dropin_name.trim();
                            let name = if name.ends_with(".conf") { name.to_string() } else { format!("{}.conf", name) };
                            if let Err(error) = crate::systemd::validate_drop_in_name(&name).and_then(|_| crate::systemd::validate_unit_name(&s.name)) {
                                self.systemd_file_status_msg = Some((error, true)); return;
                            }
                            let path = if user_mode {
                                if host_opt.is_some() { format!("~/.config/systemd/user/{}.d/{}", s.name, name) }
                                else { match crate::systemd::user_unit_directory() {
                                    Ok(directory) => directory.join(format!("{}.d", s.name)).join(&name).to_string_lossy().into_owned(),
                                    Err(error) => { self.systemd_file_status_msg = Some((error, true)); return; }
                                }}
                            } else { format!("/etc/systemd/system/{}.d/{}", s.name, name) };
                            self.queue_systemd_action(PendingSudoAction::CreateDropIn { path, content: self.systemd_new_dropin_content.clone(), user_mode }, host_opt, None);
                        }
                    });
                });
        }

        ui.add_space(6.0);

        // Editor Control Card Header
        let cur_path = self.systemd_selected_file.clone().unwrap_or_default();
        let is_dropin = cur_path.contains(".d/");
        let is_modified = self.systemd_file_content != self.systemd_file_original;
        let file_cache_key = format!("{}:{}", host_opt.map(|host| host.id.as_str()).unwrap_or("local"), cur_path);
        let save_key = PendingSudoAction::SaveUnitFile { path: cur_path.clone(), content: String::new(), user_mode }.key(host_opt.map(|host| host.id.as_str()));
        let delete_key = PendingSudoAction::DeleteDropIn { path: cur_path.clone(), user_mode }.key(host_opt.map(|host| host.id.as_str()));
        let file_busy = { let work = self.systemd_actions_in_flight.lock().unwrap(); work.contains(&save_key) || work.contains(&delete_key) };
        let file_loaded = self.systemd_file_content_cache.lock().unwrap().get(&file_cache_key).is_some_and(|content| content.is_ok());

        Frame::none()
            .fill(p.bg_card)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let path_width = (ui.available_width() - 108.0 - if is_modified { 100.0 } else { 0.0 }).max(0.0);
                    ui.add_sized([path_width, 20.0], egui::Label::new(RichText::new(format!("{} {}", if is_dropin { crate::lucide::PACKAGE } else { crate::lucide::FILE_TEXT }, &cur_path))
                        .size(11.0).monospace().color(p.info)).truncate().halign(egui::Align::Min)).hover_tip(&cur_path);
                    ui.label(RichText::new(if is_dropin { "Drop-in override" } else { "Primary unit" }).size(10.0).color(p.text_muted));
                    if is_modified { ui.label(RichText::new("● Unsaved").size(10.5).color(p.warning)); }
                });
                // A horizontal row must size to its controls, leaving the remaining height for the editor.
                ui.horizontal_wrapped(|ui| {
                    ui.set_min_height(24.0);
                    ui.spacing_mut().button_padding = egui::vec2(6.0, 3.0);
                        ui.spacing_mut().item_spacing.x = 6.0;

                        // Quick restart service button
                        let btn_restart = egui::Button::new(RichText::new(format!("{} Restart Service", crate::lucide::REFRESH_CW)).size(10.5).color(p.text_secondary))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(999.0));
                        if ui.add(btn_restart).hover_tip("Restart service to activate new configuration").clicked() {
                            self.queue_systemd_action(PendingSudoAction::ServiceAction { unit: s.name.clone(), action: "restart", user_mode }, host_opt, None);
                        }

                        // Delete Drop-In (if dropin)
                        if is_dropin {
                            let btn_del = egui::Button::new(RichText::new(format!("{} Delete Drop-In", crate::lucide::TRASH)).size(10.5).strong().color(Color32::WHITE))
                                .fill(p.danger)
                                .rounding(Rounding::same(999.0));
                            if ui.add_enabled(!is_modified && !file_busy, btn_del).hover_tip("Save or revert edits before deleting this drop-in").clicked() {
                                self.queue_systemd_action(PendingSudoAction::DeleteDropIn { path: cur_path.clone(), user_mode }, host_opt, None);
                            }
                        }

                        // Revert Button
                        if is_modified {
                            let btn_revert = egui::Button::new(RichText::new("Revert").size(10.5).color(p.text_secondary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(999.0));
                            if ui.add(btn_revert).hover_tip("Discard changes and restore original file").clicked() {
                                self.systemd_file_content = self.systemd_file_original.clone();
                            }
                        }

                        // Save & Reload Button
                        let btn_save = egui::Button::new(
                            RichText::new(format!("{} Save & Reload", crate::lucide::SAVE)).size(10.5).strong().color(Color32::WHITE),
                        )
                        .fill(if is_modified { p.accent } else { p.bg_button })
                        .rounding(Rounding::same(999.0));
                        if ui.add_enabled(file_loaded && is_modified && !file_busy, btn_save).hover_tip("Save file to disk and execute systemctl daemon-reload").clicked() {
                            self.queue_systemd_action(PendingSudoAction::SaveUnitFile { path: cur_path.clone(), content: self.systemd_file_content.clone(), user_mode }, host_opt, None);
                        }
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
        let edit_h = (ui.available_height() - 16.0).max(0.0);
        Frame::none()
            .fill(p.terminal_bg)
            .stroke(Stroke::new(1.0_f32, p.border.gamma_multiply(0.6)))
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.set_height(edit_h);
                ScrollArea::both()
                    .id_salt(("systemd_file_editor_scroll", &cur_path))
                    .max_height(edit_h).drag_to_scroll(false)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.systemd_file_content)
                                .id_salt(("systemd_file_editor", &cur_path)).interactive(file_loaded && {
                                    let work = self.systemd_actions_in_flight.lock().unwrap();
                                    !work.contains(&save_key) && !work.contains(&delete_key)
                                })
                                .font(egui::FontId::monospace(11.5)).text_color(p.log_stdout).frame(false)
                                .desired_width(f32::INFINITY)
                                .desired_rows(25)
                                .code_editor()
                                .lock_focus(true),
                        );
                    });
            });
    }

    pub(in crate::app) fn render_systemd_dependencies_tab(
        &mut self,
        ui: &mut Ui,
        s: &SystemdService,
        user_mode: bool,
        host_opt: Option<&SshRemoteHost>,
        services_list: &[SystemdService],
    ) {
        let p = self.palette();
        let (deps, is_deps_loading) = self.load_systemd_unit_dependencies(&s.name, user_mode, host_opt);

        let mut navigate_to_unit: Option<String> = None;

        // Top Toolbar
        ui.horizontal_wrapped(|ui| {
            ui.set_min_height(24.0);
            ui.spacing_mut().interact_size.y = 24.0;
            ui.spacing_mut().button_padding = egui::vec2(6.0, 3.0);
            // History Back button
            if let Some(prev) = self.systemd_deps_history.last() {
                let btn_back = egui::Button::new(
                    RichText::new(format!("{} Back", crate::lucide::ARROW_LEFT)).size(11.0).strong().color(p.accent_text),
                )
                .fill(p.accent)
                .rounding(Rounding::same(999.0));
                if ui.add(btn_back).hover_tip(format!("Back to {}", prev)).clicked() {
                    let prev_unit = self.systemd_deps_history.pop().unwrap();
                    self.select_systemd_unit(&prev_unit, user_mode, host_opt);
                    return;
                }
            }

            if is_deps_loading {
                ui.spinner();
            }

            // Visible filter surface with the same compact sizing as the view controls.
            Frame::none().fill(p.bg_button).stroke(Stroke::new(1.0_f32, p.border))
                .rounding(Rounding::same(8.0)).inner_margin(egui::Margin::symmetric(8.0, 4.0)).show(ui, |ui| {
                    ui.set_width(184.0);
                    ui.spacing_mut().interact_size.y = 16.0;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::lucide::SEARCH).size(11.0).color(p.text_muted));
                        ui.add_enabled(self.systemd_deps_view_mode == 0, egui::TextEdit::singleline(&mut self.systemd_deps_filter)
                            .font(egui::FontId::proportional(11.5)).text_color(p.text_secondary)
                            .hint_text(RichText::new("Filter dependencies…").color(p.text_muted))
                            .frame(false).margin(egui::Vec2::ZERO).clip_text(true).desired_width(ui.available_width()));
                    });
                });

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
                if ui.add(btn_ref).hover_tip("Refresh dependencies").clicked() {
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
                        ("Requires (Hard Dependencies)", "Units pulled in when this unit starts. Ordering is controlled separately by After and Before.", &deps.requires, p.danger),
                        ("Wants (Soft Dependencies)", "Suggested units that start alongside this unit. A failure does not stop this unit.", &deps.wants, p.info),
                        ("After (Order After)", "Orders this unit after these units when both are started; does not pull them in.", &deps.after, p.accent),
                        ("Before (Order Before)", "Orders this unit before these units when both are started; does not pull them in.", &deps.before, p.info),
                        ("Conflicts (Incompatible Units)", "Units that cannot run simultaneously with this unit.", &deps.conflicts, p.danger),
                        ("Required By (Reverse Hard Dependencies)", "Units on the system that strictly require this unit.", &deps.required_by, p.warning),
                        ("Wanted By (Reverse Soft Dependencies)", "Units on the system that declare Wants= on this unit.", &deps.wanted_by, p.success),
                        ("Part Of", "Stopping or restarting these units propagates to this unit.", &deps.part_of, p.accent),
                        ("Binds To", "Units whose disappearance also stops this unit.", &deps.binds_to, p.warning),
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
                                ui.set_width(ui.available_width());
                                ui.horizontal_wrapped(|ui| {
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
                                                    (crate::lucide::CHECK, p.success)
                                                } else if known.is_failed() {
                                                    (crate::lucide::X, p.danger)
                                                } else {
                                                    (crate::lucide::CIRCLE, p.text_muted)
                                                }
                                            } else if unit_name.ends_with(".target") {
                                                (crate::lucide::RADIO, Color32::from_rgb(148, 163, 184))
                                            } else if unit_name.ends_with(".socket") {
                                                (crate::lucide::NETWORK, p.info)
                                            } else if unit_name.ends_with(".slice") {
                                                (crate::lucide::LAYERS, p.accent)
                                            } else {
                                                (crate::lucide::SETTINGS, p.text_muted)
                                            };

                                            let btn = egui::Button::new(
                                                RichText::new(format!("{} {}", dot, unit_name))
                                                    .size(11.0)
                                                    .monospace()
                                                    .color(p.text_primary),
                                            )
                                            .fill(p.bg_button)
                                            .stroke(Stroke::new(1.0_f32, p.border.gamma_multiply(0.4)))
                                            .rounding(Rounding::same(8.0)).wrap_mode(egui::TextWrapMode::Truncate);
                                            let width = ui.fonts(|fonts| fonts.layout_no_wrap(unit_name.to_string(), egui::FontId::monospace(11.0), p.text_primary).size().x + 28.0)
                                                .min(ui.max_rect().width()).min(280.0);
                                            if ui.add_sized([width, 24.0], btn).hover_tip(format!("Click to view and inspect dependencies for {}", unit_name)).clicked() {
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
            let tree_h = (ui.available_height() - 16.0).max(0.0);
            Frame::none()
                .fill(p.terminal_bg)
                .stroke(Stroke::new(1.0_f32, p.border.gamma_multiply(0.6)))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.set_height(tree_h);
                    ScrollArea::both()
                        .id_salt(("systemd_deps_tree_scroll", &s.name))
                        .max_height(tree_h).drag_to_scroll(false)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if deps.tree_output.trim().is_empty() {
                                ui.label(RichText::new("No dependency tree output returned from systemctl list-dependencies.").size(11.0).color(p.text_muted));
                            } else {
                                ui.add(egui::Label::new(RichText::new(&deps.tree_output)
                                    .size(11.5).monospace().color(p.log_stdout)).extend().selectable(true));
                            }
                        });
                });
        }

        if let Some(target_unit) = navigate_to_unit {
            self.systemd_deps_history.push(s.name.clone());
            self.select_systemd_unit(&target_unit, user_mode, host_opt);
        }
    }
}
