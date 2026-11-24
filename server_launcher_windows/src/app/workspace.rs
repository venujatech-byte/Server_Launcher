use super::*;

impl LauncherApp {
    pub(in crate::app) fn render_machines_nav_bar(&mut self, ui: &mut Ui) {
        let p = self.palette();

        let mut host_to_edit: Option<SshRemoteHost> = None;
        let mut host_to_delete: Option<String> = None;
        let mut host_to_test: Option<SshRemoteHost> = None;
        let mut host_to_term: Option<SshRemoteHost> = None;
        let mut adb_to_delete: Option<String> = None;
        let mut adb_to_mirror: Option<(String, String)> = None;

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
                                ui.set_min_width(160.0);
                                ui.spacing_mut().interact_size.y = 24.0;
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.spacing_mut().button_padding = egui::vec2(8.0, 3.0);
                                let item = |label: String, color| egui::Button::new(RichText::new(label).size(11.5).color(color));
                                if ui.add(item(format!("{} Edit Host", crate::lucide::SETTINGS), p.text_secondary)).clicked() {
                                    host_to_edit = Some(host.clone());
                                    ui.close_menu();
                                }
                                if ui.add(item(format!("{} Test Connection", crate::lucide::REFRESH_CW), p.text_secondary)).clicked() {
                                    host_to_test = Some(host.clone());
                                    ui.close_menu();
                                }
                                if ui.add(item(format!("{} Launch External Terminal", crate::lucide::EXTERNAL_LINK), p.text_secondary)).clicked() {
                                    host_to_term = Some(host.clone());
                                    ui.close_menu();
                                }
                                ui.separator();
                                if ui.add(item(format!("{} Delete Host", crate::lucide::TRASH), p.danger)).clicked() {
                                    host_to_delete = Some(host.id.clone());
                                    ui.close_menu();
                                }
                            });

                            ui.add_space(6.0);
                        }

                        for adb_dev in &self.adb_devices {
                            let is_selected = self.selected_machine == adb_dev.id;
                            let adb_tooltip = format!(
                                "{} {}\nSerial: {}\nMode: {}\nRight-click for options",
                                crate::lucide::SMARTPHONE,
                                adb_dev.name,
                                adb_dev.serial,
                                if adb_dev.is_wireless { "Wireless (TCP/IP)" } else { "USB / Direct" }
                            );

                            let (dev_rect, dev_resp) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::click());
                            let dev_bg = if is_selected {
                                p.accent
                            } else if dev_resp.hovered() {
                                p.bg_card_active
                            } else {
                                p.bg_card
                            };
                            let dev_stroke = Stroke::NONE;

                            ui.painter().rect(dev_rect, Rounding::same(8.0), dev_bg, dev_stroke);
                            let dev_icon_col = if is_selected { Color32::WHITE } else { p.accent_light };
                            ui.painter().text(
                                dev_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                crate::lucide::SMARTPHONE,
                                egui::FontId::proportional(15.0),
                                dev_icon_col,
                            );

                            // Status indicator dot at bottom-right (cyan for wireless, emerald for USB)
                            let dot_center = egui::pos2(dev_rect.right() - 6.0, dev_rect.bottom() - 6.0);
                            ui.painter().circle_filled(dot_center, 4.0, p.bg_machines_nav);
                            let dot_col = if adb_dev.is_wireless { p.accent_light } else { p.success };
                            ui.painter().circle_filled(dot_center, 2.5, dot_col);

                            let dev_resp = dev_resp.on_hover_text(adb_tooltip);
                            if dev_resp.clicked() {
                                self.selected_machine = adb_dev.id.clone();
                            }

                            dev_resp.context_menu(|ui| {
                                if ui.button(format!("{} Mirror Screen", crate::lucide::SCREEN_SHARE)).clicked() {
                                    adb_to_mirror = Some((adb_dev.id.clone(), adb_dev.serial.clone()));
                                    ui.close_menu();
                                }
                                if ui.button(format!("{} External Shell", crate::lucide::TERMINAL)).clicked() {
                                    match crate::adb::launch_external_adb_terminal(&adb_dev.serial) {
                                        Ok(_) => self.adb_status_msg = Some(("External ADB terminal opened".to_string(), false, Instant::now())),
                                        Err(e) => self.adb_status_msg = Some((format!("Terminal error: {}", e), true, Instant::now())),
                                    }
                                    ui.close_menu();
                                }
                                ui.separator();
                                if ui.button(RichText::new(format!("{} Remove Device", crate::lucide::TRASH)).color(Color32::from_rgb(239, 68, 68))).clicked() {
                                    adb_to_delete = Some(adb_dev.id.clone());
                                    ui.close_menu();
                                }
                            });

                            ui.add_space(6.0);
                        }

                        // Add SSH Host or ADB Device button tile
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
                        if add_resp.on_hover_text("Add Device (SSH Remote PC or Android ADB)").clicked() {
                            self.add_device_selector_modal_open = true;
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
            self.delete_ssh_host(&id);
        }
        if let Some(id) = adb_to_delete {
            if let Some(pos) = self.adb_devices.iter().position(|d| d.id == id) {
                self.adb_devices.remove(pos);
                if self.selected_machine == id {
                    self.selected_machine = "local".to_string();
                }
                self.persist_config();
            }
        }
        if let Some((id, serial)) = adb_to_mirror {
            self.selected_machine = id;
            self.adb_active_subtab = AdbSubTab::ScreenShare;
            self.start_builtin_mirror(&serial);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // TOP NAVIGATION TABS BAR (Servers, Scripts, Commands, SystemMD, Sensors)
    // ═════════════════════════════════════════════════════════════════════════
    // The top strip keeps one geometry while the workspace below can use a compact sidebar.
    pub(in crate::app) fn render_workspace_header(&mut self, ui: &mut Ui, workspace_rect: egui::Rect, host_id: Option<&str>) {
        let p = self.palette();
        // Reserve header space per column so other detail panes can still start at the top.
        let rect = egui::Rect::from_min_size(workspace_rect.min, egui::vec2(workspace_rect.width(), 32.0));
        let mut header_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        let ui = &mut header_ui;
        ui.horizontal(|ui| {
            ui.set_min_height(32.0);
            ui.spacing_mut().item_spacing.x = 0.0;
            Frame::none()
                .fill(p.bg_sidebar)
                .inner_margin(WORKSPACE_SIDEBAR_MARGIN)
                .show(ui, |ui| {
                    ui.set_width(SERVERS_SIDEBAR_WIDTH);
                    ui.set_max_width(SERVERS_SIDEBAR_WIDTH);
                    ui.spacing_mut().item_spacing.x = ui.ctx().style().spacing.item_spacing.x;
                    self.render_sidebar_tabs_bar(ui);
                });
            ui.add_space(WORKSPACE_GAP);
            if self.active_sidebar_tab == SidebarTab::Sensors {
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width().max(0.0), 32.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = ui.ctx().style().spacing.item_spacing.x;
                        self.render_system_brief_info_panel(ui, host_id);
                    },
                );
            }
        });
    }

    pub(in crate::app) fn render_sidebar_tabs_bar(&mut self, ui: &mut Ui) {
        let p = self.palette();
        Frame::none()
            .fill(p.bg_card)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(10.0))
            .inner_margin(egui::Margin::symmetric(3.0, 3.0))
            .show(ui, |ui| {
                const TAB_GAP: f32 = 3.0;
                let tab_width = ((ui.available_width() - 4.0 * TAB_GAP) / 5.0).clamp(0.0, 66.0);
                let tabs_width = 5.0 * tab_width + 4.0 * TAB_GAP;
                ui.set_min_width(tabs_width);
                ui.set_max_width(tabs_width);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = TAB_GAP;
                    ui.spacing_mut().button_padding = egui::vec2(3.0, 4.0);
                    let tabs = [
                        (SidebarTab::Servers, SidebarTab::Servers.label()),
                        (SidebarTab::Scripts, SidebarTab::Scripts.label()),
                        (SidebarTab::Commands, SidebarTab::Commands.label()),
                        (SidebarTab::SystemMD, SidebarTab::SystemMD.label()),
                        (SidebarTab::Sensors, SidebarTab::Sensors.label()),
                    ];

                    for (tab, label) in tabs {
                        let is_active = self.active_sidebar_tab == tab;
                        let bg = if is_active {
                            p.accent
                        } else {
                            Color32::TRANSPARENT
                        };
                        let fg = if is_active {
                            p.accent_text
                        } else {
                            p.text_secondary
                        };
                        let compact_label = label.split_once(' ').map_or(label, |(_, text)| text);
                        let btn = egui::Button::new(
                            RichText::new(if tab_width < 60.0 || (tab == SidebarTab::SystemMD && tab_width < 72.0) { compact_label } else { label })
                                .size(10.5)
                                .strong()
                                .color(fg),
                        )
                        .fill(bg)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .wrap_mode(egui::TextWrapMode::Truncate);

                        let (rect, _) = ui.allocate_exact_size(egui::vec2(tab_width, 26.0), egui::Sense::hover());
                        if ui.put(rect, btn).on_hover_text(label).clicked() {
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

    pub(in crate::app) fn render_left_panel(&mut self, ui: &mut Ui) {
        ui.set_width(ui.available_width());
        ui.set_max_width(ui.available_width());
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.set_max_width(ui.available_width());
            match self.active_sidebar_tab {
                SidebarTab::Servers => self.render_servers_left_panel(ui),
                SidebarTab::Scripts => self.render_scripts_left_panel(ui, None),
                SidebarTab::Commands => self.render_commands_left_panel(ui, None),
                SidebarTab::SystemMD => self.render_systemd_left_panel(ui, None),
                SidebarTab::Sensors => self.render_sensors_left_panel(ui, None),
            }
        });
    }

    pub(in crate::app) fn render_right_panel(&mut self, ui: &mut Ui) {
        match self.active_sidebar_tab {
            SidebarTab::Servers => self.render_servers_right_panel(ui),
            SidebarTab::Scripts => self.render_scripts_right_panel(ui, None),
            SidebarTab::Commands => self.render_commands_right_panel(ui, None),
            SidebarTab::SystemMD => self.render_systemd_right_panel(ui, None),
            SidebarTab::Sensors => self.render_sensors_right_panel(ui, None),
        }
    }
}
