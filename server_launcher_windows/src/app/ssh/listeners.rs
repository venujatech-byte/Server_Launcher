use super::*;

impl LauncherApp {
    pub(in crate::app) fn render_remote_servers_left_panel(&mut self, ui: &mut Ui, host: &SshRemoteHost) {
        let p = self.palette();
        let pending_id = egui::Id::new(("ssh_listener_confirmation", &host.id));
        let pending = ui.ctx().data(|d| d.get_temp::<(crate::remote::RemoteListener, bool)>(pending_id));
        let mut requested = None;
        let is_scanning = {
            let map = self.remote_scanning.lock().unwrap();
            map.contains_key(&host.id)
        };
        let RemoteListenerSnapshot { rows: listeners, error } = {
            let map = self.remote_listeners.lock().unwrap();
            map.get(&host.id).cloned().unwrap_or_default()
        };

        let stopped = {
            let mut map = self.remote_stopped_servers.lock().unwrap();
            map.entry(host.id.clone()).or_default().iter()
                .filter(|entry| listeners.binary_search_by_key(&entry.port, |row| row.port).is_err())
                .cloned()
                .collect::<Vec<_>>()
        };

        let mut dismiss_port: Option<u16> = None;

        ui.vertical(|ui| {
            if pending.is_some() { ui.disable(); }
            ui.spacing_mut().scroll.bar_width = 6.0;
            ui.spacing_mut().scroll.floating = false;
            ui.spacing_mut().scroll.bar_inner_margin = 10.0;
            ui.add_space(8.0);
            // Header with title, counts, and action toolbar
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("REMOTE SERVERS")
                        .size(11.5)
                        .strong()
                        .color(Color32::from_rgb(148, 163, 184)),
                );
                let count_desc = if stopped.is_empty() {
                    format!("({})", listeners.len())
                } else {
                    format!("({} active, {} stopped)", listeners.len(), stopped.len())
                };
                ui.label(
                    RichText::new(count_desc)
                        .size(11.0)
                        .color(Color32::from_rgb(100, 110, 130)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;

                    // + Start Server button (accent pill)
                    let btn_add = egui::Button::new(
                        RichText::new("+ Start Server").size(11.0).strong().color(p.accent_text),
                    )
                    .fill(p.accent)
                    .rounding(Rounding::same(8.0));
                    if ui.add(btn_add).on_hover_text("Start a new server or process on remote host").clicked() {
                        self.remote_start_server_modal.open_for_host(host);
                    }
                });
            });
            ui.add_space(2.0);

