use super::*;

mod listeners;
mod terminal;
mod connection;

fn request_remote_listener_refresh(ctx: &Context, host_id: String) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<BTreeSet<String>>(egui::Id::new(REMOTE_LISTENER_REFRESH))
            .insert(host_id);
    });
    ctx.request_repaint();
}

impl LauncherApp {
    pub(in crate::app) fn render_remote_pc_dashboard(&mut self, ui: &mut Ui, host_id: &str) {
        let p = self.palette();
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
            map.contains_key(&host.id)
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

            let dialog_open = ui.ctx().data(|data| data.get_temp::<(crate::remote::RemoteListener, bool)>(
                egui::Id::new(("ssh_listener_confirmation", &host.id))
            )).is_some();
            let header_rect = ui.available_rect_before_wrap();

            // Split layout: Sidebar + remaining content; Sensors uses a compact column.
            let total_h = ui.available_height().max(0.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.set_height(total_h);
                ui.set_min_height(total_h);

                let left_w = self.workspace_sidebar_width();

                // LEFT PANEL
                Frame::none()
                    .fill(p.bg_sidebar)
                    .inner_margin(WORKSPACE_SIDEBAR_MARGIN)
                    .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = ui.ctx().style().spacing.item_spacing.x;
                    ui.set_width(left_w);
                    ui.set_max_width(left_w);
                    ui.set_height(total_h);
                    ui.set_min_height(total_h);
                    let mut body_ui = workspace_sidebar_body(ui, false);
                    let ui = &mut body_ui;
                    // Left panel content for active tab
                    match self.active_sidebar_tab {
                        SidebarTab::Servers => self.render_remote_servers_left_panel(ui, &host),
                        SidebarTab::Scripts => self.render_scripts_left_panel(ui, Some(&host.id)),
                        SidebarTab::Commands => self.render_commands_left_panel(ui, Some(&host.id)),
                        SidebarTab::SystemMD => self.render_systemd_left_panel(ui, Some(&host.id)),
                        SidebarTab::Sensors => self.render_sensors_left_panel(ui, Some(&host.id)),
                    }
                });

                ui.add_space(WORKSPACE_GAP);
                let right_w = ui.available_width().max(0.0);

                // RIGHT PANEL (Remaining width)
                ui.allocate_ui_with_layout(egui::vec2(right_w, total_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                    if dialog_open { ui.disable(); }
                    ui.spacing_mut().item_spacing.x = ui.ctx().style().spacing.item_spacing.x;
                    ui.set_width(right_w);
                    ui.set_max_width(right_w);
                    ui.set_height(total_h);
                    ui.set_min_height(total_h);
                    if self.active_sidebar_tab == SidebarTab::Sensors {
                        ui.add_space(WORKSPACE_HEADER_HEIGHT);
                    }
                    match self.active_sidebar_tab {
                        SidebarTab::Servers => self.render_remote_servers_right_panel(ui, &host),
                        SidebarTab::Scripts => self.render_scripts_right_panel(ui, Some(&host.id)),
                        SidebarTab::Commands => self.render_commands_right_panel(ui, Some(&host.id)),
                        SidebarTab::SystemMD => self.render_systemd_right_panel(ui, Some(&host.id)),
                        SidebarTab::Sensors => self.render_sensors_right_panel(ui, Some(&host.id)),
                    }
                });
            });
            if dialog_open {
                ui.scope(|ui| {
                    ui.disable();
                    self.render_workspace_header(ui, header_rect, Some(&host.id));
                });
            } else {
                self.render_workspace_header(ui, header_rect, Some(&host.id));
            }
        });
    }
}