            let search_q = self.remote_search_query.entry(host.id.clone()).or_default();
            Frame::none().fill(p.bg_button).rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(8.0, 2.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::lucide::SEARCH).color(p.text_muted));
                        let response = ui.add(egui::TextEdit::singleline(search_q).font(egui::FontId::proportional(12.0))
                            .frame(false).id_source(("remote_filter", &host.id))
                            .hint_text(RichText::new("Filter remote servers...").color(p.text_muted))
                            .desired_width((ui.available_width() - 34.0).max(0.0)));
                        if !search_q.is_empty() && ui.small_button(crate::lucide::X).on_hover_text("Clear server filter").clicked() {
                            search_q.clear();
                            response.request_focus();
                        }
                    });
                });
            ui.add_space(6.0);

            if let Some(error) = &error {
                let status = if is_scanning {
                    "Retrying port scan…"
                } else if listeners.is_empty() {
                    "Port scan failed. Use Scan Ports to retry."
                } else {
                    "Port scan failed. Showing the last successful results."
                };
                ui.add(egui::Label::new(RichText::new(status).size(11.0).color(p.danger)).wrap())
                    .on_hover_text(error.as_ref());
                ui.add_space(4.0);
            }

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
                .collect();

            let visible_stopped: Vec<_> = stopped
                .iter()
                .filter(|s| {
                    if q.is_empty() {
                        true
                    } else {
                        s.name.to_lowercase().contains(&q)
                            || s.port.to_string().contains(&q)
                            || s.proto.to_lowercase().contains(&q)
                            || s.cmd.to_lowercase().contains(&q)
                            || s.start_cmd.to_lowercase().contains(&q)
                            || s.last_pid.map(|p| p.to_string().contains(&q)).unwrap_or(false)
                    }
                })
                .collect();

            if listeners.is_empty() && stopped.is_empty() && error.is_some() {
                return;
            }
            if listeners.is_empty() && stopped.is_empty() {
                Frame::none()
                    .fill(p.bg_card)
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if is_scanning {
                            ui.label(RichText::new("Scanning remote listening ports (ss -tulpn)...").size(11.0).color(p.text_secondary));
                        } else {
                            ui.label(RichText::new("No active listening ports detected on remote PC. Click 'Scan Ports' or '+ Start Server'.").size(11.0).color(Color32::from_rgb(120, 128, 145)));
                        }
                    });
            } else if visible_listeners.is_empty() && visible_stopped.is_empty() {
                ui.label(RichText::new(format!("No servers matching \"{}\"", q)).size(11.0).color(Color32::from_rgb(120, 128, 145)));
            } else {
                ScrollArea::vertical()
                    .id_salt(format!("remote_listeners_scroll_{}", host.id))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_max_width(ui.available_width());

                        // 1. ACTIVE RUNNING SERVERS
                        for l in visible_listeners {
                            Frame::none()
                                .fill(p.bg_card)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(12.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.set_max_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        let name_width = (ui.available_width() - 78.0).max(0.0);
                                        ui.add_sized([name_width, 18.0], egui::Label::new(
                                            RichText::new(&l.name).size(11.5).strong().color(p.text_primary)
                                        ).truncate()).on_hover_text(&l.name);
                                        ui.label(RichText::new("● Running").size(11.0).color(p.success));
                                    });
                                    ui.label(RichText::new(format!(":{} · {}", l.port, l.proto.to_ascii_uppercase()))
                                        .monospace().size(11.0).color(p.text_muted));

                                    // Row 2: PID, CPU, RAM, and command
                                    if let Some(pid) = l.pid {
                                        let mut desc = format!("PID {}", pid);
                                        if let Some(cpu) = l.cpu_pct {
                                            desc.push_str(&format!(" • {:.1}% CPU", cpu));
                                        }
                                        if let Some(mem) = l.mem_mb {
                                            desc.push_str(&format!(" • {:.1} MB", mem));
                                        }
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

                                    ui.add_space(2.0);

                                    // Row 3: Action Buttons (Stop, Restart, Kill, Curl, Info)
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;

                                        // Stop Button
                                        let stop_btn = egui::Button::new(
                                            RichText::new("■ Stop").size(11.0).color(p.text_primary),
                                        )
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(8.0));
                                        let identified = l.pid.is_some() && l.start_marker.is_some();
                                        if ui.add_enabled(identified, stop_btn).on_disabled_hover_text("Process identity is unavailable.").on_hover_text("Stop this remote server gracefully").clicked() {
                                            requested = Some((l.clone(), false));
                                        }

                                        // Restart Button
                                        let restart_btn = egui::Button::new(
                                            RichText::new(format!("{} Restart", crate::lucide::REFRESH_CW)).size(11.0).color(p.accent),
                                        )
                                        .fill(p.accent_subtle)
                                        .rounding(Rounding::same(8.0));
                                        let restart_cmd = build_remote_restart_cmd(l, host.port);
                                        if ui.add_enabled(restart_cmd.is_some(), restart_btn)
                                            .on_hover_text("Restart the verified systemd service")
                                            .on_disabled_hover_text("Restart requires a verified TCP listener outside the SSH service.")
                                            .clicked()
                                        {
                                            if let Some(command) = restart_cmd {
                                                self.run_remote_ssh_command(host, &command, Some(ui.ctx().clone()));
                                                self.schedule_remote_scan(host, 2000, ui.ctx().clone());
                                            }
                                        }

                                        ui.menu_button(RichText::new("⋯").size(11.0), |ui| {
                                            ui.set_min_width(88.0);
                                            ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                                            if ui.add_enabled(identified, egui::Button::new(RichText::new("Kill").size(11.5)).min_size(egui::vec2(88.0, 24.0))).on_disabled_hover_text("Process identity is unavailable.").on_hover_text("Force stop this remote process").clicked() {
                                                requested = Some((l.clone(), true));
                                                ui.close_menu();
                                            }
                                            let name = l.name.to_ascii_lowercase();
                                            let command = l.cmd.to_ascii_lowercase();
                                            let http = l.proto.eq_ignore_ascii_case("tcp")
                                                && !matches!(l.port, 22 | 53 | 68 | 323 | 443 | 8443)
                                                && name != "https" && !name.starts_with("https /")
                                                && (!command.contains("https://") || command.contains("http://"))
                                                && (matches!(l.port, 80 | 3000 | 4173 | 5173 | 8000 | 8008 | 8080 | 8081 | 8888)
                                                    || name == "http" || name.starts_with("http /") || command.contains("http://"));
                                            if ui.add_enabled_ui(http, |ui| ui.add_sized([88.0, 24.0], egui::Button::new(RichText::new("Curl").size(11.5)))).inner
                                                .on_hover_text("Probe HTTP response")
                                                .on_disabled_hover_text("Available for HTTP over TCP").clicked() {
                                                let curl_cmd = format!("curl -I -s --connect-timeout 2 http://localhost:{} | head -n 5", l.port);
                                                self.run_remote_ssh_command(host, &curl_cmd, Some(ui.ctx().clone()));
                                                ui.close_menu();
                                            }
                                        });

                                        // Info Button
                                        let ps_btn = egui::Button::new(
                                            RichText::new(format!("{} Info", crate::lucide::INFO)).size(10.0).color(p.text_secondary),
                                        )
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(8.0));
                                        if ui.add(ps_btn).on_hover_text("Inspect process details on remote host").clicked() {
                                            let ps_cmd = if let Some(pid) = l.pid {
                                                format!("ps -u -p {} 2>/dev/null", pid)
                                            } else {
                                                format!("lsof -i :{} 2>/dev/null", l.port)
                                            };
                                            self.run_remote_ssh_command(host, &ps_cmd, Some(ui.ctx().clone()));
                                        }
                                    });
                                });
                            ui.add_space(2.0);
                        }

                        // 2. STOPPED SERVERS
                        for s in visible_stopped {
                            Frame::none()
                                .fill(p.bg_card)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(12.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.set_max_width(ui.available_width());

                                    ui.horizontal(|ui| {
                                        let name_width = (ui.available_width() - 78.0).max(0.0);
                                        ui.add_sized([name_width, 18.0], egui::Label::new(
                                            RichText::new(&s.name).size(11.5).strong().color(p.text_primary)
                                        ).truncate()).on_hover_text(&s.name);
                                        ui.label(RichText::new("● Stopped").size(11.0).color(p.text_muted));
                                    });
                                    ui.label(RichText::new(format!(":{} · {}", s.port, s.proto.to_ascii_uppercase()))
                                        .monospace().size(11.0).color(p.text_muted));

                                    // Row 2: Stopped details
                                    let desc = if let Some(last_pid) = s.last_pid {
                                        format!("Last PID {} • Offline", last_pid)
                                    } else {
                                        "Offline".to_string()
                                    };
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(desc)
                                                .size(10.0)
                                                .color(Color32::from_rgb(100, 110, 130)),
                                        )
                                        .truncate(),
                                    );

                                    ui.add_space(2.0);

                                    // Row 3: Action Buttons (Start, Restart, Dismiss, Info)
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;

                                        // Start Button
                                        let start_btn = egui::Button::new(
                                            RichText::new(format!("{} Start", crate::lucide::PLAY)).size(10.0).strong().color(p.text_primary),
                                        )
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(8.0));
                                        if ui.add(start_btn).on_hover_text("Start this remote server").clicked() {
                                            let start_cmd = build_remote_start_cmd(&s.name, s.port, if s.start_cmd.is_empty() { None } else { Some(&s.start_cmd) }, &s.cwd);
                                            self.run_remote_ssh_command(host, &start_cmd, Some(ui.ctx().clone()));
                                            self.schedule_remote_scan(host, 1500, ui.ctx().clone());
                                        }

                                        // Restart Button
                                        let restart_btn = egui::Button::new(
                                            RichText::new(format!("{} Restart", crate::lucide::REFRESH_CW)).size(11.0).color(p.accent),
                                        )
                                        .fill(p.accent_subtle)
                                        .rounding(Rounding::same(8.0));
                                        if ui.add(restart_btn).on_hover_text("Attempt to restart this remote server").clicked() {
                                            let start_cmd = build_remote_start_cmd(&s.name, s.port, if s.start_cmd.is_empty() { None } else { Some(&s.start_cmd) }, &s.cwd);
                                            self.run_remote_ssh_command(host, &start_cmd, Some(ui.ctx().clone()));
                                            self.schedule_remote_scan(host, 1500, ui.ctx().clone());
                                        }

                                        // Dismiss Button
                                        let dismiss_btn = egui::Button::new(
                                            RichText::new(format!("{} Dismiss", crate::lucide::X)).size(10.0).color(Color32::from_rgb(148, 163, 184)),
                                        )
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(8.0));
                                        if ui.add(dismiss_btn).on_hover_text("Dismiss from stopped servers list").clicked() {
                                            dismiss_port = Some(s.port);
                                        }

                                        // Info Button
                                        let info_btn = egui::Button::new(
                                            RichText::new(format!("{} Info", crate::lucide::INFO)).size(10.0).color(p.text_secondary),
                                        )
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(8.0));
                                        if ui.add(info_btn).on_hover_text("Inspect status on remote host").clicked() {
                                            let mut info_cmd = format!("lsof -i :{} 2>/dev/null", s.port);
                                            if let Some(unit) = crate::remote::remote_service_operand(&s.name) {
                                                info_cmd.push_str(&format!(" || systemctl status -- {unit}"));
                                            }
                                            self.run_remote_ssh_command(host, &info_cmd, Some(ui.ctx().clone()));
                                        }
                                    });
                                });
                            ui.add_space(2.0);
                        }
                    });
            }
        });

        // Apply any dismissals from stopped list
        if let Some(port) = dismiss_port {
            if let Ok(mut map) = self.remote_stopped_servers.lock() {
                if let Some(list) = map.get_mut(&host.id) {
                    list.retain(|s| s.port != port);
                }
            }
        }
        if let Some((listener, force)) = requested {
            let name = listener.name.to_ascii_lowercase();
            if listener.port == 22 || listener.port == host.port || name == "ssh"
                || name.split(|c: char| !c.is_ascii_alphanumeric()).any(|part| part == "sshd")
            {
                ui.ctx().data_mut(|d| d.insert_temp(pending_id, (listener, force)));
            } else {
                self.dispatch_remote_listener_stop(host, &listener, force, ui.ctx());
            }
        }
        if let Some((listener, force)) = pending {
            let ctx = ui.ctx();
            let screen = ctx.screen_rect();
            let action = if force { "Kill" } else { "Stop" };
            let mut confirmed = false;
            let mut cancelled = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
            egui::Area::new(pending_id.with("backdrop")).order(egui::Order::Foreground)
                .fixed_pos(screen.min).movable(false).show(ctx, |ui| {
                    let (rect, backdrop) = ui.allocate_exact_size(screen.size(), egui::Sense { click: true, drag: false, focusable: false });
                    ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(120));
                    cancelled |= backdrop.clicked();
                });
            egui::Area::new(pending_id.with("dialog")).order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).movable(false).show(ctx, |ui| {
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(8.0)).inner_margin(16.0).show(ui, |ui| {
                        ui.set_width(340.0);
                        ui.heading(format!("{action} SSH listener?"));
                        ui.label(format!("{} · :{}. This can disconnect your SSH session.", host.name, listener.port));
                        ui.horizontal(|ui| {
                            let cancel = ui.button("Cancel");
                            let focus_id = pending_id.with("focused");
                            if !ctx.data(|d| d.get_temp::<bool>(focus_id)).unwrap_or(false) {
                                cancel.request_focus();
                                ctx.data_mut(|d| d.insert_temp(focus_id, true));
                            }
                            cancelled |= cancel.clicked();
                            confirmed = ui.add(egui::Button::new(RichText::new(action).color(Color32::BLACK)).fill(p.danger)).clicked();
                        });
                    });
                });
            if cancelled || confirmed {
                ctx.data_mut(|d| {
                    d.remove::<(crate::remote::RemoteListener, bool)>(pending_id);
                    d.remove::<bool>(pending_id.with("focused"));
                });
                if confirmed && !cancelled {
                    let target = {
                        let snapshots = self.remote_listeners.lock().unwrap();
                        snapshots.get(&host.id).and_then(|listeners| {
                            let mut matches = listeners.rows.iter().filter(|l| l.port == listener.port && l.proto == listener.proto && l.pid == listener.pid && l.start_marker == listener.start_marker && l.name == listener.name);
                            let target = matches.next()?.clone();
                            matches.next().is_none().then_some(target)
                        })
                    };
                    if let Some(target) = target {
                        self.dispatch_remote_listener_stop(host, &target, force, ctx);
                    } else {
                        self.import_status_msg = Some(("Listener changed. Scan again before retrying.".into(), true, Instant::now()));
                    }
                }
            }
        }

    }

    pub(in crate::app) fn dispatch_remote_listener_stop(&self, host: &SshRemoteHost, l: &crate::remote::RemoteListener, force: bool, ctx: &Context) {
        let (Some(pid), Some(marker)) = (l.pid, l.start_marker) else { return };
        let host_id = host.id.clone();
        let host = host.clone();
        let listener = l.clone();
        let stopped = self.remote_stopped_servers.clone();
        let logs = self.get_remote_logs(&host_id);
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let signal = if force { "KILL" } else { "TERM" };
            match crate::procmgr::send_remote_signal(&host, pid, signal, Some(marker)) {
                Ok(()) => {
                    if !force {
                        if let Ok(mut map) = stopped.lock() {
                            if let Some(list) = map.get_mut(&host_id) {
                                if !list.iter().any(|entry| entry.port == listener.port) {
                                    list.push(RemoteStoppedServer {
                                        name: listener.name.clone(),
                                        port: listener.port,
                                        proto: listener.proto,
                                        last_pid: Some(pid),
                                        cmd: listener.cmd,
                                        start_cmd: guess_remote_start_cmd(&listener.name, listener.port),
                                        cwd: String::new(),
                                    });
                                }
                            }
                        }
                    }
                    crate::service::push_log_line(&format!("[ssh] Sent {signal} to PID {pid}."), &logs, 1000, crate::service::LogKind::Launcher);
                }
                Err(error) => crate::service::push_log_line(&format!("[ssh] Listener action failed: {error}"), &logs, 1000, crate::service::LogKind::Error),
            }
            repaint.data_mut(|data| {
                data.get_temp_mut_or_default::<BTreeSet<String>>(egui::Id::new(REMOTE_LISTENER_REFRESH))
                    .insert(host_id);
            });
            repaint.request_repaint();
        });
    }
}
